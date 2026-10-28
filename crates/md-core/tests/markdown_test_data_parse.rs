//! Port of md.win `tests/Md.Core.Tests/TestDataParseTests.cs` (plus `SmokeTests.cs`): the
//! parse-side half of the fixture-driven suite over the shared corpus (mirrored byte-identically in
//! md, md.macOS, md.Android, md.vscode and md.win, except `test.md`'s platform lines — this port
//! carries md.win's). Every fixture must parse, and each per-feature file must carry the construct
//! its name promises, so a fixture edit that loses a feature fails here rather than silently
//! weakening the corpus. The HTML-side assertions come with the HTML writer's suites.

mod common;

use std::collections::{HashMap, HashSet};

use md_core::markdown::{BlockKind, ColumnAlignment, MarkdownBlock, parser};
use md_core::text::whitespace;

const FIXTURE_NAMES: [&str; 16] = [
    "blockquotes",
    "code",
    "edge-cases",
    "headings",
    "images",
    "inline",
    "lists",
    "math",
    "mermaid",
    "notes",
    "outline",
    "page-breaks",
    "plantuml",
    "tables",
    "test",
    "thematic-breaks",
];

const EXAMPLE_NAMES: [&str; 9] = [
    "01-Welcome",
    "02-Formatting",
    "03-Tables",
    "04-Code",
    "05-Images",
    "06-Math",
    "07-Diagrams",
    "08-Plots",
    "09-Writer Tools",
];

fn load(name: &str) -> String {
    common::fixture_text(&format!("testdata/{name}.md"))
}

fn load_example(name: &str) -> String {
    common::fixture_text(&format!("examples/{name}.md"))
}

/// The `.md` file stems in a fixture directory, ordinally sorted.
fn md_stems(directory: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(common::fixture_path(directory))
        .unwrap_or_else(|e| panic!("{directory}: {e}"))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .map(|path| path.file_stem().unwrap().to_str().unwrap().to_string())
        .collect();
    names.sort();
    names
}

// MARK: - SmokeTests

#[test]
fn fixtures_are_on_disk() {
    let root = common::fixture_path("");
    assert!(root.is_dir(), "{}", root.display());
    assert_eq!(md_stems("testdata").len(), 16);
    assert_eq!(md_stems("examples").len(), 9);
}

// MARK: - TestDataParseTests

#[test]
fn corpus_is_complete() {
    assert_eq!(md_stems("testdata"), FIXTURE_NAMES);
    // Every fixture ends with exactly one LF, as the corpus promises.
    for name in FIXTURE_NAMES {
        let source = load(name);
        assert!(source.ends_with('\n'), "{name}");
        assert!(!source.ends_with("\n\n"), "{name}");
        assert!(!source.contains('\r'), "{name}");
    }
}

#[test]
fn every_fixture_parses() {
    for name in FIXTURE_NAMES {
        let source = load(name);
        let blocks = parser::parse(&source);
        assert!(!blocks.is_empty(), "{name}");
        // The one invariant the VS Code port asserts over the whole corpus: `parse` and
        // `parse_with_lines` are projections of one scanner.
        let placed = parser::parse_with_lines(&source);
        assert_eq!(blocks, placed.iter().map(|p| p.block.clone()).collect::<Vec<_>>(), "{name}");
        // Lines are 0-based, ascending, and each one starts a non-blank line.
        let lines = whitespace::normalized_lines(&source);
        let mut previous: Option<usize> = None;
        for item in &placed {
            assert!(previous.is_none_or(|p| item.line > p), "{name}: block on line {} after {previous:?}", item.line);
            assert!(item.line < lines.len(), "{name}: line {} out of range", item.line);
            assert_ne!(whitespace::trim_ws(lines[item.line]), "", "{name}: line {}", item.line);
            previous = Some(item.line);
        }
    }
}

#[test]
fn headings_fixture_covers_all_six_levels() {
    let blocks = parser::parse(&load("headings"));
    let levels: HashSet<usize> = blocks
        .iter()
        .filter_map(|b| match b {
            MarkdownBlock::Heading { level, .. } => Some(*level),
            _ => None,
        })
        .collect();
    assert!(levels.is_superset(&HashSet::from([1, 2, 3, 4, 5, 6])));
    // The fixture's own promises: seven hashes and a missing space are paragraphs, closing hashes
    // go, `C#` and `F#` keep theirs, and both setext levels appear.
    assert!(blocks.iter().any(|b| matches!(b, MarkdownBlock::Paragraph { text } if text.starts_with("####### "))));
    assert!(blocks.iter().any(|b| matches!(b, MarkdownBlock::Paragraph { text } if text.starts_with("#No space"))));
    assert!(blocks.contains(&MarkdownBlock::heading(2, "Closing hashes are stripped")));
    assert!(blocks.contains(&MarkdownBlock::heading(3, "C# and F# keep the hash glued to the word")));
    assert!(blocks.contains(&MarkdownBlock::heading(1, "Setext level 1")));
    assert!(blocks.contains(&MarkdownBlock::heading(2, "Setext level 2")));
}

#[test]
fn tables_fixture_parses_tables() {
    // Three real tables; the delimiter-less pair of lines is not one.
    let blocks = parser::parse(&load("tables"));
    let tables: Vec<_> = blocks
        .iter()
        .filter_map(|b| match b {
            MarkdownBlock::Table { header, alignments, .. } => Some((header, alignments)),
            _ => None,
        })
        .collect();
    assert_eq!(tables.len(), 3);
    assert!(
        tables
            .iter()
            .any(|(_, a)| a[..] == [ColumnAlignment::Leading, ColumnAlignment::Center, ColumnAlignment::Trailing])
    );
    assert!(tables.iter().any(|(h, _)| h.iter().any(|cell| cell == "Cell with | escaped pipe")));
    assert!(blocks.iter().any(|b| matches!(b, MarkdownBlock::Paragraph { text } if text.contains("| a | b |"))));
}

#[test]
fn lists_fixture_carries_ordered_and_unordered() {
    let lists: Vec<_> = parser::parse(&load("lists"))
        .into_iter()
        .filter_map(|b| match b {
            MarkdownBlock::List { ordered, items } => Some((ordered, items)),
            _ => None,
        })
        .collect();
    assert!(lists.iter().any(|(ordered, _)| *ordered));
    assert!(lists.iter().any(|(ordered, _)| !*ordered));
    // Nesting, a lazily continued line and task boxes are all in the file.
    assert!(lists.iter().any(|(_, items)| items.iter().any(|i| i.level > 0)));
    assert!(lists.iter().any(|(_, items)| items.iter().any(|i| i.text.contains("lazily continued"))));
    assert!(
        lists
            .iter()
            .any(|(_, items)| items.iter().any(|i| i.task == Some(true)) && items.iter().any(|i| i.task == Some(false)))
    );
}

#[test]
fn page_breaks_fixture_carries_both_spellings() {
    let source = load("page-breaks");
    assert_eq!(parser::parse(&source).iter().filter(|b| b.kind() == BlockKind::PageBreak).count(), 2);
    assert!(source.contains("\\newpage"));
    assert!(source.contains("\\pagebreak"));
}

#[test]
fn notes_fixture_has_two_notes_and_one_plain_comment() {
    let source = load("notes");
    let notes = parser::notes(&source);
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().any(|n| n.text.contains("private author note")));
    assert!(notes.iter().any(|n| n.text.contains("multi-line note\nspanning two source lines")));
    let blocks = parser::parse(&source);
    assert_eq!(blocks.iter().filter(|b| b.kind() == BlockKind::Note).count(), 2);
    assert!(!blocks.iter().any(|b| matches!(b, MarkdownBlock::Paragraph { text } if text.contains("plain comment"))));
    assert!(blocks.contains(&MarkdownBlock::paragraph("Visible prose before the notes.")));
    assert!(blocks.contains(&MarkdownBlock::paragraph("Visible prose after the notes.")));
}

#[test]
fn outline_fixture_dedupes_and_slugs() {
    let outline = parser::outline(&load("outline"));
    let slugs: Vec<&str> = outline.iter().map(|e| e.slug.as_str()).collect();
    assert!(slugs.contains(&"section"));
    assert!(slugs.contains(&"section-1"));
    assert!(slugs.contains(&"c--f"));
    assert!(!outline.iter().any(|e| e.text.contains("not a heading")));
    assert_eq!(outline.last().unwrap().text, "Setext also counts");
    // The outline's slugs are the parser's headings' slugs, walked in document order.
    let mut used = HashMap::new();
    let expected: Vec<String> = parser::parse(&load("outline"))
        .iter()
        .filter_map(|b| match b {
            MarkdownBlock::Heading { text, .. } => Some(parser::slug(text, &mut used)),
            _ => None,
        })
        .collect();
    assert_eq!(expected, slugs);
}

#[test]
fn outline_and_headings_agree_across_the_corpus() {
    // Every top-level heading `parse` emits is an outline entry with the same text and level, in
    // the same order — the shared slug counter depends on it.
    let sources = FIXTURE_NAMES
        .iter()
        .map(|name| (name.to_string(), load(name)))
        .chain(EXAMPLE_NAMES.iter().map(|name| (format!("examples:{name}"), load_example(name))));
    for (name, source) in sources {
        let headings: Vec<(usize, String)> = parser::parse(&source)
            .into_iter()
            .filter_map(|b| match b {
                MarkdownBlock::Heading { level, text } => Some((level, text)),
                _ => None,
            })
            .collect();
        let outline: Vec<(usize, String)> = parser::outline(&source).into_iter().map(|e| (e.level, e.text)).collect();
        assert_eq!(headings, outline, "{name}");
    }
}

#[test]
fn rich_fixtures_carry_their_fences() {
    // The parser does not know these languages; it only has to hand them on intact, case kept.
    fn languages(source: &str) -> Vec<Option<String>> {
        parser::parse(source)
            .into_iter()
            .filter_map(|b| match b {
                MarkdownBlock::CodeBlock { language, .. } => Some(language),
                _ => None,
            })
            .collect()
    }
    assert!(languages(&load("math")).contains(&Some("math".to_string())));
    assert_eq!(languages(&load("mermaid")).iter().filter(|l| l.as_deref() == Some("mermaid")).count(), 3);
    let plantuml = languages(&load("plantuml"));
    assert_eq!(plantuml.iter().filter(|l| l.as_deref() == Some("plantuml")).count(), 2);
    assert!(plantuml.contains(&Some("puml".to_string())));
}

#[test]
fn code_fixture_strips_an_indented_fence() {
    let code: Vec<(Option<String>, String)> = parser::parse(&load("code"))
        .into_iter()
        .filter_map(|b| match b {
            MarkdownBlock::CodeBlock { language, code } => Some((language, code)),
            _ => None,
        })
        .collect();
    assert!(code.iter().any(|(l, _)| l.as_deref() == Some("swift")));
    assert!(code.iter().any(|(l, _)| l.as_deref() == Some("json")));
    assert!(code.iter().any(|(l, c)| l.as_deref() == Some("text") && c.starts_with("an indented fence")));
    assert!(code.iter().any(|(_, c)| c == "# not a heading\n- not a list\n**not bold**"));
}

#[test]
fn thematic_breaks_fixture_has_four_rules() {
    assert_eq!(
        parser::parse(&load("thematic-breaks")).iter().filter(|b| b.kind() == BlockKind::ThematicBreak).count(),
        4
    );
}

#[test]
fn blockquotes_fixture_nests() {
    let quotes: Vec<Vec<MarkdownBlock>> = parser::parse(&load("blockquotes"))
        .into_iter()
        .filter_map(|b| match b {
            MarkdownBlock::Quote { blocks } => Some(blocks),
            _ => None,
        })
        .collect();
    assert!(!quotes.is_empty());
    assert!(
        quotes.iter().any(
            |q| q.iter().any(|b| b.kind() == BlockKind::List) && q.iter().any(|b| b.kind() == BlockKind::CodeBlock)
        )
    );
    assert!(quotes.iter().any(|q| q.iter().any(|b| match b {
        MarkdownBlock::Quote { blocks: inner } => inner.iter().any(|b| b.kind() == BlockKind::Quote),
        _ => false,
    })));
}

#[test]
fn examples_are_complete_and_parse() {
    assert_eq!(md_stems("examples"), EXAMPLE_NAMES);
    for name in EXAMPLE_NAMES {
        let source = load_example(name);
        let blocks = parser::parse(&source);
        assert!(!blocks.is_empty(), "{name}");
        assert_eq!(
            blocks,
            parser::parse_with_lines(&source).into_iter().map(|p| p.block).collect::<Vec<_>>(),
            "{name}"
        );
    }
}
