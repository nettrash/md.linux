//! The ordinal string operations every module that matches a delimiter inside author text goes
//! through, plus the code-point helpers the slug and the DOT header scan need. Port of md.win
//! `Md.Core.Text.ScalarText`.
//!
//! Rust's `str` methods are already ordinal (no culture, no ignorable characters), so most of
//! these are thin. What they keep are md.win's two contract differences from the standard
//! library, both relied upon by call sites: [`contains`] never finds an empty needle, and
//! [`first_index`] returns `None` for an empty or over-long needle.
//!
//! Offsets are UTF-8 **byte** offsets into the `&str`, always on a char boundary. md.win's are
//! UTF-16 code units; every delimiter the callers search for is ASCII, so the two only ever
//! differ in what number names the same place.

use super::unicode;

/// True when `needle` occurs in `text`. An empty needle is never found.
pub fn contains(text: &str, needle: &str) -> bool {
    !needle.is_empty() && text.contains(needle)
}

/// The first byte offset at or after `from` where `needle` begins, or `None` — also `None`
/// when the needle is empty or longer than the haystack. A `from` that is not a char boundary
/// is moved forward to the next one.
pub fn first_index(text: &str, needle: &str, from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > text.len() {
        return None;
    }
    let mut start = from.min(text.len());
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if start + needle.len() > text.len() {
        return None;
    }
    text[start..].find(needle).map(|i| start + i)
}

/// Exact prefix. An empty prefix is true.
pub fn has_prefix(text: &str, prefix: &str) -> bool {
    text.starts_with(prefix)
}

/// Exact suffix. An empty suffix is true.
pub fn has_suffix(text: &str, suffix: &str) -> bool {
    text.ends_with(suffix)
}

/// `text` cut at every occurrence of `separator` — Kotlin's `split(String)`, empty edge pieces
/// included; an empty separator yields the whole text.
pub fn split<'a>(text: &'a str, separator: &str) -> Vec<&'a str> {
    if separator.is_empty() { vec![text] } else { text.split(separator).collect() }
}

/// Every occurrence replaced, left to right, never rescanning what was written; an empty
/// target leaves the text alone.
pub fn replacing(text: &str, target: &str, replacement: &str) -> String {
    if target.is_empty() { text.to_string() } else { text.replace(target, replacement) }
}

/// `text` without its first `count` **code points**.
pub fn drop_first(text: &str, count: usize) -> &str {
    match text.char_indices().nth(count) {
        Some((offset, _)) => &text[offset..],
        None => "",
    }
}

/// Unicode general categories Mn, Mc, Me — the marks a slug keeps and a DOT name may carry.
pub fn is_mark(c: char) -> bool {
    unicode::is_mark(c)
}

/// The part of Unicode's `Alphabetic` property that is neither a letter, a mark nor a
/// letter-number: the circled and squared Latin letters, all general category `So`.
/// `Other_Alphabetic` minus `L`, `M` and `Nl`, from `DerivedCoreProperties.txt` — what closes the
/// slug's gap to Swift's `isAlphabetic` and TypeScript's `\p{Alphabetic}`.
pub fn is_enclosed_alphabetic(c: char) -> bool {
    matches!(c as u32, 0x24B6..=0x24E9 | 0x1F130..=0x1F149 | 0x1F150..=0x1F169 | 0x1F170..=0x1F189)
}

/// Swift's `String.lowercased()`: every scalar replaced by its own full lowercase mapping,
/// scalar by scalar, with no context. The simple mapping ([`unicode::to_lower_invariant`])
/// except U+0130 → `i` + U+0307; deliberately **no** `Final_Sigma` (`ΟΔΟΣ ΣΑΣ Σ` slugs to
/// `οδοσ-σασ-σ`, as on md.macOS, the source of truth).
pub fn full_lowercase(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        if c == '\u{0130}' {
            out.push('i');
            out.push('\u{0307}');
        } else {
            out.push(unicode::to_lower_invariant(c));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_needles_are_never_found() {
        assert!(!contains("abc", ""));
        assert!(contains("abc", "b"));
        assert_eq!(first_index("abc", "", 0), None);
        assert_eq!(first_index("ab", "abc", 0), None);
    }

    #[test]
    fn first_index_counts_bytes_from_a_boundary() {
        assert_eq!(first_index("жa|b|", "|", 0), Some(3));
        assert_eq!(first_index("жa|b|", "|", 4), Some(5));
        assert_eq!(first_index("жa|b|", "|", 1), Some(3));
        assert_eq!(first_index("a|", "|", 2), None);
        assert_eq!(first_index("a\u{200D}b", "ab", 0), None);
    }

    #[test]
    fn split_and_replace_keep_kotlins_contract() {
        assert_eq!(split("|a||b|", "|"), vec!["", "a", "", "b", ""]);
        assert_eq!(split("abc", ""), vec!["abc"]);
        assert_eq!(replacing("aaa", "aa", "a"), "aa");
        assert_eq!(replacing("abc", "", "x"), "abc");
    }

    #[test]
    fn drop_first_counts_code_points() {
        assert_eq!(drop_first("😀ab", 1), "ab");
        assert_eq!(drop_first("ab", 5), "");
        assert_eq!(drop_first("ab", 0), "ab");
    }

    #[test]
    fn full_lowercase_is_swifts() {
        assert_eq!(full_lowercase("İ"), "i\u{0307}");
        assert_eq!(full_lowercase("ΟΔΟΣ ΣΑΣ Σ"), "οδοσ σασ σ");
        assert_eq!(full_lowercase("ABC"), "abc");
    }

    #[test]
    fn enclosed_alphabetics() {
        assert!(is_enclosed_alphabetic('Ⓐ'));
        assert!(is_enclosed_alphabetic('\u{1F170}'));
        assert!(!is_enclosed_alphabetic('A'));
        assert!(is_mark('\u{20DD}'));
    }
}
