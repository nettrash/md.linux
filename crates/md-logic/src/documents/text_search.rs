//! Port of md.win `Documents/TextSearch.cs`.
//!
//! The find bar's engine (§3.5): ordinal, case-insensitive, wrapping — for the search and for the
//! replace, which is the same one rule and no second one. Ordinal because a Markdown document is
//! not prose in one language — a culture-sensitive search would fold "ß" into "ss" and select a
//! range whose length does not match the query, which is exactly what the caller then hands to the
//! editor's selection. No regular expressions: a query is the characters typed into the box.
//!
//! Offsets are UTF-16 units, as in md.win and everywhere in md-logic; the GTK shell converts its
//! character offsets with md-core's `text::utf16` helpers. "Case-insensitive" is .NET's
//! `OrdinalIgnoreCase`: each scalar compared after its simple invariant upper-case mapping
//! (md-core `unicode::to_upper_invariant`), which never changes a scalar's UTF-16 length, so a hit
//! is as long as the query and always starts and ends on a scalar boundary. (md.win also handles a
//! lone surrogate; a Rust string cannot hold one.)

use md_core::text::unicode;

/// Where a hit sits in the searched string; `length` is the query's, ordinal matching being 1:1
/// in UTF-16 units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Match {
    pub index: usize,
    pub length: usize,
}

impl Match {
    pub const fn new(index: usize, length: usize) -> Self {
        Match { index, length }
    }
}

/// One edit to the searched string: `[start, start + length)` becomes `text`. Plain data — nothing
/// here touches a widget.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Edit {
    pub start: usize,
    pub length: usize,
    pub text: String,
}

impl Edit {
    pub fn new(start: usize, length: usize, text: &str) -> Self {
        Edit { start, length, text: text.to_string() }
    }
}

/// One press of Replace. `apply` is the edit to make, or `None` when the caret is not standing on
/// a hit — Replace is a plain Find Next then, which is what every find bar does and what makes
/// "Replace, Replace, Replace…" walk the document. `search_from` is where the search that follows
/// starts, measured in the text the edit leaves behind: past the replacement, so one that contains
/// the query is not found again by the step that made it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplaceStep {
    pub apply: Option<Edit>,
    pub search_from: usize,
}

/// What Replace All does, decided in one pass. `text` is the whole new string and `count` the
/// number of hits replaced; `apply` is the same answer as the **single** edit that produces it —
/// from the first hit's start to the last hit's end, with everything between them carried across —
/// so the shell can put it in as one buffer edit inside one user action (one undo step). The two
/// agree by construction. With no hit, `count` is 0, `text` is the text as given and `apply` is an
/// empty edit at 0.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReplaceAllPlan {
    pub text: String,
    pub count: usize,
    pub apply: Edit,
}

/// The find panel's count: `current` is the selection's place among the hits (1-based), 0 when
/// the selection is not one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tally {
    pub current: usize,
    pub total: usize,
}

impl Tally {
    pub const fn new(current: usize, total: usize) -> Self {
        Tally { current, total }
    }
}

/// A string as UTF-16 units, plus the same units case-folded for matching.
struct Units {
    units: Vec<u16>,
    folded: Vec<u16>,
}

impl Units {
    fn new(text: &str) -> Self {
        let mut units = Vec::with_capacity(text.len());
        let mut folded = Vec::with_capacity(text.len());
        let mut buffer = [0u16; 2];
        for c in text.chars() {
            let own = c.encode_utf16(&mut buffer).len();
            units.extend_from_slice(&buffer[..own]);
            let upper = unicode::to_upper_invariant(c);
            // A simple mapping never changes the UTF-16 length; if a table ever did, matching the
            // scalar as itself keeps the offsets of the two vectors aligned.
            let mapped = if upper.len_utf16() == own { upper } else { c };
            folded.extend_from_slice(mapped.encode_utf16(&mut buffer));
        }
        Units { units, folded }
    }

    fn len(&self) -> usize {
        self.units.len()
    }

    /// `IndexOf(query, from, OrdinalIgnoreCase)`.
    fn index_of(&self, query: &Units, from: usize) -> Option<usize> {
        let n = query.len();
        if n == 0 || n > self.len() {
            return None;
        }
        (from..=self.len() - n).find(|&at| self.folded[at..at + n] == query.folded[..])
    }
}

/// The first hit at or after `from`, wrapping to the top; `None` when the query is empty or absent.
pub fn next(text: &str, query: &str, from: i64) -> Option<Match> {
    let (t, q) = (Units::new(text), Units::new(query));
    if q.len() == 0 || q.len() > t.len() {
        return None;
    }
    let start = clamp(from, t.len());
    let at = if start > t.len() - q.len() { None } else { t.index_of(&q, start) };
    at.or_else(|| t.index_of(&q, 0)).map(|at| Match::new(at, q.len()))
}

/// The last hit starting before `from`, wrapping to the bottom; `None` when the query is empty or
/// absent.
pub fn previous(text: &str, query: &str, from: i64) -> Option<Match> {
    let (t, q) = (Units::new(text), Units::new(query));
    if q.len() == 0 || q.len() > t.len() {
        return None;
    }
    let limit = clamp(from, t.len());
    last_index_before(&t, &q, limit).or_else(|| last_index_before(&t, &q, t.len())).map(|at| Match::new(at, q.len()))
}

/// Whether the selection is itself a hit for `query` — "the match we are standing on", which is
/// the whole of Replace's decision. Ordinal and case-insensitive like the search, so the `Alpha`
/// that Next selected is replaceable with `alpha` still in the query box; a selection of any other
/// length never is. A selection outside the text, or a negative one, is not a match rather than a
/// panic: the caller reads it off a live widget.
pub fn selection_is_match(text: &str, query: &str, selection_start: i64, selection_length: i64) -> bool {
    let (t, q) = (Units::new(text), Units::new(query));
    selection_is_match_units(&t, &q, selection_start, selection_length)
}

fn selection_is_match_units(t: &Units, q: &Units, selection_start: i64, selection_length: i64) -> bool {
    let n = q.len() as i64;
    if n == 0 || selection_length != n {
        return false;
    }
    if selection_start < 0 || selection_start > t.len() as i64 - n {
        return false;
    }
    let start = selection_start as usize;
    t.folded[start..start + q.len()] == q.folded[..]
}

/// One press of Replace over the selection the editor reports. When the selection is the hit
/// ([`selection_is_match`]) the step replaces it and the next search starts after what went in;
/// otherwise nothing is edited and the next search starts at the selection's end, which is where
/// [`next`] would start for Find Next. The caller applies the edit, re-reads the text and calls
/// [`next`] from [`ReplaceStep::search_from`].
pub fn replace(text: &str, query: &str, replacement: &str, selection_start: i64, selection_length: i64) -> ReplaceStep {
    let (t, q) = (Units::new(text), Units::new(query));
    let start = clamp(selection_start, t.len());
    let length = clamp(selection_length, t.len() - start);
    if selection_is_match_units(&t, &q, selection_start, selection_length) {
        let replacement_length = replacement.encode_utf16().count();
        ReplaceStep { apply: Some(Edit::new(start, length, replacement)), search_from: start + replacement_length }
    } else {
        ReplaceStep { apply: None, search_from: start + length }
    }
}

/// Every hit replaced, left to right, in one pass. The scan resumes after each hit **in the old
/// text** and never re-enters what was just put in, so a replacement that contains the query ("a"
/// → "aa") replaces each hit exactly once instead of growing forever, and the pass is linear in
/// hits. Hits do not overlap: "aa" over "aaaa" is two, not three.
pub fn replace_all(text: &str, query: &str, replacement: &str) -> ReplaceAllPlan {
    let nothing = || ReplaceAllPlan { text: text.to_string(), count: 0, apply: Edit::new(0, 0, "") };
    let (t, q) = (Units::new(text), Units::new(query));
    if q.len() == 0 || q.len() > t.len() {
        return nothing();
    }
    let replacement_units: Vec<u16> = replacement.encode_utf16().collect();

    let mut built: Vec<u16> = Vec::new();
    let mut first: Option<usize> = None;
    let mut after = 0;
    let mut from = 0;
    let mut count = 0;
    while from <= t.len() - q.len() {
        let Some(at) = t.index_of(&q, from) else { break };
        if first.is_none() {
            first = Some(at);
        } else {
            built.extend_from_slice(&t.units[after..at]); // what stood between this hit and the last
        }
        built.extend_from_slice(&replacement_units);
        after = at + q.len();
        from = after;
        count += 1;
    }
    let Some(first) = first else { return nothing() };

    // Hits start and end on scalar boundaries (see the module comment), so every slice is whole.
    let edit = Edit { start: first, length: after - first, text: String::from_utf16_lossy(&built) };
    let mut whole = Vec::with_capacity(t.len() - edit.length + built.len());
    whole.extend_from_slice(&t.units[..first]);
    whole.extend_from_slice(&built);
    whole.extend_from_slice(&t.units[after..]);
    ReplaceAllPlan { text: String::from_utf16_lossy(&whole), count, apply: edit }
}

/// "3 of 12" (2026-09-27, the find panel). [`Tally::total`] counts hits the way [`replace_all`]
/// replaces them — left to right, never overlapping — so the number the panel shows is the number
/// Replace All would change. [`Tally::current`] is the selection's place in that run when the
/// selection is a hit ([`selection_is_match`]): one plus the hits that start before it, which also
/// places a hit that overlaps a counted one.
pub fn count(text: &str, query: &str, selection_start: i64, selection_length: i64) -> Tally {
    let (t, q) = (Units::new(text), Units::new(query));
    if q.len() == 0 || q.len() > t.len() {
        return Tally::new(0, 0);
    }
    let on_hit = selection_is_match_units(&t, &q, selection_start, selection_length);
    let mut total = 0;
    let mut before = 0;
    let mut from = 0;
    while from <= t.len() - q.len() {
        let Some(at) = t.index_of(&q, from) else { break };
        total += 1;
        if on_hit && (at as i64) < selection_start {
            before += 1;
        }
        from = at + q.len();
    }
    // A hit that overlaps the last counted one ("aa" at 1 in "aaa") is placed on that one, never
    // past the total: the panel must not read "2 of 1".
    Tally::new(if on_hit { (before + 1).min(total) } else { 0 }, total)
}

/// Forward scan keeping the last hit that starts before the limit — md.win's `IndexOf` loop rather
/// than `LastIndexOf`, whose backwards window is the classic source of an off-by-one.
fn last_index_before(t: &Units, q: &Units, limit: usize) -> Option<usize> {
    let mut best = None;
    let mut i = 0;
    while i <= t.len() - q.len() {
        match t.index_of(q, i) {
            Some(at) if at < limit => {
                best = Some(at);
                i = at + 1;
            }
            _ => break,
        }
    }
    best
}

/// `Math.Clamp(value, 0, max)` for an offset read off a widget.
fn clamp(value: i64, max: usize) -> usize {
    value.clamp(0, i64::try_from(max).unwrap_or(i64::MAX)) as usize
}
