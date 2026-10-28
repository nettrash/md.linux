//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookStepperTests.cs`: Previous / Next Article
//! (§8.4–§8.5) — the reading order, entering the book from either end, and the two ends where a step
//! is not possible at all.

mod common;

use common::book_fixture::{BookFixture, file_name};
use md_logic::books::BookStepper;

fn three_articles() -> BookFixture {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    fixture.scene("02-One/01-a.md");
    fixture.scene("02-One/02-b.md");
    fixture.scene("03-Two/01-c.md");
    fixture
}

#[test]
fn the_order_is_root_articles_then_every_chapters_in_order() {
    let fixture = three_articles();

    let stepper = BookStepper::for_book(Some(&fixture.load()), None, false);

    let names: Vec<&str> = stepper.order.iter().map(|path| file_name(path)).collect();
    assert_eq!(names, ["01-Preface.md", "01-a.md", "02-b.md", "01-c.md"]);
}

#[test]
fn with_nothing_selected_next_enters_from_the_front_and_previous_from_the_back() {
    let fixture = three_articles();
    let stepper = BookStepper::for_book(Some(&fixture.load()), None, false);

    assert!(stepper.can_previous());
    assert!(stepper.can_next());
    assert_eq!(stepper.next().map(file_name), Some("01-Preface.md"));
    assert_eq!(stepper.previous().map(file_name), Some("01-c.md"));
}

#[test]
fn steps_walk_the_order_across_chapter_boundaries() {
    let fixture = three_articles();
    let book = fixture.load();
    let last = fixture.at(&["02-One", "02-b.md"]);

    let stepper = BookStepper::for_book(Some(&book), Some(&last), false);

    assert_eq!(stepper.previous().map(file_name), Some("01-a.md")); // back inside the chapter
    assert_eq!(stepper.next().map(file_name), Some("01-c.md")); // forward into the next one
}

#[test]
fn at_the_first_article_previous_is_dead_and_at_the_last_next_is() {
    let fixture = three_articles();
    let book = fixture.load();

    let front = BookStepper::for_book(Some(&book), Some(&fixture.at(&["01-Preface.md"])), false);
    assert!(!front.can_previous());
    assert_eq!(front.previous(), None);
    assert!(front.can_next());

    let back = BookStepper::for_book(Some(&book), Some(&fixture.at(&["03-Two", "01-c.md"])), false);
    assert!(back.can_previous());
    assert!(!back.can_next());
    assert_eq!(back.next(), None);
}

#[test]
fn a_selection_that_is_not_in_the_book_enters_from_the_ends_like_no_selection() {
    let fixture = three_articles();
    let other = BookFixture::new();
    let stranger = other.scene("01-a.md");

    let stepper = BookStepper::for_book(Some(&fixture.load()), Some(&stranger), false);

    assert_eq!(stepper.index, None);
    assert_eq!(stepper.next().map(file_name), Some("01-Preface.md"));
}

#[test]
fn an_empty_book_steps_nowhere() {
    let fixture = BookFixture::new();
    fixture.chapter("01-Empty");

    let stepper = BookStepper::for_book(Some(&fixture.load()), None, false);

    assert!(stepper.order.is_empty());
    assert!(!stepper.can_previous());
    assert!(!stepper.can_next());
    assert_eq!(stepper.previous(), None);
    assert_eq!(stepper.next(), None);
}

#[test]
fn separate_windows_leaves_nothing_to_step_from() {
    let fixture = three_articles();

    let stepper = BookStepper::for_book(Some(&fixture.load()), Some(&fixture.at(&["01-Preface.md"])), true);

    assert!(!stepper.can_previous());
    assert!(!stepper.can_next());
    assert!(stepper.order.is_empty());
}

#[test]
fn no_book_is_no_stepper() {
    let stepper = BookStepper::for_book(None, Some("anything.md"), false);

    assert!(!stepper.can_previous());
    assert!(!stepper.can_next());
}
