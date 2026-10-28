//! Port of md.win `tests/Md.App.Logic.Tests/FileIdentityTests.cs`.
//!
//! §5.2's canonical path. md.win's facts about `GetFinalPathNameByHandle` only hold on Windows
//! and no-op elsewhere; here every row is the Linux decision (`realpath`, no case folding), and
//! runs. The two rows that depend on the file system's case rule skip themselves on a
//! case-insensitive one (APFS, where the suite also runs during development), as md.win's skip off
//! NTFS.

mod common;

use common::document_fixtures::TempDir;
use md_logic::documents::LocalFileIdentity;
use md_logic::seams::FileIdentity;

/// md.win's `TheExtendedLengthPrefixIsStrippedSoAPickersPathMatches`. LINUX: there is no `\\?\`
/// prefix; what must hold is the same promise — the identity of a picker's plain, resolved path is
/// that path, byte for byte.
#[test]
fn the_extended_length_prefix_is_stripped_so_a_pickers_path_matches() {
    let directory = TempDir::new("md-linux-identity-");
    let file = directory.write("a.md", "x");
    assert_eq!(LocalFileIdentity.canonical(&file), file);
    assert_eq!(LocalFileIdentity::lexical(&file), file);
}

#[test]
fn a_dot_segment_path_and_its_plain_spelling_are_one_identity() {
    let directory = TempDir::new("md-linux-identity-");
    let file = directory.write("a.md", "x");
    std::fs::create_dir(directory.join(&["sub"])).unwrap();

    let plain = LocalFileIdentity.canonical(&file);
    let roundabout = LocalFileIdentity.canonical(&directory.join(&["sub", "..", "a.md"]));
    let doubled = LocalFileIdentity.canonical(&format!("{}//a.md", directory.path));

    assert_eq!(plain, roundabout);
    assert_eq!(plain, doubled);
}

#[test]
fn a_missing_path_still_has_a_stable_identity() {
    let missing = std::env::temp_dir().join("md-linux-tests").join("never-created.md");
    let missing = missing.to_str().unwrap();
    let canonical = LocalFileIdentity.canonical(missing);

    assert_eq!(LocalFileIdentity.canonical(missing), canonical);
    assert_eq!(LocalFileIdentity::lexical(missing), canonical);
    assert!(!canonical.ends_with('/'));
}

#[test]
fn an_empty_path_is_returned_unchanged_rather_than_throwing() {
    assert_eq!(LocalFileIdentity.canonical(""), "");
}

/// Whether this file system folds case (it does not on Linux; APFS does by default).
fn folds_case(directory: &TempDir) -> bool {
    std::path::Path::new(&directory.join(&["CHAPTER.MD"])).exists()
}

/// md.win's `TwoCaseSpellingsOfOneFileAreOneIdentityOnWindows`. LINUX: they are two files, and two
/// identities.
#[test]
fn two_case_spellings_of_one_file_are_one_identity_on_windows() {
    let directory = TempDir::new("md-linux-identity-");
    let file = directory.write("Chapter.md", "x");
    if folds_case(&directory) {
        eprintln!("skipped: {} folds case; on Linux the two spellings are two files", directory.path);
        return;
    }
    assert_ne!(LocalFileIdentity.canonical(&file), LocalFileIdentity.canonical(&directory.join(&["CHAPTER.MD"])));
}

/// md.win's `TheCanonicalPathIsTheFilesOwnCaseOnWindowsNotTheCallersOnWindows`. LINUX: no case is
/// ever invented — the spelling asked about is the spelling answered (it names another, missing
/// file).
#[test]
fn the_canonical_path_is_the_files_own_case_on_windows_not_the_callers_on_windows() {
    let directory = TempDir::new("md-linux-identity-");
    directory.write("Chapter.md", "x");
    if folds_case(&directory) {
        eprintln!("skipped: {} folds case", directory.path);
        return;
    }
    assert!(LocalFileIdentity.canonical(&directory.join(&["CHAPTER.MD"])).ends_with("/CHAPTER.MD"));
}

/// md.win's `AFileReachedThroughALinkedFolderHasTheTargetsIdentityOnWindows` — on Linux a symlink
/// needs no privilege, so this always runs.
#[test]
fn a_file_reached_through_a_linked_folder_has_the_targets_identity_on_windows() {
    let directory = TempDir::new("md-linux-identity-");
    let file = directory.write("real/a.md", "x");
    std::os::unix::fs::symlink(directory.join(&["real"]), directory.join(&["link"])).unwrap();

    assert_eq!(LocalFileIdentity.canonical(&file), LocalFileIdentity.canonical(&directory.join(&["link", "a.md"])));
    assert_eq!(LocalFileIdentity.canonical(&directory.join(&["link", "a.md"])), file);
}
