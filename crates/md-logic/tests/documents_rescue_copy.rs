//! Port of md.win `tests/Md.App.Logic.Tests/RescueCopyTests.cs`.
//!
//! §6.3's last-resort save: the names, and the promise that a rescue never overwrites anything.

use std::io;

use md_logic::documents::{file_names, rescue_copy};
use md_logic::testing::*;

#[test]
fn rescue_names_follow_the_macs_two_splits() {
    let rows = [
        ("01-Scene.md", 1, "01-Scene (rescued).md"),
        ("01-Scene.md", 2, "01-Scene (rescued 2).md"),
        ("01-Scene.md", 37, "01-Scene (rescued 37).md"),
        ("/Book/01-Scene.md", 1, "01-Scene (rescued).md"),
        ("notes.markdown", 1, "notes (rescued).markdown"),
        ("README", 1, "README (rescued).md"), // no extension: the Mac's ".md" fallback
        (".hidden", 1, ".hidden (rescued).md"), // Foundation: a dotfile has no extension
        ("a.b c", 1, "a (rescued).md"),       // an extension with a space is no extension
    ];
    for (file_name, attempt, expected) in rows {
        assert_eq!(rescue_copy::name_for(file_name, attempt), expected, "{file_name:?} {attempt}");
    }
}

#[test]
fn there_is_never_a_rescued_1() {
    assert!(!rescue_copy::name_for("a.md", 1).contains("(rescued 1)"));
    assert!(rescue_copy::name_for("a.md", 1).contains("(rescued)"));
}

#[test]
fn the_first_free_name_wins_and_the_occupant_is_untouched() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Book/01-Scene.md", "on disk");
    fs.add_file("/Book/01-Scene (rescued).md", "someone else's");

    let written = rescue_copy::write(&fs, "/Book/01-Scene.md", b"my buffer");

    assert_eq!(written.as_deref(), Some("/Book/01-Scene (rescued 2).md"));
    assert_eq!(fs.text(&written.unwrap()).as_deref(), Some("my buffer"));
    assert_eq!(fs.text("/Book/01-Scene (rescued).md").as_deref(), Some("someone else's"));
    assert_eq!(fs.text("/Book/01-Scene.md").as_deref(), Some("on disk"));
}

#[test]
fn a_directory_squatting_on_the_name_counts_as_taken() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Book/01-Scene.md", "on disk");
    fs.add_directory("/Book/01-Scene (rescued).md");

    assert_eq!(
        rescue_copy::write(&fs, "/Book/01-Scene.md", &[1, 2, 3]).as_deref(),
        Some("/Book/01-Scene (rescued 2).md")
    );
}

#[test]
fn a_failing_write_gives_up_at_once_rather_than_trying_a_hundred_names() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Book/01-Scene.md", "on disk");
    fs.set_next_write_failure(Some(io::Error::new(io::ErrorKind::PermissionDenied, "denied")));

    assert_eq!(rescue_copy::write(&fs, "/Book/01-Scene.md", &[1]), None);
    assert_eq!(fs.writes().len(), 1);
}

#[test]
fn a_hundred_taken_names_give_up() {
    // The literal, not MAX_ATTEMPTS: a loop bounded by the constant under test passes for any
    // value of it, and the mutation that lowers the cap to 3 survived that shape.
    assert_eq!(rescue_copy::MAX_ATTEMPTS, 100);

    let fs = FakeFileSystem::new();
    fs.add_file("/Book/a.md", "on disk");
    for attempt in 1..=100 {
        fs.add_file(&file_names::combine("/Book", &rescue_copy::name_for("a.md", attempt)), "taken");
    }

    assert_eq!(rescue_copy::write(&fs, "/Book/a.md", &[1]), None);
    assert!(fs.writes().is_empty());
}

#[test]
fn the_hundredth_name_is_still_tried_rather_than_giving_up_one_early() {
    let fs = FakeFileSystem::new();
    fs.add_file("/Book/a.md", "on disk");
    for attempt in 1..100 {
        fs.add_file(&file_names::combine("/Book", &rescue_copy::name_for("a.md", attempt)), "taken");
    }

    assert_eq!(rescue_copy::write(&fs, "/Book/a.md", &[1]).as_deref(), Some("/Book/a (rescued 100).md"));
}
