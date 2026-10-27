//! The whitespace and newline sets the parser and the writers trim with, spelled out.
//!
//! Five sets, never conflated, because every port that conflated two of them shipped a
//! divergence: the same document was two paragraphs on one platform and one on another.
//!
//! - **WS** — Foundation's `CharacterSet.whitespaces`: Unicode `Zs` ∪ U+0009 as Apple's frozen
//!   tables have it, which still puts **U+200B ZERO WIDTH SPACE** in `Zs` (current Unicode says
//!   `Cf`). U+180E is *not* in the set, and no line terminator is. Decides blank lines, fence info
//!   strings, closing fences, front-matter fences, footnote-definition and heading trims, table
//!   cells.
//! - **WSNL** — `.whitespacesAndNewlines`: WS plus the seven line terminators. Used only by the
//!   note-body trim and the raw-diagram probes. U+0085 NEL is whitespace here and content
//!   everywhere else.
//! - **SP** — U+0020 alone: every indentation drop in the block grammar.
//! - **SPTAB** — U+0020 and U+0009: thematic-break filtering, the closing-hash check and the CSV
//!   alignment cell — the *opposite* decision from the blank-line rule, on purpose, so that
//!   `U+200B 1 U+200B` is not a number on any platform.
//! - **NL** — Foundation's `.newlines`: the wider splitter the raw `.puml` / `.gv` probes use. The
//!   block scanner never uses it.
//!
//! None of this may be replaced by `char::is_whitespace`, `str::trim` or a regex `\s`: each is a
//! different set. Every member is a BMP scalar, so a `char` walk here agrees with md.win's UTF-16
//! code-unit walk. Port of md.win `Md.Core.Text.Whitespace`.

/// WS — Foundation's `.whitespaces`, nineteen scalars, U+200B included.
pub fn is_whitespace(c: char) -> bool {
    matches!(c, '\t' | ' ' | '\u{00A0}' | '\u{1680}' | '\u{2000}'..='\u{200B}' | '\u{202F}' | '\u{205F}' | '\u{3000}')
}

/// NL — Foundation's `.newlines`. CRLF is handled by the splitter, not here.
pub fn is_newline(c: char) -> bool {
    matches!(c, '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

/// WSNL — Foundation's `.whitespacesAndNewlines`, twenty-six scalars.
pub fn is_whitespace_or_newline(c: char) -> bool {
    is_whitespace(c) || is_newline(c)
}

/// `trimmingCharacters(in: .whitespaces)`. Trims scalars, as Foundation does:
/// `" \u{0301}abc"` loses the space even though space-plus-mark is one grapheme.
pub fn trim_ws(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)` — the note body and nothing else in the parser.
pub fn trim_wsnl(s: &str) -> &str {
    s.trim_matches(is_whitespace_or_newline)
}

pub fn trim_leading_ws(s: &str) -> &str {
    s.trim_start_matches(is_whitespace)
}

pub fn trim_trailing_ws(s: &str) -> &str {
    s.trim_end_matches(is_whitespace)
}

/// SPTAB trim — `CharacterSet(charactersIn: " \t")`. The CSV alignment cell only.
pub fn trim_space_tab(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}

/// SP — drop leading U+0020 only; a tab is content.
pub fn drop_leading_spaces(s: &str) -> &str {
    s.trim_start_matches(' ')
}

/// SPTAB — drop leading U+0020 / U+0009 only, not the whitespace set (the DOT header scan).
pub fn drop_space_tab(s: &str) -> &str {
    s.trim_start_matches([' ', '\t'])
}

/// The block scanner's line splitter: `\r`, `\n` and `\r\n` each end exactly one line; VT, FF,
/// NEL, LS and PS are content. The final segment is always appended, so `"a\n"` is `["a", ""]`
/// and `""` is `[""]` — observable through an unclosed fence at EOF, which gains a trailing
/// newline (a wart every port replicates).
///
/// Not `split('\n')` plus trimming `\r`: a lone CR would not split and a line ending in CR would
/// be altered. `parse`, `outline` and `notes` all use this so their line numbers agree.
pub fn normalized_lines(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                lines.push(&source[start..i]);
                if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            b'\n' => {
                lines.push(&source[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    lines.push(&source[start..]);
    lines
}

/// Foundation's `components(separatedBy: .newlines)`, CRLF as one terminator — the wider splitter
/// the raw-diagram probes use (U+0085 splits a line here and nowhere else). Kept separate from
/// [`normalized_lines`] on purpose; merging them changes which documents are diagrams.
pub fn newline_set_lines(source: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !is_newline(c) {
            continue;
        }
        lines.push(&source[start..i]);
        let mut next = i + c.len_utf8();
        if c == '\r'
            && let Some(&(j, '\n')) = chars.peek()
        {
            chars.next();
            next = j + 1;
        }
        start = next;
    }
    lines.push(&source[start..]);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whitespace_set_is_foundations_frozen_table() {
        let members: Vec<char> = (0..=0xFFFFu32).filter_map(char::from_u32).filter(|&c| is_whitespace(c)).collect();
        assert_eq!(members.len(), 19);
        assert!(is_whitespace('\u{200B}'));
        assert!(!is_whitespace('\u{180E}'));
        assert!(!is_whitespace('\n'));
        assert!(!is_whitespace('\u{0085}'));
    }

    #[test]
    fn the_whitespace_and_newline_set_has_twenty_six_members() {
        let count = (0..=0xFFFFu32).filter_map(char::from_u32).filter(|&c| is_whitespace_or_newline(c)).count();
        assert_eq!(count, 26);
    }

    #[test]
    fn trims_use_their_own_sets() {
        assert_eq!(trim_ws("\u{200B} a \t"), "a");
        assert_eq!(trim_ws("\na\n"), "\na\n");
        assert_eq!(trim_wsnl("\u{0085} a \n"), "a");
        assert_eq!(trim_space_tab("\u{200B}1\u{200B}"), "\u{200B}1\u{200B}");
        assert_eq!(drop_leading_spaces("  \tx"), "\tx");
        assert_eq!(drop_space_tab(" \t x"), "x");
        assert_eq!(trim_leading_ws(" a "), "a ");
        assert_eq!(trim_trailing_ws(" a "), " a");
        assert_eq!(trim_ws(" \u{0301}abc"), "\u{0301}abc");
    }

    #[test]
    fn normalized_lines_split_on_cr_lf_and_crlf_only() {
        assert_eq!(normalized_lines(""), vec![""]);
        assert_eq!(normalized_lines("a\n"), vec!["a", ""]);
        assert_eq!(normalized_lines("a\r\nb\rc\nd"), vec!["a", "b", "c", "d"]);
        assert_eq!(normalized_lines("a\u{0085}b\u{2028}c"), vec!["a\u{0085}b\u{2028}c"]);
        assert_eq!(normalized_lines("\r\r\n"), vec!["", "", ""]);
    }

    #[test]
    fn newline_set_lines_split_on_every_terminator() {
        assert_eq!(newline_set_lines("a\u{0085}b\u{2028}c\r\nd\u{000B}e"), vec!["a", "b", "c", "d", "e"]);
        assert_eq!(newline_set_lines("x\r"), vec!["x", ""]);
        assert_eq!(newline_set_lines(""), vec![""]);
    }
}
