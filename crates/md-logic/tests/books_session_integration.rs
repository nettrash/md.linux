//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookSessionIntegrationTests.cs`.
//!
//! The navigator over the real `TextFileSession` in article mode — the seam the documents module
//! publishes and the book window uses. Real files, real bytes: what a fake session cannot prove is
//! that a selection change actually writes the outgoing article, that the flush gate saves through
//! the same path an output reads from, and that a document window taking the file puts the pane
//! into handoff.

mod common;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use common::book_fixture::{BookFixture, FakeBookOutputs, hub};
use md_core::book::book_article_session::BookFlushHub;
use md_core::book::book_paths;
use md_core::document::book_article_opens::BookArticleOpens;
use md_logic::books::{BookFlushGateBinding, BookNavigatorModel, BookOutput, TextFileSessionArticles};
use md_logic::documents::{LocalFileSystem, SessionRole, Stage, TextFileSession};
use md_logic::testing::*;

struct Fixture {
    fixture: BookFixture,
    watcher: Rc<FakeFileWatcher>,
    registry: Rc<FakeDocumentRegistry>,
    identity: Rc<FakeFileIdentity>,
    settings: Rc<FakeSettingsStore>,
    hub: Rc<BookFlushHub>,
    session: Rc<TextFileSession>,
    navigator: Rc<BookNavigatorModel>,
    opened: Rc<RefCell<Vec<String>>>,
}

impl Fixture {
    fn new() -> Self {
        let fixture = BookFixture::new();
        fixture.article("01-Preface.md", "# Preface\n");
        fixture.article("02-One/01-a.md", "# A\n");
        fixture.article("02-One/02-b.md", "# B\n");
        let watcher = Rc::new(FakeFileWatcher::new());
        let scheduler = Rc::new(FakeScheduler::new());
        let registry = Rc::new(FakeDocumentRegistry::new());
        let identity = Rc::new(FakeFileIdentity::new());
        let settings = Rc::new(FakeSettingsStore::new());
        let session = TextFileSession::new(
            Rc::new(LocalFileSystem),
            watcher.clone(),
            scheduler,
            registry.clone(),
            identity.clone(),
            SessionRole::BookArticle,
            None,
        );
        let navigator = Rc::new(BookNavigatorModel::new(
            Rc::new(TextFileSessionArticles::new(session.clone())),
            settings.clone(),
            Rc::new(RefCell::new(BookArticleOpens::new())),
        ));
        let opened: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let opened = opened.clone();
            navigator.open_requested().connect(move |path: &str| opened.borrow_mut().push(path.to_string()));
        }
        navigator.open_book(fixture.root());
        Fixture { fixture, watcher, registry, identity, settings, hub: hub(), session, navigator, opened }
    }

    fn at(&self, parts: &[&str]) -> String {
        self.fixture.at(parts)
    }

    fn read(&self, parts: &[&str]) -> String {
        fs::read_to_string(self.at(parts)).expect("read")
    }

    /// The book window's own binding: its session's flush answers the hub.
    fn binding(&self) -> BookFlushGateBinding {
        let session = self.session.clone();
        BookFlushGateBinding::new(&self.hub, move || session.flush_now(false))
    }

    fn output(&self, outputs: Rc<FakeBookOutputs>) -> BookOutput {
        let navigator = self.navigator.clone();
        BookOutput::new(outputs, self.settings.clone(), self.hub.clone(), move || navigator.book())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.session.dispose();
    }
}

#[test]
fn the_first_article_is_loaded_with_its_bytes() {
    let f = Fixture::new();

    assert_eq!(f.session.text(), "# Preface\n");
    assert_eq!(f.session.title(), "Preface"); // the ordering prefix is not a title
    assert!(matches!(f.session.stage(), Stage::Editing(_)));
}

#[test]
fn moving_to_another_article_writes_the_outgoing_one() {
    let f = Fixture::new();
    f.session.edit("# Preface\n\nRewritten.\n");

    f.navigator.select(Some(&f.at(&["02-One", "01-a.md"])));

    assert_eq!(f.read(&["01-Preface.md"]), "# Preface\n\nRewritten.\n");
    assert_eq!(f.session.text(), "# A\n");
    assert!(!f.session.is_dirty());
}

#[test]
fn stepping_through_the_book_saves_on_every_hop() {
    let f = Fixture::new();
    f.navigator.step_next();
    f.session.edit("# A edited\n");
    f.navigator.step_next();

    assert_eq!(f.read(&["02-One", "01-a.md"]), "# A edited\n");
    assert_eq!(f.session.text(), "# B\n");
}

#[test]
fn the_flush_gate_saves_the_buffer_before_an_output_reads_it() {
    let f = Fixture::new();
    f.session.edit("# Preface\n\nUnsaved.\n");
    let binding = f.binding();

    assert!(f.hub.flush_editor());

    assert_eq!(f.read(&["01-Preface.md"]), "# Preface\n\nUnsaved.\n");
    assert_eq!(binding.requests(), 1);
}

#[test]
fn an_output_compiles_what_the_gate_just_saved() {
    let f = Fixture::new();
    f.session.edit("Front matter, unsaved.");
    let _binding = f.binding();
    let outputs = Rc::new(FakeBookOutputs::default());
    let output = f.output(outputs.clone());

    block_on(output.export_pdf());

    assert!(outputs.pdfs.borrow()[0].0.contains("Front matter, unsaved."));
}

#[test]
fn a_veto_stops_the_output_and_the_buffer_survives() {
    let f = Fixture::new();
    f.session.edit("Not going anywhere.");
    let _binding = BookFlushGateBinding::new(&f.hub, || false);
    let outputs = Rc::new(FakeBookOutputs::default());
    let output = f.output(outputs.clone());

    block_on(output.export_pdf());

    assert!(outputs.pdfs.borrow().is_empty());
    assert_eq!(f.session.text(), "Not going anywhere.");
    assert!(f.session.is_dirty());
}

#[test]
fn while_a_document_window_owns_the_article_the_pane_steps_aside_and_reclaims_it_afterwards() {
    let f = Fixture::new();
    let article = f.at(&["02-One", "01-a.md"]);
    let owner = WindowId(7);
    f.registry.register(owner, &f.identity.canonical(&article));

    f.navigator.select(Some(&article));

    assert!(matches!(f.session.stage(), Stage::Handoff(_)));
    assert_eq!(f.session.editing_path(), None);
    assert_eq!(f.session.owning_window(), Some(owner));

    f.registry.unregister(owner);
    f.session.recheck_ownership();

    assert!(matches!(f.session.stage(), Stage::Editing(_)));
    assert_eq!(f.session.text(), "# A\n");
}

#[test]
fn opening_an_article_in_its_own_window_saves_first_and_only_then_asks_for_the_window() {
    let f = Fixture::new();
    f.session.edit("# Preface\n\nSaved by the handoff.\n");

    assert!(f.navigator.open_in_window(&f.at(&["01-Preface.md"])));

    assert_eq!(f.read(&["01-Preface.md"]), "# Preface\n\nSaved by the handoff.\n");
    assert!(matches!(f.session.stage(), Stage::Handoff(_)));
    assert_eq!(f.opened.borrow().len(), 1);
}

#[test]
fn an_external_rewrite_under_unsaved_edits_is_a_conflict_and_nothing_is_clobbered() {
    let f = Fixture::new();
    f.session.edit("Mine, longer than theirs.");
    fs::write(f.at(&["01-Preface.md"]), "Theirs.").unwrap();

    assert!(!f.session.flush_now(false));

    assert!(f.session.conflicted());
    assert_eq!(f.read(&["01-Preface.md"]), "Theirs.");
    assert_eq!(f.session.text(), "Mine, longer than theirs.");

    f.session.resolve_conflict_keeping_mine();

    assert!(!f.session.conflicted());
    assert_eq!(f.read(&["01-Preface.md"]), "Mine, longer than theirs.");
}

#[test]
fn a_watcher_event_over_our_own_bytes_is_not_a_conflict() {
    let f = Fixture::new();
    f.session.edit("Ours.");
    assert!(f.session.flush_now(false));

    // The monitor fires for our own write too; the stamp — not the event — decides.
    f.watcher.raise_changed(None);

    assert!(!f.session.conflicted());
    assert_eq!(f.session.text(), "Ours.");
}

#[test]
fn a_rename_follows_the_selection_through_the_real_session_and_the_file_keeps_its_bytes() {
    let f = Fixture::new();
    f.navigator.select(Some(&f.at(&["02-One", "02-b.md"])));
    f.session.edit("# B\n\nStill B.\n");

    f.navigator.perform_rename(&f.at(&["02-One", "02-b.md"]), "Bravo");

    let renamed = f.at(&["02-One", "02-Bravo.md"]);
    assert_eq!(fs::read_to_string(&renamed).unwrap(), "# B\n\nStill B.\n");
    assert_eq!(f.navigator.selection(), Some(book_paths::standardize(&renamed)));
    assert_eq!(f.session.text(), "# B\n\nStill B.\n");
}
