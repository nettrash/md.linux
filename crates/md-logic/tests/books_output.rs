//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookOutputTests.cs`: the whole-book outputs and
//! the gate in front of them (§8.7).
//!
//! md.win's gate is a process-wide static event, so its class runs alone ("BookFlushGate"
//! collection). Here every test owns its `BookFlushHub`, and the suites run in parallel.
//!
//! LINUX: no Share Book as PDF — md.win's share test is ported against Export Book ▸ PDF…, and the
//! flush-gate count is four outputs, not five.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::book_fixture::{BookFixture, FakeBookOutputs, file_name, hub};
use common::document_fixtures::cp1251;
use common::export_harness::ExportHarness;
use md_core::book::book_article_session::BookFlushHub;
use md_core::book::{book_compiler, book_model, book_naming};
use md_core::document::page_size::PageSize;
use md_core::export::pdf_export;
use md_core::markdown::html;
use md_logic::books::{BookFlushGateBinding, BookOutput, BookOutputs, PipelineBookOutputs};
use md_logic::preview::screen_html;
use md_logic::settings::settings_keys;
use md_logic::strings::{books, exports};
use md_logic::testing::*;

fn three_articles() -> BookFixture {
    let fixture = BookFixture::new();
    fixture.article("01-Front.md", "Front matter.");
    fixture.article("02-One/01-a.md", "First.");
    fixture.article("02-One/02-b.md", "Second.");
    fixture
}

struct Output {
    output: BookOutput,
    outputs: Rc<FakeBookOutputs>,
    alerts: Rc<RefCell<Vec<(String, String)>>>,
}

fn output_with(
    fixture: &BookFixture,
    settings: Rc<FakeSettingsStore>,
    hub: Rc<BookFlushHub>,
    outputs: Rc<FakeBookOutputs>,
) -> Output {
    let root = fixture.root().to_string();
    let output = BookOutput::new(outputs.clone(), settings, hub, move || book_model::load(&root));
    let alerts: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let alerts = alerts.clone();
        output.alert_requested().connect(move |alert: &(String, String)| alerts.borrow_mut().push(alert.clone()));
    }
    Output { output, outputs, alerts }
}

fn output(fixture: &BookFixture) -> Output {
    output_with(fixture, Rc::new(FakeSettingsStore::new()), hub(), Rc::new(FakeBookOutputs::default()))
}

fn alerts_of(output: &BookOutput) -> Rc<RefCell<Vec<(String, String)>>> {
    let alerts: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = alerts.clone();
    output.alert_requested().connect(move |alert: &(String, String)| sink.borrow_mut().push(alert.clone()));
    alerts
}

fn pair(title: &str, message: &str) -> (String, String) {
    (title.to_string(), message.to_string())
}

#[test]
fn the_pdf_is_the_whole_book_one_page_per_part_at_the_stored_trim_size() {
    let fixture = three_articles();
    let settings = Rc::new(FakeSettingsStore::new());
    settings.set_string(settings_keys::PDF_PAGE_SIZE, "5x8");
    let o = output_with(&fixture, settings, hub(), Rc::new(FakeBookOutputs::default()));

    block_on(o.output.export_pdf());

    let pdfs = o.outputs.pdfs.borrow();
    assert_eq!(pdfs.len(), 1);
    let (source, title, page_size) = &pdfs[0];
    assert_eq!(*title, book_naming::display_name(file_name(fixture.root())));
    assert_eq!(page_size, "5x8");
    assert!(source.starts_with(&format!("# {title}{}", book_compiler::PAGE_SEPARATOR)));
    assert!(source.contains(&format!("{sep}# One{sep}", sep = book_compiler::PAGE_SEPARATOR)));
    assert!(source.ends_with("Second."));
}

/// md.win drives this through Share as PDF, which Linux does not have; Export Book ▸ PDF… reads the
/// same setting.
#[test]
fn an_unknown_trim_size_is_a4() {
    let fixture = three_articles();
    let settings = Rc::new(FakeSettingsStore::new());
    settings.set_string(settings_keys::PDF_PAGE_SIZE, "tabloid");
    let o = output_with(&fixture, settings, hub(), Rc::new(FakeBookOutputs::default()));

    block_on(o.output.export_pdf());

    assert_eq!(o.outputs.pdfs.borrow()[0].2, "a4");
    assert_eq!(o.output.page_size(), &PageSize::A4);
}

#[test]
fn printing_takes_no_page_size_at_all() {
    let fixture = three_articles();
    let o = output(&fixture);

    block_on(o.output.print());

    assert_eq!(o.outputs.prints.borrow().len(), 1);
}

#[test]
fn epub_and_latex_keep_the_chapter_boundaries_the_compile_flattens() {
    let fixture = three_articles();
    let o = output(&fixture);

    block_on(o.output.export_epub());
    block_on(o.output.export_latex());

    let epubs = o.outputs.epubs.borrow();
    assert_eq!(epubs.len(), 1);
    let book = &epubs[0];
    assert_eq!(book.front_units.iter().map(|unit| unit.title.as_str()).collect::<Vec<_>>(), ["Front"]);
    assert_eq!(book.sections.len(), 1);
    assert_eq!(book.sections[0].title, "One");
    assert_eq!(book.sections[0].units.iter().map(|unit| unit.title.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(
        book.sections[0].units.iter().map(|unit| unit.source.as_str()).collect::<Vec<_>>(),
        ["First.", "Second."]
    );
    // The same structure reaches LaTeX.
    assert_eq!(o.outputs.latex.borrow().as_slice(), std::slice::from_ref(book));
}

#[test]
fn with_no_book_open_every_output_says_so_and_produces_nothing() {
    let outputs = Rc::new(FakeBookOutputs::default());
    let output = BookOutput::new(outputs.clone(), Rc::new(FakeSettingsStore::new()), hub(), || None);
    let alerts = alerts_of(&output);

    block_on(output.export_pdf());
    block_on(output.export_epub());

    assert!(outputs.pdfs.borrow().is_empty());
    assert!(outputs.epubs.borrow().is_empty());
    assert_eq!(
        *alerts.borrow(),
        [
            pair(books::COULD_NOT_COMPILE_BOOK, books::NO_BOOK_ACCESSIBLE),
            pair(exports::COULD_NOT_EXPORT_EPUB, books::NO_BOOK_ACCESSIBLE),
        ]
    );
}

#[test]
fn one_unreadable_article_aborts_the_whole_book_and_names_it() {
    let fixture = three_articles();
    let outputs = Rc::new(FakeBookOutputs::default());
    let root = fixture.root().to_string();
    let output =
        BookOutput::new(outputs.clone(), Rc::new(FakeSettingsStore::new()), hub(), move || book_model::load(&root))
            .with_reader(|path| if file_name(path) == "02-b.md" { None } else { Some("text".to_string()) });
    let alerts = alerts_of(&output);

    block_on(output.export_pdf());
    block_on(output.export_latex());

    assert!(outputs.pdfs.borrow().is_empty());
    assert!(outputs.latex.borrow().is_empty());
    assert_eq!(
        *alerts.borrow(),
        [
            pair(books::COULD_NOT_COMPILE_BOOK, &books::article_unreadable("02-b")),
            pair(exports::COULD_NOT_EXPORT_LATEX, &books::article_unreadable("02-b")),
        ]
    );
}

#[test]
fn a_legacy_encoded_article_compiles_as_what_it_says_not_as_mojibake() {
    let fixture = three_articles();
    fixture.article("01-Front.md", "");
    std::fs::write(fixture.at(&["01-Front.md"]), cp1251("Привет, мир!")).unwrap();
    let o = output(&fixture);

    block_on(o.output.export_pdf());

    assert!(o.outputs.pdfs.borrow()[0].0.contains("Привет, мир!"));
}

// MARK: The flush gate

#[test]
fn every_output_asks_the_editor_to_save_first() {
    let fixture = three_articles();
    let hub = hub();
    let binding = BookFlushGateBinding::new(&hub, || true);
    let o = output_with(&fixture, Rc::new(FakeSettingsStore::new()), hub.clone(), Rc::new(FakeBookOutputs::default()));

    block_on(o.output.export_pdf());
    block_on(o.output.print());
    block_on(o.output.export_epub());
    block_on(o.output.export_latex());

    assert_eq!(binding.requests(), 4);
}

#[test]
fn a_failed_save_vetoes_the_output_silently() {
    let fixture = three_articles();
    let hub = hub();
    let _binding = BookFlushGateBinding::new(&hub, || false);
    let outputs = Rc::new(FakeBookOutputs::default());
    let first = output_with(&fixture, Rc::new(FakeSettingsStore::new()), hub.clone(), outputs.clone());
    let second = output_with(&fixture, Rc::new(FakeSettingsStore::new()), hub.clone(), outputs.clone());

    block_on(first.output.export_pdf());
    block_on(second.output.export_epub());

    assert!(outputs.pdfs.borrow().is_empty());
    assert!(outputs.epubs.borrow().is_empty());
    assert!(first.alerts.borrow().is_empty()); // the book window's footer already says why
    assert!(second.alerts.borrow().is_empty());
}

#[test]
fn no_book_window_means_no_subscriber_and_an_open_gate() {
    let fixture = three_articles();
    let hub = hub();

    // A binding that vetoes, disposed before the output runs: exactly a book window that closed.
    BookFlushGateBinding::new(&hub, || false).dispose();
    let o = output_with(&fixture, Rc::new(FakeSettingsStore::new()), hub, Rc::new(FakeBookOutputs::default()));
    block_on(o.output.export_pdf());

    assert_eq!(o.outputs.pdfs.borrow().len(), 1);
}

#[test]
fn the_gate_is_answered_synchronously_so_the_compile_reads_the_saved_bytes() {
    let hub = hub();
    let order = Rc::new(RefCell::new(Vec::new()));
    let _binding = {
        let order = order.clone();
        BookFlushGateBinding::new(&hub, move || {
            order.borrow_mut().push("flush");
            true
        })
    };

    order.borrow_mut().push("before");
    assert!(hub.flush_editor());
    order.borrow_mut().push("after");

    assert_eq!(*order.borrow(), ["before", "flush", "after"]);
}

// MARK: The pipeline as the book's outputs (md.win's App-side BookExportOutputs)

#[test]
fn the_book_pdf_and_print_reach_the_pipeline_as_paper() {
    let fixture = three_articles();
    let h = ExportHarness::new();
    let path = h.will_save(".pdf");
    let printed: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let pipeline = h.pipeline.clone();
    let outputs: Rc<dyn BookOutputs> = {
        let printed = printed.clone();
        Rc::new(PipelineBookOutputs::new(pipeline, move |html| {
            printed.borrow_mut().push(html);
            ready(Ok(()))
        }))
    };
    let settings = Rc::new(FakeSettingsStore::new());
    settings.set_string(settings_keys::PDF_PAGE_SIZE, "6x9");
    let root = fixture.root().to_string();
    let book = BookOutput::new(outputs, settings, hub(), move || book_model::load(&root));

    h.drain(book.export_pdf());
    h.drain(book.print());

    let title = book_naming::display_name(file_name(fixture.root())).to_string();
    let source = book_compiler::compile_book_source(&fixture.load(), md_core::book::book_folder::read_article).unwrap();
    let document = html::document(&source, &title, false, true);
    assert_eq!(
        h.loaded(),
        [screen_html::with_linux_fonts(&pdf_export::styled_for_export(&document, &PageSize::SIX_BY_NINE)).into_owned()]
    );
    assert_eq!(h.created()[0].pdf_requests(), [PrintGeometry::for_page(&PageSize::SIX_BY_NINE)]);
    assert!(h.written_bytes(&path).is_some());
    assert_eq!(*printed.borrow(), [screen_html::with_linux_fonts(&document).into_owned()]);
}
