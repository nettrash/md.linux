//! The EPUB 3 package: container, OPF, navigation document, the XHTML fixer, the rich-element to
//! image swap, the title-derived identifier and the stored-zip assembly. Port of md.win
//! `Md.Core.Export.EpubExport` (`EpubExport.cs`), itself a port of macOS `EPUBExport` in
//! `DocumentExport.swift`.
//!
//! Nothing here touches a browser. The app's half is exactly one thing: a PNG per rich element, in
//! document order, for each unit this module says has any. It gets the page to load from
//! [`EpubUnitPlan::document`], finds the elements with `querySelectorAll(`[`RICH_SELECTOR`]`)`, and
//! hands the bytes back — see [`assemble`]. A count that does not match the markup is refused with
//! [`EpubSnapshotCountError`] rather than shipping a book with its pictures shifted.
//!
//! There is deliberately no captured-DOM input. The EPUB body is the writer's own markup: every
//! rich container becomes an `<img>`, a ```` ```plot ```` fence is already a finished `<svg>`, and
//! code blocks stay plain because highlight.js only ever coloured the live DOM. The self-contained
//! HTML export is the path that needs a capture; this one does not.
//!
//! Byte parity is with macOS, which the three shipping ports do not have with each other:
//! `<meta charset="utf-8"/>` in every head (Android and md.vscode omit it), the book's own title in
//! the nav (Android writes the literal `Contents`), the export stylesheet plus a padding override
//! (Android ships a hand-written sheet), `unit-000` for the title page (Android starts at
//! `unit-001`), manifest ids `bookid` / `u0` / `i0`, image names `images/unit-000-rich-0.png`, and
//! the blank line before `</body>` that falls out of stripping the md-init script from an
//! already-trimmed body.
//!
//! Offsets ([`RichElement::start`], [`RichElement::length`]) are UTF-8 byte offsets on char
//! boundaries, where md.win's are UTF-16 units: every marker is ASCII, and nothing outside this
//! module reads them — the app pairs by count and order, never by offset.
//!
//! md.win's two regexes (the script stripper and the void-element closer) are hand-written
//! matchers here, each documented with the pattern it replaces and the backtracking argument that
//! makes the walk exact.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use super::epub_metadata::{self, CoverReference, EpubCover, EpubMetadata};
use super::zip_writer::{ZipEntry, ZipError, ZipWriter};
use crate::book::structured_book::{BookUnit, StructuredBook};
use crate::markdown::{MetadataField, OutlineEntry, html, parser};
use crate::text::unicode::{self, GeneralCategory};
use crate::text::whitespace;

// MARK: - Types

/// One rich container as it sits in the rendered markup: where it starts, how long it is (UTF-8
/// bytes), and whether it is mathematics (which decides the replacement image's `alt` text).
///
/// The order of these is the order `querySelectorAll(`[`RICH_SELECTOR`]`)` reports the same
/// elements in the live DOM, which is the whole pairing contract with the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RichElement {
    pub start: usize,
    pub length: usize,
    pub is_math: bool,
}

impl RichElement {
    pub fn new(start: usize, length: usize, is_math: bool) -> Self {
        RichElement { start, length, is_math }
    }

    /// One past the last byte of the element (its close tag included).
    pub fn end(&self) -> usize {
        self.start + self.length
    }

    /// The `alt` text the replacement image carries.
    pub fn alt(&self) -> &'static str {
        if self.is_math { "formula" } else { "diagram" }
    }
}

/// One photographed rich element handed back by the app: the PNG bytes and the element's layout
/// width in CSS px.
///
/// The PNG is captured at 2× so a formula stays crisp; [`RichSnapshot::display_width`] pins the
/// displayed size back to the layout width, and the stylesheet's `max-width: 100%` still shrinks it
/// on a narrow reader. Equality is structural, over the bytes, and treats two NaN widths as equal
/// (.NET's `double.Equals`).
#[derive(Clone)]
pub struct RichSnapshot {
    pub png: Vec<u8>,
    pub width: f64,
}

impl RichSnapshot {
    pub fn new(png: impl Into<Vec<u8>>, width: f64) -> Self {
        RichSnapshot { png: png.into(), width }
    }

    /// The `width` attribute's value: the layout width rounded half away from zero — Swift's
    /// `Int(width.rounded())`, which is `f64::round`, **not** .NET's banker's default. Out of range
    /// saturates and NaN is 0, as .NET 9+ converts.
    pub fn display_width(&self) -> i32 {
        self.width.round() as i32
    }
}

impl PartialEq for RichSnapshot {
    fn eq(&self, other: &Self) -> bool {
        let same_width = self.width == other.width || (self.width.is_nan() && other.width.is_nan());
        same_width && self.png == other.png
    }
}

impl Eq for RichSnapshot {}

/// md.win's `ToString`: the size and the width, never the bytes.
impl fmt::Debug for RichSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RichSnapshot({} bytes, {})", self.png.len(), self.width)
    }
}

/// One image resource inside `OEBPS/`: its archive-relative name and its bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EpubImage {
    pub file: String,
    pub data: Vec<u8>,
}

impl EpubImage {
    pub fn new(file: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        EpubImage { file: file.into(), data: data.into() }
    }
}

impl fmt::Debug for EpubImage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EpubImage({}, {} bytes)", self.file, self.data.len())
    }
}

/// One finished spine entry: its file name inside `OEBPS/` and its XHTML document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubUnit {
    pub file: String,
    pub title: String,
    pub xhtml: String,
}

impl EpubUnit {
    pub fn new(file: impl Into<String>, title: impl Into<String>, xhtml: impl Into<String>) -> Self {
        EpubUnit { file: file.into(), title: title.into(), xhtml: xhtml.into() }
    }
}

/// One table-of-contents row. Chapters carry their articles as `children`, nested one level like
/// the book itself; a single document's rows are heading anchors and never nest. md.win's `null`
/// children and an empty list write the same bytes; here both are the empty `Vec`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NavEntry {
    pub title: String,
    pub file: String,
    pub children: Vec<NavEntry>,
}

impl NavEntry {
    pub fn new(title: impl Into<String>, file: impl Into<String>) -> Self {
        NavEntry { title: title.into(), file: file.into(), children: Vec::new() }
    }

    pub fn with_children(title: impl Into<String>, file: impl Into<String>, children: Vec<NavEntry>) -> Self {
        NavEntry { title: title.into(), file: file.into(), children }
    }
}

/// One planned unit: everything the app needs to photograph it, and everything [`assemble`] needs
/// to finish it once the photographs come back.
///
/// `document` is the full standalone page the app must load in its offscreen renderer (empty for a
/// heading page, which has nothing to render). `body` is the extracted body markup — rich
/// containers still holding their source, the md-init script still on the end; [`assemble`] does
/// the replacement and the XHTML fix-up, in that order.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubUnitPlan {
    pub index: usize,
    pub file: String,
    pub title: String,
    pub document: String,
    pub body: String,
    pub rich_elements: Vec<RichElement>,
    pub is_heading_page: bool,
}

/// An export that is complete but for the photographs: the identifier, the reading order and the
/// navigation are all fixed here, so nothing a browser does can change the shape of the book.
///
/// `metadata` and `cover` come from a single document's front matter ([`epub_metadata`]); a book
/// keeps the defaults (`None`). The cover is a reference, not bytes: reading it — a file beside the
/// document, an https fetch — is the app's, and the bytes it finds go to [`assemble`] through
/// [`epub_metadata::recognise`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubPlan {
    pub title: String,
    pub identifier: String,
    pub units: Vec<EpubUnitPlan>,
    pub nav_entries: Vec<NavEntry>,
    pub metadata: Option<EpubMetadata>,
    pub cover: Option<CoverReference>,
}

impl EpubPlan {
    /// The units that carry rich content, in reading order — the ones the app must render.
    pub fn rich_units(&self) -> Vec<&EpubUnitPlan> {
        self.units.iter().filter(|unit| !unit.rich_elements.is_empty()).collect()
    }
}

/// The app handed back a number of snapshots that does not match the number of rich elements in
/// the unit's markup — the one failure that would otherwise corrupt a book silently, shifting
/// every later image onto the wrong element. md.win's `EpubSnapshotCountException`.
///
/// Swift's `zip(matches, tags)` pairs up to the shorter list and leaves the rest as source, which
/// loses a diagram without saying so; Android aborts the export on exactly this count check.
/// md.win — and this port — takes Swift's replacement semantics (see [`replacing_rich_elements`],
/// which the tests pin) and Android's guard on the way in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubSnapshotCountError {
    /// The unit's index in the spine.
    pub unit_index: usize,
    /// The unit's display title.
    pub unit_title: String,
    /// How many rich containers the markup holds.
    pub expected: usize,
    /// How many snapshots the app supplied.
    pub actual: usize,
}

impl fmt::Display for EpubSnapshotCountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EPUB unit {} (\"{}\") has {} rich element(s) in its markup but {} snapshot(s) were supplied.",
            self.unit_index, self.unit_title, self.expected, self.actual
        )
    }
}

impl std::error::Error for EpubSnapshotCountError {}

/// Why an archive could not be built. md.win throws the first and lets the zip writer's
/// `OverflowException` escape for the second; both map to the app's "Could not export EPUB".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EpubError {
    /// A unit's snapshot count does not match its rich-element count.
    SnapshotCount(EpubSnapshotCountError),
    /// The archive does not fit the classic zip format (a 4 GiB book).
    Zip(ZipError),
}

impl fmt::Display for EpubError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EpubError::SnapshotCount(error) => error.fmt(f),
            EpubError::Zip(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for EpubError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EpubError::SnapshotCount(error) => Some(error),
            EpubError::Zip(error) => Some(error),
        }
    }
}

impl From<EpubSnapshotCountError> for EpubError {
    fn from(error: EpubSnapshotCountError) -> Self {
        EpubError::SnapshotCount(error)
    }
}

impl From<ZipError> for EpubError {
    fn from(error: ZipError) -> Self {
        EpubError::Zip(error)
    }
}

// MARK: - Fixed strings

/// The container identity — the FIRST zip entry, stored uncompressed, exactly these bytes.
pub const MIMETYPE: &str = "application/epub+zip";

/// The single document's one content file.
pub const CONTENT_FILE_NAME: &str = "content.xhtml";

/// The DOM query that finds the same containers [`rich_elements`] finds in the markup, in the same
/// order. This, [`contains_rich_content`] and the marker table must be changed together: a class
/// one of them misses either skips the snapshot pass entirely or shifts every later image onto the
/// wrong element. It is deliberately a different set from the diagram-SVG export's DOM selector —
/// that one has `div.plot` and no formulas.
pub const RICH_SELECTOR: &str = ".md-mathi, .md-mathd, .mermaid, .plantuml, .graphviz";

/// Points the reader at the package document. No trailing newline.
pub const CONTAINER_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\n",
    "<rootfiles>\n",
    "<rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/>\n",
    "</rootfiles>\n",
    "</container>"
);

// MARK: - Titles

/// The EPUB title for a single document: the front-matter `title:` field if the author gave a
/// non-empty one, else the file name.
///
/// A book takes its title from its folder name; a lone document has no folder, so the file name is
/// the closest thing to a title it has — and the title is also what [`stable_identifier`] hashes,
/// so two exports of the same document reach the same identifier. The key match is
/// case-insensitive because generators write `title:` and `Title:` alike; the first non-empty one
/// wins.
pub fn document_title(source: &str, file_name: &str) -> String {
    document_title_from_front_matter(&parser::front_matter(source), file_name)
}

/// The same, for a caller that has already parsed the front matter (Swift's shape; md.win's
/// second `DocumentTitle` overload).
///
/// The value is trimmed with Foundation's `.whitespacesAndNewlines`, which is NOT .NET's
/// `string.Trim()`: the frozen tables put U+200B in the set and `char.IsWhiteSpace` does not, so a
/// title of three zero-width spaces must fall through to the file name. The ASCII key fold is exact
/// (see [`epub_metadata::field_value`]).
pub fn document_title_from_front_matter(front_matter: &[MetadataField], file_name: &str) -> String {
    epub_metadata::field_value(front_matter, "title").unwrap_or_else(|| file_name.to_string())
}

// MARK: - Body extraction and the XHTML fixer

/// Whether the rendered markup carries anything the engines must typeset — the trigger for the
/// snapshot pass.
///
/// Each test stops at the class value's closing quote, so the Graphviz container matches whatever
/// else its tag carries (`<div class="graphviz" data-engine="neato">`). `class="plot"` is
/// deliberately absent: a ```` ```plot ```` fence is finished `<svg>` by the time this runs, so a
/// plot-only document never opens a renderer — and travels into the book as vector rather than as
/// a photograph of itself.
pub fn contains_rich_content(html: &str) -> bool {
    html.contains("class=\"md-mathi\"")
        || html.contains("class=\"md-mathd\"")
        || html.contains("class=\"mermaid\"")
        || html.contains("class=\"plantuml\"")
        || html.contains("class=\"graphviz\"")
}

/// The renderer's `<body>` content: everything between the body tag and the LAST `</body>`,
/// trimmed (Foundation's `.whitespacesAndNewlines`). The md-init script is still on the end —
/// [`xhtml_body`] takes it off and leaves the newline that preceded it, which is where every
/// content file's blank line before `</body>` comes from.
///
/// Searched backwards because a code block quoting `</body>` is escaped and cannot match. Anything
/// missing hands the input back untouched; so does a close tag before the open one, where the Swift
/// would trap on the range (md.vscode guards it the same way).
pub fn body_html(document: &str) -> &str {
    let Some(start) = document.find("<body") else {
        return document;
    };
    let Some(open) = document[start + "<body".len()..].find('>').map(|at| start + "<body".len() + at) else {
        return document;
    };
    match document.rfind("</body>") {
        Some(end) if end > open => whitespace::trim_wsnl(&document[open + 1..end]),
        _ => document,
    }
}

/// Post-process rendered HTML into well-formed XHTML: scripts stripped, void elements self-closed,
/// and the one named entity the writer uses replaced with its numeric form — XML predefines only
/// amp / lt / gt / quot / apos.
pub fn xhtml_body(html: &str) -> String {
    let output = strip_script_elements(html);
    let output = self_close_void_elements(&output);
    output.replace("&bull;", "&#8226;")
}

/// md.win's `ScriptElement.Replace(html, "")` over `<script[^>]*>[\s\S]*?</script>` (case-sensitive;
/// `[\s\S]` is "any character", so the body may span lines).
///
/// From each `<script`, `[^>]*>` can only end at the first `>` after it, and the lazy body at the
/// first `</script>` after that. When either is missing at one start, it is missing at every later
/// start too (anything after a later start is after this one), so the scan stops there and the
/// rest of the markup is copied untouched — exactly what the regex leaves.
fn strip_script_elements(html: &str) -> String {
    const OPEN: &str = "<script";
    const CLOSE: &str = "</script>";
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(found) = html[cursor..].find(OPEN) {
        let start = cursor + found;
        let after_name = start + OPEN.len();
        let Some(tag_end) = html[after_name..].find('>') else {
            break;
        };
        let content = after_name + tag_end + 1;
        let Some(close) = html[content..].find(CLOSE) else {
            break;
        };
        output.push_str(&html[cursor..start]);
        cursor = content + close + CLOSE.len();
    }
    output.push_str(&html[cursor..]);
    output
}

/// md.win's `VoidElement.Replace(output, "<$1$2/>")` over
/// `<(br|hr|img)((?:[^>"]|"[^"]*")*?)[\t\n\v\f\r\x85\p{Z}]*/?>` — the void elements the writer
/// emits. Case-sensitive, and with no word boundary after the name, exactly as the regex has it.
fn self_close_void_elements(html: &str) -> String {
    let mut output = String::with_capacity(html.len() + 16);
    let mut copied = 0;
    let mut search = 0;
    while let Some(found) = html[search..].find('<') {
        let start = search + found;
        match void_element_at(html, start) {
            Some(VoidMatch { name_end, attributes_end, end }) => {
                output.push_str(&html[copied..start]);
                output.push('<');
                output.push_str(&html[start + 1..name_end]);
                output.push_str(&html[name_end..attributes_end]);
                output.push_str("/>");
                copied = end;
                search = end;
            }
            None => search = start + 1,
        }
    }
    output.push_str(&html[copied..]);
    output
}

/// One match of the void-element pattern: where group 1 (the name) and group 2 (the attribute run)
/// end, and where the whole match ends.
struct VoidMatch {
    name_end: usize,
    attributes_end: usize,
    end: usize,
}

/// The regex's attempt at `start` (a `<`), walked deterministically.
///
/// Group 2 is lazy, so the engine tries the tail `[ws]*/?>` after zero iterations, then after one,
/// and so on. The tail matches at a position exactly when, past a run of whitespace, the next
/// characters are `/>` or `>` (giving whitespace or the slash back can only leave a whitespace or a
/// `/` where the `>` must be). Each iteration is forced: `"` can only begin a quoted value, which
/// must end at the next `"`; any other character but `>` is taken alone; a `>` is never reached,
/// because the tail would have matched first. When an iteration is impossible no alternative
/// remains anywhere, so the attempt fails.
fn void_element_at(html: &str, start: usize) -> Option<VoidMatch> {
    let after = &html[start + 1..];
    let name = ["br", "hr", "img"].into_iter().find(|name| after.starts_with(name))?;
    let name_end = start + 1 + name.len();
    let mut at = name_end;
    loop {
        let rest = &html[at..];
        let past_space = rest.trim_start_matches(is_regex_space);
        let space = rest.len() - past_space.len();
        if past_space.starts_with("/>") {
            return Some(VoidMatch { name_end, attributes_end: at, end: at + space + 2 });
        }
        if past_space.starts_with('>') {
            return Some(VoidMatch { name_end, attributes_end: at, end: at + space + 1 });
        }
        match rest.chars().next()? {
            '>' => return None,
            '"' => at += 1 + rest[1..].find('"')? + 1,
            c => at += c.len_utf8(),
        }
    }
}

/// The fixer's one whitespace class, `[\t\n\v\f\r\x85\p{Z}]` — ICU's `\s` spelled out so no regex
/// engine decides what whitespace is. `\p{Z}` is .NET's Zs, Zl and Zp (U+200B is Cf, not in it).
fn is_regex_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}')
        || matches!(
            unicode::category(c),
            GeneralCategory::SpaceSeparator | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator
        )
}

// MARK: - Rich elements

/// The writer's six rich containers: opener, close tag, and whether it is mathematics. Their
/// content is escaped text, so the first close tag is always the element's own.
///
/// The Graphviz opener stops at the class attribute's closing quote rather than at the tag's `>`:
/// the writer names the layout program in the tag as well, so a whole-tag literal would recognise
/// none of the eight engines and every DOT diagram would ship as raw source.
const RICH_MARKERS: [(&str, &str, bool); 6] = [
    ("<span class=\"md-mathi\">", "</span>", true),
    ("<span class=\"md-mathd\">", "</span>", true),
    ("<div class=\"md-mathd\">", "</div>", true),
    ("<pre class=\"mermaid\">", "</pre>", false),
    ("<div class=\"plantuml\">", "</div>", false),
    ("<div class=\"graphviz\"", "</div>", false),
];

/// Every rich element in `html`, in document order — the same order the DOM query reports, so
/// captures and replacements pair up by index.
///
/// From a cursor, take the earliest of the six openers, find its close after it, emit the span and
/// move past it. Non-overlapping by construction, and an unclosed opener ends the scan (as in
/// Kotlin and TypeScript) rather than skipping ahead to a later element.
pub fn rich_elements(html: &str) -> Vec<RichElement> {
    let mut found = Vec::new();
    let mut index = 0;
    loop {
        let mut earliest: Option<(usize, &str, &str, bool)> = None;
        for (open, close, is_math) in RICH_MARKERS {
            let Some(at) = html[index..].find(open).map(|at| index + at) else {
                continue;
            };
            if earliest.is_some_and(|(start, ..)| at >= start) {
                continue;
            }
            earliest = Some((at, open, close, is_math));
        }
        let Some((start, open, close, is_math)) = earliest else {
            return found;
        };
        let content = start + open.len();
        let Some(close_at) = html[content..].find(close).map(|at| content + at) else {
            return found;
        };
        let end = close_at + close.len();
        found.push(RichElement::new(start, end - start, is_math));
        index = end;
    }
}

/// Replace each rich element in `html` — in document order — with its image tag. Empty `tags`
/// leaves the markup alone; a short list pairs up to the shorter of the two, exactly as Swift's
/// `zip` does.
pub fn replacing_rich_elements(html: &str, tags: &[&str]) -> String {
    if tags.is_empty() {
        return html.to_string();
    }
    let elements = rich_elements(html);
    if elements.is_empty() {
        return html.to_string();
    }
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;
    for (element, tag) in elements.iter().zip(tags) {
        output.push_str(&html[cursor..element.start]);
        output.push_str(tag);
        cursor = element.end();
    }
    output.push_str(&html[cursor..]);
    output
}

/// The replacement tag for one photographed element.
pub fn image_tag(file: &str, is_math: bool, width: i32) -> String {
    format!("<img src=\"{file}\" alt=\"{}\" width=\"{width}\"/>", if is_math { "formula" } else { "diagram" })
}

/// Where one unit's photographs live: `images/unit-000-rich-0.png`.
pub fn image_file_name(unit_index: usize, element_index: usize) -> String {
    format!("images/unit-{unit_index:03}-rich-{element_index}.png")
}

/// A book unit's file name: `unit-000.xhtml` is always the title page.
pub fn unit_file_name(index: usize) -> String {
    format!("unit-{index:03}.xhtml")
}

// MARK: - The XML documents

/// One XHTML5 content document, in the EPUB's shared stylesheet.
pub fn xhtml_document(title: &str, body: &str) -> String {
    [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<!DOCTYPE html>",
        "<html xmlns=\"http://www.w3.org/1999/xhtml\">",
        "<head>",
        "<meta charset=\"utf-8\"/>",
        &format!("<title>{}</title>", xml_escape(title)),
        "<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>",
        "</head>",
        "<body>",
        body,
        "</body>",
        "</html>",
    ]
    .join("\n")
}

/// The EPUB 3 navigation document: root articles first, then each chapter with its articles
/// nested one level below it. A single document's entries are heading anchors into its one content
/// file, so they never nest.
pub fn nav(title: &str, entries: &[NavEntry]) -> String {
    let title = xml_escape(title);
    [
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "<!DOCTYPE html>",
        "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">",
        "<head>",
        "<meta charset=\"utf-8\"/>",
        &format!("<title>{title}</title>"),
        "<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>",
        "</head>",
        "<body>",
        "<nav epub:type=\"toc\">",
        &format!("<h1>{title}</h1>{}</nav>", nav_list(entries)),
        "</body>",
        "</html>",
    ]
    .join("\n")
}

fn nav_list(entries: &[NavEntry]) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let items: Vec<String> = entries
        .iter()
        .map(|entry| {
            format!("<li><a href=\"{}\">{}</a>{}</li>", entry.file, xml_escape(&entry.title), nav_list(&entry.children))
        })
        .collect();
    format!("\n<ol>\n{}\n</ol>\n", items.join("\n"))
}

/// The package document: metadata, a manifest of every file, and the spine in reading order.
///
/// `svg_units` names the content documents (by index into `units`) that hold an `<svg>`. EPUB 3
/// requires the reserved manifest property `svg` on each of them — EPUBCheck reports OPF-014
/// without it and the book is invalid. Until the ```` ```plot ```` fence no EPUB this family
/// produced had ever contained one, since the engines all arrive as PNG snapshots. md.win's `null`
/// and an empty collection write the same bytes; here both are the empty slice.
///
/// The identifier and the timestamp are generated, so they are not escaped; the title is. The
/// lines between `dc:title` and `dcterms:modified` are [`epub_metadata::lines`] — with no front
/// matter (`None` or the default), the one `<dc:language>en</dc:language>` of 1.5, byte for byte —
/// and a cover is the manifest's last item, outside the spine.
pub fn opf(
    title: &str,
    identifier: &str,
    modified: &str,
    units: &[&str],
    images: &[&str],
    svg_units: &[usize],
    metadata: Option<&EpubMetadata>,
    cover: Option<&EpubCover>,
) -> String {
    let mut manifest = String::new();
    manifest
        .push_str("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
    manifest.push_str("<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n");
    for (index, unit) in units.iter().enumerate() {
        let properties = if svg_units.contains(&index) { " properties=\"svg\"" } else { "" };
        manifest.push_str(&format!(
            "<item id=\"u{index}\" href=\"{unit}\" media-type=\"application/xhtml+xml\"{properties}/>\n"
        ));
    }
    for (index, image) in images.iter().enumerate() {
        manifest.push_str(&format!("<item id=\"i{index}\" href=\"{image}\" media-type=\"image/png\"/>\n"));
    }
    if let Some(cover) = cover {
        manifest.push_str(&epub_metadata::manifest_item(cover));
        manifest.push('\n');
    }
    let spine: Vec<String> = (0..units.len()).map(|index| format!("<itemref idref=\"u{index}\"/>")).collect();

    let default_metadata;
    let metadata = match metadata {
        Some(metadata) => metadata,
        None => {
            default_metadata = EpubMetadata::default();
            &default_metadata
        }
    };
    let mut lines = vec![
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".to_string(),
        "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">".to_string(),
        "<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">".to_string(),
        format!("<dc:identifier id=\"bookid\">{identifier}</dc:identifier>"),
        format!("<dc:title>{}</dc:title>", xml_escape(title)),
    ];
    lines.extend(epub_metadata::lines(metadata, cover));
    let tail = [
        format!("<meta property=\"dcterms:modified\">{modified}</meta>"),
        "</metadata>".to_string(),
        "<manifest>".to_string(),
        manifest + "</manifest>",
        "<spine>".to_string(),
        spine.join("\n"),
        "</spine>".to_string(),
        "</package>".to_string(),
    ];
    lines.join("\n") + "\n" + &tail.join("\n")
}

/// The book's stylesheet: the export CSS the PDFs use, pulled out of a rendered document, with a
/// light EPUB override — the reader owns pages and margins.
///
/// A quirk to preserve rather than fix: because this is the export CSS verbatim, every EPUB page
/// carries `body { padding: 48px 56px }` before the override and `font-size: 11pt`. That is
/// intentional-by-omission on macOS, and the other two ports each diverge differently — Android
/// ships its own sheet, md.vscode appends a page-break and an image rule instead.
pub fn stylesheet() -> String {
    const OPEN: &str = "<style>";
    let document = html::document("", "style", false, true);
    let css = match (document.find(OPEN), document.find("</style>")) {
        (Some(start), Some(end)) if end >= start + OPEN.len() => &document[start + OPEN.len()..end],
        _ => "",
    };
    format!("{css}\n/* EPUB: the reader owns pages and margins. */\nbody {{ padding: 0.5em 5%; }}\n")
}

/// The URL namespace from RFC 4122 Appendix C.
const URL_NAMESPACE: [u8; 16] =
    [0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8];

/// A stable identifier for a book, derived from its title — an RFC 4122 version 5 (name-based)
/// UUID in the standard URL namespace.
///
/// EPUB's `dc:identifier` is what a reader uses to decide whether two files are the same
/// publication. A fresh random UUID on every export means every export is a *different* book:
/// re-exporting after fixing a typo stacks up beside the old one in the library instead of
/// replacing it, and a store that expects a stable identifier across releases cannot accept the
/// file at all. Renaming the book does change it, which is the right answer.
///
/// SHA-1 because v5 *is* SHA-1 — not a security decision, and it must never be "upgraded": that
/// would orphan every book this family has already issued. The name is hashed as UTF-8, the
/// encoding RFC 4122 §4.3 names and the only one that gives a non-ASCII title the same UUID here as
/// on Apple. (md.win's encoder would turn a lone surrogate into U+FFFD first; a Rust string cannot
/// hold one.) The 16 bytes are formatted as hex by hand, never through a mixed-endian GUID type.
pub fn stable_identifier(title: &str) -> String {
    let mut input = Vec::with_capacity(URL_NAMESPACE.len() + title.len());
    input.extend_from_slice(&URL_NAMESPACE);
    input.extend_from_slice(title.as_bytes());
    let digest = sha1(&input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0F) | 0x50; // version 5
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // RFC 4122 variant

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut uuid = String::with_capacity(45);
    uuid.push_str("urn:uuid:");
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            uuid.push('-');
        }
        uuid.push(char::from(HEX[usize::from(byte >> 4)]));
        uuid.push(char::from(HEX[usize::from(byte & 0x0F)]));
    }
    uuid
}

/// SHA-1 (FIPS 180-4), hand-written because md-core takes no crypto dependency — md.win asks
/// .NET's `SHA1.HashData`. It exists for [`stable_identifier`] alone and is public so the published
/// vectors can pin it (`tests/export_epub_identifier.rs`).
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    // Padding: the message, 0x80, zeros up to 56 mod 64, then the bit length as a big-endian u64.
    let bit_length = (data.len() as u64).wrapping_mul(8);
    let whole = data.len() - data.len() % 64;
    let mut tail = Vec::with_capacity(128);
    tail.extend_from_slice(&data[whole..]);
    tail.push(0x80);
    while tail.len() % 64 != 56 {
        tail.push(0);
    }
    tail.extend_from_slice(&bit_length.to_be_bytes());

    for block in data[..whole].as_chunks::<64>().0.iter().chain(tail.as_chunks::<64>().0) {
        let mut w = [0u32; 80];
        for (word, bytes) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for t in 16..80 {
            w[t] = (w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (t, word) in w.iter().enumerate() {
            let (f, k) = match t {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (value, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *value = value.wrapping_add(add);
        }
    }

    let mut out = [0u8; 20];
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    out
}

/// The `dcterms:modified` timestamp: UTC, second precision (EPUBCheck rejects fractions).
pub fn modified_now() -> String {
    modified(SystemTime::now())
}

/// The same, for a caller that wants a deterministic archive. md.win takes a `DateTimeOffset` and
/// converts it to UTC; a `SystemTime` is already an instant, so there is nothing to relabel. A
/// fraction of a second is dropped (rounded towards the past), as `ToString("ss")` drops it.
pub fn modified(when: SystemTime) -> String {
    let seconds = match when.duration_since(UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(before) => {
            let before = before.duration();
            let whole = i64::try_from(before.as_secs()).unwrap_or(i64::MAX);
            -whole - i64::from(before.subsec_nanos() > 0)
        }
    };
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3600,
        second_of_day / 60 % 60,
        second_of_day % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// md.win's `XmlEscape`: `&` first, then `<`, `>`, `"` — and never the apostrophe, which the Apple
/// editions and md.vscode leave alone (byte parity; XML would accept `&apos;` too).
pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// MARK: - Planning (pure; what the app must render and photograph)

/// Plan a single document: one content file, and a navigation built from the document's own
/// outline. `metadata` and `cover` are read from the front matter.
///
/// The trap a book export carries and a document must not: [`plan_book`] makes its first unit a
/// title page and counts the nav from the units past it, so reusing that path with the title page
/// removed would leave the nav pointing one file short. Here there is exactly one unit and every
/// nav row is a heading anchor into it — the slug [`parser::outline`] assigns is the same `id` the
/// HTML writer gives that heading, so a nav tap lands on the section rather than on nothing. A
/// document with no headings would leave the toc list empty, which is not valid EPUB 3, so it falls
/// back to one entry: the whole document, under its title, linking at the content file.
pub fn plan_document(source: &str, title: &str) -> EpubPlan {
    let document = html::document(source, title, false, true);
    let body = body_html(&document).to_string();
    let elements = if contains_rich_content(&body) { rich_elements(&body) } else { Vec::new() };
    let unit = EpubUnitPlan {
        index: 0,
        file: CONTENT_FILE_NAME.to_string(),
        title: title.to_string(),
        document,
        body,
        rich_elements: elements,
        is_heading_page: false,
    };
    let fields = parser::front_matter(source);
    EpubPlan {
        title: title.to_string(),
        identifier: stable_identifier(title),
        units: vec![unit],
        nav_entries: document_nav_entries(title, &parser::outline(source)),
        metadata: Some(epub_metadata::from_front_matter(&fields)),
        cover: epub_metadata::reference(epub_metadata::field_value(&fields, "cover").as_deref()),
    }
}

/// Plan a whole book: a title page, then each root article, then per chapter a heading page
/// followed by its articles — the same reading order the PDF compile uses. `metadata` and `cover`
/// stay `None`: a book keeps the defaults.
pub fn plan_book(book: &StructuredBook) -> EpubPlan {
    let mut units: Vec<EpubUnitPlan> = Vec::new();
    let mut entries: Vec<NavEntry> = Vec::new();

    fn heading_unit(index: usize, title: &str) -> EpubUnitPlan {
        EpubUnitPlan {
            index,
            file: unit_file_name(index),
            title: title.to_string(),
            document: String::new(),
            body: format!("<h1>{}</h1>", xml_escape(title)),
            rich_elements: Vec::new(),
            is_heading_page: true,
        }
    }

    fn article_unit(index: usize, article: &BookUnit) -> EpubUnitPlan {
        let document = html::document(&article.source, &article.title, false, true);
        let body = body_html(&document).to_string();
        let elements = if contains_rich_content(&body) { rich_elements(&body) } else { Vec::new() };
        EpubUnitPlan {
            index,
            file: unit_file_name(index),
            title: article.title.clone(),
            document,
            body,
            rich_elements: elements,
            is_heading_page: false,
        }
    }

    units.push(heading_unit(0, &book.title)); // unit-000 — the title page
    for article in &book.front_units {
        let unit = article_unit(units.len(), article);
        entries.push(NavEntry::new(article.title.clone(), unit.file.clone()));
        units.push(unit);
    }
    for section in &book.sections {
        let heading = heading_unit(units.len(), &section.title);
        let heading_file = heading.file.clone();
        units.push(heading);
        let mut children = Vec::with_capacity(section.units.len());
        for article in &section.units {
            let unit = article_unit(units.len(), article);
            children.push(NavEntry::new(article.title.clone(), unit.file.clone()));
            units.push(unit);
        }
        entries.push(NavEntry::with_children(section.title.clone(), heading_file, children));
    }
    EpubPlan {
        title: book.title.clone(),
        identifier: stable_identifier(&book.title),
        units,
        nav_entries: entries,
        metadata: None,
        cover: None,
    }
}

fn document_nav_entries(title: &str, outline: &[OutlineEntry]) -> Vec<NavEntry> {
    if outline.is_empty() {
        return vec![NavEntry::new(title, CONTENT_FILE_NAME)];
    }
    outline
        .iter()
        .map(|entry| NavEntry::new(entry.text.clone(), format!("{CONTENT_FILE_NAME}#{}", entry.slug)))
        .collect()
}

// MARK: - Assembly

/// Finish a plan with the app's photographs and pack the archive.
///
/// - `snapshots`: one list of PNGs per unit that has rich elements, keyed by
///   [`EpubUnitPlan::index`] and in document order within the unit. Units with no rich elements may
///   be omitted.
/// - `modified`: the `dcterms:modified` stamp; [`modified_now`] when `None`.
/// - `identifier`: overrides the title-derived identifier (a test seam).
/// - `cover`: the plan's [`EpubPlan::cover`], read by the app and recognised with
///   [`epub_metadata::recognise`]; `None` for none.
///
/// Fails with [`EpubError::SnapshotCount`] when a unit's snapshot count does not match its
/// rich-element count — never a partial book.
pub fn assemble(
    plan: &EpubPlan,
    snapshots: Option<&HashMap<usize, Vec<RichSnapshot>>>,
    modified: Option<&str>,
    identifier: Option<&str>,
    cover: Option<&EpubCover>,
) -> Result<Vec<u8>, EpubError> {
    let finished = finish(plan, &|index| snapshots.and_then(|map| map.get(&index)).map(Vec::as_slice))?;
    let modified = modified.map_or_else(modified_now, str::to_string);
    let identifier = identifier.unwrap_or(&plan.identifier);
    let members = package_members(
        &plan.title,
        &finished.units,
        &plan.nav_entries,
        &finished.images,
        identifier,
        &modified,
        plan.metadata.as_ref(),
        cover,
    );
    // Written straight from the borrowed payloads: the photographs are copied once, into the archive.
    let mut writer = ZipWriter::new();
    for (name, data) in &members {
        writer.add(name, data)?;
    }
    Ok(writer.finish()?)
}

/// The archive's members, in order — the seam the tests drive and the packing runs through
/// ([`assemble`] is these, zipped).
pub fn entries(
    plan: &EpubPlan,
    snapshots: Option<&HashMap<usize, Vec<RichSnapshot>>>,
    modified: Option<&str>,
    identifier: Option<&str>,
    cover: Option<&EpubCover>,
) -> Result<Vec<ZipEntry>, EpubSnapshotCountError> {
    let finished = finish(plan, &|index| snapshots.and_then(|map| map.get(&index)).map(Vec::as_slice))?;
    let modified = modified.map_or_else(modified_now, str::to_string);
    let members = package_members(
        &plan.title,
        &finished.units,
        &plan.nav_entries,
        &finished.images,
        identifier.unwrap_or(&plan.identifier),
        &modified,
        plan.metadata.as_ref(),
        cover,
    );
    Ok(owned(members))
}

/// The finished container's members from finished units: `mimetype` first (stored, per the OCF
/// spec), then META-INF and the OEBPS payload. md.win's second `Entries` overload.
pub fn package_entries(
    title: &str,
    units: &[EpubUnit],
    nav_entries: &[NavEntry],
    images: &[EpubImage],
    identifier: &str,
    modified: &str,
    metadata: Option<&EpubMetadata>,
    cover: Option<&EpubCover>,
) -> Vec<ZipEntry> {
    let images: Vec<(&str, &[u8])> = images.iter().map(|image| (image.file.as_str(), image.data.as_slice())).collect();
    owned(package_members(title, units, nav_entries, &images, identifier, modified, metadata, cover))
}

/// The package entries for a single document, `mimetype` first — the pure seam the macOS tests
/// drive. `body` is the already XHTML-fixed body (`xhtml_body(body_html(document))`), the same form
/// a book article is wrapped in.
pub fn document_entries(
    title: &str,
    body: &str,
    images: &[EpubImage],
    outline: &[OutlineEntry],
    modified: &str,
    identifier: Option<&str>,
    metadata: Option<&EpubMetadata>,
    cover: Option<&EpubCover>,
) -> Vec<ZipEntry> {
    let unit = EpubUnit::new(CONTENT_FILE_NAME, title, xhtml_document(title, body));
    let identifier = identifier.map_or_else(|| stable_identifier(title), str::to_string);
    package_entries(
        title,
        std::slice::from_ref(&unit),
        &document_nav_entries(title, outline),
        images,
        &identifier,
        modified,
        metadata,
        cover,
    )
}

/// The units of a plan with their photographs in place, and the photographs' archive names with
/// the app's bytes (borrowed, so a book of diagrams is copied once, into the archive).
struct Finished<'a> {
    units: Vec<EpubUnit>,
    images: Vec<(String, &'a [u8])>,
}

fn finish<'a>(
    plan: &EpubPlan,
    snapshots_for: &dyn Fn(usize) -> Option<&'a [RichSnapshot]>,
) -> Result<Finished<'a>, EpubSnapshotCountError> {
    let mut units = Vec::with_capacity(plan.units.len());
    let mut images = Vec::new();
    for unit in &plan.units {
        let taken = snapshots_for(unit.index).unwrap_or(&[]);
        if taken.len() != unit.rich_elements.len() {
            return Err(EpubSnapshotCountError {
                unit_index: unit.index,
                unit_title: unit.title.clone(),
                expected: unit.rich_elements.len(),
                actual: taken.len(),
            });
        }
        let mut body = Cow::Borrowed(unit.body.as_str());
        if !unit.rich_elements.is_empty() {
            let mut tags = Vec::with_capacity(taken.len());
            for (element, (snapshot, rich)) in taken.iter().zip(&unit.rich_elements).enumerate() {
                let file = image_file_name(unit.index, element);
                tags.push(image_tag(&file, rich.is_math, snapshot.display_width()));
                images.push((file, snapshot.png.as_slice()));
            }
            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            body = Cow::Owned(replacing_rich_elements(&body, &tags));
        }
        // A heading page's body is already the finished markup; an article's still holds the
        // md-init script the fixer strips (and the newline before it, which stays).
        let xhtml = if unit.is_heading_page {
            xhtml_document(&unit.title, &body)
        } else {
            xhtml_document(&unit.title, &xhtml_body(&body))
        };
        units.push(EpubUnit::new(unit.file.clone(), unit.title.clone(), xhtml));
    }
    Ok(Finished { units, images })
}

/// The container's members in archive order, payloads borrowed where they already exist.
fn package_members<'a, S: AsRef<str>>(
    title: &str,
    units: &'a [EpubUnit],
    nav_entries: &[NavEntry],
    images: &'a [(S, &'a [u8])],
    identifier: &str,
    modified: &str,
    metadata: Option<&EpubMetadata>,
    cover: Option<&'a EpubCover>,
) -> Vec<(String, Cow<'a, [u8]>)> {
    // Whichever units still hold an `<svg>` of their own — today that means a ```plot fence,
    // since every engine has become a PNG by now — carry properties="svg".
    let svg_units: Vec<usize> =
        units.iter().enumerate().filter(|(_, unit)| unit.xhtml.contains("<svg")).map(|(index, _)| index).collect();
    let unit_files: Vec<&str> = units.iter().map(|unit| unit.file.as_str()).collect();
    let image_files: Vec<&str> = images.iter().map(|(file, _)| file.as_ref()).collect();
    let opf = opf(title, identifier, modified, &unit_files, &image_files, &svg_units, metadata, cover);

    let mut members: Vec<(String, Cow<'a, [u8]>)> = Vec::with_capacity(6 + units.len() + images.len());
    members.push(("mimetype".to_string(), Cow::Borrowed(MIMETYPE.as_bytes())));
    members.push(("META-INF/container.xml".to_string(), Cow::Borrowed(CONTAINER_XML.as_bytes())));
    members.push(("OEBPS/content.opf".to_string(), Cow::Owned(opf.into_bytes())));
    members.push(("OEBPS/nav.xhtml".to_string(), Cow::Owned(nav(title, nav_entries).into_bytes())));
    members.push(("OEBPS/style.css".to_string(), Cow::Owned(stylesheet().into_bytes())));
    for unit in units {
        members.push((format!("OEBPS/{}", unit.file), Cow::Borrowed(unit.xhtml.as_bytes())));
    }
    for (file, data) in images {
        members.push((format!("OEBPS/{}", file.as_ref()), Cow::Borrowed(*data)));
    }
    if let Some(cover) = cover {
        members.push((format!("OEBPS/{}", cover.href()), Cow::Borrowed(cover.data.as_slice())));
    }
    members
}

fn owned(members: Vec<(String, Cow<'_, [u8]>)>) -> Vec<ZipEntry> {
    members.into_iter().map(|(name, data)| ZipEntry::new(name, data.into_owned())).collect()
}

// MARK: - Entry points

/// Build the EPUB 3 of one open document. `snapshots` is one PNG per rich element in document
/// order — [`plan_document`] says how many, and how to find them. `None` and an empty slice are
/// the same: no photographs.
pub fn build_document(
    source: &str,
    title: &str,
    snapshots: Option<&[RichSnapshot]>,
    modified: Option<&str>,
    identifier: Option<&str>,
    cover: Option<&EpubCover>,
) -> Result<Vec<u8>, EpubError> {
    let snapshots = snapshots.filter(|list| !list.is_empty()).map(|list| HashMap::from([(0, list.to_vec())]));
    assemble(&plan_document(source, title), snapshots.as_ref(), modified, identifier, cover)
}

/// Build the EPUB 3 of a whole book. `snapshots` maps a unit index (as [`plan_book`] assigned it)
/// to that unit's PNGs in document order.
pub fn build(
    book: &StructuredBook,
    snapshots: Option<&HashMap<usize, Vec<RichSnapshot>>>,
    modified: Option<&str>,
    identifier: Option<&str>,
) -> Result<Vec<u8>, EpubError> {
    assemble(&plan_book(book), snapshots, modified, identifier, None)
}
