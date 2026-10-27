//! Port of md.win `Documents/TextFileSession.cs` — one state machine for documents and book
//! articles (§6.3).
//!
//! # Driving it from GTK
//!
//! - **Create** one per document window (`SessionRole::Document`, `Some(window_id)`) and one for
//!   the Book window's detail pane (`SessionRole::BookArticle`, `None`), each with its own
//!   `FileWatcher` (a `GFileMonitor` adapter), and the app-wide registry and identity.
//! - **Subscribe**: [`TextFileSession::changed`] → refresh the title
//!   (`window_title::for_document(title, is_dirty)`), the conflict bar (`conflicted`), the
//!   save-error bar (`save_error_text`) and the footer; [`TextFileSession::text_replaced_externally`]
//!   → put the text in the buffer and restore the selection with
//!   [`editor_text::clamp_selection`](super::editor_text::clamp_selection);
//!   [`TextFileSession::identity_changed`] → the view-mode memory; [`TextFileSession::alert_requested`]
//!   → an `adw::AlertDialog`.
//! - **Text in**: the buffer's `changed` → `edit(&editor_text::from_editor(&buffer_text))`. Keep the
//!   buffer itself LF (normalise CRs in `insert-text`, so buffer offsets are model offsets), and
//!   block the `changed` handler whenever the app assigns the buffer: `set_text` deletes before it
//!   inserts, and the empty intermediate would otherwise reach `edit` as the writer's text.
//! - **Text out**: after `open`, `open_untitled`, `import_bundle` and `select`, assign `text()` to the
//!   buffer (no event says so for the first three); whenever `undo_generation` changed, assign it
//!   inside `begin_irreversible_action`/`end_irreversible_action` so Ctrl+Z never crosses files.
//! - **Offsets** are UTF-16 units; convert `GtkTextIter` character offsets with md-core's
//!   `text::utf16` helpers.
//! - **Line endings** are the session's business: the model is LF, and every write re-applies the
//!   file's own newline, encoding and BOM ([`TextFileDressing`]).
//! - **Flush**: Ctrl+S → `save_as` through the picker when untitled, else `flush_now(true)`; window
//!   deactivated, and before every print or export → `flush_now(false)`; close → untitled and
//!   `is_dirty` asks first, then `close_flush()` and `dispose()`; `GApplication::shutdown` →
//!   `terminate_flush()` on every session. Rename… / Move To… → move the file, then `retarget`.
//! - **Ownership**: the book pane's `recheck_ownership()` on every window activation, and
//!   `hand_off_for_external_open()` (false → abandon) before opening its article in its own window.
//! - **Watcher**: deliver on the main context. `GFileMonitor` (`WATCH_MOVES`) `Changed`,
//!   `ChangesDoneHint`, `Created`, `MovedIn` → `changed`; `Deleted` → `deleted`; `Renamed` /
//!   `MovedOut` of our file → `renamed(ours, other)` (`MovedOut` with no other file → `deleted`); a
//!   `Renamed` whose *other* file is ours (another editor's atomic save) → `changed` — the session
//!   treats a rename onto its own path as a change anyway.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use md_core::book::book_naming;

use super::document_loader::{self, DocumentKind};
use super::file_names;
use super::line_endings;
use super::rescue_copy::{self, failure_message};
use super::text_file_dressing::TextFileDressing;
use crate::event::{Event, HandlerId};
use crate::seams::{
    DocumentRegistry, FileIdentity, FileStamp, FileSystem, FileWatcher, Scheduler, TimerHandle, WindowId,
};
use crate::strings;
use md_core::document::plain_text_codec::TextEncoding;

/// What the surface hosting a session should show. Swift's `BookArticleSession.Stage` plus the two
/// states only a document window has: [`Stage::Untitled`] (a new document, an example, a
/// duplicate, an imported bundle) and [`Stage::Empty`] (a book pane with nothing selected).
/// Closed: these five are every state a session can be in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// No file yet: Save routes to Save As, closing while dirty asks.
    Untitled,
    /// Editing the file at this path in place.
    Editing(String),
    /// A document window owns the file; this session has stepped aside so there is never a second
    /// writer.
    Handoff(String),
    /// The file could not be read (moved or deleted outside the book).
    Unreadable(String),
    /// Nothing selected — the book pane's placeholder.
    Empty,
}

impl Stage {
    /// The file this stage refers to, or `None` for [`Stage::Untitled`] and [`Stage::Empty`].
    pub fn file_path(&self) -> Option<&str> {
        match self {
            Stage::Editing(path) | Stage::Handoff(path) | Stage::Unreadable(path) => Some(path),
            Stage::Untitled | Stage::Empty => None,
        }
    }
}

/// Which surface the session serves. The two differ in three rows only; everything else is one
/// state machine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SessionRole {
    /// A document window: "— Edited" survives an autosave, and a deleted file leaves the buffer and
    /// the path in place.
    #[default]
    Document,
    /// The book window's detail pane: every successful save is the save, and a deleted article
    /// detaches.
    BookArticle,
}

struct State {
    stage: Stage,
    text: String,
    title: String,
    dressing: TextFileDressing,
    is_dirty: bool,
    unsaved: bool,
    conflicted: bool,
    save_error_text: Option<String>,
    imported_from: Option<String>,
    last_saved_text: String,
    disk_stamp: Option<FileStamp>,
    undo_generation: u64,
    canonical_path: Option<String>,
}

/// One state machine for documents and book articles (§6.3) — the port of macOS's
/// `BookArticleSession` widened to carry what NSDocument gave the document window for free.
///
/// The promises, each pinned by a test: a write is refused — [`TextFileSession::conflicted`] — when
/// the file's (mtime, size) stamp no longer matches the one read, and a vanished file is stale too;
/// a clean flush never touches the file; a save stays in the encoding, BOM and line endings the
/// file was read with; a selection change saves the outgoing article and a failed save aborts the
/// change; closing with an unsavable buffer parks it in a "(rescued)" sibling and says so; while a
/// document window owns the article the session hands off and reclaims when that window closes.
///
/// Two kinds of "dirty", because the Mac has two. [`TextFileSession::is_dirty`] is the window
/// title's "— Edited": it means the text differs from the last *explicit* save, and an autosave
/// leaves it alone, exactly as NSDocument's autosave-in-place does.
/// [`TextFileSession::has_unsaved_changes`] means the bytes on disk are behind the buffer; it is
/// what arms the autosave and what a rescue copy is for. A book article has only one notion, so for
/// that role every successful save clears both — which is what the fourteen macOS session tests
/// pin.
///
/// Single-threaded by contract, like the Swift `@MainActor` class: the watcher delivers its events
/// on the UI thread (a `GFileMonitor` emits on the main context it was created on), and the
/// scheduler's timers are main-loop timers, so nothing in here locks. Every method takes `&self`;
/// no internal borrow is held while an event is raised or a seam is called, so a handler may call
/// back into the session.
///
/// Rust shape: C#'s events are [`Event`]s exposed by accessor; `IDisposable` is
/// [`TextFileSession::dispose`] (also run on drop). The session is created in an `Rc` because the
/// watcher's handlers and the autosave timer hold weak references back to it.
pub struct TextFileSession {
    fs: Rc<dyn FileSystem>,
    watcher: Rc<dyn FileWatcher>,
    scheduler: Rc<dyn Scheduler>,
    registry: Rc<dyn DocumentRegistry>,
    identity: Rc<dyn FileIdentity>,
    role: SessionRole,
    window_id: Option<WindowId>,
    me: Weak<TextFileSession>,
    state: RefCell<State>,
    autosave: RefCell<Option<TimerHandle>>,
    subscriptions: Cell<Option<[HandlerId; 3]>>,
    disposed: Cell<bool>,
    changed: Event<()>,
    text_replaced_externally: Event<str>,
    identity_changed: Event<Option<String>>,
    alert_requested: Event<(String, String)>,
}

impl fmt::Debug for TextFileSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("TextFileSession")
            .field("role", &self.role)
            .field("window_id", &self.window_id)
            .field("stage", &state.stage)
            .field("is_dirty", &state.is_dirty)
            .field("unsaved", &state.unsaved)
            .field("conflicted", &state.conflicted)
            .finish()
    }
}

impl TextFileSession {
    /// How long after the last keystroke the autosave fires (§6.3).
    pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

    /// A session in [`Stage::Untitled`], subscribed to `watcher`'s events.
    ///
    /// `window_id`: the window that owns the file, when there is one. A document window passes its
    /// id and the session keeps the registry's path → window entry up to date; the book pane passes
    /// `None` and only asks the registry who owns what.
    pub fn new(
        fs: Rc<dyn FileSystem>,
        watcher: Rc<dyn FileWatcher>,
        scheduler: Rc<dyn Scheduler>,
        registry: Rc<dyn DocumentRegistry>,
        identity: Rc<dyn FileIdentity>,
        role: SessionRole,
        window_id: Option<WindowId>,
    ) -> Rc<TextFileSession> {
        Rc::new_cyclic(|me: &Weak<TextFileSession>| {
            let on_changed = {
                let me = me.clone();
                watcher.changed().connect(move |path: &str| {
                    if let Some(session) = me.upgrade() {
                        session.on_file_changed(path);
                    }
                })
            };
            let on_renamed = {
                let me = me.clone();
                watcher.renamed().connect(move |(old_path, new_path): &(String, String)| {
                    if let Some(session) = me.upgrade() {
                        session.on_file_renamed(old_path, new_path);
                    }
                })
            };
            let on_deleted = {
                let me = me.clone();
                watcher.deleted().connect(move |path: &str| {
                    if let Some(session) = me.upgrade() {
                        session.on_file_deleted(path);
                    }
                })
            };
            TextFileSession {
                fs,
                watcher,
                scheduler,
                registry,
                identity,
                role,
                window_id,
                me: me.clone(),
                state: RefCell::new(State {
                    stage: Stage::Untitled,
                    text: String::new(),
                    title: strings::UNTITLED.to_string(),
                    dressing: TextFileDressing::DEFAULT,
                    is_dirty: false,
                    unsaved: false,
                    conflicted: false,
                    save_error_text: None,
                    imported_from: None,
                    last_saved_text: String::new(),
                    disk_stamp: None,
                    undo_generation: 0,
                    canonical_path: None,
                }),
                autosave: RefCell::new(None),
                subscriptions: Cell::new(Some([on_changed, on_renamed, on_deleted])),
                disposed: Cell::new(false),
                changed: Event::new(),
                text_replaced_externally: Event::new(),
                identity_changed: Event::new(),
                alert_requested: Event::new(),
            }
        })
    }

    // ---- state ----

    pub fn role(&self) -> SessionRole {
        self.role
    }

    /// The window whose registry entry this session maintains, or `None` for the book pane.
    pub fn window_id(&self) -> Option<WindowId> {
        self.window_id
    }

    pub fn stage(&self) -> Stage {
        self.state.borrow().stage.clone()
    }

    /// The buffer, always LF (§3.2). Views route edits through [`TextFileSession::edit`].
    pub fn text(&self) -> String {
        self.state.borrow().text.clone()
    }

    /// The display name: a file's stem (a book article's without its ordering prefix), a bundle's
    /// stem, or the untitled name the window was given.
    pub fn title(&self) -> String {
        self.state.borrow().title.clone()
    }

    /// Encoding, BOM and line endings to write back with (§6.3).
    pub fn dressing(&self) -> TextFileDressing {
        self.state.borrow().dressing
    }

    /// The window title's "— Edited": differs from the last explicit save or open. An autosave
    /// never clears it (§6.7).
    pub fn is_dirty(&self) -> bool {
        self.state.borrow().is_dirty
    }

    /// The bytes on disk are behind the buffer — what arms the autosave and what a rescue copy
    /// would park.
    pub fn has_unsaved_changes(&self) -> bool {
        self.state.borrow().unsaved
    }

    /// The file changed on disk under unsaved edits (or vanished). Autosave is suspended; the bar
    /// offers Reload from Disk / Keep My Version.
    pub fn conflicted(&self) -> bool {
        self.state.borrow().conflicted
    }

    /// The last save failure, shown until a save succeeds. The session stays unsaved so Retry or
    /// the next autosave can heal it.
    pub fn save_error_text(&self) -> Option<String> {
        self.state.borrow().save_error_text.clone()
    }

    /// The `.textpack` or `.textbundle` this untitled document was imported from (§6.5); display
    /// only, never written back.
    pub fn imported_from(&self) -> Option<String> {
        self.state.borrow().imported_from.clone()
    }

    /// What Revert to Saved goes back to: the text of the last explicit save or open.
    pub fn last_saved_text(&self) -> String {
        self.state.borrow().last_saved_text.clone()
    }

    /// The (mtime, size) of the file as of the last read or write; `None` when there is no file.
    pub fn disk_stamp(&self) -> Option<FileStamp> {
        self.state.borrow().disk_stamp
    }

    /// Bumped on every load and detach: the editor clears its undo stack so Ctrl+Z never crosses
    /// files.
    pub fn undo_generation(&self) -> u64 {
        self.state.borrow().undo_generation
    }

    /// The canonical spelling of the file, the registry's key; `None` when there is no file.
    pub fn canonical_path(&self) -> Option<String> {
        self.state.borrow().canonical_path.clone()
    }

    /// The file being edited in place, when there is one.
    pub fn editing_path(&self) -> Option<String> {
        match &self.state.borrow().stage {
            Stage::Editing(path) => Some(path.clone()),
            _ => None,
        }
    }

    /// Whether the document has a file at all (Save writes it; Save As, Rename, Move To and Revert
    /// need it).
    pub fn has_file_identity(&self) -> bool {
        self.state.borrow().stage.file_path().is_some()
    }

    // ---- events ----

    /// Any observable change (stage, text, dirty, conflict, error) — coarse, for a view refresh.
    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }

    /// The buffer was replaced from outside the editor (revert, silent reload, conflict
    /// resolution, a book article loaded): assign it and restore the clamped selection (§3.2).
    pub fn text_replaced_externally(&self) -> &Event<str> {
        &self.text_replaced_externally
    }

    /// The file the session edits changed (open, Save As, rename, detach) — `None` when it now has
    /// none. The view-mode memory re-decides on this (§5.2).
    pub fn identity_changed(&self) -> &Event<Option<String>> {
        &self.identity_changed
    }

    /// A modal the session cannot show itself: (title, message). Raised for a failed open and for
    /// the rescue-copy report (§6.3).
    pub fn alert_requested(&self) -> &Event<(String, String)> {
        &self.alert_requested
    }

    // ---- opening ----

    /// A document with no file: New, an example (dirty, so closing offers to save), Duplicate.
    /// `title` defaults to "Untitled".
    pub fn open_untitled(&self, text: &str, title: Option<&str>, dirty: bool) {
        self.cancel_autosave();
        self.watcher.stop();
        self.with_state(|s| {
            s.text = line_endings::normalize(text);
            s.dressing = TextFileDressing::DEFAULT;
            s.title = title.unwrap_or(strings::UNTITLED).to_string();
            s.imported_from = None;
            s.stage = Stage::Untitled;
            s.disk_stamp = None;
            s.canonical_path = None;
            s.conflicted = false;
            s.save_error_text = None;
            s.last_saved_text = s.text.clone();
            s.is_dirty = dirty;
            s.unsaved = dirty;
            s.undo_generation += 1;
        });
        self.identity_changed.emit(&None);
        self.notify();
    }

    /// Open a path: a plain file is edited in place; a `.textpack` or a `.textbundle` folder is
    /// imported read-only (§6.5). False means nothing was opened and
    /// [`TextFileSession::alert_requested`] has just carried the reason.
    ///
    /// # Panics
    ///
    /// For an empty path (md.win throws `ArgumentException`).
    pub fn open(&self, path: &str) -> bool {
        assert!(!path.is_empty(), "TextFileSession::open: empty path");
        let load = document_loader::load(path, &*self.fs);
        let title = load.alert_title().unwrap_or(strings::documents::COULD_NOT_OPEN);
        let Some(document) = load.document else {
            self.alert_requested.emit(&(title.to_string(), load.error_text));
            return false;
        };
        // The loader's dressing, not just its encoding: the loaded text is already LF, so
        // re-detecting the newline here would read LF off every bundle and quietly turn a CRLF
        // writer's first Save As into an LF file. `import_bundle`'s own detection is for callers
        // who still hold the raw text.
        if document.kind != DocumentKind::PlainText {
            return self.adopt_import(&document.path, &document.text, document.dressing);
        }

        self.cancel_autosave();
        let stamp = self.fs.stamp(path);
        let title = self.display_name_of(path);
        self.with_state(|s| {
            s.text = document.text;
            s.dressing = document.dressing;
            s.title = title;
            s.imported_from = None;
            s.stage = Stage::Editing(path.to_string());
            s.disk_stamp = stamp;
            s.conflicted = false;
            s.save_error_text = None;
            s.last_saved_text = s.text.clone();
            s.is_dirty = false;
            s.unsaved = false;
            s.undo_generation += 1;
        });
        self.watcher.watch(path);
        self.register(path);
        self.notify();
        true
    }

    /// Adopt an already-decoded bundle as an untitled document titled after the bundle (§6.5).
    /// Bundles are never written back — their `assets/` would be lost — so Save asks for a new
    /// `.md` location and the window owns no file.
    ///
    /// # Panics
    ///
    /// For an empty bundle path.
    pub fn import_bundle(&self, bundle_path: &str, text: &str, encoding: TextEncoding) -> bool {
        assert!(!bundle_path.is_empty(), "TextFileSession::import_bundle: empty path");
        // Detected here, before the normalise in adopt_import: this call takes the bundle's text
        // as it was decoded, so its own convention is still visible in the argument.
        self.adopt_import(bundle_path, text, TextFileDressing::new(encoding, false, line_endings::detect(text)))
    }

    fn adopt_import(&self, bundle_path: &str, text: &str, dressing: TextFileDressing) -> bool {
        self.cancel_autosave();
        self.watcher.stop();
        self.with_state(|s| {
            s.text = line_endings::normalize(text);
            s.dressing = dressing;
            s.title = file_names::stem_of(bundle_path).to_string();
            s.imported_from = Some(bundle_path.to_string());
            s.stage = Stage::Untitled;
            s.disk_stamp = None;
            s.canonical_path = None;
            s.conflicted = false;
            s.save_error_text = None;
            s.last_saved_text = s.text.clone();
            s.is_dirty = false;
            s.unsaved = false;
            s.undo_generation += 1;
        });
        self.identity_changed.emit(&None);
        self.notify();
        true
    }

    /// The book pane's selection change (`None` deselects): flush the current article, then load
    /// the new one or step aside if a document window owns it. False — the current article left in
    /// place — when the flush failed; the sidebar reverts its selection so no unsaved text is ever
    /// abandoned silently.
    pub fn select(&self, path: Option<&str>) -> bool {
        let target = path.map(file_names::standardize);
        if let (Stage::Editing(current), Some(target)) = (self.stage(), &target)
            && file_names::same_path(&current, target)
        {
            return true;
        }
        if !self.flush_now(false) {
            return false;
        }
        self.detach(false);
        let Some(target) = target else {
            self.notify();
            return true;
        };
        if self.owned_by_another(&target) {
            self.set_handoff(&target);
        } else {
            self.load(&target);
        }
        self.notify();
        true
    }

    // ---- editing ----

    /// The one write path for keystrokes. While conflicted the autosave stays suspended — the
    /// writer must first choose Reload from Disk or Keep My Version.
    pub fn edit(&self, text: &str) {
        let schedule = {
            let mut s = self.state.borrow_mut();
            if !matches!(s.stage, Stage::Editing(_) | Stage::Untitled) || s.text == text {
                return;
            }
            s.text = text.to_string();
            s.is_dirty = true;
            s.unsaved = true;
            matches!(s.stage, Stage::Editing(_)) && !s.conflicted
        };
        if schedule {
            self.schedule_autosave();
        }
        self.notify();
    }

    // ---- saving ----

    /// Write the buffer now. Refuses — flagging [`TextFileSession::conflicted`] — when the file on
    /// disk is no longer the one that was read. True when the write landed or there was nothing to
    /// write.
    ///
    /// `explicit_save`: Ctrl+S, Revert, Keep My Version — the ones that clear "— Edited". An
    /// autosave passes false and leaves the title's Edited marker alone, as NSDocument's
    /// autosave-in-place does (§6.7). An explicit save with nothing to write still clears the
    /// marker — otherwise a document that has only ever been autosaved could never lose it.
    pub fn flush_now(&self, explicit_save: bool) -> bool {
        let Stage::Editing(path) = self.stage() else { return true };
        if !self.state.borrow().unsaved {
            if explicit_save {
                self.mark_explicitly_saved();
                self.notify();
            }
            return true;
        }

        // A document window may have opened this article mid-session. Save our text first — an
        // unedited document window re-reads the file — then step aside.
        let owned = self.owned_by_another(&path);
        self.cancel_autosave();

        let (bytes, used, expected) = {
            let s = self.state.borrow();
            let (bytes, used) = s.dressing.dress(&s.text);
            (bytes, used, s.disk_stamp)
        };
        if let Some(expected) = expected
            && self.fs.stamp(&path) != Some(expected)
        {
            self.with_state(|s| s.conflicted = true);
            self.notify();
            return false;
        }
        if let Err(e) = self.fs.write_all_bytes_in_place(&path, &bytes) {
            let message = failure_message(&e, self.write_failure_fallback());
            self.with_state(|s| s.save_error_text = Some(message));
            self.notify();
            return false;
        }
        let stamp = self.fs.stamp(&path);
        self.with_state(|s| {
            s.disk_stamp = stamp;
            s.dressing = s.dressing.with_encoding(used);
            s.save_error_text = None;
            s.unsaved = false;
        });
        if explicit_save || self.role == SessionRole::BookArticle {
            self.mark_explicitly_saved();
        }
        if owned {
            self.set_handoff(&path);
        }
        self.notify();
        true
    }

    /// Write to a new path and edit that from now on; the old file stays where it was (§6.3). The
    /// encoding and line endings travel with the text. False leaves the session on the old file
    /// with [`TextFileSession::save_error_text`] set.
    ///
    /// LINUX NOTE: GTK's save dialog does not create the file (WinRT's does), so there is no empty
    /// file for the caller to delete after a failure.
    ///
    /// # Panics
    ///
    /// For an empty path.
    pub fn save_as(&self, path: &str) -> bool {
        assert!(!path.is_empty(), "TextFileSession::save_as: empty path");
        self.cancel_autosave();
        let (bytes, used) = {
            let s = self.state.borrow();
            s.dressing.dress(&s.text)
        };
        if let Err(e) = self.fs.write_all_bytes_in_place(path, &bytes) {
            let message = failure_message(&e, self.write_failure_fallback());
            self.with_state(|s| s.save_error_text = Some(message));
            self.notify();
            return false;
        }
        let stamp = self.fs.stamp(path);
        let title = self.display_name_of(path);
        self.with_state(|s| {
            s.dressing = s.dressing.with_encoding(used);
            s.stage = Stage::Editing(path.to_string());
            s.title = title;
            s.imported_from = None;
            s.disk_stamp = stamp;
            s.conflicted = false;
            s.save_error_text = None;
            s.unsaved = false;
        });
        self.mark_explicitly_saved();
        self.watcher.watch(path);
        self.register(path);
        self.notify();
        true
    }

    /// The file moved or was renamed — by us (Rename…, Move To…) or under us (the watcher). The
    /// session follows it: new stage, new title, new watch, new registry entry, and
    /// [`TextFileSession::identity_changed`] so the view-mode memory re-decides (§5.2).
    ///
    /// # Panics
    ///
    /// For an empty path.
    pub fn retarget(&self, new_path: &str) {
        assert!(!new_path.is_empty(), "TextFileSession::retarget: empty path");
        if !matches!(self.state.borrow().stage, Stage::Editing(_)) {
            return;
        }
        let stamp = self.fs.stamp(new_path);
        let title = self.display_name_of(new_path);
        self.with_state(|s| {
            s.stage = Stage::Editing(new_path.to_string());
            s.title = title;
            s.disk_stamp = stamp;
        });
        self.watcher.watch(new_path);
        self.register(new_path);
        self.notify();
    }

    /// Revert to Saved: the buffer goes back to the last explicit save and is written out (§6.3).
    pub fn revert_to_saved(&self) {
        if !matches!(self.state.borrow().stage, Stage::Editing(_)) {
            return;
        }
        self.with_state(|s| {
            if s.text != s.last_saved_text {
                s.text = s.last_saved_text.clone();
                s.unsaved = true;
                s.undo_generation += 1;
            }
        });
        self.flush_now(true);
        let text = self.text();
        self.text_replaced_externally.emit(&text);
        self.notify();
    }

    /// Let go of the file. The flush is attempted, and a failure — the write refused, or an
    /// unresolved conflict — parks the buffer in a rescue copy and says so (when `report_failure`)
    /// instead of discarding keystrokes behind an alert with no way out. Always completes.
    pub fn detach(&self, report_failure: bool) {
        if !self.flush_now(false) && report_failure {
            let pending = {
                let s = self.state.borrow();
                match &s.stage {
                    Stage::Editing(path) if s.unsaved => Some(path.clone()),
                    _ => None,
                }
            };
            if let Some(path) = pending {
                let name = self.display_name_of(&path);
                let message = match self.write_rescue_copy() {
                    Some(rescued) => strings::documents::rescue_kept_message(file_names::name_of(&rescued)),
                    None => self.save_error_text().unwrap_or_else(|| self.write_failure_fallback().to_string()),
                };
                self.alert_requested.emit(&(strings::documents::rescue_failed_title(&name), message));
            }
        }
        self.cancel_autosave();
        self.watcher.stop();
        self.unregister();
        self.with_state(|s| {
            s.undo_generation += 1;
            s.text = String::new();
            s.stage = Stage::Empty;
            s.title = String::new();
            s.dressing = TextFileDressing::DEFAULT;
            s.imported_from = None;
            s.conflicted = false;
            s.save_error_text = None;
            s.disk_stamp = None;
            s.last_saved_text = String::new();
            s.is_dirty = false;
            s.unsaved = false;
        });
        self.identity_changed.emit(&None);
        self.notify();
    }

    /// The window's close path (§6.3): flush, rescue on failure, report, let go. Never blocks the
    /// close.
    pub fn close_flush(&self) {
        self.detach(true);
    }

    /// The exit-time flush: when the regular write is refused right as the app exits — conflicted,
    /// file vanished, disk full — the keystrokes go to a rescue copy rather than nowhere. No modal.
    pub fn terminate_flush(&self) {
        if !self.flush_now(false) {
            let pending = {
                let s = self.state.borrow();
                matches!(s.stage, Stage::Editing(_)) && s.unsaved
            };
            if pending {
                self.write_rescue_copy();
            }
        }
    }

    /// Last-resort save into a fresh `"{name} (rescued).{ext}"` sibling. `None` when there is no
    /// file, the write failed, or a hundred names were taken. A rescue is a copy, not a save: the
    /// buffer stays dirty.
    pub fn write_rescue_copy(&self) -> Option<String> {
        let (path, bytes) = {
            let s = self.state.borrow();
            let Stage::Editing(path) = &s.stage else { return None };
            (path.clone(), s.dressing.dress(&s.text).0)
        };
        rescue_copy::write(&*self.fs, &path, &bytes)
    }

    // ---- conflicts ----

    /// "Keep My Version": write the buffer out unconditionally — no stamp check; the point is to
    /// overwrite whatever is on disk, or recreate a deleted file.
    pub fn resolve_conflict_keeping_mine(&self) {
        let Stage::Editing(path) = self.stage() else { return };
        let (bytes, used) = {
            let mut s = self.state.borrow_mut();
            s.conflicted = false;
            s.dressing.dress(&s.text)
        };
        match self.fs.write_all_bytes_in_place(&path, &bytes) {
            Ok(()) => {
                let stamp = self.fs.stamp(&path);
                self.with_state(|s| {
                    s.dressing = s.dressing.with_encoding(used);
                    s.disk_stamp = stamp;
                    s.save_error_text = None;
                    s.unsaved = false;
                });
                self.mark_explicitly_saved();
            }
            Err(e) => {
                let message = failure_message(&e, self.write_failure_fallback());
                self.with_state(|s| s.save_error_text = Some(message));
            }
        }
        self.notify();
    }

    /// "Reload from Disk": discard the buffer. Clearing the unsaved flag *first* is what makes the
    /// book's detach a no-op — writing the buffer is exactly what must not happen here. A document
    /// window keeps its window: it re-reads in place, and if the file is gone it keeps the text
    /// and lets the next save recreate the file.
    pub fn resolve_conflict_reloading(&self) {
        let Stage::Editing(path) = self.stage() else { return };
        self.with_state(|s| {
            s.unsaved = false;
            s.is_dirty = false;
        });
        if self.role == SessionRole::BookArticle {
            self.detach(false);
            self.load(&path);
        } else if !self.reload(&path, true) {
            self.with_state(|s| {
                s.conflicted = false;
                s.disk_stamp = None;
                s.unsaved = true;
                s.is_dirty = true;
            });
        }
        self.notify();
    }

    // ---- ownership ----

    /// Re-evaluate who owns the file (the app calls this when any window becomes active). Editing +
    /// a document window appeared → save and hand off; handoff + that window closed → reclaim. A
    /// failed flush stays put: the buffer must remain visible with its error, not vanish behind the
    /// handoff pane.
    pub fn recheck_ownership(&self) {
        match self.stage() {
            // The guard saves (flush_now) before it decides; a failed save keeps the stage.
            Stage::Editing(path)
                if self.owned_by_another(&path)
                    && self.flush_now(false)
                    && matches!(self.state.borrow().stage, Stage::Editing(_)) =>
            {
                self.set_handoff(&path);
            }
            Stage::Handoff(path) if !self.owned_by_another(&path) => {
                self.detach(false);
                self.load(&path);
            }
            _ => {}
        }
        self.notify();
    }

    /// The pane is about to open its article in a separate window: save, then step into handoff
    /// *before* the document opens so there is never a moment with two writers. False when the
    /// save failed and the open must be abandoned.
    pub fn hand_off_for_external_open(&self) -> bool {
        let Stage::Editing(path) = self.stage() else { return true };
        if !self.flush_now(false) {
            return false;
        }
        self.detach(false);
        self.set_handoff(&path);
        self.notify();
        true
    }

    /// The window that owns the file, for the handoff pane's "Show Window" button.
    pub fn owning_window(&self) -> Option<WindowId> {
        let path = self.state.borrow().stage.file_path().map(str::to_string)?;
        self.registry.owning(&self.identity.canonical(&path))
    }

    // ---- watcher events (delivered on the UI thread) ----

    fn on_file_changed(&self, _path: &str) {
        let Stage::Editing(path) = self.stage() else { return };
        // Our own write fires this, and so does an attribute-only touch; only a moved stamp is a
        // real change. A conflict banner over identical bytes would invite a pointless,
        // text-discarding Reload.
        let current = self.fs.stamp(&path);
        if current.is_some() && current == self.state.borrow().disk_stamp {
            return;
        }
        if self.state.borrow().unsaved {
            self.with_state(|s| s.conflicted = true);
            self.cancel_autosave();
        } else {
            self.reload(&path, false);
        }
        self.notify();
    }

    /// LINUX DECISION: a rename whose *new* name is the file being edited is another program's
    /// atomic save landing on it (write a temporary, rename it over ours) — `GFileMonitor` with
    /// `WATCH_MOVES` reports exactly that as `Renamed`, the temporary as the file and ours as the
    /// other. Following it would re-stamp the session without reading the new bytes, and the next
    /// autosave would silently overwrite the other program's save, so it is handled as the change
    /// it is. (md.win's watcher adapter makes the same distinction before the event leaves it; the
    /// guard here keeps the session safe whatever the GTK adapter does.)
    fn on_file_renamed(&self, _old_path: &str, new_path: &str) {
        if let Stage::Editing(path) = self.stage()
            && file_names::same_path(&path, new_path)
        {
            self.on_file_changed(new_path);
            return;
        }
        self.retarget(new_path);
    }

    fn on_file_deleted(&self, _path: &str) {
        if !matches!(self.state.borrow().stage, Stage::Editing(_)) {
            return;
        }
        self.cancel_autosave();
        if self.state.borrow().unsaved {
            self.with_state(|s| s.conflicted = true); // Keep My Version recreates the file
        } else if self.role == SessionRole::BookArticle {
            self.detach(false);
            return; // detach notified already
        } else {
            // NSDocument keeps the window: the buffer and the path stay, the document is dirty
            // again, and the next save recreates the file (no stamp to be stale against).
            self.with_state(|s| {
                s.disk_stamp = None;
                s.unsaved = true;
                s.is_dirty = true;
            });
        }
        self.notify();
    }

    // ---- internals ----

    fn load(&self, path: &str) {
        let title = self.display_name_of(path);
        let document = document_loader::load(path, &*self.fs).document.filter(|d| d.kind == DocumentKind::PlainText);
        let Some(document) = document else {
            self.with_state(|s| {
                s.stage = Stage::Unreadable(path.to_string());
                s.title = title;
            });
            return;
        };
        let stamp = self.fs.stamp(path);
        self.with_state(|s| {
            s.text = document.text;
            s.dressing = document.dressing;
            s.title = title;
            s.disk_stamp = stamp;
            s.undo_generation += 1;
            s.stage = Stage::Editing(path.to_string());
            s.conflicted = false;
            s.save_error_text = None;
            s.last_saved_text = s.text.clone();
            s.is_dirty = false;
            s.unsaved = false;
        });
        self.watcher.watch(path);
        self.register(path);
        let text = self.text();
        self.text_replaced_externally.emit(&text);
    }

    /// The silent follow-the-disk of a clean session: text, dressing and stamp, nothing else. The
    /// undo stack survives unless the caller is discarding the buffer on purpose.
    fn reload(&self, path: &str, reset_undo: bool) -> bool {
        let document = document_loader::load(path, &*self.fs).document.filter(|d| d.kind == DocumentKind::PlainText);
        let Some(document) = document else { return false };
        let stamp = self.fs.stamp(path);
        self.with_state(|s| {
            s.text = document.text;
            s.dressing = document.dressing;
            s.disk_stamp = stamp;
            s.conflicted = false;
            s.save_error_text = None;
            s.last_saved_text = s.text.clone();
            s.is_dirty = false;
            s.unsaved = false;
            if reset_undo {
                s.undo_generation += 1;
            }
        });
        let text = self.text();
        self.text_replaced_externally.emit(&text);
        true
    }

    fn set_handoff(&self, path: &str) {
        let title = self.display_name_of(path);
        self.with_state(|s| {
            s.stage = Stage::Handoff(path.to_string());
            s.title = title;
        });
    }

    fn schedule_autosave(&self) {
        self.cancel_autosave();
        let me = self.me.clone();
        let handle = self.scheduler.after(
            Self::AUTOSAVE_DELAY,
            Box::new(move || {
                if let Some(session) = me.upgrade() {
                    session.flush_now(false);
                }
            }),
        );
        *self.autosave.borrow_mut() = Some(handle);
    }

    fn cancel_autosave(&self) {
        // Taken out first, dropped after: the handle's cancel runs with no borrow held.
        let handle = self.autosave.borrow_mut().take();
        drop(handle);
    }

    fn mark_explicitly_saved(&self) {
        self.with_state(|s| {
            s.is_dirty = false;
            s.last_saved_text = s.text.clone();
        });
    }

    fn register(&self, path: &str) {
        let canonical = self.identity.canonical(path);
        self.with_state(|s| s.canonical_path = Some(canonical.clone()));
        if let Some(id) = self.window_id {
            self.registry.register(id, &canonical);
        }
        self.identity_changed.emit(&Some(path.to_string()));
    }

    fn unregister(&self) {
        if self.state.borrow_mut().canonical_path.take().is_none() {
            return;
        }
        if let Some(id) = self.window_id {
            self.registry.unregister(id);
        }
    }

    fn owned_by_another(&self, path: &str) -> bool {
        let owner = self.registry.owning(&self.identity.canonical(path));
        owner.is_some_and(|id| self.window_id != Some(id))
    }

    /// A book article's title drops its ordering prefix ("01-Scene.md" → "Scene"); a document's
    /// does not.
    fn display_name_of(&self, path: &str) -> String {
        match self.role {
            SessionRole::BookArticle => book_naming::display_name(file_names::name_of(path)).to_string(),
            SessionRole::Document => file_names::stem_of(path).to_string(),
        }
    }

    fn write_failure_fallback(&self) -> &'static str {
        match self.role {
            SessionRole::BookArticle => strings::documents::COULD_NOT_WRITE_ARTICLE,
            SessionRole::Document => strings::documents::COULD_NOT_WRITE_DOCUMENT,
        }
    }

    fn with_state<R>(&self, change: impl FnOnce(&mut State) -> R) -> R {
        change(&mut self.state.borrow_mut())
    }

    fn notify(&self) {
        self.changed.emit(&());
    }

    /// Unsubscribes from the watcher, cancels the autosave and drops the registry entry. Does not
    /// flush — call [`TextFileSession::close_flush`] for that, as the window does. Idempotent;
    /// also run when the session is dropped.
    pub fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        if let Some([changed, renamed, deleted]) = self.subscriptions.take() {
            self.watcher.changed().disconnect(changed);
            self.watcher.renamed().disconnect(renamed);
            self.watcher.deleted().disconnect(deleted);
        }
        self.cancel_autosave();
        self.unregister();
    }
}

impl Drop for TextFileSession {
    fn drop(&mut self) {
        self.dispose();
    }
}
