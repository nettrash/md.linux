//! The typing glue's pure half (shell-design §3.3): how GTK's buffer signals become the calls
//! md-logic's `TypingHooks` and `FormattingAdapter` expect, extracted from the GTK code so it is
//! unit-tested without a display. `EditorPane` owns one [`UserActionTracker`] and does the GTK
//! half — reading the buffer, asking the hooks, applying their answers.
//!
//! **Boundaries.** Every interactive edit of a `GtkTextBuffer` — a typed character, Backspace,
//! Delete, Cut, a paste, a drop — runs inside a *user action* (`begin-user-action` …
//! `end-user-action`, reported for the outermost pair only). The tracker takes the buffer's text
//! and selection when the outermost action begins and hands them back when it ends, which is
//! exactly md.win's `BeforeTextChanging` (old text, old selection, new text): one change, judged
//! once, whatever the buffer did inside.
//!
//! **GTK splits typing over a selection.** `gtk_text_view_commit_text` (GTK 4.22) ends the user
//! action after deleting a selection longer than one character and begins a second one for the
//! insertion — "so that there are two undo operations". The Mac and md.win make the replacement
//! *one* undo step, and md-logic's contract wants it as one replacement (that is what the retype
//! rule and the typed wrap are judged on). So a user action that removed exactly the selection it
//! began with is **held**, not judged, and the pane *bridges* it: still inside `end-user-action` —
//! before GtkTextHistory closes the action's group — it begins an outermost user action of its
//! own, which adopts the held snapshot ([`UserActionTracker::begin`]) and keeps the history group
//! open. GTK's insertion then lands in it as an inner action, and the pane ends the bridge on the
//! next high-priority idle: one group, one undo step, one replacement for the hooks. A deletion
//! nothing follows (Delete, Cut, Backspace over a selection) ends the same way, alone; an adopted
//! action is never held twice.
//!
//! **Deletion side.** A collapsed caret and the first `delete-range` inside the action tell which
//! side a key removed — the range ending at the caret is Backspace, starting there is Delete —
//! which md.win had to guess from the key.
//!
//! **Own edits and history.** The pane's own replacements (the capital, the wrap, the Enter edit,
//! a Format command, Replace) run with [`UserActionTracker::own_begin`] up; undo and redo (which
//! GtkTextHistory applies with plain inserts and deletes, outside any user action) with
//! [`UserActionTracker::set_history`]. Neither is judged; the pane reports both itself.

use md_core::text::utf16::utf16_len;
use md_logic::text::DeleteDirection;

use super::offsets;

/// The buffer as a change found it: its text and selection, in UTF-16 units, plus the collapsed
/// caret in scalars (the unit `delete-range` reports), `None` when there is a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub text: String,
    pub start: usize,
    pub length: usize,
    pub caret_char: Option<usize>,
}

impl Snapshot {
    /// From the buffer's text and its two marks (scalar offsets, either order).
    pub fn new(text: String, insert: usize, bound: usize) -> Snapshot {
        let (start, length) = offsets::selection_utf16(&text, insert, bound);
        let caret_char = (insert == bound).then_some(insert);
        Snapshot { text, start, length, caret_char }
    }

    /// `text` is this snapshot's text with exactly its (non-empty) selection taken out.
    fn lost_exactly_the_selection(&self, text: &str) -> bool {
        if self.length == 0 {
            return false;
        }
        let units: Vec<u16> = self.text.encode_utf16().collect();
        let now: Vec<u16> = text.encode_utf16().collect();
        let end = self.start + self.length;
        end <= units.len()
            && now.len() + self.length == units.len()
            && now[..self.start] == units[..self.start]
            && now[self.start..] == units[end..]
    }
}

/// What the end of the outermost user action asks the pane to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// An inner `end_user_action`, or a user action nobody watched: nothing.
    Nothing,
    /// The pane's own edit ended: report the text (the session must see it), judge nothing.
    OwnEdit,
    /// The first half of GTK's typing over a selection: the pane begins its bridge now, which
    /// adopts it.
    Held,
    /// One change to judge: `before` against the buffer as it stands now.
    Judge { before: Snapshot, direction: DeleteDirection },
}

/// The sequencing of §3.3 as a state machine over the buffer's signals (see the module comment).
#[derive(Debug, Default)]
pub struct UserActionTracker {
    depth: usize,
    own: usize,
    history: bool,
    base: Option<Snapshot>,
    held: Option<Snapshot>,
    /// The outermost action open now adopted a held snapshot: whatever it ends with is judged.
    adopted: bool,
    direction: DeleteDirection,
    saw_delete: bool,
}

impl UserActionTracker {
    pub fn new() -> UserActionTracker {
        UserActionTracker::default()
    }

    /// Inside a user action (GTK reports only the outermost pair; the tracker counts them anyway).
    #[cfg(test)]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The pane's own edit is being applied.
    #[cfg(test)]
    pub fn is_own(&self) -> bool {
        self.own > 0
    }

    /// A selection deletion waits for the insertion that may complete it.
    #[cfg(test)]
    pub fn is_holding(&self) -> bool {
        self.held.is_some()
    }

    /// The pane starts one of its own edits. Nested starts are counted.
    pub fn own_begin(&mut self) {
        self.own += 1;
    }

    pub fn own_end(&mut self) {
        self.own = self.own.saturating_sub(1);
    }

    /// Undo / redo: up from the buffer's `undo` signal until its default handler has run.
    pub fn set_history(&mut self, history: bool) {
        self.history = history;
    }

    /// `begin-user-action`: the outermost one takes the snapshot (`take` is called only then, and
    /// only when the change will be judged) — or adopts a held selection deletion, which makes
    /// this action (the pane's bridge) the whole replacement.
    pub fn begin(&mut self, take: impl FnOnce() -> Snapshot) {
        self.depth += 1;
        if self.depth > 1 {
            return;
        }
        self.direction = DeleteDirection::Unknown;
        self.saw_delete = false;
        if self.own > 0 || self.history {
            self.base = None;
            return;
        }
        self.adopted = self.held.is_some();
        self.base = Some(match self.held.take() {
            Some(held) => held,
            None => take(),
        });
    }

    /// `delete-range` (before the default handler), in scalars. The first deletion of an action
    /// that began at a collapsed caret names the side the key removed from.
    pub fn deleted(&mut self, start: usize, end: usize) {
        if self.depth == 0 || self.saw_delete {
            return;
        }
        self.saw_delete = true;
        let Some(caret) = self.base.as_ref().and_then(|b| b.caret_char) else { return };
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        self.direction = if end == caret {
            DeleteDirection::Backward
        } else if start == caret {
            DeleteDirection::Forward
        } else {
            DeleteDirection::Unknown
        };
    }

    /// `end-user-action`, with the buffer's text as it now stands.
    pub fn end(&mut self, text: &str) -> Ended {
        if self.depth == 0 {
            return Ended::Nothing;
        }
        self.depth -= 1;
        if self.depth > 0 {
            return Ended::Nothing;
        }
        if self.own > 0 {
            return Ended::OwnEdit;
        }
        let adopted = std::mem::take(&mut self.adopted);
        let Some(before) = self.base.take() else { return Ended::Nothing };
        if !adopted && before.lost_exactly_the_selection(text) {
            self.held = Some(before);
            return Ended::Held;
        }
        Ended::Judge { before, direction: self.direction }
    }

    /// A buffer `changed` outside any user action and not ours: the session must still see it.
    pub fn reports_stray_change(&self) -> bool {
        self.depth == 0 && self.own == 0 && !self.history
    }
}

/// What a Return key press is to the editor (shell-design §3.3, "Enter").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterAction {
    /// Not Return / KP_Enter / ISO_Enter.
    NotEnter,
    /// Enter with no modifier: ask the hooks; their edit, or GTK's own newline.
    Smart,
    /// Shift+Enter or Ctrl+Enter: a plain newline, always — the Mac's Control-Return parity.
    Plain,
    /// Shift+Ctrl+Enter (Zen Mode's chord, which the window's accelerators take in the capture
    /// phase before the view sees it) and anything with Alt: never the editor's to handle.
    Pass,
}

/// Classifies a key press by GDK keyval name and the three modifiers md's chords use. While an
/// input method composes, every key is the input method's ([`EnterAction::Pass`]).
pub fn enter_action(keyval_name: &str, shift: bool, control: bool, alt: bool, composing: bool) -> EnterAction {
    if !matches!(keyval_name, "Return" | "KP_Enter" | "ISO_Enter") {
        return EnterAction::NotEnter;
    }
    if composing || alt || (shift && control) {
        return EnterAction::Pass;
    }
    if shift || control { EnterAction::Plain } else { EnterAction::Smart }
}

/// The edit as the buffer should make it so GtkTextHistory keeps it as **its own undo step**.
///
/// GtkTextHistory (GTK 4.22, `gtk_text_history_end_user_action`) closes a user action that made
/// two or more changes with a barrier, so nothing coalesces into it — the capital's and the wrap's
/// delete + insert pairs are separate steps by construction. A user action that made exactly one
/// change is *hoisted* out of its group, and a lone insertion then chains onto an insertion that
/// ended where it begins, as typing does: "word" typed and a Format command inserting `****` at the
/// caret would undo together. An insertion (`length == 0`) is therefore widened to replace the
/// scalar before it (or, at the start of the text, the one after it) with that scalar plus the
/// insertion — the same text, but a delete and an insert, so it is closed off like the others.
/// Anything else, and an insertion into an empty buffer (nothing can chain there), is unchanged.
/// Offsets and lengths are UTF-16 units.
pub fn undo_unit_edit(text: &str, start: usize, length: usize, replacement: &str) -> (usize, usize, String) {
    let (start, length) = offsets::clamp_range(text, start, length);
    if length > 0 || replacement.is_empty() || text.is_empty() {
        return (start, length, replacement.to_string());
    }
    let at = offsets::char_of_utf16(text, start);
    let start = offsets::utf16_of_char(text, at);
    if at > 0 {
        let before = text.chars().nth(at - 1).map(String::from).unwrap_or_default();
        let units = utf16_len(&before);
        return (start - units, units, format!("{before}{replacement}"));
    }
    let after = text.chars().next().map(String::from).unwrap_or_default();
    (start, utf16_len(&after), format!("{replacement}{after}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(text: &str, insert: usize, bound: usize) -> Snapshot {
        Snapshot::new(text.to_string(), insert, bound)
    }

    #[test]
    fn a_typed_character_is_one_change_judged_against_the_text_before_it() {
        let mut t = UserActionTracker::new();
        t.begin(|| snap("ab", 2, 2));
        assert_eq!(t.depth(), 1);
        assert_eq!(t.end("abc"), Ended::Judge { before: snap("ab", 2, 2), direction: DeleteDirection::Unknown });
        assert_eq!(t.depth(), 0);
    }

    #[test]
    fn nested_actions_take_one_snapshot_and_end_once() {
        let mut t = UserActionTracker::new();
        let mut takes = 0;
        t.begin(|| {
            takes += 1;
            snap("x", 1, 1)
        });
        t.begin(|| unreachable!("an inner begin never snapshots"));
        assert_eq!(t.end("xy"), Ended::Nothing);
        assert!(matches!(t.end("xyz"), Ended::Judge { .. }));
        assert_eq!(takes, 1);
        assert_eq!(t.end("xyz"), Ended::Nothing, "an unbalanced end is ignored");
    }

    #[test]
    fn the_deletion_side_comes_from_the_first_range_at_a_collapsed_caret() {
        let mut t = UserActionTracker::new();
        t.begin(|| snap("II", 1, 1));
        t.deleted(0, 1);
        assert_eq!(t.end("I"), Ended::Judge { before: snap("II", 1, 1), direction: DeleteDirection::Backward });

        t.begin(|| snap("II", 1, 1));
        t.deleted(2, 1); // reversed iters are ordered
        t.deleted(0, 1); // only the first counts
        assert_eq!(t.end("I"), Ended::Judge { before: snap("II", 1, 1), direction: DeleteDirection::Forward });

        t.begin(|| snap("abc", 3, 0));
        t.deleted(0, 3); // a selection: no side
        t.begin(|| unreachable!());
        assert_eq!(t.end(""), Ended::Nothing);
        assert_eq!(t.end("x"), Ended::Judge { before: snap("abc", 3, 0), direction: DeleteDirection::Unknown });
    }

    #[test]
    fn gtk_typing_over_a_selection_reaches_the_hooks_as_one_replacement() {
        let mut t = UserActionTracker::new();
        // "word" selected, `*` typed: GTK's first action deletes the selection …
        t.begin(|| snap("a word b", 2, 6));
        t.deleted(2, 6);
        assert_eq!(t.end("a  b"), Ended::Held);
        assert!(t.is_holding());
        // … the pane's bridge begins at once and adopts it; GTK's insertion is an inner action …
        t.begin(|| unreachable!("the held snapshot is adopted"));
        assert!(!t.is_holding());
        t.begin(|| unreachable!("an inner begin never snapshots"));
        assert_eq!(t.end("a * b"), Ended::Nothing);
        // … and the bridge's end judges the whole replacement.
        assert_eq!(
            t.end("a * b"),
            Ended::Judge { before: snap("a word b", 2, 6), direction: DeleteDirection::Unknown }
        );
    }

    #[test]
    fn a_deletion_of_the_selection_alone_is_judged_when_the_bridge_ends() {
        let mut t = UserActionTracker::new();
        t.begin(|| snap("one two", 4, 7));
        assert_eq!(t.end("one "), Ended::Held);
        t.begin(|| unreachable!("the bridge adopts it"));
        // Nothing was inserted: still exactly the selection gone, but an adopted action is judged.
        assert_eq!(t.end("one "), Ended::Judge { before: snap("one two", 4, 7), direction: DeleteDirection::Unknown });
        assert!(!t.is_holding());
        // The next action is an ordinary one again.
        t.begin(|| snap("one ", 4, 4));
        assert!(matches!(t.end("one x"), Ended::Judge { .. }));
    }

    #[test]
    fn only_the_exact_selection_is_held() {
        let mut t = UserActionTracker::new();
        t.begin(|| snap("abcd", 1, 3));
        assert!(matches!(t.end("ad"), Ended::Held));
        t.begin(|| unreachable!());
        assert!(matches!(t.end("ad"), Ended::Judge { .. }));
        // A one-character overwrite is one GTK action already: the text never shows the gap.
        t.begin(|| snap("abcd", 1, 2));
        assert!(matches!(t.end("aXcd"), Ended::Judge { .. }));
        // A different deletion than the selection is judged at once.
        t.begin(|| snap("abcd", 1, 3));
        assert!(matches!(t.end("abc"), Ended::Judge { .. }));
        // A collapsed caret never holds.
        t.begin(|| snap("abcd", 2, 2));
        assert!(matches!(t.end("acd"), Ended::Judge { .. }));
    }

    #[test]
    fn own_edits_and_history_are_reported_but_never_judged() {
        let mut t = UserActionTracker::new();
        t.own_begin();
        t.begin(|| unreachable!("an own edit takes no snapshot"));
        t.deleted(0, 1);
        assert_eq!(t.end("M"), Ended::OwnEdit);
        assert!(!t.reports_stray_change());
        t.own_end();
        assert!(t.reports_stray_change());

        t.set_history(true);
        assert!(!t.reports_stray_change());
        t.begin(|| unreachable!());
        assert_eq!(t.end("m"), Ended::Nothing);
        t.set_history(false);
        assert!(t.reports_stray_change());
        t.own_end(); // saturates
        assert!(!t.is_own());
    }

    #[test]
    fn a_change_inside_an_action_is_not_a_stray_one() {
        let mut t = UserActionTracker::new();
        t.begin(|| snap("", 0, 0));
        assert!(!t.reports_stray_change());
        t.end("a");
        assert!(t.reports_stray_change());
    }

    #[test]
    fn snapshots_measure_in_utf16_and_note_the_caret_in_scalars() {
        let s = snap("a😀b", 3, 3);
        assert_eq!((s.start, s.length, s.caret_char), (4, 0, Some(3)));
        let s = snap("a😀b", 2, 1);
        assert_eq!((s.start, s.length, s.caret_char), (1, 2, None));
    }

    #[test]
    fn enter_is_classified_by_its_modifiers() {
        for key in ["Return", "KP_Enter", "ISO_Enter"] {
            assert_eq!(enter_action(key, false, false, false, false), EnterAction::Smart);
            assert_eq!(enter_action(key, true, false, false, false), EnterAction::Plain);
            assert_eq!(enter_action(key, false, true, false, false), EnterAction::Plain);
            assert_eq!(enter_action(key, true, true, false, false), EnterAction::Pass);
            assert_eq!(enter_action(key, false, false, true, false), EnterAction::Pass);
            assert_eq!(enter_action(key, false, false, false, true), EnterAction::Pass);
        }
        assert_eq!(enter_action("Tab", false, false, false, false), EnterAction::NotEnter);
        assert_eq!(enter_action("a", false, false, false, false), EnterAction::NotEnter);
    }

    #[test]
    fn an_insertion_becomes_a_replacement_of_its_neighbour() {
        assert_eq!(undo_unit_edit("word", 4, 0, "****"), (3, 1, "d****".to_string()));
        assert_eq!(undo_unit_edit("word", 0, 0, "# "), (0, 1, "# w".to_string()));
        assert_eq!(undo_unit_edit("a😀", 3, 0, "**"), (1, 2, "😀**".to_string()));
        assert_eq!(undo_unit_edit("😀b", 0, 0, "x"), (0, 2, "x😀".to_string()));
        // Inside a pair, the insertion point rounds down to the pair's start.
        assert_eq!(undo_unit_edit("😀b", 1, 0, "x"), (0, 2, "x😀".to_string()));
    }

    #[test]
    fn replacements_deletions_and_an_empty_buffer_are_left_alone() {
        assert_eq!(undo_unit_edit("word", 0, 4, "**word**"), (0, 4, "**word**".to_string()));
        assert_eq!(undo_unit_edit("word", 1, 2, ""), (1, 2, String::new()));
        assert_eq!(undo_unit_edit("word", 2, 0, ""), (2, 0, String::new()));
        assert_eq!(undo_unit_edit("", 0, 0, "****"), (0, 0, "****".to_string()));
        // Out-of-range input is clamped first: an insertion at the end, widened backwards.
        assert_eq!(undo_unit_edit("ab", 9, 9, "x"), (1, 1, "bx".to_string()));
    }
}
