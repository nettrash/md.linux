//! Port of md.win `Text/NotePreview.cs`: the one-line label a private author note gets in the Go ▸
//! Notes menu and in the Book window's Notes list (shell-design §5.6; macOS §6.9). Port of the Swift
//! `notePreview(_:)`, which neither Android nor md.vscode has — so every step of it is the Swift's.

use md_core::text::{graphemes, unicode, whitespace};

use crate::strings;

/// Swift's `collapsed.count > 50` then `prefix(50)` — grapheme clusters, not UTF-16 units.
pub const MAX_TEXT_ELEMENTS: usize = 50;

/// The label for a note's text: its first non-empty line, whitespace runs collapsed to one space,
/// cut to [`MAX_TEXT_ELEMENTS`] grapheme clusters plus an ellipsis — or
/// [`strings::EMPTY_NOTE`].
pub fn of(text: &str) -> String {
    let collapsed = collapse(first_non_empty_line(text));
    if collapsed.is_empty() {
        return strings::EMPTY_NOTE.to_string();
    }

    // md-core's graphemes walk the same UAX #29 clusters Swift's String.count does, so an emoji
    // ZWJ sequence or a base-plus-mark pair counts once here and once there.
    if graphemes::count(&collapsed) <= MAX_TEXT_ELEMENTS {
        return collapsed;
    }
    let head = graphemes::prefix(&collapsed, MAX_TEXT_ELEMENTS);
    // trimmingCharacters(in: .whitespaces) — Foundation's set (no line terminators), md-core's WS
    // trim; leading whitespace is already gone from the collapse.
    format!("{}{}", whitespace::trim_ws(head), strings::ELLIPSIS)
}

/// Swift's `split(whereSeparator: \.isNewline).first`: empty subsequences are omitted, so leading
/// blank lines are skipped and the answer is the first line with anything on it. `""` when the note
/// is blank.
fn first_non_empty_line(text: &str) -> &str {
    let mut start = 0;
    let mut i = 0;
    for cluster in graphemes::clusters(text) {
        if is_newline_element(cluster) {
            if i > start {
                return &text[start..i];
            }
            start = i + cluster.len();
        }
        i += cluster.len();
    }
    &text[start..]
}

/// Swift's `split(whereSeparator: \.isWhitespace).joined(separator: " ")`: every run of whitespace
/// becomes one U+0020 and the ends lose theirs. Unicode White_Space (.NET's `char.IsWhiteSpace`),
/// which is what `Character.isWhitespace` tests — wider than Foundation's `.whitespaces`, and the
/// newlines it also covers cannot occur here.
fn collapse(line: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut i = 0;
    for cluster in graphemes::clusters(line) {
        if let Some(c) = single_unit(cluster)
            && unicode::is_dotnet_white_space(c)
        {
            if i > start {
                parts.push(&line[start..i]);
            }
            start = i + cluster.len();
        }
        i += cluster.len();
    }
    if start < line.len() {
        parts.push(&line[start..]);
    }
    parts.join(" ")
}

// Split over grapheme clusters, never scalars: a separator fused into a larger cluster — CRLF, or a
// space wearing a combining mark — is not a separator in Swift, and md-core's own view-mode codec
// splits the same way for the same reason.

/// The cluster's one scalar when it is a single UTF-16 unit (md.win's `length == 1`).
fn single_unit(cluster: &str) -> Option<char> {
    let mut chars = cluster.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.len_utf16() == 1 => Some(c),
        _ => None,
    }
}

/// Character.isNewline: the seven Foundation terminators, plus CRLF as the single cluster it is.
fn is_newline_element(cluster: &str) -> bool {
    cluster == "\r\n" || single_unit(cluster).is_some_and(whitespace::is_newline)
}
