//! Port of md.win `Preview/ScreenHtml.cs`: the app's typography graft (§4.3).

use std::borrow::Cow;

/// The style element, exactly as it goes into the page.
///
/// LINUX DECISION (docs/linux-facts.md §7): the shared stylesheet's prose family is
/// `"American Typewriter", "Courier New", serif` and stays byte-identical across the ports. Ubuntu
/// ships neither American Typewriter nor Courier New — fontconfig maps "Courier New" to Liberation
/// Mono, so prose and code would end up in one face. The app — never md-core — appends one style
/// element that puts **Nimbus Mono PS** in front: URW's Courier, the typewriter face Ubuntu 26.04
/// actually installs (fonts-urw-base35), with real Regular, Bold, Italic and Bold Italic and
/// Cyrillic and Greek. md.win puts Lucida Sans Typewriter there for the same reason. Code is
/// untouched: the stylesheet's `code, pre` rule is more specific and keeps "Courier New" (Liberation
/// Mono), so prose and code differ as on the Mac, and KaTeX, Mermaid, Graphviz and PlantUML carry
/// their own faces.
///
/// Exactly the screen and paper callers use it: the live preview
/// ([`PreviewCoordinator`](super::PreviewCoordinator), `RenderKind::Screen`), Print and PDF
/// (`RenderKind::Paper`). HTML, EPUB and SVG exports load the pure md-core HTML, so nothing
/// Linux-specific reaches a file another port pins byte for byte.
pub const FONT_STYLE: &str =
    "<style id=\"md-linux-fonts\">body{font-family:\"Nimbus Mono PS\",\"Courier New\",serif;}</style>";

/// What the inserted text is looked up by, so a second call is a no-op.
pub const ID_MARKER: &str = "id=\"md-linux-fonts\"";

const HEAD_END: &str = "</head>";

/// `core_html` with [`FONT_STYLE`] on its own line before the first `</head>`. Idempotent (the
/// input comes back borrowed when it already carries [`ID_MARKER`] anywhere), and a fragment with
/// no head comes back untouched (borrowed) — nothing md-core's document writer produces, but the
/// Print pipeline hands this whatever it is given.
pub fn with_linux_fonts(core_html: &str) -> Cow<'_, str> {
    if core_html.contains(ID_MARKER) {
        return Cow::Borrowed(core_html);
    }
    match core_html.find(HEAD_END) {
        Some(head) => Cow::Owned(format!("{}\n{FONT_STYLE}{}", &core_html[..head], &core_html[head..])),
        None => Cow::Borrowed(core_html),
    }
}
