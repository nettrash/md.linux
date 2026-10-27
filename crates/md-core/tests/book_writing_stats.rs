//! Port of md.win `WritingStatsTests`: the footer counters (mdTests:
//! testWordCountIsLocaleAwareNotAWhitespaceSplit, the same five vectors Android's WritingStatsTest
//! pins) plus the UAX #29 rules the port implements by hand and the limits its header documents.
//! Invisible code points are spelled as escapes so the expectations stay reviewable.

use md_core::book::writing_stats::{characters, segments, words};

const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";

#[test]
fn word_count_is_locale_aware_not_a_whitespace_split() {
    assert_eq!(0, words(""));
    assert_eq!(0, words("   \n\n"));
    assert_eq!(2, words("Hello, world!"));
    // An apostrophe joins a word; a dash alone is none.
    assert_eq!(2, words("it's — done"));
    assert_eq!(3, words("One\ntwo\n\nthree"));
}

#[test]
fn segments_cover_the_text_exactly() {
    let text = "Hello, world!";
    let pieces: Vec<&str> = segments(text).into_iter().map(|(start, end)| &text[start..end]).collect();
    assert_eq!(vec!["Hello", ",", " ", "world", "!"], pieces);
    assert_eq!(text, pieces.concat());
}

#[test]
fn in_word_punctuation_follows_uax29() {
    assert_eq!(1, words("3.14")); // WB11 / WB12
    assert_eq!(1, words("1,000,000"));
    assert_eq!(1, words("a.b")); // WB6 / WB7
    assert_eq!(1, words("U.S.A."));
    assert_eq!(1, words("don\u{2019}t")); // curly apostrophe is MidNumLet
    assert_eq!(1, words("x_y")); // WB13a / WB13b
    assert_eq!(1, words("snake_case_name"));
    assert_eq!(2, words("e-mail")); // a hyphen breaks
    assert_eq!(1, words("$100"));
    assert_eq!(2, words("re: subject"));
}

#[test]
fn combining_marks_stay_inside_the_word() {
    assert_eq!(1, words("e\u{0301}tat"));
    assert_eq!(1, words("état"));
    assert_eq!(1, words("Zoë"));
    assert_eq!(2, words("Ко\u{0301}фе чай"));
    assert_eq!(6, words("Привет, мир! Как дела у тебя?"));
}

#[test]
fn symbols_and_emoji_are_not_words() {
    assert_eq!(0, words("\u{1F600} \u{1F44D}"));
    assert_eq!(0, words("— – … # * ~"));
    assert_eq!(1, words("hello \u{1F600}"));
    // ZWJ emoji sequences and flags hold together as one non-word segment.
    assert_eq!(1, segments(FAMILY).len());
    assert_eq!(1, segments("\u{1F1FA}\u{1F1F8}").len());
    assert_eq!(2, segments("\u{1F1FA}\u{1F1F8}\u{1F1E9}\u{1F1EA}").len());
}

#[test]
fn newlines_of_every_kind_separate_words() {
    assert_eq!(2, words("One\r\ntwo"));
    assert_eq!(2, words("a\u{2028}b"));
    assert_eq!(2, words("a\u{2029}b"));
    assert_eq!(2, words("a\u{000B}b"));
    // CR LF is one segment, not two (WB3).
    assert_eq!(3, segments("a\r\nb").len());
}

#[test]
fn scripts_without_spaces_follow_the_documented_limits() {
    // No dictionary: an ideograph or a Hiragana syllable is a segment each; Katakana runs join (WB13).
    assert_eq!(2, words("北京"));
    assert_eq!(1, words("テスト"));
    assert_eq!(3, words("これは"));
    // Thai and friends join only each other: an unspaced run is one word, not one per letter.
    assert_eq!(2, words("สวัสดี ครับ"));
    // Hebrew letters around a double quote: WB7b / WB7c.
    assert_eq!(1, words("צה\"ל"));
}

#[test]
fn characters_are_grapheme_clusters_like_swift() {
    assert_eq!(0, characters(""));
    assert_eq!(3, characters("abc"));
    assert_eq!(6, characters("Привет"));
    assert_eq!(1, characters("e\u{0301}"));
    assert_eq!(1, characters(FAMILY));
    assert_eq!(1, characters("\r\n"));
    assert_eq!(2, characters("\u{1F600}\u{1F600}"));
}
