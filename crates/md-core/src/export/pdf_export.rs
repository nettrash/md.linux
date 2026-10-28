//! The paper styling of a PDF export: the one rule in the export stylesheet that scales with the
//! chosen trim size. Port of md.win `Md.Core.Export.PdfExport` (itself
//! `DocumentExport.styledForExport` in md.macOS/md/DocumentExport.swift), with md.vscode's `@page`
//! box offered separately for a print pipeline that takes its paper from the document.
//!
//! Everything else about a PDF is the app's: the offscreen `WebView` that lays the page out, the
//! wait for `data-md-render-complete`, `webkit::PrintOperation` and the `gtk::PageSetup` /
//! `gtk::PrintSettings` that carry the paper geometry. This module only rewrites a string.
//!
//! WHAT SCALES WITH THE PAPER: exactly one declaration, the body's `padding`. A 6 × 9" booklet
//! wearing A4 margins would waste a quarter of its width, so each axis is scaled by its own
//! dimension (see [`PageSize::css_padding`]) — US Letter is *wider* than A4, so its horizontal
//! margin grows to 58px. WHAT DOES NOT: the 11pt body size, the white paper, `pre-wrap` code, the
//! collapsed `break-after: page` rule, the print-colour-adjust rule and every colour. They are the
//! `export: true` stylesheet, chosen by `markdown::html::document(…, export: true)` before this
//! module ever sees the string, and identical on every trim size.
//!
//! WHAT IS UNAFFECTED: the preview, the self-contained HTML export and the EPUB. macOS applies
//! `styledForExport` on the two PDF paths only (Share ▸ Rendered PDF, Export ▸ PDF, and the two
//! book equivalents) — never on print, which is always A4, never on the HTML export, whose
//! document keeps A4's `48px 56px`, and never on the EPUB, which lifts the stylesheet out of the
//! export document and adds its own `body { padding: 0.5em 5%; }`. `md.pdfPageSize` is a PDF-file
//! setting; it must not reach any other artifact.
//!
//! Both rewrites hand back [`Cow::Borrowed`] when there is nothing to rewrite — the Rust spelling
//! of md.win returning the very same string instance, which its tests pin with `Assert.Same`.

use std::borrow::Cow;

use crate::document::page_size::PageSize;

/// The body margin as the export stylesheet writes it — the needle rewritten for a trim size. It
/// is one line of the HTML writer's stylesheet (`markdown/css.rs`); the two must be edited
/// together.
pub const BODY_PADDING_RULE: &str = "padding: 48px 56px;";

/// `html` with the body margin rewritten to `page_size`'s scaled padding. For A4 the replacement
/// equals the original, so an A4 export is byte-for-byte what it was before trim sizes existed.
/// Returns the input unchanged (borrowed) when the rule is absent.
///
/// Only the **first** occurrence is touched: that is the body rule inside the head's `<style>`,
/// which always precedes any user content, so a document that happens to quote that exact CSS in
/// a code block keeps its text. (Contrast [`super::html_export::visible_page_breaks`], which
/// deliberately replaces *every* occurrence. The asymmetry is intentional in Swift, Kotlin,
/// TypeScript and C# alike.) Spliced by index: the replacement is generated text.
pub fn styled_for_export<'a>(html: &'a str, page_size: &PageSize) -> Cow<'a, str> {
    let Some(index) = html.find(BODY_PADDING_RULE) else {
        return Cow::Borrowed(html);
    };
    let mut styled = String::with_capacity(html.len() + 8);
    styled.push_str(&html[..index]);
    styled.push_str("padding: ");
    styled.push_str(&page_size.css_padding());
    styled.push(';');
    styled.push_str(&html[index + BODY_PADDING_RULE.len()..]);
    Cow::Owned(styled)
}

/// The CSS page box for `page_size`: md.vscode's `@page` block, for a print pipeline that is
/// asked to take its paper from the document rather than from the print settings. **Opt-in** —
/// see [`with_page_box`] for when not to use it.
///
/// Points are written as .NET's invariant `double.ToString()` writes them (shortest round trip,
/// `612` with no decimal point, `595.2`): md.win's culture hazard — a German culture spelling
/// `595,2pt`, which the browser would drop silently — cannot arise in Rust, but the exponent
/// switch could, so the formatting goes through `invariant_number`.
pub fn page_box_css(page_size: &PageSize) -> String {
    [
        "<style>@page { size: ",
        &invariant_number(page_size.width),
        "pt ",
        &invariant_number(page_size.height),
        "pt; margin: 0; }\n",
        // The Chromium spelling of the `-webkit-print-color-adjust: exact` already in the sheet:
        // without it the paper background and the diagram fills are dropped by the print pipeline
        // and a themed document prints as bare text.
        "html { -webkit-print-color-adjust: exact; print-color-adjust: exact; }</style>",
    ]
    .concat()
}

/// `html` with [`page_box_css`] appended after the first `</style>` — never woven into the sheet,
/// which stays byte-identical to the one every other surface uses. Unchanged (borrowed) when there
/// is no `</style>` to anchor on.
///
/// md.win's PDF path does **not** call this, and neither should this port's. On WebKitGTK the
/// paper is the `gtk::PageSetup` / `gtk::PrintSettings` paper size handed to
/// `webkit::PrintOperation` — the same single source of truth `NSPrintInfo.paperSize` is on macOS
/// and `CoreWebView2PrintSettings.PageWidth/PageHeight` on Windows; a CSS `@page size` would be a
/// second one for the same number. Worse, `margin: 0` from an author sheet overrides the printer
/// margins the app sets, and those are a deliberate decision (0.5 in, Android's reason: the body's
/// padding only wraps the whole document, so a middle page would otherwise touch the paper edge).
///
/// It is kept, and pinned, for the host that has no print settings to set: md.vscode declares the
/// page box because the VS Code host owns the dialog.
pub fn with_page_box<'a>(html: &'a str, page_size: &PageSize) -> Cow<'a, str> {
    const CLOSE: &str = "</style>";
    let Some(index) = html.find(CLOSE) else {
        return Cow::Borrowed(html);
    };
    let after = index + CLOSE.len();
    let mut boxed = String::with_capacity(html.len() + 160);
    boxed.push_str(&html[..after]);
    boxed.push('\n');
    boxed.push_str(&page_box_css(page_size));
    boxed.push_str(&html[after..]);
    Cow::Owned(boxed)
}

/// `double.ToString(CultureInfo.InvariantCulture)` on .NET Core 3.0 and later: the shortest string
/// that round-trips (the digits Rust's `{}` and `{:e}` print too), in fixed notation while the
/// decimal exponent is above −5 and below 15 (`0.0001`, `100000000000000`), and in `E` notation
/// with a sign and at least two exponent digits past either bound (`1E-05`, `1E+15`, `1.5E+300`).
/// Rust's `{}` never uses an exponent, so the switch is spelled out here. Non-finite values are
/// .NET's words.
fn invariant_number(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((scientific.as_str(), "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if value != 0.0 && !(-4..15).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        return format!("{mantissa}E{sign}{:02}", exponent.unsigned_abs());
    }
    format!("{value}")
}

#[cfg(test)]
mod tests {
    use super::invariant_number;

    #[test]
    fn numbers_are_written_as_dotnet_writes_a_double() {
        // The trim sizes this module actually formats.
        assert_eq!(invariant_number(595.2), "595.2");
        assert_eq!(invariant_number(841.8), "841.8");
        assert_eq!(invariant_number(612.0), "612");
        assert_eq!(invariant_number(419.5), "419.5");
        assert_eq!(invariant_number(1008.0), "1008");
        // The switch to E notation, at .NET's bounds.
        assert_eq!(invariant_number(1e14), "100000000000000");
        assert_eq!(invariant_number(1e15), "1E+15");
        assert_eq!(invariant_number(1.2345678901234568e17), "1.2345678901234568E+17");
        assert_eq!(invariant_number(1e21), "1E+21");
        assert_eq!(invariant_number(0.0001), "0.0001");
        assert_eq!(invariant_number(0.00001), "1E-05");
        assert_eq!(invariant_number(1e-7), "1E-07");
        assert_eq!(invariant_number(-2.5e-300), "-2.5E-300");
        assert_eq!(invariant_number(0.0), "0");
        assert_eq!(invariant_number(-0.0), "-0");
        assert_eq!(invariant_number(f64::NAN), "NaN");
        assert_eq!(invariant_number(f64::INFINITY), "Infinity");
        assert_eq!(invariant_number(f64::NEG_INFINITY), "-Infinity");
    }
}
