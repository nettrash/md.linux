//! Port of md.win `tests/Md.App.Logic.Tests/DocumentTextTests.cs`.
//!
//! §3.2 and §6.3: the model is LF, the file keeps its own dressing. These pin the round trip a
//! document makes between disk and the editor — the one place where "it looks the same" is not
//! enough, because a CRLF file silently rewritten as LF is a diff in every line of somebody's repo.

mod common;

use common::document_fixtures::{cp1251, prefixed, utf16le};
use md_core::document::plain_text_codec::TextEncoding;
use md_logic::documents::{NewLine, TextFileDressing, editor_text, line_endings, line_offsets};

// ---- LineEndings ----

#[test]
fn the_first_line_break_decides_the_files_convention() {
    let rows = [
        ("a\nb", NewLine::Lf),
        ("a\r\nb", NewLine::CrLf),
        ("a\rb", NewLine::Lf), // a lone CR is a break, but never written back
        ("no break at all", NewLine::Lf),
        ("", NewLine::Lf),
        ("a\r", NewLine::Lf), // CR at the very end: no LF follows it
    ];
    for (text, expected) in rows {
        assert_eq!(line_endings::detect(text), expected, "{text:?}");
    }
}

#[test]
fn a_mixed_file_takes_its_convention_from_the_first_break_only() {
    assert_eq!(line_endings::detect("a\r\nb\nc"), NewLine::CrLf);
    assert_eq!(line_endings::detect("a\nb\r\nc"), NewLine::Lf);
}

#[test]
fn normalize_folds_every_form_to_lf() {
    for (input, expected) in [("a\r\nb\rc\nd", "a\nb\nc\nd"), ("\r\n\r\n", "\n\n"), ("plain", "plain")] {
        assert_eq!(line_endings::normalize(input), expected, "{input:?}");
    }
}

#[test]
fn apply_is_the_inverse_of_normalize_for_both_conventions() {
    let model = "# Title\n\nBody\n";
    assert_eq!(line_endings::apply(model, NewLine::Lf), model);
    assert_eq!(line_endings::apply(model, NewLine::CrLf), "# Title\r\n\r\nBody\r\n");
    assert_eq!(line_endings::normalize(&line_endings::apply(model, NewLine::CrLf)), model);
}

// ---- EditorText ----

/// LINUX: md.win's WinUI `TextBox` reports every break as `\r`; a `GtkTextBuffer` keeps what it was
/// given, so a CR reaches it only by paste or drop. Either way none may reach the model.
#[test]
fn the_text_boxs_carriage_returns_never_reach_the_model() {
    assert_eq!(editor_text::from_editor("a\rb\rc"), "a\nb\nc");
    assert_eq!(editor_text::from_editor("a\r\nb\r\nc"), "a\nb\nc");
    assert_eq!(editor_text::from_editor("a\nb\nc"), "a\nb\nc");
}

#[test]
fn assigning_the_model_back_is_the_identity_and_round_trips() {
    let model = "one\ntwo\n";
    assert!(std::ptr::eq(editor_text::to_editor(model), model));
    // Whatever break the editor then reports folds back to the model.
    assert_eq!(editor_text::from_editor(&model.replace('\n', "\r")), model);
}

#[test]
fn an_external_replace_keeps_the_selection_clamped() {
    let rows = [
        (3, 4, 10, 3, 4),
        (20, 5, 10, 10, 0), // caret past the end of a shorter document
        (8, 5, 10, 8, 2),   // selection runs off the end: keep what is left
        (-1, -1, 10, 0, 0),
    ];
    for (start, length, text_length, expected_start, expected_length) in rows {
        assert_eq!(
            editor_text::clamp_selection(start, length, text_length),
            (expected_start, expected_length),
            "{start}, {length}, {text_length}"
        );
    }
}

// ---- LineOffsets ----

/// md.win's `text[offset..]` slices UTF-16; the texts here are ASCII until the last test, so the
/// offset is also a byte index.
#[test]
fn offset_of_line_counts_every_break_form_once() {
    let text = "zero\rone\r\ntwo\nthree";
    assert_eq!(line_offsets::offset_of_line(0, text), 0);
    assert_eq!(line_offsets::offset_of_line(1, text), 5); // after "zero\r"
    assert_eq!(line_offsets::offset_of_line(2, text), 10); // after "one\r\n"
    assert_eq!(line_offsets::offset_of_line(3, text), 14); // after "two\n"
    assert_eq!(&text[line_offsets::offset_of_line(3, text)..], "three");
}

#[test]
fn offset_of_line_past_the_end_is_the_start_of_the_last_line() {
    let text = "a\nb\nc";
    assert_eq!(line_offsets::offset_of_line(2, text), 4);
    assert_eq!(line_offsets::offset_of_line(99, text), 4);
    assert_eq!(line_offsets::offset_of_line(0, ""), 0);
    assert_eq!(line_offsets::offset_of_line(5, ""), 0);
}

#[test]
fn offset_of_line_ignores_the_unicode_line_separators_because_the_parser_does() {
    let text = "a\u{2028}b\u{2029}c\nd";
    assert_eq!(line_offsets::offset_of_line(0, text), 0);
    assert_eq!(line_offsets::offset_of_line(1, text), 6); // only the \n counted
}

#[test]
fn offsets_are_utf16_units_so_an_astral_character_counts_as_two() {
    let text = "\u{1F600}\nnext"; // one emoji = two UTF-16 units
    let offset = line_offsets::offset_of_line(1, text);
    assert_eq!(offset, 3);
    let units: Vec<u16> = text.encode_utf16().collect();
    assert_eq!(String::from_utf16(&units[offset..]).unwrap(), "next");
}

// ---- TextFileDressing ----

#[test]
fn a_plain_utf8_lf_file_round_trips_byte_for_byte() {
    let bytes = b"# Scene\nBody\n".to_vec();
    let (text, dressing) = TextFileDressing::undress(&bytes).unwrap();
    assert_eq!(text, "# Scene\nBody\n");
    assert_eq!(dressing, TextFileDressing::new(TextEncoding::Utf8, false, NewLine::Lf));
    let (written, used) = dressing.dress(&text);
    assert_eq!(written, bytes);
    assert_eq!(used, TextEncoding::Utf8);
}

#[test]
fn a_crlf_file_is_edited_as_lf_and_saved_as_crlf() {
    let bytes = b"one\r\ntwo\r\n".to_vec();
    let (text, dressing) = TextFileDressing::undress(&bytes).unwrap();
    assert_eq!(text, "one\ntwo\n");
    assert_eq!(dressing.new_line, NewLine::CrLf);
    assert_eq!(dressing.dress(&text).0, bytes);
    // And a line typed into it comes back out with the file's own break.
    assert_eq!(dressing.dress(&format!("{text}three\n")).0, b"one\r\ntwo\r\nthree\r\n");
}

#[test]
fn a_lone_carriage_return_file_becomes_lf() {
    let (text, dressing) = TextFileDressing::undress(b"one\rtwo\r").unwrap();
    assert_eq!(text, "one\ntwo\n");
    assert_eq!(dressing.new_line, NewLine::Lf);
    assert_eq!(dressing.dress(&text).0, b"one\ntwo\n");
}

#[test]
fn a_utf8_bom_is_stripped_on_read_and_written_back_on_save() {
    let bytes = prefixed(&[0xEF, 0xBB, 0xBF], b"hello");
    let (text, dressing) = TextFileDressing::undress(&bytes).unwrap();
    assert_eq!(text, "hello"); // no stray U+FEFF in the editor
    assert!(dressing.had_utf8_bom);
    assert_eq!(dressing.dress(&text).0, bytes); // the decision this design records
}

#[test]
fn a_file_without_a_bom_never_gains_one() {
    let (text, dressing) = TextFileDressing::undress(b"hello").unwrap();
    assert!(!dressing.had_utf8_bom);
    assert_eq!(dressing.dress(&text).0, b"hello");
}

#[test]
fn a_bomed_utf16_file_round_trips_through_its_own_encoding() {
    let bytes = prefixed(&[0xFF, 0xFE], &utf16le("Привет\r\n"));
    let (text, dressing) = TextFileDressing::undress(&bytes).unwrap();
    assert_eq!(text, "Привет\n");
    assert_eq!(dressing.encoding, TextEncoding::Utf16);
    assert_eq!(dressing.new_line, NewLine::CrLf);
    assert!(!dressing.had_utf8_bom); // the UTF-16 BOM is the codec's job
    assert_eq!(dressing.dress(&text).0, bytes);
}

#[test]
fn a_legacy_cp1251_file_is_saved_back_in_cp1251() {
    let bytes = cp1251("Привет, мир!");
    let (text, dressing) = TextFileDressing::undress(&bytes).unwrap();
    assert_eq!(text, "Привет, мир!");
    assert_eq!(dressing.encoding, TextEncoding::WindowsCp1251);
    let (written, used) = dressing.dress(&format!("{text} Ещё."));
    assert_eq!(written, cp1251("Привет, мир! Ещё."));
    assert_eq!(used, TextEncoding::WindowsCp1251);
}

#[test]
fn text_that_outgrows_cp1251_upgrades_to_utf8_and_says_so() {
    let (text, dressing) = TextFileDressing::undress(&cp1251("Привет")).unwrap();
    let grown = format!("{text} \u{1F642}");
    let (written, used) = dressing.dress(&grown);
    assert_eq!(written, grown.as_bytes());
    assert_eq!(used, TextEncoding::Utf8);
    // The caller must remember the upgrade, or every later autosave retries the losing encoding.
    assert_eq!(dressing.with_encoding(used).encoding, TextEncoding::Utf8);
}

#[test]
fn an_upgraded_file_does_not_acquire_a_utf8_bom() {
    let dressing = TextFileDressing::new(TextEncoding::WindowsCp1251, true, NewLine::Lf);
    let (bytes, used) = dressing.dress("\u{1F642}");
    assert_eq!(used, TextEncoding::Utf8);
    assert_eq!(bytes, "\u{1F642}".as_bytes());
    assert!(!dressing.with_encoding(used).had_utf8_bom);
}

#[test]
fn a_file_that_is_nothing_but_a_bom_reads_as_empty_and_keeps_it() {
    let (text, dressing) = TextFileDressing::undress(&[0xEF, 0xBB, 0xBF]).unwrap();
    assert_eq!(text, "");
    assert!(dressing.had_utf8_bom);
    assert_eq!(dressing.dress("").0, [0xEF, 0xBB, 0xBF]);
}

#[test]
fn an_empty_file_is_an_empty_utf8_lf_document() {
    let (text, dressing) = TextFileDressing::undress(&[]).unwrap();
    assert_eq!(text, "");
    assert_eq!(dressing, TextFileDressing::DEFAULT);
    assert!(dressing.dress("").0.is_empty());
}
