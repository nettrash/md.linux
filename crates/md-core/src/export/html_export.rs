//! The pure half of "Export as HTML" — one self-contained file that opens anywhere with no
//! engines, no folder of assets and no network. Port of md.win `Md.Core.Export.HtmlExport`
//! (itself `DocumentExport.renderedHTMLPage` / `WebRenderer.selfContainedHTML` in
//! md.macOS/md/DocumentExport.swift), split the way md.Android splits it (`markdown/HtmlExport.kt`
//! pure, `ui/Exporter` impure).
//!
//! THE SPLIT. Nothing here touches a browser. The app renders [`export_document`], loads it into
//! the offscreen `WebView`, waits for `data-md-render-complete`, runs [`CAPTURE_SCRIPT`] and hands
//! the resulting `documentElement.outerHTML` back to [`prepare_page_for_document`] together with a
//! reader for the bundled `rich/` assets. What comes back is the file's text.
//!
//! WHY THE CAPTURE IS THE LIVE DOM AND NOT THE INPUT. By the time the flag is set, Mermaid,
//! Graphviz and PlantUML have become inline `<svg>` and KaTeX has expanded its formulas into
//! markup (a ```` ```plot ```` fence needed no engine and no wait — it was already `<svg>` in the
//! bytes the page was loaded from). Nothing is left to run, so the capture script removes every
//! `<script>` and every stylesheet `<link>` — in the DOM, not by string surgery on the markup — and
//! the exported file never reaches for an engine that will not be there.
//!
//! WHAT "SELF-CONTAINED" MEANS, EXACTLY. Engine- and font-self-contained, **not**
//! asset-self-contained: the author's `<img src="photo.png">` stays relative and breaks when the
//! file moves. That is a known, accepted gap on every port; closing it here alone would be a
//! parity break, not a bug fix.

use std::fmt;

use crate::markdown::html;

/// The DOM read the app must run against the finished page, verbatim as macOS, Android and md.win
/// spell it. It returns `documentElement.outerHTML`; an empty or null result is the app's "The
/// rendered page could not be captured." error, never a page.
///
/// Joined with `"\n"` escapes, never a raw multi-line literal, so the source file's own line
/// endings cannot reach the DOM. WebKitGTK's `evaluate_javascript` hands back a
/// `javascriptcore::Value`; the app takes its *string* value (WebView2 hands back the JSON
/// encoding, which md.win must decode — WebKitGTK does not, unless the app asks for `to_json`).
pub const CAPTURE_SCRIPT: &str = concat!(
    "(function () {\n",
    "  document.querySelectorAll('script, link[rel=\"stylesheet\"]').forEach(function (el) {\n",
    "    el.remove();\n",
    "  });\n",
    "  // A stale completion flag would be misleading in a file that has\n",
    "  // nothing left to complete.\n",
    "  document.documentElement.removeAttribute('data-md-render-complete');\n",
    "  return document.documentElement.outerHTML;\n",
    "})()",
);

/// `outerHTML` omits it; without one every browser renders the file in quirks mode.
pub const DOCTYPE: &str = "<!DOCTYPE html>\n";

/// The bundled stylesheet, as the HTML writer links it and as [`prepare_page`] asks for it.
pub const KATEX_CSS_ASSET: &str = "rich/katex.min.css";

/// Where a face named by [`KATEX_CSS_ASSET`] is read from: `rich/fonts/<face>.woff2`.
pub const KATEX_FONTS_FOLDER: &str = "rich/fonts/";

/// The page-break rule as the export stylesheet writes it.
pub const EXPORT_PAGE_BREAK_RULE: &str = ".md-pagebreak { height: 0; margin: 0; break-after: page; }";

/// The preview's dashed rule, in the light border colour — the export page is always light.
pub const SCREEN_PAGE_BREAK_RULE: &str =
    ".md-pagebreak { border-top: 2px dashed rgba(43,38,32,0.16); margin: 1.6em 0; }";

/// The container class that says a captured page carries a Mermaid diagram.
pub const MERMAID_CONTAINER_NEEDLE: &str = "class=\"mermaid\"";

const HEAD_CLOSE: &str = "</head>";

/// The notice that has to travel with an exported page carrying KaTeX's stylesheet and fonts. The
/// code is MIT; the faces are **not** — they are SIL Open Font License 1.1 with reserved names, and
/// the OFL requires its notice to accompany the fonts wherever they go. Exporting is the first
/// thing md does that hands those files to somebody else, so this is the first place the
/// obligation actually bites. Legal text: copy it, never reword it.
pub const KATEX_NOTICE: &str = concat!(
    "<!--\n",
    "  Mathematics rendered with KaTeX (https://katex.org) \u{2014} MIT License,\n",
    "  Copyright (c) 2013-2020 Khan Academy and other contributors.\n",
    "  The embedded KaTeX_* fonts are licensed under the SIL Open Font\n",
    "  License 1.1 (https://scripts.sil.org/OFL); \"KaTeX\" is a Reserved Font\n",
    "  Name. The fonts are embedded unmodified.\n",
    "-->",
);

/// Mermaid writes its own theme CSS into every diagram it draws, so an exported page carrying a
/// Mermaid diagram is carrying several kilobytes of Mermaid's *source text* — not just generated
/// geometry, the way Graphviz and PlantUML output is. MIT asks for its notice to go with that, so
/// it does. Legal text: copy it, never reword it.
pub const MERMAID_NOTICE: &str = concat!(
    "<!--\n",
    "  Diagrams rendered with Mermaid (https://mermaid.js.org) \u{2014} MIT License,\n",
    "  Copyright (c) 2014-2022 Knut Sveidqvist. The diagram SVG carries\n",
    "  Mermaid's own theme stylesheet.\n",
    "-->",
);

/// The capture came back empty: the app's "The rendered page could not be captured." case. A file
/// holding a doctype and nothing else would look like a successful export, so [`prepare_page`]
/// refuses to make one (md.win throws `ArgumentException`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmptyCapture;

impl fmt::Display for EmptyCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The rendered page could not be captured.")
    }
}

impl std::error::Error for EmptyCapture {}

/// The page the app must LOAD for an HTML export: the export document, with the page-break rule
/// swapped back to the one a reader can see.
///
/// `dark: false, export: true` unconditionally — `document` collapses `dark && !export` before it
/// picks the palette, so a window theme threaded through here could never take effect anyway. The
/// `md.pdfPageSize` setting does not reach this document: an HTML file is not paper, and it keeps
/// A4's `48px 56px` body margin whatever the PDF menu says.
pub fn export_document(source: &str, title: &str) -> String {
    visible_page_breaks(&html::document(source, title, false, true))
}

/// Put the author's `\newpage` markers back on screen: in export CSS a page break collapses to
/// `break-after: page`, which paper honours and a scrolling reader never sees, so an exported HTML
/// file — read on screen — gets the preview's dashed rule instead. The PDF and print paths are
/// untouched and still get a real page break.
///
/// **Every** occurrence, unlike [`super::pdf_export::styled_for_export`]'s first-only margin
/// rewrite. The asymmetry is deliberate on every port: a document that quotes the export rule in a
/// code block gets the visible rule there too, which is the honest reading of "show me my page
/// breaks".
pub fn visible_page_breaks(document: &str) -> String {
    document.replace(EXPORT_PAGE_BREAK_RULE, SCREEN_PAGE_BREAK_RULE)
}

/// True when `export_document` pulled KaTeX in — the same signal the HTML writer uses to decide
/// whether to link the stylesheet at all, so only a document with real math carries the font
/// payload.
///
/// ASK THE INPUT, NEVER THE CAPTURED PAGE. The `<link … rich/katex.min.css>` tag has already been
/// removed from the DOM by capture time, so a captured page never matches and no exported
/// document would ever carry the stylesheet. Do not "simplify" this.
pub fn needs_katex(export_document: &str) -> bool {
    export_document.contains(KATEX_CSS_ASSET)
}

/// KaTeX's stylesheet with its web fonts embedded, ready to be dropped into an exported page — or
/// `None` if the assets are missing it.
///
/// A formula is not glyphs alone: `katex.min.css` positions every piece of it, so an export that
/// dropped the stylesheet would show the right characters in the wrong places. It cannot be linked
/// either, since the file has to stand on its own — so it is inlined, and each `@font-face` keeps
/// only its **woff2** source, rewritten as a `data:` URI. woff2 is the one format every browser
/// that matters reads; carrying the woff and ttf alternates as well would quadruple the payload for
/// nothing, and leaving them as relative paths would leave dead links in the file. Twenty faces,
/// about 300 KB before encoding.
///
/// `read_rich_asset` reads one bundled asset by its slash-separated path (`rich/katex.min.css`, or
/// `rich/fonts/<face>.woff2` with `<face>` in `[A-Za-z0-9_-]+`) — the installed `rich/` folder in
/// the app, a plain file read in the tests, which is what keeps this function testable. It returns
/// `None` for an absent file and must not panic; `Some(vec![])` is an empty file that exists. A
/// missing face leaves its rule alone: a relative path that is merely absent still degrades to a
/// fallback face, whereas a truncated `data:` URI is a parse error that takes the surrounding rules
/// with it. A stylesheet that is not valid UTF-8 is treated as missing, as Foundation's
/// `String(contentsOf:encoding:)` does.
///
/// A UTF-8 BOM on the stylesheet is kept (U+FEFF reaches the page), as md.win, Kotlin and Node
/// keep it and Foundation does not — unreachable with the vendored sheet, pinned by the
/// adversarial suite so that a re-vendored sheet with one is a red test.
pub fn embedded_katex_css(read_rich_asset: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<String> {
    let bytes = read_rich_asset(KATEX_CSS_ASSET)?;
    // A strict decode, not a lossy one: invalid bytes are a missing stylesheet on the Swift, not
    // a sheet full of U+FFFD.
    let css = String::from_utf8(bytes).ok()?;
    Some(inline_woff2_faces(&css, read_rich_asset))
}

/// The prefix every rewritten `src` run starts with.
const WOFF2_SOURCE_OPEN: &str = "src:url(fonts/";

/// What must follow the face name for a run to be a woff2 source.
const WOFF2_SOURCE_CLOSE: &str = ".woff2) format(\"woff2\")";

/// md.win's `src:url\(fonts/([A-Za-z0-9_-]+)\.woff2\) format\("woff2"\)[^;}]*`, replaced through a
/// match evaluator, matched by hand (md-core has no regex crate): every `src:` that names a woff2
/// face, with the rest of the value — the woff / ttf alternates — up to the declaration's `;` or
/// the rule's `}`.
///
/// The `[^;}]*` tail is what eats the sibling `url(fonts/X.woff) format("woff"), url(fonts/X.ttf)
/// format("truetype")` alternates: they are dead links once the file leaves this machine, so the
/// whole `src` run is replaced rather than only the woff2 term. It stops at a `;` as well as at
/// `}`, so the declarations after it survive. The face class is spelled out ASCII, never a
/// Unicode-aware `\w`: `KaTeX_İ` and `Ａwide` are not KaTeX faces and their rules are left exactly
/// as written, file or no file. The name run is greedy and `.` is outside its class, so the regex
/// never backtracks into it and the maximal run is the only candidate.
///
/// The inserted text is spliced, never read as a replacement template: 300 KB of base64 with a
/// `$&` in a `Regex.Replace` replacement string would be corrupted, and nobody would ever find it.
fn inline_woff2_faces(css: &str, read_rich_asset: &dyn Fn(&str) -> Option<Vec<u8>>) -> String {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = css[search..].find(WOFF2_SOURCE_OPEN) {
        let start = search + found;
        let name_start = start + WOFF2_SOURCE_OPEN.len();
        let name_end = name_start
            + bytes[name_start..].iter().take_while(|&&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-').count();
        if name_end == name_start || !css[name_end..].starts_with(WOFF2_SOURCE_CLOSE) {
            // Not a match here; the next candidate starts at a later `src:url(fonts/`.
            search = start + 1;
            continue;
        }
        let tail_start = name_end + WOFF2_SOURCE_CLOSE.len();
        let end = tail_start + bytes[tail_start..].iter().take_while(|&&b| b != b';' && b != b'}').count();

        out.push_str(&css[copied..start]);
        let face = &css[name_start..name_end];
        match read_rich_asset(&format!("{KATEX_FONTS_FOLDER}{face}.woff2")) {
            Some(data) => {
                out.push_str("src:url(data:font/woff2;base64,");
                out.push_str(&base64(&data));
                out.push_str(") format(\"woff2\")");
            }
            None => out.push_str(&css[start..end]),
        }
        copied = end;
        // An empty-width match cannot happen (the literal prefix is never empty), so the scan
        // always advances.
        search = end;
    }
    out.push_str(&css[copied..]);
    out
}

/// Drop the stylesheet and its licence notice into a captured page, ahead of `</head>`.
///
/// Every `</head>`, as Swift's `replacingOccurrences` does (Kotlin and TypeScript take only the
/// first); a rendered document has exactly one (all author text is escaped), so they agree on every
/// real page. `str::replace` reads nothing in the CSS as a template.
pub fn with_embedded_katex(page: &str, css: &str) -> String {
    page.replace(HEAD_CLOSE, &["<style>", css, "</style>\n", KATEX_NOTICE, "\n", HEAD_CLOSE].concat())
}

/// Drop Mermaid's notice into a captured page that holds one of its diagrams.
pub fn with_mermaid_notice(page: &str) -> String {
    if !page.contains(MERMAID_CONTAINER_NEEDLE) {
        return page.to_string();
    }
    page.replace(HEAD_CLOSE, &[MERMAID_NOTICE, "\n", HEAD_CLOSE].concat())
}

/// The finished file: the captured page with the doctype put back, KaTeX's stylesheet and notice
/// inlined when `has_math`, and Mermaid's notice added when the captured page carries one of its
/// diagrams.
///
/// The two gates read different strings on purpose. `has_math` is [`needs_katex`] of the *input*
/// document (the link is gone from the capture); Mermaid is asked of the *captured page*, where
/// the container class survives. With both, the head ends
/// `…<style>KATEXCSS</style>\n(katex notice)\n(mermaid notice)\n</head>`.
///
/// `read_rich_asset` is called only when `has_math` (at most 21 times: the sheet and twenty faces);
/// a document without math reads nothing. No stylesheet in the package: the export goes on
/// without it — unstyled formulas beat no file.
///
/// An empty `captured_html` is [`EmptyCapture`] — never written to a file.
pub fn prepare_page(
    captured_html: &str,
    read_rich_asset: &dyn Fn(&str) -> Option<Vec<u8>>,
    has_math: bool,
) -> Result<String, EmptyCapture> {
    if captured_html.is_empty() {
        return Err(EmptyCapture);
    }
    let mut page = [DOCTYPE, captured_html].concat();
    if has_math && let Some(css) = embedded_katex_css(read_rich_asset) {
        page = with_embedded_katex(&page, &css);
    }
    Ok(with_mermaid_notice(&page))
}

/// The finished file, given the captured page and the document that produced it — the form that
/// reads the math gate off `export_document` itself (md.win's three-string overload of
/// `PreparePage`), so a caller cannot get it the wrong way round. Pass the very string
/// [`export_document`] returned and the app loaded.
pub fn prepare_page_for_document(
    captured_html: &str,
    export_document: &str,
    read_rich_asset: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<String, EmptyCapture> {
    prepare_page(captured_html, read_rich_asset, needs_katex(export_document))
}

/// RFC 4648 base64 with padding and no line breaks — what .NET's `Convert.ToBase64String` writes. A
/// wrapped payload would put newlines inside a `url()` value.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHABET[usize::from(b0 >> 2)] as char);
        out.push(ALPHABET[usize::from(((b0 & 0x03) << 4) | (b1 >> 4))] as char);
        out.push(if chunk.len() > 1 { ALPHABET[usize::from(((b1 & 0x0F) << 2) | (b2 >> 6))] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[usize::from(b2 & 0x3F)] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_rfc_4648_test_vectors() {
        // RFC 4648 §10.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Every alphabet position, the two non-alphanumerics included, and no line breaks however
        // long the payload (MIME would wrap at 76).
        let all: Vec<u8> = (0..=255).collect();
        let encoded = base64(&all);
        assert!(encoded.starts_with("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJygpKissLS4v"));
        assert!(encoded.ends_with("+/w=="));
        assert_eq!(encoded.len(), 344);
        assert!(!encoded.contains('\n'));
        assert_eq!(base64(&[0xFB, 0xFF]), "+/8=");
        assert_eq!(base64(b"MZ"), "TVo=");
    }
}
