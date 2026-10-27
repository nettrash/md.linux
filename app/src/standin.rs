//! Pages for the spike until md-core's `markdown` module lands: the shared golden output (real
//! md HTML) and a deliberately tiny Markdown-to-HTML stand-in for the live-typing check.

use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/md-core/tests/fixtures")
}

const STYLE_PLACEHOLDER: &str = "<style>/* \u{2026} see stylesheet-light.css \u{2026} */</style>";

fn stylesheet(dark: bool) -> String {
    let name = if dark { "stylesheet-dark.css" } else { "stylesheet-light.css" };
    std::fs::read_to_string(fixtures().join("golden").join(name)).unwrap_or_default()
}

/// golden/document.html with the export stylesheet (white paper, 11 pt, page-break rule) —
/// the page md prints and exports to PDF.
pub fn golden_document_export() -> String {
    let html = std::fs::read_to_string(fixtures().join("golden/document.html")).unwrap_or_default();
    let css = std::fs::read_to_string(fixtures().join("golden/stylesheet-export.css")).unwrap_or_default();
    html.replacen(STYLE_PLACEHOLDER, &format!("<style>{css}</style>"), 1)
}

/// `golden/document.html` with its stylesheet placeholder filled back in (the golden test
/// swaps the CSS out for that comment), so this is byte for byte what md emits for test.md.
/// `dark` swaps in the dark stylesheet and `data-md-dark="1"`, as md's dark render does.
pub fn golden_document(dark: bool) -> String {
    let html = std::fs::read_to_string(fixtures().join("golden/document.html")).unwrap_or_default();
    restyle(&html, dark)
}

/// The golden `examples/07-Diagrams.html` body (Mermaid, Graphviz, PlantUML) inside the golden
/// document's head and closing script — the shape md's `Document()` gives every page.
pub fn golden_diagrams(dark: bool) -> String {
    let doc = std::fs::read_to_string(fixtures().join("golden/document.html")).unwrap_or_default();
    let body = std::fs::read_to_string(fixtures().join("golden/examples/07-Diagrams.html")).unwrap_or_default();
    let open = "<body data-md-dark=\"0\">\n";
    let close = "<script type=\"module\" src=\"rich/md-init.js\"></script>";
    let (Some(head_end), Some(tail_start)) = (doc.find(open), doc.rfind(close)) else {
        return String::new();
    };
    let page = format!("{}{}{}{}", &doc[..head_end + open.len()], body, "\n", &doc[tail_start..]);
    restyle(&page.replace("<title>test</title>", "<title>Diagrams</title>"), dark)
}

fn restyle(html: &str, dark: bool) -> String {
    let style = format!("<style>{}</style>", stylesheet(dark));
    let mut out = html.replacen(STYLE_PLACEHOLDER, &style, 1);
    if dark {
        out = out.replacen("<body data-md-dark=\"0\">", "<body data-md-dark=\"1\">", 1);
    }
    out
}

/// test.md, the shared feature reference, as editor text.
pub fn test_markdown() -> String {
    std::fs::read_to_string(fixtures().join("testdata/test.md")).unwrap_or_default()
}

/// A stand-in renderer (headings, paragraphs, fences) — enough for a long, styled page whose
/// height tracks the text. Not md's parser; it is replaced by `md_core::markdown` in the shell.
pub fn render(text: &str, dark: bool) -> String {
    let mut body = String::new();
    let mut para: Vec<&str> = Vec::new();
    let mut fence: Option<Vec<&str>> = None;
    let flush = |para: &mut Vec<&str>, body: &mut String| {
        if !para.is_empty() {
            body.push_str("<p>");
            body.push_str(&escape(&para.join("\n")));
            body.push_str("</p>\n");
            para.clear();
        }
    };
    for line in text.lines() {
        if let Some(lines) = fence.as_mut() {
            if line.starts_with("```") {
                body.push_str("<pre><code>");
                body.push_str(&escape(&lines.join("\n")));
                body.push_str("</code></pre>\n");
                fence = None;
            } else {
                lines.push(line);
            }
            continue;
        }
        if line.starts_with("```") {
            flush(&mut para, &mut body);
            fence = Some(Vec::new());
        } else if let Some(level) = heading_level(line) {
            flush(&mut para, &mut body);
            let title = line[level..].trim_start_matches(' ');
            body.push_str(&format!("<h{level}>{}</h{level}>\n", escape(title)));
        } else if line.chars().all(|c| c == ' ' || c == '\t') {
            flush(&mut para, &mut body);
        } else {
            para.push(line);
        }
    }
    flush(&mut para, &mut body);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>preview</title>\n<style>{}</style>\n</head>\n<body data-md-dark=\"{}\">\n{}<script type=\"module\" src=\"rich/md-init.js\"></script>\n</body>\n</html>",
        stylesheet(dark),
        if dark { "1" } else { "0" },
        body
    )
}

fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    ((1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ')).then_some(hashes)
}

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_paragraphs_and_fences() {
        let html = render("# Title\n\nsome <text>\nmore\n\n```\na < b\n```\n#nospace", false);
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<p>some &lt;text&gt;\nmore</p>"));
        assert!(html.contains("<pre><code>a &lt; b</code></pre>"));
        assert!(html.contains("<p>#nospace</p>"));
        assert!(html.contains("data-md-dark=\"0\""));
    }

    #[test]
    fn golden_pages_are_complete_documents() {
        let doc = golden_document(false);
        assert!(doc.starts_with("<!DOCTYPE html>"));
        assert!(!doc.contains(STYLE_PLACEHOLDER));
        assert!(doc.contains("rich/md-init.js"));
        let dark = golden_diagrams(true);
        assert!(dark.contains("class=\"graphviz\""));
        assert!(dark.contains("<body data-md-dark=\"1\">"));
    }
}
