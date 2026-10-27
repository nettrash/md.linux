//! Port of md.win `tests/Md.App.Logic.Tests/SettingsKeysTests.cs`: `SettingsKeysTests` (§9's
//! eleven keys, their defaults and the other names md stores state under) and
//! `InMemorySettingsStoreTests` (the store semantics the app's store must mirror).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use md_core::text::unicode;
use md_logic::settings::settings_keys::{self, ALL};
use md_logic::settings::{InMemorySettingsStore, SettingsStore};

/// The module's own source: the constants and the §9 table in its doc comment.
const SOURCE: &str = include_str!("../src/settings/settings_keys.rs");

#[test]
fn the_eleven_keys_are_pinned_in_section_9_order() {
    assert_eq!(
        ALL,
        [
            "md.viewModeMemory",
            "md.pdfPageSize",
            "md.bookBookmark",
            "md.bookOpensInSeparateWindows",
            "md.bookLastArticle",
            "md.bookViewMode",
            "md.linux.windowSize.document",
            "md.linux.windowSize.book",
            "md.continueLists",
            "md.capitalizeSentences",
            "md.wrapSelection",
        ]
    );
}

#[test]
fn each_constant_spells_its_key() {
    assert_eq!(settings_keys::VIEW_MODE_MEMORY, "md.viewModeMemory");
    assert_eq!(settings_keys::PDF_PAGE_SIZE, "md.pdfPageSize");
    assert_eq!(settings_keys::BOOK_BOOKMARK, "md.bookBookmark");
    assert_eq!(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, "md.bookOpensInSeparateWindows");
    assert_eq!(settings_keys::BOOK_LAST_ARTICLE, "md.bookLastArticle");
    assert_eq!(settings_keys::BOOK_VIEW_MODE, "md.bookViewMode");
    // md.win: "md.win.windowSize.document" / "md.win.windowSize.book".
    assert_eq!(settings_keys::DOCUMENT_WINDOW_SIZE, "md.linux.windowSize.document");
    assert_eq!(settings_keys::BOOK_WINDOW_SIZE, "md.linux.windowSize.book");
    // smart-typing.md §3.1: the same two keys on every md port.
    assert_eq!(settings_keys::CONTINUE_LISTS, "md.continueLists");
    assert_eq!(settings_keys::CAPITALIZE_SENTENCES, "md.capitalizeSentences");
    assert_eq!(settings_keys::WRAP_SELECTION, "md.wrapSelection");
}

#[test]
fn keys_are_distinct_namespaced_and_free_of_whitespace() {
    let mut distinct = ALL.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), ALL.len());
    for key in ALL {
        assert!(key.starts_with("md."), "{key}");
        assert!(!key.chars().any(unicode::is_dotnet_white_space), "{key}");
    }
}

#[test]
fn only_the_two_window_sizes_are_linux_only() {
    // md.win: Only_the_two_window_sizes_are_Windows_only.
    let linux: Vec<&str> = ALL.iter().copied().filter(|k| k.starts_with("md.linux.")).collect();
    assert_eq!(linux, [settings_keys::DOCUMENT_WINDOW_SIZE, settings_keys::BOOK_WINDOW_SIZE]);
    assert!(ALL.iter().all(|k| !k.starts_with("md.win.")), "a Windows-only key leaked into the Linux list");
}

/// Every `pub const NAME: &str = "md.…";` in the module source (rustfmt keeps each on one line).
fn key_constants_in_source() -> Vec<(String, String)> {
    SOURCE
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("pub const ")?;
            let (name, rest) = rest.split_once(": &str = \"")?;
            let value = rest.strip_suffix("\";")?;
            Some((name.to_string(), value.to_string()))
        })
        .filter(|(_, value)| value.starts_with("md."))
        .collect()
}

#[test]
fn every_key_constant_is_listed_in_all() {
    // A key added as a constant but forgotten in ALL would escape PRIVACY's "exactly these" list.
    // md.win reflects over the class; here the module's source is the list of constants.
    let mut constants: Vec<String> = key_constants_in_source().into_iter().map(|(_, value)| value).collect();
    constants.sort();
    let mut all: Vec<String> = ALL.iter().map(|k| k.to_string()).collect();
    all.sort();
    assert_eq!(constants, all);
}

#[test]
fn the_module_table_enumerates_every_key() {
    // md.win: The_design_section_9_enumerates_every_key reads docs/shell-design.md §9, which
    // md.linux does not have; the §9 table lives in settings_keys.rs's own doc comment instead,
    // and it is what PRIVACY.md is written from.
    let rows: Vec<&str> = SOURCE.lines().filter(|line| line.starts_with("//! | `md.")).collect();
    for key in ALL {
        assert!(rows.iter().any(|row| row.starts_with(&format!("//! | `{key}`"))), "{key} is missing from the table");
    }
    assert_eq!(rows.len(), ALL.len(), "the table has a row for a key ALL does not list");
}

#[test]
#[ignore = "md.linux has no PRIVACY.md yet: un-ignore when it lands"]
fn privacy_enumerates_every_key_and_the_session_file() {
    // §9: PRIVACY.md must enumerate exactly the keys and the session file (plus whatever the GTK
    // app adds to settings_keys' "other names").
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../PRIVACY.md");
    let privacy = std::fs::read_to_string(&path).expect("PRIVACY.md at the repository root");
    for key in ALL {
        assert!(privacy.contains(key), "{key}");
    }
    assert!(privacy.contains(settings_keys::SESSION_FILE_NAME));
}

#[test]
fn defaults_are_the_design_values() {
    assert_eq!(settings_keys::PDF_PAGE_SIZE_DEFAULT, "a4");
    assert_eq!(settings_keys::BOOK_VIEW_MODE_DEFAULT, "split");
    assert_eq!(settings_keys::DOCUMENT_WINDOW_SIZE_DEFAULT, "900x640");
    assert_eq!(settings_keys::BOOK_WINDOW_SIZE_DEFAULT, "1000x700");
    // smart-typing.md §3.1: both bool, both default true.
    const { assert!(settings_keys::CONTINUE_LISTS_DEFAULT) };
    const { assert!(settings_keys::CAPITALIZE_SENTENCES_DEFAULT) };
    const { assert!(settings_keys::WRAP_SELECTION_DEFAULT) };
}

#[test]
fn the_typing_keys_are_not_linux_only() {
    // md.win: The_typing_keys_are_not_Windows_only — every port stores them under this spelling;
    // a platform prefix here would split the family.
    for key in [settings_keys::CONTINUE_LISTS, settings_keys::CAPITALIZE_SENTENCES, settings_keys::WRAP_SELECTION] {
        assert!(key.starts_with("md."), "{key}");
        assert!(!key.contains(".linux."), "{key}");
        assert!(!key.contains(".win."), "{key}");
    }
}

#[test]
fn the_other_names_md_stores_state_under_are_pinned_and_are_not_settings_keys() {
    // md.win also pins BookAccessToken ("md.book", a FutureAccessList token) and the WebView2
    // folder name; neither exists on Linux (see settings_keys).
    assert_eq!(settings_keys::SESSION_FILE_NAME, "session.json");
    assert!(!ALL.contains(&settings_keys::SESSION_FILE_NAME));
    let names: Vec<String> = key_constants_in_source().into_iter().map(|(name, _)| name).collect();
    assert!(!names.iter().any(|n| n == "BOOK_ACCESS_TOKEN"));
}

// ---------------------------------------------------------------------------------------------
// InMemorySettingsStore — the app's store mirrors these semantics.

#[test]
fn absent_reads_as_none_or_the_fallback() {
    let s = InMemorySettingsStore::new();
    assert_eq!(s.get_string(settings_keys::PDF_PAGE_SIZE), None);
    assert!(!s.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, false));
    assert!(s.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, true));
    assert!(s.keys().is_empty());
}

#[test]
fn strings_and_bools_round_trip() {
    let s = InMemorySettingsStore::new();
    s.set_string(settings_keys::PDF_PAGE_SIZE, "letter");
    s.set_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, true);
    assert_eq!(s.get_string(settings_keys::PDF_PAGE_SIZE).as_deref(), Some("letter"));
    assert!(s.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, false));
    assert_eq!(s.keys().len(), 2);
}

#[test]
fn a_value_of_the_other_type_reads_as_absent() {
    let s = InMemorySettingsStore::new();
    s.set_string("k", "true");
    s.set_bool("b", true);
    assert!(!s.get_bool("k", false));
    assert!(s.get_bool("k", true));
    assert_eq!(s.get_string("b"), None);
}

#[test]
fn changed_fires_only_when_the_stored_value_changes() {
    let s = InMemorySettingsStore::new();
    let changes = Rc::new(RefCell::new(Vec::<String>::new()));
    {
        let changes = changes.clone();
        s.changed().connect(move |key| changes.borrow_mut().push(key.to_string()));
    }

    s.set_string("k", "a");
    s.set_string("k", "a"); // same value: silent, so a menu re-writing what it observed cannot loop
    s.set_string("k", "b");
    s.set_bool("x", false);
    s.set_bool("x", false);
    s.remove("k");
    s.remove("k"); // already gone: silent
    s.remove("never");

    assert_eq!(*changes.borrow(), ["k", "k", "x", "k"]);
    assert_eq!(s.get_string("k"), None);
    assert_eq!(s.keys(), ["x"]);
}

#[test]
fn replacing_a_bool_with_a_string_under_one_key_is_a_change() {
    let s = InMemorySettingsStore::new();
    let changes = Rc::new(Cell::new(0));
    {
        let changes = changes.clone();
        s.changed().connect(move |_| changes.set(changes.get() + 1));
    }
    s.set_bool("k", true);
    s.set_string("k", "True");
    assert_eq!(changes.get(), 2);
    assert_eq!(s.get_string("k").as_deref(), Some("True"));
    assert!(!s.get_bool("k", false));
}

// md.win's SetString_rejects_null cannot arise: a `&str` is never null.

#[test]
fn keys_keep_the_order_they_were_first_stored_in() {
    let s = InMemorySettingsStore::new();
    s.set_string("b", "1");
    s.set_string("a", "1");
    s.set_string("b", "2"); // a rewrite keeps its place
    s.remove("a");
    s.set_bool("c", true);
    assert_eq!(s.keys(), ["b", "c"]);
}
