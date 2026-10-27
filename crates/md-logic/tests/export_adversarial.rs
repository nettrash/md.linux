//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportAdversarialTests.cs`.
//!
//! The ways an export can go wrong that a happy-path test would never notice: a page that never
//! loads, a window that closes mid-render, two exports at once, a menu row that no longer matches
//! the text, a book whose photographs could drift between articles, and the two DOM selectors that
//! must never be confused for one another.

mod common;

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use common::export_harness::{DOCS, ExportHarness};
use common::task::Task;
use md_core::book::structured_book::{BookUnit, StructuredBook};
use md_core::document::page_size::PageSize;
use md_core::export::{diagram_svg, epub_export};
use md_logic::export::{ExportPipeline, RichSnapshotter};
use md_logic::preview::{screen_html, scripts};
use md_logic::strings::exports;
use md_logic::testing::*;

const PLAIN: &str = "# Title\n\nA paragraph.\n";
const RICH: &str = "# Title\n\nInline $x^2$ maths.\n\n```mermaid\ngraph TD;A-->B;\n```\n";

/// A print dialog that stays up until `dismissed` is completed.
fn held(
    dismissed: &Completion<()>,
) -> impl FnOnce(String) -> LocalBoxFuture<'static, Result<(), RenderError>> + 'static {
    let wait = dismissed.wait();
    move |_| {
        Box::pin(async move {
            wait.await;
            Ok(())
        })
    }
}

#[test]
fn a_page_that_never_loaded_is_the_named_error_and_no_file() {
    let h = ExportHarness::new();
    h.set_load_failure(Some(RenderError::Failed(exports::PAGE_NOT_CAPTURED.to_string())));
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_PDF, exports::PAGE_NOT_CAPTURED)]);
    assert!(h.files.writes().is_empty());
}

#[test]
fn the_renderer_is_closed_even_when_the_export_fails() {
    let h = ExportHarness::new();
    h.set_load_failure(Some(RenderError::Failed("boom".to_string())));
    h.will_save(".html");

    h.drain(h.pipeline.export_html(PLAIN, "Doc"));

    let created = h.created();
    assert_eq!(created.len(), 1);
    assert!(created[0].is_closed());
    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_HTML, "boom")]);
}

#[test]
fn the_renderer_is_closed_on_the_happy_path_too() {
    let h = ExportHarness::new();
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    let created = h.created();
    assert_eq!(created.len(), 1);
    assert!(created[0].is_closed());
}

#[test]
fn a_window_closing_under_an_export_says_nothing_to_nobody() {
    let h = ExportHarness::new();
    h.pipeline.cancel();
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert!(h.alerts.warnings().is_empty());
    assert!(h.files.writes().is_empty());
}

/// LINUX: the cancel lands while the EPUB waits on its 300 ms repaint; exec.rs's token wakes
/// nothing, so the wait notices when its timer fires — and the export ends there, silently, with no
/// file and the renderer closed.
#[test]
fn a_window_closing_mid_render_stops_at_the_next_wait_and_says_nothing() {
    let count = epub_export::plan_document(RICH, "Doc").units[0].rich_elements.len();
    let h = ExportHarness::new();
    h.set_eval(
        move |script| {
            if script == scripts::SCROLL_HEIGHT { "2000".to_string() } else { ExportHarness::rects_default(count) }
        },
    );
    h.will_save(".epub");

    let done = h.spawn(h.pipeline.export_epub(RICH, "Doc.md", None));
    assert!(!done.get());
    assert_eq!(h.scheduler.pending_timers(), 1); // the repaint wait

    h.pipeline.cancel();
    h.drain_flag(&done);

    assert!(h.alerts.warnings().is_empty());
    assert!(h.files.writes().is_empty());
    assert!(h.created()[0].captures().is_empty());
    assert!(h.created()[0].is_closed());
}

#[test]
fn a_second_export_waits_for_the_first_to_finish() {
    let h = ExportHarness::new();
    let dismissed: Completion<()> = Completion::new();
    h.will_save(".tex");

    let print = h.spawn(h.pipeline.print(PLAIN, "Doc", held(&dismissed)));
    let latex = h.spawn(h.pipeline.export_latex(PLAIN, "Doc"));

    assert!(!latex.get());
    assert!(h.pickers.save_calls().is_empty()); // the LaTeX export has not even opened its dialog

    dismissed.complete(());
    h.drain_flag(&print);
    h.drain_flag(&latex);
    assert_eq!(h.pickers.save_calls().len(), 1);
}

/// LINUX: the gate is a FIFO lock of md-logic's own (md.win's `SemaphoreSlim`); an export whose
/// future is dropped while it waits — a window torn down mid-queue — must give its turn on rather
/// than hold the next one forever.
#[test]
fn an_export_dropped_while_it_waits_gives_its_turn_to_the_next() {
    let h = ExportHarness::new();
    let dismissed: Completion<()> = Completion::new();
    h.will_save(".tex");

    let print = h.spawn(h.pipeline.print(PLAIN, "Doc", held(&dismissed)));
    let abandoned = Task::start(h.pipeline.export_html(PLAIN, "Doc"));
    assert!(!abandoned.is_completed());
    let latex = h.spawn(h.pipeline.export_latex(PLAIN, "Doc"));
    drop(abandoned);

    dismissed.complete(());
    h.drain_flag(&print);
    h.drain_flag(&latex);

    assert_eq!(h.pickers.save_calls().len(), 1);
    assert!(h.created().is_empty()); // the HTML export never ran
}

#[test]
fn busy_is_raised_around_each_export_so_the_footer_can_show_its_ring() {
    let h = ExportHarness::new();
    let states = Rc::new(RefCell::new(Vec::new()));
    {
        let states = states.clone();
        h.pipeline.busy_changed().connect(move |busy: &bool| states.borrow_mut().push(*busy));
    }
    h.will_save(".tex");

    h.drain(h.pipeline.export_latex(PLAIN, "Doc"));

    assert_eq!(*states.borrow(), [true, false]);
}

#[test]
fn a_diagram_row_that_no_longer_matches_the_text_does_nothing_at_all() {
    let h = ExportHarness::new();
    h.will_save(".svg");

    // The menu was built from a 250 ms-old snapshot; the writer has deleted the fence since.
    h.drain(h.pipeline.export_diagram_svg_at(PLAIN, "Doc", 0));

    assert!(h.created().is_empty());
    assert!(h.alerts.warnings().is_empty());
    assert!(h.pickers.save_calls().is_empty());
}

#[test]
fn the_ordinal_overload_resolves_the_same_diagram_the_menu_named() {
    let diagrams = diagram_svg::diagrams(RICH);
    let h = ExportHarness::new();
    let ordinal = diagrams[0].ordinal;
    h.set_eval(move |script| {
        if script == scripts::diagram_svg(ordinal) { ExportHarness::json(Some("<svg/>")) } else { "null".to_string() }
    });
    let path = h.will_save(".svg");

    h.drain(h.pipeline.export_diagram_svg_at(RICH, "Doc", ordinal));

    assert!(h.written_text(&path).is_some());
}

#[test]
fn each_article_of_a_book_is_photographed_against_its_own_markup() {
    let book = StructuredBook {
        title: "Two Articles".to_string(),
        front_units: vec![
            BookUnit { title: "One".to_string(), source: RICH.to_string() },
            BookUnit { title: "Two".to_string(), source: RICH.to_string() },
        ],
        sections: Vec::new(),
    };
    let plan = epub_export::plan_book(&book);
    let rich = plan.rich_units();
    assert_eq!(rich.len(), 2);
    let (first, second) = (rich[0].rich_elements.len(), rich[1].rich_elements.len());

    let h = ExportHarness::new();
    h.set_eval(
        move |script| {
            if script == scripts::SCROLL_HEIGHT { "1500".to_string() } else { ExportHarness::rects_default(first) }
        },
    );
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_book_epub(book));

    let created = h.created();
    assert_eq!(created.len(), 1); // one renderer for the whole export, reloaded per unit
    assert_eq!(created[0].loaded_html().len(), 2);
    assert_eq!(created[0].captures().len(), first + second);
    assert!(h.written_bytes(&path).is_some());
}

#[test]
fn a_snapshot_that_came_back_empty_is_refused_rather_than_packed_as_a_zero_byte_png() {
    let expected = epub_export::plan_document(RICH, "Doc").units[0].rich_elements.len();
    let h = ExportHarness::new();
    h.set_png(&[]);
    h.set_eval(move |script| {
        if script == scripts::SCROLL_HEIGHT { "2000".to_string() } else { ExportHarness::rects_default(expected) }
    });
    h.will_save(".epub");

    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));

    let warnings = h.alerts.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].message, exports::RICH_IMAGES_NOT_CAPTURED);
    assert!(h.files.writes().is_empty());
}

#[test]
fn the_image_width_is_the_measured_width_not_the_clamped_one() {
    // A zero-width formula is captured as a 1-px region (a renderer refuses an empty one) but must
    // still be written as width="0", exactly as the Mac writes Int(rect.width.rounded()).
    let scheduler = Rc::new(FakeScheduler::new());
    let renderers = Rc::new(FakeRenderSurfaceFactory::new());
    renderers.set_create(|kind| {
        let surface = FakeRenderSurface::new(kind);
        surface.set_eval_handler(|script| {
            if script == scripts::SCROLL_HEIGHT { "900".to_string() } else { "[[5,6,0,0,1]]".to_string() }
        });
        Rc::new(surface)
    });

    let plan = epub_export::plan_document("$x$\n", "Doc");
    assert_eq!(plan.units[0].rich_elements.len(), 1);

    let snapshotter = RichSnapshotter::new(renderers.clone(), scheduler.clone());
    let cancel = CancelToken::new();
    let mut task = Task::start(snapshotter.capture(&plan, &cancel));
    scheduler.advance(RichSnapshotter::REPAINT_DELAY);

    assert!(task.poll());
    let snapshots = task.output().expect("captured");
    assert_eq!(snapshots[&0].len(), 1);
    assert_eq!(snapshots[&0][0].display_width(), 0);

    // The capture itself was clamped to a pixel, because a renderer refuses an empty region.
    let created = renderers.created();
    assert_eq!(created.len(), 1);
    let captures = created[0].captures();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].0, RectD { x: 5.0, y: 6.0, width: 1.0, height: 1.0 });
}

#[test]
fn the_two_dom_selectors_are_different_and_neither_script_uses_the_others() {
    // The EPUB set has maths and no plot; the diagram set has a plot and no maths. Merging them
    // would shift every EPUB image or export the wrong diagram, and both are silent failures.
    assert!(epub_export::RICH_SELECTOR.contains(".md-mathi"));
    assert!(!epub_export::RICH_SELECTOR.contains("plot"));
    assert!(diagram_svg::DOM_SELECTOR.contains("div.plot"));
    assert!(!diagram_svg::DOM_SELECTOR.contains("md-math"));

    assert!(scripts::rich_elements().contains(epub_export::RICH_SELECTOR));
    assert!(!scripts::rich_elements().contains(diagram_svg::DOM_SELECTOR));
    assert!(scripts::diagram_svg(0).contains(diagram_svg::DOM_SELECTOR));
    assert!(!scripts::diagram_svg(0).contains(epub_export::RICH_SELECTOR));
}

#[test]
fn the_rich_element_script_is_the_macs_five_column_one_liner() {
    let script = scripts::rich_elements();
    assert!(script.starts_with("Array.from(document.querySelectorAll("));
    assert!(script.contains("r.left + window.scrollX"));
    assert!(script.contains("r.top + window.scrollY"));
    assert!(script.contains("(e.classList.contains('md-mathi') || e.classList.contains('md-mathd')) ? 1 : 0"));
    assert!(!script.contains('\r'));
}

#[test]
fn the_diagram_script_interpolates_the_ordinal_invariantly_and_handles_a_missing_node() {
    let script = scripts::diagram_svg(12);

    assert!(script.contains("nodes[12]"));
    assert!(script.contains("if (!el) return null;"));
    assert!(script.contains("return svg ? svg.outerHTML : null;"));
    assert!(!script.contains('\r'));
}

#[test]
fn the_canvas_contingency_is_written_and_spells_its_numbers_invariantly() {
    let script = scripts::canvas_rasterise(3, 2.0);

    assert!(script.contains("[3]"));
    assert!(script.contains("rect.width * 2")); // never "2,0"
    assert!(script.contains("window.__mdRaster"));
    assert!(script.contains("String.fromCharCode(bytes[b])"));
    assert!(scripts::CANVAS_RASTERISE_RESULT.contains("window.__mdRaster"));
}

#[test]
fn the_paper_html_is_idempotent_because_print_and_pdf_both_pass_through_it() {
    let once = ExportPipeline::paper_html(PLAIN, "Doc");

    assert_eq!(screen_html::with_linux_fonts(&once), once);
    assert!(once.contains(screen_html::ID_MARKER));
}

#[test]
fn a_text_bundle_whose_write_fails_says_so_and_leaves_nothing_behind() {
    let h = ExportHarness::new();
    h.pickers.queue_folder_answer(Some(DOCS));

    let pipeline = ExportPipeline::new(
        h.renderers.clone(),
        h.pickers.clone(),
        h.alerts.clone(),
        h.files.clone(),
        |_: &str| None,
        h.scheduler.clone(),
        h.identity.clone(),
        h.fetcher.clone(),
    )
    .with_bundle_writer(|_, _| Err(io::Error::other("the disk is full")));

    h.drain(pipeline.export_text_bundle(PLAIN, None, "Doc"));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_TEXT_BUNDLE, "the disk is full")]);
}
