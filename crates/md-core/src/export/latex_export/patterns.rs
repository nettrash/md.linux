//! The inline patterns of the `.tex` writer — md.win's `LaTeXExport.Writer` regexes, each a
//! hand-written matcher (md-core has no regex crate).
//!
//! A matcher answers "does the pattern match starting at byte `i`, and where are its groups?"
//! with exactly the match the backtracking engine would produce there; [`guarded`] then runs the
//! engine's leftmost, non-overlapping scan over it. Each matcher's comment spells the pattern and
//! why its backtracking cannot change the answer. Every pattern opens with an ASCII literal, so a
//! matcher first compares the byte at `i` and only slices on a char boundary; every match ends
//! after an ASCII literal.
//!
//! Two of ICU's classes are not the obvious ones and are spelled out exactly as md.win spells
//! them, so no engine's own `\s` or `.` decides where a link's URL ends: `\s` is
//! `[\t\n\v\f\r\u0085\p{Z}]` (measured on macOS; U+200B, U+FEFF and U+180E are *not* in it) and
//! `.` refuses `\n \v \f \r U+0085 U+2028 U+2029` and nothing else.

use std::ops::Range;

use crate::text::unicode::{self, GeneralCategory};

/// One match: the whole of it and its groups from 1 on, as byte ranges of the scanned text.
pub(super) struct Found {
    pub whole: Range<usize>,
    pub groups: Vec<Range<usize>>,
}

/// Tries the pattern at byte `i` of the text.
pub(super) type Matcher = fn(&str, usize) -> Option<Found>;

/// The hand-rolled lookarounds of a pattern, asked of an otherwise complete match.
pub(super) type Guard = fn(&str, &Found) -> bool;

/// Every non-overlapping match, left to right, with the hand-rolled word guard where the Swift
/// pattern had lookarounds. A rejected match resumes the scan one position later, exactly as ICU
/// does after a failed lookbehind — the delimiter's content excludes the delimiter, so there is no
/// other candidate close to backtrack to. (Every match starts on an ASCII byte, so one byte on is
/// one code unit on, and a char boundary.)
pub(super) fn guarded(text: &str, pattern: Matcher, guard: Option<Guard>) -> Vec<Found> {
    let mut matches = Vec::new();
    let mut from = 0;
    while let Some(found) = (from..text.len()).find_map(|i| pattern(text, i)) {
        if guard.is_none_or(|accepts| accepts(text, &found)) {
            from = found.whole.end;
            matches.push(found);
        } else {
            from = found.whole.start + 1;
        }
    }
    matches
}

fn found(whole: Range<usize>, groups: Vec<Range<usize>>) -> Found {
    Found { whole, groups }
}

/// A match of a pattern with a single group.
fn one_group(whole: Range<usize>, group: Range<usize>) -> Found {
    Found { whole, groups: vec![group] }
}

/// The first occurrence of the ASCII `needle` at or after byte `from` (a char boundary).
fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.get(from..)?.find(needle).map(|offset| from + offset)
}

// MARK: - Literal spans

/// `` `([^`]+)` ``: the content runs to the next backtick and may not be empty. A failed opener
/// (two backticks in a row) is retried one position on, where the second backtick opens.
pub(super) fn code_span(t: &str, i: usize) -> Option<Found> {
    if t.as_bytes()[i] != b'`' {
        return None;
    }
    let close = find_from(t, "`", i + 1)?;
    (close > i + 1).then(|| one_group(i..close + 1, i + 1..close))
}

/// `\$\$([\s\S]+?)\$\$`: lazy, so the first `$$` after at least one character of content closes.
pub(super) fn display_dollars(t: &str, i: usize) -> Option<Found> {
    lazy_pair(t, i, "$$", "$$")
}

/// `\\\[([\s\S]+?)\\\]`.
pub(super) fn display_brackets(t: &str, i: usize) -> Option<Found> {
    lazy_pair(t, i, "\\[", "\\]")
}

fn lazy_pair(t: &str, i: usize, open: &str, close: &str) -> Option<Found> {
    if !t.as_bytes()[i..].starts_with(open.as_bytes()) {
        return None;
    }
    let content = i + open.len();
    // At least one character of content: the close is searched for after the first one.
    let first = t.get(content..)?.chars().next()?;
    let end = find_from(t, close, content + first.len_utf8())?;
    Some(one_group(i..end + close.len(), content..end))
}

/// `\$([^$\n]+?)\$`, the guard ([`dollar_guard`]) checked separately. The content class excludes
/// `$`, so the only possible close is the first `$` after the opener, and a `\n` before it ends
/// the attempt.
pub(super) fn inline_dollar(t: &str, i: usize) -> Option<Found> {
    let b = t.as_bytes();
    if b[i] != b'$' {
        return None;
    }
    let mut close = i + 1;
    while close < b.len() && b[close] != b'$' && b[close] != b'\n' {
        close += 1;
    }
    if close >= b.len() || b[close] != b'$' || close == i + 1 {
        return None;
    }
    Some(one_group(i..close + 1, i + 1..close))
}

/// `\\\(([^\n]+?)\\\)`: lazy and single-line — the first `\)` after at least one character
/// closes, unless a `\n` comes first, which no later close could skip either.
pub(super) fn inline_paren(t: &str, i: usize) -> Option<Found> {
    let b = t.as_bytes();
    if !b[i..].starts_with(b"\\(") {
        return None;
    }
    let content = i + 2;
    if content >= b.len() || b[content] == b'\n' {
        return None;
    }
    // Byte-wise from the second content byte: `\)` and `\n` are ASCII, so they are never found
    // inside a multi-byte character, and a first character of several bytes is skipped whole.
    let mut j = content + 1;
    while j < b.len() {
        if b[j..].starts_with(b"\\)") {
            return Some(one_group(i..j + 2, content..j));
        }
        if b[j] == b'\n' {
            return None;
        }
        j += 1;
    }
    None
}

// MARK: - Images and links

/// ICU's `\s` as NSRegularExpression evaluates it: the five ASCII controls, NEL and every `Z`
/// (.NET's categories, which put U+200B, U+FEFF and U+180E in `Cf`).
fn is_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}')
        || matches!(
            unicode::category(c),
            GeneralCategory::SpaceSeparator | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator
        )
}

/// ICU's `.` without DOTALL: every line terminator excluded, the zero-width characters not.
fn is_dot(c: char) -> bool {
    !matches!(c, '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

/// `!\[([^\]]*)\]\(` (an image) or `\[([^\]]+)\]\(` (a link) at `i`: the label runs to the first
/// `]`, which must be followed by `(`. Returns the label and where the target starts. The greedy
/// label can only stop at the first `]` — a shorter label would put a non-`]` where `\]` must
/// match — so there is nothing to backtrack into.
fn label(t: &str, i: usize, image: bool) -> Option<(Range<usize>, usize)> {
    let b = t.as_bytes();
    let start = if image {
        if !b[i..].starts_with(b"![") {
            return None;
        }
        i + 2
    } else {
        if b[i] != b'[' {
            return None;
        }
        i + 1
    };
    let close = find_from(t, "]", start)?;
    if !image && close == start {
        return None;
    }
    if b.get(close + 1) != Some(&b'(') {
        return None;
    }
    Some((start..close, close + 2))
}

/// `([^)\s]+)`: the maximal run of characters that are neither `)` nor ICU `\s`, at least one.
fn url_end(t: &str, start: usize) -> Option<usize> {
    let length: usize = t[start..].chars().take_while(|&c| c != ')' && !is_ws(c)).map(char::len_utf8).sum();
    (length > 0).then_some(start + length)
}

/// `label \( ([^)\s]+) \)` — the untitled form. The URL run stops at the first `)` or space; only
/// a `)` lets the match go on, and a shorter URL would leave a URL character where `\)` must
/// match. Groups: the label, the URL.
fn untitled(t: &str, i: usize, image: bool) -> Option<Found> {
    let (label, url_start) = label(t, i, image)?;
    let url = url_end(t, url_start)?;
    if t.as_bytes().get(url) != Some(&b')') {
        return None;
    }
    Some(found(i..url + 1, vec![label, url_start..url]))
}

/// `label \( ([^)\s]+) [\s]+ "(.*?)" \)` — the titled form, run before the untitled one or that
/// one stops at the first `)` and the title leaks. The URL run must end at a space, the space run
/// must end at `"` (neither greedy run can give anything back that the next element would
/// accept), and the lazy title closes at the first `")` unless a line terminator comes first.
/// Groups: the label, the URL, the title.
fn titled(t: &str, i: usize, image: bool) -> Option<Found> {
    let (label, url_start) = label(t, i, image)?;
    let url = url_end(t, url_start)?;
    let spaces: usize = t[url..].chars().take_while(|&c| is_ws(c)).map(char::len_utf8).sum();
    if spaces == 0 {
        return None;
    }
    let quote = url + spaces;
    if t.as_bytes().get(quote) != Some(&b'"') {
        return None;
    }
    let title = quote + 1;
    for (offset, c) in t[title..].char_indices() {
        let p = title + offset;
        if t[p..].starts_with("\")") {
            return Some(found(i..p + 2, vec![label, url_start..url, title..p]));
        }
        if !is_dot(c) {
            return None;
        }
    }
    None
}

/// `!\[([^\]]*)\]\(([^)\s]+)[\s]+"(.*?)"\)`.
pub(super) fn image_titled(t: &str, i: usize) -> Option<Found> {
    titled(t, i, true)
}

/// `!\[([^\]]*)\]\(([^)\s]+)\)`.
pub(super) fn image(t: &str, i: usize) -> Option<Found> {
    untitled(t, i, true)
}

/// `\[([^\]]+)\]\(([^)\s]+)[\s]+"(.*?)"\)`.
pub(super) fn link_titled(t: &str, i: usize) -> Option<Found> {
    titled(t, i, false)
}

/// `\[([^\]]+)\]\(([^)\s]+)\)`.
pub(super) fn link(t: &str, i: usize) -> Option<Found> {
    untitled(t, i, false)
}

// MARK: - Footnotes and emphasis

/// `\[\^([A-Za-z0-9_-]+)\]` — the ASCII ids the parser accepts for a definition.
pub(super) fn footnote(t: &str, i: usize) -> Option<Found> {
    let b = t.as_bytes();
    if !b[i..].starts_with(b"[^") {
        return None;
    }
    let start = i + 2;
    let mut end = start;
    while end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'_' || b[end] == b'-') {
        end += 1;
    }
    if end == start || b.get(end) != Some(&b']') {
        return None;
    }
    Some(one_group(i..end + 1, start..end))
}

/// `\*\*([^*]+)\*\*`.
pub(super) fn bold(t: &str, i: usize) -> Option<Found> {
    delimited_run(t, i, b"**")
}

/// `__([^_]+)__`.
pub(super) fn bold_underscore(t: &str, i: usize) -> Option<Found> {
    delimited_run(t, i, b"__")
}

/// `~~([^~]+)~~`.
pub(super) fn strike(t: &str, i: usize) -> Option<Found> {
    delimited_run(t, i, b"~~")
}

/// `\*([^*]+)\*`.
pub(super) fn italic(t: &str, i: usize) -> Option<Found> {
    delimited_run(t, i, b"*")
}

/// `_([^_]+)_`, the guard ([`underscore_guard`]) checked separately.
pub(super) fn italic_underscore(t: &str, i: usize) -> Option<Found> {
    delimited_run(t, i, b"_")
}

/// The delimiter, a non-empty run free of its character, the delimiter again. The content can
/// only end at the first delimiter character after the opener, so if the full closing delimiter
/// is not there, nothing is.
fn delimited_run(t: &str, i: usize, delimiter: &[u8]) -> Option<Found> {
    let b = t.as_bytes();
    if !b[i..].starts_with(delimiter) {
        return None;
    }
    let mark = delimiter[0];
    let start = i + delimiter.len();
    let mut close = start;
    while close < b.len() && b[close] != mark {
        close += 1;
    }
    if close == start || !b[close..].starts_with(delimiter) {
        return None;
    }
    Some(one_group(i..close + delimiter.len(), start..close))
}

// MARK: - The word guards

/// `(?<![\p{L}\p{N}_$])` … `(?![\p{L}\p{N}_$])` around an inline formula.
pub(super) fn dollar_guard(text: &str, found: &Found) -> bool {
    not_word_around(text, found, true)
}

/// `(?<![\p{L}\p{N}_])` … `(?![\p{L}\p{N}_])` around an underscore italic.
pub(super) fn underscore_guard(text: &str, found: &Found) -> bool {
    not_word_around(text, found, false)
}

/// Checked on whole scalars: md.win's regex engine is code-unit based, its `\p{L}` cannot see a
/// supplementary-plane letter, so it decodes the pair by hand — a Rust `char` is the scalar
/// already. (md.win's lone-surrogate case, "neither letter nor number", cannot arise in a `str`.)
fn not_word_around(text: &str, found: &Found, dollar_too: bool) -> bool {
    if text[..found.whole.start].chars().next_back().is_some_and(|c| is_word_scalar(c, dollar_too)) {
        return false;
    }
    if text[found.whole.end..].chars().next().is_some_and(|c| is_word_scalar(c, dollar_too)) {
        return false;
    }
    true
}

/// The spelled-out `[\p{L}\p{N}_]` guard class (plus `$` for the formula pattern) — not `\w`,
/// which is a different set on every engine. `\p{N}` is the three number categories, not the
/// digits: `Ⅰ` (Nl), `½` (No) and `۱` (Nd) all stop a delimiter.
fn is_word_scalar(c: char, dollar_too: bool) -> bool {
    if c == '_' {
        return true;
    }
    if dollar_too && c == '$' {
        return true;
    }
    unicode::is_letter(c) || unicode::is_number(c)
}
