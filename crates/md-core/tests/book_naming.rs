//! Port of md.win `BookNamingTests`: the ordering-prefix grammar (mdTests:
//! testRenamedNameKeepsPrefixAndExtension, testDisplayNameStripsPrefixAndExtension) plus the Kotlin
//! BookOrderTest vectors for the same surface, and the two names where Kotlin contradicts Swift —
//! pinned to the Swift answer so the decision is visible.

use md_core::book::book_naming::{display_name, is_article_name, renamed_name, split_extension, split_prefix};
use md_core::book::{book_order, renumber_plan};

#[test]
fn display_name_strips_prefix_and_extension() {
    assert_eq!("Preface", display_name("01-Preface.md"));
    assert_eq!("Getting Started", display_name("02-Getting Started"));
    // An all-number name has nothing else to show — it stays whole.
    assert_eq!("01", display_name("01.md"));
}

#[test]
fn renamed_name_keeps_prefix_and_extension() {
    assert_eq!("01-Final.md", renamed_name("01-Draft.md", "Final"));
    // A loose prefix is preserved as written — only a Move normalises it.
    assert_eq!("2. config.markdown", renamed_name("2. setup.markdown", "config"));
    assert_eq!("journal.md", renamed_name("notes.md", "journal"));
    assert_eq!("02-Basics", renamed_name("02-Getting Started", "Basics"));
}

#[test]
fn kotlin_editable_names_agree() {
    assert_eq!("The Storm", display_name("02-The Storm.md"));
    assert_eq!("notes", display_name("notes.txt"));
    assert_eq!("chapter.one", display_name("01-chapter.one.md"));
    // A chapter folder is no article: its dot is part of the name.
    assert_eq!("v1.2", display_name("v1.2"));
}

#[test]
fn kotlin_renames_agree() {
    assert_eq!("02-New.md", renamed_name("02-Old.md", "New"));
    assert_eq!("ideas.txt", renamed_name("notes.txt", "ideas"));
    assert_eq!("03-Finale", renamed_name("03-Part", "Finale"));
    assert_eq!("Ideas", renamed_name("Drafts", "Ideas"));
}

#[test]
fn article_names_are_the_three_extensions_in_any_case() {
    assert!(is_article_name("intro.md"));
    assert!(is_article_name("intro.markdown"));
    assert!(is_article_name("notes.txt"));
    assert!(is_article_name("SHOUTY.MD"));
    assert!(!is_article_name("cover.png"));
    assert!(!is_article_name("draft.md.bak"));
    assert!(!is_article_name("no-extension"));
    // Swift: a dot at position 0 is not an extension separator.
    assert!(!is_article_name(".md"));
}

#[test]
fn split_extension_keeps_case_and_leaves_folder_dots_alone() {
    assert_eq!(("Notes", "MD"), split_extension("Notes.MD"));
    assert_eq!("Notes-2.MD", renamed_name("Notes.MD", "Notes-2"));
    assert_eq!(("v1.2", ""), split_extension("v1.2"));
    assert_eq!(("Notes.bak", ""), split_extension("Notes.bak"));
    assert_eq!(("notes.", ""), split_extension("notes."));
    assert_eq!(("", ""), split_extension(""));
}

#[test]
fn split_prefix_follows_the_swift_grammar() {
    assert_eq!(("01-", "intro"), split_prefix("01-intro"));
    assert_eq!(("2. ", "setup"), split_prefix("2. setup"));
    assert_eq!(("1) ", "intro"), split_prefix("1) intro"));
    assert_eq!(("4 ", "finale"), split_prefix("4 finale"));
    assert_eq!(("01-.) ", "odd"), split_prefix("01-.) odd"));
    // No digits, or nothing left after them: no prefix.
    assert_eq!(("", "appendix"), split_prefix("appendix"));
    assert_eq!(("", "2026"), split_prefix("2026"));
    assert_eq!(("", "01-"), split_prefix("01-"));
    assert_eq!(("", ""), split_prefix(""));
    // ASCII digits only — Arabic-Indic three (U+0663) is a letter of the name.
    assert_eq!(("", "\u{0663}-arabic"), split_prefix("\u{0663}-arabic"));
}

#[test]
fn split_prefix_sides_with_swift_where_kotlin_differs() {
    // Swift needs no separator: "3rd party" is numbered 3 with stem "rd party" (Kotlin: unnumbered).
    // Swift's separator set has ")" and not "_": "3_draft" is prefix "3", stem "_draft" (Kotlin:
    // prefix "3_").
    assert_eq!(("3", "rd party"), split_prefix("3rd party"));
    assert_eq!(("3", "_draft"), split_prefix("3_draft"));
    assert_eq!("rd party", display_name("3rd party.md"));
    assert_eq!("01-rd party.md", renumber_plan::renumbered_name("3rd party.md", 1));
}

#[test]
fn digit_carrying_a_combining_mark_is_not_an_ordering_digit() {
    // Swift walks graphemes: "1" + U+0301 is one non-ASCII Character, so the digit run is empty;
    // "12" + U+0301 has the run "1".
    assert_eq!(("", "1\u{0301}-x"), split_prefix("1\u{0301}-x"));
    assert_eq!(("1", "2\u{0301}-x"), split_prefix("12\u{0301}-x"));
    assert_eq!(None, book_order::leading_number("1\u{0301}-x"));
    assert_eq!(Some(1), book_order::leading_number("12\u{0301}-x"));
}
