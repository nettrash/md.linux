//! Port of md.win `tests/Md.App.Logic.Tests/DocumentAdversarialTests.cs`.
//!
//! The rows of §6 that the package's own suite left to a constant's own name, and the traps the
//! porting reports call out for this package: a conflicted save, a case-only rename, a bundle that
//! arrives with someone else's line endings, an asset reference that is not a file path at all.
//!
//! Every number here is written as a literal on purpose. A test that asserts
//! `pending_delays == [AUTOSAVE_DELAY]` passes for any delay, and a rescue loop bounded by
//! `rescue_copy::MAX_ATTEMPTS` passes for any cap — both were shown to survive the mutation that
//! changes the value the design fixes.

mod common;

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use common::document_fixtures::{TempDir, notes_pack};
use md_core::document::view_mode_memory;
use md_core::export::text_bundle;
use md_logic::documents::{
    LocalFileIdentity, NewLine, SessionRole, Stage, TextFileSession, asset_reader, document_loader, file_names,
};
use md_logic::testing::*;

const ARTICLE: &str = "/Book/01-Scene.md";
const DOCUMENT: &str = "/Docs/Chapter One.md";

fn new_window() -> WindowId {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    WindowId(NEXT.fetch_add(1, Ordering::Relaxed))
}

struct World {
    fs: Rc<FakeFileSystem>,
    watcher: Rc<FakeFileWatcher>,
    scheduler: Rc<FakeScheduler>,
    registry: Rc<FakeDocumentRegistry>,
    session: Rc<TextFileSession>,
    window_id: WindowId,
    identities: Rc<std::cell::RefCell<Vec<Option<String>>>>,
}

impl World {
    fn new(role: SessionRole) -> World {
        let clock = Rc::new(FakeClock::new());
        let fs = Rc::new(FakeFileSystem::with_clock(clock.clone()));
        let scheduler = Rc::new(FakeScheduler::with_clock(clock));
        let watcher = Rc::new(FakeFileWatcher::new());
        let registry = Rc::new(FakeDocumentRegistry::new());
        let window_id = new_window();
        let session = TextFileSession::new(
            fs.clone(),
            watcher.clone(),
            scheduler.clone(),
            registry.clone(),
            Rc::new(FakeFileIdentity::new()),
            role,
            (role == SessionRole::Document).then_some(window_id),
        );
        let identities = Rc::new(std::cell::RefCell::new(Vec::new()));
        {
            let identities = identities.clone();
            session.identity_changed().connect(move |p: &Option<String>| identities.borrow_mut().push(p.clone()));
        }
        World { fs, watcher, scheduler, registry, session, window_id, identities }
    }

    /// The fake identity's normalisation, so a registry key built here matches the session's.
    fn canonical(path: &str) -> String {
        FakeFileIdentity::normalise(path)
    }
}

fn doc() -> World {
    let world = World::new(SessionRole::Document);
    world.fs.add_file(DOCUMENT, "# Title\nBody\n");
    world
}

fn book() -> World {
    let world = World::new(SessionRole::BookArticle);
    world.fs.add_file(ARTICLE, "# Scene\n");
    world
}

// ---- the debounce the design fixes at 1.0 s (§6.3) ----

#[test]
fn the_autosave_debounce_is_exactly_one_second_not_whatever_the_constant_says() {
    assert_eq!(TextFileSession::AUTOSAVE_DELAY, Duration::from_secs(1));

    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("typed\n");

    // 999 ms after the last keystroke the file is still the one that was opened: the whole point
    // of a debounce is that a fast typist's disk is not written on every character.
    w.scheduler.advance(Duration::from_millis(999));
    assert!(w.fs.writes().is_empty());
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("# Title\nBody\n"));

    w.scheduler.advance(Duration::from_millis(1));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("typed\n"));
}

#[test]
fn every_keystroke_restarts_the_full_second_rather_than_letting_the_first_one_fire() {
    let w = doc();
    w.session.open(DOCUMENT);

    for i in 0..5 {
        w.session.edit(&"x".repeat(i + 1));
        w.scheduler.advance(Duration::from_millis(900));
        assert!(w.fs.writes().is_empty()); // 4.5 s of typing, not one write
    }
    w.scheduler.advance(Duration::from_millis(100));
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("xxxxx"));
}

// ---- "a vanished file counts as stale" (§6.3, autosave-tick row) ----

#[test]
fn a_file_that_vanished_behind_the_session_is_stale_so_no_write_recreates_it_silently() {
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("mine\n");
    w.fs.delete(DOCUMENT).unwrap(); // deleted with no watcher event: a sync client, say

    // Not "there is nothing to clobber, so write": the file the stamp was taken from is gone, and
    // recreating it would resurrect a document the writer may have deliberately deleted.
    assert!(!w.session.flush_now(true));
    assert!(w.session.conflicted());
    assert!(!w.fs.file_exists(DOCUMENT));
    assert!(w.fs.writes().is_empty());

    // The autosave is suspended too, not merely refused once.
    w.session.edit("mine, more\n");
    w.scheduler.advance(Duration::from_secs(5));
    assert!(w.fs.writes().is_empty());

    // Keep My Version is the one route that writes, and it recreates the file.
    w.session.resolve_conflict_keeping_mine();
    assert_eq!(w.fs.text(DOCUMENT).as_deref(), Some("mine, more\n"));
    assert!(!w.session.conflicted());
    assert!(!w.session.is_dirty());
}

#[test]
fn a_conflicted_session_writes_nothing_on_any_route_and_the_close_parks_the_text_instead() {
    let w = book();
    w.session.select(Some(ARTICLE));
    w.session.edit("mine\n");
    w.fs.write_externally(ARTICLE, "theirs, and a different length\n");
    assert!(!w.session.flush_now(false));
    assert!(w.session.conflicted());

    // Neither an explicit Ctrl+S, nor a fresh keystroke's autosave, nor the flush inside a
    // selection change may overwrite what the other writer put there.
    assert!(!w.session.flush_now(true));
    w.session.edit("mine, edited again\n");
    w.scheduler.advance(Duration::from_secs(5));
    assert!(w.fs.writes().is_empty());
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("theirs, and a different length\n"));

    // Closing on top of an unresolved conflict does not discard the keystrokes: they land in the
    // rescue sibling, and the other writer's file is still untouched.
    w.session.close_flush();
    assert_eq!(w.fs.text("/Book/01-Scene (rescued).md").as_deref(), Some("mine, edited again\n"));
    assert_eq!(w.fs.text(ARTICLE).as_deref(), Some("theirs, and a different length\n"));
}

// ---- a case-only rename (§13.4 stage 5; §5.2's "one identity") ----

/// LINUX: on NTFS a case-only rename is one file under a new spelling; on ext4 `chapter one.md`
/// is another name, so the rename is an ordinary one — a real move (which the case-sensitive fake
/// performs) and the watcher's old → new. What must hold is the same: the session follows, one
/// window owns one entry, and the old spelling no longer claims anything.
#[test]
fn a_case_only_rename_is_the_same_file_so_one_window_still_owns_one_entry() {
    let lowered = "/Docs/chapter one.md";
    let w = doc();
    w.session.open(DOCUMENT);
    w.session.edit("edited\n");
    w.session.flush_now(true);

    w.fs.move_file(DOCUMENT, lowered).unwrap();
    w.watcher.raise_renamed(lowered, Some(DOCUMENT));

    assert_eq!(w.session.editing_path().as_deref(), Some(lowered));
    assert_eq!(w.watcher.watched_path().as_deref(), Some(lowered));
    assert!(!file_names::same_path(DOCUMENT, lowered)); // two names on Linux

    // One registry entry, not two: the second spelling must not leave the old one claiming the
    // file, or the next window to open it would think a document already owns it.
    assert_eq!(w.registry.windows().len(), 1);
    assert_eq!(w.registry.owning(&World::canonical(lowered)), Some(w.window_id));
    assert_eq!(w.registry.owning(&World::canonical(DOCUMENT)), None);
    assert_eq!(w.registry.owning("/Docs/Nowhere.md"), None);

    // The view-mode memory is told, so §5.2 re-decides for the new spelling.
    assert!(w.identities.borrow().contains(&Some(lowered.to_string())));

    // And a save after the rename still writes one file, under the new spelling.
    w.session.edit("edited twice\n");
    assert!(w.session.flush_now(true));
    assert_eq!(w.fs.text(lowered).as_deref(), Some("edited twice\n"));
    assert!(!w.fs.file_exists(DOCUMENT));
}

// ---- a bundle brings its own line endings (§6.5) ----

#[test]
fn an_imported_crlf_bundle_still_writes_crlf_when_it_is_first_saved_as() {
    let w = doc();
    w.fs.add_file("/In/Notes.textpack", notes_pack("one\r\ntwo\r\n"));

    assert!(w.session.open("/In/Notes.textpack"));
    assert_eq!(w.session.text(), "one\ntwo\n"); // LF in the model, as always
    assert_eq!(w.session.dressing().new_line, NewLine::CrLf); // the file's own convention survives
    assert_eq!(w.session.stage(), Stage::Untitled);
    assert_eq!(w.session.title(), "Notes");

    w.fs.add_directory("/Out");
    assert!(w.session.save_as("/Out/Notes.md"));
    assert_eq!(w.fs.text("/Out/Notes.md").as_deref(), Some("one\r\ntwo\r\n"));
}

/// LINUX: md.win's name says "just because it lands on Windows"; the rule is the same either way —
/// the bundle's own convention is kept, whatever the platform's.
#[test]
fn an_lf_bundle_is_not_given_windows_line_endings_just_because_it_lands_on_windows() {
    let w = doc();
    w.fs.add_file("/In/Notes.textpack", notes_pack("one\ntwo\n"));
    w.session.open("/In/Notes.textpack");

    assert_eq!(w.session.dressing().new_line, NewLine::Lf);
    w.fs.add_directory("/Out");
    w.session.save_as("/Out/Notes.md");
    assert_eq!(w.fs.text("/Out/Notes.md").as_deref(), Some("one\ntwo\n"));
}

// ---- the identity is the file's own spelling, never a folded one (§5.2) ----

#[test]
fn the_canonical_path_is_never_case_folded_because_that_would_hide_which_spelling_is_real() {
    let root = TempDir::new("md-linux-case-");
    let file = root.write("MixedCase/Chapter One.md", "x");

    // Both legs: the identity and its lexical half. Neither may upper-case the way md-core's
    // canonical_path_with(…, fold_case: true) does — the two are not interchangeable, and folding
    // here would make every title and every bar shout.
    for canonical in [LocalFileIdentity.canonical(&file), LocalFileIdentity::lexical(&file)] {
        assert!(canonical.contains("MixedCase"), "{canonical}");
        assert!(canonical.ends_with("Chapter One.md"), "{canonical}");
        assert!(!canonical.contains("MIXEDCASE"), "{canonical}");
    }

    // md-core's folded form is deliberately a different answer.
    assert_ne!(view_mode_memory::canonical_path_with(&file, true), LocalFileIdentity::lexical(&file));
}

// ---- the picker filter and the packaging are one list (§6.1) ----

/// The Linux packaging (a `.desktop` file and a shared-mime-info package) does not exist yet; the
/// manifest half is ported in `documents_file_association.rs` and ignored until it does. What
/// md-logic owns is checked here: the Open filter offers every Markdown extension, and neither
/// `.dot` (a Word template) nor `.textbundle` (a folder) is in it.
#[test]
fn every_associated_extension_is_also_offered_by_the_open_picker_and_the_two_unclaimed_ones_stay_unclaimed() {
    for extension in document_loader::MARKDOWN_EXTENSIONS {
        assert!(document_loader::OPEN_EXTENSIONS.contains(&extension), "{extension}");
    }
    assert!(!document_loader::OPEN_EXTENSIONS.contains(&".dot"));
    assert!(!document_loader::OPEN_EXTENSIONS.contains(&".textbundle"));
}

// ---- an asset reference is not a URL, and never leaves the folder (§7.7) ----

#[test]
fn an_asset_reference_that_is_not_a_plain_file_beside_the_document_reads_nothing() {
    let references = [
        "javascript:alert(1)",
        "http://example.com/logo.png",
        "https://example.com/logo.png",
        "file:///etc/passwd",
        "data:image/png;base64,AAAA",
        // Windows' rooted spellings are relative names on Linux: only a file of exactly that name
        // inside the folder could match, and there is none.
        "C:\\Windows\\System32\\config\\SAM",
        "\\\\server\\share\\secret.png",
        "..\\..\\Windows\\System32\\config\\SAM",
        "../../etc/passwd",
        "/etc/passwd",
    ];
    for reference in references {
        let fs = Rc::new(FakeFileSystem::new());
        fs.add_file("/Docs/a.md", "# a\n");
        fs.add_file("/Docs/logo.png", "inside");
        fs.add_file("/Windows/System32/config/SAM", "secret");
        fs.add_file("/etc/passwd", "secret");

        let read = asset_reader::beside(Some("/Docs/a.md"), fs, Rc::new(FakeFileIdentity::new()));

        assert_eq!(read(reference), None, "{reference}");
        assert_eq!(read("logo.png"), Some(b"inside".to_vec())); // the control: this one is ours
    }
}

#[test]
fn the_two_containment_guards_cover_disjoint_halves_so_neither_can_be_dropped_quietly() {
    // Which layer refuses what, pinned so a later edit to either one is caught. md-core screens
    // schemes and absolute refs before resolve_asset is ever called; "javascript:" is in neither
    // screen and is refused only because no file of that name is beside the document.
    assert!(!text_bundle::is_local_relative_reference("http://example.com/logo.png"));
    assert!(!text_bundle::is_local_relative_reference("data:image/png;base64,AAAA"));
    assert!(!text_bundle::is_local_relative_reference("/etc/passwd"));

    assert!(text_bundle::is_local_relative_reference("C:\\Windows\\System32\\config\\SAM"));
    assert!(text_bundle::is_local_relative_reference("\\\\server\\share\\secret.png"));
    assert!(text_bundle::is_local_relative_reference("javascript:alert(1)"));

    // LINUX: md.win's own guard refuses the drive and UNC heads (`IsRooted` true); on Linux they
    // are relative names, so this package's rooted test is only the leading '/' md-core already
    // screens — and the containment check below is what keeps them inside the folder.
    assert!(!file_names::is_rooted("C:\\Windows\\System32\\config\\SAM"));
    assert!(!file_names::is_rooted("\\\\server\\share\\secret.png"));
    assert!(file_names::is_rooted("/etc/passwd"));

    // And the ones that survive both screens still read nothing, because the containment check
    // runs after the resolve rather than instead of it.
    let fs = Rc::new(FakeFileSystem::new());
    fs.add_file("/Docs/a.md", "# a\n");
    let read = asset_reader::beside(Some("/Docs/a.md"), fs, Rc::new(FakeFileIdentity::new()));
    assert_eq!(read("javascript:alert(1)"), None);
    assert_eq!(read("C:\\Windows\\System32\\config\\SAM"), None);
}
