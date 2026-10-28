//! Port of md.win `Text/SmartTypingAdapter.cs`: the pure half of the editor's SmartTyping hooks
//! (smart-typing.md §3.3, the retype rule of §3.4, §3.5) — everything the editor pane works out
//! before it touches the `GtkTextBuffer` that needs no state, kept here so it is tested without a
//! display. The stateful half — the tracked capital and the override of §3.4, the plan between two
//! events, the flags — is [`TypingHooks`](super::TypingHooks).
//!
//! **The letter hook.** The shell reports each change to the buffer with the text it is about to
//! hold; [`insertion_over_selection`] is the diff §3.3 asks for ("exactly one replacement of the
//! current selection"), [`is_word_insertion`] the reduction of an insertion to the one scalar
//! md-core's `smart_typing::capitalize` judges, and [`plan`] the judgement of one such insertion,
//! retype rule included — the override is the hooks' say, made before this is asked.
//! [`applies`] is the check the pane makes before it applies a plan.
//!
//! Character classes and whitespace follow the specification's §0.3–§0.4 to the letter, as the
//! pure functions do: WS19 is md-core's WS set (`whitespace::is_whitespace`, the nineteen units of
//! Foundation's `.whitespaces`, never `char::is_whitespace`), a lowercase letter is general
//! category `Ll` of the *scalar* as .NET answers it (`unicode::category`), and the uppercase
//! mapping is the pure function's own ([`upper`]), so the retype rule and the capital it undoes
//! cannot disagree.
//!
//! LINUX DECISION (offsets): every offset in and out is a UTF-16 code unit — md.win's, and the
//! shared vectors' — and strings are `&str`. The GTK shell converts a `TextIter` char offset with
//! `md_core::text::utf16::utf16_from_char_offset` on the way in and
//! `char_offset_from_utf16` on the way out. Selection arguments are `isize` like md-core's editing
//! API, so a negative or out-of-range selection is answered (with `None`) rather than impossible to
//! express. A Rust string holds no lone surrogate, so md.win's lone-surrogate cases cannot arise; an
//! offset inside a surrogate pair (which no `TextIter` produces) splits the pair, and the half that
//! would be a lone surrogate comes back as U+FFFD.
//!
//! LINUX DECISION (line breaks): the `GtkTextBuffer` holds LF-only text (GTK's Enter inserts
//! `"\n"`, and the shell hands the editor the session's LF text), which is exactly what md-core's
//! rules read and write. md.win swaps LF for CR because WinUI's `TextBox` stores a line break as
//! U+000D; nothing is swapped here.

use md_core::editing::smart_typing::{self, EnterEdit};
use md_core::text::unicode::{self, GeneralCategory};
use md_core::text::utf16::{Utf16Text, utf16_len};
use md_core::text::whitespace;

/// What the pane applies after the buffer has inserted the typed text: replace
/// `[position, position + utf16_len(inserted))` — the insertion, as it now sits in the buffer —
/// with `replacement`, the same text with its first scalar as `capital`, and put the cursor after
/// it. That replacement is the second undo unit of §3.5, the native insertion the first; `capital`
/// at `position` is what [`CapitalTracker`](super::CapitalTracker) then tracks (§3.4). `replaced`
/// is the length of the selection the insertion replaced, for [`applies`]. Offsets and lengths are
/// UTF-16 units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapitalizationPlan {
    pub position: usize,
    pub inserted: String,
    pub capital: String,
    pub replacement: String,
    pub replaced: usize,
}

impl CapitalizationPlan {
    /// md.win's positional record constructor, `Replaced` defaulting to 0.
    pub fn new(
        position: usize,
        inserted: impl Into<String>,
        capital: impl Into<String>,
        replacement: impl Into<String>,
    ) -> Self {
        CapitalizationPlan {
            position,
            inserted: inserted.into(),
            capital: capital.into(),
            replacement: replacement.into(),
            replaced: 0,
        }
    }

    /// The same plan over a selection of `replaced` units.
    pub fn replacing(mut self, replaced: usize) -> Self {
        self.replaced = replaced;
        self
    }
}

/// `(start, end)` as units when `start >= 0`, `length >= 0` and `start + length <= len`.
pub(crate) fn selection_in(len: usize, selection_start: isize, selection_length: isize) -> Option<(usize, usize)> {
    let start = usize::try_from(selection_start).ok()?;
    let length = usize::try_from(selection_length).ok()?;
    let end = start.checked_add(length)?;
    (end <= len).then_some((start, end))
}

// ── §3.3: the diff ─────────────────────────────────────────────────────────────────────────

/// The text an edit inserts in place of the selection, or `None` when the edit is anything else.
/// `new_text` is one insertion over the selection iff it equals `old[0, start) + inserted +
/// old[start + length, …)` for a non-empty `inserted`. A deletion, an undo, a replacement elsewhere
/// and an unchanged text all answer `None` — and none of them is ever capitalized or mistaken for
/// typing at the selection.
pub fn insertion_over_selection(
    old_text: &str,
    selection_start: isize,
    selection_length: isize,
    new_text: &str,
) -> Option<String> {
    let old = Utf16Text::new(old_text);
    let new = Utf16Text::new(new_text);
    insertion_units(old.units(), selection_start, selection_length, new.units())
        .map(|(start, inserted)| String::from_utf16_lossy(&new.units()[start..start + inserted]))
}

/// [`insertion_over_selection`] on units: the insertion's start and unit length.
fn insertion_units(
    old: &[u16],
    selection_start: isize,
    selection_length: isize,
    new: &[u16],
) -> Option<(usize, usize)> {
    let (start, end) = selection_in(old.len(), selection_start, selection_length)?;
    let inserted = (new.len() + (end - start)).checked_sub(old.len()).filter(|&n| n > 0)?;
    if new[..start] != old[..start] || new[start + inserted..] != old[end..] {
        return None;
    }
    Some((start, inserted))
}

// ── §3.3 first paragraph: reduction to one scalar ──────────────────────────────────────────

/// §3.3: an insertion is a *word insertion* iff it contains no line terminator and no WS19 unit
/// except at most one trailing SP, its first scalar is a Lowercase letter (`Ll`), and it contains
/// none of `://`, `www.`, `@`, `/` — §2.1f applied to the insertion itself, because the pure
/// function sees only the first scalar and cannot tell a pasted `https://a.b` or `@nettrash` from
/// a word.
pub fn is_word_insertion(insertion: &str) -> bool {
    let Some(first) = insertion.chars().next() else { return false };
    let last = insertion.chars().count() - 1;
    for (i, c) in insertion.chars().enumerate() {
        // §3.3's line terminators are LF and CR only: U+2028, U+0085 & co. are content.
        if c == '\n' || c == '\r' {
            return false;
        }
        // WS19 members are all BMP, so a char walk agrees with md.win's unit walk.
        if whitespace::is_whitespace(c) && !(c == ' ' && i == last) {
            return false;
        }
    }
    if unicode::category(first) != GeneralCategory::LowercaseLetter {
        return false;
    }
    !insertion.contains("://") && !insertion.contains("www.") && !insertion.contains('@') && !insertion.contains('/')
}

/// The first scalar of a non-empty string as its own string (one unit or a surrogate pair).
///
/// # Panics
///
/// For an empty string (md.win throws `ArgumentException`).
pub fn first_scalar(text: &str) -> String {
    text.chars().next().map(String::from).unwrap_or_else(|| panic!("first_scalar: empty text"))
}

// ── §0.7 through the pure function ─────────────────────────────────────────────────────────

/// `upper(s)` exactly as SmartTyping defines it — simple mapping, one scalar, the Georgian /
/// Greek-iota / micro-sign exclusions and the vector-decided dotless i — or `None` when undefined.
/// §0.9 item 5: on an empty document rule A applies unconditionally, so the pure function over `""`
/// is the mapping itself, and there is no second copy of §0.7 to drift from the first.
pub fn upper(scalar: &str) -> Option<String> {
    smart_typing::capitalize("", 0, 0, scalar)
}

// ── §3.4, second rule ──────────────────────────────────────────────────────────────────────

/// "Independently of the override, a single lowercase letter typed over a one-scalar selection
/// whose scalar is that letter's `upper` is always inserted as typed": the select-and-retype
/// gesture, which needs no state.
pub fn retypes_own_capital(text: &str, selection_start: isize, selection_length: isize, typed_scalar: &str) -> bool {
    let units = Utf16Text::new(text);
    let Some((start, end)) = selection_in(units.len(), selection_start, selection_length) else { return false };
    if start == end {
        return false;
    }
    let selected = &units.units()[start..end];
    if scalar_len(selected, 0) != selected.len() {
        return false;
    }
    match upper(typed_scalar) {
        Some(upper) => upper.encode_utf16().eq(selected.iter().copied()),
        None => false,
    }
}

// ── The letter hook, whole ─────────────────────────────────────────────────────────────────

/// What the hooks decide for a change they have already tracked and not waved through as typed,
/// given the text the buffer holds, its selection and the text it is about to hold. `None` means
/// "let the edit stand as it is": it is not an insertion over the selection, not a word insertion,
/// the retype rule says "as typed", or the pure function answers `None`. Pure: the override (§3.4)
/// is [`CapitalTracker`](super::CapitalTracker)'s decision, taken by
/// [`TypingHooks::before_text_changing`](super::TypingHooks::before_text_changing) before this is
/// called, and a paste is the hooks' flag — neither is asked here.
pub fn plan(
    old_text: &str,
    selection_start: isize,
    selection_length: isize,
    new_text: &str,
) -> Option<CapitalizationPlan> {
    let inserted = insertion_over_selection(old_text, selection_start, selection_length, new_text)?;
    if !is_word_insertion(&inserted) {
        return None;
    }
    let first = first_scalar(&inserted);
    if retypes_own_capital(old_text, selection_start, selection_length, &first) {
        return None;
    }
    let capital = smart_typing::capitalize(old_text, selection_start, selection_start + selection_length, &first)?;
    let replacement = format!("{capital}{}", &inserted[first.len()..]);
    // Non-negative: insertion_over_selection checked the selection.
    let position = selection_start as usize;
    Some(CapitalizationPlan::new(position, inserted, capital, replacement).replacing(selection_length as usize))
}

/// Whether a plan still fits the buffer once the edit it was made for has been applied: the
/// inserted text sits where the selection was, and the selection is either the collapsed cursor
/// after it or — should the shell report it a beat late — still exactly what the insertion
/// replaced. Anything else — the input was transformed, the edit was cancelled, another change got
/// in first — and the plan is dropped, so the worst outcome of a surprise is a missing capital,
/// never a mangled word.
pub fn applies(plan: &CapitalizationPlan, text: &str, selection_start: isize, selection_length: isize) -> bool {
    let units = Utf16Text::new(text);
    let inserted: Vec<u16> = plan.inserted.encode_utf16().collect();
    let end = plan.position + inserted.len();
    if end > units.len() || units.units()[plan.position..end] != inserted[..] {
        return false;
    }
    (selection_length == 0 && selection_start == end as isize)
        || (selection_start == plan.position as isize && selection_length == plan.replaced as isize)
}

// ── Enter ──────────────────────────────────────────────────────────────────────────────────

/// The Enter hook: md-core's `smart_typing::enter` over the buffer's own text and selection.
///
/// LINUX DECISION: the replacement keeps md-core's LF. md.win swaps it for CR, the line break
/// WinUI's `TextBox` stores; the `GtkTextBuffer` holds LF, so the edit goes in as the pure function
/// spells it and `location`, `length` and `caret` are the function's own.
pub fn enter(box_text: &str, selection_start: isize, selection_length: isize) -> Option<EnterEdit> {
    selection_in(utf16_len(box_text), selection_start, selection_length)?;
    smart_typing::enter(box_text, selection_start, selection_start + selection_length)
}

// ── §0.1: the units the rules above read ───────────────────────────────────────────────────

/// The unit length (1 or 2) of the scalar starting at unit `i`; a lone surrogate is its own scalar.
pub(crate) fn scalar_len(s: &[u16], i: usize) -> usize {
    let high = (0xD800..=0xDBFF).contains(&s[i]);
    if high && i + 1 < s.len() && (0xDC00..=0xDFFF).contains(&s[i + 1]) { 2 } else { 1 }
}
