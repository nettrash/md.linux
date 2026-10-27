//! Port of md.win `Documents/LineOffsets.cs`.
//!
//! Caret arithmetic for the Contents / Notes jumps (§3.3). The Swift routine verbatim, in UTF-16
//! units — the unit every md-logic editor offset is in (md.win's `TextBox.SelectionStart`, and
//! md-core's editing rules). The GTK shell converts at the boundary: `GtkTextIter` offsets are
//! characters, and md-core's `text::utf16::char_offset_from_utf16` / `utf16_from_char_offset` are
//! the only place the two units meet.

/// The UTF-16 offset of the first character of `line` (0-based). `\n`, `\r` and `\r\n` each count
/// once; U+2028 / U+2029 deliberately do not, because the parser that produced the line number
/// does not count them either. A line past the end gives the start of the last existing line,
/// never an out-of-range index.
pub fn offset_of_line(line: usize, text: &str) -> usize {
    let mut offset = 0;
    let mut remaining = line;
    let mut units = 0;
    let mut chars = text.chars().peekable();
    while remaining > 0 {
        let Some(ch) = chars.next() else { break };
        units += ch.len_utf16();
        if ch == '\n' {
            remaining -= 1;
            offset = units;
        } else if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
                units += 1;
            }
            remaining -= 1;
            offset = units;
        }
    }
    offset
}
