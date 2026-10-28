//! The two pure halves of "export one diagram as a standalone SVG": which blocks a document offers
//! (diagrams yes, math and plain code no), and the fix-up that turns a rendered `<svg>` into a
//! standalone file. The offscreen capture in between is a five-line DOM read in the app, not
//! covered here. Port of md.win `DiagramSvgTests.cs`.
//!
//! md.win ports mdTests.swift's "Diagram -> standalone SVG" section (eight tests) plus the three
//! the Kotlin suite adds (both dimensions in percent, the 40/41 cap boundary, the stroke-width
//! guard) and the two PlotTests integration cases that pin the plot fence into this list. The rest
//! are md.win's own: the whole Graphviz alias table, the single-quoted attribute branch, the
//! no-root case, and the three places where the family's four "what is whitespace" answers could
//! have diverged.

use md_core::export::diagram_svg::{self, Diagram};
use md_core::plot;
use md_core::text::utf16::utf16_len;

const ELLIPSIS: char = '\u{2026}';
const PROLOG: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const XMLNS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";

fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

#[track_caller]
fn has(haystack: &str, needle: &str, note: &str) {
    assert!(haystack.contains(needle), "{note} expected to contain '{needle}' in: {haystack}");
}

#[track_caller]
fn lacks(haystack: &str, needle: &str, note: &str) {
    assert!(!haystack.contains(needle), "{note} expected NOT to contain '{needle}' in: {haystack}");
}

fn engines(diagrams: &[Diagram]) -> Vec<&str> {
    diagrams.iter().map(|d| d.engine.as_str()).collect()
}

fn layouts(diagrams: &[Diagram]) -> Vec<Option<&str>> {
    diagrams.iter().map(|d| d.layout.as_deref()).collect()
}

fn ordinals(diagrams: &[Diagram]) -> Vec<usize> {
    diagrams.iter().map(|d| d.ordinal).collect()
}

// MARK: - Enumeration

#[test]
fn offers_only_diagrams_in_document_order() {
    // Inline math, a math fence and a plain code block are all NOT diagrams; the three engine
    // fences are, in document order, each with the ordinal the DOM query will index it by.
    let source = lines(&[
        "Inline $a^2$ math.",
        "",
        "```math",
        "E=mc^2",
        "```",
        "",
        "```swift",
        "let x = 1",
        "```",
        "",
        "```mermaid",
        "graph TD; A-->B",
        "```",
        "",
        "```dot",
        "digraph { a -> b }",
        "```",
        "",
        "```plantuml",
        "@startuml",
        "A->B",
        "@enduml",
        "```",
    ]);
    let diagrams = diagram_svg::diagrams(&source);
    assert_eq!(engines(&diagrams), ["mermaid", "graphviz", "plantuml"]);
    assert_eq!(ordinals(&diagrams), [0, 1, 2]);
    assert_eq!(layouts(&diagrams), [None, Some("dot"), None]);
    // The label is the first non-empty source line, so two diagrams read apart in the menu.
    assert_eq!(diagrams[0].label(), "graph TD; A-->B");
    assert_eq!(diagrams[2].label(), "@startuml");
    // Source is the fence body verbatim, newlines and all (a closed fence keeps no trailing
    // newline - the parser's own documented shape).
    assert_eq!(diagrams[2].source, "@startuml\nA->B\n@enduml");
}

#[test]
fn offers_nothing_without_diagrams() {
    // Prose, a formula and plain code - nothing that renders to an <svg>.
    let none = diagram_svg::diagrams(&lines(&[
        "# Title",
        "",
        "Text $x^2$ here.",
        "",
        "```swift",
        "let y = 1",
        "```",
        "",
        "```math",
        "E=mc^2",
        "```",
    ]));
    assert!(none.is_empty());
    assert!(diagram_svg::diagrams("").is_empty());
}

#[test]
fn recurses_into_quotes_and_covers_layout_aliases() {
    // A diagram nested in a block quote keeps its place - the HTML writer renders quoted blocks in
    // line, so its container is first in the DOM - and a layout-named Graphviz fence is offered
    // with its layout.
    let diagrams = diagram_svg::diagrams(&lines(&[
        "> ```mermaid",
        "> graph TD; A-->B",
        "> ```",
        "",
        "```neato",
        "graph { a -- b }",
        "```",
    ]));
    assert_eq!(engines(&diagrams), ["mermaid", "graphviz"]);
    assert_eq!(layouts(&diagrams), [None, Some("neato")]);
    assert_eq!(ordinals(&diagrams), [0, 1]);
}

#[test]
fn raw_diagram_document_is_a_single_diagram() {
    // An opened `.puml` / `.gv` is one whole-file diagram (the writer renders it without parsing
    // Markdown), so it is exactly one entry.
    let puml = diagram_svg::diagrams(&lines(&["@startuml", "A->B", "@enduml"]));
    assert_eq!(engines(&puml), ["plantuml"]);
    assert_eq!(puml[0].ordinal, 0);
    assert_eq!(puml[0].label(), "@startuml");
    // The whole file is the source, not just its first line.
    assert_eq!(puml[0].source, "@startuml\nA->B\n@enduml");

    let dot = diagram_svg::diagrams("digraph { a -> b }");
    assert_eq!(engines(&dot), ["graphviz"]);
    assert_eq!(dot[0].layout.as_deref(), Some("dot"));
    // The default layout is not named in the menu, even for a raw document.
    assert_eq!(dot[0].menu_title, "Graphviz: digraph { a -> b }");
}

#[test]
fn every_graphviz_layout_alias_is_offered_with_its_layout_program() {
    // The alias table the HTML writer dispatches on, walked end to end: an alias this list forgets
    // is a container the DOM query still returns, and every later figure exports as the wrong one.
    let aliases = [
        ("dot", "dot"),
        ("graphviz", "dot"),
        ("gv", "dot"),
        ("neato", "neato"),
        ("circo", "circo"),
        ("fdp", "fdp"),
        ("sfdp", "sfdp"),
        ("twopi", "twopi"),
        ("osage", "osage"),
        ("patchwork", "patchwork"),
    ];
    for (fence, layout) in aliases {
        let diagrams = diagram_svg::diagrams(&lines(&[&format!("```{fence}"), "digraph { a -> b }", "```"]));
        assert_eq!(diagrams.len(), 1, "{fence}");
        let only = &diagrams[0];
        assert_eq!(only.engine, "graphviz");
        assert_eq!(only.layout.as_deref(), Some(layout));
    }
    // The fold is the HTML writer's own, so a shouted fence is a diagram on both sides of the
    // pairing or on neither.
    assert_eq!(diagram_svg::diagrams(&lines(&["```NEATO", "graph {}", "```"]))[0].engine, "graphviz");
    assert_eq!(diagram_svg::diagrams(&lines(&["```Mermaid", "graph TD; A-->B", "```"]))[0].engine, "mermaid");
    // The PlantUML aliases, and a near miss that is not one.
    for fence in ["plantuml", "puml", "plant-uml"] {
        assert_eq!(diagram_svg::diagrams(&lines(&[&format!("```{fence}"), "@startuml", "```"]))[0].engine, "plantuml");
    }
    assert!(diagram_svg::diagrams(&lines(&["```plantuml2", "@startuml", "```"])).is_empty());
}

#[test]
fn plot_fence_is_a_diagram_and_the_dom_selector_names_its_container() {
    // PlotTests: the figure is offered to the SVG export and found in the DOM. The walk and the DOM
    // query must agree about what a diagram is, or every later figure exports as the wrong one.
    let fence =
        lines(&["```plot", "x: -10..10", "y: -2..2", "title: Damped oscillation", "sin(x) * exp(-abs(x)/5)", "```"]);
    let document = lines(&[&fence, "", "```dot", "digraph {}", "```"]);
    let found = diagram_svg::diagrams(&document);
    assert_eq!(engines(&found), ["plot", "graphviz"]);
    assert_eq!(found[0].type_name(), "Plot");
    assert_eq!(found[0].menu_title, "Plot: x: -10..10");
    has(diagram_svg::DOM_SELECTOR, "div.plot", "");
    assert_eq!(diagram_svg::DOM_SELECTOR, "pre.mermaid, div.plantuml, div.graphviz, div.plot");
    // A quoted plot keeps its place, because the renderer recurses into quotes.
    assert_eq!(engines(&diagram_svg::diagrams(&lines(&["> ```plot", "> sin(x)", "> ```"]))), ["plot"]);
}

#[test]
fn plot_saves_as_a_standalone_svg_without_a_fix_up() {
    // PlotTests: the root already carries `xmlns`, `width` and `height`, so the only thing the
    // writer adds is the XML prolog.
    let file = diagram_svg::standalone_document(&plot::plot_svg("sin(x)").expect("a valid plot"));
    assert!(file.starts_with(&format!("{PROLOG}<svg xmlns=")), "{}", &file[..60]);
    has(&file, "width=\"600\" height=\"400\"", "");
}

// MARK: - Menu titles and labels

#[test]
fn menu_titles_name_engine_and_source_snippet() {
    let mermaid = Diagram::of(0, diagram_svg::MERMAID, None, "graph TD");
    assert_eq!(mermaid.type_name(), "Mermaid");
    assert_eq!(mermaid.menu_title, "Mermaid: graph TD");

    // The default `dot` layout isn't named; a non-default one is.
    let dot = Diagram::of(1, diagram_svg::GRAPHVIZ, Some("dot"), "g");
    assert_eq!(dot.type_name(), "Graphviz");
    let neato = Diagram::of(2, diagram_svg::GRAPHVIZ, Some("neato"), "");
    assert_eq!(neato.type_name(), "Graphviz (neato)");
    assert_eq!(neato.menu_title, "Graphviz (neato)"); // no label -> type only
    assert_eq!(Diagram::of(3, diagram_svg::PLANT_UML, None, "").type_name(), "PlantUML");

    // A long first line is capped so one diagram can't dwarf the menu.
    let long = diagram_svg::diagrams(&lines(&["```mermaid", &"x".repeat(100), "```"]));
    assert_eq!(long.len(), 1);
    let label = long[0].label();
    assert!(label.ends_with(ELLIPSIS));
    assert!(utf16_len(&label) <= 41, "{}", utf16_len(&label));
}

#[test]
fn first_line_label_caps_at_forty_characters() {
    // Kotlin pins the boundary exactly: 40 characters pass through whole, 41 are capped to 40 + an
    // ellipsis.
    let forty = "a".repeat(40);
    let at_forty = diagram_svg::diagrams(&lines(&["```mermaid", &forty, "```"]))[0].label();
    assert_eq!(at_forty, forty);
    lacks(&at_forty, &ELLIPSIS.to_string(), "");

    let over_by_one = diagram_svg::diagrams(&lines(&["```mermaid", &"a".repeat(41), "```"]))[0].label();
    assert_eq!(utf16_len(&over_by_one), 41); // 40 + ellipsis
    assert!(over_by_one.ends_with(ELLIPSIS));
}

#[test]
fn label_cap_counts_grapheme_clusters_as_mac_os_does() {
    // Swift caps at 40 `Character`s - grapheme clusters. TypeScript counts code points and Kotlin
    // UTF-16 units, either of which would cut this line at 20 letters. macOS is the source of
    // truth, so the cap counts clusters.
    let letter = "e\u{0301}"; // one cluster, two UTF-16 units (COMBINING ACUTE ACCENT)
    let label = diagram_svg::diagrams(&lines(&["```mermaid", &letter.repeat(41), "```"]))[0].label();
    assert_eq!(label, format!("{}{ELLIPSIS}", letter.repeat(40)));
    assert_eq!(utf16_len(&label), 81); // 40 clusters x 2 units + the ellipsis
}

#[test]
fn label_skips_blank_lines_by_the_foundation_whitespace_set() {
    // The label trim is Foundation's `.whitespaces`, which still counts U+200B ZERO WIDTH SPACE
    // (Apple's frozen tables call it Zs). `str::trim` would not, and would hand the menu an
    // invisible label.
    let label = diagram_svg::diagrams(&lines(&["```mermaid", "\u{200B}", "  real  ", "```"]))[0].label();
    assert_eq!(label, "real");

    // And the splitter is the wide newline set, not the block scanner's CR/LF one: the parser
    // hands this fence one line, and U+2028 still ends it here.
    assert_eq!(diagram_svg::diagrams(&lines(&["```mermaid", "\u{2028}first", "```"]))[0].label(), "first");
    // An all-blank source has no label at all, and then the menu shows the type.
    let blank = &diagram_svg::diagrams(&lines(&["```mermaid", "   ", "```"]))[0];
    assert_eq!(blank.label(), "");
    assert_eq!(blank.menu_title, "Mermaid");
}

// MARK: - The standalone SVG fix-up

#[test]
fn standalone_svg_resolves_mermaid_size_from_view_box() {
    // Mermaid's root is `width="100%"` with no height - unusable in a file. The standalone
    // document must carry the XML prolog, keep the SVG namespace, and take real pixel width/height
    // from the viewBox.
    let svg = concat!(
        "<svg id=\"m\" class=\"flowchart\" viewBox=\"0 0 200 100\" ",
        "style=\"max-width: 200px;\" width=\"100%\" ",
        "xmlns=\"http://www.w3.org/2000/svg\"><g></g></svg>",
    );
    let result = diagram_svg::standalone_document(svg);
    assert!(result.starts_with(PROLOG));
    has(&result, XMLNS, "");
    has(&result, "width=\"200\"", "");
    has(&result, "height=\"100\"", "");
    lacks(&result, "width=\"100%\"", "the percentage width must be gone");
}

#[test]
fn standalone_svg_resolves_percent_width_and_height() {
    // A root whose width AND height are BOTH percentages is still unsized - pins that the `%`
    // guard decides it, not the mere presence of a height attribute.
    let result = diagram_svg::standalone_document(concat!(
        "<svg viewBox=\"0 0 300 150\" width=\"100%\" height=\"100%\" ",
        "xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    ));
    has(&result, "width=\"300\"", "");
    has(&result, "height=\"150\"", "");
    lacks(&result, "100%", "neither percentage dimension may survive");
}

#[test]
fn standalone_svg_leaves_sized_root_alone_but_adds_prolog() {
    // Graphviz / PlantUML already write absolute width/height, so the viewBox must NOT overwrite
    // them - only the prolog is added.
    let svg = concat!(
        "<svg width=\"120pt\" height=\"48pt\" viewBox=\"0.00 0.00 120.00 48.00\" ",
        "xmlns=\"http://www.w3.org/2000/svg\"><g/></svg>",
    );
    let result = diagram_svg::standalone_document(svg);
    assert!(result.starts_with(PROLOG));
    assert_eq!(result, format!("{PROLOG}{svg}"));
    has(&result, "width=\"120pt\"", "");
    has(&result, "height=\"48pt\"", "");
    lacks(&result, "width=\"120.00\"", "the viewBox must not resize a sized root");
}

#[test]
fn standalone_svg_adds_namespace_when_missing() {
    // A root without a default namespace must gain one, without disturbing an already-absolute
    // size.
    let result = diagram_svg::standalone_document("<svg viewBox=\"0 0 10 10\" width=\"10\" height=\"10\"><g/></svg>");
    has(&result, XMLNS, "");
    has(&result, "width=\"10\"", "");
    has(&result, "height=\"10\"", "");
    // The declaration goes right after `<svg`, before the other attributes.
    assert_eq!(
        result,
        format!(
            "{PROLOG}<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 10 10\" width=\"10\" height=\"10\"><g/></svg>"
        )
    );
    // A namespaced root is left with exactly one declaration.
    let already =
        diagram_svg::standalone_document("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"5\" height=\"5\"></svg>");
    assert_eq!(already.split("xmlns=").count() - 1, 1);
}

#[test]
fn width_guard_does_not_capture_stroke_width() {
    // The leading-space guard on the attribute scan means a root that carries `stroke-width` but
    // no real `width` is still unsized - the scan must not read the stroke's value as the width -
    // so it resizes from the viewBox rather than adopting "2".
    let result = diagram_svg::standalone_document("<svg viewBox=\"0 0 60 40\" stroke-width=\"2\"><path/></svg>");
    has(&result, "width=\"60\"", "");
    has(&result, "height=\"40\"", "");
    has(&result, " width=\"60\"", "");
    lacks(&result, " width=\"2\"", "the stroke width must not become the root width");
    has(&result, "stroke-width=\"2\"", "the stroke-width attribute is left exactly as it was");
}

#[test]
fn single_quoted_attributes_are_read_and_percent_is_still_caught() {
    // The scan tries `"` first and then `'`, so an engine that single-quotes its root is read the
    // same way.
    let result = diagram_svg::standalone_document("<svg viewBox='0 0 20 10' width='100%'><g/></svg>");
    // An existing value is replaced inside its own quotes, so the root keeps the engine's quoting;
    // an attribute that was missing is added double-quoted just past `<svg`. Both are what the
    // Swift does, character for character.
    has(&result, " width='20'", "");
    has(&result, " height=\"10\"", "");
    lacks(&result, "100%", "");
    // A single-quoted xmlns still counts as a declaration.
    let already =
        diagram_svg::standalone_document("<svg xmlns='http://www.w3.org/2000/svg' width='5' height='5'></svg>");
    assert_eq!(already.split("xmlns=").count() - 1, 1);
}

#[test]
fn markup_without_an_svg_root_gets_only_the_prolog() {
    // A capture that came back as something else (or an unterminated tag) is passed through
    // untouched apart from the prolog - never rewritten blindly.
    assert_eq!(
        diagram_svg::standalone_document("<div>not a diagram</div>"),
        format!("{PROLOG}<div>not a diagram</div>")
    );
    assert_eq!(
        diagram_svg::standalone_document("<svg viewBox=\"0 0 1 1\""),
        format!("{PROLOG}<svg viewBox=\"0 0 1 1\"")
    );
    assert_eq!(diagram_svg::standalone_document(""), PROLOG);
}

#[test]
fn view_box_needs_four_tokens_and_is_copied_verbatim() {
    // Fewer or more than four tokens is not a size, so the root is left alone; four tokens are
    // copied as written, decimals and all.
    lacks(&diagram_svg::standalone_document("<svg viewBox=\"0 0 10\"><g/></svg>"), "width=", "");
    lacks(&diagram_svg::standalone_document("<svg viewBox=\"0 0 10 10 10\"><g/></svg>"), "width=", "");
    has(
        &diagram_svg::standalone_document("<svg viewBox=\"0.00,0.00,120.00,48.00\"><g/></svg>"),
        " width=\"120.00\"",
        "",
    );
    // Runs of separators do not manufacture a phantom fifth token.
    has(&diagram_svg::standalone_document("<svg viewBox=\"0  0 , 30 20\"><g/></svg>"), " width=\"30\"", "");
    // A unit-bearing width is absolute, so a viewBox never overrides it; a bare `.5` is absolute
    // too, and an empty one is not.
    has(
        &diagram_svg::standalone_document("<svg width=\".5\" height=\".5\" viewBox=\"0 0 9 9\"><g/></svg>"),
        "width=\".5\"",
        "",
    );
    has(
        &diagram_svg::standalone_document("<svg width=\"\" height=\"\" viewBox=\"0 0 9 9\"><g/></svg>"),
        " width=\"9\"",
        "",
    );
}

#[test]
fn diagram_equality_is_structural_across_the_layout() {
    // The extra `layout` is real state, not a decoration: two Graphviz rows with different layouts
    // are different diagrams.
    let neato = Diagram::of(0, diagram_svg::GRAPHVIZ, Some("neato"), "graph {}");
    let dot = Diagram::of(0, diagram_svg::GRAPHVIZ, Some("dot"), "graph {}");
    assert_ne!(neato, dot);
    assert_eq!(neato, Diagram::of(0, diagram_svg::GRAPHVIZ, Some("neato"), "graph {}"));
}
