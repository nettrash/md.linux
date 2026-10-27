//! Port of md.win `BookArticleSessionTests`: the in-place editing state machine (mdTests: the
//! fourteen testSession* / testBookFlushGate* tests) against a scratch book folder, plus the file
//! monitor events and the autosave / alert seams the Swift tests reached through AppKit.
//!
//! md.win serialises these tests in one xUnit collection because its flush gate is a static event
//! every live session answers; here each test posts through its own `BookFlushHub`, so they run in
//! parallel like everything else. Save failures are provoked as md.win does, by making the article
//! read-only — which root ignores, so those cases skip with a note when run as root.

mod book_support;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

use book_support::{TempDir, cp1251, make_unwritable, read_text, set_read_only};
use md_core::book::book_article_session::{
    AutosaveScheduler, BookAlerts, BookArticleSession, BookFlushHub, BookStage, BookStageKind, DocumentOwnership,
};
use md_core::book::book_paths;

/// A scratch book folder on disk with "01-Scene.md" = "# Scene\n".
struct SessionBook {
    dir: TempDir,
    article: String,
    session: BookArticleSession,
}

impl SessionBook {
    fn new() -> Self {
        Self::with(|session| session)
    }

    fn with(configure: impl FnOnce(BookArticleSession) -> BookArticleSession) -> Self {
        let dir = TempDir::new("md-session-");
        let article = dir.join(&["01-Scene.md"]);
        fs::write(&article, "# Scene\n").unwrap();
        let mut session = configure(BookArticleSession::new());
        session.open_book(&dir.path);
        SessionBook { dir, article, session }
    }

    fn write(&self, name: &str, text: &str) -> String {
        let path = self.dir.join(&[name]);
        fs::write(&path, text).unwrap();
        path
    }
}

#[derive(Default)]
struct FakeOwnership {
    owned: RefCell<HashSet<String>>,
    shown: RefCell<Vec<String>>,
}

impl DocumentOwnership for FakeOwnership {
    fn owns(&self, path: &str) -> bool {
        self.owned.borrow().iter().any(|o| book_paths::same(o, path))
    }

    fn show_owner(&self, path: &str) {
        self.shown.borrow_mut().push(path.to_string());
    }
}

#[derive(Default)]
struct FakeAlerts {
    presented: RefCell<Vec<(String, String)>>,
}

impl BookAlerts for FakeAlerts {
    fn present_error(&self, message: &str, informative: &str) {
        self.presented.borrow_mut().push((message.to_string(), informative.to_string()));
    }
}

/// The debounced autosave, fired by hand: `fire` is what the app's timer callback does.
#[derive(Default)]
struct ManualAutosave {
    pending: Cell<bool>,
    scheduled: Cell<usize>,
}

impl AutosaveScheduler for ManualAutosave {
    fn schedule(&self) {
        self.pending.set(true);
        self.scheduled.set(self.scheduled.get() + 1);
    }

    fn cancel(&self) {
        self.pending.set(false);
    }
}

impl ManualAutosave {
    fn fire(&self, session: &mut BookArticleSession) {
        if self.pending.replace(false) {
            session.flush_now();
        }
    }
}

fn standardized(path: &str) -> String {
    book_paths::standardize(path)
}

#[test]
fn session_loads_edits_and_flushes_to_disk() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    assert_eq!("# Scene\n", session.text());
    assert_eq!(Some(standardized(&article).as_str()), session.editing_path());
    assert_eq!("Scene", session.title());

    session.edit("# Scene\n\nIt was a dark and stormy night.\n");
    assert!(session.dirty());
    assert!(session.flush_now());
    assert!(!session.dirty());
    assert_eq!("# Scene\n\nIt was a dark and stormy night.\n", read_text(&article));
    session.close_book();
}

#[test]
fn session_selection_change_saves_the_outgoing_article() {
    let mut book = SessionBook::new();
    let second = book.write("02-Scene.md", "# Second\n");
    let article = book.article.clone();
    let session = &mut book.session;

    assert!(session.select(Some(&article)));
    session.edit("# Scene, revised\n");
    assert!(session.select(Some(&second)));
    // Moving on flushed the first article and loaded the second.
    assert_eq!("# Scene, revised\n", read_text(&article));
    assert_eq!("# Second\n", session.text());
    session.close_book();
}

#[test]
fn session_refuses_to_clobber_a_file_changed_under_it() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Mine\n");
    // Someone else rewrites the file (different size, so the staleness check cannot be fooled by
    // coarse timestamps).
    book.write("01-Scene.md", "# Theirs, and much longer than before\n");

    let session = &mut book.session;
    assert!(!session.flush_now());
    assert!(session.conflicted());
    // Neither side was lost: theirs is on disk, mine is in the buffer.
    assert_eq!("# Theirs, and much longer than before\n", read_text(&article));
    assert_eq!("# Mine\n", session.text());

    // The writer decides: Keep My Version writes the buffer out.
    session.resolve_conflict_keeping_mine();
    assert!(!session.conflicted());
    assert!(!session.dirty());
    assert_eq!("# Mine\n", read_text(&article));
    session.close_book();
}

#[test]
fn session_reload_resolution_discards_the_buffer() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Mine\n");
    book.write("01-Scene.md", "# Theirs, and much longer than before\n");
    let session = &mut book.session;
    assert!(!session.flush_now());
    assert!(session.conflicted());

    session.resolve_conflict_reloading();
    assert!(!session.conflicted());
    assert_eq!("# Theirs, and much longer than before\n", session.text());
    assert_eq!(Some(standardized(&article).as_str()), session.editing_path());
    // Nothing dirty remains, so closing writes nothing.
    session.close_book();
    assert_eq!("# Theirs, and much longer than before\n", read_text(&article));
}

#[test]
fn session_close_book_flushes_pending_edits() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Closing time\n");
    book.session.close_book();
    assert_eq!("# Closing time\n", read_text(&article));
    assert_eq!(&BookStage::Empty, book.session.stage());
    assert_eq!(None, book.session.root());
}

#[test]
fn session_clean_flush_leaves_the_file_untouched() {
    // flush_now runs on every selection change and window focus; a clean session must not rewrite
    // (and re-stamp) the file each time.
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    let before = fs::metadata(&article).unwrap().modified().unwrap();
    thread::sleep(Duration::from_millis(50));

    assert!(book.session.flush_now());

    assert_eq!(before, fs::metadata(&article).unwrap().modified().unwrap());
    book.session.close_book();
}

#[test]
fn session_deselect_flushes_and_fully_detaches() {
    // select(None) is the prelude of every managed file operation: it must flush the buffer and
    // fully let go of the file before anything moves on disk.
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# Detached\n");

    assert!(session.select(None));
    assert_eq!("# Detached\n", read_text(&article));
    assert_eq!(None, session.editing_path());
    assert_eq!(&BookStage::Empty, session.stage());
    assert_eq!("", session.text());
    assert_eq!("", session.title());
    assert!(!session.dirty());
    session.close_book();
}

#[test]
fn session_failed_flush_aborts_the_selection_change() {
    // The sidebar reverts its selection when select returns false, on the contract that the session
    // still holds the unsaved article.
    let mut book = SessionBook::new();
    let second = book.write("02-Scene.md", "# Second\n");
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# Unsaved\n");
    if !make_unwritable(&article) {
        return;
    }

    assert!(!session.select(Some(&second)));
    assert_eq!(Some(standardized(&article).as_str()), session.editing_path());
    assert_eq!("# Unsaved\n", session.text());
    assert!(session.dirty());
    assert!(session.save_error_text().is_some());

    // Once the file is writable again the same move succeeds.
    set_read_only(&article, false);
    assert!(session.select(Some(&second)));
    assert_eq!("# Unsaved\n", read_text(&article));
    assert_eq!(None, session.save_error_text());
    session.close_book();
}

#[test]
fn session_hand_off_for_external_open_saves_then_steps_aside() {
    // Opening the edited article in its own window must ship the buffer to disk first and leave the
    // session in handoff so there is never a second writer.
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# For the window\n");

    assert!(session.hand_off_for_external_open());
    assert_eq!("# For the window\n", read_text(&article));
    assert_eq!(&BookStage::Handoff(standardized(&article)), session.stage());
    assert_eq!(None, session.editing_path());
    assert_eq!("Scene", session.title());
    session.close_book();
}

#[test]
fn session_steps_aside_while_a_document_owns_the_article() {
    // The two-writers guard: while a document window has the file, selecting it yields a handoff,
    // and the session reclaims it once the document goes away.
    let ownership = Rc::new(FakeOwnership::default());
    let mut book = SessionBook::with(|s| s.with_ownership(ownership.clone()));
    let article = book.article.clone();
    let session = &mut book.session;
    ownership.owned.borrow_mut().insert(article.clone());

    assert!(session.select(Some(&article)));
    assert_eq!(&BookStage::Handoff(standardized(&article)), session.stage());
    assert_eq!(None, session.editing_path());
    session.show_owning_window();
    assert_eq!(1, ownership.shown.borrow().len());

    ownership.owned.borrow_mut().clear();
    session.recheck_ownership();
    assert_eq!(Some(standardized(&article).as_str()), session.editing_path());
    assert_eq!("# Scene\n", session.text());
    session.close_book();
}

#[test]
fn recheck_ownership_saves_then_hands_off_when_a_document_opens_over_an_edited_article() {
    let ownership = Rc::new(FakeOwnership::default());
    let mut book = SessionBook::with(|s| s.with_ownership(ownership.clone()));
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# Edited here first\n");

    ownership.owned.borrow_mut().insert(article.clone());
    session.recheck_ownership();
    assert_eq!(&BookStage::Handoff(standardized(&article)), session.stage());
    assert_eq!("# Edited here first\n", read_text(&article));
    assert!(!session.dirty());

    // A failed flush stays put: the buffer must remain visible with its error.
    ownership.owned.borrow_mut().clear();
    session.recheck_ownership();
    assert_eq!(BookStageKind::Editing, session.stage().kind());
    session.edit("# Unsavable\n");
    if !make_unwritable(&article) {
        return;
    }
    ownership.owned.borrow_mut().insert(article.clone());
    session.recheck_ownership();
    assert_eq!(BookStageKind::Editing, session.stage().kind());
    assert!(session.save_error_text().is_some());
    set_read_only(&article, false);
    session.close_book();
}

#[test]
fn session_rescue_copy_parks_the_buffer_without_overwriting() {
    // The quit-time last resort: when the regular write cannot land, the buffer goes to a fresh
    // "(rescued)" sibling, clobbering nothing.
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Mine\n");
    // Occupy the first rescue name to prove the copy never overwrites.
    let taken = book.write("01-Scene (rescued).md", "occupied");

    let rescued = book.session.write_rescue_copy(&article).expect("rescued");
    assert_eq!("01-Scene (rescued 2).md", book_paths::file_name(&rescued));
    assert_eq!("# Mine\n", read_text(&rescued));
    assert_eq!("occupied", read_text(&taken));
    // The rescue is a copy, not a save: the buffer is still dirty.
    assert!(book.session.dirty());
    book.session.close_book();
}

/// A session the flush gate can reach: it lives in an `Rc<RefCell<…>>`, subscribed to a hub of its
/// own (md.win: the static `BookFlushGate.Requested`).
fn shared(book: SessionBook) -> (TempDir, String, Rc<RefCell<BookArticleSession>>, BookFlushHub) {
    let SessionBook { dir, article, session } = book;
    let session = Rc::new(RefCell::new(session));
    let hub = BookFlushHub::new();
    hub.subscribe_session(&session);
    (dir, article, session, hub)
}

#[test]
fn book_flush_gate_saves_the_buffer_before_compile() {
    // The book output actions can't reach the session directly — the flush travels through the
    // gate, and the buffer must be on disk when post returns.
    let (_dir, article, session, hub) = shared(SessionBook::new());
    assert!(session.borrow_mut().select(Some(&article)));
    session.borrow_mut().edit("# Through the gate\n");

    let gate = hub.post();
    assert!(!gate.vetoed);
    assert_eq!("# Through the gate\n", read_text(&article));
    assert!(!session.borrow().dirty());
    assert!(hub.flush_editor());
    session.borrow_mut().close_book();
}

#[test]
fn book_flush_gate_vetoes_when_the_save_fails() {
    // A compile must never ship a stale page: when the flush cannot land, the gate is vetoed and the
    // output action aborts.
    let (_dir, article, session, hub) = shared(SessionBook::new());
    assert!(session.borrow_mut().select(Some(&article)));
    session.borrow_mut().edit("# Unsavable\n");
    if !make_unwritable(&article) {
        return;
    }

    let gate = hub.post();
    assert!(gate.vetoed);
    assert!(!hub.flush_editor());
    // Nothing was lost: the buffer is still the session's to save.
    assert_eq!("# Unsavable\n", session.borrow().text());
    assert!(session.borrow().dirty());

    set_read_only(&article, false);
    session.borrow_mut().close_book();
    assert_eq!("# Unsavable\n", read_text(&article));
}

/// The Rust shape of md.win's static event: no subscriber → nothing unsaved → the gate stays open;
/// a dropped session leaves the hub (md.win's `Dispose`); a session already borrowed on this thread
/// cannot save, so it vetoes rather than let a compile read a stale file.
#[test]
fn book_flush_hub_is_open_when_empty_forgets_dropped_sessions_and_vetoes_a_busy_one() {
    let hub = BookFlushHub::new();
    assert!(hub.flush_editor());

    let (_dir, article, session, own_hub) = shared(SessionBook::new());
    hub.subscribe_session(&session);
    assert!(session.borrow_mut().select(Some(&article)));
    session.borrow_mut().edit("# Busy\n");
    {
        let _busy = session.borrow_mut();
        assert!(hub.post().vetoed);
    }
    assert!(hub.flush_editor());
    assert_eq!("# Busy\n", read_text(&article));

    session.borrow_mut().edit("# Dropped unsaved\n");
    drop(session);
    assert!(hub.flush_editor());
    assert!(own_hub.flush_editor());
    assert_eq!("# Busy\n", read_text(&article));
}

#[test]
fn session_round_trips_legacy_encoding() {
    let mut book = SessionBook::new();
    let legacy = book.dir.join(&["03-Legacy.md"]);
    let original = "Привет, мир!";
    fs::write(&legacy, cp1251(original)).unwrap();
    let session = &mut book.session;

    assert!(session.select(Some(&legacy)));
    assert_eq!(original, session.text());
    assert_eq!(1251, session.code_page());
    session.edit(&format!("{original} Ещё."));
    assert!(session.flush_now());
    // The save stayed in the file's own encoding.
    assert_eq!(cp1251(&format!("{original} Ещё.")), fs::read(&legacy).unwrap());
    assert_eq!(1251, session.code_page());

    // Text the encoding cannot hold upgrades the file to UTF-8 and says so.
    session.edit(&format!("{original} \u{1F642}"));
    assert!(session.flush_now());
    assert_eq!(65001, session.code_page());
    assert_eq!(format!("{original} \u{1F642}"), read_text(&legacy));
    session.close_book();
}

#[test]
fn session_round_trips_a_bomed_utf16_article() {
    // UTF-16 is recognised only behind its BOM; the save writes the BOM back so the file stays what
    // it was, instead of being silently rewritten as UTF-8.
    let mut book = SessionBook::new();
    let wide = book.dir.join(&["04-Wide.md"]);
    let utf16 = |text: &str| {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        bytes
    };
    fs::write(&wide, utf16("# Chapter\n")).unwrap();
    let session = &mut book.session;

    assert!(session.select(Some(&wide)));
    assert_eq!("# Chapter\n", session.text());
    assert_eq!(1200, session.code_page());
    session.edit("# Chapter\n\nWide.\n");
    assert!(session.flush_now());
    assert_eq!(utf16("# Chapter\n\nWide.\n"), fs::read(&wide).unwrap());
    session.close_book();
}

#[test]
fn session_reports_an_unreadable_article_without_failing_the_selection() {
    let mut book = SessionBook::new();
    let missing = book.dir.join(&["02-Gone.md"]);
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&missing)));
    assert_eq!(&BookStage::Unreadable(standardized(&missing)), session.stage());
    assert_eq!(None, session.editing_path());
    assert_eq!("Gone", session.title());
    assert_eq!("", session.text());
    // A real article afterwards loads normally.
    assert!(session.select(Some(&article)));
    assert_eq!("# Scene\n", session.text());
    session.close_book();
}

#[test]
fn selecting_the_edited_article_again_is_a_no_op() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# Still typing\n");
    let generation = session.undo_generation();
    assert!(session.select(Some(&article)));
    assert!(session.dirty());
    assert_eq!("# Still typing\n", session.text());
    assert_eq!(generation, session.undo_generation());
    assert_eq!("# Scene\n", read_text(&article));
    session.close_book();
}

#[test]
fn file_changed_reloads_a_clean_session_and_conflicts_a_dirty_one() {
    let autosave = Rc::new(ManualAutosave::default());
    let mut book = SessionBook::with(|s| s.with_autosave(autosave.clone()));
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));

    // Our own write leaves the stamp we recorded: nothing happens.
    book.session.edit("# Ours\n");
    assert!(book.session.flush_now());
    book.session.file_changed();
    assert!(!book.session.conflicted());
    assert_eq!("# Ours\n", book.session.text());

    // Clean session, external change: follow the disk silently.
    book.write("01-Scene.md", "# Theirs, rewritten outside\n");
    book.session.file_changed();
    assert!(!book.session.conflicted());
    assert_eq!("# Theirs, rewritten outside\n", book.session.text());
    assert!(!book.session.dirty());

    // Dirty session, external change: conflict, autosave disarmed.
    book.session.edit("# Mine again\n");
    assert!(autosave.pending.get());
    book.write("01-Scene.md", "# Theirs once more, longer still\n");
    book.session.file_changed();
    assert!(book.session.conflicted());
    assert!(!autosave.pending.get());
    assert_eq!("# Mine again\n", book.session.text());
    // Typing while conflicted does not re-arm the autosave.
    book.session.edit("# Mine again, more\n");
    assert!(!autosave.pending.get());
    book.session.resolve_conflict_reloading();
    book.session.close_book();
}

#[test]
fn file_deleted_detaches_a_clean_session_and_conflicts_a_dirty_one() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    fs::remove_file(&article).unwrap();
    book.session.file_deleted();
    assert_eq!(&BookStage::Empty, book.session.stage());

    let again = book.write("01-Scene.md", "# Scene\n");
    let session = &mut book.session;
    assert!(session.select(Some(&again)));
    session.edit("# Keep me\n");
    fs::remove_file(&again).unwrap();
    session.file_deleted();
    assert!(session.conflicted());
    assert_eq!(BookStageKind::Editing, session.stage().kind());
    // A vanished file is stale to the regular flush; Keep My Version recreates it.
    assert!(!session.flush_now());
    session.resolve_conflict_keeping_mine();
    assert_eq!("# Keep me\n", read_text(&again));
    assert!(!session.dirty());
    session.close_book();
}

#[test]
fn file_moved_follows_the_rename() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let moved = book.dir.join(&["01-Renamed.md"]);
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    session.edit("# Renamed under us\n");
    // The stamp travels with the file, so the pending save lands at the new name.
    fs::rename(&article, &moved).unwrap();
    session.file_moved(&moved);
    assert_eq!(Some(standardized(&moved).as_str()), session.editing_path());
    assert_eq!("Renamed", session.title());
    assert!(session.flush_now());
    assert_eq!("# Renamed under us\n", read_text(&moved));
    session.close_book();
}

#[test]
fn autosave_is_armed_per_keystroke_and_disarmed_on_detach() {
    let autosave = Rc::new(ManualAutosave::default());
    let mut book = SessionBook::with(|s| s.with_autosave(autosave.clone()));
    let article = book.article.clone();
    let session = &mut book.session;
    assert!(session.select(Some(&article)));
    assert!(!autosave.pending.get());

    session.edit("# One\n");
    session.edit("# One two\n");
    assert_eq!(2, autosave.scheduled.get());
    assert!(autosave.pending.get());
    autosave.fire(session);
    assert!(!session.dirty());
    assert_eq!("# One two\n", read_text(&article));

    session.edit("# One two three\n");
    assert!(session.select(None));
    assert!(!autosave.pending.get());
    assert_eq!("# One two three\n", read_text(&article));
    session.close_book();
}

#[test]
fn close_book_rescues_an_unsavable_buffer_and_says_so() {
    let alerts = Rc::new(FakeAlerts::default());
    let mut book = SessionBook::with(|s| s.with_alerts(alerts.clone()));
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Cannot land\n");
    if !make_unwritable(&article) {
        return;
    }

    book.session.close_book();
    assert_eq!(&BookStage::Empty, book.session.stage());
    let rescued = book.dir.join(&["01-Scene (rescued).md"]);
    assert!(Path::new(&rescued).is_file());
    assert_eq!("# Cannot land\n", read_text(&rescued));
    assert_eq!("# Scene\n", read_text(&article));
    let presented = alerts.presented.borrow();
    assert_eq!(1, presented.len());
    assert_eq!("Could not save \u{201C}Scene\u{201D}", presented[0].0);
    assert_eq!("Your text was kept as \u{201C}01-Scene (rescued).md\u{201D} in the same folder.", presented[0].1);
}

#[test]
fn terminate_flush_writes_or_rescues() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    assert!(book.session.select(Some(&article)));
    book.session.edit("# Quitting\n");
    book.session.terminate_flush();
    assert_eq!("# Quitting\n", read_text(&article));

    book.session.edit("# Quitting, conflicted\n");
    book.write("01-Scene.md", "# Someone else, at the last moment\n");
    book.session.terminate_flush();
    assert!(book.session.conflicted());
    assert_eq!("# Quitting, conflicted\n", read_text(&book.dir.join(&["01-Scene (rescued).md"])));
    book.session.resolve_conflict_reloading();
    book.session.close_book();
}

#[test]
fn undo_generation_changes_on_every_load_and_detach() {
    let mut book = SessionBook::new();
    let second = book.write("02-Scene.md", "# Second\n");
    let article = book.article.clone();
    let session = &mut book.session;
    let g0 = session.undo_generation();
    assert!(session.select(Some(&article)));
    let g1 = session.undo_generation();
    assert_ne!(g0, g1);
    assert!(session.select(Some(&second)));
    let g2 = session.undo_generation();
    assert_ne!(g1, g2);
    assert!(session.select(None));
    assert_ne!(g2, session.undo_generation());
    session.close_book();
}

#[test]
fn changed_fires_on_observable_transitions() {
    let mut book = SessionBook::new();
    let article = book.article.clone();
    let session = &mut book.session;
    let changes = Rc::new(Cell::new(0));
    let counter = Rc::clone(&changes);
    session.connect_changed(move || counter.set(counter.get() + 1));
    assert!(session.select(Some(&article)));
    assert!(changes.get() > 0);
    let after_select = changes.get();
    session.edit("# Typed\n");
    assert!(changes.get() > after_select);
    session.close_book();
}
