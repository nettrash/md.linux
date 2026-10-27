//! Serializes the parsed block model to LaTeX source — the `.tex` export. Port of md.win
//! `Md.Core.Export.LaTeXExport`, itself the port of `md.macOS/md/LaTeXExport.swift`,
//! byte-identical in output with the Kotlin and TypeScript editions.
//!
//! Every other output turns a formula into a picture or into KaTeX markup. This one hands the
//! mathematics back as the `$…$` the author wrote, which is why a math span — alone among
//! everything here — is copied through untouched while every other scrap of author text is
//! escaped. Where LaTeX cannot do what the preview does, it says so instead of dropping: a diagram
//! fence keeps its source in `verbatim` under a comment naming the language, an image it cannot
//! read is named in a comment below its alt text, and a footnote definition nothing references is
//! still printed at the end.
//!
//! md.win's regexes are hand-written matchers here ([`patterns`]). Offsets are UTF-8 bytes; every
//! delimiter is ASCII, so a byte walk is as exact as md.win's code-unit walk, and the two word
//! guards are asked of whole scalars, as md.win decodes surrogate pairs by hand to ask them.

mod patterns;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::RangeInclusive;

use crate::book::structured_book::{BookUnit, StructuredBook};
use crate::markdown::{ColumnAlignment, DelimitedTable, ListItem, MarkdownBlock, parser};
use crate::text::scalar_text;

use patterns::{Found, Guard, Matcher};

/// U+2014, the dash in the three comments this file writes. Spelled as an escape so no editor can
/// quietly swap it for a hyphen or an en dash.
const EM_DASH: &str = "\u{2014}";

// MARK: - Entry points

/// One document as a standalone `article` .tex file. No title block unless the front matter gives
/// one: an invented `\title` would be a line the author never wrote.
///
/// `_title` is the file's name and is **not** written into the document — a file name is not part
/// of a document, and the Swift never invents a `\title` from one. It exists so the call site that
/// also names the `.tex` can pass what it has (core-api.md Part B; md.win's one-argument overload
/// is this with the title ignored).
pub fn document(source: &str, _title: &str) -> String {
    let blocks = parser::parse(&without_sentinels(source));
    let mut writer = Writer::new(0);
    let body = writer.render_unit(&blocks);
    assemble("article", &writer, &title_block(&fields(&blocks)), &body)
}

/// A whole book as a `book` .tex file: each chapter a `\chapter`, each article a `\section`, root
/// articles first — the reading order the PDF compile and the EPUB use. An article's own headings
/// drop one level so they nest under the article's `\section`.
pub fn book(book: &StructuredBook) -> String {
    fn append(writer: &mut Writer, parts: &mut Vec<String>, article: &BookUnit) {
        parts.push(format!("\\section{{{}}}", escape(&without_sentinels(&article.title))));
        let body = writer.render_unit(&parser::parse(&without_sentinels(&article.source)));
        if !body.is_empty() {
            parts.push(body);
        }
    }

    let mut writer = Writer::new(1);
    let mut parts = Vec::new();
    for article in &book.front_units {
        append(&mut writer, &mut parts, article);
    }
    for chapter in &book.sections {
        // `book` resets the footnote counter at every chapter, so the numbers `\footnotemark`
        // cites have to restart with it.
        writer.start_chapter();
        parts.push(format!("\\chapter{{{}}}", escape(&without_sentinels(&chapter.title))));
        for article in &chapter.units {
            append(&mut writer, &mut parts, article);
        }
    }

    // A book has a title page in every other export, so it keeps one here. `\date{}` rather than
    // no date: `\maketitle` would otherwise stamp today, which nobody wrote.
    let title = vec![format!("\\title{{{}}}", escape(&without_sentinels(&book.title))), "\\date{}".to_string()];
    assemble("book", &writer, &title, &parts.join("\n\n"))
}

// MARK: - Preamble

/// Wrap a rendered body in its class, packages and title block. Only the packages the document
/// used are loaded, and each flag was set by the renderer as it emitted the command — never by
/// scanning the finished text: a code block quoting `\href` is not a link.
fn assemble(document_class: &str, writer: &Writer, title_block: &[String], body: &str) -> String {
    let mut lines = vec![format!("\\documentclass{{{document_class}}}"), "\\usepackage[utf8]{inputenc}".to_string()];

    // T2A carries the Cyrillic glyphs and has to be the *last* encoding listed: fontenc makes the
    // last one the default, and T2A holds Latin as well, so English is untouched while T1 alone
    // would leave Russian to fail character by character.
    if contains_cyrillic(body) || title_block.iter().any(|line| contains_cyrillic(line)) {
        lines.push("\\usepackage[T1,T2A]{fontenc}".to_string());
    }
    // Any formula loads amsmath: guessing which constructs the author reached for stops the whole
    // file at the first one that was not guessed.
    if writer.needs_amsmath {
        lines.push("\\usepackage{amsmath}".to_string());
    }
    if writer.needs_graphicx {
        lines.push("\\usepackage{graphicx}".to_string());
    }
    // `normalem` is not optional: plain ulem redefines `\emph` to underline.
    if writer.needs_ulem {
        lines.push("\\usepackage[normalem]{ulem}".to_string());
    }
    if writer.needs_longtable {
        lines.push("\\usepackage{longtable}".to_string());
    }
    // hyperref last — the one package with a documented loading order.
    if writer.needs_hyperref {
        lines.push("\\usepackage{hyperref}".to_string());
    }

    lines.extend(title_block.iter().cloned());
    lines.push("\\begin{document}".to_string());
    if !title_block.is_empty() {
        lines.push("\\maketitle".to_string());
    }
    lines.push(String::new());
    lines.push(body.to_string());
    lines.push(String::new());
    lines.push("\\end{document}".to_string());
    lines.join("\n") + "\n"
}

/// Front-matter fields, first occurrence winning — the rule the rest of the app applies to a
/// repeated key.
fn fields(blocks: &[MarkdownBlock]) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    for block in blocks {
        let MarkdownBlock::FrontMatter { fields: matter } = block else { continue };
        for field in matter {
            // Swift `lowercased()`: full, locale-free mapping — a Turkish-culture lowercase would
            // turn `TITLE` into `tıtle` and the title block would vanish.
            let key = scalar_text::full_lowercase(&field.key);
            fields.entry(key).or_insert_with(|| field.value.clone());
        }
    }
    fields
}

/// `\title` / `\author` / `\date` from front matter. Every other field is metadata *about* the
/// document and is dropped the way the HTML and PDF drop it.
fn title_block(fields: &HashMap<String, String>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(title) = fields.get("title") {
        lines.push(format!("\\title{{{}}}", escape(title)));
    }
    if let Some(author) = fields.get("author") {
        lines.push(format!("\\author{{{}}}", escape(author)));
    }
    if let Some(date) = fields.get("date") {
        lines.push(format!("\\date{{{}}}", escape(date)));
    } else if !lines.is_empty() {
        // `\maketitle` prints today's date when none is given — a date nobody wrote.
        lines.push("\\date{}".to_string());
    }
    lines
}

// MARK: - Sentinels

const SENTINEL_OPEN: char = '\u{E000}';
const SENTINEL_CLOSE: char = '\u{E001}';

/// The author's text with this writer's own token sentinels (U+E000, U+E001) taken out — applied
/// to every string that enters the writer, and nowhere later: the working string of an inline pass
/// is *made* of these scalars. A document holding them of its own had its own characters read back
/// as a token index; they are private-use scalars inputenc has no definition for, so dropping them
/// costs nothing typesettable. A string with nothing to strip comes back borrowed, as md.win hands
/// back the same instance.
pub fn without_sentinels(text: &str) -> Cow<'_, str> {
    if !text.chars().any(is_sentinel) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.chars().filter(|&c| !is_sentinel(c)).collect())
}

fn is_sentinel(c: char) -> bool {
    c == SENTINEL_OPEN || c == SENTINEL_CLOSE
}

// MARK: - Escaping

/// Escape LaTeX's ten special characters in one pass. One pass rather than a chain of
/// replacements because three escapes (`\`, `~`, `^`) contain characters that are themselves
/// special. `<`, `>`, `|` are deliberately left alone — not TeX specials, and the ports agree
/// character for character on this table.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    // A scalar walk is exact: every case is ASCII, so a combining mark never hides the special
    // before it.
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            '#' | '$' | '%' | '&' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Escape a URL or image path for `\href` / `\includegraphics`: far less than [`escape`], because
/// hyperref reads its argument with its own catcodes and `_ ~ &` must arrive intact. Only what
/// breaks the *argument* is escaped.
pub fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '%' | '#' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Percent-decode an image path, or hand back exactly what came in. `my%20dir` is a directory on
/// no disk; anything that is not valid percent-encoding — a stray `%`, a short escape, bytes that
/// are not UTF-8 — is a file name that happens to contain a `%`, returned untouched (never
/// repaired to U+FFFD) for [`unreadable_image`] to refuse.
///
/// md.win encodes the path a *scalar* at a time so a surrogate pair is one 4-byte sequence, and
/// hands back a path holding an unpaired surrogate; a `str` is UTF-8 already and cannot hold one.
/// Its length check counts UTF-16 units where this counts bytes: the two disagree only when what
/// follows the `%` is not ASCII, which no hex digit is, so the path comes back either way.
pub fn percent_decoded(path: &str) -> String {
    if !scalar_text::contains(path, "%") {
        return path.to_string();
    }
    let b = path.as_bytes();
    let mut bytes = Vec::with_capacity(b.len());
    let mut index = 0;
    while index < b.len() {
        if b[index] != b'%' {
            bytes.push(b[index]);
            index += 1;
            continue;
        }
        if index + 2 >= b.len() {
            return path.to_string();
        }
        let (Some(high), Some(low)) = (hex_digit(b[index + 1]), hex_digit(b[index + 2])) else {
            return path.to_string();
        };
        bytes.push((high << 4) | low);
        index += 3;
    }
    // Strict: a UTF-8-encoded surrogate or an overlong sequence is refused whole.
    String::from_utf8(bytes).unwrap_or_else(|_| path.to_string())
}

fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// True when `text` holds a Cyrillic letter — the trigger for T2A. Without it pdfTeX does not
/// stop: the document compiles and the Russian is simply not in it. A range scan, not
/// `\p{IsCyrillic}`: regex engines disagree.
pub fn contains_cyrillic(text: &str) -> bool {
    text.chars().any(|c| CYRILLIC_BLOCKS.iter().any(|block| block.contains(&(c as u32))))
}

/// Cyrillic, Cyrillic Supplement, Cyrillic Extended-A and Cyrillic Extended-B.
const CYRILLIC_BLOCKS: [RangeInclusive<u32>; 4] = [0x0400..=0x04FF, 0x0500..=0x052F, 0x2DE0..=0x2DFF, 0xA640..=0xA69F];

// MARK: - Small structural helpers

/// LaTeX reads a `[` right after `\item` or `\\` as an optional argument, so a body that begins
/// with one would lose its first words — a task checkbox is literally `[x]`. An empty group in
/// front stops the scan. Applied to *finished* LaTeX only, never to a string still holding tokens.
fn bracket_guard(body: &str) -> Cow<'_, str> {
    if body.starts_with('[') { Cow::Owned(format!("{{}}{body}")) } else { Cow::Borrowed(body) }
}

/// A header cell, set bold. `\textbf` reads its argument with a delimited macro that a top-level
/// `&` ends the row out from under; an extra group makes TeX step over it whole. Written only
/// where there is a live alignment tab to guard.
fn header_cell(cell: &str) -> String {
    if holds_alignment_tab(cell) { format!("\\textbf{{{{{cell}}}}}") } else { format!("\\textbf{{{cell}}}") }
}

/// True when finished LaTeX holds an `&` that is an alignment tab: a backslash swallows the
/// character after it (`\&`, and the second half of `\\`). A byte walk: a skip that lands inside a
/// multi-byte character lands on a continuation byte, which is neither `\` nor `&`.
fn holds_alignment_tab(latex: &str) -> bool {
    let b = latex.as_bytes();
    let mut index = 0;
    while index < b.len() {
        if b[index] == b'\\' {
            index += 2;
        } else if b[index] == b'&' {
            return true;
        } else {
            index += 1;
        }
    }
    false
}

/// A formula given an `aligned` of its own when its `\\` and `&` have nowhere else to live —
/// `\[a &= b\]` is "Misplaced alignment tab" and a top-level `\\` in a table cell ends the row
/// from inside math mode. A formula that opens an environment of its own already owns its
/// separators and is left as written.
pub fn aligned(math: &str) -> String {
    if scalar_text::contains(math, "\\begin{") {
        return math.to_string();
    }
    if !scalar_text::contains(math, "\\\\") && !scalar_text::contains(math, "&") {
        return math.to_string();
    }
    format!("\\begin{{aligned}}{math}\\end{{aligned}}")
}

const BEGIN_FIGURE: &str = "\\begin{figure}";
const END_FIGURE: &str = "\\end{figure}";

/// True when `line` sets something on the page — anything that is not white space, a comment or a
/// float. `\\` after a line that began nothing is "There's no line here to end", so the soft-break
/// join asks this first. By hand, not regex: this runs on finished LaTeX, where an escaped `\%`
/// must not read as a comment — hence no backslash case, so a `\` counts as content.
pub fn sets_something(line: &str) -> bool {
    let b = line.as_bytes();
    let mut index = 0;
    while index < b.len() {
        let c = b[index];
        if c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' {
            index += 1;
        } else if c == b'%' {
            // A comment runs to the end of its physical line.
            index += 1;
            while index < b.len() && b[index] != b'\n' {
                index += 1;
            }
        } else if b[index..].starts_with(BEGIN_FIGURE.as_bytes()) {
            match scalar_text::first_index(line, END_FIGURE, index + BEGIN_FIGURE.len()) {
                None => return false,
                Some(end) => index = end + END_FIGURE.len(),
            }
        } else {
            return true;
        }
    }
    false
}

/// Why `\includegraphics` cannot be handed this path, or `None` when it can. graphicx reads its
/// argument as a *file name*: `% # { } \` have no spelling that works there, `"` breaks graphicx's
/// own space-quoting parser, and a URL is a file TeX cannot fetch.
pub fn unreadable_image(file: &str) -> Option<&'static str> {
    if file.chars().any(|c| matches!(c, '%' | '#' | '{' | '}' | '\\' | '"')) {
        return Some("is not a file name LaTeX can read");
    }
    if uri_scheme(file).is_some() {
        return Some("is a URL, and LaTeX has nothing to fetch it with");
    }
    None
}

/// The URI scheme of `path` when it is a URL, `None` otherwise: a scheme followed by `//`, or the
/// schemeless `data:` form. Deliberately not "letters then a colon" — `C:` is a drive and
/// `notes:draft.png` a file somebody can really have.
fn uri_scheme(path: &str) -> Option<&str> {
    let b = path.as_bytes();
    let mut index = 0;
    while index < b.len() {
        let c = b[index];
        if c == b':' {
            break;
        }
        let letter = c.is_ascii_alphabetic();
        let rest = index > 0 && (c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.');
        if !letter && !rest {
            return None;
        }
        index += 1;
    }
    if index == 0 || index >= b.len() {
        return None;
    }
    let scheme = &path[..index];
    let authority = b.get(index + 1) == Some(&b'/') && b.get(index + 2) == Some(&b'/');
    if !authority && scalar_text::full_lowercase(scheme) != "data" {
        return None;
    }
    Some(scheme)
}

// MARK: - Context

/// Where a run of inline text is being written. `Restricted` is a table cell, a footnote's own
/// text or an image caption — anywhere a float or a display environment stops the whole file.
/// Only ever added, never lifted: a footnote raised from a table cell is inside the float too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    Body,
    Restricted,
}

// MARK: - Writer

/// The spans one inline pass has lifted out: for each token, the LaTeX it stands for and the
/// Markdown it was made from (the latter for alt text alone).
#[derive(Default)]
struct Spans {
    latex: Vec<String>,
    source: Vec<String>,
}

impl Spans {
    fn count(&self) -> usize {
        self.latex.len()
    }

    fn append(&mut self, latex: String, source: &str) {
        self.latex.push(latex);
        self.source.push(source.to_string());
    }
}

/// One match as the passes need it: the groups (0 the whole match), and the author's text on
/// either side of group 1 (the delimiters `wrap` has to be able to give back).
struct MatchParts<'a> {
    groups: Vec<&'a str>,
    before: &'a str,
    after: &'a str,
}

/// The five sectioning commands `article` and `book` share; six heading levels map onto them, so
/// the deepest two both land on `\subparagraph`.
const SECTIONS: [&str; 5] = ["section", "subsection", "subsubsection", "paragraph", "subparagraph"];

/// The ten keys of the HTML renderer's `GRAPHVIZ_ENGINES` table — the fences that name a Graphviz
/// layout. Kept here (not read from the HTML writer) so this module has no dependency on it; the
/// list must stay identical to `markdown::html::GRAPHVIZ_ENGINES` (a unit test below holds it so).
const GRAPHVIZ_ENGINES: [&str; 10] =
    ["dot", "graphviz", "gv", "neato", "circo", "fdp", "sfdp", "twopi", "osage", "patchwork"];

const VERBATIM_END: &str = "\\end{verbatim}";

/// Renders blocks while accumulating what the preamble owes them (package flags) and where the
/// footnotes are up to. A struct because the footnote numbering is a running count that has to
/// survive nested calls.
struct Writer {
    /// How far to push the author's headings down, so a book's `\chapter`/`\section` sit above
    /// them.
    heading_offset: usize,

    needs_amsmath: bool,
    needs_graphicx: bool,
    needs_hyperref: bool,
    needs_longtable: bool,
    needs_ulem: bool,

    // The current unit's footnote definitions, the order written (to print the uncited), the ids
    // cited, and id -> the number LaTeX will give it. Ours and LaTeX's counters agree because
    // every `\footnote` and `\footnotemark` is emitted in the order it is numbered.
    definitions: HashMap<String, String>,
    written: Vec<String>,
    cited: HashSet<String>,
    number: HashMap<String, usize>,
    counter: usize,
    // Inside a footnote's text (LaTeX cannot nest them), inside a moving argument (a title, a
    // caption), inside a table's header row (the one box whose insertions LaTeX discards).
    in_footnote: bool,
    moving_argument: bool,
    in_table_head: bool,
    /// The `\footnotetext` lines the header row being written owes, emitted after its
    /// `\end{longtable}`.
    header_notes: Vec<String>,
    context: Context,
    spans: Spans,
}

impl Writer {
    fn new(heading_offset: usize) -> Self {
        Writer {
            heading_offset,
            needs_amsmath: false,
            needs_graphicx: false,
            needs_hyperref: false,
            needs_longtable: false,
            needs_ulem: false,
            definitions: HashMap::new(),
            written: Vec::new(),
            cited: HashSet::new(),
            number: HashMap::new(),
            counter: 0,
            in_footnote: false,
            moving_argument: false,
            in_table_head: false,
            header_notes: Vec::new(),
            context: Context::Body,
            spans: Spans::default(),
        }
    }

    /// Start a chapter: `book` resets the footnote counter, so the numbers `\footnotemark` cites
    /// restart — and the id map goes with them.
    fn start_chapter(&mut self) {
        self.counter = 0;
        self.number = HashMap::new();
    }

    // MARK: Units

    /// One whole document or book article: its blocks, then any footnote definition the text never
    /// referenced. All four stores reset, `number` included — each article numbers its own notes
    /// from `[^1]`, and carrying the previous article's numbers over would make that a
    /// `\footnotemark` citing the wrong note. `counter` is not reset: a chapter is one counter
    /// however many articles it holds.
    fn render_unit(&mut self, blocks: &[MarkdownBlock]) -> String {
        self.definitions = HashMap::new();
        self.written = Vec::new();
        self.cited = HashSet::new();
        self.number = HashMap::new();
        for block in blocks {
            if let MarkdownBlock::FootnoteDefinition { id, text } = block
                && !self.definitions.contains_key(id)
            {
                self.definitions.insert(id.clone(), text.clone());
                self.written.push(id.clone());
            }
        }

        let mut output = self.render_blocks(blocks);
        // A definition nothing points at is still something the author wrote: printed, with no
        // reference of its own since it has nowhere to point back to.
        let orphans: Vec<String> = self.written.iter().filter(|id| !self.cited.contains(*id)).cloned().collect();
        if orphans.is_empty() {
            return output;
        }
        let mut trailer = vec![format!("% Footnotes defined but never referenced {EM_DASH} kept so nothing is lost.")];
        for id in &orphans {
            self.counter += 1;
            let text = self.definitions[id].clone();
            trailer.push(format!("\\footnote{{{}}}", self.footnote_text(&text)));
        }
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output + &trailer.join("\n")
    }

    /// Blocks, blank-line separated; blocks that render to nothing drop out rather than leaving
    /// holes.
    fn render_blocks(&mut self, blocks: &[MarkdownBlock]) -> String {
        let mut rendered = Vec::with_capacity(blocks.len());
        for block in blocks {
            let latex = self.render_block(block);
            if !latex.is_empty() {
                rendered.push(latex);
            }
        }
        rendered.join("\n\n")
    }

    // MARK: Blocks

    fn section_command(&self, level: usize) -> &'static str {
        let index = (level + self.heading_offset).saturating_sub(1).min(SECTIONS.len() - 1);
        SECTIONS[index]
    }

    fn render_block(&mut self, block: &MarkdownBlock) -> String {
        match block {
            MarkdownBlock::Heading { level, text } => {
                format!("\\{}{{{}}}", self.section_command(*level), self.heading_inline(text))
            }
            // Soft line breaks are breaks in the preview, the HTML and the PDF; they stay breaks
            // here rather than reflowing the way raw LaTeX would.
            MarkdownBlock::Paragraph { text } => self.inline(text, true, Context::Body),
            MarkdownBlock::List { ordered, items } => self.render_list(items, *ordered),
            MarkdownBlock::CodeBlock { language, code } => self.render_code(language.as_deref(), code),
            MarkdownBlock::Quote { blocks } => {
                format!("\\begin{{quote}}\n{}\n\\end{{quote}}", self.render_blocks(blocks))
            }
            MarkdownBlock::Table { header, alignments, rows } => self.render_table(header, alignments, rows),
            MarkdownBlock::ThematicBreak => "\\par\\noindent\\hrulefill\\par".to_string(),
            MarkdownBlock::PageBreak => "\\newpage".to_string(),
            // Private notes never reach a rendered document; front matter is hoisted into the
            // title block; a definition is inlined at first reference or printed by `render_unit`
            // if nothing references it.
            MarkdownBlock::Note { .. }
            | MarkdownBlock::FrontMatter { .. }
            | MarkdownBlock::FootnoteDefinition { .. } => String::new(),
        }
    }

    /// A section title is a moving argument, so a footnote inside it needs `\protect`.
    fn heading_inline(&mut self, text: &str) -> String {
        let saved = self.moving_argument;
        self.moving_argument = true;
        let latex = self.inline(text, false, Context::Body);
        self.moving_argument = saved;
        latex
    }

    // MARK: Lists

    /// `itemize` / `enumerate`, nested by level. The model is flat, so levels are mapped onto a
    /// stack of open environments: LaTeX errors on a `\begin{itemize}` not preceded by an `\item`
    /// (a list whose first item is already indented) and refuses to nest more than four deep.
    /// Opening at most one level per item, and stopping at four, keeps every item in the output.
    fn render_list(&mut self, items: &[ListItem], ordered: bool) -> String {
        let environment = if ordered { "enumerate" } else { "itemize" };
        let mut lines = Vec::new();
        let mut open: Vec<usize> = Vec::new();

        for item in items {
            while open.last().is_some_and(|&last| item.level < last) {
                lines.push(format!("\\end{{{environment}}}"));
                open.pop();
            }
            if open.last().is_none_or(|&last| item.level > last && open.len() < 4) {
                lines.push(format!("\\begin{{{environment}}}"));
                open.push(item.level);
            }
            let mut body = self.inline(&item.text, false, Context::Body);
            if let Some(done) = item.task {
                // A literal checkbox rather than a package for one glyph.
                body = format!("{} {body}", if done { "[x]" } else { "[\\,]" });
            }
            lines.push(format!("\\item {}", bracket_guard(&body)));
        }
        for _ in &open {
            lines.push(format!("\\end{{{environment}}}"));
        }
        lines.join("\n")
    }

    // MARK: Code, diagrams and data

    /// A fenced block. The info string decides what it is, using the names the HTML renderer
    /// answers to, so the two agree about every fence.
    fn render_code(&mut self, language: Option<&str>, code: &str) -> String {
        let name = language.unwrap_or("");
        let lowered = scalar_text::full_lowercase(name);
        match lowered.as_str() {
            "mermaid" | "plantuml" | "puml" | "plant-uml" => diagram(name, code),
            // The one rich fence this app can draw without an engine — and it still travels as
            // its source: `\includegraphics` reads no SVG without a conversion step a `.tex`
            // cannot carry.
            "plot" => diagram(name, code),
            "csv" | "tsv" => {
                // Already a table everywhere else in the app; the parse and the alignment rule are
                // shared with the HTML renderer.
                match DelimitedTable::from(code, if lowered == "tsv" { '\t' } else { ',' }) {
                    None => verbatim(code),
                    Some(table) => self.render_table(&table.header, &table.alignments, &table.rows),
                }
            }
            // The author's mathematics, untouched — the point of the whole export.
            "math" | "latex" | "tex" => self.display(&format!("\n{code}\n")),
            _ if GRAPHVIZ_ENGINES.contains(&lowered.as_str()) => diagram(name, code),
            _ => verbatim(code),
        }
    }

    // MARK: Tables

    /// A `longtable`, never a `tabular` in a `table` float: a float cannot break across a page, so
    /// a tall table is truncated at exit 0. Not being a float also lets a body cell keep a plain
    /// `\footnote`. The header row is the exception — typeset once into the box `\endhead`
    /// reinserts, whose footnote insertions LaTeX discards — so a note first cited there is split
    /// (see `footnote_reference`) and its `\footnotetext` written after `\end{longtable}`. Width
    /// is the widest row, not the header's.
    fn render_table(&mut self, header: &[String], alignments: &[ColumnAlignment], rows: &[Vec<String>]) -> String {
        self.needs_longtable = true;
        let columns = rows.iter().map(Vec::len).fold(header.len(), usize::max);
        if columns == 0 {
            return String::new();
        }

        let spec: String = (0..columns)
            .map(|column| match alignments.get(column) {
                None | Some(ColumnAlignment::Leading) => 'l',
                Some(ColumnAlignment::Center) => 'c',
                Some(ColumnAlignment::Trailing) => 'r',
            })
            .collect();

        // The header row first — its notes take the first numbers — and under `in_table_head`, so
        // a note cited from it is split rather than dropped into the saved head box.
        let saved_head = self.in_table_head;
        let saved_notes = std::mem::take(&mut self.header_notes);
        self.in_table_head = true;
        let head = self.table_row(header, columns, true);
        let notes = std::mem::replace(&mut self.header_notes, saved_notes);
        self.in_table_head = saved_head;

        let mut lines =
            vec![format!("\\begin{{longtable}}{{{spec}}}"), "\\hline".to_string(), head, "\\hline".to_string()];
        lines.push("\\endhead".to_string());
        for row in rows {
            lines.push(self.table_row(row, columns, false));
        }
        lines.push("\\hline".to_string());
        lines.push("\\end{longtable}".to_string());
        lines.extend(notes);
        lines.join("\n")
    }

    fn table_row(&mut self, cells: &[String], columns: usize, heading: bool) -> String {
        let mut rendered = Vec::with_capacity(columns);
        for column in 0..columns {
            let cell = match cells.get(column) {
                Some(cell) => self.inline(cell, false, Context::Restricted),
                None => String::new(),
            };
            rendered.push(if heading && !cell.is_empty() { header_cell(&cell) } else { cell });
        }
        format!("{} \\\\", bracket_guard(&rendered.join(" & ")))
    }

    // MARK: Footnotes

    /// A reference to `id`, resolved the moment the reader meets it: the first reference carries
    /// the note's text (that is what a LaTeX footnote is), a later one cites the number, an
    /// undefined one goes back to the text the author typed.
    fn footnote_reference(&mut self, id: &str) -> String {
        let protection = if self.moving_argument { "\\protect" } else { "" };
        // LaTeX cannot nest footnotes, so a reference inside a note's own text stays literal.
        let text = match self.definitions.get(id) {
            Some(text) if !self.in_footnote => text.clone(),
            _ => return escape(&format!("[^{id}]")),
        };
        self.cited.insert(id.to_string());
        if let Some(&existing) = self.number.get(id) {
            return format!("{protection}\\footnotemark[{existing}]");
        }
        self.counter += 1;
        self.number.insert(id.to_string(), self.counter);
        // The header row: mark here, text after the table, and the counter stepped by hand
        // because `\footnotemark[n]` does not step it — a body cell further down takes the next
        // number and would otherwise print this one's.
        if self.in_table_head {
            let mark = self.counter;
            let note = self.footnote_text(&text);
            self.header_notes.push(format!("\\footnotetext[{mark}]{{{note}}}"));
            return format!("\\stepcounter{{footnote}}{protection}\\footnotemark[{mark}]");
        }
        format!("{protection}\\footnote{{{}}}", self.footnote_text(&text))
    }

    /// A footnote's own text: inline Markdown with nesting disabled, and restricted — a footnote
    /// can no more hold a float than a table cell can.
    fn footnote_text(&mut self, text: &str) -> String {
        let saved = self.in_footnote;
        self.in_footnote = true;
        let latex = self.inline(text, false, Context::Restricted);
        self.in_footnote = saved;
        latex
    }

    // MARK: Inline

    /// Convert a block's inline Markdown to LaTeX. The commands go in first — as tokens, never as
    /// text — so the escaping pass at the end sees author text and nothing else, and markup nested
    /// inside a link label or a bold run is still plain text when the later passes look for it.
    /// The patterns are the HTML renderer's, so the preview and the `.tex` agree about what a
    /// formula is. `context` is added to whatever is in force, never substituted for it.
    fn inline(&mut self, text: &str, soft_breaks: bool, context: Context) -> String {
        // A pass owns its tokens: an image starts a second, complete pass over the alt text in the
        // middle of this one, and the two stores must never meet.
        let saved_spans = std::mem::take(&mut self.spans);
        let saved_context = self.context;
        if context == Context::Restricted {
            self.context = Context::Restricted;
        }
        let latex = self.inline_pass(text, soft_breaks);
        self.spans = saved_spans;
        self.context = saved_context;
        latex
    }

    fn inline_pass(&mut self, text: &str, soft_breaks: bool) -> String {
        let mut working = text.to_string();

        // 1. Literal spans first: code wins over math, so `$x$` in backticks stays code; `$…$`
        //    keeps its currency guard so "$5 and $10" is prose. `\texttt` rather than `\verb`:
        //    this has to survive in a title, a caption and a cell.
        working = self.substitute(&working, patterns::code_span, None, |_, g| format!("\\texttt{{{}}}", escape(g[1])));
        working = self.substitute(&working, patterns::display_dollars, None, |w, g| w.display(g[1]));
        working = self.substitute(&working, patterns::display_brackets, None, |w, g| w.display(g[1]));
        working = self
            .substitute(&working, patterns::inline_dollar, Some(patterns::dollar_guard), |w, g| w.inline_math(g[1]));
        working = self.substitute(&working, patterns::inline_paren, None, |w, g| w.inline_math(g[1]));

        // 2. Images before links (image syntax is link syntax with a `!`), links before emphasis.
        //    A title is matched only so it is consumed.
        working = self.substitute(&working, patterns::image_titled, None, |w, g| w.render_image(g[1], g[2]));
        working = self.substitute(&working, patterns::image, None, |w, g| w.render_image(g[1], g[2]));
        working = self.wrap(&working, patterns::link_titled, None, |w, g| w.href(g[2]));
        working = self.wrap(&working, patterns::link, None, |w, g| w.href(g[2]));

        // 3. Footnote references after images and links: converting one first would let an image
        //    carry a whole `\footnote` into its caption.
        working = self.substitute(&working, patterns::footnote, None, |w, g| w.footnote_reference(g[1]));

        // 4. Emphasis: bold before italic so `**` wins, underscore italic only at word boundaries
        //    so snake_case survives.
        working = self.wrap(&working, patterns::bold, None, |_, _| "\\textbf{".to_string());
        working = self.wrap(&working, patterns::bold_underscore, None, |_, _| "\\textbf{".to_string());
        working = self.wrap(&working, patterns::strike, None, |w, _| {
            w.needs_ulem = true;
            "\\sout{".to_string()
        });
        working = self.wrap(&working, patterns::italic, None, |_, _| "\\emph{".to_string());
        working = self.wrap(&working, patterns::italic_underscore, Some(patterns::underscore_guard), |_, _| {
            "\\emph{".to_string()
        });

        // 5. Everything still in the string is the author's own text. Tokens are private-use
        //    scalars and digits, none special, so they pass through.
        working = escape(&working);

        // 6. Restore the spans, and with them the soft breaks. Lines are split while the spans are
        //    still tokens (a multi-line formula is one token), but the bracket guard runs on the
        //    *restored* line. A `\\` is written only between two lines that both set something —
        //    "There's no line here to end" otherwise.
        if !soft_breaks {
            return self.restored(&working);
        }
        let lines: Vec<String> =
            scalar_text::split(&working, "\n").into_iter().map(|line| self.restored(line)).collect();
        let mut output = lines[0].clone();
        let mut opened = sets_something(&lines[0]);
        for line in &lines[1..] {
            if opened && sets_something(line) {
                output.push_str("\\\\\n");
                output.push_str(&bracket_guard(line));
                opened = true;
            } else {
                // Never two newlines in a row: a blank line is a paragraph break, and one straight
                // after a `\\` is the very error this avoids.
                if !output.ends_with('\n') {
                    output.push('\n');
                }
                output.push_str(line);
                opened = opened || sets_something(line);
            }
        }
        output
    }

    /// An inline formula: legal everywhere, but its own `\\` and `&` still need an `aligned`, and
    /// the document owes amsmath.
    fn inline_math(&mut self, math: &str) -> String {
        self.needs_amsmath = true;
        format!("${}$", aligned(math))
    }

    /// A display formula — or an inline one where LaTeX will not open a display at all (`\[` in a
    /// table cell stops the file).
    fn display(&mut self, math: &str) -> String {
        self.needs_amsmath = true;
        let body = aligned(math);
        if self.context == Context::Restricted { format!("${body}$") } else { format!("\\[{body}\\]") }
    }

    fn href(&mut self, url: &str) -> String {
        self.needs_hyperref = true;
        format!("\\href{{{}}}{{", escape_url(url))
    }

    /// An image: in body text a captioned `figure`, or the bare graphic with a `\noindent` (a
    /// `\linewidth` graphic starting a paragraph is pushed over the margin by the indent). In a
    /// restricted place there is no float to be had, so the graphic is written bare and the alt
    /// text goes beside it in italic rather than being dropped.
    fn render_image(&mut self, alt: &str, path: &str) -> String {
        let file = percent_decoded(path);
        if let Some(reason) = unreadable_image(&file) {
            return self.skipped(&file, reason, alt);
        }
        self.needs_graphicx = true;
        let include = format!("\\includegraphics[width=\\linewidth]{{{}}}", escape_url(&file));
        if self.context == Context::Restricted {
            return if alt.is_empty() { include } else { format!("{include} \\emph{{{}}}", self.alt_text(alt)) };
        }
        if alt.is_empty() {
            return format!("\\noindent{include}");
        }
        let saved = self.moving_argument;
        self.moving_argument = true;
        let caption = self.alt_text(alt);
        self.moving_argument = saved;
        format!("\\begin{{figure}}[ht]\n\\centering\n{include}\n\\caption{{{caption}}}\n\\end{{figure}}")
    }

    /// An image LaTeX cannot include: the alt text in its place, and the file named in a comment
    /// that comes **last** and ends with a newline — `%` runs to the end of its physical line, so a
    /// comment in front would swallow the words, and one without a newline would swallow the rest
    /// of a table row.
    fn skipped(&mut self, file: &str, reason: &str, alt: &str) -> String {
        let note = format!("% md: image skipped {EM_DASH} {file} {reason}.\n");
        if alt.is_empty() {
            return note;
        }
        format!("\\emph{{{}}}\n{note}", self.alt_text(alt))
    }

    /// Alt text as LaTeX: a fresh, complete pass over the Markdown the author wrote. The captured
    /// `alt` carries this pass's tokens, and re-tokenising a half-tokenised string would leave them
    /// for the outer restore never to see.
    fn alt_text(&mut self, alt: &str) -> String {
        let source = self.raw_source(alt);
        self.inline(&source, false, Context::Restricted)
    }

    // MARK: Token plumbing

    /// Put the LaTeX back where the tokens are — newest span first, since a span's replacement
    /// can only hold a token from an older pass.
    fn restored(&self, text: &str) -> String {
        resolve(text, &self.spans.latex)
    }

    /// The author's own Markdown for a stretch of the working string.
    fn raw_source(&self, text: &str) -> String {
        resolve(text, &self.spans.source)
    }

    /// Replace every match with a token standing for `transform`'s LaTeX. The transform runs
    /// **before** the span is filed because it may start an inline pass of its own (a caption)
    /// which borrows the store and hands it back.
    fn substitute(
        &mut self,
        text: &str,
        pattern: Matcher,
        guard: Option<Guard>,
        mut transform: impl FnMut(&mut Self, &[&str]) -> String,
    ) -> String {
        self.splice(text, pattern, guard, |writer, parts| {
            let latex = transform(writer, &parts.groups);
            writer.spans.append(latex, parts.groups[0]);
            (token(writer.spans.count() - 1), None)
        })
    }

    /// Replace every match with an opening token, group 1's text, and a closing token (`}`) — the
    /// command's braces hidden from the escaper, the author's words still matchable.
    fn wrap(
        &mut self,
        text: &str,
        pattern: Matcher,
        guard: Option<Guard>,
        mut open: impl FnMut(&mut Self, &[&str]) -> String,
    ) -> String {
        self.splice(text, pattern, guard, |writer, parts| {
            let latex = open(writer, &parts.groups);
            let first = writer.spans.count();
            writer.spans.append(latex, parts.before);
            writer.spans.append("}".to_string(), parts.after);
            (token(first), Some(token(first + 1)))
        })
    }

    /// The shared rewrite: match forward, build each replacement in reading order (footnote
    /// numbering follows the order the reader meets the references), then reassemble. The Swift
    /// splices backward to keep its ranges valid; appending the untouched stretches and the
    /// replacements in order gives the identical string.
    fn splice(
        &mut self,
        text: &str,
        pattern: Matcher,
        guard: Option<Guard>,
        mut build: impl FnMut(&mut Self, &MatchParts<'_>) -> (String, Option<String>),
    ) -> String {
        let matches: Vec<Found> = patterns::guarded(text, pattern, guard);
        if matches.is_empty() {
            return text.to_string();
        }

        let mut replacements = Vec::with_capacity(matches.len());
        for found in &matches {
            let mut groups = vec![&text[found.whole.clone()]];
            groups.extend(found.groups.iter().map(|group| &text[group.clone()]));
            let (before, after) = match found.groups.first() {
                Some(inner) => (&text[found.whole.start..inner.start], &text[inner.end..found.whole.end]),
                None => ("", ""),
            };
            let parts = MatchParts { groups, before, after };
            let (open, close) = build(self, &parts);
            replacements.push(match close {
                None => open,
                Some(close) => format!("{open}{}{close}", parts.groups.get(1).copied().unwrap_or("")),
            });
        }

        let mut result = String::with_capacity(text.len());
        let mut cursor = 0;
        for (found, replacement) in matches.iter().zip(&replacements) {
            result.push_str(&text[cursor..found.whole.start]);
            result.push_str(replacement);
            cursor = found.whole.end;
        }
        result.push_str(&text[cursor..]);
        result
    }
}

/// A token: U+E000, the span's decimal index, U+E001.
fn token(index: usize) -> String {
    format!("{SENTINEL_OPEN}{index}{SENTINEL_CLOSE}")
}

/// Every token in `text` replaced by its span's text, newest span first. md.win runs one ordinal
/// replace per span over the whole string, newest first — quadratic in the spans of a paragraph
/// (a 4000-line one took seconds). This expands the tokens in one left-to-right walk instead, and
/// writes the same string:
///
/// - the loop's step `k` replaces every `token(k)` present, then every older step sees the text it
///   put in, while no newer step ever does — so a span's text is expanded for exactly the tokens
///   *older* than itself, which is the `limit` here; a token at or past it stays as written;
/// - a token is found where the loop's replace would find it: no token can straddle a span's text
///   and its surroundings, because every span's text starts and ends with ASCII that is neither a
///   digit nor a sentinel, and the working string holds only whole tokens (the author's own
///   sentinels were stripped on the way in). A percent-decoded image path is the one way a span's
///   text gains a sentinel the writer did not put there, and it sits inside `{…}`.
///
/// `tests::resolve_is_the_newest_first_replace_loop` holds the two to each other.
fn resolve(text: &str, replacements: &[String]) -> String {
    if !text.contains(SENTINEL_OPEN) {
        return text.to_string();
    }
    let mut output = String::with_capacity(text.len());
    expand(&mut output, text, replacements, replacements.len());
    output
}

fn expand(output: &mut String, text: &str, replacements: &[String], limit: usize) {
    let mut rest = text;
    while let Some(open) = rest.find(SENTINEL_OPEN) {
        output.push_str(&rest[..open]);
        let after = &rest[open + SENTINEL_OPEN.len_utf8()..];
        match token_index(after).filter(|&(index, _)| index < limit) {
            Some((index, length)) => {
                expand(output, &replacements[index], replacements, index);
                rest = &after[length..];
            }
            None => {
                output.push(SENTINEL_OPEN);
                rest = after;
            }
        }
    }
    output.push_str(rest);
}

/// When `text` — what follows a U+E000 — continues a token, its index and the length of the rest
/// of it: the decimal exactly as [`token`] writes it (no leading zero), then U+E001.
fn token_index(text: &str) -> Option<(usize, usize)> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || (digits > 1 && text.starts_with('0')) || !text[digits..].starts_with(SENTINEL_CLOSE) {
        return None;
    }
    let index = text[..digits].parse::<usize>().ok()?;
    Some((index, digits + SENTINEL_CLOSE.len_utf8()))
}

/// A diagram fence: LaTeX has no renderer for it, so the source is kept verbatim under a comment
/// naming the language (the author's own spelling).
fn diagram(language: &str, code: &str) -> String {
    format!(
        "% {language} diagram source {EM_DASH} LaTeX has no renderer for it, so it is kept as written.\n{}",
        verbatim(code)
    )
}

/// Wrap code in `verbatim`, unescaped. LaTeX's verbatim scans for the *characters*
/// `\end{verbatim}`, so a block quoting them would close the environment early and spill the rest
/// as LaTeX to execute; the block is split around each occurrence and the terminator itself set
/// with `\verb`.
fn verbatim(code: &str) -> String {
    let pieces = scalar_text::split(code, VERBATIM_END);
    if pieces.len() == 1 {
        return verbatim_block(pieces[0]);
    }
    let mut output = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        if index > 0 {
            output.push(format!("\\noindent\\verb|{VERBATIM_END}|\\par"));
        }
        if !piece.is_empty() {
            output.push(verbatim_block(piece));
        }
    }
    output.join("\n")
}

fn verbatim_block(code: &str) -> String {
    format!("\\begin{{verbatim}}\n{code}\n{VERBATIM_END}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphviz_fences_are_the_html_writers() {
        let html: Vec<&str> = crate::markdown::html::GRAPHVIZ_ENGINES.iter().map(|(key, _)| *key).collect();
        assert_eq!(html, GRAPHVIZ_ENGINES);
    }

    /// md.win's `Resolve`, verbatim: one ordinal replace per span, newest first.
    fn resolve_by_replacing(text: &str, replacements: &[String]) -> String {
        if !text.contains(SENTINEL_OPEN) {
            return text.to_string();
        }
        let mut output = text.to_string();
        for (index, replacement) in replacements.iter().enumerate().rev() {
            output = scalar_text::replacing(&output, &token(index), replacement);
        }
        output
    }

    /// xorshift64*, so the run is the same on every machine.
    struct Random(u64);

    impl Random {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize % bound
        }
    }

    /// Text the way a pass can produce it: author text (digits included), whole tokens of any
    /// index — older, newer, the span's own, past the end, with a leading zero — and, when
    /// `injected`, stray sentinels of the kind a percent-decoded image path brings in, kept
    /// between ASCII that is neither a digit nor a sentinel as the path's `{…}` keeps them.
    fn piece(random: &mut Random, spans: usize, injected: bool) -> String {
        let mut out = String::new();
        for _ in 0..random.below(6) {
            match random.below(if injected { 7 } else { 5 }) {
                0 => out.push_str(["a", "7", "x9", "\\", "é", "😀"][random.below(6)]),
                1 | 2 => out.push_str(&token(random.below(spans + 2))),
                3 => out.push_str(&format!("{SENTINEL_OPEN}0{}{SENTINEL_CLOSE}", random.below(3))),
                4 => out.push_str("{}"),
                _ => {
                    out.push('{');
                    for _ in 0..random.below(4) {
                        out.push(['\u{E000}', '\u{E001}', '0', '1', '2', 'p'][random.below(6)]);
                    }
                    out.push('}');
                }
            }
        }
        format!("<{out}>")
    }

    #[test]
    fn resolve_is_the_newest_first_replace_loop() {
        let mut random = Random(0x9E37_79B9_7F4A_7C15);
        for _ in 0..20_000 {
            let spans = random.below(14);
            let replacements: Vec<String> = (0..spans).map(|_| piece(&mut random, spans, true)).collect();
            let text = piece(&mut random, spans, false);
            assert_eq!(
                resolve_by_replacing(&text, &replacements),
                resolve(&text, &replacements),
                "{text:?} {replacements:?}"
            );
        }
        assert_eq!(resolve("a\u{E000}12\u{E001}b", &[]), "a\u{E000}12\u{E001}b");
    }
}
