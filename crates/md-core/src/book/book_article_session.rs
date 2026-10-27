//! The editing state of the one article open in the book window's detail pane (md.win
//! `Book/BookArticleSession.cs`, Swift `BookArticleSession` in md.macOS BookWorkspace.swift — the
//! part that is a state machine rather than AppKit).
//!
//! What survives the port intact, because each is a promise the tests pin: a selection change
//! saves the outgoing article and a failed save aborts the change (the sidebar reverts on false);
//! a write is refused — `conflicted` — when the file's (mtime, size) stamp no longer matches the
//! one read, and a vanished file is stale too; a clean flush never touches the file; deselecting
//! fully detaches; Close Book flushes and, if that fails, parks the buffer in a "(rescued)"
//! sibling and says so; a save stays in the encoding the file was read in; the flush gate saves
//! synchronously and vetoes on failure; while a document window owns the article the session
//! hands off and reclaims when it closes.
//!
//! What Linux replaces: NSFileCoordinator / NSFilePresenter (md.win: a FileSystemWatcher) become a
//! GIO file monitor in the app feeding [`BookArticleSession::file_changed`],
//! [`BookArticleSession::file_moved`] and [`BookArticleSession::file_deleted`] — the stamp check
//! was always the guard against uncoordinated writers, and it is kept byte for byte;
//! NSDocumentController becomes [`DocumentOwnership`]; sudden-termination control has no
//! analogue, so the app calls [`BookArticleSession::terminate_flush`] from its window-closing /
//! shutdown hook; the per-article UndoManager becomes [`BookArticleSession::undo_generation`],
//! bumped on every load and detach so the editor resets its stack and Ctrl+Z never crosses
//! articles. The security scope collapses to [`BookArticleSession::root`]. Single-threaded by
//! contract, like the Swift `@MainActor` class: call it from the GTK main thread only.
//!
//! Rust shapes (md.win uses events and closures that capture the session):
//! - the debounced autosave is [`AutosaveScheduler`] with no closure: `schedule()` arms a timer
//!   and, when it fires, the app calls [`BookArticleSession::flush_now`] itself;
//! - md.win's static `BookFlushGate.Requested` event is a [`BookFlushHub`] value the app owns one
//!   of, so every test has its own and tests run in parallel; a session joins it through
//!   [`BookFlushHub::subscribe_session`] (it must live in an `Rc<RefCell<…>>` for that) and leaves
//!   it by being dropped;
//! - the `Changed` event is [`BookArticleSession::connect_changed`]. Every callback — `changed`,
//!   [`BookAlerts::present_error`], [`DocumentOwnership`], the scheduler — runs while the session is
//!   mutably borrowed, so a callback must not borrow the session's `RefCell` again; queue the view
//!   refresh (e.g. `glib::idle_add_local_once`) instead.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::Duration;

use super::article_file_system::{ArticleFileSystem, FileStamp, LocalArticleFileSystem};
use super::{book_naming, book_paths};
use crate::document::plain_text_codec::{self, TextEncoding};

/// How long after the last keystroke the autosave fires.
pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

/// What the book window's detail pane should show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BookStageKind {
    /// Nothing selected (or the book is empty).
    Empty,
    /// The article is open here, editable.
    Editing,
    /// The article is open in its own document window — that window owns the file; the pane shows
    /// a handoff notice.
    Handoff,
    /// The article could not be read.
    Unreadable,
}

/// Swift's `BookArticleSession.Stage`: the kind plus the standardized path it refers to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum BookStage {
    #[default]
    Empty,
    Editing(String),
    Handoff(String),
    Unreadable(String),
}

impl BookStage {
    pub fn kind(&self) -> BookStageKind {
        match self {
            BookStage::Empty => BookStageKind::Empty,
            BookStage::Editing(_) => BookStageKind::Editing,
            BookStage::Handoff(_) => BookStageKind::Handoff,
            BookStage::Unreadable(_) => BookStageKind::Unreadable,
        }
    }

    /// The standardized path the stage refers to (`None` only for Empty).
    pub fn path(&self) -> Option<&str> {
        match self {
            BookStage::Empty => None,
            BookStage::Editing(path) | BookStage::Handoff(path) | BookStage::Unreadable(path) => Some(path),
        }
    }
}

/// One posted request to save before the book is read from disk: a book output action posts one
/// through the [`BookFlushHub`]; the session flushes in its handler and flips
/// [`BookFlushGate::vetoed`] when the save failed, so the compile stops rather than shipping a
/// stale page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BookFlushGate {
    pub vetoed: bool,
}

/// Something that answers a flush request — the in-place editor, wherever it lives.
pub trait BookFlushSubscriber {
    fn flush_requested(&self, gate: &mut BookFlushGate);
}

/// The session answers through its `RefCell`. A session that is already borrowed is in the middle
/// of a call on this very thread, so its buffer cannot be saved right now: the gate is vetoed —
/// the compile must never read a file whose newer text is still in memory.
impl BookFlushSubscriber for RefCell<BookArticleSession> {
    fn flush_requested(&self, gate: &mut BookFlushGate) {
        match self.try_borrow_mut() {
            Ok(mut session) => session.on_flush_requested(gate),
            Err(_) => gate.vetoed = true,
        }
    }
}

/// Where flush requests are posted (md.win's static `BookFlushGate.Post` / `FlushEditor` /
/// `Requested`). Synchronous for the same reason Swift used a synchronous notification: the caller
/// reads the files the moment [`BookFlushHub::post`] returns. No book window this launch → no
/// subscriber → nothing unsaved → the gate stays open, which is exactly right. Holds its
/// subscribers weakly: a dropped session is gone from the hub (md.win's `Dispose`).
#[derive(Default)]
pub struct BookFlushHub {
    subscribers: RefCell<Vec<Weak<dyn BookFlushSubscriber>>>,
}

impl BookFlushHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer every request posted here from now on, for as long as `subscriber` lives.
    pub fn subscribe(&self, subscriber: Weak<dyn BookFlushSubscriber>) {
        self.subscribers.borrow_mut().push(subscriber);
    }

    /// [`BookFlushHub::subscribe`] for a session — what md.win's constructor does.
    pub fn subscribe_session(&self, session: &Rc<RefCell<BookArticleSession>>) {
        let weak: Weak<RefCell<BookArticleSession>> = Rc::downgrade(session);
        self.subscribe(weak);
    }

    /// Post a request and return the gate so the caller can read the verdict.
    pub fn post(&self) -> BookFlushGate {
        let mut gate = BookFlushGate::default();
        // Upgrade first and release the list, so a subscriber may subscribe or post re-entrantly.
        let live: Vec<Rc<dyn BookFlushSubscriber>> = {
            let mut subscribers = self.subscribers.borrow_mut();
            subscribers.retain(|subscriber| subscriber.strong_count() > 0);
            subscribers.iter().filter_map(Weak::upgrade).collect()
        };
        for subscriber in live {
            subscriber.flush_requested(&mut gate);
        }
        gate
    }

    /// Swift's `BookOutput.flushEditor()`: true when every editor saved (or none exists).
    pub fn flush_editor(&self) -> bool {
        !self.post().vetoed
    }
}

/// The app's registry of open document windows, keyed by file path — Swift's
/// `NSDocumentController.shared.documents`. The session steps aside while a document owns the
/// article it is editing, so there is never a second writer.
pub trait DocumentOwnership {
    /// Whether a document window currently owns `path` (compare with `book_paths::same`).
    fn owns(&self, path: &str) -> bool;

    /// Bring the owning window to the front.
    fn show_owner(&self, path: &str);
}

/// The modal "no silent failures" door (`BookLibrary.presentError`): the app installs one with the
/// book window as its parent.
pub trait BookAlerts {
    fn present_error(&self, message: &str, informative: &str);
}

/// The debounced autosave: [`AutosaveScheduler::schedule`] arms one call to
/// [`BookArticleSession::flush_now`] [`AUTOSAVE_DELAY`] after the last keystroke, replacing any
/// pending one; [`AutosaveScheduler::cancel`] disarms. The app backs it with a main-loop timer
/// (`glib::timeout_add_local_once`) whose callback calls `flush_now` on the session; tests fire it
/// by hand. Without one the session still saves on every selection change, close and gate —
/// autosave is a convenience, not the guarantee.
pub trait AutosaveScheduler {
    fn schedule(&self);
    fn cancel(&self);
}

/// The in-place article editor's state machine. See the module documentation.
pub struct BookArticleSession {
    file_system: Rc<dyn ArticleFileSystem>,
    ownership: Option<Rc<dyn DocumentOwnership>>,
    alerts: Option<Rc<dyn BookAlerts>>,
    autosave: Option<Rc<dyn AutosaveScheduler>>,
    changed: Vec<Box<dyn FnMut()>>,
    stage: BookStage,
    text: String,
    conflicted: bool,
    save_error_text: Option<String>,
    dirty: bool,
    encoding: TextEncoding,
    disk_stamp: Option<FileStamp>,
    undo_generation: u64,
    root: Option<String>,
}

impl Default for BookArticleSession {
    fn default() -> Self {
        Self::new()
    }
}

impl BookArticleSession {
    /// A session over `std::fs`, with no document registry, no alerts and no autosave.
    pub fn new() -> Self {
        BookArticleSession {
            file_system: Rc::new(LocalArticleFileSystem),
            ownership: None,
            alerts: None,
            autosave: None,
            changed: Vec::new(),
            stage: BookStage::Empty,
            text: String::new(),
            conflicted: false,
            save_error_text: None,
            dirty: false,
            encoding: TextEncoding::Utf8,
            disk_stamp: None,
            undo_generation: 0,
            root: None,
        }
    }

    pub fn with_file_system(mut self, file_system: Rc<dyn ArticleFileSystem>) -> Self {
        self.file_system = file_system;
        self
    }

    pub fn with_ownership(mut self, ownership: Rc<dyn DocumentOwnership>) -> Self {
        self.ownership = Some(ownership);
        self
    }

    pub fn with_alerts(mut self, alerts: Rc<dyn BookAlerts>) -> Self {
        self.alerts = Some(alerts);
        self
    }

    pub fn with_autosave(mut self, autosave: Rc<dyn AutosaveScheduler>) -> Self {
        self.autosave = Some(autosave);
        self
    }

    /// Called after any observable change (stage, text, dirty, conflict, error) — coarse, for view
    /// refresh. Must not borrow the session (see the module documentation).
    pub fn connect_changed(&mut self, handler: impl FnMut() + 'static) {
        self.changed.push(Box::new(handler));
    }

    pub fn stage(&self) -> &BookStage {
        &self.stage
    }

    /// The article's text while editing. Views route edits through [`BookArticleSession::edit`].
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The file changed on disk under unsaved local edits (or vanished). Autosave is suspended; the
    /// footer offers Reload / Keep My Version.
    pub fn conflicted(&self) -> bool {
        self.conflicted
    }

    /// The last save failure, shown until a save succeeds. The session stays dirty so Retry or the
    /// next autosave can heal it.
    pub fn save_error_text(&self) -> Option<&str> {
        self.save_error_text.as_deref()
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// The encoding the file was read in — what a save writes back in, unless the text outgrew it.
    pub fn encoding(&self) -> TextEncoding {
        self.encoding
    }

    /// The same encoding as the code page Foundation and .NET report (65001 / 1200 / 1251 / 28591)
    /// — the shape a status line or a Save As… wants (md.win's `Encoding.CodePage`).
    pub fn code_page(&self) -> u32 {
        match self.encoding {
            TextEncoding::Utf8 => 65001,
            TextEncoding::Utf16 => 1200,
            TextEncoding::WindowsCp1251 => 1251,
            TextEncoding::IsoLatin1 => 28591,
        }
    }

    /// Bumped on every load and detach: the editor discards its undo stack when it changes.
    pub fn undo_generation(&self) -> u64 {
        self.undo_generation
    }

    /// The open book's root (standardized), or `None`. Relative article paths are computed
    /// against it.
    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    /// The path being edited in place, when there is one.
    pub fn editing_path(&self) -> Option<&str> {
        match &self.stage {
            BookStage::Editing(path) => Some(path),
            _ => None,
        }
    }

    /// The article's display name — titles the preview, print job and share actions. Empty when
    /// nothing is selected.
    pub fn title(&self) -> String {
        self.stage.path().map(|path| book_naming::display_name(book_paths::name(path)).to_string()).unwrap_or_default()
    }

    // MARK: Book lifecycle

    /// Open the book at `root`, closing any current one first. Swift's `openBook(unscopedRoot:)` —
    /// Linux has no scope to hold.
    pub fn open_book(&mut self, root: &str) {
        self.close_book();
        self.root = Some(book_paths::standardize(root));
        self.notify();
    }

    /// Final flush, then let go. A failed flush here still detaches — the writer closed the book —
    /// but is reported, never swallowed.
    pub fn close_book(&mut self) {
        self.detach(true);
        self.root = None;
        self.notify();
    }

    // MARK: Selection

    /// Move to `path` (`None` deselects): flush the current article, then load the new one or step
    /// aside if a document window owns it. Returns false — leaving the current article in place —
    /// when the flush fails; the caller reverts its selection so no unsaved text is ever abandoned
    /// silently.
    pub fn select(&mut self, path: Option<&str>) -> bool {
        let target = path.map(book_paths::standardize);
        if let (BookStage::Editing(current), Some(target)) = (&self.stage, &target)
            && current == target
        {
            return true;
        }
        if !self.flush_now() {
            return false;
        }
        self.detach(false);
        let Some(target) = target else {
            self.notify();
            return true;
        };
        if self.owns(&target) {
            self.stage = BookStage::Handoff(target);
        } else {
            self.load(target);
        }
        self.notify();
        true
    }

    /// Detach unconditionally: the flush is attempted, and a failure — the write refused, or an
    /// unresolved conflict — parks the buffer in a rescue copy and says so (when
    /// `report_failure`), instead of discarding keystrokes behind an alert with no way out. Always
    /// completes.
    fn detach(&mut self, report_failure: bool) {
        if !self.flush_now()
            && report_failure
            && self.dirty
            && let BookStage::Editing(path) = self.stage.clone()
        {
            let name = book_naming::display_name(book_paths::name(&path)).to_string();
            let rescued = self.write_rescue_copy(&path);
            if let Some(alerts) = &self.alerts {
                let message = format!("Could not save \u{201C}{name}\u{201D}");
                match rescued {
                    Some(rescued) => alerts.present_error(
                        &message,
                        &format!(
                            "Your text was kept as \u{201C}{}\u{201D} in the same folder.",
                            book_paths::name(&rescued)
                        ),
                    ),
                    None => alerts.present_error(
                        &message,
                        self.save_error_text.as_deref().unwrap_or("The article could not be written."),
                    ),
                }
            }
        }
        self.cancel_autosave();
        self.undo_generation = self.undo_generation.wrapping_add(1);
        self.text.clear();
        self.stage = BookStage::Empty;
        self.conflicted = false;
        self.save_error_text = None;
        self.disk_stamp = None;
        self.dirty = false;
    }

    /// Read `path` into the session. A read or decode failure leaves only the stage changed, to
    /// Unreadable.
    fn load(&mut self, path: String) {
        let Ok(data) = self.file_system.read_all_bytes(&path) else {
            self.stage = BookStage::Unreadable(path);
            return;
        };
        let Some(decoded) = plain_text_codec::decode(&data) else {
            self.stage = BookStage::Unreadable(path);
            return;
        };
        self.text = decoded.text;
        self.encoding = decoded.encoding;
        self.disk_stamp = self.file_system.stamp(&path);
        self.undo_generation = self.undo_generation.wrapping_add(1);
        self.stage = BookStage::Editing(path);
        self.conflicted = false;
        self.save_error_text = None;
        self.dirty = false;
    }

    // MARK: Editing

    /// The one write path for keystrokes: mark dirty and re-arm the debounced autosave. While
    /// conflicted the autosave stays suspended — the writer must first choose Reload or Keep My
    /// Version.
    pub fn edit(&mut self, new_text: &str) {
        if self.stage.kind() != BookStageKind::Editing || self.text == new_text {
            return;
        }
        self.text = new_text.to_string();
        self.dirty = true;
        if !self.conflicted {
            self.schedule_autosave();
        }
        self.notify();
    }

    fn schedule_autosave(&self) {
        if let Some(autosave) = &self.autosave {
            autosave.cancel();
            autosave.schedule();
        }
    }

    fn cancel_autosave(&self) {
        if let Some(autosave) = &self.autosave {
            autosave.cancel();
        }
    }

    // MARK: Saving

    /// Write the buffer to disk now, if there is anything unsaved. Bails out — flagging
    /// `conflicted` — when the file on disk is no longer the one that was read. Returns true when
    /// nothing needed saving or the write landed. This is also what the autosave timer calls.
    pub fn flush_now(&mut self) -> bool {
        let BookStage::Editing(path) = &self.stage else {
            return true;
        };
        if !self.dirty {
            return true;
        }
        let path = path.clone();
        // A document window may have opened this article mid-session. Save our text first — an
        // unedited document re-reads the file — then step aside.
        let owned = self.owns(&path);
        self.cancel_autosave();

        let encoded = plain_text_codec::encode(&self.text, self.encoding);
        if let Some(expected) = self.disk_stamp
            && self.file_system.stamp(&path) != Some(expected)
        {
            self.conflicted = true;
            self.notify();
            return false;
        }
        if let Err(error) = self.file_system.write_all_bytes(&path, &encoded.data) {
            let message = error.to_string();
            self.save_error_text =
                Some(if message.is_empty() { "The article could not be written.".to_string() } else { message });
            self.notify();
            return false;
        }
        self.disk_stamp = self.file_system.stamp(&path);
        self.encoding = encoded.encoding;
        self.save_error_text = None;
        self.dirty = false;
        if owned {
            self.stage = BookStage::Handoff(path);
        }
        self.notify();
        true
    }

    /// The exit-time flush (the app's window-closing / shutdown hook): when the regular write is
    /// refused right as the app exits — conflicted, file vanished, disk full — the keystrokes go to
    /// a rescue copy next to the article rather than nowhere.
    pub fn terminate_flush(&mut self) {
        if !self.flush_now()
            && self.dirty
            && let BookStage::Editing(path) = self.stage.clone()
        {
            self.write_rescue_copy(&path);
        }
    }

    /// Last-resort save: the buffer goes to a fresh "`<name> (rescued).md`" sibling — "(rescued 2)",
    /// "(rescued 3)", … while the name is taken, never "(rescued 1)" and never overwriting anything.
    /// Returns the rescue path, `None` only when even that write failed or a hundred names were
    /// taken. A rescue is a copy, not a save: the buffer stays dirty.
    pub fn write_rescue_copy(&self, path: &str) -> Option<String> {
        let folder = book_paths::directory_name(path).unwrap_or_default();
        let file = book_paths::name(path);
        // Swift: url.deletingPathExtension().lastPathComponent and url.pathExtension (empty →
        // "md"); the two Foundation calls disagree on odd names, and so do these.
        let stem = book_paths::deleting_path_extension(file);
        let ext = match book_paths::path_extension(file) {
            "" => "md",
            ext => ext,
        };
        let encoded = plain_text_codec::encode(&self.text, self.encoding);
        for attempt in 1..=100 {
            let name = if attempt == 1 { format!("{stem} (rescued)") } else { format!("{stem} (rescued {attempt})") };
            let candidate = book_paths::combine(&folder, &format!("{name}.{ext}"));
            if self.file_system.file_exists(&candidate) {
                continue;
            }
            return self.file_system.write_all_bytes(&candidate, &encoded.data).ok().map(|()| candidate);
        }
        None
    }

    // MARK: Conflicts

    /// "Keep My Version": write the buffer out unconditionally — no stamp check; the point is to
    /// overwrite whatever is on disk (or recreate a deleted file).
    pub fn resolve_conflict_keeping_mine(&mut self) {
        let BookStage::Editing(path) = self.stage.clone() else {
            return;
        };
        self.conflicted = false;
        let encoded = plain_text_codec::encode(&self.text, self.encoding);
        match self.file_system.write_all_bytes(&path, &encoded.data) {
            Ok(()) => {
                self.encoding = encoded.encoding;
                self.disk_stamp = self.file_system.stamp(&path);
                self.save_error_text = None;
                self.dirty = false;
            }
            Err(error) => self.save_error_text = Some(error.to_string()),
        }
        self.notify();
    }

    /// "Reload from Disk": discard the buffer. Clearing dirty *first* is what makes the detach's
    /// flush a no-op — writing the buffer is exactly what must not happen here.
    pub fn resolve_conflict_reloading(&mut self) {
        let BookStage::Editing(path) = self.stage.clone() else {
            return;
        };
        self.dirty = false;
        self.detach(false);
        self.load(path);
        self.notify();
    }

    // MARK: Ownership (document windows)

    /// Re-evaluate who owns the selected article (the app calls this when any window is
    /// activated). Editing + a document appeared → save and hand off; handoff + the document closed
    /// → reclaim. A failed flush stays put: the buffer must remain visible with its error, not
    /// vanish behind the handoff pane.
    pub fn recheck_ownership(&mut self) {
        match self.stage.clone() {
            BookStage::Editing(path) => {
                if self.owns(&path) && self.flush_now() && self.stage.kind() == BookStageKind::Editing {
                    self.stage = BookStage::Handoff(path);
                }
            }
            BookStage::Handoff(path) => {
                if !self.owns(&path) {
                    self.detach(false);
                    self.load(path);
                }
            }
            BookStage::Empty | BookStage::Unreadable(_) => {}
        }
        self.notify();
    }

    /// The handoff pane's button: bring the owning document window to the front.
    pub fn show_owning_window(&self) {
        if let (BookStage::Handoff(path), Some(ownership)) = (&self.stage, &self.ownership) {
            ownership.show_owner(path);
        }
    }

    /// The workspace is about to open the current article in a separate window: save, then step
    /// into handoff *before* the document opens so there is never a moment with two writers. False
    /// when the save failed (the open is abandoned).
    pub fn hand_off_for_external_open(&mut self) -> bool {
        let BookStage::Editing(path) = self.stage.clone() else {
            return true;
        };
        if !self.flush_now() {
            return false;
        }
        self.detach(false);
        self.stage = BookStage::Handoff(path);
        self.notify();
        true
    }

    // MARK: File monitor events (the app delivers these on the main thread)

    /// Something wrote the file. Our own writes fire this too, as do attribute-only changes; only
    /// a moved stamp is a real change — a conflict banner over identical bytes would invite a
    /// pointless, text-discarding Reload. Clean session: follow the disk silently. Dirty: keep both
    /// versions (theirs on disk, ours in memory), suspend autosave, let the writer choose.
    pub fn file_changed(&mut self) {
        let BookStage::Editing(path) = self.stage.clone() else {
            return;
        };
        if let (Some(current), Some(expected)) = (self.file_system.stamp(&path), self.disk_stamp)
            && current == expected
        {
            return;
        }
        if self.dirty {
            self.conflicted = true;
            self.cancel_autosave();
        } else if let Ok(data) = self.file_system.read_all_bytes(&path)
            && let Some(fresh) = plain_text_codec::decode(&data)
        {
            self.text = fresh.text;
            self.encoding = fresh.encoding;
            self.disk_stamp = self.file_system.stamp(&path);
        }
        // An unreadable file right now keeps what we have; the next event or save decides.
        self.notify();
    }

    /// The file was renamed or moved under us: follow it.
    pub fn file_moved(&mut self, new_path: &str) {
        if self.stage.kind() != BookStageKind::Editing {
            return;
        }
        self.stage = BookStage::Editing(book_paths::standardize(new_path));
        self.notify();
    }

    /// The file is gone. A dirty buffer stays on screen as a conflict ("Keep My Version" recreates
    /// the file); a clean one just detaches.
    pub fn file_deleted(&mut self) {
        if self.stage.kind() != BookStageKind::Editing {
            return;
        }
        if self.dirty {
            self.conflicted = true;
            self.cancel_autosave();
        } else {
            self.detach(false);
        }
        self.notify();
    }

    // MARK: Plumbing

    /// The flush gate's handler: save now, and veto the request when the save failed.
    pub fn on_flush_requested(&mut self, gate: &mut BookFlushGate) {
        if !self.flush_now() {
            gate.vetoed = true;
        }
    }

    fn owns(&self, path: &str) -> bool {
        self.ownership.as_ref().is_some_and(|ownership| ownership.owns(path))
    }

    fn notify(&mut self) {
        for handler in &mut self.changed {
            handler();
        }
    }
}
