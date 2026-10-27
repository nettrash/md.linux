//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportPipelineTests.cs`.
//!
//! Every flow of §7 driven end to end over the fakes: which HTML each one loads, which surface kind
//! it asks for, whether the dialog comes before or after the rendering, what the file ends up
//! holding, and which alert a failure raises. The order-of-operations tests all work the same way —
//! cancel the dialog and look at what happened anyway, because a flow that renders first has
//! rendered even when nothing is saved, and a flow that asks first has not.
//!
//! LINUX: the four Share tests are ignored (no share sheet, see `export_pipeline`'s decisions), and
//! "the Windows font style" is the Linux one (`screen_html::with_linux_fonts`).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::export_harness::{DOCS, ExportHarness};
use md_core::book::structured_book::{BookUnit, StructuredBook};
use md_core::document::page_size::PageSize;
use md_core::export::{diagram_svg, epub_export, html_export, latex_export, pdf_export};
use md_core::markdown::html;
use md_logic::documents::file_names;
use md_logic::export::{ExportPipeline, RichSnapshotter};
use md_logic::preview::{screen_html, scripts};
use md_logic::strings::exports;
use md_logic::testing::*;

const PLAIN: &str = "# Title\n\nA paragraph.\n";
const RICH: &str = "# Title\n\nInline $x^2$ maths.\n\n```mermaid\ngraph TD;A-->B;\n```\n";

fn rich_count(source: &str) -> usize {
    epub_export::plan_document(source, "Doc").units[0].rich_elements.len()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle.as_bytes())
}

fn book(title: &str, units: &[(&str, &str)]) -> StructuredBook {
    StructuredBook {
        title: title.to_string(),
        front_units: units
            .iter()
            .map(|(title, source)| BookUnit { title: title.to_string(), source: source.to_string() })
            .collect(),
        sections: Vec::new(),
    }
}

// ─────────────────────────────── PDF ───────────────────────────────

#[test]
fn export_pdf_renders_the_paper_page_before_it_asks_where_to_save_it() {
    let h = ExportHarness::new();
    // The dialog cancels; the pages exist regardless, which is only possible if it rendered first.
    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert_eq!(h.renderers.kinds(), [RenderKind::Paper]);
    assert_eq!(h.created()[0].pdf_requests().len(), 1);
    assert_eq!(h.pickers.save_calls().len(), 1);
    assert!(h.files.writes().is_empty());
}

/// LINUX: GTK's save dialog does not create the file it returns; the PDF is written at the chosen
/// path all the same.
#[test]
fn export_pdf_writes_the_pages_over_the_file_the_picker_made() {
    let h = ExportHarness::new();
    h.set_pdf(b"%PDF-1.7\nreal\n");
    let path = h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert_eq!(h.written_bytes(&path), Some(h.pdf()));
    let call = &h.pickers.save_calls()[0];
    assert_eq!(call.suggested_name, "Doc.pdf");
    assert_eq!(call.default_extension, ".pdf");
    assert_eq!(call.choices[0].label, exports::PDF);
}

#[test]
fn the_pdf_page_is_cores_paper_html_with_the_linux_font_style() {
    let h = ExportHarness::new();
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::SIX_BY_NINE));

    let document = html::document(PLAIN, "Doc", false, true);
    let expected =
        screen_html::with_linux_fonts(&pdf_export::styled_for_export(&document, &PageSize::SIX_BY_NINE)).into_owned();
    assert_eq!(h.loaded(), [expected]);
}

#[test]
fn the_paper_geometry_is_inches_with_half_inch_margins() {
    let h = ExportHarness::new();
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::SIX_BY_NINE));

    assert_eq!(
        h.created()[0].pdf_requests(),
        [PrintGeometry { page_width_in: 6.0, page_height_in: 9.0, margin_in: 0.5 }]
    );
}

#[test]
fn a_pdf_that_produced_no_pages_is_the_named_failure() {
    let h = ExportHarness::new();
    h.set_pdf(&[]);
    h.will_save(".pdf");

    h.drain(h.pipeline.export_pdf(PLAIN, "Doc", &PageSize::A4));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_PDF, exports::PDF_PAGES_NOT_PRODUCED)]);
    assert!(h.files.writes().is_empty());
}

#[test]
#[ignore = "Linux has no share sheet: Share ▸ Rendered PDF… is not ported (Export ▸ PDF… writes the same file)"]
fn share_pdf_writes_a_temp_copy_named_after_the_title_and_shares_that_file() {}

#[test]
#[ignore = "Linux has no share sheet: Share ▸ Rendered PDF… and its alert title are not ported"]
fn share_uses_its_own_alert_title() {}

// ─────────────────────────────── HTML ──────────────────────────────

#[test]
fn export_html_loads_the_export_document_not_the_preview_document() {
    let h = ExportHarness::new();
    h.set_eval(|script| {
        if script == html_export::CAPTURE_SCRIPT {
            ExportHarness::json(Some("<html><head></head><body>x</body></html>"))
        } else {
            "null".to_string()
        }
    });
    h.will_save(".html");

    h.drain(h.pipeline.export_html("# T\n\n\\newpage\n\nAfter.\n", "Doc"));

    // The page-break rule swap is the whole difference, and loading the wrong string loses it.
    assert_eq!(h.loaded(), [html_export::export_document("# T\n\n\\newpage\n\nAfter.\n", "Doc")]);
    assert_eq!(h.renderers.kinds(), [RenderKind::Export]);
}

#[test]
fn the_exported_html_is_the_captured_page_with_a_doctype_in_utf8_without_a_bom() {
    let h = ExportHarness::new();
    h.set_eval(|script| {
        if script == html_export::CAPTURE_SCRIPT {
            ExportHarness::json(Some("<html><head></head><body>x</body></html>"))
        } else {
            "null".to_string()
        }
    });
    let path = h.will_save(".html");

    h.drain(h.pipeline.export_html(PLAIN, "Doc"));

    let bytes = h.written_bytes(&path).expect("written");
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        format!("{}<html><head></head><body>x</body></html>", html_export::DOCTYPE)
    );
    assert_eq!(h.pickers.save_calls()[0].suggested_name, "Doc.html");
}

#[test]
fn an_empty_capture_writes_nothing_and_says_the_rendered_page_could_not_be_captured() {
    let h = ExportHarness::new(); // every script answers null
    h.will_save(".html");

    h.drain(h.pipeline.export_html(PLAIN, "Doc"));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_HTML, exports::PAGE_NOT_CAPTURED)]);
    assert!(h.files.writes().is_empty());
    assert!(h.pickers.save_calls().is_empty()); // and it never got as far as asking
}

#[test]
fn the_math_gate_reads_the_input_document_because_the_capture_no_longer_has_the_link() {
    let with_math = ExportHarness::new();
    with_math.set_eval(|_| ExportHarness::json(Some("<html><head></head><body>x</body></html>")));
    with_math.will_save(".html");
    with_math.drain(with_math.pipeline.export_html("$x^2$\n", "Doc"));

    let plain = ExportHarness::new();
    plain.set_eval(|_| ExportHarness::json(Some("<html><head></head><body>x</body></html>")));
    plain.will_save(".html");
    plain.drain(plain.pipeline.export_html(PLAIN, "Doc"));

    assert!(with_math.asset_keys().iter().any(|key| key == html_export::KATEX_CSS_ASSET));
    assert!(plain.asset_keys().is_empty()); // a document without maths reads nothing
}

// ─────────────────────────────── EPUB ──────────────────────────────

#[test]
fn export_epub_asks_where_to_save_before_it_photographs_anything() {
    let h = ExportHarness::new(); // the dialog cancels

    h.drain(h.pipeline.export_epub(RICH, "notes.md", None));

    assert_eq!(h.pickers.save_calls().len(), 1);
    assert!(h.created().is_empty()); // no renderer was ever opened
}

#[test]
fn the_suggested_epub_name_is_the_title_the_book_will_carry() {
    let h = ExportHarness::new();

    h.drain(h.pipeline.export_epub("---\ntitle: From the Front Matter\n---\n\n# Ignored\n", "notes.md", None));

    let call = &h.pickers.save_calls()[0];
    assert_eq!(call.suggested_name, "From the Front Matter.epub");
    assert_eq!(call.choices[0].label, exports::EPUB);
}

#[test]
fn every_rich_element_is_photographed_at_twice_the_size_in_document_order() {
    let expected = rich_count(RICH);
    assert!(expected >= 2, "the fixture must carry both a formula and a diagram");

    let h = ExportHarness::new();
    h.set_eval(move |script| {
        if script == scripts::SCROLL_HEIGHT { "2000".to_string() } else { ExportHarness::rects_default(expected) }
    });
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));

    let created = h.created();
    assert_eq!(created.len(), 1);
    let surface = &created[0];
    assert_eq!(surface.kind(), RenderKind::Export);
    assert_eq!(surface.captures().len(), expected);
    assert!(surface.captures().iter().all(|(_, scale)| *scale == 2.0));
    assert_eq!(surface.heights(), [2000.0]); // grown to the content, which is taller than 842
    assert!(h.written_bytes(&path).is_some());
}

#[test]
fn a_very_tall_diagram_is_photographed_within_the_pixel_budget() {
    let expected = rich_count(RICH);

    let h = ExportHarness::new();
    h.set_eval(move |script| {
        if script == scripts::SCROLL_HEIGHT {
            "80000".to_string()
        } else {
            ExportHarness::rects(expected, 700.0, 40_000.0)
        }
    });
    h.will_save(".epub");

    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));

    let created = h.created();
    assert_eq!(created.len(), 1);
    let captures = created[0].captures();
    assert_eq!(captures.len(), expected);
    for (rect, scale) in captures {
        assert!(scale < 2.0);
        assert!(rect.width * scale * rect.height * scale <= RichSnapshotter::MAX_SNAPSHOT_PIXELS * 1.0001);
    }
}

#[test]
fn the_snapshot_scale_is_twice_until_the_budget_then_less() {
    assert_eq!(RichSnapshotter::scale_for(700.0, 1000.0), 2.0);
    assert_eq!(RichSnapshotter::scale_for(0.0, 0.0), 2.0);
    let tall = RichSnapshotter::scale_for(700.0, 40_000.0);
    assert!(tall < 2.0);
    assert!((RichSnapshotter::MAX_SNAPSHOT_PIXELS - 700.0 * tall * 40_000.0 * tall).abs() < 0.0005);
}

#[test]
fn the_offscreen_surface_is_never_shorter_than_the_standard_page() {
    let expected = rich_count(RICH);
    let h = ExportHarness::new();
    h.set_eval(move |script| {
        if script == scripts::SCROLL_HEIGHT { "120".to_string() } else { ExportHarness::rects_default(expected) }
    });
    h.will_save(".epub");

    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));

    // The design's own number, spelled out: asserting against the constant the code reads would
    // pass for every value it could possibly hold, and a shorter viewport reflows Mermaid's
    // max-width: 100% figures before their rects are measured (§7.1, §7.5).
    assert_eq!(RichSnapshotter::MINIMUM_HEIGHT_CSS_PX, 842.0);
    assert_eq!(h.created()[0].heights(), [842.0]);
}

#[test]
fn a_dom_count_that_disagrees_with_the_markup_aborts_the_whole_book() {
    let expected = rich_count(RICH);
    let h = ExportHarness::new();
    h.set_eval(move |script| {
        if script == scripts::SCROLL_HEIGHT { "2000".to_string() } else { ExportHarness::rects_default(expected - 1) }
    });
    h.will_save(".epub");

    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_EPUB, exports::RICH_IMAGES_NOT_CAPTURED)]);
    assert!(h.files.writes().is_empty()); // never a partial book
    assert!(h.created()[0].captures().is_empty()); // and not one pixel was taken first
}

#[test]
fn a_document_with_nothing_to_photograph_never_opens_a_renderer() {
    let h = ExportHarness::new();
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(PLAIN, "Doc.md", None));

    assert!(h.created().is_empty());
    assert!(h.written_bytes(&path).is_some());
}

#[test]
fn the_book_epub_asks_first_too() {
    let h = ExportHarness::new();

    h.drain(h.pipeline.export_book_epub(book("My Book", &[("One", RICH)])));

    assert_eq!(h.pickers.save_calls()[0].suggested_name, "My Book.epub");
    assert!(h.created().is_empty());
}

// ────────────────────────────── LaTeX ──────────────────────────────

#[test]
fn latex_never_opens_a_browser() {
    let h = ExportHarness::new();
    let path = h.will_save(".tex");

    h.drain(h.pipeline.export_latex(PLAIN, "Doc"));

    assert!(h.created().is_empty());
    assert_eq!(h.written_text(&path), Some(latex_export::document(PLAIN, "Doc")));
    let call = &h.pickers.save_calls()[0];
    assert_eq!(call.suggested_name, "Doc.tex");
    assert_eq!(call.choices[0].label, exports::LATEX);
}

#[test]
fn the_book_latex_is_the_structured_book() {
    let h = ExportHarness::new();
    let structured = book("My Book", &[("One", PLAIN)]);
    let path = h.will_save(".tex");

    h.drain(h.pipeline.export_book_latex(structured.clone()));

    assert_eq!(h.written_text(&path), Some(latex_export::book(&structured)));
    assert_eq!(h.pickers.save_calls()[0].suggested_name, "My Book.tex");
}

// ─────────────────────────────── SVG ───────────────────────────────

#[test]
fn a_diagram_is_lifted_out_of_the_pure_export_page_and_made_standalone() {
    let diagram = diagram_svg::diagrams(RICH).remove(0);
    let h = ExportHarness::new();
    let ordinal = diagram.ordinal;
    h.set_eval(move |script| {
        if script == scripts::diagram_svg(ordinal) {
            ExportHarness::json(Some("<svg viewBox=\"0 0 10 20\"></svg>"))
        } else {
            "null".to_string()
        }
    });
    let path = h.will_save(".svg");

    h.drain(h.pipeline.export_diagram_svg(RICH, "Doc", &diagram));

    assert_eq!(h.renderers.kinds(), [RenderKind::Export]);
    assert_eq!(h.loaded(), [html::document(RICH, "Doc", false, true)]);
    assert_eq!(h.written_text(&path), Some(diagram_svg::standalone_document("<svg viewBox=\"0 0 10 20\"></svg>")));
    assert_eq!(h.pickers.save_calls()[0].suggested_name, "Doc-1.svg");
}

#[test]
fn a_diagram_that_never_rendered_is_said_so_rather_than_saved_empty() {
    let diagram = diagram_svg::diagrams(RICH).remove(0);
    let h = ExportHarness::new(); // the ordinal script answers null
    h.will_save(".svg");

    h.drain(h.pipeline.export_diagram_svg(RICH, "Doc", &diagram));

    assert_eq!(h.alerts.warnings(), [Warning::new(exports::COULD_NOT_EXPORT_SVG, exports::DIAGRAM_NOT_CAPTURED)]);
    assert!(h.files.writes().is_empty());
}

// ──────────────────────────── TextBundle ───────────────────────────

#[test]
fn the_text_bundle_asks_for_a_parent_folder_and_writes_the_bundle_inside_it() {
    let h = ExportHarness::new();
    h.pickers.queue_folder_answer(Some(DOCS));

    h.drain(h.pipeline.export_text_bundle(PLAIN, None, "My Doc"));

    assert_eq!(h.pickers.folder_calls(), [Some(exports::CHOOSE_TEXT_BUNDLE_FOLDER.to_string())]);
    let bundles = h.bundles();
    assert_eq!(bundles.len(), 1);
    let (wrapper, folder) = &bundles[0];
    assert_eq!(*folder, file_names::combine(DOCS, "My Doc.textbundle"));
    assert_eq!(wrapper.text_bytes(), PLAIN.as_bytes());
    assert!(h.created().is_empty());
}

#[test]
fn an_existing_bundle_folder_is_replaced_only_when_the_user_says_so() {
    let target = file_names::combine(DOCS, "My Doc.textbundle");

    let refused = ExportHarness::new();
    refused.files.add_directory(&target);
    refused.pickers.queue_folder_answer(Some(DOCS));
    refused.drain(refused.pipeline.export_text_bundle(PLAIN, None, "My Doc"));

    assert_eq!(refused.alerts.replace_confirmations(), [exports::folder_exists("My Doc.textbundle")]);
    assert!(refused.bundles().is_empty());

    let accepted = ExportHarness::new();
    accepted.files.add_directory(&target);
    accepted.pickers.queue_folder_answer(Some(DOCS));
    accepted.alerts.queue_replace_answer(true);
    accepted.drain(accepted.pipeline.export_text_bundle(PLAIN, None, "My Doc"));

    assert_eq!(accepted.bundles().len(), 1);
}

#[test]
fn images_are_copied_only_from_the_documents_own_folder() {
    let h = ExportHarness::new();
    h.files.add_file(&format!("{DOCS}/logo.png"), [1, 2, 3]);
    h.files.add_file("/home/writer/secret.png", [9]);
    h.pickers.queue_folder_answer(Some(DOCS));

    let source = "![a](logo.png) ![b](../secret.png)\n";
    h.drain(h.pipeline.export_text_bundle(source, Some(&format!("{DOCS}/note.md")), "Note"));

    let bundles = h.bundles();
    assert_eq!(bundles.len(), 1);
    let wrapper = &bundles[0].0;
    assert_eq!(wrapper.assets().len(), 1);
    assert_eq!(wrapper.assets()[0].name, "logo.png");
    let text = String::from_utf8(wrapper.text_bytes().to_vec()).unwrap();
    assert!(text.contains("assets/logo.png"));
    assert!(text.contains("../secret.png"));
}

#[test]
fn an_unsaved_document_exports_with_an_empty_assets_folder() {
    let h = ExportHarness::new();
    h.files.add_file(&format!("{DOCS}/logo.png"), [1, 2, 3]);
    h.pickers.queue_folder_answer(Some(DOCS));

    h.drain(h.pipeline.export_text_bundle("![a](logo.png)\n", None, "Note"));

    let bundles = h.bundles();
    assert_eq!(bundles.len(), 1);
    assert!(bundles[0].0.assets().is_empty());
}

// ───────────────────────── Share and Print ─────────────────────────

#[test]
#[ignore = "Linux has no share sheet: Share ▸ Source… is not ported"]
fn share_source_shares_the_real_file_when_there_is_one() {}

#[test]
#[ignore = "Linux has no share sheet: Share ▸ Source… is not ported"]
fn share_source_of_an_untitled_document_shares_a_temp_copy_of_the_text() {}

/// LINUX: md.win's overlay is GTK's print dialog (`webkit::PrintOperation::run_dialog`); the
/// pipeline's side is unchanged — the paper HTML handed over, and the flow held until it is done.
#[test]
fn print_hands_the_overlay_the_paper_html_and_waits_for_it() {
    let h = ExportHarness::new();
    let shown: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let dismissed: Completion<()> = Completion::new();

    let task = {
        let (shown, dismissed) = (shown.clone(), dismissed.clone());
        h.spawn(h.pipeline.print(PLAIN, "Doc", move |html| {
            *shown.borrow_mut() = Some(html);
            let wait = dismissed.wait();
            Box::pin(async move {
                wait.await;
                Ok(())
            })
        }))
    };

    assert!(!task.get()); // the dialog is still up
    dismissed.complete(());
    h.drain_flag(&task);

    // Paper, not the trim size: Print is always A4 and the page CSS is untouched.
    assert_eq!(
        shown.borrow().clone(),
        Some(screen_html::with_linux_fonts(&html::document(PLAIN, "Doc", false, true)).into_owned())
    );
    assert!(h.created().is_empty()); // the print dialog's renderer is the app's own
}

#[test]
fn a_print_that_fails_says_nothing() {
    let h = ExportHarness::new();

    h.drain(h.pipeline.print(PLAIN, "Doc", |_| ready(Err(RenderError::Failed("no printer".to_string())))));

    assert!(h.alerts.warnings().is_empty());
}

// ───────────────────── the typography invariant ────────────────────

#[test]
fn only_paper_carries_the_linux_font_style_and_every_exported_file_is_pure() {
    let h = ExportHarness::new();
    let count = rich_count(RICH);
    h.set_eval(move |script| {
        if script == html_export::CAPTURE_SCRIPT {
            ExportHarness::json(Some("<html><head></head><body>x</body></html>"))
        } else if script == scripts::SCROLL_HEIGHT {
            "2000".to_string()
        } else if script.starts_with("Array.from") {
            ExportHarness::rects_default(count)
        } else {
            ExportHarness::json(Some("<svg></svg>"))
        }
    });

    let pdf = h.will_save(".pdf");
    h.drain(h.pipeline.export_pdf(RICH, "Doc", &PageSize::A4));
    let html_file = h.will_save(".html");
    h.drain(h.pipeline.export_html(RICH, "Doc"));
    let epub = h.will_save(".epub");
    h.drain(h.pipeline.export_epub(RICH, "Doc.md", None));
    let svg = h.will_save(".svg");
    h.drain(h.pipeline.export_diagram_svg(RICH, "Doc", &diagram_svg::diagrams(RICH)[0]));

    assert!(h.alerts.warnings().is_empty());

    // "every exported file is pure", asserted of the BYTES rather than of what a renderer was
    // handed. The marker is pure ASCII, so it is searched for as raw bytes in the PDF and the zip.
    // (`will_save` reuses one destination stem, so each file is checked under its own extension.)
    for path in [&pdf, &html_file, &epub, &svg] {
        let bytes = h.written_bytes(path).expect("written");
        assert!(!bytes.is_empty());
        assert!(!contains(&bytes, screen_html::ID_MARKER), "{path}");
    }

    // The other half of the rule: what the reader looks at, on screen and on paper, does carry it.
    assert!(screen_html::with_linux_fonts(&html::document(RICH, "Doc", false, false)).contains(screen_html::ID_MARKER));
    assert!(ExportPipeline::paper_html(RICH, "Doc").contains(screen_html::ID_MARKER));

    for surface in h.created() {
        let expect_style = surface.kind() == RenderKind::Paper;
        for html in surface.loaded_html() {
            assert_eq!(html.contains(screen_html::ID_MARKER), expect_style);
        }
    }
    let kinds = h.renderers.kinds();
    assert!(kinds.contains(&RenderKind::Paper));
    assert!(kinds.contains(&RenderKind::Export));
    assert!(!kinds.contains(&RenderKind::Screen));
}
