//! Port of md.win `Text/TypingHooks.cs`: the per-editor state of the two SmartTyping hooks
//! (shell-design §3.6) — [`TypingHooks`], the edit every change is reduced to ([`TextEdit`]), the
//! side a deletion key removes ([`DeleteDirection`]) and the §3.4 state machine
//! ([`CapitalTracker`]).
//!
//! LINUX DECISION (offsets): as in [`smart_typing_adapter`](super::smart_typing_adapter), every
//! offset is a UTF-16 code unit and every selection argument an `isize`; the GTK shell converts
//! `TextIter` char offsets at the boundary (`md_core::text::utf16`).

use std::cell::{Cell, RefCell, RefMut};
use std::fmt;

use md_core::editing::smart_typing::EnterEdit;
use md_core::text::utf16::{Utf16Text, utf16_len};

use super::smart_typing_adapter::{self as adapter, CapitalizationPlan};
use crate::commands::keys;
use crate::settings::TypingSettings;
use crate::settings::settings_keys;

const TRACE_LIMIT: usize = 64;

/// The per-editor state of the two SmartTyping hooks (shell-design §3.6): the half of the editor's
/// typing that is a state machine rather than a pure function — the tracked capital and the
/// override of smart-typing.md §3.4 ([`CapitalTracker`]), the plan carried from
/// [`TypingHooks::before_text_changing`] to [`TypingHooks::text_changing`], the two settings, and
/// the five flags that decide whether a change is judged at all. Kept out of the editor pane so a
/// fake text box can drive it in the widget's own event order, with the session's echo of every
/// keystroke and the widget's Undo / Redo in the loop: that is where md.win's first cut of the
/// pane lost the override — its `SetText` cleared the gesture on the echo of identical text, and
/// its `BeforeTextChanging` judged a Redo as typing — and the adapter's tests, which see only the
/// pure functions, could not tell. The pane owns one of these, feeds it the buffer's text and
/// selection at each event, and applies what it answers; nothing here touches a widget.
///
/// **Tracking sees every edit; judging sees some.** [`TypingHooks::before_text_changing`] turns
/// each change into one [`TextEdit`] and hands it to the tracker first — typing, Backspace, Delete,
/// Cut, a paste, a composition update, and the widget's own Undo and Redo, which bypass the judging
/// (§3.5) but change the text like anything else. Only then does it decide whether the change is
/// judged for a capital. The two edits this type makes itself — the capital swap and the Enter edit
/// — are applied under [`TypingHooks::applying`], so the events they raise are skipped, and are
/// reported to the tracker by [`TypingHooks::text_changing`] and [`TypingHooks::enter`] instead.
/// The pane's `set_text` echo of identical text raises no event at all; a text that differs is
/// assigned under [`TypingHooks::replacing`] and [`TypingHooks::external_text`] has already cleared
/// the tracker (§3.4: an external replacement clears both). An external replacement whose text
/// equals the buffer's — another article holding the same text, a reload the disk agrees with — is
/// one `set_text` cannot see, and the windows report it through [`TypingHooks::reset`] on the
/// session's undo generation, beside clearing the buffer's own history.
///
/// **Which unit a key removed.** A shrink at a collapsed cursor is the units before it (Backspace,
/// Ctrl+Backspace) or after it (Delete, Ctrl+Delete), and inside a run of identical units the text
/// alone cannot tell which — `II` → `I` is either. [`TypingHooks::announce_deletion`] (settled a
/// main-loop turn later, like the history flag) hands [`TextEdit::between`] the side, so that
/// removing md's `I` next to the writer's arms the override and removing the writer's leaves the
/// capital tracked.
///
/// **Flags.** A composition ([`TypingHooks::composition_started`] …
/// [`TypingHooks::composition_ended`]) skips both features. A paste
/// ([`TypingHooks::announce_paste`]) makes the change it produces never capitalized; it is
/// consumed by that change and settled a turn later in case the clipboard had nothing to give. A
/// history operation ([`TypingHooks::announce_history`]) is not typing: the change it produces is
/// not judged — a Redo that re-inserts a letter is not capitalized into a new edit that would wipe
/// the redo stack — but it is tracked, which is how Undo of a capital arms the override (§3.4: the
/// undo is an edit that removed the capital). It is settled a turn later, never consumed, because
/// one history step may report more than one change.
///
/// Every method takes `&self` (state in cells) and no borrow is held while a caller's closure runs,
/// so the shell's signal handlers — and the fake box's — may call back in from inside
/// [`TypingHooks::applying`].
pub struct TypingHooks {
    tracker: RefCell<CapitalTracker>,
    pending: RefCell<Option<CapitalizationPlan>>,
    // md.win marks these `volatile` (the settings store may raise Changed on another thread); a
    // GTK shell has one thread, so cells do.
    continue_lists: Cell<bool>,
    capitalize_sentences: Cell<bool>,
    composing: Cell<bool>,
    pasting: Cell<bool>,
    history: Cell<bool>,
    applying: Cell<bool>,
    replacing: Cell<bool>,
    deleting: Cell<DeleteDirection>,
    last_decision: RefCell<String>,
    trace: RefCell<Vec<String>>,
}

impl Default for TypingHooks {
    fn default() -> Self {
        TypingHooks {
            tracker: RefCell::new(CapitalTracker::new()),
            pending: RefCell::new(None),
            continue_lists: Cell::new(settings_keys::CONTINUE_LISTS_DEFAULT),
            capitalize_sentences: Cell::new(settings_keys::CAPITALIZE_SENTENCES_DEFAULT),
            composing: Cell::new(false),
            pasting: Cell::new(false),
            history: Cell::new(false),
            applying: Cell::new(false),
            replacing: Cell::new(false),
            deleting: Cell::new(DeleteDirection::Unknown),
            last_decision: RefCell::new(String::new()),
            trace: RefCell::new(Vec::new()),
        }
    }
}

impl fmt::Debug for TypingHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TypingHooks")
            .field("tracker", &self.tracker.borrow())
            .field("pending", &self.pending.borrow())
            .field("continue_lists", &self.continue_lists.get())
            .field("capitalize_sentences", &self.capitalize_sentences.get())
            .field("composing", &self.composing.get())
            .field("pasting", &self.pasting.get())
            .field("history", &self.history.get())
            .field("deleting", &self.deleting.get())
            .field("last_decision", &self.last_decision.borrow())
            .finish()
    }
}

/// Sets a flag for the life of the guard and clears it on drop — C#'s `try … finally`, so a
/// panicking closure cannot leave the flag up.
struct FlagGuard<'a>(&'a Cell<bool>);

impl<'a> FlagGuard<'a> {
    fn raise(flag: &'a Cell<bool>) -> Self {
        flag.set(true);
        FlagGuard(flag)
    }
}

impl Drop for FlagGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl TypingHooks {
    pub fn new() -> Self {
        Self::default()
    }

    /// The §3.4 state — the tracked capital, or the armed override — as it stands now (a copy, for
    /// inspection; the hooks drive it).
    pub fn tracker(&self) -> CapitalTracker {
        self.tracker.borrow().clone()
    }

    /// The tracker itself, for a test or a self-test that drives it directly. Never hold the guard
    /// across a call into the hooks.
    pub fn tracker_mut(&self) -> RefMut<'_, CapitalTracker> {
        self.tracker.borrow_mut()
    }

    pub fn continue_lists(&self) -> bool {
        self.continue_lists.get()
    }

    pub fn capitalize_sentences(&self) -> bool {
        self.capitalize_sentences.get()
    }

    pub fn is_composing(&self) -> bool {
        self.composing.get()
    }

    pub fn is_pasting(&self) -> bool {
        self.pasting.get()
    }

    pub fn is_history(&self) -> bool {
        self.history.get()
    }

    /// The side of the cursor the announced Backspace / Delete removes, or `Unknown` when none is
    /// announced.
    pub fn deleting(&self) -> DeleteDirection {
        self.deleting.get()
    }

    /// The pane's own `set_text` assignment is in progress: its change is not an edit.
    pub fn is_replacing(&self) -> bool {
        self.replacing.get()
    }

    /// A plan is waiting for [`TypingHooks::text_changing`].
    pub fn has_pending_plan(&self) -> bool {
        self.pending.borrow().is_some()
    }

    /// What the last [`TypingHooks::before_text_changing`] / [`TypingHooks::text_changing`] pair
    /// decided, in words, for a self-test's report: a capital that did not appear on a real
    /// keystroke is otherwise indistinguishable from one the rules declined. Never read by the app.
    pub fn last_decision(&self) -> String {
        self.last_decision.borrow().clone()
    }

    /// Every decision since [`TypingHooks::clear_trace`], oldest first (at most 64): one keystroke's
    /// answer is not the scenario's.
    pub fn trace(&self) -> Vec<String> {
        self.trace.borrow().clone()
    }

    /// Starts a new [`TypingHooks::trace`].
    pub fn clear_trace(&self) {
        self.trace.borrow_mut().clear();
    }

    fn decide(&self, decision: String) {
        let mut trace = self.trace.borrow_mut();
        if trace.len() < TRACE_LIMIT {
            trace.push(decision.clone());
        }
        *self.last_decision.borrow_mut() = decision;
    }

    // ── settings (smart-typing.md §3.1) ────────────────────────────────────────────────────

    /// Both bools, re-read on every store change for their keys. A function whose setting is off is
    /// never called, and its gesture has nothing to undo.
    pub fn configure(&self, typing: TypingSettings) {
        self.continue_lists.set(typing.continue_lists);
        self.capitalize_sentences.set(typing.capitalize_sentences);
        if !typing.capitalize_sentences {
            self.tracker.borrow_mut().clear();
        }
    }

    // ── flags ──────────────────────────────────────────────────────────────────────────────

    pub fn composition_started(&self) {
        self.composing.set(true);
    }

    pub fn composition_ended(&self) {
        self.composing.set(false);
    }

    /// A paste is about to land (§3.3): the change it produces is never capitalized.
    pub fn announce_paste(&self) {
        self.pasting.set(true);
    }

    /// One main-loop turn after the announcement: a paste that produced no change must not skip
    /// the next keystroke.
    pub fn settle_paste(&self) {
        self.pasting.set(false);
    }

    /// Undo or Redo is about to run: the change it produces is tracked but not judged (§3.5).
    pub fn announce_history(&self) {
        self.history.set(true);
    }

    /// One main-loop turn after the announcement: a history step with nothing to do must not skip
    /// the next keystroke.
    pub fn settle_history(&self) {
        self.history.set(false);
    }

    /// Backspace or Delete is about to run: the shrink the buffer reports is anchored at the cursor
    /// on that side, which is the only way to tell which of two identical units it removed. Never
    /// handled — the text view deletes as it always did; `Unknown` announces nothing.
    pub fn announce_deletion(&self, direction: DeleteDirection) {
        self.deleting.set(direction);
    }

    /// One main-loop turn after the announcement, never consumed: a key with nothing to delete
    /// reports no change, and a word deletion is still one.
    pub fn settle_deletion(&self) {
        self.deleting.set(DeleteDirection::Unknown);
    }

    /// The side a key removes from, by GDK keyval name: BackSpace the units before the cursor,
    /// Delete (and the keypad's KP_Delete) the units after it, with any modifier (Ctrl+BackSpace is
    /// a word backward; Shift+Delete is Cut, a selection the text settles by itself). Anything else
    /// `Unknown`.
    ///
    /// LINUX DECISION: md.win reads Win32 virtual keys (`VK_BACK`, `VK_DELETE`); GTK hands the
    /// shell a keyval, named here as `gdk_keyval_name` spells it. GtkTextView 4.22 also reports the
    /// same keys as its `backspace` and `delete-from-cursor` keybinding signals, which a shell may
    /// use instead.
    pub fn deletion_key(keyval_name: &str) -> DeleteDirection {
        match keyval_name {
            keys::BACKSPACE => DeleteDirection::Backward,
            keys::DELETE | keys::KP_DELETE => DeleteDirection::Forward,
            _ => DeleteDirection::Unknown,
        }
    }

    /// Whether a key chord is the text view's own Undo / Redo — Ctrl+Z, Ctrl+Y and Shift+Ctrl+Z,
    /// GtkTextView 4.22's three history bindings — and so announces a history operation. Alt
    /// excludes it: a layout that types a letter on Ctrl+Alt+Z must be judged like any other.
    ///
    /// LINUX DECISION: by GDK keyval name. With Shift held GTK reports the shifted keyval (`"Z"`),
    /// so both cases are accepted. The `GtkTextBuffer::undo` / `redo` signals, emitted for the
    /// keys and for `gtk_text_buffer_undo()` alike, are the shell's simpler hook.
    pub fn is_history_chord(keyval_name: &str, control: bool, alt: bool) -> bool {
        control && !alt && matches!(keyval_name, "z" | "Z" | "y" | "Y")
    }

    /// Our own replacement — the Enter edit, the capital — applied with the flag up so the events it
    /// raises are neither judged nor tracked again (its caller has already reported it).
    pub fn applying(&self, apply: impl FnOnce()) {
        let _flag = FlagGuard::raise(&self.applying);
        apply();
    }

    /// An edit md makes to the text on the writer's behalf that is not typing: the find bar's
    /// Replace and Replace All (shell-design §3.5), and a Format command. The text going in is the
    /// writer's own, so it must land exactly as typed — never judged for a capital, and never
    /// tracked as though the writer had typed it there. The tracking is therefore cleared first,
    /// the way a Revert or a Reload clears it (§3.4: an external replacement clears both the tracked
    /// capital and the override), and the replacement itself runs under [`TypingHooks::applying`],
    /// whose events are skipped. What is *not* cleared is the buffer's undo history: a replace is
    /// one undoable step like any other edit, which is the difference between this and
    /// [`TypingHooks::reset`] beside clearing the history.
    pub fn external_edit(&self, apply: impl FnOnce()) {
        self.reset();
        self.applying(apply);
    }

    /// The pane's `set_text` assignment, with the flag up so its events are neither judged nor
    /// reported as an edit.
    pub fn replacing(&self, replace: impl FnOnce()) {
        let _flag = FlagGuard::raise(&self.replacing);
        replace();
    }

    // ── set_text ───────────────────────────────────────────────────────────────────────────

    /// `set_text(incoming)` over a buffer holding `current`: true when the text differs and is to be
    /// assigned — an external replacement, which clears the tracked capital, the override and any
    /// pending plan (§3.4) — and false for the echo of identical text, which changes nothing and
    /// leaves the state as it is. Compared exactly, as given.
    pub fn external_text(&self, current: &str, incoming: &str) -> bool {
        if current == incoming {
            return false;
        }
        self.reset();
        true
    }

    /// An external replacement the text cannot show: the windows' signal for one is the session's
    /// undo generation — a fresh document, Revert, Reload from Disk, an article switch — and they
    /// report it here, beside clearing the buffer's own history, because the new text may equal the
    /// old. [`TypingHooks::external_text`] sees only the echo then. Clears the tracked capital, the
    /// override and any pending plan (§3.4); the flags, which describe the input in flight, are left
    /// alone.
    pub fn reset(&self) {
        self.tracker.borrow_mut().clear();
        *self.pending.borrow_mut() = None;
    }

    // ── the hooks ──────────────────────────────────────────────────────────────────────────

    /// Enter with no modifier: the continuation edit to apply as one replacement, or `None` when not
    /// ours — the setting off, a composition in progress, or the function declining. The edit is
    /// reported to the tracker on the way, since the pane applies it under
    /// [`TypingHooks::applying`] and the events are skipped: it may sit before the capital
    /// (shifting it), remove it (the exit rule clearing an item that holds it), or be an insertion
    /// elsewhere than an armed `q` (§3.4).
    pub fn enter(&self, text: &str, selection_start: isize, selection_length: isize) -> Option<EnterEdit> {
        if !self.continue_lists.get() || self.composing.get() {
            return None;
        }
        let edit = adapter::enter(text, selection_start, selection_length)?;
        self.tracker.borrow_mut().edited(&TextEdit::new(edit.location, edit.length, edit.replacement.clone()), false);
        Some(edit)
    }

    /// Before a change: the buffer's text and selection as they stand, and the text the buffer is
    /// about to hold. The change is reduced to one [`TextEdit`] and tracked (§3.4) whatever it is,
    /// unless it is our own replacement or an external assignment, which their callers report. Then
    /// the plan [`TypingHooks::text_changing`] applies is decided, or none: a composition and a
    /// history operation are never judged; a paste is judged with the paste flag (consumed here);
    /// the setting off ends it; an insertion the tracker says goes in as typed ends it; the rest is
    /// [`smart_typing_adapter::plan`](super::smart_typing_adapter::plan).
    pub fn before_text_changing(&self, text: &str, selection_start: isize, selection_length: isize, new_text: &str) {
        *self.pending.borrow_mut() = None;
        if self.applying.get() || self.replacing.get() {
            self.decide("skipped: own change".to_string());
            return;
        }

        // The paste flag is consumed by the first judged change after the announcement; a change
        // inside a composition or a history step leaves it alone.
        let judged = !self.composing.get() && !self.history.get();
        let mut pasted = false;
        if judged {
            pasted = self.pasting.replace(false);
        }

        let Some(edit) = TextEdit::between(text, selection_start, selection_length, new_text, self.deleting.get())
        else {
            self.decide("no edit".to_string());
            return;
        };
        let word = edit.is_insertion() && adapter::is_word_insertion(&edit.inserted);
        let as_typed = self.tracker.borrow_mut().edited(&edit, word);

        // The same gate as one condition, spelled out so the decision can be named.
        if !judged {
            self.decide(
                if self.composing.get() { "skipped: composition in progress" } else { "skipped: history operation" }
                    .to_string(),
            );
            return;
        }
        if !self.capitalize_sentences.get() {
            self.decide("off".to_string());
            return;
        }
        if as_typed {
            self.decide("as typed: the override".to_string());
            return;
        }
        if !word {
            self.decide("not a word insertion".to_string());
            return;
        }
        if pasted {
            self.decide("pasted".to_string());
            return;
        }
        let mut pending = adapter::plan(text, selection_start, selection_length, new_text);
        // The plan is framed by the selection reported before the change. Should a real keystroke
        // report it already moved — the insertion then does not sit over it, and the plan is `None`
        // for that reason alone — the edit the text itself shows is the same insertion, anchored
        // where it happened.
        if pending.is_none() && (edit.start as isize != selection_start || edit.length as isize != selection_length) {
            pending = adapter::plan(text, edit.start as isize, edit.length as isize, new_text);
        }
        let decision = match &pending {
            Some(planned) => format!("plan: {} at {}", planned.capital, planned.position),
            None => format!(
                "declined \"{}\" at {} (selection {}+{}, length {}\u{2192}{})",
                edit.inserted,
                edit.start,
                selection_start,
                selection_length,
                utf16_len(text),
                utf16_len(new_text)
            ),
        };
        *self.pending.borrow_mut() = pending;
        self.decide(decision);
    }

    /// After a change: the buffer's text and selection now that it holds the change. The plan to
    /// apply — replace `[position, position + inserted)` with `replacement` and put the cursor after
    /// it — once [`smart_typing_adapter::applies`](super::smart_typing_adapter::applies) confirms the
    /// insertion landed where it was decided; the capital it produces becomes the tracked one (§3.4,
    /// §3.5). `None` otherwise. The plan is consumed either way.
    pub fn text_changing(
        &self,
        text: &str,
        selection_start: isize,
        selection_length: isize,
    ) -> Option<CapitalizationPlan> {
        let plan = self.pending.borrow_mut().take()?;
        if self.applying.get() || self.replacing.get() {
            self.decide("dropped: own change".to_string());
            return None;
        }
        if !adapter::applies(&plan, text, selection_start, selection_length) {
            self.decide(format!(
                "dropped: the insertion did not land as planned (selection {}+{}, text length {})",
                selection_start,
                selection_length,
                utf16_len(text)
            ));
            return None;
        }
        self.tracker.borrow_mut().produced(plan.position, &plan.capital);
        self.decide(format!("applied: {} at {}", plan.capital, plan.position));
        Some(plan)
    }

    /// The pane applies a plan a main-loop turn after [`TypingHooks::text_changing`] returned it,
    /// not inside the signal (md.win, 2026-09-27: an edit made inside WinUI's `TextChanging` reached
    /// the screen but not `TextBox.Text`; GTK forbids changing a buffer from inside its own
    /// `insert-text` handler outright, since it invalidates the iterators). Should something have
    /// come between — the letter is no longer where the plan put it, or the cursor is no longer
    /// right after it (a second key already typed on, which the capital's cursor placement would
    /// otherwise land in front of) — the capital is not made, and the tracking it started is
    /// forgotten.
    pub fn still_applies(
        &self,
        plan: &CapitalizationPlan,
        text: &str,
        selection_start: isize,
        selection_length: isize,
    ) -> bool {
        let units = Utf16Text::new(text);
        let inserted: Vec<u16> = plan.inserted.encode_utf16().collect();
        let end = plan.position + inserted.len();
        if end <= units.len()
            && units.units()[plan.position..end] == inserted[..]
            && selection_length == 0
            && selection_start == end as isize
        {
            return true;
        }
        {
            let mut tracker = self.tracker.borrow_mut();
            if tracker.capital_at() == Some(plan.position) {
                tracker.clear();
            }
        }
        self.decide(format!("dropped: the text moved on before {} at {}", plan.capital, plan.position));
        false
    }

    /// Where the cursor belongs after an Undo or Redo, or `None` to leave the buffer's selection
    /// alone. An undo that puts back a deletion re-selects what the deletion removed (RichEdit on
    /// Windows; GtkTextHistory too, which restores the selection it recorded with the deletion), so
    /// the restored letter is left *selected* and the next keystroke would replace it. The other
    /// ports leave the cursor after the letter (§3.5), so the selection is collapsed after it — only
    /// when it is exactly the one scalar the override is armed on, which is what an Undo or Redo of
    /// the capital leaves; any other history step keeps the selection it was given.
    pub fn caret_after_history(&self, text: &str, selection_start: isize, selection_length: isize) -> Option<usize> {
        let q = self.tracker.borrow().armed_at()?;
        if selection_start != q as isize || selection_length == 0 {
            return None;
        }
        let units = Utf16Text::new(text);
        if q >= units.len() {
            return None;
        }
        let scalar = adapter::scalar_len(units.units(), q);
        (selection_length == scalar as isize).then_some(q + scalar)
    }
}

/// One change to the buffer's text as a single replacement: `[start, start + length)` of the old
/// text became `inserted`. A deletion has an empty `inserted`; an insertion has `length` 0;
/// anything the widget does — typing over a selection, Backspace, Delete, Cut, a paste, a
/// composition update, Undo, Redo — is one of these. Offsets are UTF-16 units into the old text
/// (§0.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextEdit {
    pub start: usize,
    pub length: usize,
    pub inserted: String,
}

impl TextEdit {
    pub fn new(start: usize, length: usize, inserted: impl Into<String>) -> Self {
        TextEdit { start, length, inserted: inserted.into() }
    }

    /// The end of the replaced range in the old text (exclusive).
    pub fn end(&self) -> usize {
        self.start + self.length
    }

    /// The inserted text's length in UTF-16 units.
    pub fn inserted_len(&self) -> usize {
        utf16_len(&self.inserted)
    }

    /// What the edit does to the length of the text, and so to every offset after it.
    pub fn delta(&self) -> isize {
        self.inserted_len() as isize - self.length as isize
    }

    /// Something was put in — §3.4's "insertion", whether or not something was also taken out.
    pub fn is_insertion(&self) -> bool {
        !self.inserted.is_empty()
    }

    /// Something was taken out and nothing put in — §3.4's "deletion".
    pub fn is_deletion(&self) -> bool {
        self.inserted.is_empty() && self.length > 0
    }

    /// The one replacement that turns `old_text` into `new_text`, or `None` when they are equal.
    /// Anchored at the selection when the new text is the old with exactly the selection replaced —
    /// typing, a paste, Delete and Cut over a selection, a letter over a selection — which settles
    /// the ambiguity of an edit inside a run of identical units (`aa` → `aaa`) the way the widget
    /// made it. A shrink at a collapsed cursor — Backspace, Delete, and their Ctrl word forms — is
    /// anchored at the cursor on the side `direction` names, the units before it or after it, which
    /// settles the same ambiguity for a deletion (`II` → `I` is either unit); when both sides fit and
    /// no key was announced, Backspace is assumed, and when the announced side does not fit the
    /// text, the other is taken, since the text is what happened. Otherwise (Undo, Redo, a change
    /// made somewhere other than the selection) the smallest single replacement: the common prefix
    /// and suffix are kept, never splitting a surrogate pair.
    pub fn between(
        old_text: &str,
        selection_start: isize,
        selection_length: isize,
        new_text: &str,
        direction: DeleteDirection,
    ) -> Option<TextEdit> {
        if old_text == new_text {
            return None;
        }
        let old = Utf16Text::new(old_text);
        let new = Utf16Text::new(new_text);
        let (o, n) = (old.units(), new.units());
        let text = |range: &[u16]| String::from_utf16_lossy(range);

        if let Some((start, end)) = adapter::selection_in(o.len(), selection_start, selection_length) {
            let length = end - start;
            if let Some(inserted) = (n.len() + length).checked_sub(o.len())
                && n[..start] == o[..start]
                && n[start + inserted..] == o[end..]
            {
                return Some(TextEdit::new(start, length, text(&n[start..start + inserted])));
            }

            if length == 0 && o.len() > n.len() {
                let removed = o.len() - n.len();
                let before = start.checked_sub(removed).map(|s| TextEdit::new(s, removed, ""));
                let after = TextEdit::new(start, removed, "");
                let fits_before = before.as_ref().is_some_and(|b| is_removal(o, b, n));
                let fits_after = after.end() <= o.len() && is_removal(o, &after, n);
                if direction == DeleteDirection::Forward {
                    if fits_after {
                        return Some(after);
                    }
                    if fits_before {
                        return before;
                    }
                } else {
                    if fits_before {
                        return before;
                    }
                    if fits_after {
                        return Some(after);
                    }
                }
            }
        }

        let shorter = o.len().min(n.len());
        let mut prefix = 0;
        while prefix < shorter && o[prefix] == n[prefix] {
            prefix += 1;
        }
        // A high surrogate shared by both texts whose low half differs: the pair is the edit's.
        if prefix > 0 && prefix < o.len() && is_high(o[prefix - 1]) && is_low(o[prefix]) {
            prefix -= 1;
        }

        let mut suffix = 0;
        while suffix < shorter - prefix && o[o.len() - 1 - suffix] == n[n.len() - 1 - suffix] {
            suffix += 1;
        }
        // A low surrogate shared by both texts whose high half differs: likewise.
        if suffix > 0 && o.len() - suffix > prefix && is_low(o[o.len() - suffix]) && is_high(o[o.len() - suffix - 1]) {
            suffix -= 1;
        }

        Some(TextEdit::new(prefix, o.len() - prefix - suffix, text(&n[prefix..n.len() - suffix])))
    }
}

/// `new` is `old` with exactly `edit`'s range taken out.
fn is_removal(old: &[u16], edit: &TextEdit, new: &[u16]) -> bool {
    edit.start <= new.len() && new[..edit.start] == old[..edit.start] && new[edit.start..] == old[edit.end()..]
}

fn is_high(u: u16) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}

/// The side of a collapsed cursor a deletion key removes from: `Backward` is Backspace (the units
/// before the cursor), `Forward` is Delete (the units after it), and `Unknown` is no key announced
/// — a change the text has to settle by itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum DeleteDirection {
    #[default]
    Unknown,
    Backward,
    Forward,
}

/// The state of smart-typing.md §3.4, as the hooks drive it. There is one of three: the **tracked
/// capital** — `capital = p`, the unit offset of the last capital md produced, kept right through
/// every later edit until the edit that removes that scalar; the **armed override** at `q`, set by
/// that removal, under which a word insertion starting exactly at `q` goes in as typed; or nothing.
/// Every edit to the text passes through [`CapitalTracker::edited`], in the order the widget makes
/// them, before it is applied.
///
/// **Tracking (§3.4, first list).** An edit whose range ends at or before `p` — an insertion at `p`
/// included — shifts `p` by the edit's length delta. An edit that starts after the capital's last
/// unit leaves it alone. An edit that covers the capital removes it, unless the inserted text puts
/// that same scalar back at the same offset (a paste of `Md` over the selected `M`): then
/// `capital = nil` and the override is armed at the edit's start. Producing a new capital
/// ([`CapitalTracker::produced`]) replaces the tracked one: the old one is forgotten, not armed.
///
/// **Armed (§3.4, second list).** A word insertion that starts exactly at `q` goes in as typed and
/// clears the override; any insertion that starts elsewhere clears it; an insertion at `q` that is
/// not a word (a digit, a newline, a capital) leaves it armed. Deletions never clear it: one before
/// `q` shifts `q`, one covering `q` moves `q` to its start. The edit that removed the capital may
/// itself be a word insertion at `q` — the whole word `Md` selected and `md` typed, or the
/// one-scalar retype of §3.4's last rule — and then it goes in as typed too, and the override
/// *stays* armed (the rule says the retype arms it); a later word insertion at `q` spends it.
///
/// **Undo and Redo** are edits like any other here (the hooks bypass judging for them, not
/// tracking): the undo of a capital is the replacement `M` → `m` at `p`, which removes the capital
/// and arms the override there, so the restored lowercase letter can be deleted and retyped, or
/// typed on from — an insertion elsewhere clears. A redo of the capital is a non-word insertion at
/// `q`: it puts the text back and leaves the override armed; it does not make the capital tracked
/// again, because a redo restores text, not md's memory of what it wrote — the next capital
/// produced by typing is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CapitalTracker {
    capital_at: Option<usize>,
    capital: Option<String>,
    armed_at: Option<usize>,
}

impl CapitalTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// `p`: the offset of the tracked capital, or `None` while none is tracked.
    pub fn capital_at(&self) -> Option<usize> {
        self.capital_at
    }

    /// The tracked capital itself (one scalar, one or two units), or `None` while none is tracked.
    pub fn capital(&self) -> Option<&str> {
        self.capital.as_deref()
    }

    /// `q`: where the override is armed, or `None` while it is not.
    pub fn armed_at(&self) -> Option<usize> {
        self.armed_at
    }

    pub fn is_tracking(&self) -> bool {
        self.capital_at.is_some()
    }

    pub fn is_armed(&self) -> bool {
        self.armed_at.is_some()
    }

    /// md produced `capital` at `position`: it is the tracked one now, and whatever was tracked or
    /// armed before is forgotten.
    ///
    /// # Panics
    ///
    /// For an empty `capital` (md.win throws `ArgumentException`; its negative-position and null
    /// cases cannot be expressed here).
    pub fn produced(&mut self, position: usize, capital: &str) {
        assert!(!capital.is_empty(), "produced: empty capital");
        self.capital_at = Some(position);
        self.capital = Some(capital.to_string());
        self.armed_at = None;
    }

    /// An external replacement of the whole text, or the setting turned off: nothing to remember.
    pub fn clear(&mut self) {
        self.capital_at = None;
        self.capital = None;
        self.armed_at = None;
    }

    /// One edit, before it is applied, with `word_insertion` saying whether its inserted text is a
    /// word insertion (§3.3). Updates the state as the type's documentation describes and answers
    /// true when the insertion is to go in **as typed** — a word insertion starting at the armed
    /// `q` — which the hooks turn into "no plan".
    pub fn edited(&mut self, edit: &TextEdit, word_insertion: bool) -> bool {
        if edit.length == 0 && !edit.is_insertion() {
            return false; // nothing changed
        }

        let mut armed_by_this_edit = false;
        if let (Some(p), Some(capital)) = (self.capital_at, self.capital.as_deref()) {
            let capital_len = utf16_len(capital);
            if edit.end() <= p {
                self.capital_at = Some(shift(p, edit.delta()));
            } else if edit.start >= p + capital_len || puts_back(edit, p, capital) {
                // After the capital, or the same scalar put back where it was: still standing.
            } else {
                self.capital_at = None;
                self.capital = None;
                self.armed_at = Some(edit.start);
                armed_by_this_edit = true;
            }
        }

        let Some(q) = self.armed_at else { return false };

        if !edit.is_insertion() {
            if edit.end() <= q {
                self.armed_at = Some(shift(q, edit.delta()));
            } else if edit.start < q {
                self.armed_at = Some(edit.start);
            }
            return false;
        }

        if edit.start != q {
            self.armed_at = None;
            return false;
        }
        if !word_insertion {
            return false;
        }
        if !armed_by_this_edit {
            self.armed_at = None;
        }
        true
    }
}

/// `offset + delta` for an edit that ends at or before `offset`, which can never go below the
/// edit's own start.
fn shift(offset: usize, delta: isize) -> usize {
    offset.checked_add_signed(delta).expect("an edit before an offset never moves it below zero")
}

/// The edit covers `p` but its inserted text holds `capital` at that same offset.
fn puts_back(edit: &TextEdit, p: usize, capital: &str) -> bool {
    if edit.start > p {
        return false;
    }
    let at = p - edit.start;
    let inserted: Vec<u16> = edit.inserted.encode_utf16().collect();
    let capital: Vec<u16> = capital.encode_utf16().collect();
    inserted.len() >= at + capital.len() && inserted[at..at + capital.len()] == capital[..]
}
