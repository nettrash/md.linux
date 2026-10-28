//! Extended grapheme clusters (UAX #29) — what md.win walks with `StringInfo` and Swift counts
//! as `String.count`: the footer's character count, the note preview's cut, the example-name
//! and slug probes.

use unicode_segmentation::UnicodeSegmentation;

/// The number of extended grapheme clusters (`StringInfo.LengthInTextElements`).
pub fn count(s: &str) -> usize {
    s.graphemes(true).count()
}

/// The byte length of the grapheme cluster that starts at the beginning of `s`
/// (`StringInfo.GetNextTextElementLength`, in bytes); 0 for an empty string.
pub fn next_len(s: &str) -> usize {
    s.graphemes(true).next().map_or(0, str::len)
}

/// The first `n` clusters of `s` (`StringInfo.SubstringByTextElements(0, n)`).
pub fn prefix(s: &str, n: usize) -> &str {
    match s.grapheme_indices(true).nth(n) {
        Some((byte, _)) => &s[..byte],
        None => s,
    }
}

/// The clusters of `s`, in order.
pub fn clusters(s: &str) -> impl Iterator<Item = &str> {
    s.graphemes(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_follow_uax29() {
        assert_eq!(count(""), 0);
        assert_eq!(count("e\u{0301}x"), 2);
        assert_eq!(count("👨\u{200D}👩\u{200D}👧"), 1);
        assert_eq!(count("\r\n"), 1);
        assert_eq!(next_len("e\u{0301}x"), 3);
        assert_eq!(prefix("abc", 2), "ab");
        assert_eq!(prefix("ab", 5), "ab");
    }
}
