//! What the HTML and PDF export tests share: md.win's `Has` / `Lacks` / `Count` / `ReplaceFirst`
//! (every comparison ordinal — Rust's `str` search always is), the simulated capture, the asset
//! readers, and the repository's vendored `rich/` assets. Included with `#[path]` by
//! `export_html*.rs` and `export_pdf*.rs`, so `common/mod.rs` stays as every other module's tests
//! expect it.
#![allow(dead_code)]

use std::path::PathBuf;

use md_core::export::html_export;

#[track_caller]
pub fn has(haystack: &str, needle: &str, note: &str) {
    assert!(haystack.contains(needle), "{note} expected to contain '{needle}'");
}

#[track_caller]
pub fn lacks(haystack: &str, needle: &str, note: &str) {
    assert!(!haystack.contains(needle), "{note} expected NOT to contain '{needle}'");
}

/// Non-overlapping ordinal occurrences of `needle`, left to right.
pub fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// `text` with the first `needle` replaced, which must be present.
#[track_caller]
pub fn replace_first(text: &str, needle: &str, replacement: &str) -> String {
    let at = text.find(needle).unwrap_or_else(|| panic!("expected '{needle}' to be present"));
    [&text[..at], replacement, &text[at + needle.len()..]].concat()
}

/// What the capture script does, done to a string: every `<script>…</script>` and every
/// stylesheet `<link>` removed, the doctype gone (`outerHTML` has none). The surrounding newlines
/// are text nodes and stay, exactly as they would in the DOM. A real render would also have turned
/// the diagram containers into `<svg>` and the formulas into markup; that is the engines' work, not
/// this module's, and no assertion here depends on it.
#[track_caller]
pub fn capture(document: &str) -> String {
    let page = document.strip_prefix(html_export::DOCTYPE).expect("the export document starts with the doctype");
    let page = remove_all(page, "<script", "</script>");
    remove_all(&page, "<link ", ">")
}

#[track_caller]
fn remove_all(text: &str, open: &str, close: &str) -> String {
    let mut text = text.to_string();
    while let Some(start) = text.find(open) {
        let end = text[start..].find(close).unwrap_or_else(|| panic!("unterminated {open}")) + start;
        text = [&text[..start], &text[end + close.len()..]].concat();
    }
    text
}

/// The repository root: the folder holding `rich/`, two levels above the crate.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The app's asset reader, in its test spelling: `rich/…` under the repository root, `None` for a
/// file that is not there.
pub fn read_bundled_asset(relative_path: &str) -> Option<Vec<u8>> {
    let path = repo_root().join(relative_path);
    if path.is_file() { std::fs::read(path).ok() } else { None }
}

/// The vendored KaTeX sheet in the repository.
pub fn vendored_katex_css() -> PathBuf {
    repo_root().join("rich").join("katex.min.css")
}

/// A reader over a fixed set of `(path, UTF-8 content)` files; everything else is absent.
pub fn assets(files: &[(&str, &str)]) -> impl Fn(&str) -> Option<Vec<u8>> + use<> {
    let files: Vec<(String, Vec<u8>)> =
        files.iter().map(|(path, content)| (path.to_string(), content.as_bytes().to_vec())).collect();
    move |name: &str| files.iter().find(|(path, _)| path == name).map(|(_, content)| content.clone())
}
