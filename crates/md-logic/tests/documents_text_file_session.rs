//! Port of md.win `tests/Md.App.Logic.Tests/TextFileSessionTests.cs`.
//!
//! §6.3's table and the fourteen macOS `BookArticleSession` behaviours (book.md §10.10), against
//! the fakes. The article fixture is the Mac's: `01-Scene.md` holding `"# Scene\n"`. md.win's
//! paths are `C:\…` on a case-insensitive fake; these are Linux paths on a case-sensitive one, and
//! `chmod 444` is the read-only switch (md.win: `FileAttributes.ReadOnly`).

mod common;

use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use common::document_fixtures::{cp1251, notes_pack, prefixed, utf16le};
use md_logic::documents::{SessionRole, Stage, TextFileDressing, TextFileSession};
use md_logic::strings;
use md_logic::testing::*;

const ARTICLE: &str = "/Book/01-Scene.md";
const OTHER: &str = "/Book/02-Other.md";
const DOCUMENT: &str = "/Docs/Chapter One.md";

/// md.win's `Guid.NewGuid()`.
fn new_window() -> WindowId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    WindowId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// One session with every seam a test can reach into, plus a log of what it asked for. Dropping
/// it drops the session, which disposes it (md.win's `using var w`).
struct World {
    fs: Rc<FakeFileSystem>,
    watcher: Rc<FakeFileWatcher>,
    scheduler: Rc<FakeScheduler>,
    registry: Rc<FakeDocumentRegistry>,
    session: Rc<TextFileSession>,
    window_id: WindowId,
    alerts: Rc<RefCell<Vec<(String, String)>>>,
    replacements: Rc<RefCell<Vec<String>>>,
    identities: Rc<RefCell<Vec<Option<String>>>>,
    changed_count: Rc<Cell<usize>>,
}

impl World {
    fn new(role: SessionRole) -> World {
        let clock = Rc::new(FakeClock::new());
        let fs = Rc::new(FakeFileSystem::with_clock(clock.clone()));
        let scheduler = Rc::new(FakeScheduler::with_clock(clock));
        let watcher = Rc::new(FakeFileWatcher::new());
        let registry = Rc::new(FakeDocumentRegistry::new());
        let identity = Rc::new(FakeFileIdentity::new());
        let window_id = new_window();
        let session = TextFileSession::new(
            fs.clone(),
            watcher.clone(),
            scheduler.clone(),
            registry.clone(),
            identity,
            role,
            (role == SessionRole::Document).then_some(window_id),
        );
        let alerts = Rc::new(RefCell::new(Vec::new()));
        let replacements = Rc::new(RefCell::new(Vec::new()));
        let identities = Rc::new(RefCell::new(Vec::new()));
        let changed_count = Rc::new(Cell::new(0));
        {
            let count = changed_count.clone();
            session.changed().connect(move |_| count.set(count.get() + 1));
            let alerts = alerts.clone();
            session
                .alert_requested()
                .connect(move |(t, m): &(String, String)| alerts.borrow_mut().push((t.clone(), m.clone())));
            let replacements = replacements.clone();
            session.text_replaced_externally().connect(move |t: &str| replacements.borrow_mut().push(t.to_string()));
            let identities = identities.clone();
            session.identity_changed().connect(move |p: &Option<String>| identities.borrow_mut().push(p.clone()));
        }
        World { fs, watcher, scheduler, registry, session, window_id, alerts, replacements, identities, changed_count }
    }

    /// The fakes' shared normalisation, so a registry key built here matches the session's.
    fn canonical(path: &str) -> String {
        FakeFileIdentity::normalise(path)
    }

    fn alerts(&self) -> Vec<(String, String)> {
        self.alerts.borrow().clone()
    }

    fn replacements(&self) -> Vec<String> {
        self.replacements.borrow().clone()
    }

    fn identities(&self) -> Vec<Option<String>> {
        self.identities.borrow().clone()
    }
}

fn book() -> World {
    let world = World::new(SessionRole::BookArticle);
    world.fs.add_file(ARTICLE, "# Scene\n");
    world.fs.add_file(OTHER, "# Other\n");
    world
}

fn doc() -> World {
    let world = World::new(SessionRole::Document);
    world.fs.add_file(DOCUMENT, "# Title\nBody\n");
    world
}

fn some(path: &str) -> Option<String> {
    Some(path.to_string())
}

// ---- the fourteen ----

#[test]
fn session_loads_edits_and_flushes_to_disk() {
    let w = book();
    assert!(w.session.select(Some(ARTICLE)));
    assert_eq!(w.session.text(), "# Scene\n");
    assert_eq!(w.session.editing_path(), some(ARTICLE));
    assert!(!w.session.is_dirty());

    w.session.edit("# Scene\nMore.\n");
    assert!(w.session.is_dirty());

    assert!(w.session.flush_now(false));
    assert!(!w.session.is_dirty());
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("# Scene\nMore.\n"));
}

#[test]
fn session_selection_change_saves_the_outgoing_article() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("edited A\n");

    assert!(w.session.select(Some(OTHER)));

    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("edited A\n"));
    assert_eq!(w.session.text(), "# Other\n");
    assert_eq!(w.session.editing_path(), some(OTHER));
}

#[test]
fn session_refuses_to_clobber_a_file_changed_under_it() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("mine\n");
    w.fs.write_externally(ARTICLE, "theirs, and longer\n"); // a different size: no coarse clock can mask it

    assert!(!w.session.flush_now(true));
    assert!(w.session.conflicted());
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("theirs, and longer\n"));
    assert_eq!(w.session.text(), "mine\n");

    w.session.resolve_conflict_keeping_mine();
    assert!(!w.session.conflicted());
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("mine\n"));
}

#[test]
fn session_reload_resolution_discards_the_buffer() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("mine\n");
    w.fs.write_externally(ARTICLE, "theirs, and longer\n");
    assert!(!w.session.flush_now(false));

    w.session.resolve_conflict_reloading();

    assert!(!w.session.conflicted());
    assert_eq!(w.session.text(), "theirs, and longer\n");
    let writes_before = w.fs.writes().len();
    w.session.detach(true); // Close Book writes nothing
    assert_eq!(w.fs.writes().len(), writes_before);
}

#[test]
fn session_close_book_flushes_pending_edits() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("last words\n");

    w.session.detach(true);

    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("last words\n"));
    assert!(w.alerts().is_empty());
}

#[test]
fn session_clean_flush_leaves_the_file_untouched() {
    let w = book();
    w.session.select(Some(ARTICLE));
    let before = w.fs.stamp(ARTICLE);

    assert!(w.session.flush_now(false));

    assert_eq!(w.fs.stamp(ARTICLE), before);
    assert!(w.fs.writes().is_empty());
}

#[test]
fn session_deselect_flushes_and_fully_detaches() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("saved on the way out\n");

    assert!(w.session.select(None));

    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("saved on the way out\n"));
    assert_eq!(w.session.editing_path(), None);
    assert_eq!(w.session.stage(), Stage::Empty);
    assert_eq!(w.session.text(), "");
    assert!(!w.session.is_dirty());
}

#[test]
fn session_failed_flush_aborts_the_selection_change() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("cannot be written\n");
    w.fs.set_read_only(ARTICLE, true); // chmod 444

    assert!(!w.session.select(Some(OTHER)));
    assert_eq!(w.session.editing_path(), some(ARTICLE));
    assert_eq!(w.session.text(), "cannot be written\n");
    assert!(w.session.is_dirty());
    assert!(w.session.save_error_text().is_some());

    w.fs.set_read_only(ARTICLE, false);
    assert!(w.session.select(Some(OTHER)));
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("cannot be written\n"));
}

#[test]
fn session_hand_off_for_external_open_saves_then_steps_aside() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("handed off\n");

    assert!(w.session.hand_off_for_external_open());

    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("handed off\n"));
    assert!(matches!(w.session.stage(), Stage::Handoff(_)));
    assert_eq!(w.session.editing_path(), None);
}

#[test]
fn session_steps_aside_while_a_document_owns_the_article() {
    let w = book();
    let document_window = new_window();
    w.registry.register(document_window, &World::canonical(ARTICLE));

    w.session.select(Some(ARTICLE));
    assert!(matches!(w.session.stage(), Stage::Handoff(_)));
    assert_eq!(w.session.owning_window(), Some(document_window));

    w.registry.unregister(document_window);
    w.session.recheck_ownership();

    assert!(matches!(w.session.stage(), Stage::Editing(_)));
    assert_eq!(w.session.text(), "# Scene\n");
}

#[test]
fn recheck_ownership_saves_then_hands_off_when_a_document_opens_over_an_edited_article() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("saved before handing over\n");

    w.registry.register(new_window(), &World::canonical(ARTICLE));
    w.session.recheck_ownership();

    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("saved before handing over\n"));
    assert!(matches!(w.session.stage(), Stage::Handoff(_)));
}

#[test]
fn session_rescue_copy_parks_the_buffer_without_overwriting() {
    let w = book();
    w.fs.add_file("/Book/01-Scene (rescued).md", "an earlier rescue");
    w.session.select(Some(ARTICLE));
    w.session.edit("unsavable\n");
    w.fs.set_read_only(ARTICLE, true);

    let rescued = w.session.write_rescue_copy();

    assert_eq!(rescued.as_deref(), Some("/Book/01-Scene (rescued 2).md"));
    assert_eq!(w.fs.text(&rescued.unwrap()).as_deref(), Some("unsavable\n"));
    assert_eq!(w.fs.text("/Book/01-Scene (rescued).md").as_deref(), Some("an earlier rescue"));
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("# Scene\n"));
    assert!(w.session.is_dirty()); // a rescue is a copy, not a save
}

#[test]
fn the_flush_gate_saves_the_buffer_synchronously_and_reports_its_verdict() {
    // The gate itself is the book output's (the book window wires to it); what the session owes it
    // is a synchronous flush whose bool is the veto.
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("compile me\n");

    assert!(w.session.flush_now(false));
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("compile me\n"));

    w.session.edit("and now refuse\n");
    w.fs.set_read_only(ARTICLE, true);
    assert!(!w.session.flush_now(false));
}

#[test]
fn session_round_trips_legacy_encoding() {
    let w = book();
    let original = "Привет, мир!";
    w.fs.add_file("/Book/03-Legacy.md", cp1251(original));

    w.session.select(Some("/Book/03-Legacy.md"));
    assert_eq!(w.session.text(), original);

    w.session.edit(&format!("{original} Ещё."));
    assert!(w.session.flush_now(false));

    assert_eq!(w.fs.bytes("/Book/03-Legacy.md"), Some(cp1251(&format!("{original} Ещё."))));
}

// ---- the rest of the macOS session behaviours ----

#[test]
fn session_round_trips_a_bomed_utf16_article() {
    let w = book();
    w.fs.add_file("/Book/04-Wide.md", prefixed(&[0xFF, 0xFE], &utf16le("Wide\n")));

    w.session.select(Some("/Book/04-Wide.md"));
    assert_eq!(w.session.text(), "Wide\n");
    w.session.edit("Wider\n");
    w.session.flush_now(false);

    assert_eq!(w.fs.bytes("/Book/04-Wide.md"), Some(prefixed(&[0xFF, 0xFE], &utf16le("Wider\n"))));
}

#[test]
fn session_reports_an_unreadable_article_without_failing_the_selection() {
    let w = book();
    assert!(w.session.select(Some("/Book/99-Gone.md")));
    assert!(matches!(w.session.stage(), Stage::Unreadable(_)));
    assert_eq!(w.session.title(), "Gone"); // the book strips the ordering prefix
}

#[test]
fn selecting_the_edited_article_again_is_a_no_op() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("in progress\n");
    let changes = w.changed_count.get();

    assert!(w.session.select(Some(ARTICLE)));
    // …and another spelling of the same file is the same file. (md.win: "C:/Book/…" for
    // "C:\Book\…"; on Linux a dot segment and a doubled separator are what spell one file twice.)
    assert!(w.session.select(Some("/Book/./01-Scene.md")));
    assert!(w.session.select(Some("/Book//01-Scene.md")));

    assert_eq!(w.session.text(), "in progress\n");
    assert!(w.session.is_dirty());
    assert!(w.fs.writes().is_empty());
    assert_eq!(w.changed_count.get(), changes);
}

#[test]
fn file_changed_reloads_a_clean_session_and_conflicts_a_dirty_one() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.fs.write_externally(ARTICLE, "theirs\n");
    w.watcher.raise_changed(None);
    assert!(!w.session.conflicted());
    assert_eq!(w.session.text(), "theirs\n");
    assert_eq!(w.replacements(), ["# Scene\n", "theirs\n"]);

    w.session.edit("mine\n");
    w.fs.write_externally(ARTICLE, "theirs again, longer\n");
    w.watcher.raise_changed(None);
    assert!(w.session.conflicted());
    assert_eq!(w.session.text(), "mine\n");
}

#[test]
fn our_own_write_and_an_attribute_only_touch_are_not_external_changes() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("ours\n");
    w.session.flush_now(false);
    let replacements = w.replacements().len();

    w.watcher.raise_changed(None); // the echo of our own write
    assert!(!w.session.conflicted());
    assert_eq!(w.replacements().len(), replacements);

    w.fs.touch(ARTICLE); // mtime moves, bytes do not
    w.watcher.raise_changed(None);
    assert!(!w.session.conflicted());
    assert_eq!(w.session.text(), "ours\n"); // reloaded, silently, with the same text
}

#[test]
fn file_deleted_detaches_a_clean_book_session_and_conflicts_a_dirty_one() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("mine\n");
    w.fs.delete(ARTICLE).unwrap();
    w.watcher.raise_deleted(None);
    assert!(w.session.conflicted());
    assert_eq!(w.session.text(), "mine\n");

    w.session.resolve_conflict_keeping_mine(); // recreates the file
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("mine\n"));

    w.fs.delete(ARTICLE).unwrap();
    w.watcher.raise_deleted(None);
    assert_eq!(w.session.stage(), Stage::Empty);
}

#[test]
fn file_moved_follows_the_rename() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.fs.move_file(ARTICLE, "/Book/01-Scene Renamed.md").unwrap();

    w.watcher.raise_renamed("/Book/01-Scene Renamed.md", None);

    assert_eq!(w.session.editing_path(), some("/Book/01-Scene Renamed.md"));
    assert_eq!(w.session.title(), "Scene Renamed");
    assert_eq!(w.watcher.watched_path(), some("/Book/01-Scene Renamed.md"));
}

#[test]
fn autosave_is_armed_per_keystroke_and_disarmed_on_detach() {
    let w = book();
    w.session.select(Some(ARTICLE));

    w.session.edit("a");
    assert_eq!(w.scheduler.pending_timers(), 1);
    w.scheduler.advance(Duration::from_millis(900));
    w.session.edit("ab"); // restarts the debounce
    assert_eq!(w.scheduler.pending_delays(), [TextFileSession::AUTOSAVE_DELAY]);
    assert!(w.fs.writes().is_empty());

    w.scheduler.advance(TextFileSession::AUTOSAVE_DELAY);
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("ab"));

    w.session.edit("abc");
    w.session.detach(false);
    assert_eq!(w.scheduler.pending_timers(), 0);
}

#[test]
fn close_book_rescues_an_unsavable_buffer_and_says_so() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("unsavable\n");
    w.fs.set_read_only(ARTICLE, true);

    w.session.detach(true);

    let alerts = w.alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, strings::documents::rescue_failed_title("Scene"));
    assert_eq!(alerts[0].1, strings::documents::rescue_kept_message("01-Scene (rescued).md"));
    assert_eq!(w.fs.text("/Book/01-Scene (rescued).md").as_deref(), Some("unsavable\n"));
    assert_eq!(w.session.stage(), Stage::Empty); // detach always completes
}

#[test]
fn terminate_flush_writes_or_rescues() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("in time\n");
    w.session.terminate_flush();
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("in time\n"));

    w.session.edit("too late\n");
    w.fs.set_read_only(ARTICLE, true);
    w.session.terminate_flush();
    assert_eq!(w.fs.text("/Book/01-Scene (rescued).md").as_deref(), Some("too late\n"));
    assert!(w.alerts().is_empty()); // no modal at exit
}

#[test]
fn undo_generation_changes_on_every_load_and_detach() {
    let w = book();
    let start = w.session.undo_generation();
    w.session.select(Some(ARTICLE));
    let after_load = w.session.undo_generation();
    assert_ne!(start, after_load);

    w.session.select(Some(OTHER));
    assert_ne!(after_load, w.session.undo_generation());

    let before_detach = w.session.undo_generation();
    w.session.detach(false);
    assert_ne!(before_detach, w.session.undo_generation());
}

#[test]
fn changed_fires_on_observable_transitions() {
    let w = book();
    assert_eq!(w.changed_count.get(), 0);
    w.session.select(Some(ARTICLE));
    assert!(w.changed_count.get() > 0);

    let after_select = w.changed_count.get();
    w.session.edit("x");
    assert!(w.changed_count.get() > after_select);

    let after_edit = w.changed_count.get();
    w.session.edit("x"); // the same text is not a change
    assert_eq!(w.changed_count.get(), after_edit);
}

// ---- the document-window rows of §6.3 ----

#[test]
fn opening_a_document_takes_its_stamp_watches_it_and_claims_it() {
    let w = doc();
    assert!(w.session.open(DOCUMENT));

    assert_eq!(w.session.text(), "# Title\nBody\n");
    assert_eq!(w.session.title(), "Chapter One");
    assert!(!w.session.is_dirty());
    assert_eq!(w.session.last_saved_text(), w.session.text());
    assert_eq!(w.session.disk_stamp(), w.fs.stamp(DOCUMENT));
    assert_eq!(w.watcher.watched_path(), some(DOCUMENT));
    assert_eq!(w.registry.owning(&World::canonical(DOCUMENT)), Some(w.window_id));
    assert_eq!(w.identities(), [some(DOCUMENT)]);
}

#[test]
fn a_failed_open_asks_for_the_alert_and_changes_nothing() {
    let w = doc();
    assert!(!w.session.open("/Docs/missing.md"));
    let alerts = w.alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, strings::documents::COULD_NOT_OPEN);
    assert_eq!(w.session.stage(), Stage::Untitled);
}

#[test]
fn an_imported_bundle_is_an_untitled_document_titled_after_the_bundle_and_owns_no_file() {
    let w = doc();
    w.fs.add_file("/Downloads/Trip.textpack", notes_pack("# Trip\n"));

    assert!(w.session.open("/Downloads/Trip.textpack"));

    assert_eq!(w.session.stage(), Stage::Untitled);
    assert_eq!(w.session.title(), "Trip");
    assert_eq!(w.session.imported_from(), some("/Downloads/Trip.textpack"));
    assert!(!w.session.is_dirty()); // closing an unedited import prompts nothing
    assert_eq!(w.session.canonical_path(), None);
    assert!(!w.watcher.is_watching());
}

#[test]
fn an_autosave_writes_the_file_but_leaves_the_titles_edited_marker_alone() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("# Title\nEdited\n");

    w.scheduler.advance(TextFileSession::AUTOSAVE_DELAY);

    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("# Title\nEdited\n"));
    assert!(w.session.is_dirty()); // "— Edited" survives an autosave-in-place
    assert!(!w.session.has_unsaved_changes());
}

#[test]
fn only_an_explicit_save_clears_the_edited_marker_even_with_nothing_left_to_write() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("# Title\nEdited\n");
    w.scheduler.advance(TextFileSession::AUTOSAVE_DELAY);
    assert!(w.session.is_dirty());

    assert!(w.session.flush_now(true));

    assert!(!w.session.is_dirty());
    assert_eq!(w.session.last_saved_text(), "# Title\nEdited\n");
    assert_eq!(w.fs.writes().len(), 1); // the explicit save wrote nothing new
}

#[test]
fn save_on_an_untitled_document_writes_nothing_because_the_caller_routes_to_save_as() {
    let w = doc();
    w.session.open_untitled("draft", None, true);
    assert!(w.session.flush_now(true));
    assert!(w.fs.writes().is_empty());
    assert!(w.session.is_dirty());
}

#[test]
fn save_as_writes_the_new_path_keeps_the_dressing_and_leaves_the_old_file_alone() {
    let w = doc();
    w.fs.add_file("/Docs/legacy.md", cp1251("Привет\r\n"));
    w.fs.add_directory("/Docs/Copies");
    w.session.open("/Docs/legacy.md");
    w.session.edit("Привет\nЕщё\n");

    assert!(w.session.save_as("/Docs/Copies/copy.md"));

    assert_eq!(w.fs.bytes("/Docs/Copies/copy.md"), Some(cp1251("Привет\r\nЕщё\r\n")));
    assert_eq!(w.fs.bytes("/Docs/legacy.md"), Some(cp1251("Привет\r\n")));
    assert_eq!(w.session.editing_path(), some("/Docs/Copies/copy.md"));
    assert_eq!(w.session.title(), "copy");
    assert!(!w.session.is_dirty());
    assert_eq!(w.watcher.watched_path(), some("/Docs/Copies/copy.md"));
    assert_eq!(w.registry.owning(&World::canonical("/Docs/Copies/copy.md")), Some(w.window_id));
    assert_eq!(w.registry.owning(&World::canonical("/Docs/legacy.md")), None);
}

#[test]
fn a_failed_save_as_leaves_the_session_where_it_was_with_its_error() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.fs.set_next_write_failure(Some(io::Error::other("the disk is full")));

    assert!(!w.session.save_as("/Docs/elsewhere.md"));

    assert_eq!(w.session.editing_path(), some(DOCUMENT));
    assert_eq!(w.session.save_error_text().as_deref(), Some("the disk is full"));
}

#[test]
fn rename_retargets_everything_and_re_decides_the_identity() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.fs.move_file(DOCUMENT, "/Docs/Renamed.md").unwrap();

    w.session.retarget("/Docs/Renamed.md");

    assert_eq!(w.session.editing_path(), some("/Docs/Renamed.md"));
    assert_eq!(w.session.title(), "Renamed");
    assert_eq!(w.watcher.watched_path(), some("/Docs/Renamed.md"));
    assert_eq!(w.registry.owning(&World::canonical("/Docs/Renamed.md")), Some(w.window_id));
    assert_eq!(w.identities(), [some(DOCUMENT), some("/Docs/Renamed.md")]);
}

#[test]
fn revert_goes_back_to_the_last_explicit_save_and_replaces_the_editor() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("first change\n");
    w.session.flush_now(true);
    w.session.edit("second change\n");
    w.scheduler.advance(TextFileSession::AUTOSAVE_DELAY); // the autosave put the second one on disk

    w.session.revert_to_saved();

    assert_eq!(w.session.text(), "first change\n");
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("first change\n"));
    assert!(!w.session.is_dirty());
    assert_eq!(w.replacements().last().map(String::as_str), Some("first change\n"));
}

#[test]
fn a_deleted_file_keeps_a_document_windows_buffer_and_path_and_marks_it_edited() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.fs.delete(DOCUMENT).unwrap();

    w.watcher.raise_deleted(None);

    assert_eq!(w.session.editing_path(), some(DOCUMENT));
    assert_eq!(w.session.text(), "# Title\nBody\n");
    assert!(w.session.is_dirty());
    assert!(!w.session.conflicted());

    // …and a save recreates it, with no stale stamp in the way.
    w.fs.add_directory("/Docs");
    assert!(w.session.flush_now(true));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("# Title\nBody\n"));
}

#[test]
fn a_read_only_file_opens_normally_and_only_the_autosave_fails() {
    let w = doc();
    w.fs.set_read_only(DOCUMENT, true);

    assert!(w.session.open(DOCUMENT));
    w.session.edit("try me\n");
    w.scheduler.advance(TextFileSession::AUTOSAVE_DELAY);

    assert!(w.session.save_error_text().is_some());
    assert!(w.session.is_dirty());
    assert!(w.session.has_unsaved_changes());
    assert!(w.alerts().is_empty()); // an error bar, never a modal
}

#[test]
fn a_conflict_suspends_the_autosave_until_the_writer_chooses() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("mine\n");
    w.fs.write_externally(DOCUMENT, "theirs, and longer\n");

    w.watcher.raise_changed(None);
    assert!(w.session.conflicted());
    assert_eq!(w.scheduler.pending_timers(), 0);

    w.session.edit("mine, more\n");
    assert_eq!(w.scheduler.pending_timers(), 0); // still suspended
    w.scheduler.advance(Duration::from_secs(5));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("theirs, and longer\n"));
}

#[test]
fn a_document_window_reloading_a_conflict_keeps_its_window() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("mine\n");
    w.fs.write_externally(DOCUMENT, "theirs, and longer\n");
    w.watcher.raise_changed(None);

    w.session.resolve_conflict_reloading();

    assert_eq!(w.session.editing_path(), some(DOCUMENT)); // not Stage::Empty: NSDocument keeps the window
    assert_eq!(w.session.text(), "theirs, and longer\n");
    assert!(!w.session.conflicted());
    assert!(!w.session.is_dirty());
}

#[test]
fn reloading_when_the_file_is_gone_keeps_the_text_and_lets_a_save_recreate_it() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("mine\n");
    w.fs.delete(DOCUMENT).unwrap();
    w.watcher.raise_deleted(None);
    assert!(w.session.conflicted());

    w.session.resolve_conflict_reloading();

    assert_eq!(w.session.text(), "mine\n");
    assert!(!w.session.conflicted());
    w.fs.add_directory("/Docs");
    assert!(w.session.flush_now(true));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("mine\n"));
}

#[test]
fn a_document_window_never_hands_off_to_itself() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("mine\n");

    w.session.recheck_ownership();

    assert!(matches!(w.session.stage(), Stage::Editing(_)));
    assert!(w.fs.writes().is_empty());
}

#[test]
fn an_example_opens_untitled_and_edited_so_closing_offers_to_save() {
    let w = doc();
    w.session.open_untitled("# Welcome\r\n", Some(&strings::untitled_numbered(2)), true);

    assert_eq!(w.session.stage(), Stage::Untitled);
    assert_eq!(w.session.title(), "Untitled 2");
    assert_eq!(w.session.text(), "# Welcome\n"); // the model is LF even for an example
    assert!(w.session.is_dirty());
    assert_eq!(w.session.dressing(), TextFileDressing::DEFAULT);
}

#[test]
fn editing_is_ignored_while_another_window_owns_the_file() {
    let w = book();
    w.registry.register(new_window(), &World::canonical(ARTICLE));
    w.session.select(Some(ARTICLE));

    w.session.edit("cannot type here");

    assert!(matches!(w.session.stage(), Stage::Handoff(_)));
    assert_eq!(w.session.text(), "");
    assert!(!w.session.is_dirty());
}

#[test]
fn disposing_unsubscribes_from_the_watcher_and_releases_the_registry_entry() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("pending\n");
    w.session.dispose();

    assert_eq!(w.registry.owning(&World::canonical(DOCUMENT)), None);
    assert_eq!(w.scheduler.pending_timers(), 0);
    w.fs.write_externally(DOCUMENT, "theirs\n");
    w.watcher.raise_changed(None);
    assert!(!w.session.conflicted()); // no longer listening
}

// ---- Linux additions ----

/// LINUX: `GFileMonitor` (`WATCH_MOVES`) reports another editor's atomic save — write a temporary,
/// rename it over ours — as `Renamed` with the temporary as the old name and ours as the new one.
/// That is a change to our file, never a re-target: following it would take the new stamp without
/// reading the new bytes, and the next autosave would overwrite the other editor's save.
#[test]
fn a_rename_onto_the_edited_file_is_a_change_never_a_retarget() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.fs.add_file("/Docs/.Chapter One.md.tmp", "saved by another editor\n");
    w.fs.delete(DOCUMENT).unwrap();
    w.fs.move_file("/Docs/.Chapter One.md.tmp", DOCUMENT).unwrap();

    w.watcher.raise_renamed(DOCUMENT, Some("/Docs/.Chapter One.md.tmp"));

    assert_eq!(w.session.editing_path(), some(DOCUMENT));
    assert_eq!(w.session.text(), "saved by another editor\n"); // clean: silently reloaded
    assert_eq!(w.session.disk_stamp(), w.fs.stamp(DOCUMENT));
    assert_eq!(w.identities(), [some(DOCUMENT)]); // not re-decided: the identity did not change

    // Dirty, the same event is a conflict, and nothing is written over the other editor's bytes.
    w.session.edit("mine\n");
    w.fs.add_file("/Docs/.Chapter One.md.tmp", "saved again, and longer\n");
    w.fs.delete(DOCUMENT).unwrap();
    w.fs.move_file("/Docs/.Chapter One.md.tmp", DOCUMENT).unwrap();
    w.watcher.raise_renamed(DOCUMENT, Some("/Docs/.Chapter One.md.tmp"));
    assert!(w.session.conflicted());
    w.scheduler.advance(Duration::from_secs(5));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("saved again, and longer\n"));
}
