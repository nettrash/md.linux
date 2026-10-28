//! The HTML writer: the parsed block model serialised to a self-contained, themed document — the
//! one rendering path for the preview, print, PDF, "share rendered", the HTML export and
//! (snapshotted before scripts run) EPUB. Port of md.win `Md.Core.Markdown.MarkdownHtml`. Byte
//! parity with md, md.macOS, md.Android, md.vscode and md.win is the product contract; md.vscode's
//! golden corpus, captured from the Swift bytes, is the tripwire.
//!
//! A hand-written, test-pinned *subset* of CommonMark with deliberate divergences: no raw HTML, no
//! reference links, no autolinks, no indented code, flat lists, a currency guard on inline math. A
//! CommonMark engine produces different bytes on the first real document.
//!
//! Every literal here is joined with `"\n"` rather than written as a raw multi-line string, so the
//! source file's own line endings cannot leak into the output; the document ends at `</html>` with
//! no trailing newline. Relative `rich/` URLs stay in the bytes on every surface: the host serves
//! the page from an origin under which they resolve, never through a `<base href>`, which would
//! turn `href="#slug"` into a cross-document navigation.

use std::collections::HashMap;
use std::fmt;

use super::block::{ColumnAlignment, ListItem, MarkdownBlock};
use super::css;
use super::delimited_table::DelimitedTable;
use super::inline_html;
use super::parser;
use crate::text::scalar_text;

/// Which engines a rendered body turned out to need, in the order the probes run.
///
/// `Display` is the space-joined lowercase list in the order math, mermaid, plantuml, graphviz,
/// highlight — the words `golden/engine-needs.json` records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct EngineNeeds {
    pub math: bool,
    pub mermaid: bool,
    pub plantuml: bool,
    pub graphviz: bool,
    pub highlight: bool,
}

impl EngineNeeds {
    pub const NONE: EngineNeeds =
        EngineNeeds { math: false, mermaid: false, plantuml: false, graphviz: false, highlight: false };

    pub fn new(math: bool, mermaid: bool, plantuml: bool, graphviz: bool, highlight: bool) -> Self {
        EngineNeeds { math, mermaid, plantuml, graphviz, highlight }
    }
}

impl fmt::Display for EngineNeeds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let words: Vec<&str> = [
            (self.math, "math"),
            (self.mermaid, "mermaid"),
            (self.plantuml, "plantuml"),
            (self.graphviz, "graphviz"),
            (self.highlight, "highlight"),
        ]
        .into_iter()
        .filter_map(|(on, word)| on.then_some(word))
        .collect();
        f.write_str(&words.join(" "))
    }
}

/// The body markup of a document and the engines it needs — md.vscode's `renderBody` split.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderedBody {
    pub html: String,
    pub needs: EngineNeeds,
}

/// Fenced-block info strings that select the bundled Graphviz engine, mapped to the layout program
/// (`neato -Tsvg`, `circo -Tsvg`, …). Ten keys, eight engines, in md.win's dictionary order; every
/// value must be a `Viz.engines` name or the render throws and the block falls back to its source.
/// The *value* reaches `data-engine`, never the author's word, so the attribute needs no escaping —
/// never widen this with author-controlled text. The LaTeX writer and the diagram-SVG classifier
/// read the same table (through [`graphviz_engine`]).
pub const GRAPHVIZ_ENGINES: [(&str, &str); 10] = [
    ("dot", "dot"),
    ("graphviz", "dot"),
    ("gv", "dot"),
    ("neato", "neato"),
    ("circo", "circo"),
    ("fdp", "fdp"),
    ("sfdp", "sfdp"),
    ("twopi", "twopi"),
    ("osage", "osage"),
    ("patchwork", "patchwork"),
];

/// The layout engine an (already case-folded) info word selects, or `None` — an ordinal lookup in
/// [`GRAPHVIZ_ENGINES`].
pub fn graphviz_engine(word: &str) -> Option<&'static str> {
    GRAPHVIZ_ENGINES.iter().find(|(key, _)| *key == word).map(|(_, engine)| *engine)
}

/// A full HTML document for `source`. `title` becomes the `<title>` — always the caller's string,
/// never sniffed from an H1 or front matter. `export` styles for paper: 11pt, code wraps, page
/// breaks collapse to real page boundaries, and the page is plain white in the light palette
/// regardless of `dark` — the tinted paper and cream-on-carbon ink are screen themes. (md.win's
/// `export` defaults to `false`.)
pub fn document(source: &str, title: &str, dark: bool, export: bool) -> String {
    // The first line of the Swift, shadowing the parameter: an export is always light, and the
    // shadowed value feeds both the palette and `data-md-dark`, so Mermaid and PlantUML render
    // light in an export too. Three reachable stylesheets, not four.
    let dark = dark && !export;

    let body = body(source, title, dark);
    let needs = body.needs;

    // `<head>` ORDER IS LOAD-BEARING. KaTeX and mhchem share `defer` and must stay in that order:
    // deferred classic scripts run in document order, KaTeX defines the global, mhchem registers
    // `\ce{}` onto it, then the deferred module md-init.js renders. Both are the same KaTeX 0.17.0
    // build and are replaced together. Appended with NO leading newline — glued flush onto
    // `</style>`.
    let mut head = String::new();
    if needs.math {
        head.push_str(
            &[
                "<link rel=\"stylesheet\" href=\"rich/katex.min.css\">",
                "<script defer src=\"rich/katex.min.js\"></script>",
                "<script defer src=\"rich/mhchem.min.js\"></script>",
            ]
            .join("\n"),
        );
    }
    if needs.mermaid {
        head.push_str("\n<script src=\"rich/mermaid.min.js\"></script>");
    }
    // Viz.js is Graphviz. It is included for PlantUML too — inherited from md, whose stated reason
    // (PlantUML's Graphviz-backed layouts) does not survive testing, the TeaVM build carrying
    // Smetana — and kept for parity: revisit in all the ports together or not at all. PlantUML
    // itself is never in the head; md-init.js imports it lazily.
    if needs.plantuml || needs.graphviz {
        head.push_str("\n<script src=\"rich/viz-global.js\"></script>");
    }
    // highlight.js is deferred like KaTeX (Android omits the `defer`; Swift's bytes are canonical).
    if needs.highlight {
        head.push_str("\n<script defer src=\"rich/highlight.min.js\"></script>");
    }

    // An empty head yields `</style>\n</head>`; an empty source leaves a blank line between
    // `<body …>` and the module script. Both are part of the bytes.
    [
        "<!DOCTYPE html>".to_string(),
        "<html lang=\"en\">".to_string(),
        "<head>".to_string(),
        "<meta charset=\"utf-8\">".to_string(),
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">".to_string(),
        format!("<title>{}</title>", escape(title)),
        format!("<style>{}</style>{head}", css(dark, export)),
        "</head>".to_string(),
        // Read by md-init.js as `document.body.dataset.mdDark === '1'`; the theme is chosen by this
        // attribute, never by CSS.
        format!("<body data-md-dark=\"{}\">", if dark { "1" } else { "0" }),
        body.html,
        "<script type=\"module\" src=\"rich/md-init.js\"></script>".to_string(),
        "</body>".to_string(),
        "</html>".to_string(),
    ]
    .join("\n")
}

/// The document body and the engines it needs. Three mutually exclusive paths: a raw PlantUML
/// file, a raw Graphviz file, or Markdown. Neither `title` nor `dark` influences the body; they are
/// kept for symmetry with [`document`] and md.vscode's `renderBody`, whose goldens call it this
/// way.
pub fn body(source: &str, title: &str, dark: bool) -> RenderedBody {
    let _ = (title, dark);
    // An opened `.puml` / `.gv`: bare diagram source with no fence, rendered as one diagram rather
    // than parsed into paragraphs of its own text. The whole file, trailing newline included, is
    // escaped, so the close tag lands on its own line. The needs are hard-coded: no Markdown here,
    // so no math, Mermaid or highlightable code is possible.
    if parser::is_raw_plant_uml(source) {
        return RenderedBody {
            html: format!("<div class=\"plantuml\">{}</div>", escape(source)),
            needs: EngineNeeds { plantuml: true, ..EngineNeeds::NONE },
        };
    }
    if parser::is_raw_graphviz(source) {
        return RenderedBody {
            html: format!("<div class=\"graphviz\" data-engine=\"dot\">{}</div>", escape(source)),
            needs: EngineNeeds { graphviz: true, ..EngineNeeds::NONE },
        };
    }

    let blocks = parser::parse(source);
    // Top-level headings carry a GitHub-style anchor id from the same `slug` counter the outline
    // uses, walked in the same order, so `[…](#slug)` links and the contents menu always agree. A
    // heading nested in a quote goes through `render_block`, gets no id and consumes no counter.
    let mut slugs: HashMap<String, usize> = HashMap::new();
    let mut rendered = Vec::with_capacity(blocks.len());
    let mut definitions: Vec<(&str, &str)> = Vec::new();
    for block in &blocks {
        match block {
            MarkdownBlock::Heading { level, text } => rendered.push(format!(
                "<h{level} id=\"{}\">{}</h{level}>",
                parser::slug(text, &mut slugs),
                inline(text, false)
            )),
            _ => rendered.push(render_block(block)),
        }
        if let MarkdownBlock::FootnoteDefinition { id, text } = block {
            definitions.push((id, text));
        }
    }

    // Joined with a single "\n". Blocks that render to "" — notes, front matter, footnote
    // definitions — still take part in the join and leave blank lines. Observable; reproduced.
    // Footnotes are a whole-document affair and need the finished body.
    let body = with_footnotes(rendered.join("\n"), &definitions);
    let needs = needs(&body);
    RenderedBody { html: body, needs }
}

/// The paper-and-ink stylesheet: light preview, dark preview, or export (always light —
/// `css(true, true)` is the export sheet).
pub fn css(dark: bool, export: bool) -> String {
    css::stylesheet(dark, export)
}

/// The five probes, exact strings, run on the *emitted markup* and never on the block list: a
/// diagram nested in a quote is emitted by the recursive [`render_block`] and would otherwise ship
/// without its engine; a code block that merely quotes a container string cannot trigger it
/// because its content is escaped. That argument is FALSE for math — `md-mathi` / `md-mathd` hold
/// no `<` or `"`, so prose naming the class pulls KaTeX in. Verified, harmless at runtime, a real
/// byte difference; reproduced on purpose. `graphviz` matches the tag prefix without `>` because
/// `data-engine` varies.
pub fn needs(body: &str) -> EngineNeeds {
    EngineNeeds {
        math: body.contains("md-mathi") || body.contains("md-mathd"),
        mermaid: body.contains("<pre class=\"mermaid\">"),
        plantuml: body.contains("<div class=\"plantuml\">"),
        graphviz: body.contains("<div class=\"graphviz\""),
        highlight: body.contains("<pre><code class=\"language-"),
    }
}

// MARK: - Blocks

/// [`render_block`] over every block, joined with `"\n"` — the inside of a quote.
pub fn render_blocks(blocks: &[MarkdownBlock]) -> String {
    blocks.iter().map(render_block).collect::<Vec<_>>().join("\n")
}

/// One block to markup. A heading reaches here only when nested (inside a quote) and carries no
/// id — the top-level form is emitted by [`body`], which owns the slug counter.
pub fn render_block(block: &MarkdownBlock) -> String {
    match block {
        MarkdownBlock::Heading { level, text } => format!("<h{level}>{}</h{level}>", inline(text, false)),
        // The only caller asking for soft breaks; the conversion happens inside `inline`, before
        // protected spans are restored, so a multi-line formula keeps its own newlines.
        MarkdownBlock::Paragraph { text } => format!("<p>{}</p>", inline(text, true)),
        MarkdownBlock::List { ordered, items } => render_list(items, *ordered),
        MarkdownBlock::CodeBlock { language, code } => render_code_block(language.as_deref(), code),
        MarkdownBlock::Quote { blocks } => format!("<blockquote>\n{}\n</blockquote>", render_blocks(blocks)),
        MarkdownBlock::Table { header, alignments, rows } => render_table(header, alignments, rows),
        MarkdownBlock::ThematicBreak => "<hr>".to_string(),
        // A dashed rule in the preview; in export and print the CSS collapses it to a page boundary.
        MarkdownBlock::PageBreak => "<div class=\"md-pagebreak\"></div>".to_string(),
        // Private author notes live in the editor and the notes panel only.
        MarkdownBlock::Note { .. } => String::new(),
        // Metadata about the document, not part of it: parsed for its fields, drawn nowhere.
        MarkdownBlock::FrontMatter { .. } => String::new(),
        // Gathered by `body` and printed at the foot of the page.
        MarkdownBlock::FootnoteDefinition { .. } => String::new(),
    }
}

/// The info string selects a rich renderer. Order matters, first match wins, and the code
/// languages are deliberately last so `mermaid`, `dot`, `csv`, `tex` and `plot` can never come back
/// as highlightable source.
fn render_code_block(language: Option<&str>, code: &str) -> String {
    // The parser's first space-delimited word of the info string, or `None` when empty. Folded with
    // `full_lowercase` — Swift's `lowercased()`, the FULL Unicode mapping, never the SIMPLE 1:1
    // mapping, which leaves U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE alone: ```İstanbul would
    // emit `class="language-İstanbul"` where Swift, Kotlin (`lowercase(Locale.ROOT)`) and
    // TypeScript (`toLowerCase()`) all emit `class="language-i̇stanbul"` (i + U+0307). No locale is
    // consulted either way — a Turkish locale must not spell `LATEX` differently.
    let lang = scalar_text::full_lowercase(language.unwrap_or(""));
    match lang.as_str() {
        // Mermaid is a `<pre>`; everything else a `<div>`.
        "mermaid" => return format!("<pre class=\"mermaid\">{}</pre>", escape(code)),
        "plantuml" | "puml" | "plant-uml" => return format!("<div class=\"plantuml\">{}</div>", escape(code)),
        // Data pasted out of a spreadsheet, drawn as a table while the source stays data.
        "csv" => return render_delimited(code, ','),
        "tsv" => return render_delimited(code, '\t'),
        // A fence is a `<div>`; `$$…$$` in a paragraph is a `<span>`; both `.md-mathd`.
        "math" | "latex" | "tex" => return format!("<div class=\"md-mathd\">{}</div>", escape(code)),
        // The one rich block with no engine behind it: the finished `<svg>` is already in the
        // string every surface receives, and the class is exactly `plot`, so a plot-only document
        // trips no probe. The container is emitted whatever happens — the SVG export pairs figures
        // with fences by counting `div.plot` in document order.
        "plot" => return crate::plot::render_plot(code),
        _ => {}
    }
    if let Some(engine) = graphviz_engine(&lang) {
        // Escaping is what makes DOT's HTML-like labels safe: `n [label=<<b>hi</b>>]` becomes
        // `&lt;&lt;b&gt;…`; md-init.js reads `el.textContent`, which decodes it back.
        return format!("<div class=\"graphviz\" data-engine=\"{engine}\">{}</div>", escape(code));
    }
    if !lang.is_empty() {
        // A real code language, tagged for highlight.js. The class must *begin* with `language-`:
        // md-init.js selects `code[class^="language-"]`. An unknown hint the "common" build lacks
        // simply is not highlighted.
        return format!("<pre><code class=\"language-{}\">{}</code></pre>", escape(&lang), escape(code));
    }
    format!("<pre><code>{}</code></pre>", escape(code))
}

/// The flat, level-tagged item list with explicit markers and indentation — mirroring the
/// on-screen preview rather than rebuilding `<ul>`/`<ol>` from a model that is not a tree. Rows are
/// concatenated with no separator: the whole list is one line.
fn render_list(items: &[ListItem], ordered: bool) -> String {
    let mut rows = String::new();
    for item in items {
        // .NET's "F2" with the invariant culture — never a decimal comma, which would write
        // `padding-left:1,60em` and the rule would be dropped.
        let indent = format_f2(item.level as f64 * 1.6);
        let marker = match (item.task, item.ordinal) {
            // Task beats ordered. Entities, not characters: the EPUB builder rewrites `&bull;` to
            // `&#8226;` because named entities are undefined in XML.
            (Some(done), _) => (if done { "&#9745;" } else { "&#9744;" }).to_string(),
            (None, Some(ordinal)) if ordered => format!("{ordinal}."),
            _ => "&bull;".to_string(),
        };
        let done_class = if item.task == Some(true) { " done" } else { "" };
        rows.push_str(&format!(
            "<div class=\"md-item{done_class}\" style=\"padding-left:{indent}em\"><span class=\"md-marker\">{marker}</span><span>{}</span></div>",
            inline(&item.text, false)
        ));
    }
    format!("<div class=\"md-list\">{rows}</div>")
}

/// .NET's `double.ToString("F2", CultureInfo.InvariantCulture)` for a finite, non-negative value:
/// the exact binary value rounded to two decimals, an exact tie away from zero. Rust's `{:.2}` is
/// correctly rounded too but does not promise .NET's tie rule, so the ties — exactly the values
/// `n + j/8` with `j` odd, the only binary fractions ending in a 5 at the third decimal — are
/// settled here first. (The list indent, `level × 1.6`, never lands on one at any real level.)
fn format_f2(value: f64) -> String {
    let eighths = value * 8.0;
    if eighths.fract() == 0.0 && eighths < 9.0e15 {
        let k = eighths as u64;
        if k % 2 == 1 {
            // k / 8 = 12.5·k hundredths, a tie, rounded up: ⌈12.5·k⌉ = (25·k + 1) / 2.
            let hundredths = (25 * k).div_ceil(2);
            return format!("{}.{:02}", hundredths / 100, hundredths % 100);
        }
    }
    format!("{value:.2}")
}

/// A ```` ```csv ```` / ```` ```tsv ```` block as a table. One that parses to nothing stays a
/// *bare* code block — nothing the author wrote disappears, and the bare form trips no highlight
/// gate.
fn render_delimited(code: &str, separator: char) -> String {
    match DelimitedTable::from(code, separator) {
        None => format!("<pre><code>{}</code></pre>", escape(code)),
        Some(table) => render_table(&table.header, &table.alignments, &table.rows),
    }
}

/// Inline style per cell, no space after the colon; every cell through the inline pass; `<tbody>`
/// even with no rows; a column past the alignment array is left. One line.
fn render_table(header: &[String], alignments: &[ColumnAlignment], rows: &[Vec<String>]) -> String {
    let align = |i: usize| match alignments.get(i) {
        Some(ColumnAlignment::Center) => "center",
        Some(ColumnAlignment::Trailing) => "right",
        _ => "left",
    };

    let mut html = String::from("<table><thead><tr>");
    for (i, cell) in header.iter().enumerate() {
        html.push_str(&format!("<th style=\"text-align:{}\">{}</th>", align(i), inline(cell, false)));
    }
    html.push_str("</tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr>");
        for (i, cell) in row.iter().enumerate() {
            html.push_str(&format!("<td style=\"text-align:{}\">{}</td>", align(i), inline(cell, false)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

// MARK: - Footnotes

/// The placeholder [`inline`] leaves, and the only shape [`with_footnotes`] recognises:
/// `<sup class="md-fnref" data-fn="([A-Za-z0-9_-]+)"></sup>`.
const FOOTNOTE_MARKER_OPEN: &str = "<sup class=\"md-fnref\" data-fn=\"";
const FOOTNOTE_MARKER_CLOSE: &str = "\"></sup>";

/// One placeholder in `body`: where it is and its id.
struct FootnoteMarker<'a> {
    start: usize,
    end: usize,
    id: &'a str,
}

/// Every placeholder, left to right, as md.win's regex finds them. The id class excludes `"`, so
/// the id is the maximal identifier run after the opener and must be followed by the closer; a
/// failed candidate is retried at the next `<sup` opener.
fn footnote_markers(body: &str) -> Vec<FootnoteMarker<'_>> {
    let mut markers = Vec::new();
    let mut from = 0;
    while let Some(start) = scalar_text::first_index(body, FOOTNOTE_MARKER_OPEN, from) {
        let id_start = start + FOOTNOTE_MARKER_OPEN.len();
        let id_len =
            body[id_start..].bytes().take_while(|&c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-').count();
        let id_end = id_start + id_len;
        if id_len > 0 && body[id_end..].starts_with(FOOTNOTE_MARKER_CLOSE) {
            let end = id_end + FOOTNOTE_MARKER_CLOSE.len();
            markers.push(FootnoteMarker { start, end, id: &body[id_start..id_end] });
            from = end;
        } else {
            from = start + 1;
        }
    }
    markers
}

/// Turn the placeholder references into numbered links and append the notes. Numbering is by order
/// of first *reference* — the order a reader meets them — not definition order. A reference with no
/// definition goes back to the text the author typed; a definition nobody cited is still printed,
/// after the cited ones, with no back-link.
fn with_footnotes(body: String, definitions: &[(&str, &str)]) -> String {
    let markers = footnote_markers(&body);
    if markers.is_empty() && definitions.is_empty() {
        return body;
    }

    // First definition wins on a duplicate id, as a duplicate link reference would.
    let mut defined: HashMap<&str, &str> = HashMap::new();
    for &(id, text) in definitions {
        defined.entry(id).or_insert(text);
    }

    // Walk the references forward once: each id its number, each reference its occurrence, so
    // repeated citations of one note each get a distinct anchor to come back to.
    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    let mut number: HashMap<&str, usize> = HashMap::new();
    let mut ordered: Vec<&str> = Vec::new();
    let mut references: Vec<usize> = Vec::with_capacity(markers.len());
    for marker in &markers {
        let occurrence = occurrences.get(marker.id).copied().unwrap_or(0) + 1;
        occurrences.insert(marker.id, occurrence);
        references.push(occurrence);
        if defined.contains_key(marker.id) && !number.contains_key(marker.id) {
            number.insert(marker.id, ordered.len() + 1);
            ordered.push(marker.id);
        }
    }
    // Then every still-unnumbered definition, in definition order, takes the trailing numbers.
    for &(id, _) in definitions {
        if number.contains_key(id) {
            continue;
        }
        number.insert(id, ordered.len() + 1);
        ordered.push(id);
    }

    // md.win substitutes back to front so the earlier ranges stay valid; the ranges do not overlap,
    // so building front to back gives the same string.
    let mut result = String::with_capacity(body.len() + 64);
    let mut last = 0;
    for (marker, &occurrence) in markers.iter().zip(&references) {
        result.push_str(&body[last..marker.start]);
        match number.get(marker.id) {
            Some(n) if defined.contains_key(marker.id) => {
                let anchor = if occurrence == 1 { format!("fnref-{n}") } else { format!("fnref-{n}-{occurrence}") };
                result.push_str(&format!("<sup class=\"md-fnref\" id=\"{anchor}\"><a href=\"#fn-{n}\">{n}</a></sup>"));
            }
            _ => result.push_str(&escape(&format!("[^{}]", marker.id))),
        }
        last = marker.end;
    }
    result.push_str(&body[last..]);

    if ordered.is_empty() {
        return result;
    }
    let mut items = String::new();
    for id in &ordered {
        let n = number[id];
        // A note's own text is inline Markdown. A further reference inside it has missed the
        // numbering pass and is cleaned back to literal text below.
        let text = inline(defined.get(id).copied().unwrap_or(""), false);
        // The back-link targets the *first* citation, never `fnref-n-2`.
        let back = if occurrences.contains_key(id) {
            format!(" <a class=\"md-fnback\" href=\"#fnref-{n}\">&#8617;</a>")
        } else {
            String::new()
        };
        items.push_str(&format!("<li id=\"fn-{n}\">{text}{back}</li>"));
    }
    // A leading newline on top of the blank lines the definitions already left in the join.
    result.push_str(&format!("\n<section class=\"md-footnotes\"><hr><ol>{items}</ol></section>"));
    // No `data-fn=` may survive into the page. The template is literally `[^$1]`.
    let leftovers = footnote_markers(&result);
    if leftovers.is_empty() {
        return result;
    }
    let mut cleaned = String::with_capacity(result.len());
    let mut last = 0;
    for marker in &leftovers {
        cleaned.push_str(&result[last..marker.start]);
        cleaned.push_str("[^");
        cleaned.push_str(marker.id);
        cleaned.push(']');
        last = marker.end;
    }
    cleaned.push_str(&result[last..]);
    cleaned
}

// MARK: - Inline

/// A block's inline Markdown to HTML — code and math spans lifted out first, the rest escaped, span
/// syntax converted, soft breaks (paragraphs only, `soft_breaks`) inserted, the protected spans
/// restored. The private `inline_html` module spells out the phase order, which is the
/// specification. (md.win's `softBreaks` defaults to `false`.)
pub fn inline(text: &str, soft_breaks: bool) -> String {
    inline_html::inline(text, soft_breaks)
}

/// The four-character HTML escape: `&amp;` first, then `&lt;`, `&gt;`, `&quot;`; the apostrophe
/// untouched.
pub fn escape(s: &str) -> String {
    inline_html::escape(s)
}
