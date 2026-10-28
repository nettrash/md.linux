//! The pure side of "export one diagram as a real vector `.svg` file": which blocks a document
//! offers, and the fix-up that turns a diagram's rendered root `<svg>` (read out of the offscreen
//! `WebView` DOM as `outerHTML`) into a self-standing SVG document. Port of md.win
//! `Md.Core.Export.DiagramSvg` (itself macOS `DiagramSVG` in `DocumentExport.swift`).
//!
//! Only the three *diagram* engines qualify — Mermaid, Graphviz and PlantUML each render to an
//! inline `<svg>` — plus the ```` ```plot ```` fence, which is already a real `<svg>` in the markup
//! before any script runs. Math does **not**: KaTeX lays a formula out as HTML + CSS, never SVG, so
//! a formula has no vector to export and is deliberately never offered.
//!
//! [`diagrams`] and `querySelectorAll(DOM_SELECTOR)` must enumerate the same set in the same
//! order: the capture step pulls the matching `<svg>` back out by [`Diagram::ordinal`], so one
//! kind counted here and not there shifts every later figure onto the wrong source. It is
//! deliberately a *different* set from the EPUB rich containers — this one has `div.plot` and no
//! formulas, that one has formulas and no plot. Do not derive one from the other.
//!
//! The tag scanning is plain ordinal string work on ASCII needles, so every offset it slices at is
//! a char boundary: the input is a serializer's tag syntax, never author prose. The one place
//! author text does flow through — the fence info string — is folded and matched exactly as the
//! HTML writer folds it.

use crate::markdown::{MarkdownBlock, html, parser};
use crate::text::{graphemes, whitespace};

/// Mermaid — rendered into `<pre class="mermaid">`.
pub const MERMAID: &str = "mermaid";

/// PlantUML — rendered into `<div class="plantuml">`.
pub const PLANT_UML: &str = "plantuml";

/// Graphviz — rendered into `<div class="graphviz" data-engine="…">`.
pub const GRAPHVIZ: &str = "graphviz";

/// The ```` ```plot ```` fence — already an `<svg>` inside `<div class="plot">`.
pub const PLOT: &str = "plot";

/// The DOM query that finds the rendered containers [`diagrams`] describes, in the same document
/// order. The four engine names are also the four container classes, which is what keeps this
/// string and the classifier honest.
pub const DOM_SELECTOR: &str = "pre.mermaid, div.plantuml, div.graphviz, div.plot";

const XML_PROLOG: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const SVG_NAMESPACE_ATTRIBUTE: &str = " xmlns=\"http://www.w3.org/2000/svg\"";
const SVG_OPEN: &str = "<svg";
const DEFAULT_LAYOUT: &str = "dot";
const LABEL_LIMIT: usize = 40;
const ELLIPSIS: char = '\u{2026}'; // HORIZONTAL ELLIPSIS

/// One diagram the document offers for SVG export, in document order. Equality is structural over
/// every field, `layout` included: two Graphviz rows with different layouts are different
/// diagrams.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagram {
    /// 0-based position among the document's rendered diagram containers — the index the DOM
    /// query reports them in, so the app picks the nth match of [`DOM_SELECTOR`]. The saved file
    /// is named with `ordinal + 1`.
    pub ordinal: usize,
    /// Which engine draws it: [`MERMAID`], [`PLANT_UML`], [`GRAPHVIZ`] or [`PLOT`] (Swift's
    /// `Diagram.Kind` raw value, and the container's CSS class).
    pub engine: String,
    /// The diagram's own source — the fence body, or the whole file for a raw `.puml` / `.gv`
    /// document.
    pub source: String,
    /// The menu row, as [`Diagram::of`] composes it.
    pub menu_title: String,
    /// The Graphviz layout program (`dot` / `neato` / …) for a [`GRAPHVIZ`] diagram; `None` for
    /// the others. Swift calls this field `engine`; here that name is taken by the family above.
    /// Menu label only — the capture reads the DOM, not this.
    pub layout: Option<String>,
}

impl Diagram {
    /// The diagram as the menu shows it: the type, plus the source label when there is one.
    pub fn of(ordinal: usize, engine: &str, layout: Option<&str>, source: &str) -> Diagram {
        let mut diagram = Diagram {
            ordinal,
            engine: engine.to_string(),
            source: source.to_string(),
            menu_title: String::new(),
            layout: layout.map(str::to_string),
        };
        let label = diagram.label();
        let type_name = diagram.type_name();
        diagram.menu_title = if label.is_empty() { type_name } else { format!("{type_name}: {label}") };
        diagram
    }

    /// A short label lifted from the diagram's source — its first non-empty line, trimmed and
    /// capped — so a reader can tell two diagrams apart in the menu. Empty when the source has no
    /// non-blank line. A pure function of [`Diagram::source`], as it is in every port.
    pub fn label(&self) -> String {
        first_line(&self.source)
    }

    /// The engine's display name, naming the Graphviz layout when it is not the default `dot` (a
    /// `neato` graph reads quite differently). An engine this module does not know comes back
    /// verbatim rather than panicking — a menu row is not worth a crash.
    pub fn type_name(&self) -> String {
        match self.engine.as_str() {
            MERMAID => "Mermaid".to_string(),
            PLANT_UML => "PlantUML".to_string(),
            GRAPHVIZ => match self.layout.as_deref() {
                Some(layout) if layout != DEFAULT_LAYOUT => format!("Graphviz ({layout})"),
                _ => "Graphviz".to_string(),
            },
            PLOT => "Plot".to_string(),
            other => other.to_string(),
        }
    }
}

/// The diagrams a document offers, in document order.
///
/// Mirrors exactly how the HTML writer decides what becomes a diagram, so this list pairs
/// index-for-index with the rendered DOM's containers:
///
/// - a raw `.puml` / `.gv` document is one diagram — the whole file, which the writer renders
///   without parsing Markdown at all;
/// - otherwise every fenced block whose info string names Mermaid, PlantUML, a Graphviz layout or
///   `plot` — **including one nested in a block quote**, which the writer draws by recursing into
///   the quote, so the walk recurses too and the quoted diagram keeps its place. Only quotes
///   recurse; lists cannot contain fences in this parser.
///
/// Math fences and every other code block are skipped: a formula is not SVG, and ordinary code is
/// not a diagram.
pub fn diagrams(source: &str) -> Vec<Diagram> {
    if parser::is_raw_plant_uml(source) {
        return vec![Diagram::of(0, PLANT_UML, None, source)];
    }
    if parser::is_raw_graphviz(source) {
        return vec![Diagram::of(0, GRAPHVIZ, Some(DEFAULT_LAYOUT), source)];
    }
    let mut found = Vec::new();
    append(&parser::parse(source), &mut found);
    found
}

fn append(blocks: &[MarkdownBlock], into: &mut Vec<Diagram>) {
    for block in blocks {
        match block {
            MarkdownBlock::CodeBlock { language, code } => {
                if let Some((engine, layout)) = classify(language.as_deref()) {
                    into.push(Diagram::of(into.len(), engine, layout, code));
                }
            }
            MarkdownBlock::Quote { blocks } => append(blocks, into),
            _ => {}
        }
    }
}

/// Classify a fence info string the way the HTML writer does — same fold, same families, same
/// alias table — or `None` for anything that is not a diagram (math, csv, plain code, a bare
/// fence).
///
/// The fold is the writer's own `full_lowercase` (Swift's `lowercased()`), and the Graphviz aliases
/// are the writer's own table ([`html::graphviz_engine`]) — ONE copy, as md.win's comment asks once
/// both are in the tree: a layout added to one table and not the other offers a fence the renderer
/// will not draw, and the ordinals part company. (md.win folds with `ToLowerInvariant` here and
/// `FullLowercase` in its writer; the two agree on every word this classifier accepts.)
fn classify(language: Option<&str>) -> Option<(&'static str, Option<&'static str>)> {
    let lang = crate::text::scalar_text::full_lowercase(language.unwrap_or(""));
    match lang.as_str() {
        MERMAID => return Some((MERMAID, None)),
        "plantuml" | "puml" | "plant-uml" => return Some((PLANT_UML, None)),
        _ => {}
    }
    if let Some(layout) = html::graphviz_engine(&lang) {
        return Some((GRAPHVIZ, Some(layout)));
    }
    // A plot is a diagram here even though no engine draws it: the renderer already put a
    // finished `<svg>` in the container, which is exactly what this command saves. It must be in
    // this list, and `div.plot` in DOM_SELECTOR, or every later diagram exports the wrong figure.
    if lang == PLOT {
        return Some((PLOT, None));
    }
    None
}

/// The first non-empty line of `source`, trimmed and capped so one long line cannot dwarf the
/// menu. Purely cosmetic — a human reads it, nothing re-parses it.
///
/// The splitter is the wide, Foundation `.newlines` set (Swift's `Character.isNewline`, U+0085 /
/// U+2028 / U+2029 included, CRLF as one), not the block scanner's CR/LF one; the trim is
/// `.whitespaces` (no line terminators, U+200B in), never `str::trim`. The cap counts UAX #29
/// extended grapheme clusters — Swift's `Character` count. Unlike md.win, whose .NET text elements
/// predate GB9c and split an Indic conjunct (क + virama + ष) in two, `unicode-segmentation` applies
/// GB9c, so a Devanagari label is cut where macOS cuts it (pinned in the adversarial suite).
fn first_line(source: &str) -> String {
    for raw in whitespace::newline_set_lines(source) {
        let trimmed = whitespace::trim_ws(raw);
        if trimmed.is_empty() {
            continue;
        }
        if graphemes::count(trimmed) <= LABEL_LIMIT {
            return trimmed.to_string();
        }
        let mut capped = whitespace::trim_ws(graphemes::prefix(trimmed, LABEL_LIMIT)).to_string();
        capped.push(ELLIPSIS);
        return capped;
    }
    String::new()
}

// MARK: - SVG fix-up

/// Turn a diagram's rendered root `<svg …>…</svg>` into a standalone `.svg` document: guarantee
/// the SVG namespace, give an unsized root real pixel dimensions from its `viewBox`, and prepend
/// the XML prolog so the file is a well-formed standalone document any browser or vector editor
/// opens.
///
/// Mermaid emits `width="100%"` and no `height` — fine inside a flowing page (the page CSS caps
/// it), useless in a file, where it renders at zero or full-viewport height. Graphviz and PlantUML
/// already write absolute `width`/`height`, and a plot writes both plus the namespace, so those are
/// left exactly as they were drawn.
///
/// The order is load-bearing: namespace first, then size. Both insert right after `<svg`, so the
/// other order gives the same attributes in different bytes.
pub fn standalone_document(svg_outer_html: &str) -> String {
    [XML_PROLOG, &with_resolved_size(&in_namespaced(svg_outer_html))].concat()
}

/// The root `<svg …>` opening tag as `start..end`, or `None` when there is not one — then only the
/// prolog is added. Engine `outerHTML` never puts a `>` inside the root tag's attribute values, so
/// the first `>` really does close it. The scan is `find("<svg")`, not a parser: a commented-out
/// root comes first, exactly as on every other port.
fn opening_tag(svg: &str) -> Option<(usize, usize)> {
    let open = svg.find(SVG_OPEN)?;
    let close = svg[open + SVG_OPEN.len()..].find('>')? + open + SVG_OPEN.len();
    Some((open, close + 1))
}

/// Ensure the root carries the default SVG namespace so the standalone file is well-formed. Every
/// engine already declares it, but a file must not lean on that.
fn in_namespaced(svg: &str) -> String {
    let Some((start, end)) = opening_tag(svg) else {
        return svg.to_string();
    };
    if attribute("xmlns", &svg[start..end]).is_some() {
        return svg.to_string();
    }
    // Right after `<svg`, before the other attributes.
    let at = start + SVG_OPEN.len();
    [&svg[..at], SVG_NAMESPACE_ATTRIBUTE, &svg[at..]].concat()
}

/// Give the root real dimensions when it lacks them. If both `width` and `height` are already
/// absolute lengths the engine sized it — leave it untouched. Otherwise, when a 4-number `viewBox`
/// is present, set `width`/`height` to the viewBox's own width and height (copied verbatim, so
/// `0.00 0.00 120.00 48.00` would give `"120.00"`), which is what makes a Mermaid `width="100%"`
/// file open at its true size.
fn with_resolved_size(svg: &str) -> String {
    let Some((start, end)) = opening_tag(svg) else {
        return svg.to_string();
    };
    let tag = &svg[start..end];
    if is_absolute_length(attribute("width", tag)) && is_absolute_length(attribute("height", tag)) {
        return svg.to_string();
    }
    let Some(view_box) = view_box(tag) else {
        return svg.to_string();
    };
    if view_box.len() != 4 {
        return svg.to_string();
    }
    let resized = set_attribute("width", view_box[2], tag);
    let resized = set_attribute("height", view_box[3], &resized);
    [&svg[..start], &resized, &svg[end..]].concat()
}

/// The value span of a whole attribute `name="…"` (or `name='…'`) inside an opening tag. **The
/// leading space is load-bearing**: it matches only a whole attribute, so `width` never captures
/// `stroke-width`. Double quotes are tried first.
fn attribute_value_range(name: &str, tag: &str) -> Option<(usize, usize)> {
    for quote in ['"', '\''] {
        let key = format!(" {name}={quote}");
        let Some(at) = tag.find(&key) else {
            continue;
        };
        let value_start = at + key.len();
        let Some(close) = tag[value_start..].find(quote) else {
            continue;
        };
        return Some((value_start, value_start + close));
    }
    None
}

fn attribute<'a>(name: &str, tag: &'a str) -> Option<&'a str> {
    attribute_value_range(name, tag).map(|(start, end)| &tag[start..end])
}

/// Set `name`'s value inside its own quotes, or add `name="value"` just past `<svg` when absent.
fn set_attribute(name: &str, value: &str, tag: &str) -> String {
    if let Some((start, end)) = attribute_value_range(name, tag) {
        return [&tag[..start], value, &tag[end..]].concat();
    }
    let at = SVG_OPEN.len();
    [&tag[..at], " ", name, "=\"", value, "\"", &tag[at..]].concat()
}

/// Whether an attribute value is an absolute SVG length: present, and a number (optionally with a
/// unit like `pt`/`px`), but not a percentage. A missing value and `width="100%"` are both "not
/// absolute", which is exactly what makes a Mermaid root get resized and a Graphviz root not.
///
/// The digit test is ASCII, as md.win's and md.vscode's are. macOS asks `Character.isNumber`,
/// which is true for every numeric scalar (`٥ ５ Ⅰ 〇 一 ½ ① 𝟠`), so a root sized `width="٥"` is
/// left alone there and resized here. They agree on ASCII, which is the whole of the real input
/// set — Mermaid, Graphviz, PlantUML and the plot writer all emit ASCII numerals. The trim is the
/// `.whitespaces` set, not `str::trim`.
fn is_absolute_length(value: Option<&str>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let trimmed = whitespace::trim_ws(value);
    if trimmed.is_empty() || trimmed.ends_with('%') {
        return false;
    }
    let first = trimmed.as_bytes()[0];
    first == b'.' || first.is_ascii_digit()
}

/// The `viewBox`'s space/comma-separated tokens, or `None`. Empty tokens are dropped, so a run of
/// spaces does not manufacture a phantom fifth token.
fn view_box(tag: &str) -> Option<Vec<&str>> {
    attribute("viewBox", tag).map(|value| value.split([' ', ',']).filter(|token| !token.is_empty()).collect())
}
