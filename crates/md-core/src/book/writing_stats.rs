//! The author-facing counters in the book footer ("N words · M characters") (md.win
//! `Book/WritingStats.cs`, Swift `WritingStats.words(in:)` and `DerivedText.characters` in md.macOS
//! BookWorkspace.swift).
//!
//! Swift counts with Foundation's `enumerateSubstrings(.byWords)`, Kotlin with
//! `BreakIterator.getWordInstance()` counting only segments that contain a letter or digit — both
//! UAX #29 word segmentation with ICU behind it. md-core takes no ICU, so the segmentation is
//! implemented here from the UAX #29 rules (WB1–WB16, WB999) over a property table built from
//! .NET's Unicode categories ([`unicode::category`], the same tables md.win reads) plus the
//! code-point lists the table needs (MidLetter, MidNum, MidNumLet, Katakana, Hebrew_Letter,
//! Ideographic, Hiragana, Regional_Indicator).
//!
//! Both siblings run ICU's rules, not the bare UAX #29 defaults, and ICU's root tailorings (CLDR
//! segments/root.xml) are reproduced where a 490-line differential against Foundation's
//! `enumerateSubstrings(.byWords)` showed them: the colon family (U+003A, U+FE55, U+FF1A) is *not*
//! MidLetter ("a:b" is two words; only Swedish and Finnish keep it), wide decimal digits
//! (U+FF10–FF19) *are* Numeric ("１２３" is one word), and a segment counts as a word when it
//! carries a letter, a digit, a letter-number (Roman numerals) or an alphabetic symbol (circled or
//! squared Latin letters) — ICU's word tokens, wider than Kotlin's `isLetterOrDigit` filter, which
//! misses "Ⅻ".
//!
//! Known limits against the siblings, in order of how likely a writer meets them: no dictionary
//! segmentation — a CJK ideograph or a Hiragana syllable is one segment each (so counts one word
//! each, roughly the CJK convention of counting characters, but higher than ICU's dictionary words:
//! "北京" is 1 on macOS, 2 here); scripts ICU hands to a dictionary (Thai, Lao, Khmer, Myanmar, the
//! Tai scripts and their neighbours — Line_Break=SA) form their own class here, joining only with
//! themselves like ICU's dictionary characters — so "ก1" is two words as on macOS but an unspaced
//! Thai run counts as one word where ICU's dictionary would find several; the
//! Extended_Pictographic set behind WB3c is a coarse range (it only shapes emoji sequences, which
//! never count as words); Foundation's Japanese dictionary path reports a bare connector or space
//! between Katakana runs as a word token ("カ_カ" and "カタ カナ" count 3 on macOS) — the segments
//! match here, the connector is not counted; and property lists follow Unicode 15 — characters
//! added since are classified by category alone.

use crate::text::graphemes;
use crate::text::unicode::{self, GeneralCategory};

/// Locale-independent UAX #29 word count: segments carrying a letter, digit, letter-number or
/// alphabetic symbol.
pub fn words(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    segments(text).into_iter().filter(|&(start, end)| is_word_segment(&text[start..end])).count()
}

/// Swift's `text.count`: extended grapheme clusters, not UTF-16 units or scalars — "é" is 1, a
/// family emoji is 1, "\r\n" is 1. (The Android footer shows `length` and already disagrees with
/// macOS; this port follows the source of truth.)
pub fn characters(text: &str) -> usize {
    graphemes::count(text)
}

/// UAX #29 word segments as byte ranges (on char boundaries), covering the whole text.
pub fn segments(text: &str) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    if text.is_empty() {
        return segments;
    }
    let mut starts = Vec::with_capacity(text.len());
    let mut props = Vec::with_capacity(text.len());
    let mut pictographic = Vec::with_capacity(text.len());
    // A Rust string holds no lone surrogate, so md.win's "breaks like any other symbol" branch for
    // one cannot arise.
    for (index, c) in text.char_indices() {
        starts.push(index);
        props.push(classify(c));
        pictographic.push(is_extended_pictographic(c as u32));
    }
    let mut segment_start = 0;
    for i in 1..props.len() {
        if is_boundary(&props, &pictographic, i) {
            segments.push((starts[segment_start], starts[i]));
            segment_start = i;
        }
    }
    segments.push((starts[segment_start], text.len()));
    segments
}

/// Whether a segment is a word token in ICU's sense: it carries a letter or digit (as Kotlin's
/// `isLetterOrDigit`), a letter-number such as a Roman numeral ("Ⅻ" is one word on macOS, none on
/// Android), or one of the alphabetic symbols — circled, parenthesized and squared Latin letters
/// ("🄰", "ⓐ") — that Foundation counts. Circled digits ("①"), fractions ("½") and superscripts are
/// not words.
fn is_word_segment(segment: &str) -> bool {
    segment.chars().any(|c| {
        unicode::is_letter_or_digit(c)
            || unicode::category(c) == GeneralCategory::LetterNumber
            || is_alphabetic_symbol(c as u32)
    })
}

/// Other_Alphabetic symbols: circled Latin letters, parenthesized and squared Latin letters.
fn is_alphabetic_symbol(v: u32) -> bool {
    matches!(v, 0x24B6..=0x24E9 | 0x1F110..=0x1F129 | 0x1F130..=0x1F149 | 0x1F150..=0x1F169)
}

// MARK: Word_Break property values

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wb {
    Other,
    Cr,
    Lf,
    Newline,
    Extend,
    Zwj,
    RegionalIndicator,
    Format,
    Katakana,
    HebrewLetter,
    ALetter,
    SingleQuote,
    DoubleQuote,
    MidNumLet,
    MidLetter,
    MidNum,
    Numeric,
    ExtendNumLet,
    WSegSpace,
    /// Line_Break=SA letters (Thai, Lao, Khmer, Myanmar, the Tai scripts): ICU's dictionary
    /// characters, which join only each other.
    Complex,
}

fn classify(c: char) -> Wb {
    let v = c as u32;
    match v {
        0x0D => return Wb::Cr,
        0x0A => return Wb::Lf,
        0x0B | 0x0C | 0x85 | 0x2028 | 0x2029 => return Wb::Newline,
        0x200D => return Wb::Zwj,
        0x200C => return Wb::Extend,
        0x200B => return Wb::Other, // ZWSP: Cf, but excluded from Format by the table
        0x27 => return Wb::SingleQuote,
        0x22 => return Wb::DoubleQuote,
        0x2E | 0x2018 | 0x2019 | 0x2024 | 0xFE52 | 0xFF07 | 0xFF0E => return Wb::MidNumLet,
        // ICU's root rules subtract the colon family (U+003A, U+FE55, U+FF1A) from MidLetter —
        // only the Swedish and Finnish tailorings keep it — so "a:b" is two words on macOS and
        // Android; the colons fall through to Other.
        0xB7 | 0x387 | 0x55F | 0x5F4 | 0x2027 | 0xFE13 => return Wb::MidLetter,
        0x2C | 0x3B | 0x37E | 0x589 | 0x60C | 0x60D | 0x66C | 0x7F8 | 0x2044 | 0xFE10 | 0xFE14 | 0xFE50 | 0xFE54
        | 0xFF0C | 0xFF1B => return Wb::MidNum,
        0x66B => return Wb::Numeric,       // Arabic decimal separator (Line_Break=NU)
        0x202F => return Wb::ExtendNumLet, // narrow no-break space
        0x5F3 => return Wb::ALetter,       // Hebrew geresh
        _ => {}
    }
    if (0x1F1E6..=0x1F1FF).contains(&v) {
        return Wb::RegionalIndicator;
    }
    if (0xFF9E..=0xFF9F).contains(&v) || (0x1F3FB..=0x1F3FF).contains(&v) {
        return Wb::Extend;
    }
    if matches!(v, 0x2C2..=0x2C5 | 0x2D2..=0x2D7 | 0x2DE | 0x2DF | 0x2E5..=0x2EB | 0x2ED | 0x2EF..=0x2FF | 0x55A..=0x55C | 0x55E)
    {
        return Wb::ALetter; // modifier symbols and Armenian marks the table lists as ALetter
    }
    let category = unicode::category(c);
    match category {
        GeneralCategory::NonSpacingMark | GeneralCategory::EnclosingMark | GeneralCategory::SpacingCombiningMark => {
            return Wb::Extend;
        }
        GeneralCategory::Format => return Wb::Format,
        // UAX #29 leaves the fullwidth digits out of Numeric (Line_Break=ID); ICU's root rules add
        // every wide decimal digit back, so "１２３" is one word.
        GeneralCategory::DecimalDigitNumber => return Wb::Numeric,
        GeneralCategory::ConnectorPunctuation => return Wb::ExtendNumLet,
        // No-break spaces are Glue, not WSegSpace.
        GeneralCategory::SpaceSeparator => return if v == 0xA0 || v == 0x2007 { Wb::Other } else { Wb::WSegSpace },
        _ => {}
    }
    if is_katakana(v) {
        return Wb::Katakana;
    }
    if is_hebrew_letter(v) {
        return Wb::HebrewLetter;
    }
    if matches!(
        category,
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::LetterNumber
    ) {
        if is_ideographic(v) || is_hiragana(v) {
            return Wb::Other;
        }
        return if is_complex_context(v) { Wb::Complex } else { Wb::ALetter };
    }
    Wb::Other
}

/// Line_Break=SA: the South-East Asian scripts ICU segments with a dictionary (letters only; their
/// marks are Extend).
fn is_complex_context(v: u32) -> bool {
    matches!(
        v,
        0xE01..=0xE3A | 0xE40..=0xE4E                       // Thai
            | 0xE81..=0xEDF                                 // Lao
            | 0x1000..=0x109F | 0xA9E0..=0xA9FE | 0xAA60..=0xAA7F // Myanmar
            | 0x1780..=0x17D3 | 0x17D7 | 0x17DC | 0x17DD    // Khmer
            | 0x1950..=0x1974                               // Tai Le
            | 0x1980..=0x19DF                               // New Tai Lue
            | 0x1A00..=0x1A1E                               // Buginese
            | 0x1A20..=0x1AAD                               // Tai Tham
            | 0x1B00..=0x1B7C                               // Balinese
            | 0xA980..=0xA9DF                               // Javanese
            | 0xAA00..=0xAA5F                               // Cham
            | 0xAA80..=0xAADF // Tai Viet
    )
}

fn is_katakana(v: u32) -> bool {
    matches!(
        v,
        0x3031..=0x3035
            | 0x309B
            | 0x309C
            | 0x30A0..=0x30FA
            | 0x30FC..=0x30FF
            | 0x31F0..=0x31FF
            | 0x32D0..=0x32FE
            | 0x3300..=0x3357
            | 0xFF66..=0xFF9D
            | 0x1AFF0..=0x1AFF3
            | 0x1AFF5..=0x1AFFB
            | 0x1AFFD
            | 0x1AFFE
            | 0x1B000
            | 0x1B120..=0x1B122
            | 0x1B155
            | 0x1B164..=0x1B167
    )
}

fn is_hiragana(v: u32) -> bool {
    matches!(
        v,
        0x3041..=0x3096
            | 0x309D..=0x309F
            | 0x1B001
            | 0x1B11F
            | 0x1B132
            | 0x1B150
            | 0x1B151
            | 0x1B152
            | 0x1B002..=0x1B11E
            | 0x1F200
    )
}

fn is_hebrew_letter(v: u32) -> bool {
    matches!(
        v,
        0x5D0..=0x5EA
            | 0x5EF..=0x5F2
            | 0xFB1D
            | 0xFB1F..=0xFB28
            | 0xFB2A..=0xFB36
            | 0xFB38..=0xFB3C
            | 0xFB3E
            | 0xFB40
            | 0xFB41
            | 0xFB43
            | 0xFB44
            | 0xFB46..=0xFB4F
    )
}

fn is_ideographic(v: u32) -> bool {
    matches!(
        v,
        0x3006
            | 0x3007
            | 0x3021..=0x3029
            | 0x3038..=0x303A
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFA6D
            | 0xFA70..=0xFAD9
            | 0x16FE4
            | 0x17000..=0x187F7
            | 0x18800..=0x18CD5
            | 0x18D00..=0x18D08
            | 0x1B170..=0x1B2FB
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2EBE0
            | 0x2F800..=0x2FA1D
            | 0x30000..=0x3134A
            | 0x31350..=0x323AF
    )
}

fn is_extended_pictographic(v: u32) -> bool {
    matches!(
        v,
        0xA9 | 0xAE
            | 0x203C
            | 0x2049
            | 0x2122
            | 0x2139
            | 0x2194..=0x2199
            | 0x21A9
            | 0x21AA
            | 0x231A
            | 0x231B
            | 0x2328
            | 0x23CF
            | 0x23E9..=0x23F3
            | 0x23F8..=0x23FA
            | 0x24C2
            | 0x25AA
            | 0x25AB
            | 0x25B6
            | 0x25C0
            | 0x25FB..=0x25FE
            | 0x2600..=0x27BF
            | 0x2934
            | 0x2935
            | 0x2B05..=0x2B07
            | 0x2B1B
            | 0x2B1C
            | 0x2B50
            | 0x2B55
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
            | 0x1F000..=0x1FAFF
            | 0x1FC00..=0x1FFFD
    )
}

// MARK: Boundary rules

fn is_ignorable(p: Wb) -> bool {
    matches!(p, Wb::Extend | Wb::Format | Wb::Zwj)
}

fn is_ah_letter(p: Wb) -> bool {
    matches!(p, Wb::ALetter | Wb::HebrewLetter)
}

fn is_mid_num_let_q(p: Wb) -> bool {
    matches!(p, Wb::MidNumLet | Wb::SingleQuote)
}

fn is_newline(p: Wb) -> bool {
    matches!(p, Wb::Newline | Wb::Cr | Wb::Lf)
}

/// Whether a word boundary falls before scalar `i`.
fn is_boundary(p: &[Wb], pictographic: &[bool], i: usize) -> bool {
    let prev = p[i - 1];
    let cur = p[i];
    if prev == Wb::Cr && cur == Wb::Lf {
        return false; // WB3
    }
    if is_newline(prev) {
        return true; // WB3a
    }
    if is_newline(cur) {
        return true; // WB3b
    }
    if prev == Wb::Zwj && pictographic[i] {
        return false; // WB3c
    }
    if prev == Wb::WSegSpace && cur == Wb::WSegSpace {
        return false; // WB3d
    }
    if is_ignorable(cur) {
        return false; // WB4: X (Extend | Format | ZWJ)* → X
    }

    // WB4 lookback: the last scalar that is not glued onto its predecessor.
    let Some(left) = (0..i).rev().find(|&k| !is_ignorable(p[k])) else {
        return true; // only glue before us → WB999
    };
    let l = p[left];

    if is_ah_letter(l) && is_ah_letter(cur) {
        return false; // WB5
    }
    if is_ah_letter(l) && (cur == Wb::MidLetter || is_mid_num_let_q(cur)) && is_ah_letter(next_real(p, i)) {
        return false; // WB6
    }
    if (l == Wb::MidLetter || is_mid_num_let_q(l)) && is_ah_letter(cur) && is_ah_letter(prev_real(p, left)) {
        return false; // WB7
    }
    if l == Wb::HebrewLetter && cur == Wb::SingleQuote {
        return false; // WB7a
    }
    if l == Wb::HebrewLetter && cur == Wb::DoubleQuote && next_real(p, i) == Wb::HebrewLetter {
        return false; // WB7b
    }
    if l == Wb::DoubleQuote && cur == Wb::HebrewLetter && prev_real(p, left) == Wb::HebrewLetter {
        return false; // WB7c
    }
    if l == Wb::Numeric && cur == Wb::Numeric {
        return false; // WB8
    }
    if is_ah_letter(l) && cur == Wb::Numeric {
        return false; // WB9
    }
    if l == Wb::Numeric && is_ah_letter(cur) {
        return false; // WB10
    }
    if (l == Wb::MidNum || is_mid_num_let_q(l)) && cur == Wb::Numeric && prev_real(p, left) == Wb::Numeric {
        return false; // WB11
    }
    if l == Wb::Numeric && (cur == Wb::MidNum || is_mid_num_let_q(cur)) && next_real(p, i) == Wb::Numeric {
        return false; // WB12
    }
    if l == Wb::Katakana && cur == Wb::Katakana {
        return false; // WB13
    }
    if l == Wb::Complex && cur == Wb::Complex {
        return false; // ICU: dictionary run
    }
    // WB13a / WB13b without Katakana: ICU segments Katakana with its Japanese dictionary, outside
    // the connector rules, so "カ_カ" is three tokens on macOS.
    if (is_ah_letter(l) || matches!(l, Wb::Numeric | Wb::ExtendNumLet)) && cur == Wb::ExtendNumLet {
        return false; // WB13a
    }
    if l == Wb::ExtendNumLet && (is_ah_letter(cur) || cur == Wb::Numeric) {
        return false; // WB13b
    }
    if l == Wb::RegionalIndicator && cur == Wb::RegionalIndicator {
        // WB15 / WB16
        let run =
            p[..=left].iter().rev().filter(|&&k| !is_ignorable(k)).take_while(|&&k| k == Wb::RegionalIndicator).count();
        if run % 2 == 1 {
            return false;
        }
    }
    true // WB999
}

/// The property of the first non-glue scalar after `i`, or Other at the end.
fn next_real(p: &[Wb], i: usize) -> Wb {
    p[i + 1..].iter().copied().find(|&k| !is_ignorable(k)).unwrap_or(Wb::Other)
}

/// The property of the last non-glue scalar before `i`, or Other at the start.
fn prev_real(p: &[Wb], i: usize) -> Wb {
    p[..i].iter().rev().copied().find(|&k| !is_ignorable(k)).unwrap_or(Wb::Other)
}
