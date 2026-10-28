//! Port of md.win `tests/Md.App.Logic.Tests/WindowTitleTests.cs`.
//!
//! The title and the "— Edited" proxy (shell-design.md §6.7), and the per-process untitled
//! numbering.

use md_logic::strings;
use md_logic::windows::UntitledNames;
use md_logic::windows::window_title;

#[test]
fn a_saved_document_is_its_name_and_a_dirty_one_carries_the_em_dash_edited() {
    assert_eq!(window_title::for_document("Notes", false), "Notes");
    assert_eq!(window_title::for_document("Notes", true), "Notes \u{2014} Edited");
}

#[test]
fn the_suffix_is_the_macs_bytes() {
    // U+2014 EM DASH with a space either side; the same constant strings publishes.
    assert_eq!(window_title::EDITED_SUFFIX, " \u{2014} Edited");
    assert_eq!(window_title::EDITED_SUFFIX, strings::EDITED_SUFFIX);
}

#[test]
fn an_example_shows_untitled_edited() {
    // §6.7: an Example opens untitled and dirty.
    assert_eq!(window_title::for_document(&window_title::untitled(1), true), "Untitled \u{2014} Edited");
}

#[test]
fn untitled_windows_are_numbered_from_the_second() {
    for (ordinal, expected) in [(1, "Untitled"), (2, "Untitled 2"), (11, "Untitled 11")] {
        assert_eq!(window_title::untitled(ordinal), expected);
    }
}

#[test]
fn the_book_window_is_the_books_name_or_book_and_appends_the_article_being_edited() {
    assert_eq!(window_title::for_book(None, None), "Book");
    assert_eq!(window_title::for_book(Some(""), Some("")), "Book");
    assert_eq!(window_title::for_book(Some("My Book"), None), "My Book");
    assert_eq!(window_title::for_book(Some("My Book"), Some("Preface")), "My Book \u{2014} Preface");
}

#[test]
fn the_lowest_free_untitled_number_is_handed_out() {
    let mut names = UntitledNames::new();
    let (name, first) = names.take_name();
    assert_eq!(name, "Untitled");
    let (name, second) = names.take_name();
    assert_eq!(name, "Untitled 2");
    assert_eq!(names.take_name().0, "Untitled 3");

    names.release(second);
    assert_eq!(names.take_name().0, "Untitled 2");

    names.release(first);
    assert_eq!(names.take_name().0, "Untitled");
}
