//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookSidebarModelTests.cs`: the sidebar over a
//! real folder tree (§8.3) — which rows, in which order, called what, and which of Move Up / Move
//! Down each row offers.

mod common;

use common::book_fixture::BookFixture;
use md_core::book::book_paths;
use md_logic::books::book_sidebar_model;
use md_logic::books::{BookRowKind, BookRowKind::*};
use md_logic::strings::books;

#[test]
fn rows_are_root_articles_then_each_chapter_each_closed_by_a_new_article_row() {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    fixture.scene("02-One/01-a.md");
    fixture.scene("02-One/02-b.md");
    fixture.scene("03-Two/01-c.md");

    let rows = book_sidebar_model::rows(Some(&fixture.load()));

    let shape: Vec<(BookRowKind, &str)> = rows.iter().map(|row| (row.kind, row.title.as_str())).collect();
    assert_eq!(
        shape,
        [
            (RootArticle, "01-Preface"),
            (NewArticle, books::NEW_ARTICLE_ROW),
            (ChapterHeader, "02-One"),
            (ChapterArticle, "01-a"),
            (ChapterArticle, "02-b"),
            (NewArticle, books::NEW_ARTICLE_ROW),
            (ChapterHeader, "03-Two"),
            (ChapterArticle, "01-c"),
            (NewArticle, books::NEW_ARTICLE_ROW),
        ]
    );
}

#[test]
fn a_new_article_row_carries_the_folder_it_creates_in() {
    let fixture = BookFixture::new();
    let chapter = fixture.chapter("02-One");

    let rows = book_sidebar_model::rows(Some(&fixture.load()));

    let new_rows: Vec<_> = rows.iter().filter(|row| row.kind == NewArticle).collect();
    assert_eq!(new_rows.len(), 2); // the root's, and the empty chapter's
    assert_eq!(new_rows[0].folder, book_paths::standardize(fixture.root()));
    assert_eq!(new_rows[1].folder, book_paths::standardize(&chapter));
    assert!(new_rows.iter().all(|row| row.path.is_none()));
}

#[test]
fn only_articles_are_selectable() {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    fixture.scene("02-One/01-a.md");

    let rows = book_sidebar_model::rows(Some(&fixture.load()));

    let selectable: Vec<BookRowKind> = rows.iter().filter(|row| row.is_selectable()).map(|row| row.kind).collect();
    assert_eq!(selectable, [RootArticle, ChapterArticle]);
}

#[test]
fn a_chapter_article_belongs_to_its_chapter_folder_not_the_root() {
    let fixture = BookFixture::new();
    let chapter = fixture.chapter("02-One");
    fixture.scene("02-One/01-a.md");

    let rows = book_sidebar_model::rows(Some(&fixture.load()));
    let row = rows.iter().find(|row| row.kind == ChapterArticle).expect("the article row");

    assert_eq!(row.folder, book_paths::standardize(&chapter));
}

#[test]
fn no_book_is_no_rows() {
    assert!(book_sidebar_model::rows(None).is_empty());
}

#[test]
fn move_up_is_dead_at_the_top_and_move_down_at_the_bottom() {
    let fixture = BookFixture::new();
    let first = fixture.scene("01-a.md");
    let middle = fixture.scene("02-b.md");
    let last = fixture.scene("03-c.md");
    let book = fixture.load();

    let top = book_sidebar_model::commands_for(Some(&book), Some(&first)).expect("commands");
    assert!(!top.can_move_up());
    assert!(top.can_move_down());

    let centre = book_sidebar_model::commands_for(Some(&book), Some(&middle)).expect("commands");
    assert!(centre.can_move_up());
    assert!(centre.can_move_down());

    let bottom = book_sidebar_model::commands_for(Some(&book), Some(&last)).expect("commands");
    assert!(bottom.can_move_up());
    assert!(!bottom.can_move_down());
}

#[test]
fn an_article_renumbers_among_its_own_folder_and_a_chapter_among_the_chapters() {
    let fixture = BookFixture::new();
    fixture.scene("02-One/01-a.md");
    fixture.scene("02-One/02-b.md");
    let chapter = fixture.chapter("02-One");
    fixture.chapter("03-Two");
    let book = fixture.load();

    let article = book_sidebar_model::commands_for(Some(&book), Some(&format!("{chapter}/02-b.md"))).expect("commands");
    assert_eq!(article.folder, book_paths::standardize(&chapter));
    assert_eq!(article.siblings, ["01-a.md", "02-b.md"]);
    assert_eq!(article.index, 1);
    assert!(!article.is_chapter);
    assert_eq!(article.delete_message(), books::DELETE_ARTICLE_MESSAGE);

    let section = book_sidebar_model::commands_for(Some(&book), Some(&chapter)).expect("commands");
    assert_eq!(section.folder, book_paths::standardize(fixture.root()));
    assert_eq!(section.siblings, ["02-One", "03-Two"]);
    assert_eq!(section.index, 0);
    assert!(section.is_chapter);
    assert_eq!(section.delete_message(), books::DELETE_CHAPTER_MESSAGE);
}

#[test]
fn a_path_from_another_book_has_no_commands() {
    let fixture = BookFixture::new();
    let other = BookFixture::new();
    fixture.scene("01-a.md");
    let stranger = other.scene("01-a.md");

    assert_eq!(book_sidebar_model::commands_for(Some(&fixture.load()), Some(&stranger)), None);
    assert_eq!(book_sidebar_model::commands_for(Some(&fixture.load()), None), None);
    assert_eq!(book_sidebar_model::commands_for(None, Some(&stranger)), None);
}

#[test]
fn the_selected_row_is_found_by_path_and_nothing_else_is() {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    let inner = fixture.scene("02-One/01-a.md");
    let rows = book_sidebar_model::rows(Some(&fixture.load()));

    assert_eq!(book_sidebar_model::index_of_article(&rows, Some(&inner)), Some(3));
    assert_eq!(book_sidebar_model::index_of_article(&rows, None), None);
    // A chapter folder is a row, but never a selected one.
    assert_eq!(book_sidebar_model::index_of_article(&rows, Some(&fixture.at(&["02-One"]))), None);
}
