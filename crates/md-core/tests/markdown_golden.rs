//! Port of md.win `tests/Md.Core.Tests/GoldenTests.cs`: the golden corpus — md.vscode's
//! whole-document snapshots of the HTML writer, adopted verbatim as the family's parity gate. The
//! three shipping apps assert `contains(...)` on single constructs; sixteen Swift-vs-Kotlin
//! divergences went unnoticed until a 3,555-document differential run found them, so md.vscode
//! records the rendered BODY of every fixture, pins the `<head>` skeleton once, the three
//! stylesheets once and the "which engines does each document need" table once. These files are
//! not documentation and not a specification: a tripwire. Every comparison is byte for byte — no
//! newline normalisation, no trimming.
//!
//! PROVENANCE OF THE TWO PLATFORM GOLDENS. `testdata/test.md` is the per-repo exception (it names
//! the platform and its build command), so `golden/testdata/test.html` and `golden/document.html`
//! could not be copied from md.vscode: md.win regenerated them from its renderer once every other
//! golden was byte-exact and that renderer had reproduced md.vscode's own two goldens
//! byte-for-byte, then diffed them against those originals — four hunks, exactly the four lines
//! `test.md` localises (the `platforms` JSON, the two image URLs and the build-command fence). This
//! port shares md.win's fixtures byte for byte, Windows lines included, so it is pinned to md.win's
//! two goldens; `engine-needs.json` is untouched by the localisation.
//!
//! The md.win theories over all 25 fixtures are one generated module per fixture here, so a
//! failure names its fixture.

mod common;

use md_core::markdown::{html, parser};

const TESTDATA: [&str; 16] = [
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

const EXAMPLES: [&str; 9] = [
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

fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    TESTDATA.iter().map(|name| ("testdata", *name)).chain(EXAMPLES.iter().map(|name| ("examples", *name)))
}

fn source(directory: &str, name: &str) -> String {
    common::fixture_text(&format!("{directory}/{name}.md"))
}

fn golden(relative: &str) -> String {
    common::fixture_text(&format!("golden/{relative}"))
}

/// The placeholder the document golden carries in place of the stylesheet; two U+2026.
const STYLESHEET_PLACEHOLDER: &str = "/* \u{2026} see stylesheet-light.css \u{2026} */";

// MARK: - Golden comparison

/// Byte-for-byte golden comparison with a readable failure: a 114 KB body that differs in one byte
/// should say which line, and show the neighbourhood as a unified diff, not two walls of text.
#[track_caller]
fn assert_golden(expected: &str, actual: &str, name: &str) {
    if expected == actual {
        return;
    }
    panic!(
        "{name} differs from the golden (ours {} bytes, golden {})\n{}",
        actual.len(),
        expected.len(),
        unified(expected, actual)
    );
}

/// The first run of differing lines, in unified-diff shape with three lines of context on each
/// side. Lines are split on LF only, so a stray CR shows up as a visible `\r` in the hunk.
fn unified(expected: &str, actual: &str) -> String {
    let golden: Vec<&str> = expected.split('\n').collect();
    let ours: Vec<&str> = actual.split('\n').collect();
    let mut first = 0;
    while first < golden.len() && first < ours.len() && golden[first] == ours[first] {
        first += 1;
    }
    // Trim the common tail so the hunk covers only what changed (an insertion shows as such).
    let mut tail_golden = golden.len();
    let mut tail_ours = ours.len();
    while tail_golden > first && tail_ours > first && golden[tail_golden - 1] == ours[tail_ours - 1] {
        tail_golden -= 1;
        tail_ours -= 1;
    }
    let context_start = first.saturating_sub(3);
    let context_end = golden.len().min(tail_golden + 3);
    let mut out = String::from("--- golden\n+++ ours\n");
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        context_start + 1,
        context_end - context_start,
        context_start + 1,
        (context_end - context_start + tail_ours) as isize - tail_golden as isize
    ));
    for line in &golden[context_start..first] {
        out.push_str(&format!(" {}\n", show(line)));
    }
    for line in &golden[first..tail_golden] {
        out.push_str(&format!("-{}\n", show(line)));
    }
    for line in &ours[first..tail_ours] {
        out.push_str(&format!("+{}\n", show(line)));
    }
    for line in &golden[tail_golden..context_end] {
        out.push_str(&format!(" {}\n", show(line)));
    }
    if first < golden.len() && first < ours.len() && tail_golden - first == 1 && tail_ours - first == 1 {
        let column = golden[first].chars().zip(ours[first].chars()).take_while(|(g, o)| g == o).count();
        out.push_str(&format!("first difference on line {} at column {}\n", first + 1, column + 1));
    }
    out
}

/// A line with its control, C1 and private-use characters made visible, cut to 200 characters.
fn show(line: &str) -> String {
    let mut out = String::new();
    for (count, c) in line.chars().enumerate() {
        match c {
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20
                || (0x7F..=0xA0).contains(&(c as u32))
                || (0xE000..=0xF8FF).contains(&(c as u32)) =>
            {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
        if count >= 200 {
            out.push_str(" [...]");
            break;
        }
    }
    out
}

/// The ids of `<h[1-6] id="([^"]*)"`, in order.
fn heading_ids(html: &str) -> Vec<&str> {
    let mut ids = Vec::new();
    let mut from = 0;
    while let Some(offset) = html[from..].find("<h") {
        let start = from + offset;
        let rest = &html[start + 2..];
        if let Some(after) = rest.strip_prefix(|c: char| ('1'..='6').contains(&c))
            && let Some(id) = after.strip_prefix(" id=\"")
            && let Some(close) = id.find('"')
        {
            ids.push(&id[..close]);
        }
        from = start + 2;
    }
    ids
}

// MARK: - The snapshots, per fixture

/// `RendersEveryFixtureBodyAsRecorded`, `WrapsEveryFixtureInAWholeDocument` and
/// `KeepsTheOutlineAndTheEmittedAnchorIdsInStep`, one case each.
fn renders_body_as_recorded(directory: &str, name: &str) {
    // Bodies, not documents, so a stylesheet change shows up once rather than twenty-five times.
    // Neither `title` nor `dark` reaches the body.
    let actual = html::body(&source(directory, name), name, false).html + "\n";
    assert_golden(&golden(&format!("{directory}/{name}.html")), &actual, &format!("{directory}/{name}.html"));
}

fn wraps_in_a_whole_document(directory: &str, name: &str) {
    // A fixture that renders an empty body would still pass its golden, so the skeleton is
    // asserted too.
    let html = html::document(&source(directory, name), name, false, false);
    assert!(html.starts_with("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n"));
    assert!(html.contains("\n<body data-md-dark=\"0\">\n"));
    assert!(html.ends_with("<script type=\"module\" src=\"rich/md-init.js\"></script>\n</body>\n</html>"));
}

fn keeps_outline_and_anchor_ids_in_step(directory: &str, name: &str) {
    // One slug counter serves both, walked over top-level headings in document order, so a
    // contents entry always finds its anchor. A heading nested in a quote appears in neither.
    let source = source(directory, name);
    let body = html::body(&source, name, false).html;
    let outline: Vec<String> = parser::outline(&source).into_iter().map(|e| e.slug).collect();
    assert_eq!(heading_ids(&body), outline);
}

macro_rules! fixture_cases {
    ($($module:ident => ($directory:literal, $name:literal),)*) => {
        $(
            mod $module {
                #[test]
                fn renders_every_fixture_body_as_recorded() {
                    super::renders_body_as_recorded($directory, $name);
                }

                #[test]
                fn wraps_every_fixture_in_a_whole_document() {
                    super::wraps_in_a_whole_document($directory, $name);
                }

                #[test]
                fn keeps_the_outline_and_the_emitted_anchor_ids_in_step() {
                    super::keeps_outline_and_anchor_ids_in_step($directory, $name);
                }
            }
        )*
    };
}

fixture_cases! {
    testdata_blockquotes => ("testdata", "blockquotes"),
    testdata_code => ("testdata", "code"),
    testdata_edge_cases => ("testdata", "edge-cases"),
    testdata_headings => ("testdata", "headings"),
    testdata_images => ("testdata", "images"),
    testdata_inline => ("testdata", "inline"),
    testdata_lists => ("testdata", "lists"),
    testdata_math => ("testdata", "math"),
    testdata_mermaid => ("testdata", "mermaid"),
    testdata_notes => ("testdata", "notes"),
    testdata_outline => ("testdata", "outline"),
    testdata_page_breaks => ("testdata", "page-breaks"),
    testdata_plantuml => ("testdata", "plantuml"),
    testdata_tables => ("testdata", "tables"),
    testdata_test => ("testdata", "test"),
    testdata_thematic_breaks => ("testdata", "thematic-breaks"),
    examples_01_welcome => ("examples", "01-Welcome"),
    examples_02_formatting => ("examples", "02-Formatting"),
    examples_03_tables => ("examples", "03-Tables"),
    examples_04_code => ("examples", "04-Code"),
    examples_05_images => ("examples", "05-Images"),
    examples_06_math => ("examples", "06-Math"),
    examples_07_diagrams => ("examples", "07-Diagrams"),
    examples_08_plots => ("examples", "08-Plots"),
    examples_09_writer_tools => ("examples", "09-Writer Tools"),
}

// MARK: - The single snapshots

#[test]
fn needs_the_recorded_engines() {
    // JSON.stringify(table, null, 2) + "\n", written by hand rather than through a serializer
    // whose newline and escaping are version-dependent. Word order is the probe order.
    let rows: Vec<String> = all()
        .map(|(directory, name)| {
            let needs = html::body(&source(directory, name), name, false).needs;
            format!("  \"{directory}/{name}\": \"{needs}\"")
        })
        .collect();
    let actual = format!("{{\n{}\n}}\n", rows.join(",\n"));
    assert_golden(&golden("engine-needs.json"), &actual, "engine-needs.json");
}

#[test]
fn wraps_a_document_as_recorded() {
    // The only place the skeleton bytes are pinned on a real document: head order, the `</style>`
    // glue, `data-md-dark`, the module script last, no trailing newline. The stylesheet is replaced
    // by a placeholder so that it is pinned once, in its own files.
    let html = html::document(&source("testdata", "test"), "test", false, false);
    let actual = html.replace(&html::css(false, false), STYLESHEET_PLACEHOLDER);
    assert_golden(&golden("document.html"), &actual, "document.html");
    assert!(actual.ends_with("</html>"));
}

#[test]
fn styles_as_recorded_in_all_three_variants() {
    // Three reachable sheets, not four: `dark && !export` runs before the palette is computed.
    assert_golden(&golden("stylesheet-light.css"), &(html::css(false, false) + "\n"), "stylesheet-light.css");
    assert_golden(&golden("stylesheet-dark.css"), &(html::css(true, false) + "\n"), "stylesheet-dark.css");
    assert_golden(&golden("stylesheet-export.css"), &(html::css(false, true) + "\n"), "stylesheet-export.css");
    assert_eq!(html::css(false, true), html::css(true, true));
}

// MARK: - The fixtures' own promises, HTML side

#[test]
fn notes_fixture_keeps_private_notes_out_of_the_html() {
    // Both spellings vanish — the `note:` ones the panel shows and the plain comments — and the
    // prose around them is untouched.
    let html = html::document(&source("testdata", "notes"), "notes", false, false);
    assert!(html.contains("Visible prose before"));
    assert!(html.contains("Visible prose after"));
    assert!(!html.contains("private author note"));
    assert!(!html.contains("plain comment"));
    assert!(!html.contains("<!--"));
}

#[test]
fn images_fixture_emits_img_tags() {
    let html = html::body(&source("testdata", "images"), "images", false).html;
    assert!(
        html.contains(
            "<img src=\"https://nettrash.me/favicon.ico\" alt=\"nettrash.me favicon\" title=\"The favicon\">"
        )
    );
    assert!(
        html.contains(
            "<a href=\"https://nettrash.me\"><img src=\"https://nettrash.me/favicon.ico\" alt=\"badge\"></a>"
        )
    );
}

#[test]
fn rich_fixtures_emit_their_containers() {
    let math = html::document(&source("testdata", "math"), "math", false, false);
    assert!(math.contains("class=\"md-mathi\""));
    assert!(math.contains("class=\"md-mathd\""));
    assert!(math.contains("rich/katex.min.js"));
    // The currency guard survives a real document: the price is prose, not a formula.
    assert!(math.contains("$99.99"));
    assert!(html::body(&source("testdata", "mermaid"), "mermaid", false).html.contains("<pre class=\"mermaid\">"));
    // plantuml.md is the only place the ```puml alias is pinned on a real file.
    let plantuml = source("testdata", "plantuml");
    assert!(plantuml.contains("```puml\n"));
    assert_eq!(html::body(&plantuml, "plantuml", false).html.matches("<div class=\"plantuml\">").count(), 3);
}

// MARK: - The corpus's own honesty

fn files_under(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn golden_files_carry_no_carriage_return_and_no_bom() {
    // A mis-configured clone (core.autocrlf) would rewrite every expected output at once; the
    // failure should say so rather than show 25 diffs on the first character of line two.
    let mut files = Vec::new();
    files_under(&common::fixture_path("golden"), &mut files);
    assert_eq!(files.len(), 25 + 1 + 1 + 3 + 1);
    for file in &files {
        let bytes = std::fs::read(file).unwrap();
        assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "{} has a BOM", file.display());
        assert!(!bytes.contains(&b'\r'), "{} has a CR", file.display());
    }
    // Every body golden and every stylesheet ends with exactly one LF; the document golden ends at
    // `</html>` with none.
    for (directory, name) in all() {
        let text = golden(&format!("{directory}/{name}.html"));
        assert!(text.ends_with('\n'), "{name}.html");
        assert!(!text.ends_with("\n\n"), "{name}.html ends with two newlines");
    }
    assert!(golden("document.html").ends_with("</html>"));
}

#[test]
fn document_golden_embeds_the_test_body_golden() {
    // The text between `<body data-md-dark="0">\n` and the module script in document.html is
    // byte-identical to test.html minus its trailing newline — the two files were made from the
    // same render and must stay in step when either is regenerated.
    let document = golden("document.html");
    let open = "<body data-md-dark=\"0\">\n";
    let close = "\n<script type=\"module\" src=\"rich/md-init.js\"></script>";
    let start = document.find(open).unwrap() + open.len();
    let end = start + document[start..].find(close).unwrap();
    assert!(start > open.len() && end > start);
    let body = golden("testdata/test.html");
    assert_eq!(&body[..body.len() - 1], &document[start..end]);
}

#[test]
fn windows_test_fixture_localises_exactly_the_four_platform_lines() {
    // The per-repo exception, pinned so nobody "fixes" the corpus into agreement: the JSON
    // platforms line, the two image URLs and the build-command fence — and the body golden carries
    // their rendered forms. md-core shares md.win's corpus byte for byte, so these are its lines.
    let source = source("testdata", "test");
    assert!(source.contains("  \"platforms\": [\"Windows\"],\n"));
    assert!(
        source
            .contains("   ```powershell\n   dotnet build src\\Md.App\\Md.App.csproj `\n     -p:Platform=x64\n   ```\n")
    );
    assert!(source.contains("(https://nettrash.me/favicon-192x192.png \"Optional title\")"));
    assert!(source.contains("[![Alt text](https://nettrash.me/md-icon.png)](https://nettrash.me)"));

    let golden = golden("testdata/test.html");
    assert!(golden.contains("  &quot;platforms&quot;: [&quot;Windows&quot;],"));
    assert!(golden.contains("<pre><code class=\"language-powershell\">dotnet build src\\Md.App\\Md.App.csproj `\n  -p:Platform=x64</code></pre>"));
    assert!(!golden.contains("Visual Studio Code"));
    assert!(!golden.contains("npm ci"));
    // The engine table is unaffected by the localisation: the fence still names a language.
    assert_eq!(html::body(&source, "test", false).needs.to_string(), "math mermaid plantuml highlight");
}
