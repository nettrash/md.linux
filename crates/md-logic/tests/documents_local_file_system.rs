//! Port of md.win `tests/Md.App.Logic.Tests/LocalFileSystemTests.cs`.
//!
//! The seam adapters that touch a real disk (§6.2, §6.3): the in-place write whose stamp the
//! session compares, against a scratch folder per test.
//!
//! md.win's file also tests its `FileSystemWatcherAdapter`. That adapter is not ported — the GTK app
//! implements the `FileWatcher` seam over `GFileMonitor`, and those tests belong with it — so its
//! four adapter-only tests are here as ignored placeholders that say what the app's own test must
//! check, and the two that pin the *session's* half of the contract (the echo of our own write, a
//! stranger's file renamed onto ours) run against the real disk with the fake watcher standing in
//! for the monitor's event.

mod common;

use std::io;
use std::os::unix::fs::MetadataExt;
use std::rc::Rc;

use common::document_fixtures::TempDir;
use md_logic::documents::{LocalFileIdentity, LocalFileSystem, SessionRole, TextFileSession};
use md_logic::testing::*;

const GFILEMONITOR: &str = "FileSystemWatcherAdapter is not ported: the GTK app implements FileWatcher over GFileMonitor, \
                            and this check belongs to the app's own tests";

#[test]
fn a_write_in_place_keeps_the_file_and_moves_its_stamp() {
    let root = TempDir::new("md-linux-fs-");
    let fs = LocalFileSystem::INSTANCE;
    let path = root.join(&["a.md"]);
    fs.write_all_bytes_in_place(&path, b"one").unwrap();
    let first = fs.stamp(&path).unwrap();
    assert_eq!(first.size, 3);
    let inode = std::fs::metadata(&path).unwrap().ino();

    // A longer, then a shorter body: the size half of the stamp is what catches a rewrite a coarse
    // time stamp would hide.
    fs.write_all_bytes_in_place(&path, b"one two three").unwrap();
    assert_eq!(fs.stamp(&path).unwrap().size, 13);
    fs.write_all_bytes_in_place(&path, b"hi").unwrap();
    assert_eq!(fs.read_all_bytes(&path).unwrap(), b"hi");
    assert_eq!(fs.stamp(&path).unwrap().size, 2); // truncated, not overwritten in place
    // LINUX: in place means the same inode — not a new file renamed over the old one.
    assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
}

#[test]
fn stamp_is_null_for_a_missing_file_which_is_what_makes_a_vanished_file_stale() {
    let root = TempDir::new("md-linux-fs-");
    let fs = LocalFileSystem::INSTANCE;
    assert_eq!(fs.stamp(&root.join(&["never.md"])), None);
    assert!(!fs.file_exists(&root.join(&["never.md"])));
    // A folder is not a file and has no stamp.
    assert_eq!(fs.stamp(&root.path), None);
    assert!(!fs.file_exists(&root.path));
    assert!(fs.directory_exists(&root.path));
}

#[test]
fn move_delete_and_enumerate_cover_folders_as_well_as_files() {
    let root = TempDir::new("md-linux-fs-");
    let fs = LocalFileSystem::INSTANCE;
    std::fs::create_dir(root.join(&["bundle"])).unwrap();
    fs.write_all_bytes_in_place(&root.join(&["bundle", "text.md"]), b"x").unwrap();

    fs.move_file(&root.join(&["bundle"]), &root.join(&["moved"])).unwrap();
    assert!(fs.directory_exists(&root.join(&["moved"])));
    assert!(!fs.directory_exists(&root.join(&["bundle"])));
    assert!(fs.enumerate_entries(&root.join(&["moved"])).unwrap().contains(&root.join(&["moved", "text.md"])));

    fs.delete(&root.join(&["moved"])).unwrap();
    assert!(!fs.directory_exists(&root.join(&["moved"])));
    fs.delete(&root.join(&["never.md"])).unwrap(); // deleting nothing is not an error
}

/// LINUX: `rename(2)` replaces an existing file (and an empty folder) without a word; md.win's
/// `File.Move(…, overwrite: false)` and `Directory.Move` refuse, and so must this — a Rename… or
/// Move To… onto an existing name would otherwise destroy that file.
#[test]
fn move_refuses_an_existing_destination_because_rename_would_replace_it() {
    let root = TempDir::new("md-linux-fs-");
    let fs = LocalFileSystem::INSTANCE;
    let a = root.write("a.md", "mine");
    let b = root.write("b.md", "someone else's");
    std::fs::create_dir(root.join(&["empty"])).unwrap();
    std::fs::create_dir(root.join(&["full"])).unwrap();
    std::os::unix::fs::symlink(root.join(&["nowhere"]), root.join(&["dangling"])).unwrap();

    for target in [b.clone(), root.join(&["empty"]), root.join(&["dangling"])] {
        let error = fs.move_file(&a, &target).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{target}");
        assert!(error.to_string().contains(&target), "{error}");
    }
    let error = fs.move_file(&root.join(&["full"]), &root.join(&["empty"])).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);

    assert_eq!(std::fs::read_to_string(&a).unwrap(), "mine");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "someone else's");

    // A free name moves, and the stamp travels with the file (a rename keeps the mtime).
    let before = fs.stamp(&a).unwrap();
    fs.move_file(&a, &root.join(&["c.md"])).unwrap();
    assert_eq!(fs.stamp(&root.join(&["c.md"])), Some(before));
    assert!(!fs.file_exists(&a));
}

/// LINUX: every error names the file it is about, as .NET's messages (which the save-error bar
/// shows) do.
#[test]
fn errors_name_the_path_they_are_about() {
    let root = TempDir::new("md-linux-fs-");
    let fs = LocalFileSystem::INSTANCE;
    let missing = root.join(&["no-folder", "a.md"]);
    let error = fs.write_all_bytes_in_place(&missing, b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().starts_with(&format!("{missing}: ")), "{error}");
    let error = fs.read_all_bytes(&missing).unwrap_err();
    assert!(error.to_string().starts_with(&format!("{missing}: ")), "{error}");
}

#[test]
fn ensure_directory_creates_the_session_folder_on_a_first_run() {
    let root = TempDir::new("md-linux-fs-");
    let folder = root.join(&["share", "me.nettrash.md"]);
    LocalFileSystem::INSTANCE.ensure_directory(&folder).unwrap();
    assert!(LocalFileSystem::INSTANCE.directory_exists(&folder));
    LocalFileSystem::INSTANCE.ensure_directory(&folder).unwrap(); // and again is fine
}

#[test]
#[ignore = "FileSystemWatcherAdapter is not ported: the GTK app implements FileWatcher over GFileMonitor; \
            its test must show an external write arriving as `changed` on the main context"]
fn the_watcher_reports_an_external_write_through_the_ui_thread() {
    panic!("{GFILEMONITOR}");
}

#[test]
#[ignore = "FileSystemWatcherAdapter is not ported: the GTK app implements FileWatcher over GFileMonitor; \
            its test must show stop() silences the monitor and a watch after teardown is refused"]
fn a_stopped_or_disposed_watcher_is_silent() {
    panic!("{GFILEMONITOR}");
}

#[test]
#[ignore = "FileSystemWatcherAdapter is not ported: the GTK app implements FileWatcher over GFileMonitor; \
            its test must show a watch on a bare name or a missing folder is a no-op, not a crash"]
fn a_watch_on_a_path_with_no_folder_is_a_no_op_rather_than_a_crash() {
    panic!("{GFILEMONITOR}");
}

#[test]
#[ignore = "FileSystemWatcherAdapter is not ported: the GTK app implements FileWatcher over GFileMonitor; \
            its test must show a file created at the watched name arrives as `changed`"]
fn a_created_file_is_reported_as_a_change_so_a_delete_then_create_save_is_not_missed() {
    panic!("{GFILEMONITOR}");
}

/// The end-to-end version of the guard: a real in-place write, the monitor's echo of it, and a
/// session that must recognise the echo as its own rather than raise a conflict over its own bytes.
/// Everything here is the shipping code except the watcher (whose event the test raises where
/// `GFileMonitor` would), the registry and the scheduler.
#[test]
fn our_own_write_echoes_back_through_the_watcher_and_the_stamp_filters_it() {
    let root = TempDir::new("md-linux-fs-");
    let watcher = Rc::new(FakeFileWatcher::new());
    let scheduler = Rc::new(FakeScheduler::new());
    let session = TextFileSession::new(
        Rc::new(LocalFileSystem),
        watcher.clone(),
        scheduler.clone(),
        Rc::new(FakeDocumentRegistry::new()),
        Rc::new(LocalFileIdentity),
        SessionRole::Document,
        Some(WindowId(1)),
    );

    let path = root.write("echo.md", "# Start\n");
    assert!(session.open(&path));

    session.edit("# Start\nand more, which is longer\n");
    scheduler.advance(TextFileSession::AUTOSAVE_DELAY);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Start\nand more, which is longer\n");

    // The monitor reports our own write; the handler runs on the UI thread, after the autosave set
    // the stamp — the only place it can run.
    assert!(watcher.raise_changed(None));
    assert!(!session.conflicted());
    assert!(!session.has_unsaved_changes());

    // A real external write, by contrast, is seen.
    session.edit("# Start\nmine\n");
    std::fs::write(&path, "somebody else entirely, at another length\n").unwrap();
    watcher.raise_changed(None);
    assert!(session.conflicted());
    assert_eq!(session.text(), "# Start\nmine\n");
}

/// md.win's adapter turns "something was renamed onto our name" into a change before the session
/// sees it. `GFileMonitor` with `WATCH_MOVES` reports that as `Renamed` (the stranger's file as the
/// old name, ours as the new), so the session itself must never re-target on it: following it
/// would silently point the next save at bytes the writer never opened. Real disk, real move.
#[test]
fn a_strangers_file_renamed_onto_our_name_is_a_change_and_never_re_targets_the_session() {
    let root = TempDir::new("md-linux-fs-");
    let watcher = Rc::new(FakeFileWatcher::new());
    let scheduler = Rc::new(FakeScheduler::new());
    let session = TextFileSession::new(
        Rc::new(LocalFileSystem),
        watcher.clone(),
        scheduler,
        Rc::new(FakeDocumentRegistry::new()),
        Rc::new(LocalFileIdentity),
        SessionRole::Document,
        Some(WindowId(2)),
    );
    let ours = root.write("ours.md", "ours");
    let stranger = root.write("stranger.md", "not ours, and longer");
    assert!(session.open(&ours));

    LocalFileSystem.delete(&ours).unwrap();
    LocalFileSystem.move_file(&stranger, &ours).unwrap(); // lands on the watched name
    watcher.raise_renamed(&ours, Some(&stranger));

    assert_eq!(session.editing_path(), Some(ours.clone())); // never "our file moved to X"
    assert_eq!(session.text(), "not ours, and longer"); // a clean session follows the disk
    assert_eq!(session.disk_stamp(), LocalFileSystem.stamp(&ours));
}
