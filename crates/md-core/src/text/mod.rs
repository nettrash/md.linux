//! Text primitives every other module builds on. Port of md.win `Md.Core.Text`, plus the
//! Unicode answers .NET gives md.win for free.

pub mod graphemes;
pub mod scalar_text;
pub mod unicode;
#[rustfmt::skip]
pub(crate) mod unicode_tables;
pub mod utf16;
pub mod whitespace;
