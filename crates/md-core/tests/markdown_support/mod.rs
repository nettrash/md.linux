//! The shape most markdown tests assert, shared by every parser- and writer-side suite — md.win's
//! `Blocks` helper class (`MarkdownParserTests.cs`). Every expected value in those suites was
//! produced by the real md.macOS parser (the Swift source compiled as an oracle), not reasoned
//! about — the point of the family is byte parity with what ships.
#![allow(dead_code)]

use md_core::markdown::{BlockKind, ColumnAlignment, ListItem, MarkdownBlock, MetadataField, parser};

/// The three characters the whole parity family is written around: COMBINING ACUTE ACCENT,
/// VARIATION SELECTOR-16, ZERO WIDTH JOINER. Each fuses onto the ASCII delimiter before it in a
/// grapheme-aware string model; a code-unit (or byte) walk passes these by construction, which is
/// exactly why they stay — they pin outputs, and a port that reached for graphemes, normalisation
/// or a culture-sensitive search would break them silently.
pub const MARKS: [&str; 3] = ["\u{0301}", "\u{FE0F}", "\u{200D}"];

pub const NBSP: &str = "\u{00A0}";
pub const NEL: &str = "\u{0085}";
pub const ZWSP: &str = "\u{200B}";

pub fn parse(source: &str) -> Vec<MarkdownBlock> {
    parser::parse(source)
}

pub fn kinds(source: &str) -> Vec<BlockKind> {
    parse(source).iter().map(MarkdownBlock::kind).collect()
}

/// `Blocks.First<T>`: the first block of a non-empty parse (the variant is asserted by the
/// accessor the caller applies, or by comparing with a whole block).
pub fn first(source: &str) -> MarkdownBlock {
    let blocks = parse(source);
    assert!(!blocks.is_empty(), "no blocks in {source:?}");
    blocks.into_iter().next().unwrap()
}

/// `Blocks.Single<T>`: the only block.
pub fn single(source: &str) -> MarkdownBlock {
    let blocks = parse(source);
    assert_eq!(blocks.len(), 1, "expected one block in {source:?}, got {blocks:?}");
    blocks.into_iter().next().unwrap()
}

/// `string.Join("\n", lines)`.
pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

/// Owned rows from string-slice rows, for comparing tables.
pub fn rows(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter().map(|row| row.iter().map(|cell| cell.to_string()).collect()).collect()
}

pub fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

// `Blocks.As<T>`: the variant's fields, or a failure naming what the block was.

pub fn heading(block: &MarkdownBlock) -> (usize, String) {
    match block {
        MarkdownBlock::Heading { level, text } => (*level, text.clone()),
        other => panic!("expected a heading, got {other:?}"),
    }
}

pub fn paragraph(block: &MarkdownBlock) -> String {
    match block {
        MarkdownBlock::Paragraph { text } => text.clone(),
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

pub fn list(block: &MarkdownBlock) -> (bool, Vec<ListItem>) {
    match block {
        MarkdownBlock::List { ordered, items } => (*ordered, items.clone()),
        other => panic!("expected a list, got {other:?}"),
    }
}

pub fn items(block: &MarkdownBlock) -> Vec<ListItem> {
    list(block).1
}

pub fn code(block: &MarkdownBlock) -> (Option<String>, String) {
    match block {
        MarkdownBlock::CodeBlock { language, code } => (language.clone(), code.clone()),
        other => panic!("expected a code block, got {other:?}"),
    }
}

pub fn quote(block: &MarkdownBlock) -> Vec<MarkdownBlock> {
    match block {
        MarkdownBlock::Quote { blocks } => blocks.clone(),
        other => panic!("expected a quote, got {other:?}"),
    }
}

pub fn table(block: &MarkdownBlock) -> (Vec<String>, Vec<ColumnAlignment>, Vec<Vec<String>>) {
    match block {
        MarkdownBlock::Table { header, alignments, rows } => (header.clone(), alignments.clone(), rows.clone()),
        other => panic!("expected a table, got {other:?}"),
    }
}

pub fn front_matter(block: &MarkdownBlock) -> Vec<MetadataField> {
    match block {
        MarkdownBlock::FrontMatter { fields } => fields.clone(),
        other => panic!("expected front matter, got {other:?}"),
    }
}

pub fn note(block: &MarkdownBlock) -> String {
    match block {
        MarkdownBlock::Note { text } => text.clone(),
        other => panic!("expected a note, got {other:?}"),
    }
}

pub fn footnote(block: &MarkdownBlock) -> (String, String) {
    match block {
        MarkdownBlock::FootnoteDefinition { id, text } => (id.clone(), text.clone()),
        other => panic!("expected a footnote definition, got {other:?}"),
    }
}

// Block constructors, short.

pub fn p(text: &str) -> MarkdownBlock {
    MarkdownBlock::paragraph(text)
}

pub fn h(level: usize, text: &str) -> MarkdownBlock {
    MarkdownBlock::heading(level, text)
}

pub fn cb(language: Option<&str>, code: &str) -> MarkdownBlock {
    MarkdownBlock::code_block(language, code)
}

pub fn fd(id: &str, text: &str) -> MarkdownBlock {
    MarkdownBlock::footnote_definition(id, text)
}

pub fn field(key: &str, value: &str) -> MetadataField {
    MetadataField::new(key, value)
}

pub fn item(text: &str, level: usize, ordinal: Option<u32>, task: Option<bool>) -> ListItem {
    ListItem::new(text, level, ordinal, task)
}
