//! Adversarial pins for the HTML and PDF export strings. Port of md.win
//! `PdfhtmlAdversarialTests.cs`, written there by the refuter.
//!
//! EVERY expectation below was measured on a sibling port, never read off the implementation:
//!
//! * SWIFT — `swiftc -O probe.swift` against the real `NSRegularExpression` /
//!   `replacingOccurrences` / `range(of:)` / `String(contentsOf:encoding:)` that
//!   md.macOS/md/DocumentExport.swift uses. Each expectation carries its printed line.
//! * TS — `vitest run` inside md.vscode against `src/export/html.ts` and `src/export/pdf.ts`.
//!
//! Two of these were found by mutation testing in md.win: the whole suite stayed green with the
//! `src` run's terminator class narrowed from `[^;}]` to `[^}]`, and with the face-name class
//! widened from `[A-Za-z0-9_-]` to .NET's Unicode `[\w-]`. Both mutants are byte divergences from
//! every shipping port; both are killed here, against this port's hand-written matcher.

#[path = "common/export_html.rs"]
mod html_common;

use html_common::{assets, count, vendored_katex_css};
use md_core::document::page_size::PageSize;
use md_core::export::{html_export, pdf_export};
use md_core::markdown::html;

const TURKISH_I: char = '\u{0130}'; // LATIN CAPITAL LETTER I WITH DOT ABOVE
const FULLWIDTH_A: char = '\u{FF21}'; // FULLWIDTH LATIN CAPITAL LETTER A

/// The `src` run ends at a `;` as well as at the rule's `}`. The vendored `katex.min.css` happens
/// to put `src` last in every rule, so its twenty faces never exercise the semicolon — and md.win's
/// whole suite stayed green with the terminator narrowed to `[^}]`, which would have swallowed the
/// declarations after it.
///
/// SWIFT probe (`NSRegularExpression`, pattern verbatim from `embeddedKatexCSS`), printed:
/// `@font-face{font-family:KaTeX_Main;src:url(data:font/woff2;base64,TVo=) format("woff2");font-weight:700;font-style:normal}.katex{font-size:1.21em}`
/// A `font-weight` eaten here is not a cosmetic loss: the bold face would be selected for every
/// weight, so a document's bold math would come out of the export in the wrong stroke.
#[test]
fn the_src_run_stops_at_a_semicolon_and_not_only_at_the_rules_closing_brace() {
    let sheet = concat!(
        "@font-face{font-family:KaTeX_Main;",
        "src:url(fonts/KaTeX_Main-Regular.woff2) format(\"woff2\"),",
        "url(fonts/KaTeX_Main-Regular.ttf) format(\"truetype\");",
        "font-weight:700;font-style:normal}.katex{font-size:1.21em}",
    );

    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, sheet),
        ("rich/fonts/KaTeX_Main-Regular.woff2", "MZ"),
    ]));

    assert_eq!(
        css.as_deref(),
        Some(concat!(
            "@font-face{font-family:KaTeX_Main;",
            "src:url(data:font/woff2;base64,TVo=) format(\"woff2\");",
            "font-weight:700;font-style:normal}.katex{font-size:1.21em}",
        ))
    );
}

/// A face whose name leaves `[A-Za-z0-9_-]` is not a KaTeX face, and its rule is left exactly as
/// written — even when a file of that name is sitting in the package.
///
/// The class is spelled out in every port. A Unicode-aware `[\w-]` would match `KaTeX_İ` and
/// `Ａwide` and inline them — a byte divergence, and a lookup key that a Turkish or a
/// full-width-folding culture disagrees about. Here the files ARE supplied, so only the class
/// decides ("not matched" and "matched, file missing" would otherwise produce the same output).
///
/// SWIFT probe printed, with `KaTeX_İ` present in the font map:
/// `@font-face{src:url(fonts/KaTeX_İ.woff2) format("woff2")}` — untouched.
#[test]
fn a_face_name_outside_the_ascii_class_is_left_alone_even_when_its_file_is_right_there() {
    let sheet = format!(
        "@font-face{{src:url(fonts/KaTeX_{TURKISH_I}.woff2) format(\"woff2\")}}\
         @font-face{{src:url(fonts/{FULLWIDTH_A}wide.woff2) format(\"woff2\")}}\
         @font-face{{src:url(fonts/Ok-1_2.woff2) format(\"woff2\")}}"
    );
    let turkish_face = format!("rich/fonts/KaTeX_{TURKISH_I}.woff2");
    let wide_face = format!("rich/fonts/{FULLWIDTH_A}wide.woff2");

    // Every face is readable. The ASCII one proves the reader works, so a green run cannot mean
    // "nothing was inlined at all".
    let css = html_export::embedded_katex_css(&assets(&[
        (html_export::KATEX_CSS_ASSET, &sheet),
        (&turkish_face, "z"),
        (&wide_face, "z"),
        ("rich/fonts/Ok-1_2.woff2", "y"),
    ]));

    assert_eq!(
        css,
        Some(format!(
            "@font-face{{src:url(fonts/KaTeX_{TURKISH_I}.woff2) format(\"woff2\")}}\
             @font-face{{src:url(fonts/{FULLWIDTH_A}wide.woff2) format(\"woff2\")}}\
             @font-face{{src:url(data:font/woff2;base64,eQ==) format(\"woff2\")}}"
        ))
    );
}

/// Both head insertions take **every** `</head>`, which is Swift's `replacingOccurrences` — not
/// Kotlin's and TypeScript's `replaceFirst`.
///
/// A rendered document has exactly one `</head>` (every scrap of author text is escaped), so the
/// ports agree on every real page and the choice is invisible — until somebody "simplifies" it in
/// one direction or the other. The Swift is the source of truth for this module's bytes.
///
/// SWIFT probe on `<html><head></head><head></head>…`: `two-head insert count: 2`. TS probe, same
/// input through `selfContainedHTML`: `mermaid notice count: 1`.
#[test]
fn both_head_insertions_take_every_occurrence_like_the_swift_and_not_like_the_type_script() {
    let captured =
        concat!("<html><head></head><head></head>", "<body><pre class=\"mermaid\"><svg></svg></pre></body></html>");
    let page = html_export::prepare_page(
        captured,
        &assets(&[(html_export::KATEX_CSS_ASSET, ".katex{font-size:1.21em}")]),
        true,
    )
    .unwrap();

    assert_eq!(count(&page, "</head>"), 2);
    assert_eq!(count(&page, html_export::KATEX_NOTICE), 2); // Swift: 2. Kotlin/TS: 1.
    assert_eq!(count(&page, html_export::MERMAID_NOTICE), 2); // Swift: 2. Kotlin/TS: 1.
    assert_eq!(count(&page, "<style>.katex{font-size:1.21em}</style>"), 2);
    // …and the order inside each head is still stylesheet, KaTeX notice, Mermaid notice.
    assert_eq!(
        count(
            &page,
            &format!(
                "<style>.katex{{font-size:1.21em}}</style>\n{}\n{}\n</head>",
                html_export::KATEX_NOTICE,
                html_export::MERMAID_NOTICE
            )
        ),
        2
    );
}

/// A document whose *title* quotes the body rule has its title rewritten and its stylesheet left
/// alone. Ugly, shared by every port, and deliberately not "fixed" here.
///
/// The rewrite takes the first `padding: 48px 56px;` in the file, and the skeleton puts `<title>`
/// ahead of `<style>`. The rule contains no `<`, `>` or `&`, so a title made of it survives
/// escaping verbatim and gets there first. A document named after a CSS declaration is absurd; a
/// port that quietly disagreed with the others about which occurrence wins is not.
///
/// SWIFT probe (`range(of:)` + `replacingCharacters`) on
/// `<title>padding: 48px 56px;</title><style>body { padding: 48px 56px; }</style>` printed
/// `<title>padding: 34px 39px;</title><style>body { padding: 48px 56px; }</style>`. TS probe on
/// the real A5 document: styled title `padding: 34px 39px;`, remaining body-rule count 1,
/// a5-padding count 1.
#[test]
fn a_document_titled_like_the_body_rule_has_its_title_rewritten_and_its_sheet_spared() {
    let html = html::document("hello", pdf_export::BODY_PADDING_RULE, false, true);
    assert!(html.find("<title>").unwrap() < html.find("<style>").unwrap(), "the skeleton puts the title first");

    let styled = pdf_export::styled_for_export(&html, &PageSize::A5);

    assert!(styled.contains("<title>padding: 34px 39px;</title>"));
    assert_eq!(count(&styled, "padding: 34px 39px;"), 1);
    // The sheet keeps A4's margin: the one occurrence the rewrite was aiming at was spent.
    assert_eq!(count(&styled, pdf_export::BODY_PADDING_RULE), 1);
    assert!(styled[styled.find("<style>").unwrap()..].contains("padding: 48px 56px;"));
}

/// "Self-contained" is engine- and font-self-contained, **not** asset-self-contained: the author's
/// relative `<img>` is still relative in the exported file.
///
/// A known, accepted gap on every port (html.ts spells it out: "closing it here alone would be a
/// parity break, not a bug fix"). It is pinned because it looks exactly like a bug and is the
/// obvious thing for a future contributor to "fix" on one platform. TS probe of
/// `![cat](photo.png)`: `<img src="photo.png" alt="cat">`.
#[test]
fn the_authors_relative_image_stays_relative_in_the_exported_file() {
    let document = html_export::export_document("![cat](photo.png)", "T");
    assert!(document.contains("src=\"photo.png\""));

    // The capture is the live DOM with the scripts and stylesheet links gone; an <img> is neither,
    // so it survives untouched into the file.
    let page = html_export::prepare_page_for_document(&document["<!DOCTYPE html>\n".len()..], &document, &assets(&[]))
        .unwrap();
    assert!(page.contains("src=\"photo.png\""));
    assert!(!page.contains("data:image"));
    assert!(!page.contains("base64"));
}

/// KNOWN DIVERGENCE, unreachable with the shipped asset: a UTF-8 BOM on `katex.min.css` reaches the
/// exported page here, where Foundation would have eaten it.
///
/// SWIFT probe: `String(contentsOf:encoding:.utf8)` of `EF BB BF ".katex{a:b}"` returns a non-nil
/// 11-character string whose first scalar is U+002E — the BOM is stripped. `String::from_utf8`
/// does not strip it, and neither do .NET's `Encoding.GetString`, Kotlin's
/// `ByteArray.toString(Charsets.UTF_8)` nor Node's `readFileSync(…, 'utf8')`, so this port sides
/// with md.win, Android and md.vscode against the one they all port from.
///
/// It cannot fire today: the vendored `rich/katex.min.css` starts with `@font-fa`, and the same
/// file is vendored byte-identically into every app. This test exists so that a re-vendored,
/// BOM-carrying sheet is a red test rather than a silently dropped first rule in every exported
/// page with math. If it ever fires, strip a leading U+FEFF in every port together — not in one
/// port alone.
#[test]
fn a_bom_on_the_stylesheet_is_carried_into_the_page_where_foundation_would_have_eaten_it() {
    let vendored = std::fs::read(vendored_katex_css()).expect("the vendored KaTeX sheet");
    assert!(
        !vendored.starts_with(&[0xEF, 0xBB, 0xBF]),
        "the vendored sheet must stay BOM-free; the divergence below is only latent"
    );

    let mut bommed = vec![0xEF, 0xBB, 0xBF];
    bommed.extend_from_slice(b".katex{font-size:1.21em}");
    let css =
        html_export::embedded_katex_css(&|name: &str| (name == html_export::KATEX_CSS_ASSET).then(|| bommed.clone()));

    // The leading escape is U+FEFF, spelled out: an invisible literal in a source file is a
    // riddle, and this is the whole point of the assertion.
    assert_eq!(css.as_deref(), Some("\u{FEFF}.katex{font-size:1.21em}")); // Swift returns ".katex{font-size:1.21em}"
}
