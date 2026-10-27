//! Port of md.win `tests/Md.App.Logic.Tests/FileNamesTests.cs`.
//!
//! §6.4's name rule and the path arithmetic the whole package rests on. md.win pins Windows' rules
//! on every OS (both separators, case folded, Win32's name rule); these pin the Linux decisions in
//! `documents::file_names` — `/` only, byte-for-byte names, the Mac's name rule — and each row whose
//! answer changed says so.

use md_logic::documents::file_names;
use md_logic::strings;

#[test]
fn a_valid_name_is_accepted() {
    for name in ["Chapter One", "notes.md", ".hidden", "a b", "CONSOLE", "COM10"] {
        assert!(file_names::validate(Some(name)), "{name:?}");
    }
    // LINUX: every one of these is refused by Win32 and is an ordinary name on Linux (and the
    // Mac): a backslash, Windows' other punctuation, a trailing dot or space, a device name, a
    // control character.
    for name in [
        "a\\b",
        "a*b",
        "a?b",
        "a\"b",
        "a<b",
        "a>b",
        "a|b",
        "trailing.",
        "trailing ",
        "CON",
        "con.md",
        "LPT9",
        "nul",
        "a\tb",
    ] {
        assert!(file_names::validate(Some(name)), "{name:?}");
    }
}

#[test]
fn an_invalid_name_is_rejected() {
    assert!(!file_names::validate(None));
    for name in ["", "   ", "a/b", "a:b"] {
        assert!(!file_names::validate(Some(name)), "{name:?}");
    }
    // LINUX: what the file system itself refuses — NUL, and the two names every folder holds.
    for name in ["a\0b", ".", ".."] {
        assert!(!file_names::validate(Some(name)), "{name:?}");
    }
}

#[test]
fn every_documented_invalid_character_is_in_the_message_the_dialog_shows() {
    // The message is fixed verbatim; if the rule and the wording ever drift apart, this fails.
    for c in file_names::INVALID_CHARACTERS.chars() {
        assert!(strings::documents::INVALID_NAME_MESSAGE.contains(c), "{c:?}");
        assert!(!file_names::validate(Some(&format!("a{c}b"))), "{c:?}");
    }
}

/// md.win's `ForWindowsMakesAnyTitleSafeToHandAPicker`, for `for_linux`.
#[test]
fn for_linux_makes_any_title_safe_to_hand_a_picker() {
    let rows = [
        (Some("a/b:c"), "a-b-c"),
        (Some(""), "Document"),
        (Some("   "), "Document"),
        (Some("Notes"), "Notes"),
        (None, "Document"),
        // LINUX: md.win gives "trailing", "_CON" and "Document"; on Linux these are ordinary names.
        (Some("trailing. "), "trailing. "),
        (Some("CON"), "CON"),
        (Some("..."), "..."),
        // LINUX: what Linux cannot hold.
        (Some("a\0b"), "a-b"),
        (Some(".."), "Document"),
    ];
    for (input, expected) in rows {
        assert_eq!(file_names::for_linux(input), expected, "{input:?}");
    }
}

#[test]
fn rename_keeps_the_extension() {
    let rows = [
        ("Scene.md", "Act One", "Act One.md"),
        ("Scene.markdown", "Act One", "Act One.markdown"),
        ("README", "Notes", "Notes"), // no extension to keep
        ("notes.", "Other", "Other"), // Foundation: a trailing dot is not an extension
        (".hidden", "Other", "Other"),
    ];
    for (original, stem, expected) in rows {
        assert_eq!(file_names::with_extension_of(original, stem), expected, "{original:?}");
    }
}

/// md.win's `NameAndDirectorySplitOnBothSeparatorsOnEveryOs`. LINUX: `/` is the only separator,
/// so a Windows path is one name.
#[test]
fn name_and_directory_split_on_both_separators_on_every_os() {
    let rows = [
        ("/Docs/a.md", "a.md", "/Docs"),
        ("/tmp/a.md", "a.md", "/tmp"),
        ("/a.md", "a.md", "/"),
        ("a.md", "a.md", ""),
        ("/Docs/", "Docs", "/"),
        ("/Docs/sub/", "sub", "/Docs"),
        ("C:\\Docs\\a.md", "C:\\Docs\\a.md", ""),
        ("/share/back\\slash.md", "back\\slash.md", "/share"),
    ];
    for (path, name, directory) in rows {
        assert_eq!(file_names::name_of(path), name, "{path:?}");
        assert_eq!(file_names::directory_of(path), directory, "{path:?}");
    }
}

#[test]
fn stem_and_extension_follow_foundations_two_disagreeing_splits() {
    let rows = [
        ("/Docs/01-Scene.md", "01-Scene", ".md"),
        ("/Docs/README", "README", ""),
        ("/Docs/notes.", "notes.", ""), // Foundation keeps the whole name
        ("/Docs/.hidden", ".hidden", ""),
        ("/Docs/a.b c", "a", ""), // Foundation's two splits disagree: the stem drops it, the extension is empty
    ];
    for (path, stem, extension) in rows {
        assert_eq!(file_names::stem_of(path), stem, "{path:?}");
        assert_eq!(file_names::extension_of(path), extension, "{path:?}");
    }
}

#[test]
fn combine_keeps_the_separator_the_directory_already_uses() {
    let rows =
        [("/Docs", "a.md", "/Docs/a.md"), ("/", "a.md", "/a.md"), ("/tmp/", "a.md", "/tmp/a.md"), ("", "a.md", "a.md")];
    for (directory, name, expected) in rows {
        assert_eq!(file_names::combine(directory, name), expected, "{directory:?}");
    }
}

#[test]
fn standardize_collapses_dot_segments_without_asking_the_host() {
    let rows = [
        ("/Docs/../Docs/./a.md", "/Docs/a.md"),
        ("/Docs/sub/../../a.md", "/a.md"),
        ("/../../a.md", "/a.md"), // ".." at the root is the root
        ("/tmp/./a.md", "/tmp/a.md"),
        ("/Docs/", "/Docs"),
        ("//Docs//a.md", "/Docs/a.md"),
        ("relative/path", "relative/path"), // no working directory is invented
    ];
    for (path, expected) in rows {
        assert_eq!(file_names::standardize(path), expected, "{path:?}");
    }
}

/// md.win's `SamePathIsWindowsRuleEverywhere`: LINUX — case matters, `\` is a character, and only
/// spellings that collapse to one path are one file.
#[test]
fn same_path_is_windows_rule_everywhere() {
    let rows = [
        ("/Docs/a.md", "/Docs/a.md", true),
        ("/Docs/a.md", "/Docs//a.md", true),
        ("/Docs/sub/../a.md", "/Docs/a.md", true),
        ("/Docs/a.md", "/docs/A.MD", false), // two files on ext4
        ("/Docs/a.md", "/Docs\\a.md", false),
        ("/Docs/a.md", "/Docs2/a.md", false),
        ("/Docs/a.md", "/Docs/a.md/b", false),
        ("a.md", "/a.md", false), // a relative spelling is not the root's file
    ];
    for (a, b, same) in rows {
        assert_eq!(file_names::same_path(a, b), same, "{a:?} {b:?}");
    }
}

/// LINUX: a leading `/` is the only way a reference escapes relativity; md.win's drive and UNC
/// heads are relative names here.
#[test]
fn is_rooted_catches_every_way_a_reference_escapes_relativity() {
    let rows = [
        ("/x", true),
        ("//server/share", true),
        ("photo.png", false),
        ("images/photo.png", false),
        ("C:photo.png", false),
        ("C:\\x", false),
        ("C:/x", false),
        ("\\x", false),
        ("\\\\server\\share", false),
    ];
    for (reference, rooted) in rows {
        assert_eq!(file_names::is_rooted(reference), rooted, "{reference:?}");
    }
}

#[test]
fn resolve_relative_joins_and_collapses() {
    let rows = [
        ("/Docs", "photo.png", "/Docs/photo.png"),
        ("/Docs", "images/photo.png", "/Docs/images/photo.png"),
        ("/Docs", "./photo.png", "/Docs/photo.png"),
        ("/Docs/sub", "../photo.png", "/Docs/photo.png"),
        ("/tmp/docs", "photo.png", "/tmp/docs/photo.png"),
        ("/Docs", "images\\photo.png", "/Docs/images\\photo.png"), // LINUX: one name
    ];
    for (directory, relative, expected) in rows {
        assert_eq!(file_names::resolve_relative(directory, relative).as_deref(), Some(expected), "{relative:?}");
    }
}

#[test]
fn resolve_relative_refuses_anything_that_is_not_beside_the_document() {
    let rows = [
        ("/Docs", "/Windows/x.png"), // rooted
        ("/Docs", "/x.png"),
        ("/Docs", "../../../x.png"), // climbs past the root
        ("/Docs", ""),
    ];
    for (directory, relative) in rows {
        assert_eq!(file_names::resolve_relative(directory, relative), None, "{relative:?}");
    }
}
