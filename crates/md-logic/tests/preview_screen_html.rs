//! Port of md.win `tests/Md.App.Logic.Tests/Preview/ScreenHtmlTests.cs`: the typography graft
//! (§4.3). The bytes are pinned because they are the only difference between what a Linux reader
//! sees and what every other port renders; the idempotence is pinned because Print hands the same
//! HTML through two pipelines.
//!
//! LINUX: Nimbus Mono PS in front (docs/linux-facts.md §7) and the id `md-linux-fonts`, where md.win
//! has Lucida Sans Typewriter and `md-win-fonts`.

use std::borrow::Cow;

use md_logic::preview::screen_html::{FONT_STYLE, ID_MARKER, with_linux_fonts};

const PAGE: &str = "<!DOCTYPE html>\n<html><head><title>t</title></head>\n<body>x</body></html>";

#[test]
fn font_style_is_the_exact_bytes_the_design_pins() {
    assert_eq!(
        FONT_STYLE,
        "<style id=\"md-linux-fonts\">body{font-family:\"Nimbus Mono PS\",\"Courier New\",serif;}</style>"
    );
}

#[test]
fn style_goes_on_its_own_line_before_the_first_head_end() {
    assert_eq!(
        with_linux_fonts(PAGE),
        "<!DOCTYPE html>\n<html><head><title>t</title>\n\
         <style id=\"md-linux-fonts\">body{font-family:\"Nimbus Mono PS\",\"Courier New\",serif;}</style>\
         </head>\n<body>x</body></html>"
    );
}

#[test]
fn only_the_first_head_end_is_used() {
    let twice = with_linux_fonts("<head>a</head><head>b</head>");
    assert_eq!(twice, format!("<head>a\n{FONT_STYLE}</head><head>b</head>"));
}

#[test]
fn a_second_call_changes_nothing() {
    let once = with_linux_fonts(PAGE).into_owned();
    assert_eq!(with_linux_fonts(&once), once);
}

#[test]
fn html_that_already_carries_the_id_is_left_alone() {
    // Whatever spelling of the style element the caller already has, one is enough.
    let own = "<html><head><style id=\"md-linux-fonts\">body{}</style></head></html>";
    assert!(matches!(with_linux_fonts(own), Cow::Borrowed(s) if std::ptr::eq(s, own)));
}

#[test]
fn a_fragment_with_no_head_comes_back_untouched() {
    let fragment = "<p>just a body</p>";
    assert!(matches!(with_linux_fonts(fragment), Cow::Borrowed(s) if std::ptr::eq(s, fragment)));
}

#[test]
fn code_stays_courier_new() {
    // The style only touches body: the shared stylesheet's `code, pre` rule is more specific and
    // keeps monospace ("Courier New", which fontconfig maps to Liberation Mono), and KaTeX /
    // Mermaid / PlantUML carry their own faces.
    assert!(FONT_STYLE.contains("body{font-family:"));
    assert!(!FONT_STYLE.contains("code"));
    assert!(!FONT_STYLE.contains("pre"));
}

#[test]
fn nimbus_mono_ps_leads_and_courier_new_is_the_fallback() {
    // LINUX: md.win's LucidaSansTypewriterLeadsAndCourierNewIsTheFallback.
    assert!(FONT_STYLE.contains("\"Nimbus Mono PS\",\"Courier New\",serif"));
    // The face has Regular, Bold, Italic and Bold Italic in fonts-urw-base35, so nothing here is
    // synthesised; the Georgia the other ports fall back to is deliberately NOT named, and neither
    // is the archive's Courier Prime, whose four files fontconfig cannot tell apart.
    assert!(!FONT_STYLE.contains("Georgia"));
    assert!(!FONT_STYLE.contains("Courier Prime"));
}

#[test]
fn a_document_that_mentions_the_marker_in_its_body_loses_the_linux_font() {
    // KNOWN, and pinned so it is a decision rather than a surprise: idempotence is decided by
    // searching the *whole* page for the id, so a reader who writes `id="md-linux-fonts"` in a
    // fenced code block gets a preview in Courier New. Nothing else breaks and no port renders it
    // differently, but if this is ever to be fixed the search has to be scoped to the head.
    let page = format!("<html><head></head><body><pre>&lt;style {ID_MARKER}&gt;</pre></body></html>");

    assert!(matches!(with_linux_fonts(&page), Cow::Borrowed(_)));
    assert!(!with_linux_fonts(&page).contains(FONT_STYLE));
}

#[test]
fn the_marker_is_the_id_of_the_style_it_inserts() {
    // The two constants have to agree or idempotence silently stops working and Print stacks a
    // second copy of the style on every pass.
    assert!(FONT_STYLE.contains(ID_MARKER));
    assert_eq!(ID_MARKER, "id=\"md-linux-fonts\"");
}
