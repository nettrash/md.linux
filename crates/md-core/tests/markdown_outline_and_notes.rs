//! Port of md.win `tests/Md.Core.Tests/OutlineAndNotesTests.cs`: the outline, the notes panel and
//! the anchor slug. The slug is the join between the Contents outline and the ids the HTML writer
//! assigns: one shared, positional counter walked in document order, so a line one side counts as
//! a heading and the other does not drifts every later anchor and a table-of-contents link scrolls
//! to nothing.

mod markdown_support;

use std::collections::HashMap;

use markdown_support::{NBSP, NEL, kinds, lines};
use md_core::markdown::{BlockKind, NoteEntry, OutlineEntry, parser};

fn slug(text: &str) -> String {
    parser::slug(text, &mut HashMap::new())
}

fn texts(entries: &[OutlineEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.text.as_str()).collect()
}

fn slugs(entries: &[OutlineEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.slug.as_str()).collect()
}

fn note_texts(entries: &[NoteEntry]) -> Vec<&str> {
    entries.iter().map(|n| n.text.as_str()).collect()
}

// MARK: - Outline

#[test]
fn outline_levels_slugs_and_lines() {
    let outline = parser::outline("# One\n\ntext\n\n## Two\n\n```\n# not a heading\n```\n\nSetext\n---");
    assert_eq!(outline.len(), 3);
    assert_eq!(outline[0], OutlineEntry::new(1, "One", "one", 0));
    assert_eq!(outline[1].slug, "two");
    // A setext entry points at the text line, not the underline.
    assert_eq!(outline[2], OutlineEntry::new(2, "Setext", "setext", 10));
}

#[test]
fn duplicate_heading_slugs_are_deduped() {
    assert_eq!(slugs(&parser::outline("# Same\n\n# Same")), ["same", "same-1"]);
}

#[test]
fn outline_skips_underline_after_multi_line_paragraph() {
    // Only a single buffered line can be setext — the rule `parse` applies.
    assert!(parser::outline("line1\nline2\n---").is_empty());
    assert_eq!(parser::outline("only\n---").len(), 1);
    assert_eq!(parser::outline("  Title  \n==="), [OutlineEntry::new(1, "Title", "title", 0)]);
}

#[test]
fn outline_counts_lines_with_the_parsers_own_splitter() {
    assert_eq!(parser::outline("# One\r\n\r\n# Two").iter().map(|e| e.line).collect::<Vec<_>>(), [0, 2]);
}

#[test]
fn outline_skips_a_comment_closed_or_not() {
    assert_eq!(texts(&parser::outline("<!-- x -->\n\n# After")), ["After"]);
    assert!(parser::outline("<!-- never closed\n# Swallowed").is_empty());
}

#[test]
fn outline_skips_headings_inside_a_quote() {
    // A quoted heading gets no id and consumes no counter in the HTML writer either.
    assert_eq!(texts(&parser::outline("> # h\n\n# g")), ["g"]);
}

#[test]
fn outline_does_not_underline_what_is_not_plain_text() {
    assert!(parser::outline("\\newpage\n---").is_empty());
    assert_eq!(texts(&parser::outline("- item\n# h")), ["h"]);
}

#[test]
fn outline_sees_setext_headings_parse_does_not() {
    // Shipped divergence, replicated by every port: a table header row and a list-item
    // continuation line are not excluded from plain text, so each yields a phantom entry (and
    // consumes a slug) that `parse` never emits.
    assert_eq!(texts(&parser::outline("| a |\n---")), ["| a |"]);
    assert!(!kinds("| a |\n---").contains(&BlockKind::Heading));
    assert_eq!(texts(&parser::outline("- item\ncontinuation\n---")), ["continuation"]);
}

#[test]
fn footnote_definition_does_not_leak_into_the_outline() {
    // `parse` claims the definition line, so the `---` under it is a rule, not an underline.
    assert_eq!(texts(&parser::outline("# Real\n\n[^a]: The note.\n---\n\nText[^a].")), ["Real"]);
}

#[test]
fn front_matter_does_not_leak_into_the_outline_or_notes() {
    // The closing `---` underlines the last metadata line; a raw scan would read a setext heading
    // the rendered document does not contain and steal a slug.
    let outline = parser::outline("---\ntitle: My Post\n---\n\n# Hello\n");
    assert_eq!(texts(&outline), ["Hello"]);
    assert_eq!(outline[0].slug, "hello");
    assert_eq!(parser::outline("---\nHello: x\n---\n\n# Hello: x\n")[0].slug, "hello-x");
    assert!(parser::outline("---\ntitle: A\n---").is_empty());
    // Unclosed front matter is ordinary text, and its `---` is a rule.
    assert_eq!(texts(&parser::outline("---\ntitle: A\n\n# h")), ["h"]);

    assert!(parser::notes("---\ntitle: X\n<!-- note: hidden -->\n---\n\nBody.").is_empty());
    assert_eq!(parser::notes("---\ntitle: X\n---\n\n<!-- note: real -->").len(), 1);
}

// MARK: - Notes

#[test]
fn notes_helper_finds_line() {
    assert_eq!(parser::notes("start\n\n<!-- note: fix me -->\n\nend"), [NoteEntry::new("fix me", 2)]);
}

#[test]
fn notes_list_every_note_with_the_line_it_opens_on() {
    assert_eq!(
        parser::notes("a\n<!-- note: one\ntwo -->\n<!-- note: three -->"),
        [NoteEntry::new("one\ntwo", 1), NoteEntry::new("three", 3)]
    );
    assert_eq!(parser::notes("<!-- note: open\nmore"), [NoteEntry::new("open\nmore", 0)]);
    assert_eq!(parser::notes("   <!-- note: x -->"), [NoteEntry::new("x", 0)]);
}

#[test]
fn notes_ignore_a_fence_a_quote_and_a_tab_indented_comment() {
    assert!(parser::notes("```\n<!-- note: not really -->\n```").is_empty());
    // The scan does not recurse into quotes, and a tab before `<!--` is not a comment start.
    assert!(parser::notes("> <!-- note: q -->").is_empty());
    assert!(parser::notes("\t<!-- note: x -->").is_empty());
}

#[test]
fn parser_note_and_fence_trims_are_foundations_own() {
    // The body is trimmed with the newline-bearing set: U+0085 is whitespace here and is not a
    // blank line anywhere else. Kotlin's own set kept it, and a note vanished from its panel.
    assert_eq!(note_texts(&parser::notes(&format!("<!--{NEL} note: n -->"))), ["n"]);
    assert_eq!(note_texts(&parser::notes(&format!("<!-- note: n {NEL}-->"))), ["n"]);
    assert_eq!(note_texts(&parser::notes("<!-- note:\n\u{2028}n\u{2029}\n -->")), ["n"]);
}

// MARK: - Slug

#[test]
fn slug_lowercases_keeps_letters_and_digits_and_maps_a_space_to_a_hyphen() {
    assert_eq!(slug("Getting Started"), "getting-started");
    assert_eq!(slug("Chapter 12"), "chapter-12");
    assert_eq!(slug("Hello, World! 2026"), "hello-world-2026");
    assert_eq!(slug("Ünïcödé Ñ"), "ünïcödé-ñ");
    assert_eq!(slug("日本語 テスト"), "日本語-テスト");
    // Every U+0020 becomes a hyphen, so two spaces are two hyphens.
    assert_eq!(slug("a  b"), "a--b");
    assert_eq!(slug("  "), "--");
}

#[test]
fn slug_drops_punctuation_like_git_hub() {
    assert_eq!(slug("C# & F#!"), "c--f");
}

#[test]
fn slug_keeps_hyphen_and_underscore_but_drops_every_other_whitespace() {
    assert_eq!(slug("a-b_c"), "a-b_c");
    assert_eq!(slug("---"), "---");
    assert_eq!(slug("_"), "_");
    assert_eq!(slug("a\tb"), "ab");
    assert_eq!(slug(&format!("a{NBSP}b")), "ab");
    assert_eq!(slug("a\u{2003}b"), "ab");
}

#[test]
fn parser_slug_keeps_what_the_android_port_keeps() {
    // A grapheme walk dropped the hyphen and the mark together; a code-point walk keeps both.
    assert_eq!(slug("a -\u{0301}b c"), "a--\u{0301}b-c");
    // A ZWJ is a format character and is kept by neither.
    assert_eq!(slug("Heading\u{200D}"), "heading");
    // A combining mark on a letter rides along.
    assert_eq!(slug("Cafe\u{0301}"), "cafe\u{0301}");
    let mut used = HashMap::new();
    assert_eq!(parser::slug("Getting Started", &mut used), "getting-started");
    assert_eq!(parser::slug("Getting Started", &mut used), "getting-started-1");
}

#[test]
fn slug_keeps_an_astral_letter_which_a_utf16_walk_would_drop() {
    assert_eq!(slug("\u{1D400} Heading"), "\u{1D400}-heading");
}

#[test]
fn slug_lowercases_the_whole_string_with_the_full_mapping() {
    // U+0130 lowercases to two scalars in Swift and JavaScript, and the second is a mark the slug
    // keeps. A simple 1:1 mapping gives `i`.
    assert_eq!(slug("\u{0130}"), "i\u{0307}");
    assert_eq!(slug("\u{212A}\u{212B}"), "k\u{00E5}");
}

#[test]
fn slug_does_not_apply_final_sigma() {
    // Swift maps each scalar on its own, so a Greek capital sigma is always `σ`, never the
    // word-final `ς` that Java and JavaScript produce. md.macOS is the source of truth.
    assert_eq!(slug("ΟΔΟΣ ΣΑΣ Σ"), "οδοσ-σασ-σ");
}

#[test]
fn slug_keeps_the_enclosed_alphabetics() {
    // Unicode's Alphabetic property reaches past letters, marks and letter-numbers to the circled
    // and squared Latin letters (general category So); Swift and TypeScript keep them.
    assert_eq!(slug("\u{24B6}\u{1F130} x"), "\u{24D0}\u{1F130}-x");
}

#[test]
fn slug_keeps_numbers_of_every_kind() {
    // Nl, No, a CJK numeral (a letter anyway) and a non-ASCII decimal digit all survive.
    assert_eq!(slug("Ⅹ ½ 三 ٣"), "ⅹ-½-三-٣");
}

#[test]
fn slug_never_normalises() {
    // NFC and NFD are different anchors on every platform alike.
    assert_ne!(slug("café"), slug("cafe\u{0301}"));
}

#[test]
fn slug_falls_back_to_section_when_nothing_survives() {
    assert_eq!(slug("!!!"), "section");
    assert_eq!(slug(""), "section");
    // …and the fallback shares the counter with a heading literally named `section`.
    let mut used = HashMap::new();
    assert_eq!(["!", "?", "section"].map(|t| parser::slug(t, &mut used)), ["section", "section-1", "section-2"]);
}

#[test]
fn slug_deduplicates_through_the_callers_counter() {
    let mut used = HashMap::new();
    assert_eq!(parser::slug("Getting Started", &mut used), "getting-started");
    assert_eq!(parser::slug("Getting Started", &mut used), "getting-started-1");
    assert_eq!(parser::slug("Getting Started", &mut used), "getting-started-2");
}

#[test]
fn slug_can_still_produce_a_duplicate_id_exactly_as_git_hub_does() {
    // The counter counts the bare slug: `Same`, `Same 1`, `Same` yield `same`, `same-1`, `same-1`.
    // Replicated, not fixed.
    let mut used = HashMap::new();
    assert_eq!(["Same", "Same 1", "Same"].map(|t| parser::slug(t, &mut used)), ["same", "same-1", "same-1"]);
}

#[test]
fn outline_slugs_follow_document_order_across_every_heading_kind() {
    // The invariant the HTML writer relies on: the same counter, walked in the same order,
    // skipping front matter, fences and footnote definitions.
    let source = lines(&[
        "---",
        "title: Skipped",
        "---",
        "",
        "# Same",
        "",
        "## Same",
        "",
        "```",
        "# not a heading",
        "```",
        "",
        "[^a]: a definition, not plain text",
        "---",
        "",
        "Setext",
        "===",
    ]);
    assert_eq!(slugs(&parser::outline(&source)), ["same", "same-1", "setext"]);
}
