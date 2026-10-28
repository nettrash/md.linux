//! The string half of a PDF export: the one declaration that scales with the chosen trim size.
//! Port of md.win `PdfExportTests.cs`, itself the margin cases in mdTests.swift (the padding table
//! is in the page-size tests; these pin the rewrite that consumes it) plus the export-CSS cases
//! `testHTMLPageBreakMarkerAndExportCSS` / `testExportPageIsPlainWhiteAndAlwaysLight` seen from the
//! paper side. The macOS PDF tests that read a real MediaBox and count pages need a WebView and are
//! the app's; nothing here renders anything.

mod common;
#[path = "common/export_html.rs"]
mod html_common;

use std::borrow::Cow;

use html_common::{count, has, lacks, replace_first};
use md_core::document::page_size::PageSize;
use md_core::export::{html_export, pdf_export};
use md_core::markdown::html;

fn doc(source: &str, dark: bool, export: bool) -> String {
    html::document(source, "T", dark, export)
}

fn export_doc(source: &str) -> String {
    doc(source, false, true)
}

/// md.win's `Assert.Same`: the very input, handed back without a copy.
#[track_caller]
fn assert_same(input: &str, output: Cow<'_, str>) {
    match output {
        Cow::Borrowed(returned) => assert!(std::ptr::eq(input, returned), "a different string came back"),
        Cow::Owned(copy) => panic!("expected the input itself back, got a copy: {copy}"),
    }
}

#[test]
fn a4_is_the_historical_margin_so_an_a4_export_is_byte_for_byte_what_it_always_was() {
    let html = export_doc("# Title\n\nProse.");
    // The replacement text equals the original, so trim sizes cost the default nothing.
    assert_eq!(html, pdf_export::styled_for_export(&html, &PageSize::A4));
    assert_eq!(pdf_export::BODY_PADDING_RULE, "padding: 48px 56px;");
    has(&html, &format!("    {}", pdf_export::BODY_PADDING_RULE), "the sheet's body rule");
}

#[test]
fn every_trim_size_rewrites_the_body_margin_to_its_own_scaled_padding() {
    let html = export_doc("# Title\n\nProse.");
    let expected = [
        (PageSize::A4, "48px 56px"),
        (PageSize::A5, "34px 39px"),
        (PageSize::US_LETTER, "45px 58px"),
        (PageSize::US_LEGAL, "57px 58px"),
        (PageSize::SIX_BY_NINE, "37px 41px"),
        (PageSize::FIVE_BY_EIGHT, "33px 34px"),
        (PageSize::DIGEST, "35px 37px"),
    ];

    for (size, padding) in expected {
        let styled = pdf_export::styled_for_export(&html, &size);
        assert_eq!(size.css_padding(), padding);
        has(&styled, &format!("padding: {padding};"), size.id);
        assert_eq!(count(&styled, &format!("padding: {padding};")), 1);
        // Exactly one body rule, whichever size: the needle is gone unless it IS the answer.
        assert_eq!(count(&styled, pdf_export::BODY_PADDING_RULE), if size.id == "a4" { 1 } else { 0 });
    }
}

#[test]
fn only_the_first_occurrence_is_rewritten_so_a_quoted_rule_in_a_code_block_survives() {
    // The rule contains no <, > or & — a code block quoting it reaches the markup verbatim, so this
    // is a document a user could really write, not a contrived string.
    let html = export_doc("Look:\n\n```\nbody { padding: 48px 56px; }\n```\n");
    assert_eq!(count(&html, pdf_export::BODY_PADDING_RULE), 2);

    let styled = pdf_export::styled_for_export(&html, &PageSize::SIX_BY_NINE);
    has(&styled, "padding: 37px 41px;", "the head's body rule");
    // The author's text is untouched: the sheet always precedes any user content.
    assert_eq!(count(&styled, pdf_export::BODY_PADDING_RULE), 1);
    has(&styled, "body { padding: 48px 56px; }", "the code block");
    assert!(styled.find("padding: 37px 41px;").unwrap() < styled.find(pdf_export::BODY_PADDING_RULE).unwrap());
}

#[test]
fn the_margin_is_the_only_thing_that_scales_with_the_paper() {
    let html = export_doc("# Title\n\nProse.\n\n```\ncode\n```\n\n\\newpage\n\nAfter.");
    for size in PageSize::all() {
        let styled = pdf_export::styled_for_export(&html, size);
        let padding = format!("padding: {};", size.css_padding());
        // Put the A4 rule back and the two documents are the same bytes: nothing else moved.
        assert_eq!(html, replace_first(&styled, &padding, pdf_export::BODY_PADDING_RULE));

        // The rest of the paper styling is the `export: true` sheet, identical on every size.
        has(&styled, "font-size: 11pt;", size.id);
        has(&styled, "pre { white-space: pre-wrap; overflow-wrap: anywhere; }", size.id);
        has(&styled, "html, body { background: #FFFFFF; }", size.id);
        has(&styled, ":root { color-scheme: light; }", size.id);
        has(&styled, ".md-pagebreak { height: 0; margin: 0; break-after: page; }", size.id);
        has(
            &styled,
            "* { -webkit-print-color-adjust: exact; print-color-adjust: exact; box-sizing: border-box; }",
            size.id,
        );
        has(&styled, "<body data-md-dark=\"0\">", size.id);
        has(&styled, "<div class=\"md-pagebreak\"></div>", size.id);

        // WHICH SHEET: the export variant, byte for byte as md.vscode recorded it, with exactly one
        // declaration rewritten. Anchored to the golden file, not to a second call.
        let mut sheet = common::fixture_text("golden/stylesheet-export.css");
        sheet.pop();
        has(
            &styled,
            &format!("<style>{}</style>", replace_first(&sheet, pdf_export::BODY_PADDING_RULE, &padding)),
            size.id,
        );
        assert_eq!(count(&styled, "<style>"), 1);
    }
}

#[test]
fn an_html_without_the_body_rule_comes_back_unchanged() {
    let plain = "<p>no stylesheet here</p>";
    assert_same(plain, pdf_export::styled_for_export(plain, &PageSize::DIGEST));
    assert_same(plain, pdf_export::styled_for_export(plain, &PageSize::A4));
    assert_same(plain, pdf_export::styled_for_export(plain, &PageSize::SIX_BY_NINE));
}

#[test]
fn the_export_sheet_is_the_paper_variant_whatever_the_window_theme() {
    // `export: true` collapses `dark`, so the paper is white and the ink dark on both paths, and
    // Mermaid and PlantUML read `data-md-dark="0"` and render light too.
    for dark in [false, true] {
        let styled = pdf_export::styled_for_export(&doc("hello", dark, true), &PageSize::FIVE_BY_EIGHT).into_owned();
        has(&styled, "html, body { background: #FFFFFF; }", "");
        has(&styled, ":root { color-scheme: light; }", "");
        has(&styled, "<body data-md-dark=\"0\">", "");
        lacks(&styled, "#241E18", "the dark paper");
        lacks(&styled, "rgba(231,219,194,0.16)", "the dark border");
        lacks(&styled, "font-size: 13pt;", "the screen body size");
    }
}

#[test]
fn the_pdf_page_size_reaches_neither_the_preview_nor_the_html_export_nor_the_sheet() {
    // The stylesheet itself knows nothing about trim sizes: it always writes A4's margin, and only
    // the two PDF paths rewrite it. `md.pdfPageSize` is a PDF-file setting.
    has(&html::css(false, true), pdf_export::BODY_PADDING_RULE, "the export sheet");
    has(&html::css(false, false), pdf_export::BODY_PADDING_RULE, "the light sheet");
    has(&html::css(true, false), pdf_export::BODY_PADDING_RULE, "the dark sheet");
    has(&doc("hi", false, false), pdf_export::BODY_PADDING_RULE, "the preview");
    has(&html_export::export_document("hi", "T"), pdf_export::BODY_PADDING_RULE, "the HTML export");
}

#[test]
fn the_page_box_names_the_trim_size_in_points_and_zeroes_the_printer_margin() {
    assert_eq!(
        pdf_export::page_box_css(&PageSize::A4),
        concat!(
            "<style>@page { size: 595.2pt 841.8pt; margin: 0; }\n",
            "html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }</style>",
        )
    );
    // A whole number of points carries no decimal point, exactly as the TypeScript writes it.
    has(&pdf_export::page_box_css(&PageSize::US_LETTER), "size: 612pt 792pt;", "");
    has(&pdf_export::page_box_css(&PageSize::SIX_BY_NINE), "size: 432pt 648pt;", "");
    has(&pdf_export::page_box_css(&PageSize::A5), "size: 419.5pt 595.3pt;", "");
    has(&pdf_export::page_box_css(&PageSize::DIGEST), "size: 396pt 612pt;", "");
}

#[test]
fn the_paper_styling_is_the_same_bytes_under_any_culture() {
    // md.win's [Theory] over tr-TR, de-DE, ar-SA and fa-IR, switching CultureInfo.CurrentCulture:
    // a German culture would write `595,2pt` and the browser would drop the declaration. Rust has
    // no ambient culture, so each case pins the same invariant bytes.
    for _culture in ["tr-TR", "de-DE", "ar-SA", "fa-IR"] {
        assert_eq!(
            pdf_export::page_box_css(&PageSize::A4),
            concat!(
                "<style>@page { size: 595.2pt 841.8pt; margin: 0; }\n",
                "html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }</style>",
            )
        );
        has(&pdf_export::page_box_css(&PageSize::A5), "size: 419.5pt 595.3pt;", "");

        let styled = pdf_export::styled_for_export(&export_doc("hi"), &PageSize::A5).into_owned();
        has(&styled, "padding: 34px 39px;", "");
        lacks(&styled, pdf_export::BODY_PADDING_RULE, "");
    }
}

#[test]
fn the_page_box_is_appended_after_the_sheet_and_never_woven_into_it() {
    let html = export_doc("hi");
    let with_box = pdf_export::with_page_box(&html, &PageSize::SIX_BY_NINE);

    // The sheet's own bytes survive: the family's stylesheet stays byte-identical everywhere.
    has(&with_box, &format!("<style>{}</style>\n<style>@page", html::css(false, true)), "");
    has(&with_box, "size: 432pt 648pt;", "");
    // Appended, not inserted anywhere else: the head still closes right after it.
    has(&with_box, "print-color-adjust: exact; }</style>\n</head>", "");
    assert_eq!(count(&with_box, "<style>"), 2);

    // No `</style>` to anchor on: unchanged rather than mangled.
    let plain = "<p>plain</p>";
    assert_same(plain, pdf_export::with_page_box(plain, &PageSize::A4));
}

#[test]
#[ignore = "Rust has no null: &str and &PageSize cannot be absent, so every call md.win makes here is a compile error"]
fn null_arguments_are_rejected_rather_than_silently_producing_half_a_page() {
    // md.win asserts ArgumentNullException for StyledForExport(null, A4), StyledForExport("x",
    // null), PageBoxCss(null), WithPageBox(null, A4) and WithPageBox("x", null). The borrow
    // checker makes all five unrepresentable; there is nothing left to run.
}
