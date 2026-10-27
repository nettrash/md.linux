//! What the `.tex` writer's tests share: md.win's `TexBody` / `TexPreamble` helpers, the ordinal
//! assertions its suite spells out (xUnit's string asserts are culture-sensitive, so md.win never
//! uses them; Rust's are ordinal already, the helpers are kept for their messages), and the
//! invisible code points the inputs are built from. Included by the LaTeX test files with
//! `#[path]`, so `common/mod.rs` stays as every other module's tests expect it.
#![allow(dead_code)]

use md_core::export::latex_export;
use md_core::text::{scalar_text, whitespace};

// Three ways to make the character in front part of a longer grapheme: a combining acute, a
// variation selector, a zero-width joiner. Spelled as escapes so nothing invisible is typed into
// a test file.
pub const ACUTE: &str = "\u{0301}";
pub const VS16: &str = "\u{FE0F}";
pub const ZWJ: &str = "\u{200D}";
pub const MARKS: [&str; 3] = [ACUTE, VS16, ZWJ];
pub const E000: &str = "\u{E000}";
pub const E001: &str = "\u{E001}";
pub const EM_DASH: &str = "\u{2014}";
pub const SUP2: &str = "\u{00B2}";

/// md.win's one-argument `LaTeXExport.Document(source)`: the title is the file's name and is
/// written nowhere, so any string stands for its absence.
pub fn tex(source: &str) -> String {
    latex_export::document(source, "")
}

/// The body between `\begin{document}` and `\end{document}`, trimmed (`Whitespace.TrimWSNL`) —
/// most assertions are about the content, not the preamble.
#[track_caller]
pub fn tex_body(source: &str) -> String {
    let tex = tex(source);
    let opening = "\\begin{document}\n";
    let closing = "\n\\end{document}";
    let (Some(start), Some(end)) = (tex.find(opening), tex.find(closing)) else {
        panic!("the document is not wrapped in a document environment: <{tex}>");
    };
    whitespace::trim_wsnl(&tex[start + opening.len()..end]).to_string()
}

/// The preamble: everything before `\begin{document}`.
pub fn tex_preamble(source: &str) -> String {
    let tex = tex(source);
    match tex.find("\\begin{document}") {
        Some(start) => tex[..start].to_string(),
        None => tex,
    }
}

#[track_caller]
pub fn assert_contains(text: &str, part: &str) {
    assert!(text.contains(part), "expected to find <{part}> in <{text}>");
}

#[track_caller]
pub fn assert_contains_because(text: &str, part: &str, because: &str) {
    assert!(text.contains(part), "{because}: expected to find <{part}> in <{text}>");
}

#[track_caller]
pub fn assert_not_contains(text: &str, part: &str) {
    assert!(!text.contains(part), "did not expect <{part}> in <{text}>");
}

#[track_caller]
pub fn assert_not_contains_because(text: &str, part: &str, because: &str) {
    assert!(!text.contains(part), "{because}: did not expect <{part}> in <{text}>");
}

#[track_caller]
pub fn assert_starts_with(text: &str, prefix: &str) {
    assert!(text.starts_with(prefix), "expected <{text}> to start with <{prefix}>");
}

#[track_caller]
pub fn assert_ends_with(text: &str, suffix: &str) {
    assert!(text.ends_with(suffix), "expected <{text}> to end with <{suffix}>");
}

/// How many times `part` occurs, counted the way md.win counts (`ScalarText.Split(...).Count - 1`).
pub fn occurrences(text: &str, part: &str) -> usize {
    scalar_text::split(text, part).len() - 1
}

/// `char.ConvertFromUtf32`.
pub fn chr(code_point: u32) -> String {
    char::from_u32(code_point).expect("a scalar value").to_string()
}

/// The lines joined with `\n`.
pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}
