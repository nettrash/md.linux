//! Port of md.win `Md.Core.Markdown` (minus `Plot`, which is [`crate::plot`]): the block model,
//! the parser, the inline and HTML writers, the stylesheet and delimited tables.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`block`] | `MarkdownBlock.cs` |
//! | [`parser`] | `MarkdownParser.cs` |
//! | [`html`] | `MarkdownHtml.cs` |
//! | `inline_html` (behind [`html::inline`] / [`html::escape`]) | `MarkdownInlineHtml.cs` |
//! | `css` (behind [`html::css`]) | `MarkdownCss.cs` |
//! | [`delimited_table`] | `DelimitedTable.cs` |
//!
//! The block model's types are re-exported here, so `markdown::MarkdownBlock` and
//! `markdown::block::MarkdownBlock` name the same thing.

pub mod block;
mod css;
pub mod delimited_table;
pub mod html;
mod inline_html;
pub mod parser;

pub use block::{
    BlockKind, ColumnAlignment, ListItem, MarkdownBlock, MetadataField, NoteEntry, OutlineEntry, PlacedBlock,
};
pub use delimited_table::DelimitedTable;
pub use html::{EngineNeeds, RenderedBody};
