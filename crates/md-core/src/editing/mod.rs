//! Port of md.win `Md.Core.Text.SmartTyping` and `Md.Core.Text.MarkdownFormatting`: the keystroke
//! and formatting rules every md port shares, pinned by `typing-vectors.json` and
//! `formatting-vectors.json`. Specifications: `docs/smart-typing.md` and `docs/formatting.md`.
//!
//! **Offsets are UTF-16 code units** everywhere in this module — in, out and in the vectors —
//! including in the entry points that take a `&str` (the exception to md-core's byte-offset rule
//! that `docs/port-plan.md` names). Selection offsets are `isize` and may come in either order:
//! the rules themselves answer an offset outside the text, as md.win's `int` does. Results are
//! always offsets into the text, so they are `usize`. GTK counts in characters: convert with
//! [`crate::text::utf16::utf16_from_char_offset`] and
//! [`crate::text::utf16::char_offset_from_utf16`].
//!
//! Each rule has a `&str` entry point and a `_utf16` one over
//! [`Utf16Text`](crate::text::utf16::Utf16Text); the rules run on the latter so that their index
//! arithmetic reads line for line like the C#. A Rust string cannot hold a lone surrogate, so the
//! `_utf16` entry points are the only way to give the rules one (md.win's native lone-surrogate
//! tests use them).

pub mod markdown_formatting;
pub mod smart_typing;

pub use markdown_formatting::{FormatEdit, FormatOp};
pub use smart_typing::EnterEdit;
