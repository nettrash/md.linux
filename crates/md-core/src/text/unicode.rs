//! The Unicode properties md.win reads from .NET, answered from tables dumped from .NET itself
//! (`tools/unicode-tables/gen.cs` → `unicode_tables.rs`).
//!
//! Rust's `char` has no general category, and its `to_uppercase` / `to_lowercase` are the FULL
//! mappings (`ß` → `SS`). md.win asks `Rune.GetUnicodeCategory`, `Rune.ToUpperInvariant` and
//! `Rune.ToLowerInvariant` — the simple, one-scalar mappings — and the shared test vectors were
//! settled against those answers, so this module answers exactly what .NET answers. Never reach
//! for `char::is_alphabetic`, `char::is_whitespace` or `char::to_uppercase` in ported code: each
//! is a different set or mapping.

use super::unicode_tables::{CATEGORY_RANGES, SIMPLE_LOWER, SIMPLE_UPPER};

/// .NET's `System.Globalization.UnicodeCategory`, same order and values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GeneralCategory {
    UppercaseLetter = 0,
    LowercaseLetter = 1,
    TitlecaseLetter = 2,
    ModifierLetter = 3,
    OtherLetter = 4,
    NonSpacingMark = 5,
    SpacingCombiningMark = 6,
    EnclosingMark = 7,
    DecimalDigitNumber = 8,
    LetterNumber = 9,
    OtherNumber = 10,
    SpaceSeparator = 11,
    LineSeparator = 12,
    ParagraphSeparator = 13,
    Control = 14,
    Format = 15,
    Surrogate = 16,
    PrivateUse = 17,
    ConnectorPunctuation = 18,
    DashPunctuation = 19,
    OpenPunctuation = 20,
    ClosePunctuation = 21,
    InitialQuotePunctuation = 22,
    FinalQuotePunctuation = 23,
    OtherPunctuation = 24,
    MathSymbol = 25,
    CurrencySymbol = 26,
    ModifierSymbol = 27,
    OtherSymbol = 28,
    OtherNotAssigned = 29,
}

impl GeneralCategory {
    fn from_raw(raw: u8) -> Self {
        use GeneralCategory::*;
        const ALL: [GeneralCategory; 30] = [
            UppercaseLetter,
            LowercaseLetter,
            TitlecaseLetter,
            ModifierLetter,
            OtherLetter,
            NonSpacingMark,
            SpacingCombiningMark,
            EnclosingMark,
            DecimalDigitNumber,
            LetterNumber,
            OtherNumber,
            SpaceSeparator,
            LineSeparator,
            ParagraphSeparator,
            Control,
            Format,
            Surrogate,
            PrivateUse,
            ConnectorPunctuation,
            DashPunctuation,
            OpenPunctuation,
            ClosePunctuation,
            InitialQuotePunctuation,
            FinalQuotePunctuation,
            OtherPunctuation,
            MathSymbol,
            CurrencySymbol,
            ModifierSymbol,
            OtherSymbol,
            OtherNotAssigned,
        ];
        ALL.get(raw as usize).copied().unwrap_or(OtherNotAssigned)
    }
}

/// `Rune.GetUnicodeCategory` / `CharUnicodeInfo.GetUnicodeCategory` for a scalar.
pub fn category(c: char) -> GeneralCategory {
    let cp = c as u32;
    match CATEGORY_RANGES.binary_search_by(|&(start, end, _)| {
        if end < cp {
            std::cmp::Ordering::Less
        } else if start > cp {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(index) => GeneralCategory::from_raw(CATEGORY_RANGES[index].2),
        Err(_) => GeneralCategory::OtherNotAssigned,
    }
}

/// `Rune.IsLetter` / `char.IsLetter`: Lu, Ll, Lt, Lm, Lo.
pub fn is_letter(c: char) -> bool {
    (category(c) as u8) <= GeneralCategory::OtherLetter as u8
}

/// `Rune.IsNumber`: Nd, Nl, No.
pub fn is_number(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::DecimalDigitNumber | GeneralCategory::LetterNumber | GeneralCategory::OtherNumber
    )
}

/// `Rune.IsDigit` / `char.IsDigit`: Nd only.
pub fn is_digit(c: char) -> bool {
    category(c) == GeneralCategory::DecimalDigitNumber
}

/// `Rune.IsLetterOrDigit`: a letter or Nd.
pub fn is_letter_or_digit(c: char) -> bool {
    is_letter(c) || is_digit(c)
}

/// `Rune.IsLower`: Ll.
pub fn is_lower(c: char) -> bool {
    category(c) == GeneralCategory::LowercaseLetter
}

/// `Rune.IsUpper`: Lu.
pub fn is_upper(c: char) -> bool {
    category(c) == GeneralCategory::UppercaseLetter
}

/// Mn, Mc, Me.
pub fn is_mark(c: char) -> bool {
    matches!(
        category(c),
        GeneralCategory::NonSpacingMark | GeneralCategory::SpacingCombiningMark | GeneralCategory::EnclosingMark
    )
}

/// .NET's `char.IsWhiteSpace` / `Rune.IsWhiteSpace`: Zs, Zl, Zp, U+0009–U+000D and U+0085.
/// Only `NotePreview` asks this (it mirrors Swift's `Character.isWhitespace`); the parser uses
/// [`super::whitespace`] and must never use this.
pub fn is_dotnet_white_space(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000D}' | '\u{0085}')
        || matches!(
            category(c),
            GeneralCategory::SpaceSeparator | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator
        )
}

fn mapped(table: &[(u32, u32)], c: char) -> char {
    match table.binary_search_by_key(&(c as u32), |&(from, _)| from) {
        Ok(index) => char::from_u32(table[index].1).unwrap_or(c),
        Err(_) => c,
    }
}

/// `Rune.ToUpperInvariant`: the simple one-scalar mapping (`ß` stays `ß`).
pub fn to_upper_invariant(c: char) -> char {
    mapped(SIMPLE_UPPER, c)
}

/// `Rune.ToLowerInvariant`: the simple one-scalar mapping.
pub fn to_lower_invariant(c: char) -> char {
    mapped(SIMPLE_LOWER, c)
}

/// `string.ToUpperInvariant()`: [`to_upper_invariant`] applied to every scalar.
pub fn upper_invariant(s: &str) -> String {
    s.chars().map(to_upper_invariant).collect()
}

/// `string.ToLowerInvariant()`: [`to_lower_invariant`] applied to every scalar.
pub fn lower_invariant(s: &str) -> String {
    s.chars().map(to_lower_invariant).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_match_dotnet() {
        assert_eq!(category('A'), GeneralCategory::UppercaseLetter);
        assert_eq!(category('a'), GeneralCategory::LowercaseLetter);
        assert_eq!(category('ǅ'), GeneralCategory::TitlecaseLetter);
        assert_eq!(category('\u{0301}'), GeneralCategory::NonSpacingMark);
        assert_eq!(category('\u{20DD}'), GeneralCategory::EnclosingMark);
        assert_eq!(category('5'), GeneralCategory::DecimalDigitNumber);
        assert_eq!(category('Ⅻ'), GeneralCategory::LetterNumber);
        assert_eq!(category(' '), GeneralCategory::SpaceSeparator);
        assert_eq!(category('\u{200B}'), GeneralCategory::Format);
        assert_eq!(category('\u{E000}'), GeneralCategory::PrivateUse);
        assert_eq!(category('\u{0378}'), GeneralCategory::OtherNotAssigned);
        assert_eq!(category('Ⓐ'), GeneralCategory::OtherSymbol);
        assert_eq!(category('\u{10FFFF}'), GeneralCategory::OtherNotAssigned);
    }

    #[test]
    fn case_mappings_are_the_simple_ones() {
        assert_eq!(to_upper_invariant('ß'), 'ß');
        assert_eq!(to_upper_invariant('ǆ'), 'Ǆ');
        assert_eq!(to_upper_invariant('ǅ'), 'Ǆ');
        assert_eq!(to_upper_invariant('я'), 'Я');
        assert_eq!(to_lower_invariant('ẞ'), 'ß');
        assert_eq!(to_lower_invariant('İ'), 'İ');
        assert_eq!(upper_invariant("straße"), "STRAßE");
        assert_eq!(lower_invariant("ΟΔΟΣ"), "οδοσ");
    }

    #[test]
    fn predicates() {
        assert!(is_letter('ж') && is_letter('ǅ') && !is_letter('1'));
        assert!(is_number('½') && is_number('Ⅻ') && !is_digit('½'));
        assert!(is_letter_or_digit('٣'));
        assert!(is_mark('\u{0903}'));
        assert!(
            is_dotnet_white_space('\u{0085}')
                && is_dotnet_white_space('\u{2029}')
                && !is_dotnet_white_space('\u{200B}')
        );
    }
}
