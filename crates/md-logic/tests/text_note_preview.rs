//! Port of md.win `tests/Md.App.Logic.Tests/NotePreviewTests.cs` — shell-design §5.6, the Go ▸
//! Notes label. Four steps, each of which the Swift does in a way a naive port gets wrong: the
//! first line is the first NON-EMPTY one, a line of spaces still counts as that line, whitespace
//! collapses on Unicode White_Space, and the 50 is grapheme clusters, not UTF-16 units. Every
//! non-ASCII character is written as an escape so an editor or a normalising tool cannot quietly
//! change what is being pinned.

use md_logic::strings;
use md_logic::text::note_preview;

const ACUTE: &str = "\u{0301}"; // COMBINING ACUTE ACCENT
const E_ACUTE: &str = "e\u{0301}"; // one cluster, two UTF-16 units
const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}"; // one cluster, eleven units
const ELLIPSIS: &str = "\u{2026}";

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[test]
fn a_note_with_nothing_on_its_first_line_is_the_empty_note() {
    for text in [
        "",
        "\n\n\n",
        "   ",
        "   \n fix me", // a line of spaces IS the first line; collapsing it empties it
        "\u{2003}\u{00A0}",
    ] {
        assert_eq!(note_preview::of(text), "(empty note)", "{text:?}");
    }
}

#[test]
fn the_empty_note_literal_comes_from_strings() {
    assert_eq!(note_preview::of(""), strings::EMPTY_NOTE);
    assert_eq!(strings::ELLIPSIS, ELLIPSIS);
}

#[test]
fn leading_blank_lines_are_skipped() {
    assert_eq!(note_preview::of("\n\n\nfix me\nand more"), "fix me");
}

#[test]
fn only_the_first_line_is_used() {
    assert_eq!(note_preview::of("one\ntwo\nthree"), "one");
}

/// Character.isNewline: the seven Foundation terminators, and CRLF as one of them.
#[test]
fn every_line_terminator_swift_knows_ends_the_first_line() {
    for terminator in ["\r", "\n", "\r\n", "\u{000B}", "\u{000C}", "\u{0085}", "\u{2028}", "\u{2029}"] {
        assert_eq!(note_preview::of(&format!("one{terminator}two")), "one", "{terminator:?}");
    }
}

#[test]
fn carriage_return_line_feed_is_one_separator_and_not_two() {
    assert_eq!(note_preview::of("\r\n\r\none\r\ntwo"), "one");
}

#[test]
fn whitespace_runs_collapse_to_one_space_and_the_ends_lose_theirs() {
    assert_eq!(note_preview::of("  a \t  b c   "), "a b c");
}

#[test]
fn collapsing_uses_unicode_white_space_and_not_just_space_and_tab() {
    assert_eq!(note_preview::of("a\u{2003}\u{00A0}b"), "a b");
}

#[test]
fn fifty_text_elements_are_kept_whole() {
    let fifty = "a".repeat(50);
    assert_eq!(note_preview::of(&fifty), fifty);
    assert_eq!(note_preview::of(&format!("{fifty}b")), format!("{fifty}{ELLIPSIS}"));
}

#[test]
fn the_cut_is_trimmed_before_the_ellipsis_is_added() {
    // Element 50 is the space between the two words, so the cut ends in one.
    let text = format!("{} bcd", "a".repeat(49));
    assert_eq!(note_preview::of(&text), format!("{}{ELLIPSIS}", "a".repeat(49)));
}

#[test]
fn combining_marks_count_as_one_element_each() {
    let text = E_ACUTE.repeat(60);
    let preview = note_preview::of(&text);
    assert_eq!(preview, format!("{}{ELLIPSIS}", E_ACUTE.repeat(50)));
    assert_eq!(utf16_len(&preview), 101); // 50 x 2 units plus the ellipsis
}

#[test]
fn emoji_count_as_one_element_each() {
    let text = FAMILY.repeat(60);
    let preview = note_preview::of(&text);
    assert_eq!(preview, format!("{}{ELLIPSIS}", FAMILY.repeat(50)));
    assert_eq!(utf16_len(&preview), 551); // 50 x 11 units plus the ellipsis
}

#[test]
fn a_space_wearing_a_combining_mark_is_not_a_separator() {
    // Swift splits on Characters, so a cluster whose base is a space is not whitespace; md-core's
    // own view-mode codec splits the same way for the same reason.
    let text = format!("a {ACUTE}b");
    assert_eq!(note_preview::of(&text), text);
}
