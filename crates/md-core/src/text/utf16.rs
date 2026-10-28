//! UTF-16 offsets — the unit md.win, the shared test vectors and every other md port count in.
//!
//! The editing rules (smart typing, formatting) take and return selections in UTF-16 code units,
//! as `typing-vectors.json` and `formatting-vectors.json` spell them, and are ported on
//! [`Utf16Text`] so their index arithmetic reads line for line like the C#. GTK counts in
//! characters (Unicode scalars) and Rust strings in bytes; the converters here are the only
//! place those meet.

/// A string as UTF-16 code units, the way C# holds one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Utf16Text(pub Vec<u16>);

impl Utf16Text {
    pub fn new(s: &str) -> Self {
        Utf16Text(s.encode_utf16().collect())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    /// Back to a Rust string. Unpaired surrogates (only an edit that splits a pair can make one)
    /// become U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

impl From<&str> for Utf16Text {
    fn from(s: &str) -> Self {
        Utf16Text::new(s)
    }
}

/// The length of `s` in UTF-16 code units (`string.Length` in C#).
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// The UTF-16 offset of byte offset `byte` (which must be a char boundary of `s`).
pub fn utf16_from_byte(s: &str, byte: usize) -> usize {
    utf16_len(&s[..byte])
}

/// The byte offset of UTF-16 offset `unit`, or `None` when it falls inside a surrogate pair or
/// past the end.
pub fn byte_from_utf16(s: &str, unit: usize) -> Option<usize> {
    let mut units = 0;
    for (byte, c) in s.char_indices() {
        if units == unit {
            return Some(byte);
        }
        units += c.len_utf16();
        if units > unit {
            return None;
        }
    }
    (units == unit).then_some(s.len())
}

/// The UTF-16 offset of char (scalar) offset `chars` — GTK's `TextIter` offset to the vectors'.
pub fn utf16_from_char_offset(s: &str, chars: usize) -> usize {
    s.chars().take(chars).map(char::len_utf16).sum()
}

/// The char (scalar) offset of UTF-16 offset `unit`, or `None` inside a surrogate pair or past
/// the end.
pub fn char_offset_from_utf16(s: &str, unit: usize) -> Option<usize> {
    let mut units = 0;
    for (index, c) in s.chars().enumerate() {
        if units == unit {
            return Some(index);
        }
        units += c.len_utf16();
        if units > unit {
            return None;
        }
    }
    (units == unit).then(|| s.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_round_trip() {
        let s = "a😀жb";
        assert_eq!(utf16_len(s), 5);
        assert_eq!(utf16_from_byte(s, 5), 3);
        assert_eq!(byte_from_utf16(s, 3), Some(5));
        assert_eq!(byte_from_utf16(s, 2), None);
        assert_eq!(byte_from_utf16(s, 5), Some(s.len()));
        assert_eq!(byte_from_utf16(s, 6), None);
        assert_eq!(utf16_from_char_offset(s, 2), 3);
        assert_eq!(char_offset_from_utf16(s, 3), Some(2));
        assert_eq!(char_offset_from_utf16(s, 2), None);
        assert_eq!(char_offset_from_utf16(s, 5), Some(4));
        assert_eq!(Utf16Text::new(s).to_string_lossy(), s);
    }
}
