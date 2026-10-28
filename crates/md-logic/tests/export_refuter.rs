//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportRefuterTests.cs`.
//!
//! The rows a refuter's mutation run found unguarded in md.win, plus the traps export.md and the
//! family's own notes name. Each was a live mutant there: the code was already right, and every
//! other test still passed with it made wrong.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::export_harness::{DOCS, ExportHarness};
use common::task::Task;
use md_core::book::structured_book::{BookUnit, StructuredBook};
use md_core::document::page_size::PageSize;
use md_core::export::{diagram_svg, epub_export};
use md_logic::export::{ExportPipeline, RichSnapshotter, export_names};
use md_logic::preview::scripts;
use md_logic::strings::exports;
use md_logic::testing::*;

const PLAIN: &str = "# Title\n\nA paragraph.\n";
const RICH: &str = "# Title\n\nInline $x^2$ maths.\n\n```mermaid\ngraph TD;A-->B;\n```\n";

// Two diagrams, so "the row the menu named" and "the row next to it" are different answers.
const TWO_DIAGRAMS: &str = "# Two\n\n```mermaid\ngraph TD;A-->B;\n```\n\n```graphviz\ndigraph { a -> b; }\n```\n";

// ── the repaint wait (§7.5 step 2) ────────────────────────────────────────────────────────

#[test]
fn nothing_is_measured_until_the_macs_three_hundred_millisecond_repaint_has_elapsed() {
    // The wait is KEPT deliberately: Mermaid's `max-width: 100%` means its layout is not final
    // until the surface has been resized and laid out again — a rect read before that is a rect of
    // the old width, and the picture is cropped. So the number and the ordering are both asserted.
    assert_eq!(RichSnapshotter::REPAINT_DELAY, Duration::from_millis(300));

    let scheduler = Rc::new(FakeScheduler::new());
    let renderers = Rc::new(FakeRenderSurfaceFactory::new());
    renderers.set_create(|kind| {
        let surface = FakeRenderSurface::new(kind);
        surface.set_eval_handler(|script| {
            if script == scripts::SCROLL_HEIGHT { "2000".to_string() } else { "[[1,2,3,4,1]]".to_string() }
        });
        Rc::new(surface)
    });

    let plan = epub_export::plan_document("$x$\n", "Doc");
    let snapshotter = RichSnapshotter::new(renderers.clone(), scheduler.clone());
    let cancel = CancelToken::new();
    let mut task = Task::start(snapshotter.capture(&plan, &cancel));

    // The height is set, and then the flow stops dead on a timer.
    let created = renderers.created();
    assert_eq!(created.len(), 1);
    let surface = &created[0];
    assert_eq!(surface.heights(), [2000.0]);
    assert!(!task.is_completed());
    assert_eq!(scheduler.pending_delays(), [Duration::from_millis(300)]);
    assert!(!surface.evals().contains(&scripts::rich_elements()));
    assert!(surface.captures().is_empty());

    // 299 ms is not enough — the wait is a real wait, not a yield.
    scheduler.advance(Duration::from_millis(299));
    assert!(!task.poll());
    assert!(surface.captures().is_empty());

    scheduler.advance(Duration::from_millis(1));
    assert!(task.poll());
    assert!(task.output().is_ok());
    assert!(surface.evals().contains(&scripts::rich_elements()));
    assert_eq!(surface.captures().len(), 1);
}

// ── the diagram the menu named, and only that one (§7.6) ──────────────────────────────────

#[test]
fn the_ordinal_picks_its_own_diagram_and_never_the_neighbour_when_the_row_has_gone() {
    let diagrams = diagram_svg::diagrams(TWO_DIAGRAMS);
    assert_eq!(diagrams.len(), 2);

    // Asked for the second: the second is what the script is run for, not the first.
    {
        let h = ExportHarness::new();
        h.set_eval(|script| {
            if script == scripts::diagram_svg(1) {
                ExportHarness::json(Some("<svg id=\"second\"/>"))
            } else {
                "null".to_string()
            }
        });
        let path = h.will_save(".svg");

        h.drain(h.pipeline.export_diagram_svg_at(TWO_DIAGRAMS, "Doc", 1));

        assert!(h.written_text(&path).expect("written").contains("second"));
        assert_eq!(h.pickers.save_calls()[0].suggested_name, "Doc-2.svg"); // 1-based for the reader
    }

    // The writer deleted the second fence between the menu build and the click. The command does
    // nothing at all — no renderer, no dialog, no alert — rather than saving the survivor under
    // the name of the diagram that is gone.
    {
        let h = ExportHarness::new();
        h.set_eval(|_| ExportHarness::json(Some("<svg id=\"first\"/>")));
        h.will_save(".svg");

        h.drain(h.pipeline.export_diagram_svg_at("# One\n\n```mermaid\ngraph TD;A-->B;\n```\n", "Doc", 1));

        assert!(h.created().is_empty());
        assert!(h.pickers.save_calls().is_empty());
        assert!(h.alerts.warnings().is_empty());
        assert!(h.files.writes().is_empty());
    }
}

// ── the busy signal (§7.1) ────────────────────────────────────────────────────────────────

/// LINUX: md.win's third flow here is Share ▸ Source…, which Linux does not have; Export ▸
/// TextBundle… (its folder dialog cancelled) is the same kind of instant flow and raises busy just
/// the same.
#[test]
fn print_never_raises_busy_so_no_footer_ring_spins_behind_the_overlay() {
    let h = ExportHarness::new();
    let states = Rc::new(RefCell::new(Vec::new()));
    {
        let states = states.clone();
        h.pipeline.busy_changed().connect(move |busy: &bool| states.borrow_mut().push(*busy));
    }

    h.drain(h.pipeline.print(PLAIN, "Doc", |_| ready(Ok(()))));
    assert!(states.borrow().is_empty()); // print, and print alone

    h.will_save(".tex");
    h.drain(h.pipeline.export_latex(PLAIN, "Doc"));
    assert_eq!(*states.borrow(), [true, false]); // every other flow does raise it

    h.drain(h.pipeline.export_text_bundle(PLAIN, None, "Doc"));
    assert_eq!(*states.borrow(), [true, false, true, false]);

    // And the delay the footer waits before it draws anything is the design's, published here
    // rather than spelled again in the window that draws it.
    assert_eq!(ExportPipeline::BUSY_RING_DELAY, Duration::from_millis(500));
}

// ── the TextBundle's Replace question (§7.7) ──────────────────────────────────────────────

#[test]
fn an_ordinary_file_in_the_way_is_the_same_replace_question_as_a_folder() {
    // "Exists" is the save panel's question, and a plain file called `Doc.textbundle` occupies the
    // name exactly as a folder does. Answering Cancel must leave it untouched; answering Replace
    // must go ahead.
    let target = format!("{DOCS}/Doc.textbundle");

    {
        let cancelled = ExportHarness::new();
        cancelled.files.add_file(&target, "not a bundle at all");
        cancelled.pickers.queue_folder_answer(Some(DOCS));
        cancelled.alerts.queue_replace_answer(false);

        cancelled.drain(cancelled.pipeline.export_text_bundle(PLAIN, None, "Doc"));

        assert_eq!(cancelled.alerts.replace_confirmations(), [exports::folder_exists("Doc.textbundle")]);
        assert!(cancelled.bundles().is_empty());
        assert!(cancelled.alerts.warnings().is_empty()); // a cancel is silence, never an alert
    }

    {
        let replaced = ExportHarness::new();
        replaced.files.add_file(&target, "not a bundle at all");
        replaced.pickers.queue_folder_answer(Some(DOCS));
        replaced.alerts.queue_replace_answer(true);

        replaced.drain(replaced.pipeline.export_text_bundle(PLAIN, None, "Doc"));

        let bundles = replaced.bundles();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].1, target);
    }
}

// ── a book whose articles are not all rich (§7.5, core-api §A4 "epub") ────────────────────

#[test]
fn only_the_rich_articles_of_a_mixed_book_are_opened_and_their_photographs_keep_their_own_index() {
    // The drift this guards against is silent: snapshots keyed by a running counter instead of the
    // unit's own index put article three's diagram into article five, and the book still saves. A
    // book with a plain article between two rich ones is where a counter and an index disagree.
    let book = StructuredBook {
        title: "Mixed".to_string(),
        front_units: vec![
            BookUnit { title: "One".to_string(), source: RICH.to_string() },
            BookUnit { title: "Two".to_string(), source: PLAIN.to_string() },
            BookUnit { title: "Three".to_string(), source: RICH.to_string() },
        ],
        sections: Vec::new(),
    };
    let plan = epub_export::plan_book(&book);
    let rich: Vec<(usize, usize)> =
        plan.rich_units().iter().map(|unit| (unit.index, unit.rich_elements.len())).collect();
    assert_eq!(rich.len(), 2);
    assert!(rich[1].0 - rich[0].0 > 1, "the plain article must sit between the two rich ones");

    let scheduler = Rc::new(FakeScheduler::new());
    let renderers = Rc::new(FakeRenderSurfaceFactory::new());
    let count = rich[0].1;
    renderers.set_create(move |kind| {
        let surface = FakeRenderSurface::new(kind);
        surface.set_eval_handler(move |script| {
            if script == scripts::SCROLL_HEIGHT { "1500".to_string() } else { ExportHarness::rects_default(count) }
        });
        Rc::new(surface)
    });

    let snapshotter = RichSnapshotter::new(renderers.clone(), scheduler.clone());
    let cancel = CancelToken::new();
    let mut task = Task::start(snapshotter.capture(&plan, &cancel));
    for _ in 0..8 {
        if task.is_completed() {
            break;
        }
        scheduler.advance(RichSnapshotter::REPAINT_DELAY);
        task.poll();
    }
    assert!(task.is_completed());

    let snapshots = task.output().expect("captured");
    let mut keys: Vec<usize> = snapshots.keys().copied().collect();
    keys.sort_unstable();
    assert_eq!(keys, [rich[0].0, rich[1].0]);
    assert_eq!(renderers.created()[0].loaded_html().len(), 2); // the plain unit never opened a page

    // And md-core accepts exactly this map — a key it did not expect would fail.
    assert!(!epub_export::assemble(&plan, Some(&snapshots), None, None, None).expect("assembled").is_empty());
}

// ── the comma-decimal trap (multi-port grammar divergence) ────────────────────────────────

/// md.win runs this under de-DE and ru-RU, where .NET's default number format would write "1,5".
/// Rust's formatting has no culture at all, so the trap cannot be set; the assertions — every
/// number the export path spells, and the geometry, which is arithmetic — run as they are, once.
#[test]
fn a_comma_decimal_ui_culture_changes_no_script_no_name_and_no_geometry() {
    assert_eq!(export_names::diagram_svg("Doc", 2), "Doc-3.svg");
    assert!(scripts::diagram_svg(2).contains("[2]"));
    assert!(scripts::canvas_rasterise(0, 2.0).contains("* 2)"));
    assert!(!scripts::canvas_rasterise(0, 1.5).contains(",5"));
    assert!(scripts::canvas_rasterise(0, 1.5).contains("1.5"));

    // A4's 595.2 × 841.8 pt is 8.2666… × 11.691… in, whatever the locale.
    let a4 = PrintGeometry::for_page(&PageSize::A4);
    assert!((a4.page_width_in - 595.2 / 72.0).abs() < 1e-10);
    assert!((a4.page_height_in - 841.8 / 72.0).abs() < 1e-10);
    assert_eq!(a4.margin_in, 0.5);
}

// ── the dialog's empty file (Appendix A) ──────────────────────────────────────────────────

/// LINUX: GTK's save dialog creates no file, so the 0-byte trap is Windows'; rendering first and
/// asking second (the Mac's order) still means a failed PDF never asks at all.
#[test]
fn a_pdf_that_cannot_be_produced_never_opens_a_picker_and_so_leaves_no_empty_file() {
    let h = ExportHarness::new();
    h.set_pdf(&[]);
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert!(h.pickers.save_calls().is_empty());
    assert!(h.files.writes().is_empty());
    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_PDF, exports::PDF_PAGES_NOT_PRODUCED)]);
}
