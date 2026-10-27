//! Port of md.win `tests/Md.Core.Tests/MarkdownParserTests.cs`. Every expected value was produced
//! by the real md.macOS parser (the Swift source compiled as an oracle), not reasoned about.

mod markdown_support;

use markdown_support::*;
use md_core::markdown::{BlockKind, ColumnAlignment, MarkdownBlock, parser};

use BlockKind as K;

// MARK: - Headings

#[test]
fn heading_levels() {
    for level in 1..=6 {
        let heading = heading(&first(&format!("{} Title", "#".repeat(level))));
        assert_eq!(heading.0, level);
        assert_eq!(heading.1, "Title");
    }
}

#[test]
fn heading_requires_space() {
    assert_eq!(kinds("#Title"), [K::Paragraph]);
    // A tab does not count either: the marker test is against U+0020 alone.
    assert_eq!(kinds("#\tTitle"), [K::Paragraph]);
    assert_eq!(parse("#\t")[0], p("#\t"));
}

#[test]
fn heading_seven_hashes_is_paragraph() {
    assert_eq!(kinds("####### too deep"), [K::Paragraph]);
    assert_eq!(kinds("######## deeper still"), [K::Paragraph]);
}

#[test]
fn heading_closing_hashes_stripped() {
    assert_eq!(heading(&first("## Title ##")).1, "Title");
    assert_eq!(heading(&first("## Title #")).1, "Title");
    assert_eq!(heading(&first("# Title ##   ")).1, "Title");
    assert_eq!(heading(&first("#   Title   ")).1, "Title");
}

#[test]
fn heading_preserves_trailing_hash_in_word() {
    // The closing-hash strip needs SPTAB before the run, so `C#` and `F#` keep theirs.
    assert_eq!(heading(&first("# C#")).1, "C#");
    assert_eq!(heading(&first("# F# notes")).1, "F# notes");
    assert_eq!(heading(&first("# Title#")).1, "Title#");
}

#[test]
fn heading_of_only_hashes_is_empty() {
    // A bare `#` is a level-1 heading with no text; a text of only `#`s strips to nothing.
    assert_eq!(single("#"), h(1, ""));
    assert_eq!(single("# #"), h(1, ""));
    assert_eq!(single("#  ##"), h(1, ""));
}

#[test]
fn heading_allows_unbounded_indentation() {
    // Asymmetric with a fence, which caps at three spaces. Recorded, not fixed.
    assert_eq!(single("        # Deep"), h(1, "Deep"));
}

#[test]
fn setext_headings() {
    let h1 = parse("My Title\n===");
    assert_eq!(h1, [h(1, "My Title")]);

    // Exactly one block: the underline must not also emit a thematic break.
    let h2 = parse("My Title\n---");
    assert_eq!(h2, [h(2, "My Title")]);

    // The underline is WS-trimmed and the text is too.
    assert_eq!(single("Title\n  ===  "), h(1, "Title"));
    assert_eq!(single("  Title  \n==="), h(1, "Title"));
}

#[test]
fn setext_underline_may_be_short() {
    // Setext is tested before the break set, so a lone `-` (a list marker) and `--` (never a rule)
    // both underline; `- - -` keeps its interior spaces and is a rule after a paragraph.
    assert_eq!(single("Title\n-"), h(2, "Title"));
    assert_eq!(single("Title\n--"), h(2, "Title"));
    assert_eq!(kinds("Title\n- - -"), [K::Paragraph, K::ThematicBreak]);
}

#[test]
fn setext_fires_only_with_one_buffered_line() {
    // Two buffered lines and the `---` is a rule — the rule `outline()` applies too.
    assert_eq!(kinds("line1\nline2\n---"), [K::Paragraph, K::ThematicBreak]);
    // An ATX heading is not a buffered paragraph line.
    assert_eq!(kinds("# h\n---"), [K::Heading, K::ThematicBreak]);
}

#[test]
fn standalone_rule_still_parses_after_setext_change() {
    assert_eq!(kinds("---"), [K::ThematicBreak]);
}

// MARK: - Paragraphs

#[test]
fn paragraph_preserves_soft_breaks() {
    assert_eq!(paragraph(&first("line one\nline two")), "line one\nline two");
}

#[test]
fn blank_line_separates_paragraphs() {
    assert_eq!(kinds("first\n\nsecond"), [K::Paragraph, K::Paragraph]);
}

#[test]
fn paragraph_keeps_raw_lines() {
    // Only a setext heading's text is trimmed; leading and trailing spaces survive.
    assert_eq!(paragraph(&first("  spaced  \n  again  ")), "  spaced  \n  again  ");
}

#[test]
fn empty_source_parses_to_nothing() {
    assert!(parse("").is_empty());
    assert!(parse("\n").is_empty());
    assert!(parse("\t").is_empty());
}

#[test]
fn parser_blank_line_set_is_foundations_own() {
    // U+200B is a space separator to Apple's frozen tables and a format character to every current
    // one; a line holding one is a blank line on all three shipping platforms.
    assert_eq!(parse(&format!("para\n{ZWSP}\nnext")).len(), 2);
    assert_eq!(parse(&format!("para\n \t{NBSP}\nnext")).len(), 2);
    // It separates two lists as well.
    assert_eq!(kinds(&format!("- a\n{ZWSP}\n- b")), [K::List, K::List]);
}

#[test]
fn parser_line_endings_are_scalar_exact() {
    // CR, LF and CRLF each end exactly one line.
    assert_eq!(paragraph(&first("one\r\ntwo\rthree\nfour")), "one\ntwo\nthree\nfour");
    assert_eq!(parse("a\r\n\r\nb").len(), 2);
    assert_eq!(parse("a\rb\r\rc"), [p("a\nb"), p("c")]);
    assert_eq!(single("```\r\na\r\n```\r\n"), cb(None, "a"));
}

#[test]
fn wider_newline_set_is_content_to_the_block_scanner() {
    // NEL, LS and PS split lines for the raw-diagram probes and nowhere else.
    assert_eq!(single("a\u{0085}b"), p("a\u{0085}b"));
    assert_eq!(single("a\u{2028}\u{2029}b"), p("a\u{2028}\u{2029}b"));
}

// MARK: - Lists

#[test]
fn unordered_list() {
    let (ordered, items) = list(&first("- a\n- b\n* c\n+ d"));
    assert!(!ordered);
    assert_eq!(items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d"]);
    assert!(items.iter().all(|i| i.ordinal.is_none()));
}

#[test]
fn ordered_list() {
    let (ordered, items) = list(&first("1. one\n2. two\n3) three"));
    assert!(ordered);
    assert_eq!(items.iter().map(|i| i.ordinal).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
    assert_eq!(
        markdown_support::items(&first("1) a\n2) b")).iter().map(|i| i.ordinal).collect::<Vec<_>>(),
        [Some(1), Some(2)]
    );
}

#[test]
fn mixed_run_is_ordered() {
    // One flag for the whole run; the bullet item keeps a null ordinal.
    let (ordered, items) = list(&first("- a\n1. b"));
    assert!(ordered);
    assert_eq!(items.iter().map(|i| i.ordinal).collect::<Vec<_>>(), [None, Some(1)]);
}

#[test]
fn nested_list_levels() {
    assert_eq!(items(&first("- top\n  - nested\n    - deeper")).iter().map(|i| i.level).collect::<Vec<_>>(), [0, 1, 2]);
}

#[test]
fn tab_indented_nested_list_recognised() {
    let items = items(&first("- top\n\t- nested"));
    assert_eq!(items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["top", "nested"]);
    assert!(items[1].level > items[0].level);
    // A tab advances to the next 4-column stop and the level is indent / 2 — the only place a tab
    // indents anything in this grammar.
    assert_eq!(
        markdown_support::items(&first("- a\n   - b\n\t\t- c")).iter().map(|i| i.level).collect::<Vec<_>>(),
        [0, 1, 4]
    );
}

#[test]
fn task_list() {
    let items = items(&first("- [ ] todo\n- [x] done\n- [X] also"));
    assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), [Some(false), Some(true), Some(true)]);
    assert_eq!(items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["todo", "done", "also"]);
}

#[test]
fn task_box_needs_a_space_or_the_end_of_the_line() {
    assert_eq!(items(&first("- [ ]x"))[0], item("[ ]x", 0, None, None));
    assert_eq!(items(&first("- [ ]"))[0], item("", 0, None, Some(false)));
    assert_eq!(items(&first("- [X]"))[0], item("", 0, None, Some(true)));
    // The spaces after the box go; the text's own trailing spaces stay.
    assert_eq!(items(&first("  - [x]  spaced  "))[0], item("spaced  ", 1, None, Some(true)));
}

#[test]
fn bare_marker_is_an_item_with_empty_text() {
    for marker in ["-", "*", "+"] {
        let (_, items) = list(&single(marker));
        assert_eq!(items, [item("", 0, None, None)]);
    }
}

#[test]
fn marker_followed_by_tab_is_not_an_item() {
    assert_eq!(kinds("-\tfoo"), [K::Paragraph]);
}

#[test]
fn list_item_continuation_is_absorbed() {
    let blocks = parse("- First item\n  with continuation\n- Second item");
    assert_eq!(blocks.len(), 1);
    let items = items(&blocks[0]);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].text, "First item with continuation");
    assert_eq!(items[1].text, "Second item");
}

#[test]
fn list_breaks_into_a_table_but_absorbs_a_footnote_definition() {
    // The three continuation break-sets are deliberately not unified: the list loop carries the
    // table lookahead and not the footnote check.
    assert_eq!(kinds("- item\n| a |\n| --- |"), [K::List, K::Table]);
    assert_eq!(items(&first("- item\n[^a]: note"))[0].text, "item [^a]: note");
}

#[test]
fn list_continuation_breaks_on_every_other_block_start() {
    assert_eq!(kinds("- a\n# h"), [K::List, K::Heading]);
    // Heading indentation is unbounded, so an indented `#` line ends the item too.
    assert_eq!(kinds("- item\n  # heading"), [K::List, K::Heading]);
    assert_eq!(kinds("- a\n> q"), [K::List, K::Quote]);
    assert_eq!(kinds("- a\n***"), [K::List, K::ThematicBreak]);
    assert_eq!(kinds("- a\n\\newpage"), [K::List, K::PageBreak]);
    assert_eq!(kinds("- a\n<!-- c -->"), [K::List]);
    assert_eq!(kinds("- a\n```\nx\n```"), [K::List, K::CodeBlock]);
}

#[test]
fn parser_ordered_list_marker_is_ascii_digits_only() {
    // `Character.isNumber` admitted ½ and ٣ on Apple and `toIntOrNull` read ٣ as 3 on Android;
    // CommonMark says ASCII digits, and with that set every port agrees.
    assert_eq!(kinds("½. half"), [K::Paragraph]);
    assert_eq!(kinds("٣. three"), [K::Paragraph]);
    // A digit carrying a mark ends the run before the `.`, so the line joins the item above.
    for mark in MARKS {
        let (_, items) = list(&single(&format!("1. one\n2{mark}. two")));
        assert_eq!(items.iter().map(|i| i.text.clone()).collect::<Vec<_>>(), [format!("one 2{mark}. two")]);
    }
    let (ordered, items) = list(&first("1. one\n2. two"));
    assert!(ordered);
    assert_eq!(items.iter().map(|i| i.ordinal).collect::<Vec<_>>(), [Some(1), Some(2)]);
}

#[test]
fn ordered_marker_loses_leading_zeros_and_caps_at_nine_digits() {
    assert_eq!(items(&first("007. item"))[0].ordinal, Some(7));
    assert_eq!(kinds("1234567890. too many digits"), [K::Paragraph]);
    assert_eq!(items(&first("123456789. nine"))[0].ordinal, Some(123_456_789));
}

// MARK: - Code fences

#[test]
fn fenced_code_with_language() {
    assert_eq!(single("```swift\nlet x = 1\n```"), cb(Some("swift"), "let x = 1"));
}

#[test]
fn tilde_fence() {
    assert_eq!(single("~~~\nplain\n~~~"), cb(None, "plain"));
    // A closing fence must use the opening character.
    assert_eq!(single("```\n~~~\nx\n```"), cb(None, "~~~\nx"));
}

#[test]
fn fence_content_is_not_interpreted() {
    assert_eq!(code(&first("```\n# not a heading\n```")).1, "# not a heading");
}

#[test]
fn unclosed_fence_consumes_to_end() {
    assert_eq!(code(&first("```\na\nb")).1, "a\nb");
}

#[test]
fn unclosed_fence_gains_the_trailing_empty_line() {
    // Wart, replicated: the line splitter always appends a final element, so an unclosed fence in
    // a source ending with a newline gains a trailing "\n". A closed fence is unaffected.
    assert_eq!(code(&first("```\na\n")).1, "a\n");
    assert_eq!(code(&first("```\na\n```\n")).1, "a");
}

#[test]
fn indented_fence_strips_indent() {
    assert_eq!(code(&first("  ```\n  indented\n  ```")).1, "indented");
    assert_eq!(single("  ```js\n  x\n  ```"), cb(Some("js"), "x"));
    // Up to the opening indent is removed, U+0020 only, stopping at the first non-space.
    assert_eq!(code(&first("   ```\n     x\n  y\nz\n   ```")).1, "  x\ny\nz");
}

#[test]
fn fence_indented_four_spaces_or_by_a_tab_is_a_paragraph() {
    // There is no indented-code branch anywhere, so both fall through to a paragraph.
    assert_eq!(parse("    ```\n    code\n    ```"), [p("    ```\n    code\n    ```")]);
    assert_eq!(parse("\t```\ncode"), [p("\t```\ncode")]);
}

#[test]
fn closing_fence_may_be_indented_arbitrarily_and_be_longer() {
    let blocks = parse("```\ncode\n        `````\nafter");
    assert_eq!(blocks, [cb(None, "code"), p("after")]);
    // …but never shorter.
    assert_eq!(single("````\n```\nx\n````"), cb(None, "```\nx"));
}

#[test]
fn backtick_in_info_string_is_refused_for_a_backtick_fence_only() {
    // The refused opener is prose; the would-be closer then opens an unclosed, empty fence.
    assert_eq!(parse("```js `x`\ncode\n```"), [p("```js `x`\ncode"), cb(None, "")]);
    assert_eq!(single("~~~js `x`\ncode\n~~~"), cb(Some("js"), "code"));
}

#[test]
fn language_is_the_first_space_delimited_word_of_the_info_string() {
    // The trim is the WS set; the split is on U+0020 alone, so a tab or an NBSP is interior.
    assert_eq!(code(&first("```js extra\nx\n```")).0.as_deref(), Some("js"));
    assert_eq!(code(&first("```js\tfoo\nx\n```")).0.as_deref(), Some("js\tfoo"));
    assert_eq!(code(&first(&format!("```{NBSP}js{NBSP}extra\nx\n```"))).0, Some(format!("js{NBSP}extra")));
    // Never lowercased here — the renderer folds case at dispatch.
    assert_eq!(code(&first("```JS\nx\n```")).0.as_deref(), Some("JS"));
}

#[test]
fn parser_note_and_fence_trims_are_foundations_own() {
    // The Android port trimmed with space and tab only, so a fence padded with U+00A0 kept a
    // language nobody typed — and a closing fence padded with U+00A0 did not close.
    assert_eq!(single(&format!("```{NBSP}js\ncode()\n```")), cb(Some("js"), "code()"));
    assert_eq!(kinds(&format!("```\ncode()\n```{NBSP}\n\nafter")), [K::CodeBlock, K::Paragraph]);
    // The note half lives in the outline-and-notes suite.
    assert_eq!(
        parser::notes(&format!("<!--{NEL} note: n -->")).iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
        ["n"]
    );
}

#[test]
fn fence_wins_over_every_other_block_start() {
    assert_eq!(parse("```\n---\n# nope\n> nope\n- nope\n```"), [cb(None, "---\n# nope\n> nope\n- nope")]);
}

// MARK: - Block quotes

#[test]
fn block_quote() {
    let quote = quote(&first("> quoted\n> text"));
    assert_eq!(quote, [p("quoted\ntext")]);
}

#[test]
fn quote_ends_at_a_blank_line_and_at_an_unquoted_line() {
    // No lazy continuation.
    assert_eq!(kinds("> a\n\n> b"), [K::Quote, K::Quote]);
    assert_eq!(kinds("> a\nb"), [K::Quote, K::Paragraph]);
}

#[test]
fn quote_marker_strips_exactly_one_optional_space() {
    assert_eq!(quote(&first(">  two spaces")), [p(" two spaces")]);
    assert_eq!(quote(&first("   > x")), [p("x")]);
    // A marker with nothing behind it is an empty quote.
    assert!(quote(&single(">")).is_empty());
    assert!(quote(&single("> ")).is_empty());
}

#[test]
fn nested_block_quote() {
    let outer = quote(&first("> > deep"));
    assert_eq!(outer[0].kind(), K::Quote);

    // The stripped inner text is re-parsed whole, so deeper markers nest inside the paragraph.
    let levels = quote(&single("> a\n>> b\n>>> c"));
    assert_eq!(levels[0].kind(), K::Paragraph);
    let second = quote(&levels[1]);
    assert_eq!(second[0], p("b"));
    assert_eq!(quote(&second[1]), [p("c")]);
}

#[test]
fn quote_holds_any_block() {
    assert_eq!(quote(&single("> ```\n> x\n> ```")), [cb(None, "x")]);
    assert_eq!(quote(&single("> - a\n> - b"))[0].kind(), K::List);
    let (header, _, rows) = table(&quote(&single("> a | b\n> --- | ---\n> 1 | 2"))[0]);
    assert_eq!(header, ["a", "b"]);
    assert_eq!(rows, markdown_support::rows(&[&["1", "2"]]));
}

#[test]
fn deeply_nested_quote_does_not_overflow() {
    let blocks = parser::parse(&format!("{} deep", ">".repeat(5000)));
    assert!(!blocks.is_empty());
    assert_eq!(blocks[0].kind(), K::Quote);
}

#[test]
fn quote_nesting_caps_at_thirty_three_levels() {
    // The cap is part of the output shape: quote nodes at depths 0…32, the innermost holding one
    // paragraph with the still-`>`-laden remainder.
    fn descend(source: &str) -> (usize, MarkdownBlock) {
        let mut depth = 0;
        let mut current = parser::parse(source).remove(0);
        while let MarkdownBlock::Quote { blocks } = current {
            depth += 1;
            current = blocks.into_iter().next().unwrap();
        }
        (depth, current)
    }
    let (depth, current) = descend(&format!("{} deep", ">".repeat(5000)));
    assert_eq!(depth, 33);
    assert_eq!(current, p(&format!("{} deep", ">".repeat(4967))));

    let (depth, current) = descend(&format!("{} deep", ">".repeat(40)));
    assert_eq!(depth, 33);
    assert_eq!(current, p(">>>>>>> deep"));
}

// MARK: - Thematic breaks

#[test]
fn thematic_breaks() {
    for rule in ["---", "***", "___", "- - -", "****", "----------", " - -\t- "] {
        assert_eq!(kinds(rule), [K::ThematicBreak], "{rule:?}");
    }
}

#[test]
fn dashes_under_text_are_not_rule_when_too_short() {
    assert_eq!(kinds("--"), [K::Paragraph]);
    assert_eq!(kinds("**"), [K::Paragraph]);
}

#[test]
fn thematic_break_filters_only_space_and_tab() {
    // Narrower than every other predicate, which trim the WS set: an NBSP disqualifies.
    assert_eq!(parse(&format!("{NBSP}---")), [p(&format!("{NBSP}---"))]);
}

#[test]
fn thematic_break_refuses_a_mixed_run() {
    assert_eq!(kinds("--*"), [K::Paragraph]);
}

// MARK: - Tables

#[test]
fn table_parsing() {
    let (header, alignments, rows) =
        table(&single(&lines(&["| Name | Age |", "| :--- | ---: |", "| Ann  | 30 |", "| Bob  | 25 |"])));
    assert_eq!(header, ["Name", "Age"]);
    assert_eq!(alignments, [ColumnAlignment::Leading, ColumnAlignment::Trailing]);
    assert_eq!(rows, markdown_support::rows(&[&["Ann", "30"], &["Bob", "25"]]));
}

#[test]
fn table_center_alignment() {
    assert_eq!(table(&first("| A | B |\n|:-:|:-:|\n| 1 | 2 |")).1, [ColumnAlignment::Center, ColumnAlignment::Center]);
}

#[test]
fn table_cannot_distinguish_colon_dash_from_dash() {
    // There is no "explicitly left" alignment in this model.
    assert_eq!(table(&first("| A |\n| :--- |\n| 1 |")).1, [ColumnAlignment::Leading]);
}

#[test]
fn table_escaped_pipe() {
    assert_eq!(table(&first("| Col |\n| --- |\n| a \\| b |")).2, rows(&[&["a | b"]]));
    // Any other escape stays as written, and a trailing lone backslash is kept.
    assert_eq!(table(&first("| a |\n| --- |\n| a\\b |")).2, rows(&[&["a\\b"]]));
    assert_eq!(table(&first("| a |\n| --- |\n| x\\")).2, rows(&[&["x\\"]]));
}

#[test]
fn not_a_table_without_delimiter_row() {
    assert_eq!(kinds("a | b | c\nx | y | z"), [K::Paragraph]);
}

#[test]
fn delimiter_row_must_have_exactly_as_many_cells_as_the_header() {
    assert_eq!(parse("| a | b |\n| --- |\n| 1 | 2 |"), [p("| a | b |\n| --- |\n| 1 | 2 |")]);
}

#[test]
fn rows_are_padded_and_truncated_to_the_header_width() {
    let (_, _, table_rows) = table(&single("| a | b |\n| --- | --- |\n| 1 |\n| 1 | 2 | 3 |"));
    assert_eq!(table_rows, rows(&[&["1", ""], &["1", "2"]]));
}

#[test]
fn table_rows_end_at_a_line_without_a_pipe() {
    let blocks = parse("| a |\n| --- |\n| 1 |\nplain");
    assert_eq!(blocks.iter().map(MarkdownBlock::kind).collect::<Vec<_>>(), [K::Table, K::Paragraph]);
    assert_eq!(table(&blocks[0]).2, rows(&[&["1"]]));
    // A row of a single pipe is one empty cell (the trailing test runs after the leading drop).
    let single = parse("| a |\n| --- |\n   |   \nx");
    assert_eq!(single.iter().map(MarkdownBlock::kind).collect::<Vec<_>>(), [K::Table, K::Paragraph]);
    assert_eq!(table(&single[0]).2, rows(&[&[""]]));
}

#[test]
fn table_cannot_interrupt_a_paragraph() {
    // The paragraph loop carries no table lookahead. Reachable, shipped.
    assert_eq!(parse("Intro\n| a |\n| --- |"), [p("Intro\n| a |\n| --- |")]);
}

#[test]
fn heading_wins_over_table() {
    assert_eq!(parse("# a | b\n---|---"), [h(1, "a | b"), p("---|---")]);
}

#[test]
fn table_wins_over_list_and_quote_only_when_its_delimiter_row_is_clean() {
    let (header, _, rows) = table(&single("- a | b\n--- | ---"));
    assert_eq!(header, ["- a", "b"]);
    assert!(rows.is_empty());
    // `> ---` fails the `:?-+:?` test at the top level, so the quote branch takes the run and the
    // table is found inside it.
    let quote = quote(&single("> a | b\n> --- | ---"));
    assert_eq!(table(&quote[0]).0, ["a", "b"]);
}

// MARK: - Front matter

#[test]
fn yaml_front_matter_is_parsed_and_hidden() {
    let blocks = parse(&lines(&[
        "---",
        "title: My Book",
        "author: nettrash",
        "date: 2026-07-24",
        "---",
        "",
        "# Chapter One",
        "",
        "Text.",
    ]));
    let matter = front_matter(&blocks[0]);
    assert_eq!(matter, [field("title", "My Book"), field("author", "nettrash"), field("date", "2026-07-24")]);
    // The opening `---` must not survive as a thematic break, nor the metadata as prose.
    let kinds: Vec<_> = blocks.iter().map(MarkdownBlock::kind).collect();
    assert!(!kinds.contains(&K::ThematicBreak));
    assert_eq!(kinds, [K::FrontMatter, K::Heading, K::Paragraph]);
}

#[test]
fn toml_front_matter_uses_equals() {
    let matter = front_matter(&first("+++\ntitle = \"Quoted\"\ndraft = false\n+++\n\nBody."));
    assert_eq!(matter, [field("title", "Quoted"), field("draft", "false")]);
    // A colon is not a TOML separator, and `+++` is not a rule: the whole thing is prose.
    assert_eq!(parse("+++\nk: v\n+++\n"), [p("+++\nk: v\n+++")]);
    assert_eq!(front_matter(&first("+++\n# c\na = 1\n+++")), [field("a", "1")]);
}

#[test]
fn front_matter_only_at_the_very_top_and_only_when_closed() {
    // An unclosed opener stays a thematic break.
    assert_eq!(kinds("---\n\nJust a rule above.")[0], K::ThematicBreak);
    assert_eq!(kinds("---\ntitle: A"), [K::ThematicBreak, K::Paragraph]);

    // A rule, prose, and a rule again: every word survives.
    let divider = parse("---\n\nIntro the reader must see.\n\n---\n\nMore.");
    assert_eq!(divider[0].kind(), K::ThematicBreak);
    assert!(divider.contains(&p("Intro the reader must see.")));

    // The blank-line guard on its own — the only input that isolates it. Kotlin additionally pins
    // how it then reads: rule, setext h2, paragraph.
    let blank_first = parse("---\n\ntitle: A\n---\n\nbody");
    assert!(!blank_first.iter().any(|b| b.kind() == K::FrontMatter));
    assert_eq!(blank_first, [MarkdownBlock::ThematicBreak, h(2, "title: A"), p("body")]);

    // No `key: value` anywhere means it was never metadata: a break plus a setext heading.
    let setext = parse("---\nChapter One\n---\n\nText.");
    assert!(!setext.iter().any(|b| b.kind() == K::FrontMatter));
    assert!(setext.contains(&h(2, "Chapter One")));
    assert_eq!(kinds("---\n# c\n---\nBody"), [K::ThematicBreak, K::Heading, K::ThematicBreak, K::Paragraph]);

    // A fence further down is an ordinary thematic break too.
    assert!(!kinds("# Title\n\n---\n\ntitle: not metadata\n\n---").contains(&K::FrontMatter));
    // And it is never front matter inside a block quote.
    let quote = quote(&first("> ---\n> title: x\n> ---"));
    assert_eq!(quote, [MarkdownBlock::ThematicBreak, h(2, "title: x")]);
}

#[test]
fn front_matter_skips_what_the_flat_scan_cannot_read() {
    let blocks = parse(&lines(&["---", "title: Deep", "# a comment", "tags:", "  - one", "  - two", "---", "Body."]));
    let matter = front_matter(&blocks[0]);
    assert_eq!(matter[0], field("title", "Deep"));
    assert!(matter.contains(&field("tags", "")));
    assert!(!matter.iter().any(|f| f.key.starts_with('-')));
    // Exactly one block follows, and it is the body paragraph: the block is consumed whole.
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[1], p("Body."));
}

#[test]
fn front_matter_reads_every_line_with_a_separator() {
    // The scan is flat: an indented child with its own colon is a field too, a line with an empty
    // key is skipped, and a value keeps every colon after the first.
    assert_eq!(front_matter(&first("---\nk: v\n  child: 1\n---\n")), [field("k", "v"), field("child", "1")]);
    assert_eq!(front_matter(&first("---\n: x\nk: v\n---\n")), [field("k", "v")]);
    assert_eq!(front_matter(&first("---\ntime: 12:30:00\n---\n")), [field("time", "12:30:00")]);
    // The closer may be the last line of the document.
    assert_eq!(parse("---\ntitle: A\n---"), [MarkdownBlock::front_matter(vec![field("title", "A")])]);
}

#[test]
fn front_matter_keeps_duplicate_keys_and_their_order() {
    // A record of what the author wrote, not a dictionary.
    assert_eq!(front_matter(&first("---\ntag: a\ntag: b\n---\n\nBody.")), [field("tag", "a"), field("tag", "b")]);
    assert_eq!(parser::front_matter("---\na: 1\na: 2\n---\n"), [field("a", "1"), field("a", "2")]);
}

#[test]
fn front_matter_strips_one_layer_of_matching_quotes() {
    // `"` is tried before `'`; both ends must carry the same quote; a lone quote is content.
    assert_eq!(
        front_matter(&first("---\nt: 'x'\nu: \"y'\nv: '\n---\n\nb")),
        [field("t", "x"), field("u", "\"y'"), field("v", "'")]
    );
}

#[test]
fn front_matter_accepts_dots_as_a_yaml_closer() {
    assert_eq!(parser::front_matter("---\ntitle: A\n...\n\nbody"), [field("title", "A")]);
}

#[test]
fn front_matter_accessor() {
    assert_eq!(parser::front_matter("---\nauthor: Ann\n---\n\nHi."), [field("author", "Ann")]);
    // Empty, not null — the Swift contract; the VS Code port returns null and is the odd one out.
    assert!(parser::front_matter("# Plain\n\nNo metadata.").is_empty());
}

// MARK: - Footnote definitions

#[test]
fn footnote_definition_is_parsed_and_not_drawn_in_place() {
    let blocks = parse("Text[^a].\n\n[^a]: The note.");
    assert_eq!(blocks.last().unwrap(), &fd("a", "The note."));
}

#[test]
fn footnote_definition_absorbs_wrapped_lines() {
    let blocks = parse("X[^a].\n\n[^a]: first line\n  second line\n\nAfter.");
    assert_eq!(blocks[1], fd("a", "first line second line"));
    assert_eq!(blocks.last().unwrap(), &p("After."));
    // Each absorbed line is WS-trimmed before the joining space.
    assert_eq!(footnote(&first(&format!("[^a]: n\n{NBSP} more{NBSP} "))).1, "n more");
}

#[test]
fn consecutive_footnote_definitions_are_separate() {
    assert_eq!(parse("[^a]: one\n[^b]: two"), [fd("a", "one"), fd("b", "two")]);
}

#[test]
fn footnote_continuation_breaks_on_a_list_item_but_not_on_a_table() {
    // The footnote loop carries the footnote check and the list check and no table lookahead.
    assert_eq!(kinds("[^a]: note\n- item"), [K::FootnoteDefinition, K::List]);
    assert_eq!(single("[^a]: note\n| a |\n| --- |"), fd("a", "note | a | | --- |"));
    assert_eq!(kinds("[^a]: n\n# h"), [K::FootnoteDefinition, K::Heading]);
    assert_eq!(kinds("[^a]: n\n> q"), [K::FootnoteDefinition, K::Quote]);
    assert_eq!(kinds("[^a]: n\n---"), [K::FootnoteDefinition, K::ThematicBreak]);
    assert_eq!(kinds("[^a]: n\n\\pagebreak"), [K::FootnoteDefinition, K::PageBreak]);
    assert_eq!(kinds("[^a]: n\n<!-- c -->"), [K::FootnoteDefinition]);
    assert_eq!(kinds("[^a]: n\n```\nx\n```"), [K::FootnoteDefinition, K::CodeBlock]);
    // The paragraph loop has no footnote check: a definition cannot interrupt prose.
    assert_eq!(parse("p\n[^a]: n"), [p("p\n[^a]: n")]);
}

#[test]
fn footnote_identifier_character_set() {
    assert_eq!(parser::parse_footnote_definition("[^a-1_B]: ok"), Some(("a-1_B", "ok")));
    assert_eq!(parser::parse_footnote_definition("[^a-]: x"), Some(("a-", "x")));
    assert_eq!(parser::parse_footnote_definition("[^1]: x"), Some(("1", "x")));
    // Non-ASCII is rejected: the renderer's reference pattern is ASCII-only, so a Unicode
    // identifier would be a definition no reference could ever name.
    for bad in [
        "[^café]: no",
        "[^сн]: no",
        "[^two words]: no",
        "[^a.b]: no",
        "[^]: no",
        "[a]: not a footnote",
        "[^a] no colon",
        "[^a]]: x",
        "[^ a]: x",
    ] {
        assert_eq!(parser::parse_footnote_definition(bad), None, "{bad:?}");
    }
}

#[test]
fn footnote_definition_text_is_trimmed_with_foundation_whitespace() {
    assert_eq!(parser::parse_footnote_definition("[^a]:x"), Some(("a", "x")));
    assert_eq!(parser::parse_footnote_definition("[^a]:  x  y  "), Some(("a", "x  y")));
    assert_eq!(parser::parse_footnote_definition("  [^a]:"), Some(("a", "")));
    assert_eq!(parser::parse_footnote_definition(&format!("[^a]:{NBSP}note")), Some(("a", "note")));
}

#[test]
fn unicode_spacing_and_word_boundaries_agree_across_platforms() {
    // Padded with a non-breaking space, a fence was metadata on Apple and a paragraph on Android.
    // The inline half of the Swift test belongs to the HTML writer.
    assert_eq!(parser::front_matter(&format!("---\ntitle: A\n---{NBSP}\n\nbody")), [field("title", "A")]);
    assert!(parser::parse_footnote_definition(&format!("{NBSP}[^a]: note")).is_some());
    assert_eq!(parser::parse_footnote_definition(&format!("[^a]: note{NBSP}")).map(|d| d.1), Some("note"));
}

// MARK: - Page breaks

#[test]
fn page_break_parses() {
    let blocks = parse("before\n\n\\newpage\n\nafter");
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[1], MarkdownBlock::PageBreak);
    assert_eq!(kinds("\\pagebreak"), [K::PageBreak]);
}

#[test]
fn page_break_variant_interrupts_paragraph() {
    let blocks = parse("line one\n\\pagebreak\nline two");
    assert_eq!(blocks.iter().map(MarkdownBlock::kind).collect::<Vec<_>>(), [K::Paragraph, K::PageBreak, K::Paragraph]);
}

#[test]
fn page_break_needs_the_line_to_itself() {
    assert_eq!(parse("\\newpage now"), [p("\\newpage now")]);
}

// MARK: - Author notes and HTML comments

#[test]
fn note_comment_becomes_note_block() {
    assert_eq!(parse("<!-- note: check the intro -->"), [MarkdownBlock::note("check the intro")]);
}

#[test]
fn note_prefix_is_case_folded() {
    assert_eq!(note(&first("<!-- NOTE: shouty -->")), "shouty");
    assert_eq!(note(&first("<!-- Note: titled -->")), "titled");
}

#[test]
fn plain_comment_is_dropped() {
    assert_eq!(kinds("a\n\n<!-- just a comment -->\n\nb"), [K::Paragraph, K::Paragraph]);
    assert!(parse("<!-- x -->").is_empty());
    // A comment interrupts a paragraph like any other block start.
    assert_eq!(parse("p\n<!-- c -->\nq"), [p("p"), p("q")]);
}

#[test]
fn multiline_note() {
    assert_eq!(single("<!-- note: first\nsecond -->"), MarkdownBlock::note("first\nsecond"));
}

#[test]
fn empty_note_is_still_a_note() {
    assert_eq!(parse("<!-- note:-->"), [MarkdownBlock::note("")]);
}

#[test]
fn text_after_the_closing_marker_is_discarded() {
    // Known wart: the whole closing line is consumed.
    assert!(parse("<!-- c --> visible text").is_empty());
}

#[test]
fn unclosed_comment_swallows_to_the_end() {
    assert_eq!(parse("<!-- note: open\nmore"), [MarkdownBlock::note("open\nmore")]);
    assert!(parse("<!-- never closed\n# Swallowed").is_empty());
}

#[test]
fn comment_start_allows_spaces_only() {
    // A tab before `<!--` disqualifies the line: the indentation drop is U+0020 alone.
    assert_eq!(parse("\t<!-- x -->"), [p("\t<!-- x -->")]);
    assert_eq!(parse("   <!-- note: x -->"), [MarkdownBlock::note("x")]);
}

// MARK: - Mixed document

#[test]
fn mixed_document_block_sequence() {
    let kinds = kinds(&lines(&[
        "# Title",
        "",
        "Intro paragraph.",
        "",
        "- one",
        "- two",
        "",
        "> a quote",
        "",
        "```",
        "code",
        "```",
        "",
        "---",
    ]));
    assert_eq!(kinds, [K::Heading, K::Paragraph, K::List, K::Quote, K::CodeBlock, K::ThematicBreak]);
}

#[test]
fn paragraph_breaks_on_every_block_start_except_table_and_footnote() {
    assert_eq!(kinds("p\n- a"), [K::Paragraph, K::List]);
    assert_eq!(kinds("p\n# h"), [K::Paragraph, K::Heading]);
    assert_eq!(kinds("p\n> q"), [K::Paragraph, K::Quote]);
    assert_eq!(kinds("p\n```\nx\n```"), [K::Paragraph, K::CodeBlock]);
}

// MARK: - Scalar-exact block delimiters
//
// Every expected value is what the Kotlin port already produced and what the Swift now produces
// through ScalarText. A code-unit or byte scanner passes these by construction; they stay because
// they pin outputs, not mechanisms.

#[test]
fn parser_list_marker_survives_a_mark_on_its_space() {
    for mark in MARKS {
        let (ordered, items) = list(&first(&format!("- {mark}[draft]")));
        assert!(!ordered);
        assert_eq!(items.iter().map(|i| i.text.clone()).collect::<Vec<_>>(), [format!("{mark}[draft]")]);
    }
}

#[test]
fn parser_fence_survives_a_mark_on_its_opening_run() {
    for mark in MARKS {
        assert_eq!(first(&format!("```{mark}js\ncode()\n```")), cb(Some(&format!("{mark}js")), "code()"));
    }
}

#[test]
fn parser_table_row_survives_a_mark_on_its_only_pipe() {
    for mark in MARKS {
        let blocks = parse(&format!("a | b\n--- | ---\n1 |{mark} 2"));
        assert_eq!(blocks.len(), 1);
        assert_eq!(table(&blocks[0]).2, rows(&[&["1", &format!("{mark} 2")]]));
    }
}

#[test]
fn parser_footnote_definition_survives_a_mark_on_its_colon() {
    for mark in MARKS {
        assert_eq!(first(&format!("[^a]:{mark} the note")), fd("a", &format!("{mark} the note")));
        let line = format!("[^a]:{mark} the note");
        let expected = format!("{mark} the note");
        assert_eq!(parser::parse_footnote_definition(&line), Some(("a", expected.as_str())));
    }
}

#[test]
fn parser_quote_marker_survives_a_mark_after_it() {
    for mark in MARKS {
        assert_eq!(quote(&first(&format!(">{mark} quoted"))), [p(&format!("{mark} quoted"))]);
        // The stripper eats `>` and exactly one space; the author's mark stays.
        assert_eq!(quote(&first(&format!("> {mark}text"))), [p(&format!("{mark}text"))]);
    }
}

#[test]
fn parser_heading_survives_a_mark_on_its_marker_space() {
    for mark in MARKS {
        assert_eq!(first(&format!("# {mark}Heading")), h(1, &format!("{mark}Heading")));
        assert_eq!(
            parser::outline(&format!("# {mark}Heading")).iter().map(|e| e.text.clone()).collect::<Vec<_>>(),
            [format!("{mark}Heading")]
        );
    }
}

#[test]
fn parser_comment_end_survives_a_mark_after_it() {
    // The loudest of them: a mark on the closing `-->` left the comment open on Apple and the rest
    // of the document was swallowed into it.
    for mark in MARKS {
        assert_eq!(
            parse(&format!("<!-- note: private -->{mark}\n\nafter")),
            [MarkdownBlock::note("private"), p("after")]
        );
        assert_eq!(
            parser::outline(&format!("<!-- x -->{mark}\n\n# After"))
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            ["After"]
        );
        assert_eq!(
            parser::notes(&format!("<!-- note: n -->{mark}")).iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            ["n"]
        );
        // A mark right after `<!--` still opens a comment; it is plain, so only `after` remains.
        assert_eq!(parse(&format!("<!--{mark} hidden -->\n\nafter")), [p("after")]);
    }
}

#[test]
fn parser_front_matter_field_survives_a_mark_on_its_separator() {
    for mark in MARKS {
        let matter = front_matter(&first(&format!("---\ntitle: T\nauthor:{mark} A\n---\n\nbody")));
        assert_eq!(matter.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["title", "author"]);
        assert_eq!(matter.iter().map(|f| f.value.clone()).collect::<Vec<_>>(), ["T".to_string(), format!("{mark} A")]);
        // Quote stripping counts units, so the mark behind the opening quote survives.
        assert_eq!(
            front_matter(&first(&format!("---\ntitle: \"{mark}Q\"\n---\n\nbody"))),
            [field("title", &format!("{mark}Q"))]
        );
    }
}

#[test]
fn parser_task_box_is_scalar_exact() {
    for mark in MARKS {
        let items = items(&first(&format!("- [ ] {mark}task")));
        assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), [Some(false)]);
        assert_eq!(items.iter().map(|i| i.text.clone()).collect::<Vec<_>>(), [format!("{mark}task")]);
    }
}

// MARK: - parse_with_lines

#[test]
fn parse_with_lines_is_the_same_walk_as_parse() {
    // Holds by construction — both are projections of one scanner — and is asserted over the whole
    // fixture corpus in the test-data suite.
    let source = lines(&[
        "---",
        "title: T",
        "---",
        "",
        "# Heading",
        "",
        "Paragraph.",
        "",
        "- item",
        "",
        "> quoted",
        "",
        "```js",
        "code()",
        "```",
        "",
        "| a | b |",
        "| --- | --- |",
        "| 1 | 2 |",
        "",
        "---",
        "",
        "\\newpage",
        "",
        "<!-- note: n -->",
        "",
        "[^a]: note",
    ]);
    assert_eq!(
        parser::parse(&source),
        parser::parse_with_lines(&source).into_iter().map(|p| p.block).collect::<Vec<_>>()
    );
}

#[test]
fn parse_with_lines_records_the_line_each_top_level_block_started_on() {
    let placed =
        parser::parse_with_lines(&lines(&["# Heading", "", "Paragraph", "continued", "", "```", "code", "```"]));
    assert_eq!(placed.iter().map(|p| p.line).collect::<Vec<_>>(), [0, 2, 5]);
}

#[test]
fn parse_with_lines_places_front_matter_on_line_zero() {
    let placed = parser::parse_with_lines(&lines(&["---", "title: T", "---", "", "# H"]));
    assert_eq!(
        placed.iter().map(|p| (p.block.kind(), p.line)).collect::<Vec<_>>(),
        [(K::FrontMatter, 0), (K::Heading, 4)]
    );
}

#[test]
fn parse_with_lines_gives_a_setext_heading_the_line_of_its_text() {
    let placed = parser::parse_with_lines(&lines(&["Intro", "", "Title", "==="]));
    assert_eq!(
        placed.iter().map(|p| (p.block.kind(), p.line)).collect::<Vec<_>>(),
        [(K::Paragraph, 0), (K::Heading, 2)]
    );
    // Blocks nested in a quote carry no line; the quote itself does.
    assert_eq!(
        parser::parse_with_lines("\n> q\n\np").iter().map(|p| (p.block.kind(), p.line)).collect::<Vec<_>>(),
        [(K::Quote, 1), (K::Paragraph, 3)]
    );
}

// MARK: - Raw diagram documents

#[test]
fn raw_plant_uml_document_is_recognised() {
    for source in [
        "@startuml\nAlice -> Bob: hi\n@enduml\n",
        "' header comment\n\n@startmindmap\n* root\n@endmindmap",
        "   \n@startuml\n@enduml",
        "\r\n@startuml\r\nA -> B\r\n@enduml\r\n",
        "' note\r\n\r\n@startmindmap\r\n* r\r\n@endmindmap",
        "' note\r\r@startuml\r@enduml",
        // The probe splits on the wide newline set, so NEL and LS end lines here.
        "' c\u{0085}@startuml\u{0085}@enduml",
        "\u{2028}@startjson\u{2028}{}\u{2028}@endjson",
    ] {
        assert!(parser::is_raw_plant_uml(source), "{source:?}");
    }
    // The first qualifying line decides, so prose that merely mentions the opener stays Markdown.
    assert!(!parser::is_raw_plant_uml("# Title\n\nSome prose about @startuml in passing."));
    assert!(!parser::is_raw_plant_uml(""));
    assert!(!parser::is_raw_plant_uml("' only a comment"));
}

#[test]
fn raw_graphviz_document_matches_dots_grammar() {
    // `graph` is an ordinary English word, so the opener is matched against
    // `[strict] (graph | digraph) [ID] '{'` rather than by prefix.
    for yes in [
        "digraph { a -> b }",
        "graph {}",
        "strict digraph G {\n}",
        "DiGraph Foo {\n}",
        "digraph{a}",
        "digraph\n{\n  a\n}",
        "// generated\n\n/* by hand */\ndigraph { a }",
        "digraph \"my graph\" {\n}",
        "digraph cafe\u{0301} {\n}",
        "digraph café {\n}",
        "digraph Ⅹ {\n}",
        "digraph \u{1D518} {\n}",
        "digraph my_graph2 {",
        "// generated\u{0085}digraph G {\u{0085}}",
        "\r\ndigraph G {\r\n  a -> b;\r\n}\r\n",
        "// generated\r\ndigraph G {\r\n}\r\n",
        "  strict graph\tG\t{",
    ] {
        assert!(parser::is_raw_graphviz(yes), "{yes:?}");
    }
    for no in [
        // A `#` line is deliberately not a DOT comment: every Markdown heading starts with one.
        "# Notes\n\ngraph { the mental model }",
        "graph theory is a branch of maths.\n\nSee $\\frac{a}{b}$.",
        "digraph models are useful { in theory }",
        "graphviz is a fine tool { see }",
        "digraphs are a topic { here }",
        "digraph without a brace",
        "# Title\n\nSome prose about digraph { } in passing.",
        "digraph \"unterminated {",
        "digraph a b {",
        // `strict` must be followed by U+0020: the keyword test is a literal prefix.
        "strict\tgraph {",
        "",
    ] {
        assert!(!parser::is_raw_graphviz(no), "{no:?}");
    }
}

#[test]
fn raw_diagram_line_splitter_is_the_wide_newline_set() {
    assert_eq!(parser::lines("a\nb\r\nc\rd\u{0085}e\u{2028}f\u{2029}g\n"), ["a", "b", "c", "d", "e", "f", "g", ""]);
    assert_eq!(parser::lines("a\u{000B}b\u{000C}c"), ["a", "b", "c"]);
}
