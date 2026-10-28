//! Pure: `GtkTextIter` char offsets ↔ md-logic's UTF-16 offsets (shell-design §3.2).
//!
//! A `GtkTextIter` offset counts Unicode scalars; every md-core / md-logic editing API counts
//! UTF-16 code units (the shared vectors do). The two meet only here, over md-core
//! `text::utf16`, on the buffer's text read once per event. Every conversion clamps: an offset
//! past the end answers the end, and a UTF-16 offset that falls between the two halves of a
//! surrogate pair (which no md-logic answer produces, but a caller could compute) answers the
//! scalar that pair belongs to, never a panic.

use md_core::text::utf16::{char_offset_from_utf16, utf16_from_char_offset, utf16_len};

/// The UTF-16 offset of scalar offset `chars` in `text`; past the end → the text's UTF-16 length.
pub fn utf16_of_char(text: &str, chars: usize) -> usize {
    utf16_from_char_offset(text, chars)
}

/// The scalar offset of UTF-16 offset `units` in `text`; past the end → the scalar count, inside a
/// surrogate pair → the pair's own scalar (rounded down).
pub fn char_of_utf16(text: &str, units: usize) -> usize {
    if let Some(chars) = char_offset_from_utf16(text, units) {
        return chars;
    }
    let mut seen = 0;
    for (index, c) in text.chars().enumerate() {
        let next = seen + c.len_utf16();
        if next > units {
            return index;
        }
        seen = next;
    }
    text.chars().count()
}

/// A selection given by the buffer's two marks (`insert`, `selection_bound`, in scalars, either
/// order) as md-logic wants it: `(start, length)` in UTF-16 units.
pub fn selection_utf16(text: &str, insert: usize, bound: usize) -> (usize, usize) {
    let (first, last) = if insert <= bound { (insert, bound) } else { (bound, insert) };
    let start = utf16_of_char(text, first);
    let end = utf16_of_char(text, last);
    (start, end - start)
}

/// A UTF-16 range `[start, start + length)` as scalar offsets `(start, end)`, clamped into the
/// text the way md.win's `Math.Clamp` pair does: the start first, then what is left of the length.
pub fn range_chars(text: &str, start: usize, length: usize) -> (usize, usize) {
    let total = utf16_len(text);
    let start = start.min(total);
    let end = start + length.min(total - start);
    (char_of_utf16(text, start), char_of_utf16(text, end))
}

/// Clamps a UTF-16 range into `text`: `(start, length)`.
pub fn clamp_range(text: &str, start: usize, length: usize) -> (usize, usize) {
    let total = utf16_len(text);
    let start = start.min(total);
    (start, length.min(total - start))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every vector below is (text, char offset, UTF-16 offset).
    const ROUND_TRIPS: &[(&str, usize, usize)] = &[
        ("", 0, 0),
        ("plain", 0, 0),
        ("plain", 3, 3),
        ("plain", 5, 5),
        ("Привет", 2, 2),
        ("Привет", 6, 6),
        // A combining mark is its own scalar (and its own unit): é as e + U+0301.
        ("e\u{301}x", 1, 1),
        ("e\u{301}x", 2, 2),
        ("e\u{301}x", 3, 3),
        // CJK is BMP: one unit per scalar.
        ("北京大学", 2, 2),
        // An emoji is a surrogate pair: one scalar, two units.
        ("a😀b", 1, 1),
        ("a😀b", 2, 3),
        ("a😀b", 3, 4),
        // A ZWJ sequence is several scalars, each pair two units.
        ("👩\u{200D}💻!", 1, 2),
        ("👩\u{200D}💻!", 2, 3),
        ("👩\u{200D}💻!", 3, 5),
        ("👩\u{200D}💻!", 4, 6),
    ];

    #[test]
    fn chars_and_utf16_round_trip() {
        for &(text, chars, units) in ROUND_TRIPS {
            assert_eq!(utf16_of_char(text, chars), units, "{text:?} char {chars}");
            assert_eq!(char_of_utf16(text, units), chars, "{text:?} unit {units}");
        }
    }

    #[test]
    fn out_of_range_is_clamped_to_the_end() {
        assert_eq!(utf16_of_char("a😀b", 99), 4);
        assert_eq!(char_of_utf16("a😀b", 99), 3);
        assert_eq!(utf16_of_char("", 5), 0);
        assert_eq!(char_of_utf16("", 5), 0);
    }

    #[test]
    fn inside_a_surrogate_pair_rounds_down_to_the_pair() {
        assert_eq!(char_of_utf16("a😀b", 2), 1);
        assert_eq!(char_of_utf16("😀", 1), 0);
    }

    #[test]
    fn selections_are_ordered_and_measured_in_units() {
        assert_eq!(selection_utf16("a😀b", 1, 3), (1, 3));
        assert_eq!(selection_utf16("a😀b", 3, 1), (1, 3));
        assert_eq!(selection_utf16("a😀b", 2, 2), (3, 0));
        assert_eq!(selection_utf16("Привет", 0, 6), (0, 6));
        assert_eq!(selection_utf16("ab", 9, 0), (0, 2));
    }

    #[test]
    fn ranges_convert_and_clamp() {
        assert_eq!(range_chars("a😀b", 1, 2), (1, 2));
        assert_eq!(range_chars("a😀b", 0, 4), (0, 3));
        assert_eq!(range_chars("a😀b", 3, 99), (2, 3));
        assert_eq!(range_chars("a😀b", 99, 1), (3, 3));
        assert_eq!(range_chars("e\u{301}x", 1, 1), (1, 2));
        assert_eq!(clamp_range("abc", 2, 5), (2, 1));
        assert_eq!(clamp_range("abc", 7, 5), (3, 0));
    }
}
