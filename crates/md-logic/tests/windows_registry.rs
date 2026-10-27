//! Port of md.win `tests/Md.App.Logic.Tests/WindowRegistryTests.cs`.
//!
//! Ownership and the Window menu's rows (shell-design.md §8.6, §2.8), including the one place the
//! real registry deliberately differs from `FakeDocumentRegistry`: a session detaching from its file
//! is not a window closing. md.win's paths compare ignoring case; these compare exactly, and the
//! two rows that pinned the Windows rule pin the Linux one.

use std::cell::Cell;
use std::rc::Rc;

use md_logic::testing::*;
use md_logic::windows::WindowRegistry;

const A: WindowId = WindowId(1);
const B: WindowId = WindowId(2);

fn row(window: WindowId, title: &str) -> (WindowId, String) {
    (window, title.to_string())
}

#[test]
fn a_window_joins_the_menu_before_it_has_any_file() {
    let registry = WindowRegistry::new();
    registry.add(A, "Untitled");

    assert_eq!(registry.windows(), [row(A, "Untitled")]);
    assert_eq!(registry.path_of(A), None);
}

#[test]
fn rows_are_in_creation_order() {
    let registry = WindowRegistry::new();
    registry.add(A, "Untitled");
    registry.add(B, "Untitled 2");
    registry.set_title(A, "a");

    assert_eq!(registry.windows(), [row(A, "a"), row(B, "Untitled 2")]);
}

/// md.win's `OwnershipIsCanonicalPathToWindowAndIsCaseInsensitive`. LINUX: case-SENSITIVE — on ext4
/// `/docs/A.MD` is another file, and claiming it would let two windows write different files as if
/// they were one.
#[test]
fn ownership_is_canonical_path_to_window_and_is_case_insensitive() {
    let registry = WindowRegistry::new();
    registry.add(A, "a");
    registry.register(A, "/Docs/a.md");

    assert_eq!(registry.owning("/Docs/a.md"), Some(A));
    assert_eq!(registry.owning("/docs/A.MD"), None);
    assert_eq!(registry.owning("/Docs/b.md"), None);
    assert_eq!(registry.path_of(A).as_deref(), Some("/Docs/a.md"));
}

#[test]
fn a_window_owns_at_most_one_path_so_a_save_as_moves_the_ownership() {
    let registry = WindowRegistry::new();
    registry.add(A, "a");
    registry.register(A, "/Docs/a.md");
    registry.register(A, "/Docs/b.md");

    assert_eq!(registry.owning("/Docs/a.md"), None);
    assert_eq!(registry.owning("/Docs/b.md"), Some(A));
}

#[test]
fn unregistering_drops_the_file_but_keeps_the_window_in_the_menu() {
    // TextFileSession::detach unregisters; the window is still open and must still be listed.
    let registry = WindowRegistry::new();
    registry.add(A, "a");
    registry.register(A, "/Docs/a.md");
    registry.unregister(A);

    assert_eq!(registry.owning("/Docs/a.md"), None);
    assert_eq!(registry.windows(), [row(A, "a")]);
}

#[test]
fn removing_takes_the_row_and_the_ownership_together() {
    let registry = WindowRegistry::new();
    registry.add(A, "a");
    registry.register(A, "/Docs/a.md");
    registry.remove(A);

    assert_eq!(registry.owning("/Docs/a.md"), None);
    assert!(registry.windows().is_empty());
}

#[test]
fn find_by_path_canonicalises_first() {
    let registry = WindowRegistry::new();
    let identity = FakeFileIdentity::new();
    identity.alias("/Docs/LINK.md", "/Docs/a.md");
    registry.add(A, "a");
    registry.register(A, &identity.canonical("/Docs/a.md"));

    assert_eq!(registry.find_by_path("/Docs/LINK.md", &identity), Some(A));
    assert_eq!(registry.find_by_path("/Docs/./a.md", &identity), Some(A));
    assert_eq!(registry.find_by_path("/Docs/other.md", &identity), None);
    assert_eq!(registry.find_by_path("", &identity), None);
}

#[test]
fn changed_fires_on_every_real_mutation_and_never_for_a_write_of_what_is_already_there() {
    let registry = WindowRegistry::new();
    let changes = Rc::new(Cell::new(0));
    {
        let changes = changes.clone();
        registry.changed().connect(move |_| changes.set(changes.get() + 1));
    }

    registry.add(A, "a"); // 1
    registry.add(A, "a"); // no change
    registry.set_title(A, "a"); // no change
    registry.set_title(A, "a \u{2014} Edited"); // 2
    registry.register(A, "/Docs/a.md"); // 3
    registry.register(A, "/Docs/a.md"); // same file, same window: no change (md.win: "c:\docs\a.md")
    registry.unregister(A); // 4
    registry.unregister(A); // nothing left to drop
    registry.remove(A); // 5
    registry.remove(A); // gone already

    assert_eq!(changes.get(), 5);
}

/// md.win's `ACaseOnlyRenameKeepsOwnershipButNotTheNewSpelling` pins a .NET limitation: after a
/// case-only rename `PathOf` still answered with the old spelling. LINUX: a case-only rename is a
/// rename like any other — the ownership moves to the new name, the old one claims nothing, and
/// `path_of` is the new spelling. The limitation does not exist here.
#[test]
fn a_case_only_rename_keeps_ownership_but_not_the_new_spelling() {
    let registry = WindowRegistry::new();
    registry.add(A, "Notes");
    registry.register(A, "/Docs/Notes.md");
    registry.register(A, "/Docs/NOTES.MD"); // Rename… changed only the casing

    assert_eq!(registry.owning("/Docs/NOTES.MD"), Some(A));
    assert_eq!(registry.owning("/Docs/Notes.md"), None);
    assert_eq!(registry.windows().len(), 1);
    assert_eq!(registry.path_of(A).as_deref(), Some("/Docs/NOTES.MD"));
}

#[test]
fn a_title_for_a_window_that_is_not_open_is_ignored() {
    let registry = WindowRegistry::new();
    registry.set_title(A, "ghost");
    assert!(registry.windows().is_empty());
}

/// A window whose session registers before the window manager added its row still gets one, in
/// creation order (md.win's `Register` comment; no test of its own there).
#[test]
fn a_window_that_registers_first_gets_a_row() {
    let registry = WindowRegistry::new();
    registry.register(A, "/Docs/a.md");
    registry.add(B, "b");

    assert_eq!(registry.windows(), [row(A, ""), row(B, "b")]);
    registry.set_title(A, "a");
    assert_eq!(registry.windows(), [row(A, "a"), row(B, "b")]);
}
