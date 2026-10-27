//! The block model. Port of md.win `Md.Core.Markdown.MarkdownBlock` (and the small records beside
//! it): eleven block kinds, and that is the whole model.

/// The eleven block kinds.
///
/// There is no image, math, CSV, HTML or link-reference-definition kind: math, CSV/TSV and the
/// diagram languages are decided by the *renderer* from a code block's language, or by the
/// inline pass. The list is flat except for [`BlockKind::Quote`], the only recursive kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Heading,
    Paragraph,
    List,
    CodeBlock,
    Quote,
    Table,
    ThematicBreak,
    PageBreak,
    Note,
    FrontMatter,
    FootnoteDefinition,
}

/// Column alignment of a GFM table, as written in the delimiter row. `:---` and `---` are both
/// [`ColumnAlignment::Leading`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnAlignment {
    Leading,
    Center,
    Trailing,
}

/// One `key: value` line of a document's front matter. Order is the order written and duplicate
/// keys are kept — a record of what the author wrote, not a dictionary; consumers take the first
/// occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MetadataField {
    pub key: String,
    pub value: String,
}

impl MetadataField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        MetadataField { key: key.into(), value: value.into() }
    }
}

/// A single list row. `level` is a depth tag (`indent / 2`), not a tree, so the renderer insets
/// without reconstructing nested lists. `ordinal` is `None` for bullets (at most nine digits, so it
/// fits a `u32`); `task` is `None` for a plain item, `Some(false)` for `[ ]`, `Some(true)` for
/// `[x]` / `[X]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ListItem {
    pub text: String,
    pub level: usize,
    pub ordinal: Option<u32>,
    pub task: Option<bool>,
}

impl ListItem {
    pub fn new(text: impl Into<String>, level: usize, ordinal: Option<u32>, task: Option<bool>) -> Self {
        ListItem { text: text.into(), level, ordinal, task }
    }
}

/// One table-of-contents entry. `line` is 0-based; for a setext heading it is the text line, not
/// the underline. `slug` is the id the HTML writer gives the same heading.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutlineEntry {
    pub level: usize,
    pub text: String,
    pub slug: String,
    pub line: usize,
}

impl OutlineEntry {
    pub fn new(level: usize, text: impl Into<String>, slug: impl Into<String>, line: usize) -> Self {
        OutlineEntry { level, text: text.into(), slug: slug.into(), line }
    }
}

/// One private author note (`<!-- note: … -->`). `line` is the 0-based line the comment opens on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteEntry {
    pub text: String,
    pub line: usize,
}

impl NoteEntry {
    pub fn new(text: impl Into<String>, line: usize) -> Self {
        NoteEntry { text: text.into(), line }
    }
}

/// A top-level block paired with the 0-based source line it started on — what an editor needs for
/// scroll sync. Kept beside the block rather than inside it so [`MarkdownBlock`] stays identical to
/// the Swift model every export path depends on. Blocks nested in a quote carry no line: the
/// recursion re-parses stripped text whose lines do not address the outer document.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlacedBlock {
    pub block: MarkdownBlock,
    pub line: usize,
}

/// One parsed block. Blocks carry no id and no source range (the Swift preview keys on index);
/// equality is structural, list members included, so two parses of one document compare equal —
/// the `parse == parse_with_lines.map(block)` invariant is asserted that way.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MarkdownBlock {
    /// ATX or setext heading; `text` is trimmed and has any closing `#` run stripped.
    Heading {
        level: usize,
        text: String,
    },
    /// Raw lines joined by `"\n"`. Not trimmed: leading and trailing spaces survive.
    Paragraph {
        text: String,
    },
    /// One flag for the whole run: `- a\n1. b` is ordered with ordinals `[None, Some(1)]`.
    List {
        ordered: bool,
        items: Vec<ListItem>,
    },
    /// `language` is the first U+0020-delimited word of the info string, `None` when empty.
    /// Never lowercased here — the renderer folds case at dispatch.
    CodeBlock {
        language: Option<String>,
        code: String,
    },
    /// The only recursive kind.
    Quote {
        blocks: Vec<MarkdownBlock>,
    },
    /// Header cells, one alignment per header cell, and body rows padded or truncated to the
    /// header width.
    Table {
        header: Vec<String>,
        alignments: Vec<ColumnAlignment>,
        rows: Vec<Vec<String>>,
    },
    ThematicBreak,
    /// A line holding exactly `\newpage` or `\pagebreak`.
    PageBreak,
    /// A `<!-- note: … -->` author note; never rendered.
    Note {
        text: String,
    },
    /// Only ever the first block, only at quote depth 0.
    FrontMatter {
        fields: Vec<MetadataField>,
    },
    /// `[^id]: text`, with wrapped continuation lines absorbed. `id` is ASCII `[A-Za-z0-9_-]` only.
    FootnoteDefinition {
        id: String,
        text: String,
    },
}

impl MarkdownBlock {
    pub fn kind(&self) -> BlockKind {
        match self {
            MarkdownBlock::Heading { .. } => BlockKind::Heading,
            MarkdownBlock::Paragraph { .. } => BlockKind::Paragraph,
            MarkdownBlock::List { .. } => BlockKind::List,
            MarkdownBlock::CodeBlock { .. } => BlockKind::CodeBlock,
            MarkdownBlock::Quote { .. } => BlockKind::Quote,
            MarkdownBlock::Table { .. } => BlockKind::Table,
            MarkdownBlock::ThematicBreak => BlockKind::ThematicBreak,
            MarkdownBlock::PageBreak => BlockKind::PageBreak,
            MarkdownBlock::Note { .. } => BlockKind::Note,
            MarkdownBlock::FrontMatter { .. } => BlockKind::FrontMatter,
            MarkdownBlock::FootnoteDefinition { .. } => BlockKind::FootnoteDefinition,
        }
    }

    // The C# records' positional constructors, for callers and tests that build blocks by hand.

    pub fn heading(level: usize, text: impl Into<String>) -> Self {
        MarkdownBlock::Heading { level, text: text.into() }
    }

    pub fn paragraph(text: impl Into<String>) -> Self {
        MarkdownBlock::Paragraph { text: text.into() }
    }

    pub fn list(ordered: bool, items: Vec<ListItem>) -> Self {
        MarkdownBlock::List { ordered, items }
    }

    pub fn code_block(language: Option<&str>, code: impl Into<String>) -> Self {
        MarkdownBlock::CodeBlock { language: language.map(str::to_string), code: code.into() }
    }

    pub fn quote(blocks: Vec<MarkdownBlock>) -> Self {
        MarkdownBlock::Quote { blocks }
    }

    pub fn table(header: Vec<String>, alignments: Vec<ColumnAlignment>, rows: Vec<Vec<String>>) -> Self {
        MarkdownBlock::Table { header, alignments, rows }
    }

    pub fn note(text: impl Into<String>) -> Self {
        MarkdownBlock::Note { text: text.into() }
    }

    pub fn front_matter(fields: Vec<MetadataField>) -> Self {
        MarkdownBlock::FrontMatter { fields }
    }

    pub fn footnote_definition(id: impl Into<String>, text: impl Into<String>) -> Self {
        MarkdownBlock::FootnoteDefinition { id: id.into(), text: text.into() }
    }
}
