//! The block-level Markdown parser: a pragmatic subset of CommonMark plus the GitHub extensions
//! (fences, task lists, tables, footnotes) and md's own additions (front matter, page breaks,
//! author notes). Line-oriented, single pass, cheap enough to re-run on every keystroke. Port of
//! md.win `Md.Core.Markdown.MarkdownParser`.
//!
//! This is upstream of everything — preview, HTML, PDF, EPUB, LaTeX, outline, notes — so a
//! one-line divergence here is "the same document is a different document on Linux". The branch
//! order in `scan` is normative, the three continuation break-sets are deliberately different, and
//! every documented wart is kept (see the tests): an unclosed fence at EOF gains a trailing
//! newline, `<!-- c --> text` drops the text, [`outline`] lists a phantom setext heading over a
//! table header, tables cannot interrupt paragraphs but can interrupt list items, heading
//! indentation is unbounded while a fence's is capped at three, and quote nesting stops at depth
//! 32 as part of the output shape.
//!
//! Character model: md.win walks UTF-16 code units and compares single ASCII chars; this walks
//! UTF-8 bytes and compares single ASCII bytes, which is the same decision — every delimiter is one
//! ASCII scalar, and no byte of a multi-byte sequence equals one. Every slice is taken at an offset
//! produced by an ASCII match or a char walk, so it is always on a char boundary. Only [`slug`] and
//! the DOT name scan walk code points. Every comparison is ordinal; trims use the sets in
//! [`crate::text::whitespace`]; digits are ASCII `'0'..='9'`; nothing normalises.

use std::collections::HashMap;

use super::block::{ColumnAlignment, ListItem, MarkdownBlock, MetadataField, NoteEntry, OutlineEntry, PlacedBlock};
use crate::text::scalar_text;
use crate::text::unicode;
use crate::text::whitespace::{
    drop_leading_spaces, drop_space_tab, newline_set_lines, normalized_lines, trim_ws, trim_wsnl,
};

/// Block quotes recurse once per marker; past this depth the remainder stays a paragraph. The cap
/// shapes the output (33 nested quotes for a run of 5000 markers) and must not move.
const QUOTE_DEPTH_LIMIT: usize = 32;

// MARK: - Entry points

/// Parse Markdown source into a flat list of blocks.
pub fn parse(source: &str) -> Vec<MarkdownBlock> {
    parse_at_depth(source, 0)
}

/// md.win's `Parse(source, quoteDepth)`: the quote depth is internal to the recursion.
fn parse_at_depth(source: &str, quote_depth: usize) -> Vec<MarkdownBlock> {
    scan(source, quote_depth).into_iter().map(|placed| placed.block).collect()
}

/// The same walk as [`parse`], recording the 0-based line each top-level block started on.
/// `parse(s)` equals `parse_with_lines(s)` mapped to its blocks by construction: both are
/// projections of one scanner.
pub fn parse_with_lines(source: &str) -> Vec<PlacedBlock> {
    scan(source, 0)
}

/// The document's front matter, or an empty list. Runs a full parse so the accessor cannot
/// disagree with the document.
pub fn front_matter(source: &str) -> Vec<MetadataField> {
    for block in parse(source) {
        if let MarkdownBlock::FrontMatter { fields } = block {
            return fields;
        }
    }
    Vec::new()
}

// MARK: - The scanner

fn placed(block: MarkdownBlock, line: usize) -> PlacedBlock {
    PlacedBlock { block, line }
}

fn is_blank(line: &str) -> bool {
    trim_ws(line).is_empty()
}

fn scan(source: &str, quote_depth: usize) -> Vec<PlacedBlock> {
    let lines = normalized_lines(source);
    let mut blocks = Vec::new();
    let mut i = 0;

    // Front matter: only at the very start of the document, never inside a quote. Without it the
    // opening `---` reads as a rule and the metadata as prose.
    if quote_depth == 0
        && let Some((fields, next)) = parse_front_matter(&lines)
    {
        blocks.push(placed(MarkdownBlock::FrontMatter { fields }, 0));
        i = next;
    }

    while i < lines.len() {
        let line = lines[i];
        // Captured before any branch moves the cursor: the paragraph branch reads it after its own
        // loop, and a setext heading takes the line of its text, not its underline.
        let start = i;

        // 1. Blank line — separator, nothing to emit. WS set: a lone U+200B is blank. This MUST be
        // the same predicate the continuation loops break on (`trim_ws(l).is_empty()`): with
        // `line.is_empty()` here, a whitespace-only line is refused by every branch, the paragraph
        // loop breaks without appending, `i` never advances, and the parser spins forever on
        // `"a\n   \nb"`.
        if is_blank(line) {
            i += 1;
            continue;
        }

        // 2. Fenced code. Wins over everything: inside a fence `---`, `# x`, `> x`, `- x` are code.
        if let Some(fence) = FenceMarker::from(line) {
            let mut code: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() {
                if fence.closes(lines[i]) {
                    i += 1;
                    break;
                }
                code.push(fence.strip_indent(lines[i]));
                i += 1;
            }
            // Unclosed → consumes to EOF, and because `normalized_lines` always appends a final
            // element, a source ending in a newline gives the code a trailing "\n". Wart, kept.
            blocks.push(placed(
                MarkdownBlock::CodeBlock { language: fence.language.map(str::to_string), code: code.join("\n") },
                start,
            ));
            continue;
        }

        // 3. Thematic break — before the list branch, so `- - -` and `* * *` are rules.
        if is_thematic_break(line) {
            blocks.push(placed(MarkdownBlock::ThematicBreak, start));
            i += 1;
            continue;
        }

        // 4. Page break: `\newpage` / `\pagebreak` alone on a line.
        if is_page_break(line) {
            blocks.push(placed(MarkdownBlock::PageBreak, start));
            i += 1;
            continue;
        }

        // 5. Footnote definition, with soft-wrapped continuation absorbed. Wins over heading,
        // table, quote and list. This loop carries the footnote check and NO table lookahead.
        if let Some((id, first)) = parse_footnote_definition(line) {
            let mut text = first.to_string();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if is_blank(l) {
                    break;
                }
                if parse_footnote_definition(l).is_some()
                    || FenceMarker::from(l).is_some()
                    || is_thematic_break(l)
                    || parse_heading(l).is_some()
                    || is_quote(l)
                    || is_page_break(l)
                    || is_comment_start(l)
                    || list_marker(l).is_some()
                {
                    break;
                }
                text.push(' ');
                text.push_str(trim_ws(l));
                i += 1;
            }
            blocks.push(placed(MarkdownBlock::FootnoteDefinition { id: id.to_string(), text }, start));
            continue;
        }

        // 6. HTML comment, possibly spanning lines. `<!-- note: … -->` becomes a note; any other
        // comment is dropped. The WHOLE closing line is consumed (text after `-->` is lost) and an
        // unclosed comment swallows to EOF — both kept.
        if is_comment_start(line) {
            let mut raw: Vec<&str> = Vec::new();
            while i < lines.len() {
                raw.push(lines[i]);
                let closed = scalar_text::contains(lines[i], "-->");
                i += 1;
                if closed {
                    break;
                }
            }
            if let Some(note) = note_text(&raw.join("\n")) {
                blocks.push(placed(MarkdownBlock::Note { text: note }, start));
            }
            continue;
        }

        // 7. ATX heading — before the table branch, so `# a | b` over `---|---` is a heading.
        if let Some((level, text)) = parse_heading(line) {
            blocks.push(placed(MarkdownBlock::Heading { level, text: text.to_string() }, start));
            i += 1;
            continue;
        }

        // 8. GFM table — the only branch with lookahead. Wins over quote and list.
        if i + 1 < lines.len()
            && let Some((header, alignments)) = parse_table(line, lines[i + 1])
        {
            let mut rows = Vec::new();
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !is_blank(lines[i]) {
                rows.push(split_table_row(lines[i], Some(header.len())));
                i += 1;
            }
            blocks.push(placed(MarkdownBlock::Table { header, alignments, rows }, start));
            continue;
        }

        // 9. Block quote: the run of `>` lines, one marker stripped, parsed recursively. No lazy
        // continuation. The depth cap keeps a line of thousands of `>` off the stack — and is part
        // of the output shape.
        if is_quote(line) {
            let mut inner: Vec<&str> = Vec::new();
            while i < lines.len() && is_quote(lines[i]) {
                inner.push(strip_quote_marker(lines[i]));
                i += 1;
            }
            let inner_text = inner.join("\n");
            let inner_blocks = if quote_depth < QUOTE_DEPTH_LIMIT {
                parse_at_depth(&inner_text, quote_depth + 1)
            } else {
                vec![MarkdownBlock::Paragraph { text: inner_text }]
            };
            blocks.push(placed(MarkdownBlock::Quote { blocks: inner_blocks }, start));
            continue;
        }

        // 10. List: consecutive items, each absorbing its continuation lines. This loop carries
        // the table lookahead and NO footnote check ("- item\n[^a]: note" is one item).
        if list_marker(line).is_some() {
            let mut items = Vec::new();
            let mut ordered = false;
            while i < lines.len() {
                let Some(marker) = list_marker(lines[i]) else { break };
                ordered = ordered || marker.ordinal.is_some();
                let mut text = marker.text.to_string();
                i += 1;
                while i < lines.len() {
                    let l = lines[i];
                    if is_blank(l) {
                        break;
                    }
                    if list_marker(l).is_some()
                        || FenceMarker::from(l).is_some()
                        || is_thematic_break(l)
                        || parse_heading(l).is_some()
                        || is_quote(l)
                        || is_page_break(l)
                        || is_comment_start(l)
                    {
                        break;
                    }
                    if i + 1 < lines.len() && parse_table(l, lines[i + 1]).is_some() {
                        break;
                    }
                    text.push(' ');
                    text.push_str(trim_ws(l));
                    i += 1;
                }
                items.push(ListItem { text, level: marker.level, ordinal: marker.ordinal, task: marker.task });
            }
            blocks.push(placed(MarkdownBlock::List { ordered, items }, start));
            continue;
        }

        // 11. Paragraph: raw lines until a blank or a block start. Setext is recognised only here,
        // only with exactly one buffered line, and before the break set — so `Title\n---` is one
        // heading and never heading + rule. No table lookahead: a table cannot interrupt a
        // paragraph. The first line is always appended, since every predicate in the break set was
        // already refused above.
        let mut paragraph: Vec<&str> = Vec::new();
        let mut emitted_heading = false;
        while i < lines.len() {
            let l = lines[i];
            if is_blank(l) {
                break;
            }
            if paragraph.len() == 1
                && let Some(level) = setext_underline(l)
            {
                blocks.push(placed(MarkdownBlock::Heading { level, text: trim_ws(paragraph[0]).to_string() }, start));
                i += 1;
                emitted_heading = true;
                break;
            }
            if FenceMarker::from(l).is_some()
                || is_thematic_break(l)
                || parse_heading(l).is_some()
                || is_quote(l)
                || list_marker(l).is_some()
                || is_page_break(l)
                || is_comment_start(l)
            {
                break;
            }
            paragraph.push(l);
            i += 1;
        }
        if !emitted_heading && !paragraph.is_empty() {
            blocks.push(placed(MarkdownBlock::Paragraph { text: paragraph.join("\n") }, start));
        }
    }

    blocks
}

// MARK: - Front matter

/// A front-matter block if the document opens with one: `---` (YAML, closed by `---` or `...`) or
/// `+++` (TOML, closed by `+++`). Three guards, each load-bearing because the YAML opener is
/// spelled like a thematic break and getting this wrong hides the reader's prose: the block must
/// be closed, the line after the opener must not be blank, and at least one `key: value` field
/// must be read. Nothing is committed until all three pass. Returns the fields and the index of
/// the line after the closer.
fn parse_front_matter(lines: &[&str]) -> Option<(Vec<MetadataField>, usize)> {
    if lines.is_empty() {
        return None;
    }
    let (closers, separator): (&[&str], char) = match trim_ws(lines[0]) {
        "---" => (&["---", "..."], ':'),
        "+++" => (&["+++"], '='),
        _ => return None,
    };

    if lines.len() <= 1 || is_blank(lines[1]) {
        return None;
    }

    let close = (1..lines.len()).find(|&index| closers.contains(&trim_ws(lines[index])))?;

    let mut fields = Vec::new();
    for line in &lines[1..close] {
        let trimmed = trim_ws(line);
        // Comments and list items are skipped; the block is still consumed. The scan is flat, so
        // an indented child that carries its own separator is read as a field of its own.
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        let Some(split) = trimmed.find(separator) else { continue };
        let key = trim_ws(&trimmed[..split]);
        if key.is_empty() {
            continue;
        }
        let mut value = trim_ws(&trimmed[split + 1..]);
        // One layer of matching quotes, `"` tried before `'`. md.win counts UTF-16 units and this
        // counts bytes; the two agree, since a value that starts and ends with an ASCII quote is
        // one unit long exactly when it is one byte long. A mark behind the opening quote stays.
        for quote in ['"', '\''] {
            if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
                value = &value[1..value.len() - 1];
                break;
            }
        }
        fields.push(MetadataField { key: key.to_string(), value: value.to_string() });
    }
    if fields.is_empty() {
        return None;
    }
    Some((fields, close + 1))
}

// MARK: - Footnote definitions

/// `[^id]: the note` → `(id, text)`, or `None`. Identifiers are ASCII `[A-Za-z0-9_-]` — narrower
/// than Pandoc on purpose: it is exactly the set the HTML writer's reference pattern accepts, so no
/// definition can exist that no reference can name. The `:` must follow `]` immediately; the text
/// is WS-trimmed.
pub fn parse_footnote_definition(line: &str) -> Option<(&str, &str)> {
    let trimmed = trim_ws(line);
    if !trimmed.starts_with("[^") {
        return None;
    }
    let close = 2 + trimmed[2..].find(']')?;
    let id = &trimmed[2..close];
    if id.is_empty() || !id.bytes().all(is_footnote_identifier) {
        return None;
    }
    if trimmed.as_bytes().get(close + 1) != Some(&b':') {
        return None;
    }
    Some((id, trim_ws(&trimmed[close + 2..])))
}

fn is_footnote_identifier(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

// MARK: - Headings

/// ATX heading: any number of leading spaces (unbounded, unlike a fence), 1–6 `#`, then U+0020 or
/// end of line. A tab after the run does not count.
fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let b = line.as_bytes();
    let mut p = 0;
    while p < b.len() && b[p] == b' ' {
        p += 1;
    }
    if p >= b.len() || b[p] != b'#' {
        return None;
    }
    let mut level = 0;
    while p < b.len() && b[p] == b'#' && level < 7 {
        level += 1;
        p += 1;
    }
    if !(1..=6).contains(&level) {
        return None;
    }
    if p < b.len() && b[p] != b' ' {
        return None;
    }
    Some((level, strip_closing_hashes(trim_ws(&line[p..]))))
}

/// Remove a closing `#` run only when preceded by SPTAB, per CommonMark — so `C#` and `F#` keep
/// theirs; a text of only `#`s becomes empty.
fn strip_closing_hashes(text: &str) -> &str {
    let b = text.as_bytes();
    let mut end = b.len();
    while end > 0 && b[end - 1] == b'#' {
        end -= 1;
    }
    if end == b.len() {
        return text;
    }
    if end == 0 {
        return "";
    }
    // A byte that is not ASCII (the last byte of a mark, say) is neither space nor tab, as the
    // UTF-16 unit it stands for is not.
    let before = b[end - 1];
    if before != b' ' && before != b'\t' {
        return text;
    }
    trim_ws(&text[..end])
}

/// A non-empty WS-trimmed line of only `=` (1) or only `-` (2).
fn setext_underline(line: &str) -> Option<usize> {
    let t = trim_ws(line);
    if t.is_empty() {
        return None;
    }
    if t.bytes().all(|c| c == b'=') {
        return Some(1);
    }
    if t.bytes().all(|c| c == b'-') {
        return Some(2);
    }
    None
}

// MARK: - Page breaks & comments

fn is_page_break(line: &str) -> bool {
    let t = trim_ws(line);
    t == "\\newpage" || t == "\\pagebreak"
}

/// Leading U+0020 only (a tab disqualifies), then `<!--`.
fn is_comment_start(line: &str) -> bool {
    drop_leading_spaces(line).starts_with("<!--")
}

/// `<!-- note: … -->` → the note text (possibly empty); any other comment → `None`. `-->` is
/// searched from index 0, not from the opener, so a stray `-->` before `<!--` drops the whole
/// comment — wart, kept. The body is the only WSNL trim in the parser (U+0085 is whitespace here).
/// The prefix is tested case-folded (.NET's simple invariant mapping, scalar by scalar — which is
/// what lowercasing the whole body and testing its prefix decides) and dropped from the original:
/// five scalars, since no lowercase mapping yields any of `note:` from fewer.
fn note_text(comment: &str) -> Option<String> {
    let open = scalar_text::first_index(comment, "<!--", 0)?;
    let close = scalar_text::first_index(comment, "-->", 0).unwrap_or(comment.len());
    if open + 4 > close {
        return None;
    }
    let body = trim_wsnl(&comment[open + 4..close]);
    if !lowercase_starts_with(body, "note:") {
        return None;
    }
    Some(trim_wsnl(scalar_text::drop_first(body, 5)).to_string())
}

/// `lower_invariant(text).starts_with(prefix)` for an ASCII `prefix`, without lowering the whole
/// text: the simple mapping turns one scalar into one scalar, so the prefix is decided by as many
/// leading scalars as it has.
fn lowercase_starts_with(text: &str, prefix: &str) -> bool {
    let mut chars = text.chars();
    prefix.chars().all(|expected| chars.next().is_some_and(|c| unicode::to_lower_invariant(c) == expected))
}

// MARK: - Outline, notes & anchors

/// Every ATX / setext heading outside a fence and outside front matter, with its 0-based line and
/// the same slug the HTML writer assigns — the two share one counter walked in document order, so
/// any line this counts as a heading and [`parse`] does not (or the reverse) drifts every later
/// anchor. Front matter and footnote definitions are therefore skipped exactly as [`parse`] skips
/// them. A table header row and a list continuation line are *not* excluded from plain text, so
/// `| a |\n---` yields a phantom entry — shipped divergence, replicated by every port.
pub fn outline(source: &str) -> Vec<OutlineEntry> {
    let lines = normalized_lines(source);
    let mut entries = Vec::new();
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut fence: Option<FenceMarker<'_>> = None;
    let mut previous_plain: Option<(&str, usize)> = None;
    // `parse` treats an underline as setext only when exactly ONE line is buffered.
    let mut plain_run = 0;
    let mut i = 0;
    if let Some((_, next)) = parse_front_matter(&lines) {
        i = next;
    }
    while i < lines.len() {
        let line = lines[i];
        if let Some(open) = &fence {
            if open.closes(line) {
                fence = None;
            }
            previous_plain = None;
            plain_run = 0;
            i += 1;
            continue;
        }
        if let Some(open) = FenceMarker::from(line) {
            fence = Some(open);
            previous_plain = None;
            plain_run = 0;
            i += 1;
            continue;
        }
        if is_comment_start(line) {
            // A different loop shape from `parse`, same net cursor; an unclosed comment ends at
            // lines.len() + 1, which the while guard tolerates.
            while i < lines.len() && !scalar_text::contains(lines[i], "-->") {
                i += 1;
            }
            previous_plain = None;
            plain_run = 0;
            i += 1;
            continue;
        }
        if let Some((level, text)) = parse_heading(line) {
            entries.push(OutlineEntry { level, text: text.to_string(), slug: slug(text, &mut used), line: i });
            previous_plain = None;
            plain_run = 0;
            i += 1;
            continue;
        }
        if let Some((text, line_number)) = previous_plain
            && plain_run == 1
            && let Some(level) = setext_underline(line)
        {
            entries.push(OutlineEntry {
                level,
                text: text.to_string(),
                slug: slug(text, &mut used),
                line: line_number,
            });
            previous_plain = None;
            plain_run = 0;
            i += 1;
            continue;
        }
        let trimmed = trim_ws(line);
        let is_plain = !trimmed.is_empty()
            && !is_thematic_break(line)
            && !is_quote(line)
            && list_marker(line).is_none()
            && !is_page_break(line)
            && parse_footnote_definition(line).is_none();
        plain_run = if is_plain { plain_run + 1 } else { 0 };
        previous_plain = if is_plain { Some((trimmed, i)) } else { None };
        i += 1;
    }
    entries
}

/// Every author note outside a fence and outside front matter, with the 0-based line its comment
/// opens on.
pub fn notes(source: &str) -> Vec<NoteEntry> {
    let lines = normalized_lines(source);
    let mut entries = Vec::new();
    let mut fence: Option<FenceMarker<'_>> = None;
    let mut i = 0;
    if let Some((_, next)) = parse_front_matter(&lines) {
        i = next;
    }
    while i < lines.len() {
        let line = lines[i];
        if let Some(open) = &fence {
            if open.closes(line) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if let Some(open) = FenceMarker::from(line) {
            fence = Some(open);
            i += 1;
            continue;
        }
        if is_comment_start(line) {
            let start = i;
            let mut raw: Vec<&str> = Vec::new();
            while i < lines.len() {
                raw.push(lines[i]);
                let closed = scalar_text::contains(lines[i], "-->");
                i += 1;
                if closed {
                    break;
                }
            }
            if let Some(note) = note_text(&raw.join("\n")) {
                entries.push(NoteEntry { text: note, line: start });
            }
            continue;
        }
        i += 1;
    }
    entries
}

/// GitHub-style anchor slug, unique within one document through the caller's `used` counts
/// (`title`, `title-1`, …). The whole string is lowercased first with the full mapping
/// ([`scalar_text::full_lowercase`]), then walked by code point: letters, numbers, marks, `_` and
/// `-` survive, U+0020 alone becomes `-`, everything else (tab, NBSP, ZWJ, punctuation) is
/// dropped; empty → `section`.
///
/// Code points, not units: a surrogate half is neither a letter nor a mark, so a UTF-16 walk would
/// drop `𝐀`. Marks are kept even on a character the slug drops (`a -́b` → `a--́b-c`), which is
/// what Android's code-point walk always did. The counter counts the bare slug, so `Same`,
/// `Same 1`, `Same` give `same`, `same-1`, `same-1` — a duplicate id, as on GitHub; replicated, not
/// fixed. Never normalise: NFC and NFD headings are different anchors on every platform.
pub fn slug(text: &str, used: &mut HashMap<String, usize>) -> String {
    let mut builder = String::with_capacity(text.len());
    for c in scalar_text::full_lowercase(text).chars() {
        if is_slug_kept(c) {
            builder.push(c);
        } else if c == ' ' {
            builder.push('-');
        }
    }
    let slug = if builder.is_empty() { "section".to_string() } else { builder };
    let seen = used.get(&slug).copied().unwrap_or(0);
    used.insert(slug.clone(), seen + 1);
    if seen == 0 { slug } else { format!("{slug}-{seen}") }
}

/// Swift: `isAlphabetic || numericType != nil || Mn/Mc/Me || _ -`. Here: letters (L*), numbers
/// (Nd, Nl, No — which is `numericType != nil` except the CJK numeral ideographs, which are Lo and
/// letters anyway), marks, the enclosed Latin letters that make up the rest of `Alphabetic`, and
/// the two punctuation marks GitHub keeps.
fn is_slug_kept(c: char) -> bool {
    c == '_'
        || c == '-'
        || unicode::is_letter(c)
        || unicode::is_number(c)
        || scalar_text::is_mark(c)
        || scalar_text::is_enclosed_alphabetic(c)
}

// MARK: - Raw diagram documents

/// True when `source` is a bare PlantUML file rather than Markdown: its first non-blank,
/// non-`'`-comment line starts with `@start` (any diagram: uml, mindmap, gantt, json, …). The
/// first qualifying line decides, so prose mentioning `@startuml` stays Markdown. Lines come from
/// the wide newline set; the trim is WSNL.
pub fn is_raw_plant_uml(source: &str) -> bool {
    for raw_line in lines(source) {
        let line = trim_wsnl(raw_line);
        if line.is_empty() || line.starts_with('\'') {
            continue;
        }
        return line.starts_with("@start");
    }
    false
}

/// True when `source` is a bare Graphviz DOT file: its first non-blank, non-comment (`//`, `/*`)
/// line matches `[strict] (graph|digraph) [ID] '{'`, keywords case-insensitive, the brace allowed
/// on a later line. A `#` line is deliberately not a comment here — every Markdown heading starts
/// with one, and skipping it would let the probe read past a title into prose. The `{` pre-guard
/// is cheap, not sufficient.
pub fn is_raw_graphviz(source: &str) -> bool {
    if !source.contains('{') {
        return false;
    }
    for raw_line in lines(source) {
        let line = trim_wsnl(raw_line);
        if line.is_empty() || line.starts_with("//") || line.starts_with("/*") {
            continue;
        }
        let lowered = unicode::lower_invariant(line);
        let mut head = lowered.as_str();
        if let Some(rest) = head.strip_prefix("strict ") {
            head = trim_ws(rest);
        }
        // `digraph` first: it contains `graph`, and the shorter prefix would misread it.
        for keyword in ["digraph", "graph"] {
            if let Some(tail) = head.strip_prefix(keyword) {
                return is_graph_header(tail);
            }
        }
        return false;
    }
    false
}

/// The wide, Foundation-`.newlines` splitter the two raw-diagram probes use — not the block
/// scanner's.
pub fn lines(source: &str) -> Vec<&str> {
    newline_set_lines(source)
}

/// What follows the keyword: nothing, `{`, or a name (bare or quoted) separated from the keyword
/// by SPTAB and then nothing or `{`. The separator rule is what keeps `digraphs` and `graphviz`
/// words; the empty/brace short-circuit before it is what lets `digraph{a}` through.
fn is_graph_header(tail: &str) -> bool {
    let mut rest = drop_space_tab(tail);
    if rest.is_empty() || rest.starts_with('{') {
        return true;
    }
    if !tail.starts_with([' ', '\t']) {
        return false;
    }
    if let Some(quoted) = rest.strip_prefix('"') {
        let Some(close) = quoted.find('"') else { return false };
        rest = &quoted[close + 1..];
    } else {
        // By code point, keeping marks: Swift asks `isLetter` of a whole grapheme, so an NFD
        // `café` is one letter there; walking units, U+0301 is a mark and nothing else. Scalars
        // also keep an astral letter, which a UTF-16 unit scan would end the name at.
        let length: usize = rest.chars().take_while(|&c| is_graph_name_char(c)).map(char::len_utf8).sum();
        if length == 0 {
            return false;
        }
        rest = &rest[length..];
    }
    let rest = drop_space_tab(rest);
    rest.is_empty() || rest.starts_with('{')
}

fn is_graph_name_char(c: char) -> bool {
    c == '_' || unicode::is_letter(c) || unicode::is_number(c) || scalar_text::is_mark(c)
}

// MARK: - Thematic break

/// Three or more of one of `-`, `*`, `_` with only SPTAB between — narrower than every other
/// predicate, so `U+00A0---` is a paragraph.
fn is_thematic_break(line: &str) -> bool {
    let mut marker = 0u8;
    let mut count = 0;
    for c in line.bytes() {
        if c == b' ' || c == b'\t' {
            continue;
        }
        if c != b'-' && c != b'*' && c != b'_' {
            return false;
        }
        if count == 0 {
            marker = c;
        } else if c != marker {
            return false;
        }
        count += 1;
    }
    count >= 3
}

// MARK: - Block quote

fn is_quote(line: &str) -> bool {
    drop_leading_spaces(line).starts_with('>')
}

/// Leading spaces, one `>`, and exactly one optional space — a mark after that stays.
fn strip_quote_marker(line: &str) -> &str {
    let b = line.as_bytes();
    let mut p = 0;
    while p < b.len() && b[p] == b' ' {
        p += 1;
    }
    if p < b.len() && b[p] == b'>' {
        p += 1;
    }
    if p < b.len() && b[p] == b' ' {
        p += 1;
    }
    &line[p..]
}

// MARK: - Lists

struct Marker<'a> {
    level: usize,
    ordinal: Option<u32>,
    text: &'a str,
    task: Option<bool>,
}

/// `-` / `*` / `+` or 1–9 ASCII digits then `.` / `)`, followed by U+0020 or end of line (a tab
/// does not count; a bare `-` is an item with empty text). Indentation counts spaces and tabs to
/// the next 4-column stop — the only place a tab indents — and `level` is `indent / 2`. Ten or
/// more digits is not a marker; leading zeros are lost.
fn list_marker(line: &str) -> Option<Marker<'_>> {
    let b = line.as_bytes();
    let mut indent = 0usize;
    let mut start = 0;
    while start < b.len() {
        match b[start] {
            b' ' => indent += 1,
            b'\t' => indent += 4 - indent % 4,
            _ => break,
        }
        start += 1;
    }
    if start >= b.len() {
        return None;
    }

    let first = b[start];
    let mut ordinal = None;
    let rest_start;
    if first == b'-' || first == b'*' || first == b'+' {
        rest_start = start + 1;
    } else if first.is_ascii_digit() {
        let mut d = start;
        while d < b.len() && b[d].is_ascii_digit() {
            d += 1;
        }
        let digits = d - start;
        if digits > 9 || d >= b.len() {
            return None;
        }
        let delim = b[d];
        if delim != b'.' && delim != b')' {
            return None;
        }
        // At most nine ASCII digits: always fits, leading zeros dropped by the arithmetic.
        ordinal = Some(b[start..d].iter().fold(0u32, |value, &digit| value * 10 + u32::from(digit - b'0')));
        rest_start = d + 1;
    } else {
        return None;
    }

    if rest_start < b.len() && b[rest_start] != b' ' {
        return None;
    }
    let mut t = rest_start;
    while t < b.len() && b[t] == b' ' {
        t += 1;
    }
    let mut text = &line[t..];

    // md.win tests `"[ ] "` / `"[ ]"` ordinally, then `"[x] "` / `"[x]"` on a lowercased copy. No
    // scalar but `x` and `X` lowercases to `x`, and the brackets and space are their own lowercase,
    // so the second test is exactly the four ASCII spellings below.
    let tb = text.as_bytes();
    let mut task = None;
    let boxed = tb.len() >= 3 && tb[0] == b'[' && tb[2] == b']' && (tb.len() == 3 || tb[3] == b' ');
    if boxed && tb[1] == b' ' {
        task = Some(false);
        text = unchecked_text(text);
    } else if boxed && (tb[1] == b'x' || tb[1] == b'X') {
        task = Some(true);
        text = unchecked_text(text);
    }
    Some(Marker { level: indent / 2, ordinal, text, task })
}

/// The three-unit box and the spaces after it; a mark the author put on the next character stays.
fn unchecked_text(text: &str) -> &str {
    drop_leading_spaces(scalar_text::drop_first(text, 3))
}

// MARK: - Tables (GFM)

/// Header row plus delimiter row (`| :--- | :---: | ---: |`), or `None`. Every delimiter cell must
/// be `:?-+:?`; the header must have exactly as many cells.
fn parse_table(header: &str, delimiter: &str) -> Option<(Vec<String>, Vec<ColumnAlignment>)> {
    if !header.contains('|') {
        return None;
    }
    if !trim_ws(delimiter).contains('-') {
        return None;
    }
    let delim_cells = split_table_row(delimiter, None);
    if delim_cells.is_empty() {
        return None;
    }
    let mut alignments = Vec::with_capacity(delim_cells.len());
    for cell in &delim_cells {
        let c = trim_ws(cell);
        if c.is_empty() {
            return None;
        }
        let mut saw_dash = false;
        for ch in c.bytes() {
            if ch == b'-' {
                saw_dash = true;
            } else if ch != b':' {
                return None;
            }
        }
        if !saw_dash {
            return None;
        }
        let left = c.starts_with(':');
        let right = c.ends_with(':');
        alignments.push(if left && right {
            ColumnAlignment::Center
        } else if right {
            ColumnAlignment::Trailing
        } else {
            ColumnAlignment::Leading
        });
    }
    let header_cells = split_table_row(header, None);
    if header_cells.len() != alignments.len() {
        return None;
    }
    Some((header_cells, alignments))
}

/// One row into WS-trimmed cells. Optional leading and trailing pipe (the trailing test runs after
/// the leading drop, so a row of a single `|` is one empty cell); `\|` is a literal pipe, `\x`
/// stays `\x`, a trailing lone backslash is kept. With `columns`, padded with `""` or truncated to
/// that width.
fn split_table_row(row: &str, columns: Option<usize>) -> Vec<String> {
    let trimmed = trim_ws(row);
    let b = trimmed.as_bytes();
    let mut start = 0;
    let mut end = b.len();
    if start < end && b[start] == b'|' {
        start += 1;
    }
    if end > start && b[end - 1] == b'|' {
        end -= 1;
    }

    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;
    for ch in trimmed[start..end].chars() {
        if escaped {
            if ch != '|' {
                current.push('\\');
            }
            current.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '|' {
            cells.push(trim_ws(&current).to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    if escaped {
        current.push('\\');
    }
    cells.push(trim_ws(&current).to_string());

    if let Some(width) = columns {
        cells.resize(width, String::new());
    }
    cells
}

// MARK: - Fence helper

/// A fenced-code delimiter: up to three leading spaces (four or a tab is not a fence — there is no
/// indented-code branch, so the line falls through to a paragraph), a run of at least three `` ` ``
/// or `~`, and an info string whose first U+0020-delimited word is the language. A backtick
/// fence's info string may not contain a backtick; a tilde fence's may.
struct FenceMarker<'a> {
    character: u8,
    count: usize,
    indent: usize,
    language: Option<&'a str>,
}

impl<'a> FenceMarker<'a> {
    fn from(line: &'a str) -> Option<FenceMarker<'a>> {
        let b = line.as_bytes();
        let mut indent = 0;
        while indent < b.len() && b[indent] == b' ' {
            indent += 1;
        }
        if indent > 3 || indent >= b.len() {
            return None;
        }
        let first = b[indent];
        if first != b'`' && first != b'~' {
            return None;
        }
        let mut run = indent;
        while run < b.len() && b[run] == first {
            run += 1;
        }
        let count = run - indent;
        if count < 3 {
            return None;
        }
        // WS trim, then split on U+0020 alone: "```js\tfoo" has language "js\tfoo".
        let info = trim_ws(&line[run..]);
        if first == b'`' && info.contains('`') {
            return None;
        }
        let language = match info.find(' ') {
            Some(space) => &info[..space],
            None => info,
        };
        Some(FenceMarker { character: first, count, indent, language: (!language.is_empty()).then_some(language) })
    }

    /// Same char, at least as long, indented arbitrarily (no 3-space cap), nothing but WS after.
    fn closes(&self, line: &str) -> bool {
        let b = line.as_bytes();
        let mut p = 0;
        while p < b.len() && b[p] == b' ' {
            p += 1;
        }
        let mut run = p;
        while run < b.len() && b[run] == self.character {
            run += 1;
        }
        if run - p < self.count {
            return false;
        }
        is_blank(&line[run..])
    }

    /// Remove up to the opening fence's indentation — U+0020 only, stopping at the first non-space.
    fn strip_indent<'l>(&self, line: &'l str) -> &'l str {
        let b = line.as_bytes();
        let mut removed = 0;
        while removed < self.indent && removed < b.len() && b[removed] == b' ' {
            removed += 1;
        }
        &line[removed..]
    }
}
