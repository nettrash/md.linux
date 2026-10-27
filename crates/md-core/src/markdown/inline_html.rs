//! The span pass — port of md.win `Md.Core.Markdown.MarkdownInlineHtml`, itself the port of the
//! private `inline` / `protect` / `token` / `mathSpan` / `escape` / `replace` family in
//! `MarkdownHTML.swift`. This function, not a browser delimiter scan, decides what is a formula;
//! that is why `rich/auto-render.min.js` is bundled by the apps and referenced by nothing.
//!
//! THE PHASE ORDER IS THE SPECIFICATION. Any reordering silently changes real documents:
//!
//! 1. protect: code spans → `$$…$$` → `\[…\]` → `$…$` (currency guard) → `\(…\)`
//! 2. escape the whole remaining literal text (`& < > "`)
//! 3. spans: img(title) → img → link(title) → link → footnote ref → `**` → `__` → `~~` → `*` → `_`
//! 4. soft breaks → `<br>\n` (paragraphs only)
//! 5. restore the protected spans, ascending, by literal replace
//!
//! Everything is escaped before phase 3, so every later pattern speaks in *escaped* terms — a link
//! title reads `&quot;…&quot;`.
//!
//! md.win runs .NET regexes for most of these, md.macOS ICU's (NSRegularExpression); md-core has
//! no regex engine, so every pattern is a hand-written matcher that reproduces the leftmost,
//! non-overlapping, replace-all semantics of the regex it stands in for (each one's comment spells
//! the pattern and why its backtracking cannot change the answer). Three of ICU's classes are not
//! the obvious ones, and are spelled out exactly as md.win spells them: `\w` is
//! `[\p{Alphabetic}\p{M}\p{Nd}\p{Pc} U+200C U+200D]`, asked of the whole code point on each side
//! (`is_word`); `\s` is `[\t\n\v\f\r\x85\p{Z}]` (measured; NEL and VT in, U+200B and U+FEFF out —
//! `is_space`); `.` excludes the seven line terminators, not just `\n` (`is_dot`). Kotlin spells
//! the word guard `[\p{L}\p{N}_$]` and both LaTeX writers `[\p{L}\p{N}_]`; they disagree with the
//! Swift on `²$x$` and `é$x$` (NFD). This file follows the Swift HTML writer, whose bytes the
//! golden corpus was captured from — port spec OQ-3, recorded, not resolved.
//!
//! Offsets are UTF-8 bytes. Every pattern opens with an ASCII literal, so a match can only start
//! on a char boundary, and every match ends after an ASCII literal; each matcher tests the byte at
//! a position before slicing there.

use std::ops::Range;

use crate::text::scalar_text;
use crate::text::unicode::{self, GeneralCategory};

const TOKEN_OPEN: char = '\u{E000}';
const TOKEN_CLOSE: char = '\u{E001}';

/// A block's inline Markdown to HTML. See the module comment for the phase order, which is the
/// specification.
pub fn inline(text: &str, soft_breaks: bool) -> String {
    let mut protected_spans: Vec<String> = Vec::new();

    // 1. Protect. Code wins over math, so `$x$` inside backticks stays literal code; the inline
    //    `$…$` form carries the currency guard so "$5 and $10" is left as prose. A footnote
    //    reference needs no protection: `[^id]` has no `](`, so no link or image pattern can match
    //    it, and one inside backticks is already a code token by now.
    let mut working = text.to_string();
    working = protect(&code_span_matches(&working), &working, &mut protected_spans, |c| {
        format!("<code>{}</code>", escape(c))
    });
    working = protect(&display_dollar_matches(&working), &working, &mut protected_spans, |c| math_span(c, true));
    working = protect(&display_bracket_matches(&working), &working, &mut protected_spans, |c| math_span(c, true));
    working = protect(&inline_dollar_matches(&working), &working, &mut protected_spans, |c| math_span(c, false));
    working = protect(&inline_paren_matches(&working), &working, &mut protected_spans, |c| math_span(c, false));

    // 2. Escape the literal text; the private-use tokens pass through untouched.
    working = escape(&working);

    // 3. Span syntax → tags, every one a replace-all.
    working = replace_all(&working, |t, i| {
        let (end, alt, src, title) = titled_target(t, i, true)?;
        Some((end, format!("<img src=\"{}\" alt=\"{}\" title=\"{}\">", &t[src], &t[alt], &t[title])))
    });
    working = replace_all(&working, |t, i| {
        let (end, alt, src) = untitled_target(t, i, true)?;
        Some((end, format!("<img src=\"{}\" alt=\"{}\">", &t[src], &t[alt])))
    });
    working = replace_all(&working, |t, i| {
        let (end, label, href, title) = titled_target(t, i, false)?;
        Some((end, format!("<a href=\"{}\" title=\"{}\">{}</a>", &t[href], &t[title], &t[label])))
    });
    working = replace_all(&working, |t, i| {
        let (end, label, href) = untitled_target(t, i, false)?;
        Some((end, format!("<a href=\"{}\">{}</a>", &t[href], &t[label])))
    });
    //    Footnote references after images and links, and must: their markup is full of quotes and
    //    angle brackets, so converting first would carry it into an `alt` or nest `<a>` in `<a>`.
    //    Running last, a reference inside a link label merely stops the label being a link. The
    //    number is not known yet — it depends on first-reference order across the whole document —
    //    so this leaves a placeholder for the writer's footnote pass.
    working = replace_all(&working, |t, i| {
        let (end, id) = footnote_ref(t, i)?;
        Some((end, format!("<sup class=\"md-fnref\" data-fn=\"{}\"></sup>", &t[id])))
    });
    working = replace_all(&working, |t, i| {
        let (end, content) = delimited_run(t, i, b"**")?;
        Some((end, format!("<strong>{}</strong>", &t[content])))
    });
    working = replace_all(&working, |t, i| {
        let (end, content) = delimited_run(t, i, b"__")?;
        Some((end, format!("<strong>{}</strong>", &t[content])))
    });
    working = replace_all(&working, |t, i| {
        let (end, content) = delimited_run(t, i, b"~~")?;
        Some((end, format!("<del>{}</del>", &t[content])))
    });
    working = replace_all(&working, |t, i| {
        let (end, content) = delimited_run(t, i, b"*")?;
        Some((end, format!("<em>{}</em>", &t[content])))
    });
    working = emphasis_underscores(&working);

    // 4. Soft breaks (paragraphs only), before restore so a multi-line display formula keeps its
    //    own newlines rather than getting `<br>`s injected.
    if soft_breaks {
        working = working.replace('\n', "<br>\n");
    }

    // 5. Restore, ascending, by LITERAL replace — never a template replace, or a protected `$$`
    //    would be read as one.
    //
    //    KNOWN LATENT BUG, REPRODUCED DELIBERATELY: a later protect pass can swallow an earlier
    //    pass's token (a code span inside a math span), and because restore runs ascending the
    //    enclosed token is restored before its enclosing HTML is back in `working`, so the raw
    //    U+E000 / U+E001 survive: `$$ `x` $$` → `<span class="md-mathd"> U+E000 0 U+E001 </span>`.
    //    Every shipping app does this; byte parity is the contract (OQ-6).
    for (index, span) in protected_spans.iter().enumerate() {
        working = working.replace(&token(index), span);
    }
    working
}

/// The four-character HTML escape, `&` first. `'` is deliberately not escaped; `"` is, so a link
/// URL landing in a double-quoted `href` cannot break out — that one line is the whole XSS story
/// here. No raw-HTML passthrough, no autolinks, anywhere. Ordinal, never grapheme-aware: an `<`
/// followed by a combining mark is escaped too (Swift's cluster-matching replace leaves it, and md
/// follows TypeScript and Kotlin there on purpose — see the adversarial tests).
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// A KaTeX target. The LaTeX is escaped for HTML; KaTeX reads the decoded `textContent`.
fn math_span(latex: &str, display: bool) -> String {
    format!("<span class=\"md-math{}\">{}</span>", if display { "d" } else { "i" }, escape(latex))
}

/// U+E000, the decimal index, U+E001. Private-use, so `escape` never touches it and no span
/// pattern matches it.
fn token(index: usize) -> String {
    format!("{TOKEN_OPEN}{index}{TOKEN_CLOSE}")
}

/// One protect-pattern match: the whole match and its group 1.
struct Span {
    whole: Range<usize>,
    content: Range<usize>,
}

/// Replace every match with a unique token, appending `transform(group1)` to the store. md.win
/// rewrites BACK-TO-FRONT so earlier ranges stay valid while the index counts forward — so the
/// *last* match in the text takes the *lowest* index of the pass. This builds the same string
/// front to back and hands out the indices in that same reverse order. Kotlin walks forward; the
/// difference is visible only through the token leak, where the leaked digits differ. Swift and
/// TypeScript are followed.
fn protect(spans: &[Span], text: &str, store: &mut Vec<String>, transform: impl Fn(&str) -> String) -> String {
    if spans.is_empty() {
        return text.to_string();
    }
    let base = store.len();
    for span in spans.iter().rev() {
        store.push(transform(&text[span.content.clone()]));
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (i, span) in spans.iter().enumerate() {
        out.push_str(&text[last..span.whole.start]);
        out.push_str(&token(base + spans.len() - 1 - i));
        last = span.whole.end;
    }
    out.push_str(&text[last..]);
    out
}

/// Every non-overlapping match of a pattern, leftmost first: `at(text, i)` tries a match starting
/// at byte `i` and returns its end and group 1; a failed start moves on one position, a match
/// resumes after its end — the regex engine's scan.
fn all_matches(text: &str, at: impl Fn(&str, usize) -> Option<(usize, Range<usize>)>) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if let Some((end, content)) = at(text, i) {
            spans.push(Span { whole: i..end, content });
            i = end;
        } else {
            i += 1;
        }
    }
    spans
}

/// A regex replace-all over `at`, which returns a match's end and its replacement text.
fn replace_all(text: &str, at: impl Fn(&str, usize) -> Option<(usize, String)>) -> String {
    let mut out: Option<String> = None;
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        if let Some((end, replacement)) = at(text, i) {
            let out = out.get_or_insert_with(|| String::with_capacity(text.len() + 32));
            out.push_str(&text[last..i]);
            out.push_str(&replacement);
            last = end;
            i = end;
        } else {
            i += 1;
        }
    }
    match out {
        None => text.to_string(),
        Some(mut out) => {
            out.push_str(&text[last..]);
            out
        }
    }
}

/// The first occurrence of `needle` at or after byte `from`.
fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    scalar_text::first_index(text, needle, from)
}

// MARK: - Phase 1 patterns

/// `` `([^`]+)` ``: the content runs to the next backtick and may not be empty. A failed opener
/// (two backticks in a row) is retried one position on, where the second backtick opens.
fn code_span_matches(text: &str) -> Vec<Span> {
    all_matches(text, |t, i| {
        if t.as_bytes()[i] != b'`' {
            return None;
        }
        let close = find_from(t, "`", i + 1)?;
        (close > i + 1).then(|| (close + 1, i + 1..close))
    })
}

/// `\$\$([\s\S]+?)\$\$`: lazy, so the first `$$` after at least one character of content closes.
/// (`$$` can only begin on a char boundary, so searching from the byte after the opener's first
/// content byte is searching from after its first content character.)
fn display_dollar_matches(text: &str) -> Vec<Span> {
    all_matches(text, |t, i| lazy_pair(t, i, "$$", "$$"))
}

/// `\\\[([\s\S]+?)\\\]`.
fn display_bracket_matches(text: &str) -> Vec<Span> {
    all_matches(text, |t, i| lazy_pair(t, i, "\\[", "\\]"))
}

fn lazy_pair(t: &str, i: usize, open: &str, close: &str) -> Option<(usize, Range<usize>)> {
    if !t.as_bytes()[i..].starts_with(open.as_bytes()) {
        return None;
    }
    let content = i + open.len();
    let end = find_from(t, close, content + 1)?;
    Some((end + close.len(), content..end))
}

/// `\\\(([^\n]+?)\\\)`: lazy and single-line — the first `\)` after at least one character closes,
/// unless a `\n` comes first, which no later close could skip either.
fn inline_paren_matches(text: &str) -> Vec<Span> {
    all_matches(text, |t, i| {
        let b = t.as_bytes();
        if !b[i..].starts_with(b"\\(") {
            return None;
        }
        let content = i + 2;
        if content >= b.len() || b[content] == b'\n' {
            return None;
        }
        let mut j = content + 1;
        while j < b.len() {
            if b[j..].starts_with(b"\\)") {
                return Some((j + 2, content..j));
            }
            if b[j] == b'\n' {
                return None;
            }
            j += 1;
        }
        None
    })
}

/// `(?<![\w$])\$([^$\n]+?)\$(?![\w$])` — the currency guard, the most load-bearing pattern in the
/// file. A delimiting `$` may be neither preceded nor followed by a word character or another `$`;
/// no newline inside. Because the content class excludes `$`, the only possible close is the first
/// `$` after the opener, so the scan is exactly the regex: a failed guard moves on one position, a
/// match resumes after its close.
fn inline_dollar_matches(text: &str) -> Vec<Span> {
    let b = text.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' || (i > 0 && (b[i - 1] == b'$' || is_word_before(text, i))) {
            i += 1;
            continue;
        }
        let mut close = i + 1;
        while close < b.len() && b[close] != b'$' && b[close] != b'\n' {
            close += 1;
        }
        if close >= b.len()
            || b[close] != b'$'
            || close == i + 1
            || (close + 1 < b.len() && (b[close + 1] == b'$' || is_word_after(text, close + 1)))
        {
            i += 1;
            continue;
        }
        spans.push(Span { whole: i..close + 1, content: i + 1..close });
        i = close + 1;
    }
    spans
}

// MARK: - Phase 3 patterns

/// ICU's `\s`, spelled out: `White_Space` = the five ASCII controls, NEL and every Z*.
fn is_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}')
        || matches!(
            unicode::category(c),
            GeneralCategory::SpaceSeparator | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator
        )
}

/// ICU's `.` without DOTALL: anything but the seven line terminators (U+2028 is the only Zl,
/// U+2029 the only Zp).
fn is_dot(c: char) -> bool {
    !matches!(c, '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}')
        && !matches!(unicode::category(c), GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator)
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
    let length: usize = t[start..].chars().take_while(|&c| c != ')' && !is_space(c)).map(char::len_utf8).sum();
    (length > 0).then_some(start + length)
}

/// `label \( ([^)\s]+) \)` — the untitled image (`image`) or link. The URL run stops at the first
/// `)` or space; only a `)` lets the match go on, and a shorter URL would leave a URL character
/// where `\)` must match.
fn untitled_target(t: &str, i: usize, image: bool) -> Option<(usize, Range<usize>, Range<usize>)> {
    let (label, url_start) = label(t, i, image)?;
    let url = url_end(t, url_start)?;
    if t.as_bytes().get(url) != Some(&b')') {
        return None;
    }
    Some((url + 1, label, url_start..url))
}

/// `label \( ([^)\s]+) [\s]+ &quot;(.*?)&quot;\)` — the titled image or link, run before the
/// untitled form or that one stops at the first `)` and the title leaks. The URL run must end at a
/// space, the space run must end at `&quot;` (neither greedy run can give anything back that the
/// next element would accept), and the lazy title closes at the first `&quot;)` unless a line
/// terminator comes first.
fn titled_target(t: &str, i: usize, image: bool) -> Option<(usize, Range<usize>, Range<usize>, Range<usize>)> {
    let (label, url_start) = label(t, i, image)?;
    let url = url_end(t, url_start)?;
    let spaces: usize = t[url..].chars().take_while(|&c| is_space(c)).map(char::len_utf8).sum();
    if spaces == 0 {
        return None;
    }
    let quote = url + spaces;
    if !t[quote..].starts_with("&quot;") {
        return None;
    }
    let title = quote + "&quot;".len();
    for (offset, c) in t[title..].char_indices() {
        let p = title + offset;
        if t[p..].starts_with("&quot;)") {
            return Some((p + "&quot;)".len(), label, url_start..url, title..p));
        }
        if !is_dot(c) {
            return None;
        }
    }
    None
}

/// `\[\^([A-Za-z0-9_-]+)\]` — ASCII ids because the parser accepts exactly that set for a
/// definition: nothing to escape into an `id` attribute, and no definition can exist that no
/// reference can name.
fn footnote_ref(t: &str, i: usize) -> Option<(usize, Range<usize>)> {
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
    Some((end + 1, start..end))
}

/// `\*\*([^*]+)\*\*`, `__([^_]+)__`, `~~([^~]+)~~` and `\*([^*]+)\*`: the delimiter, a non-empty
/// run free of its character, the delimiter again. The content can only end at the first
/// delimiter character after the opener, so if the full closing delimiter is not there, nothing
/// is.
fn delimited_run(t: &str, i: usize, delimiter: &[u8]) -> Option<(usize, Range<usize>)> {
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
    Some((close + delimiter.len(), start..close))
}

/// `(?<![\w])_([^_]+)_(?![\w])` → `<em>$1</em>`, replace-all: the word-boundary guard that lets
/// `snake_case` survive. `__bold__` was consumed by the pass before. The content class excludes
/// `_`, so the close is the first `_` after the opener and backtracking cannot rescue a failed
/// guard.
fn emphasis_underscores(text: &str) -> String {
    let b = text.as_bytes();
    let mut builder: Option<String> = None;
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'_' || (i > 0 && is_word_before(text, i)) {
            i += 1;
            continue;
        }
        let mut close = i + 1;
        while close < b.len() && b[close] != b'_' {
            close += 1;
        }
        if close >= b.len() || close == i + 1 || (close + 1 < b.len() && is_word_after(text, close + 1)) {
            i += 1;
            continue;
        }
        let out = builder.get_or_insert_with(|| String::with_capacity(text.len() + 16));
        out.push_str(&text[last..i]);
        out.push_str("<em>");
        out.push_str(&text[i + 1..close]);
        out.push_str("</em>");
        last = close + 1;
        i = close + 1;
    }
    match builder {
        None => text.to_string(),
        Some(mut out) => {
            out.push_str(&text[last..]);
            out
        }
    }
}

/// The code point ending just before byte `index` (a char boundary) is a word character.
fn is_word_before(text: &str, index: usize) -> bool {
    text[..index].chars().next_back().is_some_and(is_word)
}

/// The code point starting at byte `index` (a char boundary) is a word character.
fn is_word_after(text: &str, index: usize) -> bool {
    text[index..].chars().next().is_some_and(is_word)
}

/// ICU's `\w`: `Alphabetic` (L*, Nl, the marks and the enclosed Latin letters that carry
/// `Other_Alphabetic`), M*, Nd, Pc, ZWNJ and ZWJ. `ф`, `٣`, a combining acute, `_` and `Ⅹ` are
/// word characters; `½` (No), `²` (No) and `-` are not. Asked of a whole scalar, so `𝐀` counts as
/// ICU and TypeScript count it.
fn is_word(c: char) -> bool {
    if unicode::is_letter(c) || scalar_text::is_mark(c) || scalar_text::is_enclosed_alphabetic(c) {
        return true;
    }
    if c == '\u{200C}' || c == '\u{200D}' {
        return true;
    }
    matches!(
        unicode::category(c),
        GeneralCategory::LetterNumber | GeneralCategory::DecimalDigitNumber | GeneralCategory::ConnectorPunctuation
    )
}
