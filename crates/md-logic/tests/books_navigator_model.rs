//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookNavigatorModelTests.cs`.
//!
//! The workspace's selection rules (§8.5, book.md §13.7) against a real book folder: what a click, a
//! rename, a move, a delete and a failed save each do to the selection, what gets remembered, and
//! the handshake that keeps a document window and the book pane from both writing one file.
//!
//! md.win runs this class in a "BookArticleOpens" collection because `OpenInWindow` marks a
//! process-wide static. Here each navigator shares its own `BookArticleOpens` with the test, so the
//! tests run in parallel and need no `Reset()`.

mod common;

use std::cell::Cell;
use std::fs;
use std::rc::Rc;

use common::book_fixture::{BookFixture, Navigator, file_name};
use md_core::book::book_paths;
use md_logic::settings::settings_keys;
use md_logic::strings::{books, documents};
use md_logic::testing::*;

fn small_book() -> BookFixture {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    fixture.scene("02-One/01-a.md");
    fixture.scene("02-One/02-b.md");
    fixture.scene("03-Two/01-c.md");
    fixture
}

fn with_last_article(fixture: &BookFixture, last: &str) -> Navigator {
    let settings = Rc::new(FakeSettingsStore::new());
    settings.set_string(settings_keys::BOOK_LAST_ARTICLE, last);
    let n = Navigator::with_settings(settings);
    n.model.open_book(fixture.root());
    n
}

// MARK: Opening and remembering

#[test]
fn opening_a_book_selects_the_first_article_and_hands_it_to_the_session() {
    let fixture = small_book();

    let n = Navigator::open(&fixture);

    assert_eq!(n.selected_name().as_deref(), Some("01-Preface.md"));
    assert_eq!(n.session.editing(), n.selection());
    assert_eq!(n.session.selected(), [n.selection()]);
}

#[test]
fn the_last_article_written_in_is_remembered_relative_to_the_root_with_forward_slashes() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.select(Some(&fixture.at(&["02-One", "02-b.md"])));

    assert_eq!(n.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).as_deref(), Some("02-One/02-b.md"));
    assert!(!n.model.relative_path(&n.selection().unwrap()).contains('\\'));
}

#[test]
fn a_remembered_article_wins_over_the_first_one_when_the_book_is_reopened() {
    let fixture = small_book();

    let n = with_last_article(&fixture, "02-One/02-b.md");

    assert_eq!(n.selected_name().as_deref(), Some("02-b.md"));
}

#[test]
fn a_remembered_article_that_is_gone_falls_back_to_the_first_one() {
    let fixture = small_book();

    let n = with_last_article(&fixture, "02-One/99-vanished.md");

    assert_eq!(n.selected_name().as_deref(), Some("01-Preface.md"));
}

#[test]
fn a_remembered_path_is_never_followed_out_of_the_book() {
    let fixture = small_book();

    let n = with_last_article(&fixture, "../elsewhere.md");

    assert_eq!(n.selected_name().as_deref(), Some("01-Preface.md"));
}

#[test]
fn an_empty_book_selects_nothing_and_the_session_is_left_empty() {
    let fixture = BookFixture::new();
    fixture.chapter("01-Empty");

    let n = Navigator::open(&fixture);

    assert_eq!(n.selection(), None);
    assert_eq!(n.session.editing(), None);
    assert!(!n.settings.was_written(settings_keys::BOOK_LAST_ARTICLE));
}

#[test]
fn closing_the_book_deselects_and_forgets_the_listing() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.close_book();

    assert_eq!(n.selection(), None);
    assert_eq!(n.model.book(), None);
    assert_eq!(n.model.root(), None);
    assert_eq!(n.session.editing(), None);
    // A detach, not a deselect: the final save is reported, never silently abandoned.
    assert_eq!(n.session.detaches(), [true]);
}

#[test]
fn closing_a_book_lets_go_even_when_the_final_save_fails() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.session.flush_fails.set(true);
    n.model.close_book();

    assert_eq!(n.model.book(), None);
    assert_eq!(n.session.editing(), None);
    assert_eq!(n.session.detaches(), [true]);
}

// MARK: A failed flush

#[test]
fn a_failed_flush_leaves_the_selection_on_the_article_that_will_not_save() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let stuck = n.selection();

    n.session.flush_fails.set(true);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    assert_eq!(n.selection(), stuck);
    assert_eq!(n.session.editing(), stuck);
}

#[test]
fn a_failed_flush_aborts_a_whole_managed_operation_before_anything_is_touched() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let before = fixture.names();

    n.session.flush_fails.set(true);
    let ran = Cell::new(false);
    let performed = n.model.perform_managed(|previous| {
        ran.set(true);
        previous.map(str::to_string)
    });

    assert!(!performed);
    assert!(!ran.get());
    assert_eq!(fixture.names(), before);
}

#[test]
fn a_failed_flush_keeps_a_delete_from_happening() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.session.flush_fails.set(true);
    n.model.perform_delete(&fixture.at(&["01-Preface.md"]));

    assert!(fs::metadata(fixture.at(&["01-Preface.md"])).is_ok());
}

// MARK: Creating

#[test]
fn a_new_article_is_created_seeded_and_selected() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.create_article("  Scene Two  ", &fixture.at(&["02-One"]));

    let created = fixture.at(&["02-One", "Scene Two.md"]);
    assert_eq!(fixture.text(&created), "# Scene Two\n");
    assert_eq!(n.selection(), Some(book_paths::standardize(&created)));
}

#[test]
fn an_empty_or_whitespace_only_name_creates_nothing() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let before = fixture.names();

    n.model.create_article("   ", fixture.root());
    n.model.create_chapter(" ");

    assert_eq!(fixture.names(), before);
}

#[test]
fn a_new_chapter_appears_without_disturbing_the_selection() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let selected = n.selection();
    let selections = n.session.selected().len();

    n.model.create_chapter("04-Three");

    assert!(fs::metadata(fixture.at(&["04-Three"])).is_ok_and(|m| m.is_dir()));
    assert_eq!(n.selection(), selected);
    assert_eq!(n.session.selected().len(), selections); // a folder creation never detaches the editor
}

// MARK: Renaming

#[test]
fn renaming_an_article_keeps_its_number_and_extension_and_the_selection_follows() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    n.model.perform_rename(&fixture.at(&["02-One", "01-a.md"]), "Arrival");

    assert_eq!(fixture.names_in(&fixture.at(&["02-One"])), ["01-Arrival.md", "02-b.md"]);
    assert_eq!(n.selected_name().as_deref(), Some("01-Arrival.md"));
}

#[test]
fn renaming_a_chapter_carries_the_selected_article_with_it() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "02-b.md"])));

    n.model.perform_rename(&fixture.at(&["02-One"]), "Beginnings");

    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["02-Beginnings", "02-b.md"]))));
}

#[test]
fn an_unusable_name_is_refused_with_the_rename_alert_and_changes_nothing() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let before = fixture.names();

    n.model.perform_rename(&fixture.at(&["01-Preface.md"]), "a/b");

    assert_eq!(fixture.names(), before);
    assert_eq!(n.alerts(), [(documents::COULD_NOT_RENAME.to_string(), documents::INVALID_NAME_MESSAGE.to_string())]);
}

#[test]
fn a_colliding_rename_is_reported_with_the_file_systems_own_message() {
    // Two articles under the same ordering number: renaming one onto the other's name is the
    // collision the Mac surfaces through the OS error, not through a check of its own.
    let fixture = BookFixture::new();
    fixture.scene("01-a.md");
    fixture.scene("01-b.md");
    let n = Navigator::open(&fixture);

    n.model.perform_rename(&fixture.at(&["01-a.md"]), "b");

    assert_eq!(fixture.names(), ["01-a.md", "01-b.md"]);
    let alerts = n.alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, documents::COULD_NOT_RENAME);
    assert!(!alerts[0].1.is_empty());
}

#[test]
fn a_rename_to_the_name_it_already_has_changes_nothing_and_says_nothing() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let selected = n.selection();

    n.model.perform_rename(&fixture.at(&["01-Preface.md"]), "Preface");

    assert!(n.alerts().is_empty());
    assert!(fixture.names().contains(&"01-Preface.md".to_string()));
    assert_eq!(n.selection(), selected);
}

// MARK: Moving

#[test]
fn moving_down_swaps_the_numbers_and_the_selection_follows_its_article() {
    let fixture = BookFixture::new();
    fixture.scene("01-a.md");
    fixture.scene("02-b.md");
    fixture.scene("03-c.md");
    let n = Navigator::open(&fixture);
    assert_eq!(n.selected_name().as_deref(), Some("01-a.md"));

    n.model.move_by(&fixture.at(&["01-a.md"]), 1);

    assert_eq!(fixture.names(), ["01-b.md", "02-a.md", "03-c.md"]);
    assert_eq!(n.selected_name().as_deref(), Some("02-a.md"));
}

#[test]
fn moving_up_numbers_every_unprefixed_sibling_on_the_way() {
    let fixture = BookFixture::new();
    fixture.scene("1. intro.md");
    fixture.scene("notes.md");
    fixture.scene("02-end.md");
    let n = Navigator::open(&fixture);

    // Reading order is 1. intro, 02-end, notes → move "notes" (index 2) up one.
    n.model.move_by(&fixture.at(&["notes.md"]), -1);

    assert_eq!(fixture.names(), ["01-intro.md", "02-notes.md", "03-end.md"]);
}

#[test]
fn moving_a_chapter_renumbers_the_chapters_and_keeps_the_article_selected() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["03-Two", "01-c.md"])));

    n.model.move_by(&fixture.at(&["03-Two"]), -1);

    let names = fixture.names();
    assert!(names.contains(&"01-Two".to_string()));
    assert!(names.contains(&"02-One".to_string()));
    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["01-Two", "01-c.md"]))));
}

#[test]
fn a_move_off_the_end_of_the_group_does_nothing() {
    let fixture = BookFixture::new();
    fixture.scene("01-a.md");
    fixture.scene("02-b.md");
    let n = Navigator::open(&fixture);
    let before = fixture.names();

    n.model.move_by(&fixture.at(&["01-a.md"]), -1);

    assert_eq!(fixture.names(), before);
    assert_eq!(n.selected_name().as_deref(), Some("01-a.md"));
}

// MARK: Deleting

#[test]
fn deleting_the_selected_article_falls_to_the_next_one_in_reading_order() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    n.model.perform_delete(&fixture.at(&["02-One", "01-a.md"]));

    assert!(fs::metadata(fixture.at(&["02-One", "01-a.md"])).is_err());
    assert_eq!(n.selected_name().as_deref(), Some("02-b.md"));
}

#[test]
fn deleting_the_last_article_falls_back_to_the_one_before_it() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["03-Two", "01-c.md"])));

    n.model.perform_delete(&fixture.at(&["03-Two", "01-c.md"]));

    assert_eq!(n.selected_name().as_deref(), Some("02-b.md"));
}

#[test]
fn deleting_the_chapter_the_selection_lives_in_falls_to_the_first_survivor_after_it() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "02-b.md"])));

    n.model.perform_delete(&fixture.at(&["02-One"]));

    assert!(fs::metadata(fixture.at(&["02-One"])).is_err());
    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["03-Two", "01-c.md"]))));
}

#[test]
fn deleting_something_else_leaves_the_selection_alone() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));
    let selected = n.selection();

    n.model.perform_delete(&fixture.at(&["01-Preface.md"]));

    assert_eq!(n.selection(), selected);
}

#[test]
fn deleting_the_only_article_leaves_nothing_selected() {
    let fixture = BookFixture::new();
    fixture.scene("01-a.md");
    let n = Navigator::open(&fixture);

    n.model.perform_delete(&fixture.at(&["01-a.md"]));

    assert_eq!(n.selection(), None);
    assert_eq!(n.session.editing(), None);
}

#[test]
fn deleting_something_already_gone_is_reported_and_nothing_moves() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.perform_delete(&fixture.at(&["99-never.md"]));

    let alerts = n.alerts();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].0, books::COULD_NOT_DELETE);
    assert_eq!(n.selected_name().as_deref(), Some("01-Preface.md"));
}

// MARK: Re-listing

#[test]
fn a_reload_re_points_the_selection_at_the_listings_own_spelling_of_the_path() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    // The same file, spelled with a dot segment: the reload hands back the listing's path.
    n.model.select(Some(&format!("{}/./02-One/01-a.md", fixture.root())));
    n.model.reload(None);

    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["02-One", "01-a.md"]))));
}

#[test]
fn a_reload_prefers_the_path_it_is_given_when_that_still_exists() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.reload(Some(&fixture.at(&["03-Two", "01-c.md"])));

    assert_eq!(n.selected_name().as_deref(), Some("01-c.md"));
}

#[test]
fn a_folder_that_cannot_be_listed_shows_as_no_book() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.reload(None);
    assert!(n.model.book().is_some());

    let gone = Navigator::with_settings(Rc::new(FakeSettingsStore::new()));
    gone.model.open_book(&fixture.at(&["no-such-folder"]));

    assert_eq!(gone.model.book(), None);
    assert_eq!(gone.selection(), None);
}

// MARK: Separate windows

#[test]
fn while_articles_open_in_their_own_windows_the_sidebar_holds_no_selection() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    n.model.set_opens_in_separate_windows(true);

    assert_eq!(n.selection(), None);

    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    assert_eq!(n.selection(), None);
    let opened = n.opened();
    assert_eq!(opened.len(), 1);
    assert_eq!(file_name(&opened[0]), "01-a.md");
}

#[test]
fn opening_the_article_being_edited_hands_the_file_over_before_the_window_is_asked_for() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    assert!(n.model.open_in_window(&fixture.at(&["02-One", "01-a.md"])));

    assert_eq!(n.session.hand_offs.get(), 1);
    assert_eq!(n.session.editing(), None);
    assert_eq!(n.opened().len(), 1);
    assert!(n.opens.borrow_mut().claim_open(&fixture.at(&["02-One", "01-a.md"])));
}

#[test]
fn opening_an_article_that_is_not_being_edited_needs_no_handoff() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);

    assert!(n.model.open_in_window(&fixture.at(&["03-Two", "01-c.md"])));

    assert_eq!(n.session.hand_offs.get(), 0);
    assert_eq!(n.opened().len(), 1);
}

#[test]
fn a_failed_handoff_abandons_the_open_so_two_writers_never_exist() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let selected = n.selection().unwrap();

    n.session.flush_fails.set(true);
    assert!(!n.model.open_in_window(&selected));

    assert!(n.opened().is_empty());
    assert_eq!(n.session.hand_offs.get(), 1);
    assert!(!n.opens.borrow_mut().claim_open(&selected));
}

// MARK: The jump reset

#[test]
fn every_selection_change_announces_itself_so_one_shot_jumps_are_cleared() {
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let announcements = Rc::new(Cell::new(0));
    {
        let announcements = announcements.clone();
        n.model.selection_changing().connect(move |_| announcements.set(announcements.get() + 1));
    }

    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));
    n.model.select(None);

    assert_eq!(announcements.get(), 2);
}

#[test]
fn every_change_is_announced_after_the_state_is_settled() {
    // LINUX (Rust shape): the model holds no borrow while it raises `changed`, so a handler may read
    // it back — the window re-renders from inside the event.
    let fixture = small_book();
    let n = Navigator::open(&fixture);
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let model = Rc::new(n);
    {
        let (seen, reader) = (seen.clone(), Rc::downgrade(&model));
        model.model.changed().connect(move |_| {
            if let Some(n) = reader.upgrade() {
                seen.borrow_mut().push(n.model.selection().map(|path| file_name(&path).to_string()));
            }
        });
    }

    model.model.step_next();

    assert_eq!(*seen.borrow(), [Some("01-a.md".to_string())]);
}
