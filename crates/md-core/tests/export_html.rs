//! The pure half of "Export as HTML": the document the app loads, and the finished file it writes
//! once the offscreen `WebView` has handed the live DOM back. Port of md.win `HtmlExportTests.cs`.
//!
//! Nothing here renders anything — the browser's part (load, wait for `data-md-render-complete`,
//! run the capture script) is simulated by `capture`, which does to a string exactly what the
//! script does to the DOM: drop every `<script>` and every stylesheet `<link>`, and hand back
//! `documentElement.outerHTML` (no doctype).
//!
//! md.win ports mdTests.swift's `testEmbeddedKatexCSSInlinesEveryFaceAsWoff2`,
//! `testHTMLExportKeepsPageBreaksVisible` and `testExportPageIsPlainWhiteAndAlwaysLight`, plus the
//! string-level halves of RichRenderTests' three self-contained-export proofs and the two cases the
//! Kotlin suite adds (a missing face, a missing stylesheet). The rest are md.win's own: the two
//! licence notices byte for byte, the two gates reading two different strings, the `$`-template
//! hazard, and the four cultures.

mod common;
#[path = "common/export_html.rs"]
mod html_common;

use std::cell::RefCell;

use html_common::{assets, capture, count, has, lacks, read_bundled_asset};
use md_core::document::page_size::PageSize;
use md_core::export::html_export::{self, EmptyCapture};
use md_core::export::pdf_export;
use md_core::markdown::html;

const DASH: char = '\u{2014}'; // EM DASH, in both notices

/// `golden/stylesheet-export.css` without its final newline (md.win's `[..^1]`).
fn golden_export_sheet() -> String {
    let mut sheet = common::fixture_text("golden/stylesheet-export.css");
    sheet.pop();
    sheet
}

// MARK: - the licence notices

#[test]
fn the_katex_notice_is_the_verbatim_mit_and_ofl_text() {
    let expected = [
        "<!--".to_string(),
        format!("  Mathematics rendered with KaTeX (https://katex.org) {DASH} MIT License,"),
        "  Copyright (c) 2013-2020 Khan Academy and other contributors.".to_string(),
        "  The embedded KaTeX_* fonts are licensed under the SIL Open Font".to_string(),
        "  License 1.1 (https://scripts.sil.org/OFL); \"KaTeX\" is a Reserved Font".to_string(),
        "  Name. The fonts are embedded unmodified.".to_string(),
        "-->".to_string(),
    ]
    .join("\n");
    assert_eq!(html_export::KATEX_NOTICE, expected);
    // Legal text, so pin its shape too: seven lines, no trailing newline, no CR from a checkout.
    assert_eq!(html_export::KATEX_NOTICE.split('\n').count(), 7);
    assert!(html_export::KATEX_NOTICE.ends_with("-->"));
    lacks(html_export::KATEX_NOTICE, "\r", "");
    has(html_export::KATEX_NOTICE, "SIL Open Font", "the OFL obligation the fonts travel under");
}

#[test]
fn the_mermaid_notice_is_the_verbatim_mit_text() {
    let expected = [
        "<!--".to_string(),
        format!("  Diagrams rendered with Mermaid (https://mermaid.js.org) {DASH} MIT License,"),
        "  Copyright (c) 2014-2022 Knut Sveidqvist. The diagram SVG carries".to_string(),
        "  Mermaid's own theme stylesheet.".to_string(),
        "-->".to_string(),
    ]
    .join("\n");
    assert_eq!(html_export::MERMAID_NOTICE, expected);
    assert_eq!(html_export::MERMAID_NOTICE.split('\n').count(), 5);
    lacks(html_export::MERMAID_NOTICE, "\r", "");
    // A straight apostrophe, not U+2019: the ports all spell it this way.
    has(html_export::MERMAID_NOTICE, "Mermaid's own theme stylesheet.", "");
}

#[test]
fn the_capture_script_is_the_dom_read_the_app_must_run() {
    let expected = [
        "(function () {",
        "  document.querySelectorAll('script, link[rel=\"stylesheet\"]').forEach(function (el) {",
        "    el.remove();",
        "  });",
        "  // A stale completion flag would be misleading in a file that has",
        "  // nothing left to complete.",
        "  document.documentElement.removeAttribute('data-md-render-complete');",
        "  return document.documentElement.outerHTML;",
        "})()",
    ]
    .join("\n");
    assert_eq!(html_export::CAPTURE_SCRIPT, expected);
    lacks(html_export::CAPTURE_SCRIPT, "\r", "a CRLF checkout must not reach the DOM");
    assert_eq!(html_export::DOCTYPE, "<!DOCTYPE html>\n");
}

// MARK: - the document the app loads

#[test]
fn the_export_document_swaps_the_page_break_rule_for_the_one_a_reader_can_see() {
    let document = html_export::export_document("a\n\n\\newpage\n\nb", "T");
    // The marker survives; only the rule changes, so the break is visible in a scrolled file.
    has(&document, "<div class=\"md-pagebreak\"></div>", "");
    has(&document, html_export::SCREEN_PAGE_BREAK_RULE, "");
    lacks(&document, "break-after: page", "invisible on screen");
    lacks(&document, html_export::EXPORT_PAGE_BREAK_RULE, "");
    // The light border colour is hard-coded: an export page is always light.
    has(&document, "rgba(43,38,32,0.16)", "");
    lacks(&document, "rgba(231,219,194,0.16)", "");
}

#[test]
fn the_page_break_swap_takes_every_occurrence_unlike_the_padding_rewrite() {
    // The rule has no <, > or & in it, so a code block quoting it reaches the markup verbatim.
    let source = "See:\n\n```\n.md-pagebreak { height: 0; margin: 0; break-after: page; }\n```\n";
    let raw = html::document(source, "T", false, true);
    assert_eq!(count(&raw, html_export::EXPORT_PAGE_BREAK_RULE), 2);

    let document = html_export::export_document(source, "T");
    assert_eq!(count(&document, html_export::EXPORT_PAGE_BREAK_RULE), 0);
    assert_eq!(count(&document, html_export::SCREEN_PAGE_BREAK_RULE), 2);
    // Contrast: a document quoting the PDF margin rule the same way keeps its copy, because that
    // rewrite touches only the first occurrence. Both asymmetries are deliberate.
    let quotes_the_margin = html::document("```\nbody { padding: 48px 56px; }\n```\n", "T", false, true);
    assert_eq!(count(&quotes_the_margin, pdf_export::BODY_PADDING_RULE), 2);
    assert_eq!(
        count(&pdf_export::styled_for_export(&quotes_the_margin, &PageSize::A5), pdf_export::BODY_PADDING_RULE),
        1
    );
}

#[test]
fn the_export_document_is_always_the_light_paper_page_and_carries_a4_margins() {
    let document = html_export::export_document("hello", "T");
    has(&document, "html, body { background: #FFFFFF; }", "");
    has(&document, ":root { color-scheme: light; }", "");
    has(&document, "<body data-md-dark=\"0\">", "");
    lacks(&document, "#241E18", "the dark paper");
    // No window theme reaches it: `document` collapses `dark && !export` before the palette.
    assert_eq!(html::document("hello", "T", true, true), html::document("hello", "T", false, true));
    // And no trim size reaches it either: an HTML file is not paper.
    has(&document, pdf_export::BODY_PADDING_RULE, "A4's margin, whatever md.pdfPageSize says");

    // WHICH SHEET: the export variant, byte for byte as md.vscode recorded it, with exactly one
    // rule swapped. Anchored to the golden file rather than to a second call of the same code.
    let sheet = golden_export_sheet();
    has(&document, &format!("<style>{}</style>", html_export::visible_page_breaks(&sheet)), "the export sheet");
    assert_eq!(count(&document, "<style>"), 1);
}

// MARK: - the two gates

#[test]
fn needs_katex_asks_the_input_document_not_the_captured_page() {
    let math = html_export::export_document("Formula $a^2$ here.", "T");
    assert!(html_export::needs_katex(&math));
    has(&math, "<link rel=\"stylesheet\" href=\"rich/katex.min.css\">", "");

    // By capture time the link is gone from the DOM. Asking the page would gate every math export
    // off and no exported document would ever carry the stylesheet.
    let captured = capture(&math);
    assert!(!html_export::needs_katex(&captured));
    lacks(&captured, "rich/", "");

    assert!(!html_export::needs_katex(&html_export::export_document("no maths here", "T")));
    // Currency is not math, so it pulls nothing in (the writer's rule, pinned again here).
    assert!(!html_export::needs_katex(&html_export::export_document("it costs $5 and $10 today", "T")));
    assert_eq!(html_export::KATEX_CSS_ASSET, "rich/katex.min.css");
}

#[test]
fn the_mermaid_notice_is_gated_on_the_captured_page_not_the_input() {
    let reader = assets(&[]);
    let mermaid = html_export::export_document("```mermaid\ngraph TD\nA-->B\n```\n", "T");
    let page = html_export::prepare_page_for_document(&capture(&mermaid), &mermaid, &reader).unwrap();
    has(&page, html_export::MERMAID_NOTICE, "");
    has(&page, &format!("{}\n</head>", html_export::MERMAID_NOTICE), "inserted immediately before </head>");

    // Graphviz and PlantUML bake geometry, not Mermaid's own stylesheet: no notice for them.
    let dot = html_export::export_document("```dot\ndigraph { a -> b }\n```\n", "T");
    lacks(&html_export::prepare_page_for_document(&capture(&dot), &dot, &reader).unwrap(), "mermaid.js.org", "");
    let plain = html_export::export_document("hello", "T");
    lacks(&html_export::prepare_page_for_document(&capture(&plain), &plain, &reader).unwrap(), "mermaid.js.org", "");
    assert_eq!(html_export::MERMAID_CONTAINER_NEEDLE, "class=\"mermaid\"");
}

// MARK: - the finished file

#[test]
fn prepare_page_puts_back_the_doctype_that_outer_html_drops() {
    // Without one every browser renders the file in quirks mode; the macOS test reads
    // `document.compatMode == "CSS1Compat"` back out of a real load.
    let page = html_export::prepare_page("<html><head></head><body>x</body></html>", &assets(&[]), false).unwrap();
    assert_eq!(page, "<!DOCTYPE html>\n<html><head></head><body>x</body></html>");
    assert!(page.starts_with("<!DOCTYPE html>\n"));
}

#[test]
fn an_empty_capture_is_rejected_rather_than_written_to_a_file() {
    // The app's "The rendered page could not be captured." case: a file with a doctype and
    // nothing else would look like a successful export. md.win throws ArgumentException; here it
    // is an error value the caller cannot ignore. md.win's six ArgumentNullException lines (a
    // null capture, reader or page) have no Rust spelling: `&str` and `&dyn Fn` cannot be null,
    // so the compiler rejects those calls.
    assert_eq!(html_export::prepare_page("", &assets(&[]), false), Err(EmptyCapture));
    assert_eq!(html_export::prepare_page("", &assets(&[]), true), Err(EmptyCapture));
    let document = html_export::export_document("x", "T");
    assert_eq!(html_export::prepare_page_for_document("", &document, &assets(&[])), Err(EmptyCapture));
    assert_eq!(EmptyCapture.to_string(), "The rendered page could not be captured.");
}

#[test]
fn a_plain_document_carries_no_engine_and_no_font_payload() {
    let document = html_export::export_document("# Title\n\nJust prose, no formulas.\n", "plain");
    let reads = RefCell::new(Vec::<String>::new());
    let reader = |name: &str| {
        reads.borrow_mut().push(name.to_string());
        read_bundled_asset(name)
    };
    let page = html_export::prepare_page_for_document(&capture(&document), &document, &reader).unwrap();

    lacks(&page, "data:font/woff2", "no font payload without math");
    lacks(&page, "rich/", "no engine, no asset folder");
    lacks(&page, "<script", "nothing left to run");
    lacks(&page, "<link", "nothing left to fetch");
    has(&page, "Just prose", "");
    assert!(page.starts_with("<!DOCTYPE html>\n"));
    // The 300 KB stylesheet is not even read for a document that has no math.
    assert!(reads.borrow().is_empty());
    // macOS bounds the plain export at 100 KB; ours is the same page minus the engines.
    assert!(page.len() < 100_000, "plain export should stay small");
}

#[test]
fn a_plot_only_document_carries_no_engine_and_no_font_payload() {
    // A plot is finished SVG in the bytes the page is loaded from: no engine, no wait, no fonts.
    let source = "# Two figures\n\n```plot\nx: -10..10\ny: -2..2\ntitle: Damped oscillation\nsin(x) * exp(-abs(x)/5)\n```\n\n```plot\nx: -4..4\ny: -1..3\n(x>0)*sqrt(x)\n```\n";
    let document = html_export::export_document(source, "plots");
    has(&document, "<div class=\"plot\"><svg", "");
    for engine in
        ["katex.min.js", "mhchem.min.js", "mermaid.min.js", "viz-global.js", "viz-standalone", "highlight.min.js"]
    {
        lacks(&document, engine, "a plot needs no engine");
    }

    let page = html_export::prepare_page_for_document(&capture(&document), &document, &read_bundled_asset).unwrap();
    assert!(page.starts_with("<!DOCTYPE html>\n"));
    lacks(&page, "data:font/woff2", "");
    lacks(&page, "rich/", "");
    lacks(&page, "<script", "");
    lacks(&page, "<link", "");
    assert!(count(&page, "<svg") >= 2, "both figures survive");
    assert!(count(&page, "<polyline") >= 2, "both series survive");
}

// MARK: - the KaTeX stylesheet

#[test]
fn embedded_katex_css_inlines_every_face_as_woff2() {
    // The real bundled assets, read the way the package will read them.
    assert!(read_bundled_asset(html_export::KATEX_CSS_ASSET).is_some());
    let css = html_export::embedded_katex_css(&read_bundled_asset).expect("the vendored sheet");

    lacks(&css, "url(fonts/", "no dead relative link survives");
    has(&css, "url(data:font/woff2;base64,", "");
    lacks(&css, "format(\"woff\")", "the alternates are dead weight off this machine");
    lacks(&css, "format(\"truetype\")", "");
    // Every face, and only woff2: twenty rules, twenty data URIs.
    assert_eq!(count(&css, "@font-face"), count(&css, "url(data:font/woff2;base64,"));
    assert_eq!(count(&css, "@font-face"), 20);
    has(&css, ".katex", "still a stylesheet, not just fonts");
    // Base64 with no line breaks: a wrapped payload would put newlines inside a url() value.
    let source = String::from_utf8(read_bundled_asset(html_export::KATEX_CSS_ASSET).unwrap()).unwrap();
    assert_eq!(count(&css, "\n"), count(&source, "\n"));
    lacks(&css, "\r", "");
}

#[test]
fn the_woff2_run_swallows_the_woff_and_truetype_alternates() {
    // The `[^;}]*` tail eats the sibling alternates, so the whole `src` run is replaced.
    let sheet = "@font-face{font-family:KaTeX_Main;src:url(fonts/KaTeX_Main-Regular.woff2) format(\"woff2\"),url(fonts/KaTeX_Main-Regular.woff) format(\"woff\"),url(fonts/KaTeX_Main-Regular.ttf) format(\"truetype\")}.katex{font-size:1.21em}";
    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, sheet),
        ("rich/fonts/KaTeX_Main-Regular.woff2", "MZ"),
    ]));

    assert_eq!(
        css.as_deref(),
        Some(
            "@font-face{font-family:KaTeX_Main;src:url(data:font/woff2;base64,TVo=) format(\"woff2\")}.katex{font-size:1.21em}"
        )
    );
}

#[test]
fn a_missing_face_leaves_its_rule_alone_rather_than_emit_a_broken_data_uri() {
    // A relative path that is merely absent still degrades to a fallback face; a truncated data:
    // URI is a parse error that takes the surrounding rules with it.
    let sheet = concat!(
        "@font-face{src:url(fonts/Present.woff2) format(\"woff2\")}",
        "@font-face{src:url(fonts/Absent.woff2) format(\"woff2\"),url(fonts/Absent.ttf) format(\"truetype\")}",
    );
    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, sheet),
        ("rich/fonts/Present.woff2", "x"),
    ]));

    assert_eq!(
        css.as_deref(),
        Some(concat!(
            "@font-face{src:url(data:font/woff2;base64,eA==) format(\"woff2\")}",
            "@font-face{src:url(fonts/Absent.woff2) format(\"woff2\"),url(fonts/Absent.ttf) format(\"truetype\")}",
        ))
    );
}

#[test]
fn a_missing_stylesheet_is_null_and_the_export_goes_on_without_it() {
    // Unstyled formulas beat no file. (Kotlin pins the same two cases.)
    assert_eq!(html_export::embedded_katex_css(&assets(&[])), None);
    // Not valid UTF-8: treated as missing, as Foundation's String(contentsOf:encoding:) does.
    let invalid = |name: &str| (name == html_export::KATEX_CSS_ASSET).then(|| vec![0xFF, 0xFE, 0x41]);
    assert_eq!(html_export::embedded_katex_css(&invalid), None);

    let document = html_export::export_document("Formula $a^2$ here.", "T");
    let page = html_export::prepare_page_for_document(&capture(&document), &document, &assets(&[])).unwrap();
    assert_eq!(page, format!("<!DOCTYPE html>\n{}", capture(&document)));
    lacks(&page, "SIL Open Font", "no fonts shipped, no font notice");
}

#[test]
fn an_empty_face_file_is_a_file_not_a_missing_one() {
    // Parity with Foundation's Data(contentsOf:), which reads a zero-byte file happily. Only a
    // `None` read means "absent"; do not "fix" this into a length check.
    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, "@font-face{src:url(fonts/Empty.woff2) format(\"woff2\")}"),
        ("rich/fonts/Empty.woff2", ""),
    ]));
    assert_eq!(css.as_deref(), Some("@font-face{src:url(data:font/woff2;base64,) format(\"woff2\")}"));
}

#[test]
fn only_ascii_face_names_are_rewritten() {
    // The pattern spells its class out: [A-Za-z0-9_-]. A name outside it is not a KaTeX face and
    // must be left alone rather than sent through a lookup no culture agrees on.
    let sheet = concat!(
        "@font-face{src:url(fonts/KaTeX_\u{0130}.woff2) format(\"woff2\")}",
        "@font-face{src:url(fonts/\u{FF21}wide.woff2) format(\"woff2\")}",
        "@font-face{src:url(fonts/Ok-1_2.woff2) format(\"woff2\")}",
    );
    // The non-ASCII faces are READABLE. Without them the test could not tell "the pattern did not
    // match" from "it matched and the file was missing" — see the adversarial suite for the wider
    // pin.
    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, sheet),
        ("rich/fonts/KaTeX_\u{0130}.woff2", "z"),
        ("rich/fonts/\u{FF21}wide.woff2", "z"),
        ("rich/fonts/Ok-1_2.woff2", "y"),
    ]))
    .unwrap();

    assert_eq!(count(&css, "url(data:font/woff2;base64,"), 1);
    has(&css, "url(fonts/KaTeX_\u{0130}.woff2)", "");
    has(&css, "url(fonts/\u{FF21}wide.woff2)", "");
}

#[test]
fn generated_text_is_spliced_never_read_as_a_replacement_template() {
    // A stylesheet holding $&, $1 or $$ would be silently mangled by a regex replacement string.
    let css = "a::after{content:\"$& $1 $$ $` $'\"}";
    let page = html_export::with_embedded_katex("<html><head></head><body></body></html>", css);
    has(&page, &format!("<style>{css}</style>"), "");
    has(&page, &format!("<style>{css}</style>\n{}\n</head>", html_export::KATEX_NOTICE), "");
}

// MARK: - the whole pipeline

#[test]
fn a_math_export_carries_the_stylesheet_and_both_licence_notices() {
    let source = "Formula $a^2 + b^2 = c^2$ and\n\n$$\\int_0^1 x\\,dx$$\n\n```mermaid\ngraph TD\nA-->B\n```\n";
    let document = html_export::export_document(source, "math");
    let page = html_export::prepare_page_for_document(&capture(&document), &document, &read_bundled_asset).unwrap();

    assert!(page.starts_with("<!DOCTYPE html>\n"));
    has(&page, "SIL Open Font", "the OFL notice travels with the faces");
    has(&page, "url(data:font/woff2;base64,", "");
    lacks(&page, "url(fonts/", "");
    lacks(&page, "rich/", "no engine, no stylesheet link");
    lacks(&page, "<script", "");
    lacks(&page, "<link", "");

    // With both, the head ends: …</style>\n(katex notice)\n(mermaid notice)\n</head>
    has(&page, &format!("</style>\n{}\n{}\n</head>", html_export::KATEX_NOTICE, html_export::MERMAID_NOTICE), "");
    assert_eq!(count(&page, "</head>"), 1);
    assert_eq!(count(&page, html_export::KATEX_NOTICE), 1);
    assert_eq!(count(&page, html_export::MERMAID_NOTICE), 1);
}

#[test]
fn the_insertion_touches_the_head_and_nothing_else() {
    let document = html_export::export_document("Formula $a^2$ here.", "T");
    let captured = capture(&document);
    let page = html_export::prepare_page_for_document(&captured, &document, &read_bundled_asset).unwrap();

    // Everything before and after the insertion point is the captured page, byte for byte.
    let at = captured.find("</head>").unwrap();
    assert!(at > 0);
    assert!(page.starts_with(&format!("<!DOCTYPE html>\n{}", &captured[..at])));
    assert!(page.ends_with(&captured[at..]));
}

#[test]
fn the_overload_reads_the_math_gate_off_the_document_the_app_loaded() {
    let document = html_export::export_document("Formula $a^2$ here.", "T");
    let captured = capture(&document);
    assert_eq!(
        html_export::prepare_page(&captured, &read_bundled_asset, true),
        html_export::prepare_page_for_document(&captured, &document, &read_bundled_asset)
    );

    let plain = html_export::export_document("no maths", "T");
    assert_eq!(
        html_export::prepare_page(&capture(&plain), &read_bundled_asset, false),
        html_export::prepare_page_for_document(&capture(&plain), &plain, &read_bundled_asset)
    );
}

#[test]
fn the_export_is_the_same_bytes_under_any_culture() {
    // md.win's [Theory] over tr-TR, de-DE, ar-SA and fa-IR, switching CultureInfo.CurrentCulture
    // around the second run. Rust has no ambient culture for anything here to consult, so each
    // case is the same two runs compared — kept so that a locale-reading change is a red test.
    let source = "# Title\n\nFormula $a^2$ here.\n\n```mermaid\ngraph TD\nA-->B\n```\n\n\\newpage\n\nAfter.";
    let reader = assets(&[
        (
            html_export::KATEX_CSS_ASSET,
            "@font-face{src:url(fonts/KaTeX_Main-Regular.woff2) format(\"woff2\"),url(fonts/KaTeX_Main-Regular.ttf) format(\"truetype\")}.katex{font-size:1.21em}",
        ),
        ("rich/fonts/KaTeX_Main-Regular.woff2", "MZ"),
    ]);

    let document = html_export::export_document(source, "T");
    let expected = html_export::prepare_page_for_document(&capture(&document), &document, &reader).unwrap();

    for _culture in ["tr-TR", "de-DE", "ar-SA", "fa-IR"] {
        let under_culture = html_export::export_document(source, "T");
        assert_eq!(document, under_culture);
        assert_eq!(
            expected,
            html_export::prepare_page_for_document(&capture(&under_culture), &under_culture, &reader).unwrap()
        );
        assert!(html_export::needs_katex(&under_culture));
        has(&expected, "url(data:font/woff2;base64,TVo=)", "");
    }
}
