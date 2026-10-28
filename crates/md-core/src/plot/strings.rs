//! Small string helpers.
//!
//! Written out rather than reached for: the ports have different ideas of what `trim` and `split`
//! mean, and `str::trim` / `char::is_whitespace` (like .NET's `string.Trim()` /
//! `char.IsWhiteSpace`) strip far more than the grammar's space and tab.
//!
//! Every grammar character is ASCII, so the scans here walk bytes: a byte of a multi-byte UTF-8
//! sequence is ≥ 0x80 and never matches, exactly as a non-ASCII UTF-16 unit never matches in
//! md.win, and every offset they return sits next to an ASCII byte or at an end — always a char
//! boundary, so slicing at it cannot panic.

use crate::text::{unicode, whitespace};

/// The grammar's whitespace: space and tab, nothing else (whitespace's SPTAB set).
pub(super) fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

pub(super) fn is_digit(c: u8) -> bool {
    c.is_ascii_digit()
}

pub(super) fn is_lower_letter(c: u8) -> bool {
    c.is_ascii_lowercase()
}

pub(super) fn is_identifier_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

pub(super) fn is_identifier_part(c: u8) -> bool {
    is_identifier_start(c) || is_digit(c)
}

/// Neither NaN nor ±∞.
pub(super) fn is_finite_number(v: f64) -> bool {
    v.is_finite()
}

/// Trim space and tab from both ends — SPTAB, never the wide Unicode set.
pub(super) fn trim(text: &str) -> &str {
    whitespace::trim_space_tab(text)
}

/// The locale-independent lowercase (.NET's `ToLowerInvariant`), never a culture's: a Turkish
/// locale spells `AUTO` with a dotless ı and the directive stops matching. Only ASCII words are
/// ever compared with the result.
pub(super) fn lowercased(text: &str) -> String {
    unicode::lower_invariant(text)
}

/// [`lowercased`], one character at a time.
pub(super) fn lowercased_char(c: char) -> char {
    unicode::to_lower_invariant(c)
}

/// Lines, on CRLF, LF or CR — the parser's own set, not the wide Unicode one. Always appends the
/// trailing segment. The block scanner splits the same way, so this is its splitter.
pub(super) fn split_lines(source: &str) -> Vec<&str> {
    whitespace::normalized_lines(source)
}

/// The space-and-tab separated tokens of `text`, empty ones dropped.
pub(super) fn split_whitespace(text: &str) -> Vec<&str> {
    text.split([' ', '\t']).filter(|token| !token.is_empty()).collect()
}

/// Ordinal membership.
pub(super) fn contains(list: &[&str], value: &str) -> bool {
    list.contains(&value)
}

/// The index of the first `first``second` pair, or `None`.
pub(super) fn index_of_pair(text: &str, first: u8, second: u8) -> Option<usize> {
    text.as_bytes().windows(2).position(|pair| pair[0] == first && pair[1] == second)
}

/// The index of the first `needle`, or `None`.
pub(super) fn index_of(text: &str, needle: u8) -> Option<usize> {
    text.as_bytes().iter().position(|&c| c == needle)
}

/// The index of the `)` matching the `(` at `open`, or `None`.
pub(super) fn matching_parenthesis(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0i64;
    for (index, &c) in text.as_bytes().iter().enumerate().skip(open) {
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// The index of the first comma at parenthesis depth 0, or `None`.
pub(super) fn top_level_comma(text: &str) -> Option<usize> {
    let mut depth = 0i64;
    for (index, &c) in text.as_bytes().iter().enumerate() {
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
        } else if c == b',' && depth == 0 {
            return Some(index);
        }
    }
    None
}

/// Does `text` begin with `word` (ASCII) as a whole word?
pub(super) fn starts_with_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    if !bytes.starts_with(word.as_bytes()) {
        return false;
    }
    if bytes.len() == word.len() {
        return true;
    }
    !is_identifier_part(bytes[word.len()])
}

/// The characters of a label, for the legend gutter — code points, as the TypeScript reference
/// counts them (`for (const _ of text)`) and Swift's scalars do. A UTF-16 length would
/// double-count an emoji and a UTF-8 length would count it four times; either shifts the gutter
/// and changes the figure's bytes.
pub(super) fn count_characters(text: &str) -> usize {
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_splits_on_space_and_tab_only() {
        assert_eq!(trim("\t a b \t"), "a b");
        assert_eq!(trim("\u{00A0}T\u{00A0}"), "\u{00A0}T\u{00A0}");
        assert_eq!(split_whitespace(" 1,2\t3,4  \u{00A0}5,6 "), vec!["1,2", "3,4", "\u{00A0}5,6"]);
        assert_eq!(split_lines("a\r\nb\rc\nd\u{2028}e"), vec!["a", "b", "c", "d\u{2028}e"]);
    }

    #[test]
    fn scans_find_ascii_delimiters_only() {
        assert_eq!(index_of_pair("1...2", b'.', b'.'), Some(1));
        assert_eq!(index_of_pair("..", b'.', b'.'), Some(0));
        assert_eq!(index_of_pair(".", b'.', b'.'), None);
        assert_eq!(index_of("é,1", b','), Some(2));
        assert_eq!(matching_parenthesis("(a(b)c)d", 0), Some(6));
        assert_eq!(matching_parenthesis("(a", 0), None);
        assert_eq!(top_level_comma("f(a,b),c"), Some(6));
        assert!(starts_with_word("for t", "for"));
        assert!(starts_with_word("for", "for"));
        assert!(!starts_with_word("format", "for"));
        assert!(starts_with_word("for\u{00E9}", "for"));
        assert_eq!(count_characters("\u{1F600}a\u{0301}"), 3);
    }

    #[test]
    fn lowercase_is_invariant_and_keeps_the_dotted_capital_i() {
        assert_eq!(lowercased("AUTO"), "auto");
        assert_eq!(lowercased("\u{0130}"), "\u{0130}");
        assert_eq!(lowercased("\u{212A}"), "k");
    }
}
