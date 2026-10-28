//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookAdversarialTests.cs`: the traps the porting
//! reports name, and the mutants md.win's first suite let live. Each test exists because a
//! plausible edit passed the whole suite there.
//!
//! LINUX: the names a file system refuses are Linux's (`/`, `:` kept from the Mac, NUL, `.`, `..`),
//! not Windows' (`CON`, `a?b`, `trailing.` are ordinary names here and on the Mac). The flush gate
//! is each test's own hub, so no collection is needed.

mod common;

use std::collections::HashSet;
use std::fs;
use std::rc::Rc;

use common::book_fixture::{BookFixture, FakeBookOutputs, Navigator, hub};
use md_core::book::book_article_session::BookFlushHub;
use md_core::book::book_paths;
use md_logic::books::{BookFlushGateBinding, BookLibraryModel, BookOutput, book_sidebar_model};
use md_logic::settings::settings_keys;
use md_logic::strings::documents;
use md_logic::testing::*;

// MARK: What a failed save must not remember

/// book.md §13.7: `if session.select(url) { lastArticlePath = … }` — the remembered article is the
/// one the writer actually reached. Writing it before the flush is answered quietly reopens the
/// wrong article next launch.
#[test]
fn an_article_the_session_refused_to_open_is_never_the_remembered_one() {
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");
    fixture.scene("02-One/01-a.md");
    let n = Navigator::open(&fixture);
    assert_eq!(n.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).as_deref(), Some("01-Preface.md"));

    n.session.flush_fails.set(true);
    n.model.select(Some(&fixture.at(&["02-One", "01-a.md"])));

    assert_eq!(n.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).as_deref(), Some("01-Preface.md"));
    assert!(!n.settings.writes().iter().any(|(_, value)| *value == SettingValue::from("02-One/01-a.md")));
}

// MARK: A veto is silent, whatever else is wrong

/// §8.7: a vetoed output "aborts silently — the footer already says why". The gate is asked
/// *before* the listing is read, so a veto with no book open must still raise nothing.
#[test]
fn a_veto_is_silent_even_when_there_is_no_book_to_compile() {
    let outputs = Rc::new(FakeBookOutputs::default());
    let hub: Rc<BookFlushHub> = hub();
    let output = BookOutput::new(outputs.clone(), Rc::new(FakeSettingsStore::new()), hub.clone(), || None);
    let alerts = Rc::new(std::cell::RefCell::new(Vec::new()));
    {
        let alerts = alerts.clone();
        output.alert_requested().connect(move |alert: &(String, String)| alerts.borrow_mut().push(alert.clone()));
    }

    let _binding = BookFlushGateBinding::new(&hub, || false);
    block_on(output.export_pdf());
    block_on(output.export_epub());
    block_on(output.print());

    assert!(alerts.borrow().is_empty());
    assert!(outputs.pdfs.borrow().is_empty());
    assert!(outputs.epubs.borrow().is_empty());
    assert!(outputs.prints.borrow().is_empty());
}

// MARK: The example book's second copy

/// §8.2 spells the deduped name "Example Book 2". Only asserting the third name lets a rule that
/// skips straight from the base name to "… 3" through.
#[test]
fn the_second_example_book_is_number_two() {
    let taken: HashSet<&str> = HashSet::from(["Example Book"]);

    assert_eq!(
        BookLibraryModel::deduped_name(
            "Example Book",
            |name| taken.contains(name),
            BookLibraryModel::DEFAULT_MAX_ATTEMPTS
        )
        .as_deref(),
        Some("Example Book 2")
    );
}

// MARK: Names Linux will not hold (§8.3, §6.4, book.md §16.3)

#[test]
fn a_chapter_name_windows_refuses_is_refused_out_loud() {
    for typed in ["a/b", "a:b", ".."] {
        let fixture = BookFixture::new();
        fixture.scene("01-Preface.md");
        let n = Navigator::open(&fixture);

        n.model.create_chapter(typed);

        assert_eq!(
            n.alerts(),
            [(documents::COULD_NOT_RENAME.to_string(), documents::INVALID_NAME_MESSAGE.to_string())],
            "{typed}"
        );
        assert_eq!(fixture.names(), ["01-Preface.md"], "{typed}");
    }
}

/// md.win's chapter names `CON`, `a?b` and `trailing.` are ordinary on Linux, and land.
#[test]
fn a_chapter_name_only_windows_refuses_lands_on_linux() {
    let fixture = BookFixture::new();
    let n = Navigator::open(&fixture);

    for typed in ["CON", "a?b", "trailing."] {
        n.model.create_chapter(typed);
    }

    assert!(n.alerts().is_empty());
    assert_eq!(fixture.names(), ["CON", "a?b", "trailing."]);
}

#[test]
fn an_article_name_is_vetted_with_its_extension_on_and_a_trailing_space_is_still_legal() {
    let fixture = BookFixture::new();
    let n = Navigator::open(&fixture);

    // "a:b" is refused with or without ".md" (md.win: "CON", a device name either way).
    n.model.create_article("a:b", fixture.root());
    assert_eq!(n.alerts().len(), 1);
    assert!(fixture.names().is_empty());

    // "Draft " trims to "Draft"; nothing about it is refusable, and the file lands.
    n.model.create_article("Draft ", fixture.root());
    assert_eq!(n.alerts().len(), 1);
    assert_eq!(fixture.names(), ["Draft.md"]);
}

// MARK: Path traps (book.md §16.1, §18.5)

/// `combine(root, rooted)` returns the second argument whole, so a remembered value that is an
/// absolute path would walk straight out of the folder the app holds rights to.
#[test]
fn an_absolute_remembered_path_is_never_followed_out_of_the_book() {
    let outside = BookFixture::new();
    let secret = outside.scene("secret.md");
    let fixture = BookFixture::new();
    fixture.scene("01-Preface.md");

    let settings = Rc::new(FakeSettingsStore::new());
    settings.set_string(settings_keys::BOOK_LAST_ARTICLE, &secret);
    let n = Navigator::with_settings(settings);
    n.model.open_book(fixture.root());

    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["01-Preface.md"]))));
}

// MARK: The two-phase reorder (§8.2, book.md §17)

/// The swap every one-phase executor corrupts: both targets collide with a source. The bytes are
/// checked, not just the names — a staging bug that swaps the names and not the contents would
/// otherwise pass.
#[test]
fn a_swap_of_identical_stems_keeps_both_articles_and_their_bytes() {
    let fixture = BookFixture::new();
    fixture.article("01-a.md", "# first\n");
    fixture.article("02-a.md", "# second\n");
    let n = Navigator::open(&fixture);
    assert_eq!(n.selected_name().as_deref(), Some("01-a.md"));

    n.model.move_by(&fixture.at(&["01-a.md"]), 1);

    assert_eq!(fixture.names(), ["01-a.md", "02-a.md"]);
    assert_eq!(fixture.text(&fixture.at(&["01-a.md"])), "# second\n");
    assert_eq!(fixture.text(&fixture.at(&["02-a.md"])), "# first\n");
    // The selection followed its article through the plan, not its old name.
    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["02-a.md"]))));
}

/// The staging names are hidden `.md-reorder-*` temporaries. None may survive the operation, and
/// none may ever reach the listing.
#[test]
fn a_reorder_leaves_no_staging_names_behind_and_none_in_the_listing() {
    let fixture = BookFixture::new();
    fixture.scene("01-a.md");
    fixture.scene("02-a.md");
    fixture.scene("03-c.md");
    let n = Navigator::open(&fixture);

    n.model.move_by(&fixture.at(&["03-c.md"]), -1);

    let everything: Vec<String> =
        fs::read_dir(fixture.root()).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    assert!(!everything.iter().any(|name| name.starts_with(".md-reorder-")));
    assert_eq!(everything.len(), 3);
    assert!(!book_sidebar_model::rows(n.model.book().as_ref()).iter().any(|row| row.title.starts_with('.')));
}

// MARK: The case-only rename (book.md §16.4)

/// md-core routes a case-only rename through the reorder's staging name (a case-insensitive volume
/// resolves the destination to the source). From the navigator that has to look like any other
/// rename: the folder is renamed and the article inside it stays selected.
#[test]
fn a_case_only_chapter_rename_lands_and_carries_the_selected_article() {
    let fixture = BookFixture::new();
    fixture.article("02-one/01-a.md", "# a\n");
    let n = Navigator::open(&fixture);
    assert_eq!(n.selected_name().as_deref(), Some("01-a.md"));

    n.model.perform_rename(&fixture.at(&["02-one"]), "One");

    assert_eq!(fixture.names(), ["02-One"]);
    assert_eq!(n.selection(), Some(book_paths::standardize(&fixture.at(&["02-One", "01-a.md"]))));
    assert_eq!(fixture.text(&fixture.at(&["02-One", "01-a.md"])), "# a\n");
}
