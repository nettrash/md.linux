//! The two pure keystroke helpers behind list continuation and sentence capitalization in the
//! editor: [`enter`] decides what Return does on a list item, a quote line or a table row, and
//! [`capitalize`] decides whether the one lowercase letter being typed starts a line or a
//! sentence. Both are SmartTyping, the specification every md port implements
//! (`docs/smart-typing.md`), ported function for function from md.win's
//! `Md.Core.Text.SmartTyping`, itself a port of the TypeScript reference that generated the
//! shared vectors.
//!
//! **The vectors are the contract.** `tests/fixtures/typing-vectors.json` holds 1350 vectors
//! (512 `enter`, 838 `capitalize`), byte-identical in md, md.macOS, md.Android, md.win and here.
//! No port may diverge from a vector; a disputed vector is changed in the JSON first, in one
//! commit across all repos. Where the prose of the specification and a vector disagree, the
//! vector wins and the disagreement is recorded — the one md.win carries is in `upper`.
//!
//! **Character model.** Every offset in, out and in the vectors is a UTF-16 code unit (C#'s
//! `string.Length`), the same model as Kotlin and the reference; the Swift port works in
//! `NSString` offsets for the same reason. The rules run on [`Utf16Text`] so that their index
//! arithmetic reads line for line like the C#; the `&str` entry points convert on the way in.
//! Where a rule reads a *scalar* (a letter, a mark, a symbol, the typed character) the pair is
//! decoded by hand and a lone surrogate is its own scalar of category `Cs`, which belongs to no
//! class. Character classes are Unicode general categories as .NET answers them
//! ([`crate::text::unicode`]) — never `char::is_alphabetic` / `char::is_lowercase`, which
//! include `Other_Alphabetic` / `Other_Lowercase`. Digits are ASCII `0–9`. "Whitespace" is
//! always WS19, the nineteen units of Foundation's `.whitespaces` spelled out in `is_ws19`,
//! and never `char::is_whitespace`, `str::trim` or a regex `\s` — the family has already
//! shipped a divergence from "whitespace" having four answers. Every comparison is ordinal;
//! nothing normalises; there are no regular expressions (§0.8: ICU, `java.util.regex` and .NET
//! disagree on `\s`, `\w` and `\b`).
//!
//! **Structure.** The reference materialises a line array (its own comment says §0.14 forbids
//! that in a port, not in the oracle). This port keeps the reference's shape so that the two can
//! be read side by side, but the "line array" is a table of `(start, end)` offsets built in one
//! forward pass, and every line is read as a slice of the original units — no copy per line.
//! The state scan (§0.10) is one pass to the caret's line; every other walk is bounded by the
//! caret's run (the lines between the nearest blank line above and the caret) or by the caret's
//! line, and the table test of a line is memoised per call so a run of a thousand items stays
//! linear. The three reads below the caret that §0.14 names — the front-matter closer, the
//! delimiter row under a header, and the "table continues below" line — are the only ones.
//!
//! **Deliberate divergences from what md renders** are listed in the specification's §5 with a
//! vector id each (fences and comments inside quotes, a table directly under a paragraph, the
//! bare-marker line, code spans reset per line, …) and §6 lists where md's own parser differs
//! from CommonMark. None of them is a bug to fix here: fixing one side toward CommonMark or
//! toward the renderer breaks the vectors on five platforms.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::text::unicode::{self, GeneralCategory};
use crate::text::utf16::Utf16Text;

/// One undoable edit that [`enter`] asks the platform to perform on the *original* text: replace
/// `[location, location + length)` (UTF-16 code units) with `replacement` and place a collapsed
/// caret at `caret`, a UTF-16 offset into the text after the edit. The replacement always uses
/// U+000A, even in a CRLF document (§0.1); the adapter normalises if the control needs it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EnterEdit {
    pub location: usize,
    pub length: usize,
    pub replacement: String,
    pub caret: usize,
}

// MARK: - §0.1 units and scalars

const SP: u16 = 0x20;
const TAB: u16 = 0x09;
const CR: u16 = 0x0D;
const LF: u16 = 0x0A;

const BANG: u16 = b'!' as u16;
const HASH: u16 = b'#' as u16;
const DOLLAR: u16 = b'$' as u16;
const RPAREN: u16 = b')' as u16;
const LPAREN: u16 = b'(' as u16;
const STAR: u16 = b'*' as u16;
const DASH: u16 = b'-' as u16;
const DOT: u16 = b'.' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const EQUALS: u16 = b'=' as u16;
const GT: u16 = b'>' as u16;
const AT_SIGN: u16 = b'@' as u16;
const LBRACKET: u16 = b'[' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const RBRACKET: u16 = b']' as u16;
const CARET: u16 = b'^' as u16;
const UNDERSCORE: u16 = b'_' as u16;
const BACKTICK: u16 = b'`' as u16;
const PIPE: u16 = b'|' as u16;
const TILDE: u16 = b'~' as u16;

/// The unit at `i`, or `None` past the end — the reference's `charCodeAt` NaN, which compares
/// equal to nothing.
fn u(s: &[u16], i: usize) -> Option<u16> {
    s.get(i).copied()
}

fn is_high(u: u16) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}

/// The scalar starting at unit `i` and its unit length; a lone surrogate is its own scalar.
fn scalar_at(s: &[u16], i: usize) -> (u32, usize) {
    let u = s[i];
    if is_high(u) && i + 1 < s.len() && is_low(s[i + 1]) {
        return (0x10000 + ((u32::from(u) - 0xD800) << 10) + (u32::from(s[i + 1]) - 0xDC00), 2);
    }
    (u32::from(u), 1)
}

/// The scalar ending just before unit `i` and its unit length, or `None` at the start.
fn scalar_before(s: &[u16], i: usize) -> Option<(u32, usize)> {
    if i == 0 {
        return None;
    }
    let u = s[i - 1];
    if is_low(u) && i >= 2 && is_high(s[i - 2]) {
        return Some(scalar_at(s, i - 2));
    }
    Some((u32::from(u), 1))
}

fn scalars_of(s: &[u16]) -> Vec<u32> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (cp, len) = scalar_at(s, i);
        out.push(cp);
        i += len;
    }
    out
}

// MARK: - ordinal ASCII helpers (every needle this file searches for is ASCII)

fn eq_ascii(s: &[u16], a: &str) -> bool {
    s.len() == a.len() && s.iter().zip(a.bytes()).all(|(&x, y)| x == u16::from(y))
}

fn starts_with_ascii(s: &[u16], a: &str) -> bool {
    s.len() >= a.len() && eq_ascii(&s[..a.len()], a)
}

fn find_ascii(s: &[u16], a: &str) -> Option<usize> {
    if a.len() > s.len() {
        return None;
    }
    (0..=s.len() - a.len()).find(|&i| eq_ascii(&s[i..i + a.len()], a))
}

fn rfind_ascii(s: &[u16], a: &str) -> Option<usize> {
    if a.len() > s.len() {
        return None;
    }
    (0..=s.len() - a.len()).rev().find(|&i| eq_ascii(&s[i..i + a.len()], a))
}

fn units(a: &str) -> impl Iterator<Item = u16> + '_ {
    a.bytes().map(u16::from)
}

// MARK: - §0.3 WS19, blank, trim

/// WS19 — the nineteen scalars of Foundation's `.whitespaces` (U+0009, U+0020, U+00A0, U+1680,
/// U+2000–U+200A, U+200B, U+202F, U+205F, U+3000), all BMP. The one meaning of "whitespace",
/// "blank" and "trim" in this file.
fn is_ws19(u: impl Into<u32>) -> bool {
    let u = u.into();
    u == 0x09
        || u == 0x20
        || u == 0xA0
        || u == 0x1680
        || (0x2000..=0x200A).contains(&u)
        || u == 0x200B
        || u == 0x202F
        || u == 0x205F
        || u == 0x3000
}

fn is_blank(s: &[u16]) -> bool {
    s.iter().all(|&c| is_ws19(c))
}

fn trim(s: &[u16]) -> &[u16] {
    let mut a = 0;
    let mut b = s.len();
    while a < b && is_ws19(s[a]) {
        a += 1;
    }
    while b > a && is_ws19(s[b - 1]) {
        b -= 1;
    }
    &s[a..b]
}

// MARK: - §0.4 character classes (general category of the scalar)

/// The general category of one scalar; a lone surrogate is `Cs`.
fn category(cp: u32) -> GeneralCategory {
    char::from_u32(cp).map_or(GeneralCategory::Surrogate, unicode::category)
}

fn is_letter(cp: u32) -> bool {
    matches!(
        category(cp),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

fn is_lowercase(cp: u32) -> bool {
    category(cp) == GeneralCategory::LowercaseLetter
}

fn is_uppercase(cp: u32) -> bool {
    category(cp) == GeneralCategory::UppercaseLetter
}

fn is_mark(cp: u32) -> bool {
    matches!(
        category(cp),
        GeneralCategory::NonSpacingMark | GeneralCategory::SpacingCombiningMark | GeneralCategory::EnclosingMark
    )
}

fn is_symbol_so_sk(cp: u32) -> bool {
    matches!(category(cp), GeneralCategory::OtherSymbol | GeneralCategory::ModifierSymbol)
}

fn is_digit(u: impl Into<u32>) -> bool {
    (0x30..=0x39).contains(&u.into())
}

// MARK: - §0.7 case mapping — simple, one-to-one, locale-independent

/// `upper(s)` for one scalar, or `None` when undefined (§0.7): a Georgian letter, a Greek letter
/// with ypogegrammeni, U+00B5 MICRO SIGN, a surrogate, or a scalar whose simple uppercase mapping
/// is itself (`ß`, `ŉ`, `ǰ`: their full mappings are two scalars, and the reference's one-scalar
/// guard rejects them the same way).
///
/// `Rune.ToUpperInvariant` ([`unicode::to_upper_invariant`], dumped from .NET) is the simple
/// mapping the specification names for C#, with one documented exception. .NET's invariant
/// casing pins U+0131 LATIN SMALL LETTER DOTLESS I to itself on every platform (`pal_casing.c`
/// special-cases it so ICU matches Windows NLS), while `UnicodeData.txt` maps it to U+0049 and
/// the vector `cap-typed-dotless-i` pins `ı` → `I`, as Swift, JavaScript and Java all do. The
/// vectors win, so that one scalar is written out here. (Rust's `char::to_uppercase` is the
/// *full* mapping and must not be used: it turns `ß` into `SS`.)
fn upper(cp: u32) -> Option<u32> {
    if (0x10D0..=0x10FF).contains(&cp) || (0x2D00..=0x2D2F).contains(&cp) {
        return None; // Georgian
    }
    if (0x1F80..=0x1FAF).contains(&cp) || cp == 0x1FB3 || cp == 0x1FC3 || cp == 0x1FF3 {
        return None; // ypogegrammeni
    }
    if cp == 0xB5 {
        return None; // MICRO SIGN: never GREEK CAPITAL MU in a unit prefix
    }
    if (0xD800..=0xDFFF).contains(&cp) {
        return None;
    }
    if cp == 0x0131 {
        return Some(0x0049); // dotless i: the Unicode simple mapping, not .NET's invariant pin
    }
    let c = char::from_u32(cp)?;
    let mapped = u32::from(unicode::to_upper_invariant(c));
    (mapped != cp).then_some(mapped)
}

// MARK: - §0.2 lines

/// One line as unit offsets; `end` excludes the terminator.
#[derive(Clone, Copy, Debug)]
struct Line {
    start: usize,
    end: usize,
}

/// The text plus its line table, and the per-call memo of [`table_context`]. Lines are never
/// copied: [`Doc::l`] is a slice of the text, borrowed from the text rather than from the `Doc`,
/// so the memo can be written while a line is held.
struct Doc<'a> {
    t: &'a [u16],
    lines: Vec<Line>,
    /// Per line asked: `None` no table, else `(header index, header cell count)`.
    table: HashMap<usize, Option<(usize, usize)>>,
}

impl<'a> Doc<'a> {
    fn new(t: &'a [u16]) -> Self {
        Doc { t, lines: split_lines(t), table: HashMap::new() }
    }

    fn count(&self) -> usize {
        self.lines.len()
    }

    fn l(&self, k: usize) -> &'a [u16] {
        let line = self.lines[k];
        &self.t[line.start..line.end]
    }
}

/// One forward pass: U+000A, U+000D U+000A (one terminator) or a lone U+000D end a line; nothing
/// else does.
fn split_lines(t: &[u16]) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < t.len() {
        let u = t[i];
        if u == CR {
            lines.push(Line { start, end: i });
            i += if i + 1 < t.len() && t[i + 1] == LF { 2 } else { 1 };
            start = i;
        } else if u == LF {
            lines.push(Line { start, end: i });
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    lines.push(Line { start, end: t.len() });
    lines
}

/// Index of the line containing `caret`, or `None` when the caret sits strictly between the CR
/// and the LF of one terminator (§0.2). Lines never overlap, so the last line whose start is at
/// or before the caret is the only candidate.
fn line_of(lines: &[Line], caret: usize) -> Option<usize> {
    let mut lo = 0;
    let mut hi = lines.len() - 1;
    while lo < hi {
        let mid = (lo + hi + 1) >> 1;
        if lines[mid].start <= caret {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (caret <= lines[lo].end).then_some(lo)
}

// MARK: - §0.6 columns

fn columns(run: &[u16]) -> usize {
    let mut col = 0;
    for &c in run {
        if c == TAB {
            col += 4 - (col % 4);
        } else {
            col += 1;
        }
    }
    col
}

// MARK: - §0.12 block tests on one line

fn is_thematic_break(s: &[u16]) -> bool {
    let mut n = 0;
    let mut ch: Option<u16> = None;
    for &c in s {
        if c == SP || c == TAB {
            continue;
        }
        if c != DASH && c != STAR && c != UNDERSCORE {
            return false;
        }
        match ch {
            None => ch = Some(c),
            Some(first) if first != c => return false,
            Some(_) => {}
        }
        n += 1;
    }
    n >= 3
}

fn is_atx_heading(s: &[u16]) -> bool {
    let mut i = 0;
    while i < s.len() && s[i] == SP {
        i += 1;
    }
    let mut n = 0;
    while i < s.len() && s[i] == HASH {
        n += 1;
        i += 1;
    }
    if !(1..=6).contains(&n) {
        return false;
    }
    i == s.len() || s[i] == SP
}

fn is_page_break(s: &[u16]) -> bool {
    let t = trim(s);
    eq_ascii(t, "\\newpage") || eq_ascii(t, "\\pagebreak")
}

fn is_footnote_id_unit(u: u16) -> bool {
    (0x41..=0x5A).contains(&u) || (0x61..=0x7A).contains(&u) || is_digit(u) || u == DASH || u == UNDERSCORE
}

/// Length of the footnote prefix `[^id]:` + WS19* measured on `t` from `from` (the `[`), or
/// `None`.
fn footnote_prefix_length(t: &[u16], from: usize) -> Option<usize> {
    if !(u(t, from) == Some(LBRACKET) && u(t, from + 1) == Some(CARET)) {
        return None;
    }
    let mut i = from + 2;
    let id_start = i;
    while i < t.len() && is_footnote_id_unit(t[i]) {
        i += 1;
    }
    if i == id_start {
        return None;
    }
    if !(u(t, i) == Some(RBRACKET) && u(t, i + 1) == Some(COLON)) {
        return None;
    }
    i += 2;
    while i < t.len() && is_ws19(t[i]) {
        i += 1;
    }
    Some(i - from)
}

fn is_footnote_definition(s: &[u16]) -> bool {
    footnote_prefix_length(trim(s), 0).is_some()
}

fn leading_sp_count(s: &[u16]) -> usize {
    s.iter().take_while(|&&c| c == SP).count()
}

fn is_quote_line(s: &[u16]) -> bool {
    u(s, leading_sp_count(s)) == Some(GT)
}

fn is_comment_start(s: &[u16]) -> bool {
    starts_with_ascii(&s[leading_sp_count(s)..], "<!--")
}

// MARK: - §0.11 prefix grammar

/// The §0.11 prefix of one line as offsets into it: `indent0` is `[0, indent0)`, `quotes`
/// follows it, then `indent1`, so the row prefix (`indent0 + quotes + indent1`) is the line's
/// first [`Prefix::row_prefix_len`] units.
#[derive(Clone, Copy, Debug, Default)]
struct Prefix {
    indent0: usize,
    quotes_len: usize,
    /// Offset within quotes where the last group starts (§1.3c); `None` when there is none.
    last_group_start: Option<usize>,
    depth: usize,
    indent1: usize,
    /// After indent0 + quotes: the "inner text" (§0.13).
    inner_start: usize,
    has_marker: bool,
    /// `0` for an ordered marker.
    bullet: u16,
    number: u32,
    /// `.` or `)` for an ordered marker.
    delimiter: u16,
    /// `[ ]` | `[x]` | `[X]` followed by SP.
    has_box: bool,
    /// The content is exactly a box (`- [ ]`): content for the caret, a box for §1.3a / §1.4.
    box_at_end: bool,
    /// Where content starts.
    prefix_end: usize,
    /// §0.11 list indent in columns.
    list_indent_cols: usize,
    /// §0.11 list level = floor(columns / 2), the parser's `indent / 2`.
    list_level: usize,
}

impl Prefix {
    fn indent0_quotes_len(&self) -> usize {
        self.indent0 + self.quotes_len
    }

    fn row_prefix_len(&self) -> usize {
        self.indent0 + self.quotes_len + self.indent1
    }
}

/// Parse the §0.11 prefix of one line. A marker or box counts only when followed by SP: one at
/// the line end is content for both functions (the parser renders a bare `-` as an empty item,
/// but Enter must not delete a `2020.` line and a typed letter would land directly after the
/// unit — `-` + `a` is `-a`; §6 records the difference).
fn parse_prefix(s: &[u16]) -> Prefix {
    let mut p = Prefix::default();
    let mut i = 0;
    // indent0 := run of SP / TAB
    while i < s.len() && (s[i] == SP || s[i] == TAB) {
        i += 1;
    }
    p.indent0 = i;
    // quotes := ( run of SP, ">", at most one SP )*  — only when indent0 has no
    // TAB (the parser's `isQuote` drops SP only: `\t> x` is a paragraph).
    let quotes_start = i;
    let mut depth = 0;
    if !s[..i].contains(&TAB) {
        loop {
            let mut j = i;
            while j < s.len() && s[j] == SP {
                j += 1;
            }
            if j < s.len() && s[j] == GT {
                p.last_group_start = Some(i - quotes_start);
                j += 1;
                if j < s.len() && s[j] == SP {
                    j += 1;
                }
                i = j;
                depth += 1;
            } else {
                break;
            }
        }
    }
    p.quotes_len = i - quotes_start;
    p.depth = depth;
    // indent1 := run of SP / TAB, only after quotes
    let indent1_start = i;
    if depth > 0 {
        while i < s.len() && (s[i] == SP || s[i] == TAB) {
            i += 1;
        }
    }
    p.indent1 = i - indent1_start;
    p.inner_start = quotes_start + p.quotes_len; // inner text = line minus indent0 and quotes (§0.13)
    p.list_indent_cols =
        columns(if depth > 0 { &s[indent1_start..indent1_start + p.indent1] } else { &s[..p.indent0] });
    p.list_level = p.list_indent_cols / 2;
    let after_indent = i;
    p.prefix_end = after_indent;

    // marker is absent when the inner text is a thematic break (tested first, as the parser does)
    if !is_thematic_break(&s[p.inner_start..]) {
        let mut m = after_indent;
        let mut bullet = 0;
        let mut num = 0;
        let mut delim = 0;
        let mut ok = false;
        let first = u(s, m);
        if first == Some(DASH) || first == Some(STAR) || first == Some(b'+' as u16) {
            bullet = s[m];
            m += 1;
            ok = true;
        } else if first.is_some_and(is_digit) {
            let mut d = m;
            while d < s.len() && is_digit(s[d]) {
                d += 1;
            }
            let digits = d - m;
            let ud = u(s, d);
            if digits <= 9 && (ud == Some(DOT) || ud == Some(RPAREN)) {
                num = decimal_of(&s[m..m + digits]);
                delim = s[d];
                m = d + 1;
                ok = true;
            }
        }
        if ok {
            // followed by SP (a marker at the line end is content, §0.11)
            let followed_by_sp = m < s.len() && s[m] == SP;
            if followed_by_sp {
                p.has_marker = true;
                p.bullet = bullet;
                p.number = num;
                p.delimiter = delim;
                // the marker absorbs the run of SP after it (the parser drops every SP)
                while m < s.len() && s[m] == SP {
                    m += 1;
                }
                p.prefix_end = m;
                // box := "[ ]" | "[x]" | "[X]", followed by SP (same refinement)
                if m + 3 <= s.len()
                    && s[m] == LBRACKET
                    && s[m + 2] == RBRACKET
                    && (s[m + 1] == SP || s[m + 1] == b'x' as u16 || s[m + 1] == b'X' as u16)
                {
                    let mut e = m + 3;
                    let b_sp = e < s.len() && s[e] == SP;
                    if b_sp {
                        p.has_box = true;
                        while e < s.len() && s[e] == SP {
                            e += 1;
                        }
                        p.prefix_end = e;
                    } else if e == s.len() {
                        // §0.11: a box that is the whole content (`- [ ]`, `1. [x]`) is
                        // content for the caret test (§1.2) and for §1.3 (the item is not
                        // empty), but the line HAS a box for §1.3a and §1.4: the parser
                        // renders an empty task, and Enter continues the checklist
                        // (`enter-empty-task-no-space`).
                        p.box_at_end = true;
                    }
                }
            }
        }
    }
    p
}

/// The digit run (at most nine ASCII digits) as a decimal integer; leading zeros allowed.
fn decimal_of(digits: &[u16]) -> u32 {
    digits.iter().fold(0, |n, &c| n * 10 + u32::from(c - b'0' as u16))
}

// MARK: - §0.13 table grammar

fn split_cells(row: &[u16]) -> Vec<Vec<u16>> {
    let mut r = trim(row);
    if r.first() == Some(&PIPE) {
        r = &r[1..];
    }
    if r.last() == Some(&PIPE) {
        r = &r[..r.len() - 1];
    }
    let mut cells = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut escaped = false;
    for &ch in r {
        if escaped {
            if ch != PIPE {
                cur.push(BACKSLASH);
            }
            cur.push(ch);
            escaped = false;
        } else if ch == BACKSLASH {
            escaped = true;
        } else if ch == PIPE {
            cells.push(trim(&cur).to_vec());
            cur.clear();
        } else {
            cur.push(ch);
        }
    }
    if escaped {
        cur.push(BACKSLASH);
    }
    cells.push(trim(&cur).to_vec());
    cells
}

fn inner_marker_exists(inner: &[u16]) -> bool {
    parse_prefix(inner).has_marker
}

fn is_table_pair(header: &[u16], delimiter: &[u16]) -> bool {
    if !header.contains(&PIPE) {
        return false;
    }
    if inner_marker_exists(header) {
        return false;
    }
    if is_atx_heading(header) || is_footnote_definition(header) || is_comment_start(header) {
        return false;
    }
    if !trim(delimiter).contains(&DASH) {
        return false;
    }
    {
        // §0.13: a delimiter line with a `marker` is excluded only when its
        // content contains no `|`: `- ` and `- x` are list items to a writer,
        // while `- | -` and `- |` are delimiter rows (the pipe says table, and
        // the parser tests tables before lists) — §5 table-delimiter-list-marker
        let dp = parse_prefix(delimiter);
        if dp.has_marker && !delimiter[dp.prefix_end..].contains(&PIPE) {
            return false;
        }
    }
    let d_cells = split_cells(delimiter);
    for c in &d_cells {
        if c.is_empty() {
            return false;
        }
        let mut dash = false;
        for &u in c {
            if u == DASH {
                dash = true;
            } else if u != COLON {
                return false;
            }
        }
        if !dash {
            return false;
        }
    }
    split_cells(header).len() == d_cells.len()
}

// MARK: - §0.10 state scan

struct Scan {
    /// Per line ≤ c: front matter / fence line / comment line.
    code: Vec<bool>,
    /// Per line ≤ c: blank, code (as above) or ATX heading.
    special: Vec<bool>,
    /// The caret's line is code (incl. tentative front matter).
    caret_code: bool,
}

/// Fence opener test (§0.10): ≤ 3 leading SP, a run of `` ` `` or `~` of length ≥ 3, no backtick
/// in the rest of a backtick fence. The fence unit and the run length.
fn fence_opener(s: &[u16]) -> Option<(u16, usize)> {
    let i = leading_sp_count(s);
    if i > 3 {
        return None;
    }
    let c = u(s, i)?;
    if c != BACKTICK && c != TILDE {
        return None;
    }
    let mut j = i;
    while j < s.len() && s[j] == c {
        j += 1;
    }
    let n = j - i;
    if n < 3 {
        return None;
    }
    if c == BACKTICK && s[j..].contains(&BACKTICK) {
        return None;
    }
    Some((c, n))
}

fn fence_closes(s: &[u16], ch: u16, n: usize) -> bool {
    let i = leading_sp_count(s);
    let mut j = i;
    while j < s.len() && s[j] == ch {
        j += 1;
    }
    if j - i < n {
        return false;
    }
    is_blank(&s[j..])
}

fn is_front_matter_closer(t: &[u16], yaml: bool) -> bool {
    if yaml { eq_ascii(t, "---") || eq_ascii(t, "...") } else { eq_ascii(t, "+++") }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Normal,
    Fence,
    Comment,
}

fn scan_state(doc: &Doc, c: usize, for_enter: bool) -> Scan {
    let mut code = vec![false; c + 1];
    let mut special = vec![false; c + 1];
    let mut caret_code = false;

    // --- front matter (all the parser's guards)
    let mut fm_end: Option<usize> = None;
    let opener = trim(doc.l(0));
    let yaml = eq_ascii(opener, "---");
    let toml = eq_ascii(opener, "+++");
    let separator = if yaml {
        Some(COLON)
    } else if toml {
        Some(EQUALS)
    } else {
        None
    };
    if let Some(separator) = separator {
        let mut holds = doc.count() > 1 && !is_blank(doc.l(1));
        let mut close: Option<usize> = None;
        if holds {
            close = (1..doc.count()).find(|&k| is_front_matter_closer(trim(doc.l(k)), yaml));
            if close.is_none() {
                holds = false;
            }
        }
        if holds && let Some(close) = close {
            let mut field = false;
            for k in 1..close {
                let t = trim(doc.l(k));
                if t.is_empty() || t[0] == HASH || t[0] == DASH {
                    continue;
                }
                let Some(sep) = t.iter().position(|&x| x == separator) else { continue };
                if is_blank(&t[..sep]) {
                    continue;
                }
                field = true;
                break;
            }
            if !field {
                holds = false;
            }
        }
        if holds {
            fm_end = close;
        } else if c >= 1 {
            // tentative front matter (§0.10): a block that is already closed above
            // the caret is complete and rejected — prose. Otherwise the caret's
            // line is code iff every line of the tested range is non-blank and
            // field-like. The range is 1 ..< c for capitalize (the caret's line
            // is being typed) and 1 … c for enter (the line is complete).
            let closed = (1..c).any(|k| is_front_matter_closer(trim(doc.l(k)), yaml));
            if !closed {
                let last = if for_enter { c } else { c - 1 };
                let mut tentative = true;
                let mut field_seen = false;
                for k in 1..=last {
                    let t = trim(doc.l(k));
                    if t.is_empty() {
                        tentative = false;
                        break;
                    }
                    let sep = t.iter().position(|&x| x == separator);
                    let hash = t[0] == HASH;
                    let dash = t[0] == DASH;
                    // a field mirrors parseFrontMatter's guard: a `#` or `-` line is never
                    // a field (`---⏎- key: v⏎---` renders as rule + list + rule)
                    let is_field = !hash && !dash && sep.is_some_and(|sep| sep > 0 && !is_blank(&t[..sep]));
                    // a `#` comment (YAML and TOML) or a `-` list item (YAML only: TOML has
                    // no `-` lists) counts only under a field line: a list or heading
                    // directly under a bare `---` is Markdown
                    let field_like = is_field || ((hash || (dash && separator == COLON)) && field_seen);
                    if !field_like {
                        tentative = false;
                        break;
                    }
                    if is_field {
                        field_seen = true;
                    }
                }
                if tentative {
                    caret_code = true;
                }
            }
        }
    }
    if let Some(fm_end) = fm_end {
        let last = fm_end.min(c);
        code[..=last].fill(true);
        special[..=last].fill(true);
        if fm_end >= c {
            caret_code = true;
        }
    }

    // --- fences and comments, at quote depth 0 only
    let mut state = ScanMode::Normal;
    let mut fence_char = 0;
    let mut fence_run = 0;
    for k in fm_end.map_or(0, |f| f + 1)..=c {
        let s = doc.l(k);
        match state {
            ScanMode::Normal => {
                if let Some((ch, n)) = fence_opener(s) {
                    state = ScanMode::Fence;
                    fence_char = ch;
                    fence_run = n;
                    code[k] = true;
                } else if is_comment_start(s) {
                    code[k] = true;
                    state = if find_ascii(s, "-->").is_some() { ScanMode::Normal } else { ScanMode::Comment };
                }
            }
            ScanMode::Fence => {
                code[k] = true;
                if fence_closes(s, fence_char, fence_run) {
                    state = ScanMode::Normal;
                }
            }
            ScanMode::Comment => {
                code[k] = true;
                if find_ascii(s, "-->").is_some() {
                    state = ScanMode::Normal;
                }
            }
        }
        if code[k] || is_blank(s) || is_atx_heading(s) {
            special[k] = true;
        }
    }
    if code[c] {
        caret_code = true;
    }
    Scan { code, special, caret_code }
}

// MARK: - §0.13 tableContext

fn depth_of(doc: &Doc, i: usize) -> usize {
    parse_prefix(doc.l(i)).depth
}

fn inner<'a>(doc: &Doc<'a>, i: usize) -> &'a [u16] {
    let s = doc.l(i);
    &s[parse_prefix(s).inner_start..]
}

/// Whether line `k` is a table row (§0.13), and if so the header's index and the header's cell
/// count. Memoised per call: the ancestor walk and the inline scan ask this for every line of the
/// caret's run.
fn table_context(doc: &mut Doc, scan: &Scan, k: usize) -> Option<(usize, usize)> {
    if let Some(&known) = doc.table.get(&k) {
        return known;
    }
    let mut found = None;
    let d = depth_of(doc, k);
    let mut top = k;
    while top >= 1 && !is_blank(doc.l(top - 1)) && depth_of(doc, top - 1) == d && !scan.code[top - 1] {
        top -= 1;
    }
    let mut h = top;
    while h <= k {
        if h + 1 >= doc.count() || depth_of(doc, h + 1) != d || !is_table_pair(inner(doc, h), inner(doc, h + 1)) {
            h += 1;
            continue;
        }
        // the pair at h opens a table; it runs while the lines below contain a
        // pipe. A line without one ends it (the parser's row loop stops there)
        // and is not a header itself, so the search resumes below that line —
        // greedy consumption, never a second pair inside the first table's rows.
        match (h + 2..=k).find(|&j| !doc.l(j).contains(&PIPE)) {
            None => {
                found = Some((h, split_cells(inner(doc, h)).len()));
                break;
            }
            Some(ended) => h = ended + 1,
        }
    }
    doc.table.insert(k, found);
    found
}

fn is_pipe_line(s: &[u16]) -> bool {
    // §0.13: the inner text (line minus indent0 and quotes), after indent1, starts with `|`
    let p = parse_prefix(s);
    u(s, p.row_prefix_len()) == Some(PIPE)
}

// MARK: - §0.9 inputs, outputs and offsets

/// §0.9: order the selection, reject an offset outside the text, strictly between the CR and the
/// LF of one terminator, or between the halves of a surrogate pair; produce `text′` (the text
/// with the selection removed — the original units themselves when the selection is empty, so
/// nothing is copied on the common path) with the caret at `start`.
fn normalize(text: &[u16], selection_start: isize, selection_end: isize) -> Option<(usize, usize, Cow<'_, [u16]>)> {
    let start = usize::try_from(selection_start.min(selection_end)).ok()?;
    let end = usize::try_from(selection_start.max(selection_end)).ok()?;
    if end > text.len() {
        return None;
    }
    if between_crlf(text, start) || between_crlf(text, end) {
        return None;
    }
    // §0.9: an offset strictly between the high and the low half of a surrogate
    // pair would split one scalar in two (a corrupt document, and one Swift's
    // NSString bridge repairs differently from Kotlin and C#)
    if between_surrogates(text, start) || between_surrogates(text, end) {
        return None;
    }
    let t = if start != end { Cow::Owned([&text[..start], &text[end..]].concat()) } else { Cow::Borrowed(text) };
    Some((start, end, t))
}

fn between_crlf(text: &[u16], o: usize) -> bool {
    o > 0 && o < text.len() && text[o - 1] == CR && text[o] == LF
}

fn between_surrogates(text: &[u16], o: usize) -> bool {
    o > 0 && o < text.len() && is_high(text[o - 1]) && is_low(text[o])
}

// MARK: - §1 enter

/// §1.4 `next(n)`: `n + 1`, unless that would need more than nine digits.
fn next_number(n: u32) -> u32 {
    if n + 1 > 999_999_999 { n } else { n + 1 }
}

fn marker_prime(m: &Prefix, number: u32) -> Vec<u16> {
    if m.bullet != 0 {
        vec![m.bullet, SP]
    } else {
        let mut out: Vec<u16> = units(&number.to_string()).collect();
        out.push(m.delimiter);
        out.push(SP);
        out
    }
}

fn enter_edit(location: usize, length: usize, replacement: &[u16], caret: usize) -> EnterEdit {
    EnterEdit { location, length, replacement: String::from_utf16_lossy(replacement), caret }
}

/// What Return does (§1): `None` for the platform's own newline, or one [`EnterEdit`] — the next
/// list item or quote line, an exited empty item, an outdented one, or a new table row.
/// `selection_start` and `selection_end` are UTF-16 offsets into `text`, in either order; one
/// outside the text, inside a CR LF pair or inside a surrogate pair gives `None` (§0.9).
pub fn enter(text: &str, selection_start: isize, selection_end: isize) -> Option<EnterEdit> {
    enter_utf16(&Utf16Text::new(text), selection_start, selection_end)
}

/// [`enter`] on text already in UTF-16 code units.
pub fn enter_utf16(text: &Utf16Text, selection_start: isize, selection_end: isize) -> Option<EnterEdit> {
    let text = text.units();
    let (start, end, t) = normalize(text, selection_start, selection_end)?;
    let caret = start;
    let mut doc = Doc::new(&t);
    let c = line_of(&doc.lines, caret)?; // §0.2
    let scan = scan_state(&doc, c, true);
    if scan.caret_code {
        return None; // §1.0
    }

    let removed = end - start;
    let line = doc.l(c);
    let line_start = doc.lines[c].start;
    let line_end = doc.lines[c].end;

    // §1.1 table row: the selection must lie within one line of `text`
    let within_line = !text[start..end].iter().any(|&u| u == CR || u == LF);
    if within_line && let Some((header, n)) = table_context(&mut doc, &scan, c) {
        let p = parse_prefix(line);
        let row_prefix = &line[..p.row_prefix_len()];
        let mut cells: Vec<u16> = vec![PIPE];
        for _ in 0..n {
            cells.extend(units("  |"));
        }
        let row = [&[LF][..], row_prefix, &cells].concat();
        // insert the row after the line end `x_prime` of text′ (at or after caret′);
        // with a selection the same edit is one range from `start` that also
        // deletes the selection (§0.9)
        let insert_after = |x_prime: usize| {
            let x = x_prime + removed;
            if removed == 0 {
                return enter_edit(x, 0, &row, x + 1 + row_prefix.len() + 2);
            }
            let tail = &text[end..x];
            enter_edit(start, x - start, &[tail, &row].concat(), start + tail.len() + 1 + row_prefix.len() + 2)
        };
        if c == header {
            if caret == line_start {
                return None; // column 0 of the header: a plain newline pushes the table down
            }
            return Some(insert_after(doc.lines[header + 1].end));
        }
        // §1.1: the table continues below the caret's row iff line c+1 exists,
        // has the same quote depth and contains a `|` unit (the parser's row
        // loop). A blank row ends the table only when it is the LAST row: a
        // blank line in the middle would orphan the rows below it
        // (`enter-table-blank-row-mid-table`).
        let continues_below =
            c + 1 < doc.count() && parse_prefix(doc.l(c + 1)).depth == p.depth && doc.l(c + 1).contains(&PIPE);
        if removed == 0 && !continues_below && split_cells(&line[p.inner_start..]).iter().all(Vec::is_empty) {
            // end the table; inside a quote the writer stays in the quote (as §1.3b)
            let r = if p.depth > 0 { &line[..p.indent0_quotes_len()] } else { &[][..] };
            return Some(enter_edit(line_start, line_end - line_start, r, line_start + r.len()));
        }
        if removed == 0 && c >= header + 2 && caret == line_start {
            // column 0 of a body row: the new row goes ABOVE the caret's row, which
            // moves down intact exactly as a plain newline would push it — the
            // writer's only "insert a row above" gesture (A.1). The delimiter row
            // keeps the insert-after branch: a row between header and delimiter
            // would break the table.
            let above = [row_prefix, &cells, &[LF][..]].concat();
            return Some(enter_edit(line_start, 0, &above, line_start + row_prefix.len() + 2));
        }
        return Some(insert_after(line_end));
    }

    let pc = parse_prefix(line);
    let rel = caret - line_start;
    if rel < pc.prefix_end {
        return None; // §1.2
    }
    let content = &line[pc.prefix_end..];

    // replaces text′[line_start, caret′) with r
    let replace_head = |r: &[u16]| enter_edit(line_start, (caret - line_start) + removed, r, line_start + r.len());

    // §1.3 empty item
    if is_blank(content) && (pc.has_marker || pc.depth > 0) {
        if pc.has_marker {
            // a. outdent
            if let Some(anc) = ancestor(&mut doc, &scan, c, &pc) {
                let ancestor_line = doc.l(anc);
                let a = parse_prefix(ancestor_line);
                let mut r = ancestor_line[..a.row_prefix_len()].to_vec();
                r.extend(marker_prime(&a, next_number(a.number)));
                if a.has_box || a.box_at_end {
                    r.extend(units("[ ] "));
                }
                return Some(replace_head(&r));
            }
            // b. terminate a list item
            return Some(replace_head(if pc.depth > 0 { &line[..pc.indent0_quotes_len()] } else { &[] }));
        }
        // c. terminate a quote level (depth > 0 here, so a last group exists)
        let last = pc.last_group_start.unwrap_or(0);
        return Some(replace_head(if last == 0 { &[] } else { &line[..pc.indent0 + last] }));
    }

    // §1.4 continue
    if pc.has_marker || pc.depth > 0 {
        let mut r = vec![LF];
        r.extend_from_slice(&line[..pc.row_prefix_len()]);
        if pc.has_marker {
            r.extend(marker_prime(&pc, next_number(pc.number)));
        }
        if pc.has_box || pc.box_at_end {
            r.extend(units("[ ] "));
        }
        return Some(enter_edit(start, removed, &r, start + r.len()));
    }

    None // §1.5
}

/// §1.3a ancestor walk: the index of the ancestor line, or `None`.
fn ancestor(doc: &mut Doc, scan: &Scan, c: usize, p: &Prefix) -> Option<usize> {
    for k in (0..c).rev() {
        let s = doc.l(k);
        if is_blank(s) {
            return None;
        }
        let pk = parse_prefix(s);
        if pk.depth != p.depth {
            return None;
        }
        if scan.code[k] {
            return None;
        }
        if table_context(doc, scan, k).is_some() {
            return None;
        }
        let inner = &s[pk.inner_start..];
        if is_blank(inner) {
            return None; // `> ` between quoted items: the renderer sees a blank line
        }
        if is_thematic_break(inner) || is_atx_heading(inner) || is_page_break(inner) {
            return None;
        }
        // §1.3a: the ancestor's list LEVEL (floor(columns / 2), the parser's
        // `indent / 2`) is strictly smaller — a 1-SP or 3-SP item renders as a
        // sibling, so Enter on it must not manufacture another sibling
        if pk.has_marker && pk.list_level < p.list_level {
            return Some(k);
        }
    }
    None
}

// MARK: - §2 capitalize

/// §2.2 openers: `* _ ~ " ' ( [ { « » ‹ › “ ” ‘ ’ „ ‚ ¿ ¡` (and `!` only as the `![` pair, tested
/// by the callers).
fn is_opener(u: u16) -> bool {
    matches!(
        u,
        0x2A | 0x5F
            | 0x7E
            | 0x22
            | 0x27
            | 0x28
            | 0x5B
            | 0x7B
            | 0x00AB
            | 0x00BB
            | 0x2039
            | 0x203A
            | 0x201C
            | 0x201D
            | 0x2018
            | 0x2019
            | 0x201E
            | 0x201A
            | 0x00BF
            | 0x00A1
    )
}

/// §2.2 closers: `) ] } " ' » « ‹ › ” ’ “ ‘ * _ ~`.
fn is_closer(u: u16) -> bool {
    matches!(
        u,
        0x29 | 0x5D
            | 0x7D
            | 0x22
            | 0x27
            | 0x00BB
            | 0x00AB
            | 0x2039
            | 0x203A
            | 0x201D
            | 0x2019
            | 0x201C
            | 0x2018
            | 0x2A
            | 0x5F
            | 0x7E
    )
}

/// §2.2: the quotation marks among the openers (the colon rule, §2.5 1a).
fn is_quote_opener(u: u16) -> bool {
    matches!(u, 0x22 | 0x27 | 0x00AB | 0x00BB | 0x2039 | 0x203A | 0x201C | 0x201D | 0x2018 | 0x2019 | 0x201E | 0x201A)
}

/// §2.2: guillemets, which carry French spacing after them when opening.
fn is_guillemet(u: u16) -> bool {
    matches!(u, 0x00AB | 0x2039 | 0x00BB | 0x203A)
}

fn is_terminator(u: u16) -> bool {
    u == DOT || u == BANG || u == b'?' as u16
}

/// §2.3 dash units: em dash, en dash, minus, horizontal bar (Unicode's "quotation dash"), figure
/// dash, the pasted bullets • ‣ · and the section / pilcrow signs § ¶ (Po since Unicode 6.1,
/// leads to a writer).
fn is_dash_unit(u: u16) -> bool {
    matches!(u, 0x2014 | 0x2013 | 0x2212 | 0x2015 | 0x2012 | 0x2022 | 0x2023 | 0xB7 | 0xA7 | 0xB6)
}

/// §2.3: arrows (U+2190–U+21FF) are leads at the content start only; they are not in the
/// mid-line group.
fn is_arrow_unit(u: u16) -> bool {
    (0x2190..=0x21FF).contains(&u)
}

/// §2.3 `dash` lead run: dash units, arrows or U+002D, but never a single hyphen-minus (a list
/// marker).
fn is_lead_dash_unit(u: u16) -> bool {
    is_dash_unit(u) || is_arrow_unit(u) || u == DASH
}

/// §2.5 step 1b: the mid-line dash group — dash units or the hyphen-minus, mixed freely; arrows
/// are not in it.
fn is_mid_line_dash_unit(u: u16) -> bool {
    is_dash_unit(u) || u == DASH
}

fn is_section_unit(u: u16) -> bool {
    u == 0xA7 || u == 0xB6
}

fn is_roman_unit(u: u16) -> bool {
    matches!(u, 0x69 | 0x76 | 0x78 | 0x49 | 0x56 | 0x58) // i v x I V X
}

/// §2.3 `number` body: `Digit{1,9} ( "." Digit{1,9} )* ( "." | ")" )?` — the end index after
/// it, or `None`.
fn number_body_end(s: &[u16], i: usize) -> Option<usize> {
    let digits = |from: usize| {
        let mut j = from;
        while j < s.len() && is_digit(s[j]) && j - from < 9 {
            j += 1;
        }
        j
    };
    let mut j = digits(i);
    if j == i {
        return None;
    }
    while u(s, j) == Some(DOT) && j + 1 < s.len() && is_digit(s[j + 1]) {
        j = digits(j + 1);
    }
    if u(s, j) == Some(DOT) || u(s, j) == Some(RPAREN) {
        j += 1;
    }
    Some(j)
}

/// §2.3 `enum` body: `"(" ( Digit{1,3} | Letter | roman{1,4} ) ")"` or the same without the
/// `"("` — the end index after the `)`, or `None`.
fn enum_body_end(s: &[u16], i: usize) -> Option<usize> {
    let mut j = i;
    let paren = u(s, j) == Some(LPAREN);
    if paren {
        j += 1;
    }
    let b = j;
    let mut e: Option<usize> = None;
    {
        let mut d = b;
        while d < s.len() && is_digit(s[d]) && d - b < 3 {
            d += 1;
        }
        if d > b && !u(s, d).is_some_and(is_digit) {
            e = Some(d);
        }
    }
    if e.is_none() {
        let mut r = b;
        while r < s.len() && is_roman_unit(s[r]) && r - b < 4 {
            r += 1;
        }
        if r > b && u(s, r) == Some(RPAREN) {
            e = Some(r);
        }
    }
    if e.is_none() && b < s.len() {
        let (cp, len) = scalar_at(s, b);
        if is_letter(cp) {
            e = Some(b + len);
        }
    }
    match e {
        Some(e) if u(s, e) == Some(RPAREN) => Some(e + 1),
        _ => None,
    }
}

/// §2.3 `citation` lead body: `"[" Digit{1,4} "]"` — the end index after the `]`, or `None`.
fn citation_body_end(s: &[u16], i: usize) -> Option<usize> {
    if u(s, i) != Some(LBRACKET) {
        return None;
    }
    let mut d = i + 1;
    while d < s.len() && is_digit(s[d]) && d - (i + 1) < 4 {
        d += 1;
    }
    if d == i + 1 || u(s, d) != Some(RBRACKET) {
        return None;
    }
    Some(d + 1)
}

/// §2.5 step 1b: start of the `symbols` run (§2.3, keycaps included) that ends at unit `end` of
/// `pre`, or `end` when there is none.
fn symbols_run_start(pre: &[u16], end: usize) -> usize {
    let mut j = end;
    while let Some((cp, len)) = scalar_before(pre, j) {
        if cp == 0x20E3 {
            // keycap: ( Digit | "#" | "*" ) U+FE0F? U+20E3
            let mut k = j - 1;
            if k > 0 && pre[k - 1] == 0xFE0F {
                k -= 1;
            }
            let base = if k > 0 { Some(pre[k - 1]) } else { None };
            if base.is_some_and(|b| is_digit(b) || b == HASH || b == STAR) {
                j = k - 1;
                continue;
            }
            j -= 1;
            continue;
        }
        if is_symbol_so_sk(cp) || cp == 0xFE0E || cp == 0xFE0F || cp == 0x200D || (0x1F3FB..=0x1F3FF).contains(&cp) {
            j -= len;
            continue;
        }
        break;
    }
    j
}

/// §2.5 step 2: a footnote reference `[^id]` or a bracketed digit run `[12]` whose `]` sits at
/// `end - 1` of `pre` — its start, or `None`.
fn reference_start(pre: &[u16], end: usize) -> Option<usize> {
    if end < 3 || pre[end - 1] != RBRACKET {
        return None;
    }
    let mut j = end - 1;
    while j > 0 && is_footnote_id_unit(pre[j - 1]) {
        j -= 1;
    }
    if j == end - 1 {
        return None;
    }
    if j >= 2 && pre[j - 1] == CARET && pre[j - 2] == LBRACKET {
        return Some(j - 2);
    }
    let all_digits = pre[j..end - 1].iter().all(|&x| is_digit(x));
    if all_digits && j >= 1 && pre[j - 1] == LBRACKET {
        return Some(j - 1);
    }
    None
}

fn strip_openers(tok: &[u16]) -> &[u16] {
    let mut o = 0;
    while o < tok.len() {
        if is_opener(tok[o]) {
            o += 1;
            continue;
        }
        if tok[o] == BANG && u(tok, o + 1) == Some(LBRACKET) {
            o += 1;
            continue;
        }
        break;
    }
    &tok[o..]
}

/// §2.5 B(ii): one Letter scalar followed by zero or more Marks.
fn is_single_letter(cps: &[u32]) -> bool {
    match cps.split_first() {
        Some((&first, rest)) => is_letter(first) && rest.iter().all(|&cp| is_mark(cp)),
        None => false,
    }
}

/// §2.5 B(ii′): a compact initials chain — two or more groups of one Uppercase letter (+ Marks)
/// separated by single `.` units (`И.И`, `J.R.R`, `U.S.A`; the final `.` is the terminator run).
fn is_initials_chain(cps: &[u32]) -> bool {
    let mut groups = 0;
    let mut i = 0;
    while i < cps.len() {
        if !is_uppercase(cps[i]) {
            return false;
        }
        i += 1;
        while i < cps.len() && is_mark(cps[i]) {
            i += 1;
        }
        groups += 1;
        if i == cps.len() {
            break;
        }
        if cps[i] != u32::from(DOT) {
            return false;
        }
        i += 1;
        if i == cps.len() {
            return false;
        }
    }
    groups >= 2
}

/// §2.5 B(ii): the token before `tok_start` (over ≥ 1 WS19) is itself a single letter plus a
/// terminator run — an initials chain (`J. R.`) or a spaced abbreviation (`z. B.`).
fn prev_token_is_initial(pre: &[u16], tok_start: usize) -> bool {
    let mut i = tok_start;
    while i > 0 && is_ws19(pre[i - 1]) {
        i -= 1;
    }
    if i == tok_start {
        return false;
    }
    let e = i;
    while i > 0 && !is_ws19(pre[i - 1]) {
        i -= 1;
    }
    let t = strip_openers(&pre[i..e]);
    let mut k = t.len();
    while k > 0 && is_terminator(t[k - 1]) {
        k -= 1;
    }
    if k == t.len() {
        return false;
    }
    is_single_letter(&scalars_of(&t[..k]))
}

/// §2.6 abbreviation list, stored exactly as listed (final dot removed, inner dots kept);
/// compared ordinally after [`fold`]. Laid out by language, as md.win lists them.
#[rustfmt::skip]
const ABBREVIATIONS: &[&str] = &[
    // English
    "a.d", "a.m", "al", "approx", "apr", "assn", "aug", "ave", "b.c", "blvd", "ca", "cf", "ch", "co", "corp", "dec",
    "dept", "dr", "e.g", "e.u", "ed", "eds", "eq", "eqs", "esp", "etc", "excl", "ext", "feb", "ff", "fig", "figs",
    "fri", "govt", "i.e", "ibid", "inc", "incl", "jan", "jr", "jul", "jun", "ltd", "misc", "mr", "mrs", "ms", "mt",
    "nov", "oct", "p.m", "ph.d", "pp", "prof", "rd", "resp", "sep", "sept", "sr", "st", "tel", "thu", "tue", "u.k",
    "u.s", "univ", "viz", "vol", "vs",
    // German
    "abs", "bspw", "bzgl", "bzw", "d.h", "evtl", "exkl", "geb", "ggf", "hrsg", "inkl", "jh", "mio", "mrd", "nr",
    "o.ä", "o.g", "s.o", "s.u", "sog", "std", "str", "tsd", "u.a", "u.u", "usw", "vgl", "z.b", "z.t", "zzgl",
    // French
    "art", "av", "chap", "éd", "env", "ex", "mlle", "mme", "p.ex", "réf", "ste", "tél", "trad",
    // Spanish
    "aprox", "avda", "cap", "dña", "dpto", "ej", "núm", "p.ej", "pág", "págs", "sra", "srta", "ud", "uds",
    // Russian
    "акад", "англ", "г", "гг", "гл", "гос", "греч", "др", "зам", "изд", "им", "исп", "ит", "кв", "коп", "корп", "лат",
    "млн", "млрд", "напр", "нем", "н.э", "обл", "пер", "перев", "пл", "пп", "пр", "прим", "просп", "проф", "ред",
    "руб", "рус", "св", "см", "сокр", "сост", "ст", "стр", "табл", "тел", "т.д", "т.е", "т.к", "т.н", "т.о", "т.п",
    "т.ч", "тыс", "укр", "ул", "фр", "чел", "чл", "шт", "экз",
    // Ukrainian
    "буд", "вул", "грн", "див", "ін", "рр", "стор", "тис", "т.зв",
];

/// [`ABBREVIATIONS`] as UTF-16, built once.
fn abbreviations() -> &'static HashSet<Vec<u16>> {
    static SET: OnceLock<HashSet<Vec<u16>>> = OnceLock::new();
    SET.get_or_init(|| ABBREVIATIONS.iter().map(|a| a.encode_utf16().collect()).collect())
}

/// §2.6 fold: ASCII A–Z, Latin-1 À–Þ (not ×), Cyrillic А–Я, Ё Є І Ї Ґ only; every other unit is
/// left as is.
fn fold(s: &[u16]) -> Vec<u16> {
    s.iter()
        .map(|&u| match u {
            0x41..=0x5A => u + 0x20,
            0xC0..=0xDE if u != 0xD7 => u + 0x20,
            0x410..=0x42F => u + 0x20,
            0x401 => 0x451,
            0x404 => 0x454,
            0x406 => 0x456,
            0x407 => 0x457,
            0x490 => 0x491,
            _ => u,
        })
        .collect()
}

/// The index after the WS19 run starting at `j`, or `None` when there is none.
fn ws_after(s: &[u16], j: usize) -> Option<usize> {
    let mut w = j;
    while w < s.len() && is_ws19(s[w]) {
        w += 1;
    }
    (w > j).then_some(w)
}

/// §2.3 `prefix2` length on line `s` (measured on `before`, the line up to the caret).
fn prefix2_length(s: &[u16]) -> usize {
    let p = parse_prefix(s);
    let mut i = p.prefix_end;
    let mut heading = false;
    let mut footnote = false;
    if !p.has_marker {
        let base = p.row_prefix_len();
        // headingPrefix := indent0 quotes indent1 "#"{1,6} SP  (+ the WS19 run after it)
        let mut h = base;
        let mut n = 0;
        while h < s.len() && s[h] == HASH {
            n += 1;
            h += 1;
        }
        // the parser's parseHeading drops SP only: `\t# h` is a paragraph (§0.12)
        let tab_free = !s[..p.indent0].contains(&TAB)
            && !s[p.indent0_quotes_len()..p.indent0_quotes_len() + p.indent1].contains(&TAB);
        if tab_free && (1..=6).contains(&n) && u(s, h) == Some(SP) {
            h += 1;
            while h < s.len() && is_ws19(s[h]) {
                h += 1;
            }
            i = h;
            heading = true;
        } else if let Some(f) = footnote_prefix_length(s, base) {
            // footnotePrefix := indent0 quotes indent1 "[^" id "]:" WS19*
            i = base + f;
            footnote = true;
        }
    }
    if !heading && !footnote {
        // prefix WS19*: every §0.11 prefix — a marker, a box, a quote group or
        // nothing at all — absorbs the WS19 run after it, as headingPrefix does:
        // the parser drops SP only, but `- \tt`, `> t` and `​t` all
        // render `t` as the first visible unit of the line
        while i < s.len() && is_ws19(s[i]) {
            i += 1;
        }
    }
    // lead := ( section | enum | citation | number | dash | symbols ) WS19+
    //   enum and citation only as the first lead (directly after prefix,
    //   headingPrefix or footnotePrefix);
    //   number only as the first lead after headingPrefix (A.2: `2.5 cups`)
    let lead_after = |body: Option<usize>| body.and_then(|e| ws_after(s, e));
    let mut first = true;
    loop {
        let mut j = i;
        let unit = u(s, j);
        let mut w: Option<usize> = None;
        if unit.is_some_and(is_section_unit) {
            // section := run of ( "§" | "¶" ), WS19*, number
            let mut k = j;
            while k < s.len() && is_section_unit(s[k]) {
                k += 1;
            }
            let mut m = k;
            while m < s.len() && is_ws19(s[m]) {
                m += 1;
            }
            w = lead_after(number_body_end(s, m));
            if w.is_none() {
                w = ws_after(s, k); // a bare `§ ` is a dash-group lead
            }
        } else if first && let Some(x) = lead_after(enum_body_end(s, j)) {
            w = Some(x);
        } else if first && let Some(x) = lead_after(citation_body_end(s, j)) {
            w = Some(x);
        } else if first
            && heading
            && let Some(x) = lead_after(number_body_end(s, j))
        {
            w = Some(x);
        }
        if let Some(x) = w {
            i = x;
            first = false;
            continue;
        }
        if let Some(first_unit) = unit
            && is_lead_dash_unit(first_unit)
        {
            while j < s.len() && is_lead_dash_unit(s[j]) {
                j += 1;
            }
            if j == i + 1 && first_unit == DASH {
                break; // a single hyphen-minus is never a lead
            }
        } else {
            while j < s.len() {
                let (cp, len) = scalar_at(s, j);
                // a keycap sequence (1️⃣ #️⃣ *️⃣): Digit, `#` or `*`, optional U+FE0F, U+20E3
                if is_digit(cp) || cp == u32::from(HASH) || cp == u32::from(STAR) {
                    let mut k = j + 1;
                    if u(s, k) == Some(0xFE0F) {
                        k += 1;
                    }
                    if u(s, k) == Some(0x20E3) {
                        j = k + 1;
                        continue;
                    }
                    break;
                }
                let sym = is_symbol_so_sk(cp)
                    || cp == 0xFE0E
                    || cp == 0xFE0F
                    || cp == 0x200D
                    || cp == 0x20E3
                    || (0x1F3FB..=0x1F3FF).contains(&cp);
                if !sym {
                    break;
                }
                j += len;
            }
            if j == i {
                break;
            }
        }
        match ws_after(s, j) {
            None => break,
            Some(x) => {
                i = x;
                first = false;
            }
        }
    }
    i
}

/// The unit at `idx`, `typed`'s first unit just past the caret, or `None` past the line end.
fn next_at(line: &[u16], idx: usize, typed_unit: Option<u16>) -> Option<u16> {
    if idx < line.len() {
        Some(line[idx])
    } else if idx == line.len() {
        typed_unit
    } else {
        None
    }
}

fn run_of(line: &[u16], idx: usize, ch: u16) -> usize {
    line[idx..].iter().take_while(|&&x| x == ch).count()
}

/// §2.1 inline scan over `P` = lines `s+1 … c`. True when the caret is inside a code span,
/// display math or inline math.
fn inside_inline(doc: &mut Doc, scan: &Scan, c: usize, before: &[u16], typed: &[u16]) -> bool {
    // s = the last special line above c (excluded from P); a thematic break
    // and a page break end a paragraph the same way (§2.1)
    // a table row (§0.13) ends a paragraph too: the parser's row loop consumed
    // it, so an unclosed `$$` in a cell does not leak into the prose below
    let mut s: Option<usize> = None;
    for k in (0..c).rev() {
        if scan.special[k]
            || is_thematic_break(doc.l(k))
            || is_page_break(doc.l(k))
            || table_context(doc, scan, k).is_some()
        {
            s = Some(k);
            break;
        }
    }
    // a quote line or a marker line starts a new block: P begins there (included)
    let after_s = s.map_or(0, |s| s + 1);
    let mut first = after_s;
    for k in (after_s..=c).rev() {
        if is_quote_line(doc.l(k)) || parse_prefix(doc.l(k)).has_marker {
            first = k;
            break;
        }
    }
    let mut display = false;
    for k in first..=c {
        let is_caret_line = k == c;
        let line = if is_caret_line { before } else { doc.l(k) };
        let mut code = 0;
        let mut inline = false;
        let mut i = 0;
        // the unit at idx, `typed`'s first unit just past the caret, or None past the line end
        let typed_unit = if is_caret_line { typed.first().copied() } else { None };
        while i < line.len() {
            let unit = line[i];
            if code > 0 {
                // 1.
                if unit == BACKTICK {
                    let n = run_of(line, i, BACKTICK);
                    if n == code {
                        code = 0;
                    }
                    i += n;
                    continue;
                }
                i += 1;
                continue;
            }
            if display {
                // 2.
                if (unit == DOLLAR && next_at(line, i + 1, typed_unit) == Some(DOLLAR))
                    || (unit == BACKSLASH && next_at(line, i + 1, typed_unit) == Some(RBRACKET))
                {
                    display = false;
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            if inline {
                // 3.
                if unit == DOLLAR {
                    inline = false;
                    i += 1;
                    continue;
                }
                if unit == BACKSLASH && next_at(line, i + 1, typed_unit) == Some(RPAREN) {
                    inline = false;
                    i += 2;
                    continue;
                }
                if unit == BACKSLASH {
                    i += 2;
                    continue;
                }
                i += 1;
                continue;
            }
            // 4. normal
            if unit == BACKTICK {
                code = run_of(line, i, BACKTICK);
                i += code;
                continue;
            }
            if unit == BACKSLASH {
                let n = next_at(line, i + 1, typed_unit);
                if n == Some(LBRACKET) {
                    display = true;
                } else if n == Some(LPAREN) {
                    inline = true;
                }
                i += 2;
                continue;
            }
            if unit == DOLLAR {
                if next_at(line, i + 1, typed_unit) == Some(DOLLAR) {
                    display = true;
                    i += 2;
                    continue;
                }
                let prev_ok = match scalar_before(line, i) {
                    None => true,
                    Some((prev, _)) => {
                        !(is_letter(prev)
                            || is_digit(prev)
                            || prev == u32::from(UNDERSCORE)
                            || prev == u32::from(DOLLAR))
                    }
                };
                let next_ok = next_at(line, i + 1, typed_unit).is_some_and(|n| !is_ws19(n) && !is_digit(n));
                if prev_ok && next_ok {
                    inline = true;
                }
                i += 1;
                continue;
            }
            i += 1;
        }
        // the caret line: state at the caret decides
        if is_caret_line {
            return code > 0 || display || inline;
        }
    }
    false
}

/// The typed text's one scalar and its `upper` (§2.1a), or `None`.
fn typed_upper(typed: &[u16]) -> Option<u32> {
    let tcp = scalars_of(typed);
    if tcp.len() != 1 {
        return None;
    }
    if !is_lowercase(tcp[0]) {
        return None;
    }
    upper(tcp[0])
}

/// What the one lowercase letter being typed becomes (§2): its uppercase when it is the first
/// letter of a line or of a sentence within the line, Markdown-aware, or `None` to insert
/// `typed` unchanged. `typed` holds exactly one scalar; anything else is `None`. The selection is
/// UTF-16 offsets into `text`, in either order; the result replaces it.
pub fn capitalize(text: &str, selection_start: isize, selection_end: isize, typed: &str) -> Option<String> {
    // Asked on every keystroke: a typed text that cannot be capitalized is answered before the
    // document is converted (the same `None` §2.1a gives after the offsets are checked).
    let mut scalars = typed.chars();
    match (scalars.next(), scalars.next()) {
        (Some(c), None) if is_lowercase(u32::from(c)) && upper(u32::from(c)).is_some() => {}
        _ => return None,
    }
    capitalize_utf16(&Utf16Text::new(text), selection_start, selection_end, &Utf16Text::new(typed))
}

/// [`capitalize`] on text already in UTF-16 code units. A lone surrogate — in `text` or as
/// `typed` — is a scalar of category `Cs`: never a letter, never whitespace.
pub fn capitalize_utf16(
    text: &Utf16Text,
    selection_start: isize,
    selection_end: isize,
    typed: &Utf16Text,
) -> Option<String> {
    let text = text.units();
    let typed = typed.units();
    let (start, _, t) = normalize(text, selection_start, selection_end)?;
    let caret = start;
    // §2.1a typed is one Lowercase letter with a defined upper()
    let up = typed_upper(typed)?;
    let upper_typed = char::from_u32(up)?.to_string();

    let mut doc = Doc::new(&t);
    let c = line_of(&doc.lines, caret)?; // §0.2
    let scan = scan_state(&doc, c, false);
    if scan.caret_code {
        return None; // §2.1b
    }
    let line = doc.l(c);
    if table_context(&mut doc, &scan, c).is_some() || is_pipe_line(line) {
        return None; // §2.1c
    }
    let rel = caret - doc.lines[c].start;
    let before = &line[..rel];
    if inside_inline(&mut doc, &scan, c, before, typed) {
        return None; // §2.1d
    }

    // §2.1e link destination
    if let Some(lp) = rfind_ascii(before, "](")
        && !before[lp + 2..].contains(&RPAREN)
    {
        return None;
    }

    // §2.1f current token. prefix2 is measured on `before` (§2.3): a caret
    // inside the SP run a marker absorbs is still at the content start.
    let p2 = prefix2_length(before);
    let last_ws = before.iter().rposition(|&x| is_ws19(x));
    let token_start = last_ws.map_or(0, |w| w + 1).max(p2.min(before.len()));
    let token = &before[token_start..];
    if find_ascii(token, "://").is_some()
        || find_ascii(token, "www.").is_some()
        || token.contains(&AT_SIGN)
        || token.contains(&SLASH)
    {
        return None;
    }

    // §2.4: a task box being typed by hand — `[` directly after the marker's SP
    // run and the typed letter is `x` — is not link text: `- [x] done` must not
    // become `- [X] done`
    {
        let p = parse_prefix(line);
        if p.has_marker
            && !p.has_box
            && before.len() == p.prefix_end + 1
            && before[p.prefix_end] == LBRACKET
            && typed == [b'x' as u16]
        {
            return None;
        }
    }

    // §2.2 pre = before minus its trailing run of openers. A guillemet opener
    // (« ‹ » ›) that is preceded by WS19, by another opener or by nothing but
    // prefix2 carries the WS19 run after it (French spacing: `« Bonjour »`);
    // a closing `»` or `«` (`Done.» then`, `»Hallo.« dann`) keeps its closer role.
    let mut pre_end = before.len();
    let mut quote_stripped = false;
    while pre_end > 0 {
        let unit = before[pre_end - 1];
        if is_opener(unit) {
            pre_end -= 1;
            if is_quote_opener(unit) {
                quote_stripped = true;
            }
            continue;
        }
        if unit == BANG && pre_end < before.len() && before[pre_end] == LBRACKET {
            pre_end -= 1;
            continue;
        }
        let mut w = pre_end;
        while w > 0 && is_ws19(before[w - 1]) {
            w -= 1;
        }
        if w < pre_end && w > 0 && is_guillemet(before[w - 1]) {
            let g = w - 1;
            if g == p2 || (g > 0 && (is_ws19(before[g - 1]) || is_opener(before[g - 1]))) {
                pre_end = w;
                continue;
            }
        }
        break;
    }
    let pre = &before[..pre_end];

    // §2.4 rule A — line start: pre is exactly prefix2, or pre is empty (the
    // caret is at column 0, ahead of whatever prefix the line has: `|- item`,
    // or a selection that starts at the line start is being replaced)
    if pre_end == p2 || pre.is_empty() {
        return Some(upper_typed);
    }

    // §2.5 rule B — sentence start within the line
    let mut i = pre.len();
    let ws_end = i;
    while i > 0 && is_ws19(pre[i - 1]) {
        i -= 1;
    }
    if i == ws_end {
        return None; // 1. ≥ 1 WS19
    }
    // 1a. direct speech after a colon: `:` before the WS19 run and a quotation
    //     opener among the stripped openers (`Он сказал: «`, `He said: "`).
    //     (`i > 0` is md.win's `pre[i - 1]` made total: an all-WS19 `pre` is
    //     always absorbed by prefix2 and answered by rule A above.)
    if quote_stripped && i > 0 && pre[i - 1] == COLON {
        return Some(upper_typed);
    }
    let mut via_dash = false; // 1b. optional group: WS19+ (dash-run | symbols-run) WS19+
    {
        let mut j = i;
        while j > 0 && is_mid_line_dash_unit(pre[j - 1]) {
            j -= 1;
        }
        if j == i {
            j = symbols_run_start(pre, i); // an emoji / keycap between two sentences (`Done. 🎉 next`)
        } else {
            via_dash = true;
        }
        if j < i {
            let mut w = j;
            while w > 0 && is_ws19(pre[w - 1]) {
                w -= 1;
            }
            if w < j {
                i = w;
            } else {
                via_dash = false;
            }
        }
    }
    let mut closers = 0;
    loop {
        // 2. closers, and a footnote reference / citation
        if i > 0
            && pre[i - 1] == RBRACKET
            && let Some(r) = reference_start(pre, i)
        {
            // `text.[^1] Next`, `text.[12] Next` — skipped like a closer
            i = r;
            closers += 1;
            continue;
        }
        if i > 0 && is_closer(pre[i - 1]) {
            i -= 1;
            closers += 1;
            continue;
        }
        break;
    }
    if closers > 0 {
        // French `? »`: WS19 between the terminator and a closer
        while i > 0 && is_ws19(pre[i - 1]) {
            i -= 1;
        }
    }
    let term_end = i;
    let mut dots = 0;
    let mut bang_q = false;
    while i > 0 && is_terminator(pre[i - 1]) {
        if pre[i - 1] == DOT {
            dots += 1;
        } else {
            bang_q = true;
        }
        i -= 1;
    }
    if i == term_end {
        return None; // 3. run length ≥ 1
    }
    if dots >= 2 {
        return None; // ellipsis
    }
    if via_dash && pre[term_end - 1] != DOT {
        return None; // `? —` / `! —` are dialogue tags
    }
    let tok_end = i;
    while i > 0 && !is_ws19(pre[i - 1]) {
        i -= 1; // 4. token
    }
    let mut tok_start = i;
    let mut tok = strip_openers(&pre[tok_start..tok_end]);
    if tok.is_empty()
        && dots == 0
        && tok_end >= 1
        && is_ws19(pre[tok_end - 1])
        && !(tok_end >= 2 && is_ws19(pre[tok_end - 2]))
    {
        // French spacing (`Bonjour !`, `Ça va ?`): exactly one WS19 unit before the
        // run, then the token — never reaching into prefix2
        let mut j = tok_end - 1;
        while j > 0 && !is_ws19(pre[j - 1]) {
            j -= 1;
        }
        if j >= p2 && j < tok_end - 1 {
            tok_start = j;
            tok = strip_openers(&pre[j..tok_end - 1]);
        }
    }
    if tok.is_empty() {
        return None; // (i)
    }
    if bang_q {
        return Some(upper_typed); // a run with `?` or `!` ends the sentence whatever the token
    }
    let tcps = scalars_of(tok);
    if is_single_letter(&tcps) && tcps[0] != 0x44F {
        // (ii) one Letter (+ Marks)
        if is_uppercase(tcps[0]) {
            if prev_token_is_initial(pre, tok_start) {
                return None; // `J. R.`, `z. B.`
            }
        } else {
            return None; // `p. 42`, `т. е.`, `J.` after a name is Lu
        }
    }
    if is_initials_chain(&tcps) {
        return None; // (ii′) compact initials `И.И.`, `J.R.R.`, `U.S.A.`
    }
    if abbreviations().contains(&fold(tok)) {
        return None; // (iii)
    }
    Some(upper_typed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviation_list_matches_md_win() {
        // md.win's HashSet holds 192 entries; a duplicate here would hide a lost one.
        assert_eq!(ABBREVIATIONS.len(), abbreviations().len());
        assert_eq!(ABBREVIATIONS.len(), 192);
    }

    #[test]
    fn upper_is_the_simple_mapping_with_the_dotless_i_exception() {
        assert_eq!(upper(u32::from('a')), Some(u32::from('A')));
        assert_eq!(upper(u32::from('ß')), None);
        assert_eq!(upper(0x0131), Some(0x49));
        assert_eq!(upper(0xB5), None);
        assert_eq!(upper(0x10D0), None);
        assert_eq!(upper(0x1F80), None);
        assert_eq!(upper(0xD800), None);
        assert_eq!(upper(0x10428), Some(0x10400));
    }

    #[test]
    fn lines_split_on_lf_crlf_and_lone_cr() {
        let t = Utf16Text::new("a\r\nb\rc\nd");
        let lines = split_lines(t.units());
        let spans: Vec<(usize, usize)> = lines.iter().map(|l| (l.start, l.end)).collect();
        assert_eq!(spans, vec![(0, 1), (3, 4), (5, 6), (7, 8)]);
        assert_eq!(line_of(&lines, 2), None); // between CR and LF
        assert_eq!(line_of(&lines, 1), Some(0));
        assert_eq!(line_of(&lines, 3), Some(1));
    }
}
