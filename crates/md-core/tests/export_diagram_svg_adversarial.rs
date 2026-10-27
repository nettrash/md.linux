//! Adversarial parity tests for `diagram_svg` and the diagram file name. Port of md.win
//! `DiagramsvgAdversarialTests.cs`.
//!
//! Every expected value below came from an oracle, never from reading a port's output back: the
//! **Swift** column is `md.macOS/md/DocumentExport.swift`'s own `DiagramSVG.firstLine /
//! standaloneDocument` and `DocumentExport.sanitized`, lifted verbatim into a file compiled with
//! `swiftc` and fed these exact inputs; the **TypeScript** column is `md.vscode/src/export/svg.ts`
//! bundled with esbuild and driven under node. Where the ports disagree the test says so and names
//! each answer.
//!
//! Two of them close holes a one-character mutation walked straight through in md.win: swapping
//! the two halves of the SVG fix-up (same attributes, different bytes) and widening the
//! control-character test to `<= ' '`. Two more pin divergences from macOS the oracles found — the
//! ASCII digit test (shared by this port) and the Indic-conjunct cluster count (which this port,
//! unlike md.win, does NOT share: it agrees with macOS, and the test asserts the macOS answer).

use md_core::export::diagram_svg::{self, Diagram};
use md_core::export::export_file_names::{portable_stem, sanitized};
use md_core::text::utf16::utf16_len;

const PROLOG: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
const NS: &str = "xmlns=\"http://www.w3.org/2000/svg\"";
const ELLIPSIS: char = '\u{2026}';

fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

fn engines(diagrams: &[Diagram]) -> Vec<&str> {
    diagrams.iter().map(|d| d.engine.as_str()).collect()
}

// MARK: - The byte order of the fix-up

#[test]
fn fix_up_namespaces_the_root_before_it_sizes_it() {
    // `standaloneDocument` is `withResolvedSize(inNamespaced(svg))`, and BOTH steps insert their
    // attribute immediately after `<svg`, so the order decides the bytes of the saved file whenever
    // a root needs the namespace AND has no width/height to overwrite. Doing it the other way round
    // produces the same attributes in a different order - a silent byte-parity break.
    //
    // Swift (swiftc oracle) and md.vscode (node oracle) both return exactly this:
    assert_eq!(
        diagram_svg::standalone_document("<svg viewBox=\"0 0 60 40\" stroke-width=\"2\"><path/></svg>"),
        format!("{PROLOG}<svg height=\"40\" width=\"60\" {NS} viewBox=\"0 0 60 40\" stroke-width=\"2\"><path/></svg>")
    );

    // The mixed case: `width` exists (single-quoted, a percentage) so it is REPLACED inside the
    // engine's own quotes, while `height` is missing and is INSERTED double-quoted just past
    // `<svg` - ahead of the namespace, which went in first.
    assert_eq!(
        diagram_svg::standalone_document("<svg viewBox='0 0 20 10' width='100%'><g/></svg>"),
        format!("{PROLOG}<svg height=\"10\" {NS} viewBox='0 0 20 10' width='20'><g/></svg>")
    );
}

#[test]
fn the_first_svg_wins_even_inside_an_html_comment() {
    // The scan is `find("<svg")`, not a parser, so a commented-out root is the root it fixes up -
    // and the real one after it is left alone. Not a defect to fix: the input is one element's
    // `outerHTML` read back out of the DOM, never a page. Pinned because every port does exactly
    // this, byte for byte (Swift and md.vscode oracles agree).
    assert_eq!(
        diagram_svg::standalone_document(
            "<!-- <svg width=\"1\" height=\"1\"/> --><svg viewBox=\"0 0 4 4\" width=\"100%\"><g/></svg>"
        ),
        format!(
            "{PROLOG}<!-- <svg {NS} width=\"1\" height=\"1\"/> --><svg viewBox=\"0 0 4 4\" width=\"100%\"><g/></svg>"
        )
    );
}

// MARK: - Documented divergences from macOS

#[test]
fn absolute_length_asks_for_an_ascii_digit_where_mac_os_asks_for_any_number() {
    // DIVERGENCE, deliberate and shared with md.win and md.vscode. Swift's guard is
    // `first == "." || first.isNumber`, and `Character.isNumber` is true for every numeric scalar
    // - the swiftc oracle answers "N" for ٥ (U+0665), ５ (U+FF15), Ⅰ (U+2160), 〇, 一, ½, ① and 𝟠
    // alike - so macOS reads `width="٥"` as already sized and leaves the root untouched. This port
    // tests '0'..='9', so the same root counts as unsized and is resized from the viewBox. Kotlin is
    // a third answer again (`isDigit`, Nd only). Unreachable in practice: the input is one engine's
    // own `outerHTML`, and every engine emits ASCII numerals.
    let svg = "<svg width=\"\u{0665}\" height=\"\u{0665}\" viewBox=\"0 0 9 9\"><g/></svg>";
    assert_eq!(
        diagram_svg::standalone_document(svg),
        format!("{PROLOG}<svg {NS} width=\"9\" height=\"9\" viewBox=\"0 0 9 9\"><g/></svg>")
    );
    // macOS, from the oracle, keeps the numeral:
    //   <?xml …?>\n<svg xmlns="…" width="٥" height="٥" viewBox="0 0 9 9"><g/></svg>
    // ASCII is where the ports do agree, and that is the whole of the real input set.
    assert_eq!(
        diagram_svg::standalone_document("<svg width=\"5\" height=\"5\" viewBox=\"0 0 9 9\"><g/></svg>"),
        format!("{PROLOG}<svg {NS} width=\"5\" height=\"5\" viewBox=\"0 0 9 9\"><g/></svg>")
    );
}

#[test]
fn the_label_cap_counts_uax29_clusters_and_keeps_an_indic_conjunct_whole_like_mac_os() {
    // md.win: TheLabelCapCountsDotNetTextElementsWhichSplitAnIndicConjunct, ported to this port's
    // answer. .NET's grapheme breaker does not apply UAX #29's GB9c (the Indic conjunct rule added
    // in Unicode 15.1), so there क् + ष is TWO text elements and md.win keeps 20 conjuncts (61
    // UTF-16 units). `unicode-segmentation` applies GB9c, as Swift's `Character` does, so this
    // port keeps what the swiftc oracle keeps: 40 whole क्ष conjuncts (121 UTF-16 units).
    // md.vscode counts code points and keeps 13 conjuncts plus a stray क (41 units). Cosmetic only:
    // the label is a menu row.
    let conjunct = "\u{0915}\u{094D}\u{0937}";
    let label = diagram_svg::diagrams(&lines(&["```mermaid", &conjunct.repeat(41), "```"]))[0].label();
    assert_eq!(label, format!("{}{ELLIPSIS}", conjunct.repeat(40)));
    assert_eq!(utf16_len(&label), 121);
}

// MARK: - Enumeration the DOM has to pair with

#[test]
fn raw_diagram_probes_short_circuit_the_markdown_walk() {
    // A `.puml` / `.gv` document is rendered without parsing Markdown at all, so it is ONE diagram
    // - the whole file - even when the file's body would otherwise parse into fences of its own.
    // Get this wrong and the walk offers two rows while the DOM holds one container, and the
    // second export saves the first figure. Expectations from the md.vscode oracle.
    let puml = lines(&["@startuml", "```mermaid", "graph TD", "```", "@enduml"]);
    let from_puml = diagram_svg::diagrams(&puml);
    assert_eq!(engines(&from_puml), ["plantuml"]);
    assert_eq!(from_puml[0].label(), "@startuml");
    assert_eq!(from_puml[0].source, puml); // the whole file, fence and all

    let gv = lines(&["digraph G {", " a -> b", "}", "", "```mermaid", "graph TD", "```"]);
    let from_gv = diagram_svg::diagrams(&gv);
    assert_eq!(engines(&from_gv), ["graphviz"]);
    assert_eq!(from_gv[0].layout.as_deref(), Some("dot"));
    assert_eq!(from_gv[0].label(), "digraph G {");
    assert_eq!(from_gv[0].source, gv);
}

#[test]
fn ordinals_pair_with_the_dom_across_quotes_math_and_plots() {
    // The walk's index IS the index the capture indexes `querySelectorAll(DOM_SELECTOR)` by, so
    // this document - plot, quoted graphviz, math (not a diagram), quoted mermaid, plantuml - must
    // come back as exactly four rows numbered 0..3 in that order. The md.vscode oracle returns
    // exactly this list.
    let source = lines(&[
        "```plot",
        "sin(x)",
        "```",
        "",
        "> ```dot",
        "> digraph{}",
        "> ```",
        "",
        "```math",
        "x",
        "```",
        "",
        "> ```mermaid",
        "> graph TD",
        "> ```",
        "",
        "```plantuml",
        "@startuml",
        "```",
    ]);
    let found = diagram_svg::diagrams(&source);
    assert_eq!(engines(&found), ["plot", "graphviz", "mermaid", "plantuml"]);
    assert_eq!(found.iter().map(|d| d.ordinal).collect::<Vec<_>>(), [0, 1, 2, 3]);
    assert_eq!(
        found.iter().map(|d| d.menu_title.as_str()).collect::<Vec<_>>(),
        ["Plot: sin(x)", "Graphviz: digraph{}", "Mermaid: graph TD", "PlantUML: @startuml"]
    );
    // A thrice-nested quote still lands in the list, because the writer keeps recursing.
    assert_eq!(
        engines(&diagram_svg::diagrams(&lines(&["> > > ```neato", "> > > graph {}", "> > > ```"]))),
        ["graphviz"]
    );
}

// MARK: - The Linux layer next to the family rule

#[test]
fn the_linux_layer_keeps_control_characters_and_real_spaces() {
    // md.win: TheWindowsLayerReplacesControlCharactersWithoutTouchingRealSpaces, ported to this
    // port's file-name decision (`export_file_names`): Linux refuses only NUL, so a line break
    // pasted into a title stays, exactly as the family stem and the Mac keep it (Swift and
    // md.vscode both return "My Draft\nNotes" and "Draft  Notes" for these). The spaces are
    // ordinary characters and survive, interior runs included.
    assert_eq!(portable_stem("My Draft\nNotes"), "My Draft\nNotes");
    assert_eq!(sanitized("My Draft\nNotes", "svg"), "My Draft\nNotes.svg");

    assert_eq!(portable_stem("  Draft  Notes  "), "Draft  Notes");
    assert_eq!(sanitized("  Draft  Notes  ", "pdf"), "Draft  Notes.pdf");

    // A control character next to a space, which is where a "<= ' '" slip would show: both kept.
    assert_eq!(sanitized("a\u{0001} b", "svg"), "a\u{0001} b.svg");

    // And the diagram name the SVG export composes, `{stem}-{ordinal + 1}.svg`, on a title that
    // goes through both layers.
    let ordinal = 2;
    assert_eq!(format!("{}-{}.svg", sanitized("My Draft\nNotes", ""), ordinal + 1), "My Draft\nNotes-3.svg");
}
