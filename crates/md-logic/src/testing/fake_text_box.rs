//! Port of md.win `FakeTextBox` — the editor as the typing hooks see it.

use std::cell::RefCell;
use std::fmt;

use crate::event::Event;

#[derive(Clone)]
struct Unit {
    before: Vec<u16>,
    start: usize,
    length: usize,
    after: Vec<u16>,
    caret: usize,
}

#[derive(Default)]
struct State {
    text: Vec<u16>,
    selection_start: usize,
    selection_length: usize,
    undo: Vec<Unit>,
    redo: Vec<Unit>,
}

/// A text editor as [`TypingHooks`](crate::text::TypingHooks) sees it, in md.win's `TextBox`
/// model: text and selection (UTF-16 units, like every offset the hooks take), the three change
/// events in the widget's order ([`FakeTextBox::before_text_changing`] with the text about to be
/// held, then the change, then [`FakeTextBox::text_changing`] and [`FakeTextBox::text_changed`]),
/// an undo stack of one unit per change that a [`FakeTextBox::set_text`] clears, Undo / Redo that
/// replay a unit through the same events without recording a new one, Cut as the deletion it is,
/// and the [`FakeTextBox::paste`] event Ctrl+V raises. Every edit that is not a history replay
/// clears the redo stack, as a real one does — which is what a capital applied to a Redo destroys.
/// A programmatic [`FakeTextBox::set_selected_text`] is always its own undo unit (the GTK shell
/// gets the same by applying it in its own user action).
///
/// Every method takes `&self` and no borrow is held while an event is raised: handlers re-enter
/// the box (the pane applies a capital from inside `text_changing`).
///
/// md.win's box reports CR line breaks (WinUI stores U+000D); this one stores whatever it is given,
/// and the Linux tests give it LF, the `GtkTextBuffer`'s spelling.
#[derive(Default)]
pub struct FakeTextBox {
    state: RefCell<State>,
    before_text_changing: Event<str>,
    text_changing: Event<()>,
    text_changed: Event<()>,
    paste: Event<()>,
}

impl fmt::Debug for FakeTextBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("FakeTextBox")
            .field("text", &String::from_utf16_lossy(&state.text))
            .field("selection_start", &state.selection_start)
            .field("selection_length", &state.selection_length)
            .field("undo_units", &state.undo.len())
            .field("redo_units", &state.redo.len())
            .finish()
    }
}

impl FakeTextBox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The text (a surrogate half split off by an out-of-range edit would read as U+FFFD; no test
    /// makes one).
    pub fn text(&self) -> String {
        String::from_utf16_lossy(&self.state.borrow().text)
    }

    /// The text's length in UTF-16 units.
    pub fn len(&self) -> usize {
        self.state.borrow().text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.borrow().text.is_empty()
    }

    pub fn selection_start(&self) -> usize {
        self.state.borrow().selection_start
    }

    pub fn selection_length(&self) -> usize {
        self.state.borrow().selection_length
    }

    pub fn can_undo(&self) -> bool {
        !self.state.borrow().undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.state.borrow().redo.is_empty()
    }

    pub fn undo_units(&self) -> usize {
        self.state.borrow().undo.len()
    }

    pub fn redo_units(&self) -> usize {
        self.state.borrow().redo.len()
    }

    /// Args: the text the box is about to hold; the box itself still reports the old text and
    /// selection.
    pub fn before_text_changing(&self) -> &Event<str> {
        &self.before_text_changing
    }

    /// Raised synchronously once the box holds the new text and reports the cursor after the change.
    pub fn text_changing(&self) -> &Event<()> {
        &self.text_changing
    }

    pub fn text_changed(&self) -> &Event<()> {
        &self.text_changed
    }

    /// What Ctrl+V and the context menu raise before the clipboard text goes in.
    pub fn paste(&self) -> &Event<()> {
        &self.paste
    }

    /// Clamps the start into the text and the length into what follows it.
    pub fn select(&self, start: usize, length: usize) {
        let mut state = self.state.borrow_mut();
        let start = start.min(state.text.len());
        state.selection_start = start;
        state.selection_length = length.min(state.text.len() - start);
    }

    /// The user types: the selection is replaced and the cursor put after the insertion — one undo
    /// unit.
    pub fn type_text(&self, text: &str) {
        let (start, length) = self.selection();
        self.edit(start, length, text);
    }

    /// Deletes the selection, or the one scalar before the cursor — a surrogate pair as one.
    pub fn backspace(&self) {
        let (start, length) = self.selection();
        if length > 0 {
            self.edit(start, length, "");
        } else if start > 0 {
            let pair = {
                let text = &self.state.borrow().text;
                start >= 2 && is_low(text[start - 1]) && is_high(text[start - 2])
            };
            let length = if pair { 2 } else { 1 };
            self.edit(start - length, length, "");
        }
    }

    /// Ctrl+X and Edit ▸ Cut: the selection removed as one edit, through the same events a deletion
    /// raises. The clipboard is not modelled.
    pub fn cut(&self) {
        let (start, length) = self.selection();
        if length > 0 {
            self.edit(start, length, "");
        }
    }

    /// Deletes the selection, or the one scalar after the cursor.
    pub fn delete(&self) {
        let (start, length) = self.selection();
        if length > 0 {
            self.edit(start, length, "");
        } else if start < self.len() {
            let pair = {
                let text = &self.state.borrow().text;
                start + 1 < text.len() && is_high(text[start]) && is_low(text[start + 1])
            };
            self.edit(start, if pair { 2 } else { 1 }, "");
        }
    }

    pub fn selected_text(&self) -> String {
        let state = self.state.borrow();
        let end = state.selection_start + state.selection_length;
        String::from_utf16_lossy(&state.text[state.selection_start..end])
    }

    /// The pane's replacement path: an edit like any other — its own undo unit, and the redo stack
    /// is gone.
    pub fn set_selected_text(&self, value: &str) {
        let (start, length) = self.selection();
        self.edit(start, length, value);
    }

    /// The clipboard going in. Ctrl+V and the context menu raise [`FakeTextBox::paste`] first;
    /// whether a programmatic paste does is the caller's to say. An empty clipboard changes nothing.
    pub fn paste_text(&self, clipboard: &str, raises_paste_event: bool) {
        if raises_paste_event {
            self.paste.emit(&());
        }
        if !clipboard.is_empty() {
            self.type_text(clipboard);
        }
    }

    /// Restores the unit's text with the cursor where the replaced range ended — after restored
    /// text, not selecting it.
    pub fn undo(&self) {
        let unit = {
            let mut state = self.state.borrow_mut();
            let Some(unit) = state.undo.pop() else { return };
            state.redo.push(unit.clone());
            unit
        };
        self.change(unit.before, unit.start + unit.length, 0);
    }

    pub fn redo(&self) {
        let unit = {
            let mut state = self.state.borrow_mut();
            let Some(unit) = state.redo.pop() else { return };
            state.undo.push(unit.clone());
            unit
        };
        self.change(unit.after, unit.caret, 0);
    }

    pub fn clear_undo_redo_history(&self) {
        let mut state = self.state.borrow_mut();
        state.undo.clear();
        state.redo.clear();
    }

    /// The whole text replaced, the undo history cleared, the selection clamped (WinUI's
    /// `Text = value`).
    pub fn set_text(&self, value: &str) {
        self.clear_undo_redo_history();
        let value: Vec<u16> = value.encode_utf16().collect();
        let (start, length) = self.selection();
        let start = start.min(value.len());
        let length = length.min(value.len() - start);
        self.change(value, start, length);
    }

    fn selection(&self) -> (usize, usize) {
        let state = self.state.borrow();
        (state.selection_start, state.selection_length)
    }

    fn edit(&self, start: usize, length: usize, replacement: &str) {
        let replacement: Vec<u16> = replacement.encode_utf16().collect();
        let (after, caret) = {
            let mut state = self.state.borrow_mut();
            let after = [&state.text[..start], &replacement[..], &state.text[start + length..]].concat();
            let caret = start + replacement.len();
            let before = state.text.clone();
            state.undo.push(Unit { before, start, length, after: after.clone(), caret });
            state.redo.clear();
            (after, caret)
        };
        self.change(after, caret, 0);
    }

    fn change(&self, new_text: Vec<u16>, start: usize, length: usize) {
        self.before_text_changing.emit(&String::from_utf16_lossy(&new_text));
        {
            let mut state = self.state.borrow_mut();
            state.text = new_text;
            state.selection_start = start;
            state.selection_length = length;
        }
        self.text_changing.emit(&());
        self.text_changed.emit(&());
    }
}

fn is_high(u: u16) -> bool {
    (0xD800..=0xDBFF).contains(&u)
}

fn is_low(u: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&u)
}
