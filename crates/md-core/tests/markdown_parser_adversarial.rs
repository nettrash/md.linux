//! Port of md.win `tests/Md.Core.Tests/ParserAdversarialTests.cs`: adversarial parity tests added
//! by review. Every expected value was produced by the real md.macOS parser —
//! `MarkdownParser.swift` + `ScalarText.swift` compiled into a dump oracle and run on these exact
//! inputs — never by reading a port's output back. Each test names the defect or the surviving
//! mutant that motivated it.

mod markdown_support;

use std::sync::mpsc;
use std::time::Duration;

use markdown_support::{MARKS, fd, field, h, item, p};
use md_core::markdown::{BlockKind, ColumnAlignment, DelimitedTable, MarkdownBlock, NoteEntry, OutlineEntry, parser};

use BlockKind as K;

fn kinds(blocks: &[MarkdownBlock]) -> Vec<BlockKind> {
    blocks.iter().map(MarkdownBlock::kind).collect()
}

fn single(blocks: Vec<MarkdownBlock>) -> MarkdownBlock {
    assert_eq!(blocks.len(), 1, "expected one block, got {blocks:?}");
    blocks.into_iter().next().unwrap()
}

/// The md.win copy under review shipped with mutant M36 live (`line.Length == 0` as the blank-line
/// test). Every other branch refuses a whitespace-only line, the paragraph loop breaks before
/// appending, the cursor never moves, and `parse("a\n   \nb")` spun forever. Swift trims the WS
/// set: such a line is a separator. Bounded so a regression fails instead of hanging the run.
#[test]
fn whitespace_only_line_is_a_blank_line_and_never_hangs_the_parser() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(parser::parse("a\n   \nb\n\t\nc\n\u{200B}\nd\n\u{00A0}\ne\n\u{3000}\nf\n"));
    });
    let blocks = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("parser did not return: whitespace-only line loops forever");
    assert_eq!(blocks, [p("a"), p("b"), p("c"), p("d"), p("e"), p("f")]);

    // The same separator inside every container: it ends a list, splits a quote's paragraphs, ends
    // a footnote definition, and ends a table's rows (the next row becomes prose).
    assert_eq!(kinds(&parser::parse("- a\n   \n- b\n")), [K::List, K::List]);
    let quote = single(parser::parse("> a\n>    \n> b\n"));
    assert_eq!(quote, MarkdownBlock::quote(vec![p("a"), p("b")]));
    assert_eq!(parser::parse("[^a]: x\n \t \ny\n"), [fd("a", "x"), p("y")]);
    let table = parser::parse("a | b\n--- | ---\n1 | 2\n   \n3 | 4\n");
    assert_eq!(kinds(&table), [K::Table, K::Paragraph]);
    assert_eq!(table[1], p("3 | 4"));
    // A document of only spaces or only a ZWSP parses to nothing, like the empty document.
    assert!(parser::parse("   ").is_empty());
    assert!(parser::parse("\u{200B}").is_empty());
}

/// Survivor K05: the front-matter blank guard compared `lines[1].Length`; a line of spaces
/// straight after the opener must defeat front matter exactly as an empty line does — the `---` is
/// then a rule and `title: A` a setext H2 (which the outline also lists).
#[test]
fn front_matter_blank_guard_uses_the_whitespace_set() {
    let blocks = parser::parse("---\n   \ntitle: A\n---\n\nbody\n");
    assert_eq!(blocks, [MarkdownBlock::ThematicBreak, h(2, "title: A"), p("body")]);
    assert_eq!(parser::outline("---\n   \ntitle: A\n---\n\nbody\n"), [OutlineEntry::new(2, "title: A", "title-a", 2)]);
    assert!(parser::front_matter("---\n   \ntitle: A\n---\n\nbody\n").is_empty());
    // A ZWSP-only line is blank to the WS set as well.
    assert_eq!(parser::parse("---\n\u{200B}\ntitle: A\n---\nbody\n")[0].kind(), K::ThematicBreak);
    // Whereas a document that is nothing but front matter is one block, closed or not by EOF.
    assert_eq!(parser::parse("---\ntitle: X\n---"), [MarkdownBlock::front_matter(vec![field("title", "X")])]);
    assert_eq!(parser::parse("---\ntitle: X\n---\n\n   \n"), [MarkdownBlock::front_matter(vec![field("title", "X")])]);
    assert_eq!(parser::parse("---\ntitle: X\n"), [MarkdownBlock::ThematicBreak, p("title: X")]);
    // A closer on line 1 leaves no field, so it is never metadata.
    assert_eq!(parser::parse("---\n---\ntitle: A\n---\nbody\n")[0].kind(), K::ThematicBreak);
}

/// Survivor K06: a closing fence must carry nothing but whitespace after its run. A line that
/// starts with the fence run but goes on is code, on both fence characters; and a fence opened with
/// four backticks is not closed by three, while three is closed by four.
#[test]
fn closing_fence_must_be_bare_and_at_least_as_long() {
    let cb = MarkdownBlock::code_block;
    assert_eq!(parser::parse("```\ncode\n``` x\nmore\n```\nafter\n"), [cb(None, "code\n``` x\nmore"), p("after")]);
    assert_eq!(parser::parse("~~~js\ncode\n~~~ js\nmore\n~~~"), [cb(Some("js"), "code\n~~~ js\nmore")]);
    assert_eq!(parser::parse("````\ncode\n```\nmore\n````\nafter\n"), [cb(None, "code\n```\nmore"), p("after")]);
    assert_eq!(parser::parse("```\ncode\n````\nafter\n"), [cb(None, "code"), p("after")]);
    // A tilde fence is never closed by backticks.
    assert_eq!(parser::parse("~~~\ncode\n```\nmore\n~~~\n"), [cb(None, "code\n```\nmore")]);
    // The outline and the notes walk fences with the same rule, so a `# heading` after a
    // non-closing "``` x" line stays inside the fence for them too.
    assert!(parser::outline("```\n``` x\n# inside\n```\n").is_empty());
    assert!(parser::notes("```\n``` x\n<!-- note: inside -->\n```\n").is_empty());
}

/// Mutant K07 (a mixed `=`/`-` run accepted as an underline) was killed by the existing suite;
/// this pins the oracle's answers directly: a setext underline is all `=` or all `-`, a mixed run
/// is prose, and so the two lines stay one paragraph. Trimmed with the WS set on both sides.
#[test]
fn setext_underline_refuses_a_mixed_run() {
    assert_eq!(parser::parse("Title\n-=-\n"), [p("Title\n-=-")]);
    assert_eq!(parser::parse("Title\n=-=\n"), [p("Title\n=-=")]);
    assert_eq!(parser::parse("Title\n  ===  \n"), [h(1, "Title")]);
    assert_eq!(parser::parse("Title\n\u{00A0}===\u{00A0}\n"), [h(1, "Title")]);
    assert!(parser::outline("Title\n-=-\n").is_empty());
    // An underline straight after a list item is a rule (the item's continuation loop breaks on
    // it), while `===` is not a rule and is absorbed into the item.
    assert_eq!(kinds(&parser::parse("- a\n---\n")), [K::List, K::ThematicBreak]);
    let list = single(parser::parse("- a\n===\n"));
    assert_eq!(list, MarkdownBlock::list(false, vec![item("a ===", 0, None, None)]));
}

/// Mutant K09 (`(indent + 1) / 2`) was killed by the existing suite; this pins the odd indents and
/// the tab stops from the oracle: the level is `indent / 2` with integer division, so one and three
/// spaces round down; tabs advance to the next 4-column stop (and only in the list-marker indent).
#[test]
fn list_level_is_indent_divided_by_two_rounded_down() {
    let MarkdownBlock::List { items: odd, .. } = single(parser::parse("- a\n - b\n   - c\n     - d\n")) else {
        panic!("not a list")
    };
    assert_eq!(odd.iter().map(|i| i.level).collect::<Vec<_>>(), [0, 0, 1, 2]);
    let MarkdownBlock::List { items: tabs, .. } =
        single(parser::parse("- a\n\t- b\n  \t- c\n    - d\n \t- e\n\t\t- f\n"))
    else {
        panic!("not a list")
    };
    assert_eq!(tabs.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["a", "b", "c", "d", "e", "f"]);
    assert_eq!(tabs.iter().map(|i| i.level).collect::<Vec<_>>(), [0, 2, 2, 2, 2, 4]);
}

/// Survivor K11: a list item's continuation loop breaks on a comment start, so a note written under
/// an item is a note block (and listed by `notes`), not text glued to the item. The footnote loop
/// breaks on it as well; the plain comment is then dropped.
#[test]
fn list_and_footnote_continuation_break_on_a_comment_start() {
    assert_eq!(
        parser::parse("- item\n<!-- note: n -->\nrest\n"),
        [MarkdownBlock::list(false, vec![item("item", 0, None, None)]), MarkdownBlock::note("n"), p("rest")]
    );
    assert_eq!(parser::notes("- item\n<!-- note: n -->\nrest\n"), [NoteEntry::new("n", 1)]);
    assert_eq!(
        parser::parse("1. one\n   <!-- plain -->\nafter"),
        [MarkdownBlock::list(true, vec![item("one", 0, Some(1), None)]), p("after")]
    );
    assert_eq!(parser::parse("[^a]: x\n<!-- c -->\ny"), [fd("a", "x"), p("y")]);
    // A tab before `<!--` is not a comment start, so that line IS continuation.
    let tabbed = single(parser::parse("- item\n\t<!-- note: n -->"));
    assert_eq!(tabbed, MarkdownBlock::list(false, vec![item("item <!-- note: n -->", 0, None, None)]));
}

/// Survivor K13: the CSV walker keeps a final field that has no separator and no newline after it —
/// a one-column file without a trailing newline is still a row. Derived from the Swift
/// `if !field.isEmpty || !row.isEmpty`, which the report reproduces verbatim.
#[test]
fn delimited_last_field_without_separator_or_newline_is_a_row() {
    assert_eq!(DelimitedTable::parse_delimited("x", ','), [["x"]]);
    assert_eq!(DelimitedTable::parse_delimited("a\nb", ','), [["a"], ["b"]]);
    assert_eq!(DelimitedTable::parse_delimited("a,b\nc", ','), [vec!["a", "b"], vec!["c"]]);
    // …but a quoted empty field at the very end is dropped, as in Swift (`""` → no row).
    assert!(DelimitedTable::parse_delimited("\"\"", ',').is_empty());
    let table = DelimitedTable::from("n\n1\n2", ',').expect("a table");
    assert_eq!(table.header, ["n"]);
    assert_eq!(table.alignments, [ColumnAlignment::Trailing]);
    assert_eq!(table.rows, [["1"], ["2"]]);
}

/// The brief's remaining invented inputs, each checked against the Swift oracle: CR-only and
/// CRLF-only files, a footnote definition inside a quote, a delimiter row carrying a mark, the
/// 32/33/34-deep quote boundary, blank-separated items, and a heading whose closing hashes carry a
/// mark on the last (or an inner) hash.
#[test]
fn briefs_invented_inputs_match_the_swift_oracle() {
    // CRLF-only and CR-only documents parse as their LF twins would.
    let source = "# Title\r\n\r\n- a\r\n- b\r\n\r\n> q\r\n> r\r\n\r\n| a | b |\r\n| --- | --- |\r\n| 1 | 2 |\r\n\r\n```js\r\ncode\r\n```\r\n\r\n[^a]: note\r\n  more\r\n\r\n---\r\ntitle: x\r\n---\r\n";
    let crlf = parser::parse(source);
    // The trailing `---` / `title: x` / `---` is a rule and then a setext H2 — this is not the top
    // of the document, so it is never front matter (oracle: HR, H2|title: x).
    assert_eq!(
        kinds(&crlf),
        [K::Heading, K::List, K::Quote, K::Table, K::CodeBlock, K::FootnoteDefinition, K::ThematicBreak, K::Heading]
    );
    assert_eq!(crlf[5], fd("a", "note more"));
    assert_eq!(crlf[2], MarkdownBlock::quote(vec![p("q\nr")]));
    assert_eq!(crlf[7], h(2, "title: x"));
    // CRLF counts as one line for the outline's line numbers: the setext text sits on line 20.
    assert_eq!(
        parser::outline(source),
        [OutlineEntry::new(1, "Title", "title", 0), OutlineEntry::new(2, "title: x", "title-x", 20)]
    );
    assert_eq!(
        parser::parse("# Title\r\rpara\rline2\r\r- a\r- b\r"),
        [
            h(1, "Title"),
            p("para\nline2"),
            MarkdownBlock::list(false, vec![item("a", 0, None, None), item("b", 0, None, None)]),
        ]
    );

    // A footnote definition inside a quote, with its continuation; a bare `>` line separates.
    assert_eq!(
        parser::parse("> [^a]: note\n> more\n>\n> text[^a]\n"),
        [MarkdownBlock::quote(vec![fd("a", "note more"), p("text[^a]")])]
    );
    assert_eq!(
        parser::parse("> [^a]: note\n> - item\n"),
        [MarkdownBlock::quote(vec![fd("a", "note"), MarkdownBlock::list(false, vec![item("item", 0, None, None)])])]
    );

    // A mark anywhere in a delimiter cell makes the pair prose; a mark after a header pipe does not.
    for mark in MARKS {
        assert_eq!(
            parser::parse(&format!("a | b\n---{mark} | ---\n1 | 2\n")),
            [p(&format!("a | b\n---{mark} | ---\n1 | 2"))]
        );
        assert_eq!(
            parser::parse(&format!("a | b\n:---: | ---:{mark}\n1 | 2\n")),
            [p(&format!("a | b\n:---: | ---:{mark}\n1 | 2"))]
        );
        let MarkdownBlock::Table { header, .. } = single(parser::parse(&format!("a |{mark} b\n--- | ---\n1 | 2\n")))
        else {
            panic!("not a table")
        };
        assert_eq!(header, ["a".to_string(), format!("{mark} b")]);
    }

    // The depth cap: 32 markers nest 32 deep with `x`; 33 nest 33 deep with `x`; 34 nest 33 deep
    // and the innermost paragraph keeps the one marker the cap refused to recurse into.
    fn descend(source: &str) -> (usize, MarkdownBlock) {
        let mut depth = 0;
        let mut current = single(parser::parse(source));
        while let MarkdownBlock::Quote { blocks } = current {
            depth += 1;
            current = single(blocks);
        }
        (depth, current)
    }
    assert_eq!(descend(&format!("{} x\n", ">".repeat(32))), (32, p("x")));
    assert_eq!(descend(&format!("{} x\n", ">".repeat(33))), (33, p("x")));
    assert_eq!(descend(&format!("{} x\n", ">".repeat(34))), (33, p("> x")));

    // Blank-separated items are separate lists; a run's ordered flag is per run.
    assert_eq!(
        parser::parse("- a\n\n- b\n\n1. c\n"),
        [
            MarkdownBlock::list(false, vec![item("a", 0, None, None)]),
            MarkdownBlock::list(false, vec![item("b", 0, None, None)]),
            MarkdownBlock::list(true, vec![item("c", 0, Some(1), None)]),
        ]
    );

    // Closing hashes: a mark on the last hash means the text does not end in `#`, so nothing is
    // stripped; a mark on an inner hash leaves a one-hash run preceded by the mark, not a space —
    // also kept. Only a run preceded by U+0020 / U+0009 is stripped, and a run preceded by NBSP is
    // not.
    assert_eq!(parser::parse("# Title ##\u{0301}\n"), [h(1, "Title ##\u{0301}")]);
    assert_eq!(parser::parse("# Title #\u{0301}#\n"), [h(1, "Title #\u{0301}#")]);
    assert_eq!(parser::parse("# Title\t##\n"), [h(1, "Title")]);
    assert_eq!(parser::parse("# Title\u{00A0}##\n"), [h(1, "Title\u{00A0}##")]);
    assert_eq!(parser::parse("# ##\n"), [h(1, "")]);
    assert_eq!(
        parser::outline("# Title ##\u{0301}\n"),
        [OutlineEntry::new(1, "Title ##\u{0301}", "title-\u{0301}", 0)]
    );

    // Slugs: no Final_Sigma, full mapping for U+0130, enclosed alphabetics kept, marks kept,
    // ZWJ/ZWSP dropped, the bare-slug counter producing GitHub's duplicate id.
    let slugs: Vec<String> = parser::outline(
        "# \u{039F}\u{0394}\u{039F}\u{03A3} \u{03A3}\u{0391}\u{03A3} \u{03A3}\n# \u{0130}stanbul\n# \u{01C5} \u{1E9E}\n# \u{24B6} \u{1F130} \u{1F150} \u{1F170} \u{24D0}\n# \u{216B} \u{00BD} \u{0663} \u{00B2}\n# \u{1D400} a\u{200D}b c\u{200B}d\n# Same\n# Same 1\n# Same\n# same-1\n",
    )
    .into_iter()
    .map(|e| e.slug)
    .collect();
    assert_eq!(
        slugs,
        [
            "\u{03BF}\u{03B4}\u{03BF}\u{03C3}-\u{03C3}\u{03B1}\u{03C3}-\u{03C3}",
            "i\u{0307}stanbul",
            "\u{01C6}-\u{00DF}",
            "\u{24D0}-\u{1F130}-\u{1F150}-\u{1F170}-\u{24D0}",
            "\u{217B}-\u{00BD}-\u{0663}-\u{00B2}",
            "\u{1D400}-ab-cd",
            "same",
            "same-1",
            "same-1",
            "same-1-1",
        ]
    );

    // Ordinals: nine digits parse, ten do not (the line becomes a continuation), leading zeros go.
    let MarkdownBlock::List { items: ordinals, .. } =
        single(parser::parse("123456789. nine\n1234567890. ten\n0. zero\n007) seven\n"))
    else {
        panic!("not a list")
    };
    assert_eq!(ordinals.iter().map(|i| i.ordinal).collect::<Vec<_>>(), [Some(123_456_789), Some(0), Some(7)]);
    assert_eq!(ordinals[0].text, "nine 1234567890. ten");

    // Footnote ids of one `-` / `_`, an empty text, a tab before the text, and `[^ a]` refused (so
    // it is absorbed as continuation).
    assert_eq!(
        parser::parse("[^-]: dash\n[^_]: underscore\n[^a]:\n[^a]:\t x\n[^ a]: sp\n"),
        [fd("-", "dash"), fd("_", "underscore"), fd("a", ""), fd("a", "x [^ a]: sp")]
    );
}
