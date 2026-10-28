//! md's own stylesheet, byte for byte — port of md.win `Md.Core.Markdown.MarkdownCss`, itself the
//! port of `MarkdownHTML.css(dark:export:)`: one literal with seven interpolations and the same
//! 101 lines in every variant.
//!
//! Built as an array of lines joined with `"\n"`, never as a raw multi-line literal, so the
//! source file's own line endings cannot leak into the output (a CRLF checkout would emit `\r\n`
//! and fail every stylesheet golden silently). No trailing newline — the Swift literal ends at the
//! last rule and `document()` glues `</style>` straight onto it.
//!
//! Three reachable sheets, not four: `dark && !export` runs in `document` before the palette is
//! computed and is repeated here, so a caller reaching [`stylesheet`] directly cannot produce the
//! never-shipped dark-ink-on-white variant no golden covers.
//!
//! Hazards the shipping tests pin: every `/*` has exactly one `*/` (a comment that closes early
//! makes the CSS parser swallow the next rule whole, and the Graphviz ink rules sit under the
//! longest one); the blank line inside the Graphviz comment is intentional; line 32 is genuinely
//! blank in the preview variants (the empty `pre_wrap` interpolation), leaving a comment that
//! describes a rule which is not there; `\newpage` is one backslash in the output; the only
//! indentation is 4 spaces inside `body { … }` and the rich-block rule and 3 spaces continuing each
//! comment. Kotlin's sheet differs (font, hljs rules, string tone) and is not a reference.

pub(crate) fn stylesheet(dark: bool, export: bool) -> String {
    let is_dark = dark && !export;
    let pick = |dark_value: &'static str, light_value: &'static str| if is_dark { dark_value } else { light_value };

    // The first four come from the app's asset catalog (PaperBackground, PaperInk,
    // PaperBackgroundSecondary, AccentColor); the rest exist only in CSS.
    let paper = pick("#241E18", "#F4EFE2");
    let ink = pick("#E7DBC2", "#2B2620");
    let secondary = pick("#2F2820", "#EAE2CF");
    let accent = pick("#C99A55", "#9C6B2E");
    let muted = pick("#B3A98E", "#6B635A");
    // Literally ink at 16 % alpha, emitted as this exact string and never computed: the tests grep
    // for it and the HTML export's page-break swap substitutes it.
    let border = pick("rgba(231,219,194,0.16)", "rgba(43,38,32,0.16)");
    // The hljs string tone: a shade of the ink, warmer and darker than `muted`. Android ships
    // #6A5433 / #B79A67 here; Apple is the reference.
    let code_string = pick("#CDBF9E", "#4A4034");

    let pre_wrap = if export { "pre { white-space: pre-wrap; overflow-wrap: anywhere; }" } else { "" };
    let page_break = if export {
        ".md-pagebreak { height: 0; margin: 0; break-after: page; }".to_string()
    } else {
        format!(".md-pagebreak {{ border-top: 2px dashed {border}; margin: 1.6em 0; }}")
    };

    let lines: [String; 101] = [
        "/* Force backgrounds to render in print / PDF so the content chrome".to_string(),
        "   (code blocks, table headers) survives, rather than being dropped. */".to_string(),
        "* { -webkit-print-color-adjust: exact; print-color-adjust: exact; box-sizing: border-box; }".to_string(),
        format!(":root {{ color-scheme: {}; }}", if is_dark { "dark" } else { "light" }),
        "/* On paper the page keeps its own single color: the paper tint is a".to_string(),
        "   screen theme, and a content-height background would end mid-page".to_string(),
        "   next to the white A4 margins. */".to_string(),
        format!("html, body {{ background: {}; }}", if export { "#FFFFFF" } else { paper }),
        "body {".to_string(),
        format!("    color: {ink};"),
        "    font-family: \"American Typewriter\", \"Courier New\", serif;".to_string(),
        format!("    font-size: {}pt;", if export { "11" } else { "13" }),
        "    line-height: 1.55;".to_string(),
        "    margin: 0;".to_string(),
        "    padding: 48px 56px;".to_string(),
        "    -webkit-text-size-adjust: 100%;".to_string(),
        "}".to_string(),
        "h1, h2, h3, h4, h5, h6 { font-weight: bold; line-height: 1.25; margin: 1.2em 0 0.5em; }".to_string(),
        "h1 { font-size: 2em; }".to_string(),
        "h2 { font-size: 1.6em; }".to_string(),
        "h3 { font-size: 1.3em; }".to_string(),
        "h4 { font-size: 1.1em; }".to_string(),
        "h5 { font-size: 1em; }".to_string(),
        format!("h6 {{ font-size: 0.9em; color: {muted}; }}"),
        "p { margin: 0 0 0.9em; }".to_string(),
        format!("a {{ color: {accent}; }}"),
        "code, pre { font-family: \"Courier New\", monospace; }".to_string(),
        format!("code {{ background: {secondary}; padding: 0.1em 0.3em; border-radius: 4px; font-size: 0.92em; }}"),
        format!("pre {{ background: {secondary}; padding: 12px 14px; border-radius: 8px; overflow-x: auto; }}"),
        "/* In export, code wraps: paper can't scroll a too-wide block, so a".to_string(),
        "   long line would otherwise be clipped at the block's edge. */".to_string(),
        pre_wrap.to_string(),
        "pre code { background: none; padding: 0; font-size: 0.92em; }".to_string(),
        "/* Syntax highlighting (highlight.js). Deliberately NOT a stock hljs".to_string(),
        "   theme — github / monokai and the rest are bright rainbows that fight".to_string(),
        "   the warm-paper, American-Typewriter look. Instead three quiet tones".to_string(),
        "   taken from the page's own palette: language keywords and the".to_string(),
        "   structural names in the warm accent, comments in muted italic,".to_string(),
        "   strings a softer shade of the ink; numbers and everything else stay".to_string(),
        "   plain ink. The code face is still Courier New (inherited from `pre".to_string(),
        "   code`). Applied by md-init.js over the live DOM, so it reaches".to_string(),
        "   preview, print, PDF and HTML export — not EPUB, which snapshots this".to_string(),
        "   HTML before any script runs, so its code blocks stay plain. */".to_string(),
        ".hljs-keyword, .hljs-selector-tag, .hljs-built_in, .hljs-literal,".to_string(),
        format!(".hljs-type, .hljs-title, .hljs-section, .hljs-name, .hljs-doctag {{ color: {accent}; }}"),
        format!(".hljs-comment, .hljs-quote, .hljs-meta {{ color: {muted}; font-style: italic; }}"),
        ".hljs-string, .hljs-regexp, .hljs-symbol, .hljs-char,".to_string(),
        format!(".hljs-attr, .hljs-attribute, .hljs-addition {{ color: {code_string}; }}"),
        format!(".hljs-deletion {{ color: {muted}; text-decoration: line-through; }}"),
        ".hljs-emphasis { font-style: italic; }".to_string(),
        ".hljs-strong { font-weight: bold; }".to_string(),
        format!(
            "blockquote {{ margin: 0 0 0.9em; padding-left: 14px; border-left: 4px solid {accent}; color: {muted}; }}"
        ),
        format!("hr {{ border: none; border-top: 1px solid {border}; margin: 1.4em 0; }}"),
        "/* The author's `\\newpage`: a dashed rule on screen; in export / print".to_string(),
        "   it collapses to an invisible marker where a new page starts (the".to_string(),
        "   PDF capture splits pages at it, and paginated printing breaks). */".to_string(),
        page_break,
        "table { border-collapse: collapse; margin: 0 0 0.9em; }".to_string(),
        format!("th, td {{ border: 1px solid {border}; padding: 6px 12px; }}"),
        format!("th {{ background: {secondary}; }}"),
        ".md-list { margin: 0 0 0.9em; }".to_string(),
        ".md-item { display: flex; gap: 0.5em; margin: 0.22em 0; }".to_string(),
        format!(".md-marker {{ color: {muted}; min-width: 1.5em; text-align: right; }}"),
        format!(".md-item.done {{ color: {muted}; text-decoration: line-through; }}"),
        "/* Rich blocks: diagrams and display formulas render as SVG/markup, not".to_string(),
        "   code — drop the code-block chrome, centre them, allow horizontal".to_string(),
        "   scroll. Inline math (.md-mathi) flows with the text. */".to_string(),
        ".mermaid, .plantuml, .graphviz, .plot, .md-mathd {".to_string(),
        "    background: none; padding: 6px 0; margin: 0 0 0.9em;".to_string(),
        "    overflow-x: auto; text-align: center;".to_string(),
        "}".to_string(),
        ".mermaid svg, .plantuml svg, .graphviz svg, .plot svg { max-width: 100%; height: auto; }".to_string(),
        "/* Graphviz draws in plain black on a transparent ground (md-init.js".to_string(),
        "   asks for `bgcolor=transparent`). Recolor it to the page's ink here,".to_string(),
        "   in CSS, rather than passing colors to the engine: these are".to_string(),
        "   presentation attributes, which any CSS rule outranks, and leaving".to_string(),
        "   the engine's own attributes alone keeps the layout metrics — and so".to_string(),
        "   the label positions it computed — exactly as Graphviz intended.".to_string(),
        String::new(),
        "   An author's own `fontcolor` / `color` survives: Graphviz writes".to_string(),
        "   those out as attributes too, so each rule is scoped to the value the".to_string(),
        "   engine emits when nothing was asked for — text with no fill of its".to_string(),
        "   own, and explicit black. */".to_string(),
        format!(".graphviz svg text:not([fill]) {{ fill: {ink}; }}"),
        format!(".graphviz svg text[fill=\"black\"] {{ fill: {ink}; }}"),
        format!(".graphviz svg [stroke=\"black\"] {{ stroke: {ink}; }}"),
        format!(".graphviz svg [fill=\"black\"]:not(text) {{ fill: {ink}; }}"),
        "/* Footnotes. The references are superscript numerals in the running".to_string(),
        "   text; the notes themselves sit under a rule at the foot of the".to_string(),
        "   document, a size down, with a back-link to where they were cited. */".to_string(),
        format!(".md-fnref a {{ text-decoration: none; color: {accent}; }}"),
        format!(".md-footnotes {{ margin-top: 2em; font-size: 0.9em; color: {muted}; }}"),
        ".md-footnotes hr { margin: 0 0 0.8em; }".to_string(),
        ".md-footnotes ol { margin: 0; padding-left: 1.6em; }".to_string(),
        ".md-footnotes li { margin: 0.35em 0; }".to_string(),
        format!(".md-fnback {{ text-decoration: none; color: {accent}; }}"),
        "/* Images render at their natural size, only capped to the page width;".to_string(),
        "   height follows so the aspect ratio never distorts. */".to_string(),
        "img { max-width: 100%; height: auto; }".to_string(),
        ".md-mathd .katex-display { margin: 0; }".to_string(),
        ".katex-display { overflow-x: auto; overflow-y: hidden; padding: 2px 0; }".to_string(),
    ];
    lines.join("\n")
}
