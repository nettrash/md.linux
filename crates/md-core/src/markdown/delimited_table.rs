//! The table a ```` ```csv ```` / ```` ```tsv ```` block describes — header row, inferred
//! alignments, body rows — shared by the HTML and LaTeX writers so one spreadsheet cannot be
//! right-aligned in the PDF and left-aligned in the `.tex`. Port of md.win
//! `Md.Core.Markdown.DelimitedTable`.
//!
//! The parser knows nothing about CSV: the block reaches the renderer as `CodeBlock("csv", …)`,
//! the renderer dispatches on the lowercased language (`csv` → `','`, `tsv` → `'\t'`) and falls
//! back to a bare `<pre><code>` when [`DelimitedTable::from`] returns `None`, so nothing the
//! author wrote disappears.

use super::block::ColumnAlignment;
use crate::text::whitespace;

/// Header cells, one inferred alignment per column (possibly more than the header has), and the
/// body rows exactly as parsed — neither padded nor truncated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DelimitedTable {
    pub header: Vec<String>,
    pub alignments: Vec<ColumnAlignment>,
    pub rows: Vec<Vec<String>>,
}

impl DelimitedTable {
    /// Build the table, or `None` when `code` parses to no rows. Alignment is inferred from the
    /// **body only** — the header never votes — and a column whose every filled cell is a number is
    /// [`ColumnAlignment::Trailing`]. Empty cells are ignored, not disqualifying; a column with no
    /// filled cell is leading. The alignment list may be longer than the header when a body row is
    /// wider.
    pub fn from(code: &str, separator: char) -> Option<DelimitedTable> {
        let mut rows = Self::parse_delimited(code, separator);
        if rows.is_empty() || rows[0].is_empty() {
            return None;
        }
        let body = rows.split_off(1);
        let header = rows.pop().unwrap_or_default();

        let columns = body.iter().map(Vec::len).fold(header.len(), usize::max);

        let mut alignments = Vec::with_capacity(columns);
        for column in 0..columns {
            let mut filled = 0;
            let mut numeric = true;
            for row in &body {
                let Some(raw) = row.get(column) else { continue };
                // SPTAB on purpose: Foundation's `.whitespaces` holds U+200B and Java's Zs trim does
                // not, so `U+200B 1 U+200B` must not be a number on any platform. The opposite call
                // from the parser's blank-line rule; do not unify the two.
                let cell = whitespace::trim_space_tab(raw);
                if cell.is_empty() {
                    continue;
                }
                filled += 1;
                if !Self::is_decimal_number(cell) {
                    numeric = false;
                }
            }
            alignments.push(if filled > 0 && numeric { ColumnAlignment::Trailing } else { ColumnAlignment::Leading });
        }
        Some(DelimitedTable { header, alignments, rows: body })
    }

    /// `[+-]? digits* ( '.' digits* )? ( [eE] [+-]? digits+ )?` with at least one digit before the
    /// exponent, ASCII digits only, the whole string consumed.
    ///
    /// Never `str::parse::<f64>`: it takes `inf`, `NaN` and `infinity` in any case, as .NET's
    /// `double.TryParse` takes thousands separators and culture decimals, Swift's `Double(_:)` takes
    /// hex and Java's takes `f`/`d` suffixes — standard libraries aligning the same spreadsheet
    /// different ways. Never a Unicode digit test either: `Nd` accepts `٣`.
    pub fn is_decimal_number(cell: &str) -> bool {
        let b = cell.as_bytes();
        let mut i = 0;
        let take_digits = |i: &mut usize| {
            let start = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            *i - start
        };

        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut digits = take_digits(&mut i);
        if i < b.len() && b[i] == b'.' {
            i += 1;
            digits += take_digits(&mut i);
        }
        if digits == 0 {
            return false;
        }

        if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
            i += 1;
            if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
                i += 1;
            }
            if take_digits(&mut i) == 0 {
                return false;
            }
        }
        i == b.len()
    }

    /// RFC 4180: a field may be quoted, a quoted field may hold the separator and line breaks, a
    /// doubled quote inside one is a literal quote. A quote only *opens* a quoted field while the field
    /// is still empty, so `5" pipe` is literal. A last row without a trailing newline is still a row; a
    /// trailing empty field is kept.
    ///
    /// Line endings are normalised first (CRLF, then lone CR) rather than matched — spreadsheet exports
    /// are where CRLF comes from. md.win walks UTF-16 units like Kotlin and TypeScript and this walks
    /// scalars, which splits identically for any BMP separator (the only kind a C# `char` can be): a
    /// separator carrying a combining mark *does* split here, where the Swift's grapheme walk did not.
    /// Recorded divergence; the code-unit behaviour is the contract for this port.
    pub fn parse_delimited(text: &str, separator: char) -> Vec<Vec<String>> {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut rows = Vec::new();
        let mut row: Vec<String> = Vec::new();
        let mut field = String::new();
        let mut quoted = false;
        let mut chars = normalized.chars().peekable();

        while let Some(c) = chars.next() {
            if quoted {
                if c == '"' {
                    if chars.peek() == Some(&'"') {
                        field.push('"');
                        chars.next();
                        continue;
                    }
                    quoted = false;
                } else {
                    field.push(c);
                }
                continue;
            }
            if c == '"' && field.is_empty() {
                quoted = true;
            } else if c == separator {
                row.push(std::mem::take(&mut field));
            } else if c == '\n' {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            } else {
                field.push(c);
            }
        }
        if !field.is_empty() || !row.is_empty() {
            row.push(field);
            rows.push(row);
        }
        rows
    }
}
