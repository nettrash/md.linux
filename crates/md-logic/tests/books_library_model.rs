//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookLibraryModelTests.cs`: the open-book
//! setting and the example book's name (§8.2).

use std::collections::HashSet;
use std::rc::Rc;

use md_logic::books::BookLibraryModel;
use md_logic::settings::settings_keys;
use md_logic::testing::*;

#[test]
fn no_setting_and_an_empty_setting_both_mean_no_book() {
    let settings = Rc::new(FakeSettingsStore::new());
    let library = BookLibraryModel::new(settings.clone());

    assert_eq!(library.book_path(), None);
    assert!(!library.has_book());

    settings.set_string(settings_keys::BOOK_BOOKMARK, "");
    assert_eq!(library.book_path(), None);
    assert!(!library.has_book());
}

#[test]
fn opening_and_closing_a_book_is_one_key() {
    let settings = Rc::new(FakeSettingsStore::new());
    let library = BookLibraryModel::new(settings.clone());

    library.store("/home/writer/Books/My Book");
    assert_eq!(library.book_path().as_deref(), Some("/home/writer/Books/My Book"));
    assert!(library.has_book());

    library.close_book();
    assert_eq!(library.book_path(), None);
    assert_eq!(settings.removals(), [settings_keys::BOOK_BOOKMARK]);
}

#[test]
fn the_example_book_keeps_its_name_when_the_folder_is_free() {
    assert_eq!(
        BookLibraryModel::deduped_name("Example Book", |_| false, BookLibraryModel::DEFAULT_MAX_ATTEMPTS).as_deref(),
        Some("Example Book")
    );
}

#[test]
fn an_occupied_name_counts_up_and_never_says_two_1() {
    let taken: HashSet<&str> = HashSet::from(["Example Book", "Example Book 2"]);

    assert_eq!(
        BookLibraryModel::deduped_name(
            "Example Book",
            |name| taken.contains(name),
            BookLibraryModel::DEFAULT_MAX_ATTEMPTS
        )
        .as_deref(),
        Some("Example Book 3")
    );
    assert!(!taken.contains("Example Book 1"));
}

#[test]
fn a_folder_full_of_example_books_gives_up_rather_than_loop() {
    assert_eq!(BookLibraryModel::deduped_name("Example Book", |_| true, 5), None);
}
