//! `EditorPane` — the source editor (shell-design §3.1): a `gtk::TextView` (class `md-editor`) in a
//! `gtk::ScrolledWindow` under an overlay placeholder, LF text only, UTF-16 offsets at its surface,
//! md-logic's typing hooks fed at the buffer's user-action boundaries, caret jumps, the editor half
//! of the scroll sync.
//!
//! Every decision is md-logic's: `TypingHooks` (the capital, the override, Enter, the flags),
//! `FormattingAdapter` (Format commands and the typed wrap), `EditorText`, `LineOffsets`,
//! `EditorJumpTracker`, `ScrollSync` / `ScrollSyncGuard`. This file reads the buffer at GTK's
//! natural boundaries, asks, and applies the answer; [`super::typing`] holds the sequencing as a
//! pure, tested state machine, and [`super::offsets`] the unit conversions.
//!
//! **How a keystroke flows.** `begin-user-action` snapshots the text and selection;
//! `delete-range` names the side a collapsed caret lost; `end-user-action` hands the snapshot and
//! the new text to `FormattingAdapter::wrap` and `TypingHooks::before_text_changing` /
//! `text_changing` as md.win's `BeforeTextChanging` / `TextChanging` pair. A capital or a wrap is
//! applied on a `Priority::HIGH` idle — after GtkTextHistory has closed the keystroke's own group
//! and before the next input event — as its own user action under `TypingHooks::applying` /
//! `external_edit`, so it is its own undo step (asserted by the self-test, §16.3). GTK's typing
//! over a selection arrives as two user actions; the pane bridges them into one — one undo step,
//! as on the Mac — and [`super::typing::UserActionTracker`] judges them as one replacement.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::prelude::*;
use md_core::editing::markdown_formatting::FormatEdit;
use md_core::text::utf16::utf16_len;
use md_logic::commands::CommandId;
use md_logic::documents::{editor_text, line_offsets};
use md_logic::event::{Event, HandlerId};
use md_logic::preview::{EditorJump, EditorJumpTracker};
use md_logic::seams::{Scheduler, TimerHandle};
use md_logic::settings::{SettingsStore, TypingSettings, settings_keys, wrap_selection_setting};
use md_logic::text::{CapitalizationPlan, DeleteDirection, TypingHooks, formatting_adapter};
use md_logic::view::{ScrollSync, ScrollSyncGuard};

use super::offsets;
use super::typing::{self, Ended, EnterAction, Snapshot, UserActionTracker};
use crate::services::glib_seams::{GlibScheduler, SystemClock};

/// The editor's inset on every side, the Mac's `textContainerInset` (shell-design §3.1).
pub const INSET: i32 = 16;

/// How long an announced paste may wait for the clipboard before its flag is dropped. GTK reads
/// the clipboard asynchronously and emits `paste-done` only when something arrived; an empty
/// clipboard emits nothing, and the flag must not swallow the capital of the next word typed.
const PASTE_SETTLE: Duration = Duration::from_secs(1);

/// How long after the buffer gave up the PRIMARY selection a new selection waits before it claims
/// it again (see `select_in`).
const PRIMARY_RECLAIM: Duration = Duration::from_millis(3);

/// The editor of a document window or of the Book window's article.
pub struct EditorPane {
    me: Weak<EditorPane>,
    view: gtk::TextView,
    buffer: gtk::TextBuffer,
    scrolled: gtk::ScrolledWindow,
    root: gtk::Overlay,
    placeholder: gtk::Label,
    hooks: TypingHooks,
    tracker: RefCell<UserActionTracker>,
    wrap_selection: Cell<bool>,
    settings: RefCell<Option<(Rc<dyn SettingsStore>, HandlerId)>>,
    jumps: RefCell<EditorJumpTracker>,
    /// Undo / redo in flight: the snapshot its `undo` / `redo` signal found.
    history_before: RefCell<Option<Snapshot>>,
    /// Up while `end-user-action` is being handled: GtkTextHistory's group is still open then, so
    /// an irreversible action (`set_text`, `reset_history`) must wait for the next turn.
    in_end: Cell<bool>,
    /// Up while the `\r` filter re-inserts normalised text.
    normalizing: Cell<bool>,
    /// The pane's own outermost user action is open around GTK's typing over a selection (see
    /// [`EditorPane::bridge`]); it ends on the next high-priority idle.
    bridging: Cell<bool>,
    /// Counts the external texts applied: a `set_text` deferred out of `end-user-action` is
    /// dropped when a newer one landed first (an echo of the old text must not overwrite it).
    text_serial: Cell<u64>,
    paste_settle: RefCell<Option<TimerHandle>>,
    last_selection: Cell<(i32, i32)>,
    /// When the selection last became empty — the moment the buffer released PRIMARY.
    released_at: Cell<Option<Instant>>,
    /// Bumped by every selection the pane makes; a deferred one applies only if still the latest.
    select_generation: Cell<u64>,
    /// Bumped by every key press in the view, so a deferred selection never lands after typing.
    keys_pressed: Cell<u64>,
    scroll_sync: RefCell<Option<Rc<ScrollSync>>>,
    guard: ScrollSyncGuard,
    text_edited: Event<str>,
    selection_changed: Event<()>,
    history_changed: Event<()>,
    scrolled_event: Event<f64>,
}

impl EditorPane {
    pub fn new() -> Rc<EditorPane> {
        let buffer = gtk::TextBuffer::new(None);
        buffer.set_enable_undo(true);
        let view = gtk::TextView::builder()
            .buffer(&buffer)
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(true)
            .top_margin(INSET)
            .bottom_margin(INSET)
            .left_margin(INSET)
            .right_margin(INSET)
            // md owns capitalisation and never corrects (smart-typing.md §3.2): no spell checking
            // and never UPPERCASE_SENTENCES from an on-screen keyboard or input method.
            .input_hints(gtk::InputHints::NO_SPELLCHECK)
            .input_purpose(gtk::InputPurpose::FreeForm)
            .hexpand(true)
            .vexpand(true)
            .build();
        view.add_css_class("md-editor");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .build();
        // GTK 4.22's TextView has no placeholder; the Mac draws an overlay too.
        let placeholder = gtk::Label::builder()
            .label(md_logic::strings::EDITOR_PLACEHOLDER)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_top(INSET)
            .margin_start(INSET)
            .can_target(false)
            .can_focus(false)
            .build();
        placeholder.add_css_class("md-placeholder");
        let root = gtk::Overlay::builder().child(&scrolled).hexpand(true).vexpand(true).build();
        root.add_overlay(&placeholder);

        let pane = Rc::new_cyclic(|me| EditorPane {
            me: me.clone(),
            view,
            buffer,
            scrolled,
            root,
            placeholder,
            hooks: TypingHooks::new(),
            tracker: RefCell::new(UserActionTracker::new()),
            wrap_selection: Cell::new(settings_keys::WRAP_SELECTION_DEFAULT),
            settings: RefCell::new(None),
            jumps: RefCell::new(EditorJumpTracker::new()),
            history_before: RefCell::new(None),
            in_end: Cell::new(false),
            normalizing: Cell::new(false),
            bridging: Cell::new(false),
            text_serial: Cell::new(0),
            paste_settle: RefCell::new(None),
            last_selection: Cell::new((0, 0)),
            released_at: Cell::new(None),
            select_generation: Cell::new(0),
            keys_pressed: Cell::new(0),
            scroll_sync: RefCell::new(None),
            guard: ScrollSyncGuard::new(Rc::new(SystemClock)),
            text_edited: Event::new(),
            selection_changed: Event::new(),
            history_changed: Event::new(),
            scrolled_event: Event::new(),
        });
        pane.wire();
        pane
    }

    /// The overlay holding the scrolled editor and its placeholder.
    pub fn widget(&self) -> gtk::Widget {
        self.root.clone().upcast()
    }

    /// The `GtkTextView` itself — for focus checks, and for the self-test.
    #[cfg(feature = "selftest")]
    pub fn text_view(&self) -> gtk::TextView {
        self.view.clone()
    }

    /// The placeholder label (visible only while the buffer is empty).
    #[cfg(feature = "selftest")]
    pub fn placeholder(&self) -> gtk::Label {
        self.placeholder.clone()
    }

    /// The typing hooks, for the self-test's report (`TypingHooks::trace`).
    #[cfg(feature = "selftest")]
    pub fn typing_hooks(&self) -> &TypingHooks {
        &self.hooks
    }

    /// The buffer's text (LF only).
    pub fn text(&self) -> String {
        self.buffer.text(&self.buffer.start_iter(), &self.buffer.end_iter(), true).to_string()
    }

    /// External text (open, Revert, Reload, an example, an article switch): history cleared,
    /// selection clamped. The session's echo of identical text changes nothing.
    pub fn set_text(&self, text: &str) {
        if self.in_end.get() {
            // GtkTextHistory's group is still open; an irreversible action now would be refused.
            // A newer text applied before the idle wins.
            let me = self.me.clone();
            let text = text.to_string();
            let serial = self.text_serial.get();
            glib::idle_add_local_full(glib::Priority::HIGH, move || {
                if let Some(pane) = me.upgrade()
                    && pane.text_serial.get() == serial
                {
                    pane.set_text(&text);
                }
                glib::ControlFlow::Break
            });
            return;
        }
        // An open bridge closes first: its replacement is judged against the text it was typed in.
        self.flush_held();
        if !self.hooks.external_text(&self.text(), text) {
            return;
        }
        self.text_serial.set(self.text_serial.get() + 1);
        let (start, length) = self.selection_utf16();
        self.buffer.begin_irreversible_action();
        self.hooks.replacing(|| self.buffer.set_text(editor_text::to_editor(text)));
        self.buffer.end_irreversible_action();
        let now = self.text();
        let (start, length) = editor_text::clamp_selection(start as i64, length as i64, utf16_len(&now));
        self.select_in(&now, start, length);
        self.update_placeholder();
    }

    /// The session's `undo_generation` changed: the hooks reset and an empty irreversible action
    /// (never the buffer's history alone — md.win §3.6).
    pub fn reset_history(&self) {
        if self.in_end.get() {
            let me = self.me.clone();
            glib::idle_add_local_full(glib::Priority::HIGH, move || {
                if let Some(pane) = me.upgrade() {
                    pane.reset_history();
                }
                glib::ControlFlow::Break
            });
            return;
        }
        self.flush_held();
        self.hooks.reset();
        self.buffer.begin_irreversible_action();
        self.buffer.end_irreversible_action();
    }

    /// (start, length) in UTF-16 units.
    pub fn selection_utf16(&self) -> (usize, usize) {
        let text = self.text();
        let (insert, bound) = self.marks();
        offsets::selection_utf16(&text, insert, bound)
    }

    /// Selects `[start, start + length)` (UTF-16, clamped) and scrolls it into view; the focus is
    /// left where it is.
    pub fn select_utf16(&self, start: usize, length: usize) {
        let text = self.text();
        self.select_in(&text, start, length);
        self.scroll_to_selection();
    }

    /// The find bar's hit from the editor (Ctrl+G, Replace All): selected, scrolled to, and the
    /// editor focused.
    pub fn select_range(&self, start: usize, length: usize) {
        self.select_utf16(start, length);
        self.focus();
    }

    /// The selected text ("" for a caret).
    pub fn selected_text(&self) -> String {
        match self.buffer.selection_bounds() {
            Some((start, end)) => self.buffer.text(&start, &end, true).to_string(),
            None => String::new(),
        }
    }

    pub fn has_selection(&self) -> bool {
        self.buffer.has_selection()
    }

    pub fn can_undo(&self) -> bool {
        self.buffer.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.buffer.can_redo()
    }

    /// Edit ▸ Undo (and Ctrl+Z from the window's accelerator): the buffer's `undo` signal does the
    /// rest, exactly as for GtkTextView's own binding.
    pub fn undo(&self) {
        self.buffer.undo();
    }

    pub fn redo(&self) {
        self.buffer.redo();
    }

    pub fn cut(&self) {
        self.buffer.cut_clipboard(&self.view.clipboard(), self.view.is_editable());
    }

    pub fn copy(&self) {
        self.buffer.copy_clipboard(&self.view.clipboard());
    }

    /// Edit ▸ Paste: announced here, since the buffer's paste does not raise the view's
    /// `paste-clipboard` (Ctrl+V and the context menu do).
    pub fn paste(&self) {
        self.announce_paste();
        self.buffer.paste_clipboard(&self.view.clipboard(), None, self.view.is_editable());
    }

    /// Edit ▸ Delete.
    pub fn delete_selection(&self) {
        self.buffer.delete_selection(true, self.view.is_editable());
    }

    pub fn select_all(&self) {
        self.buffer.select_range(&self.buffer.start_iter(), &self.buffer.end_iter());
    }

    /// A Format menu row or chord: one user action, its own undo step, the selection the edit
    /// names, focus back in the editor (md.win `ApplyFormat`). A command that makes no edit does
    /// nothing.
    pub fn apply_format(&self, command: CommandId) {
        if let Some(op) = formatting_adapter::op_for(command) {
            self.flush_held();
            let text = self.text();
            let (start, length) = self.selection_utf16();
            if let Some(edit) = formatting_adapter::edit(&text, start as isize, length as isize, op) {
                self.apply_format_edit(&edit);
            }
        }
        self.focus();
    }

    /// Performed once per jump id: the caret to the start of the line, scrolled into view, the
    /// editor focused. GTK defers the scroll until the lines are laid out, which covers a Notes
    /// jump that just switched Preview → Edit. The window then calls `editor_jump_handled(id)`.
    pub fn apply_jump(&self, jump: EditorJump) {
        if !self.jumps.borrow_mut().claim(Some(&jump)) {
            return;
        }
        let text = self.text();
        let offset = line_offsets::offset_of_line(jump.line, &text);
        let at = self.buffer.iter_at_offset(offsets::char_of_utf16(&text, offset) as i32);
        self.buffer.place_cursor(&at);
        self.view.scroll_to_mark(&self.buffer.get_insert(), 0.0, true, 0.0, 0.3);
        self.focus();
    }

    /// Find & replace's edit: `[start, start + length)` (UTF-16) becomes `text`, the caret after
    /// it — one user action under `TypingHooks::external_edit`, one undo step, and the session
    /// sees it like a keystroke.
    pub fn replace_range_utf16(&self, start: usize, length: usize, text: &str) {
        self.flush_held();
        let current = self.text();
        let (start, length) = offsets::clamp_range(&current, start, length);
        self.hooks.external_edit(|| self.apply_own(start, length, text, start + utf16_len(text), 0));
    }

    pub fn focus(&self) {
        self.view.grab_focus();
    }

    /// The typing switches' store (`md.continueLists`, `md.capitalizeSentences`,
    /// `md.wrapSelection`), followed live; `None` for the defaults. The window passes `None` when
    /// it closes: the store outlives it.
    pub fn use_settings(&self, settings: Option<Rc<dyn SettingsStore>>) {
        if let Some((old, id)) = self.settings.borrow_mut().take() {
            old.changed().disconnect(id);
        }
        let Some(store) = settings else {
            self.hooks.configure(TypingSettings::DEFAULTS);
            self.wrap_selection.set(settings_keys::WRAP_SELECTION_DEFAULT);
            return;
        };
        let me = self.me.clone();
        let id = store.changed().connect(move |key: &str| {
            if (TypingSettings::is_key(key) || wrap_selection_setting::is_key(key))
                && let Some(pane) = me.upgrade()
            {
                pane.read_settings();
            }
        });
        *self.settings.borrow_mut() = Some((store, id));
        self.read_settings();
    }

    /// Hands the pane its half of the scroll link (shell-design §3.5); `ArticlePanes` calls it.
    pub fn attach_scroll_sync(&self, sync: &Rc<ScrollSync>) {
        let me = self.me.clone();
        sync.set_scroll_editor(Some(Box::new(move |fraction| {
            if let Some(pane) = me.upgrade() {
                pane.apply_scroll_fraction(fraction);
            }
        })));
        *self.scroll_sync.borrow_mut() = Some(Rc::clone(sync));
    }

    /// Args: the LF text after every change.
    pub fn text_edited(&self) -> &Event<str> {
        &self.text_edited
    }

    pub fn selection_changed(&self) -> &Event<()> {
        &self.selection_changed
    }

    /// `can-undo` / `can-redo` changed.
    pub fn history_changed(&self) -> &Event<()> {
        &self.history_changed
    }

    /// Args: the scroll fraction (shell-design §3.5).
    #[cfg(feature = "selftest")]
    pub fn scrolled(&self) -> &Event<f64> {
        &self.scrolled_event
    }

    // ── wiring ─────────────────────────────────────────────────────────────────────────────

    fn wire(self: &Rc<Self>) {
        let me = Rc::downgrade(self);
        let with = move |f: &dyn Fn(&EditorPane)| {
            if let Some(pane) = me.upgrade() {
                f(&pane);
            }
        };

        let w = with.clone();
        self.buffer.connect_begin_user_action(move |_| w(&|p| p.on_begin_user_action()));
        let w = with.clone();
        self.buffer.connect_delete_range(move |_, start, end| {
            let (start, end) = (start.offset().max(0) as usize, end.offset().max(0) as usize);
            w(&|p| p.tracker.borrow_mut().deleted(start, end));
        });
        let w = with.clone();
        self.buffer.connect_end_user_action(move |_| w(&|p| p.on_end_user_action()));
        let w = with.clone();
        self.buffer.connect_changed(move |_| w(&|p| p.on_changed()));
        let pane = Rc::downgrade(self);
        self.buffer.connect_insert_text(move |buffer, at, text| {
            let Some(p) = pane.upgrade() else { return };
            if !text.contains('\r') || p.normalizing.get() {
                return;
            }
            // §3.2: a CR or CRLF from a paste or a drop goes in as LF, so the buffer and the
            // session hold the same string. The nested insert revalidates `at`.
            buffer.stop_signal_emission_by_name("insert-text");
            p.normalizing.set(true);
            buffer.insert(at, &editor_text::from_editor(text));
            p.normalizing.set(false);
        });

        // Undo / redo: before the default handler and after it.
        let w = with.clone();
        self.buffer.connect_undo(move |_| w(&|p| p.history_will_change()));
        let w = with.clone();
        self.buffer.connect_local("undo", true, move |_| {
            w(&|p| p.history_did_change());
            None
        });
        let w = with.clone();
        self.buffer.connect_redo(move |_| w(&|p| p.history_will_change()));
        let w = with.clone();
        self.buffer.connect_local("redo", true, move |_| {
            w(&|p| p.history_did_change());
            None
        });

        let w = with.clone();
        self.buffer.connect_paste_done(move |_, _| w(&|p| p.settle_paste()));
        let w = with.clone();
        self.view.connect_paste_clipboard(move |_| w(&|p| p.announce_paste()));
        let w = with.clone();
        self.view.connect_preedit_changed(move |_, preedit| {
            let composing = !preedit.is_empty();
            w(&|p| {
                if composing {
                    p.hooks.composition_started();
                } else {
                    p.hooks.composition_ended();
                }
            });
        });

        let w = with.clone();
        self.buffer.connect_can_undo_notify(move |_| w(&|p| p.history_changed.emit(&())));
        let w = with.clone();
        self.buffer.connect_can_redo_notify(move |_| w(&|p| p.history_changed.emit(&())));
        let w = with.clone();
        self.buffer.connect_mark_set(move |buffer, _, mark| {
            if *mark == buffer.get_insert() || *mark == buffer.selection_bound() {
                w(&|p| p.selection_moved());
            }
        });
        let w = with.clone();
        self.buffer.connect_cursor_position_notify(move |_| w(&|p| p.selection_moved()));
        let w = with.clone();
        self.buffer.connect_has_selection_notify(move |buffer| {
            if !buffer.has_selection() {
                w(&|p| p.released_at.set(Some(Instant::now())));
            }
        });

        let w = with.clone();
        self.scrolled.vadjustment().connect_value_changed(move |adjustment| {
            let (value, upper, page) = (adjustment.value(), adjustment.upper(), adjustment.page_size());
            w(&|p| p.editor_did_scroll(value, upper - page));
        });

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pane = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
            let Some(p) = pane.upgrade() else { return glib::Propagation::Proceed };
            p.keys_pressed.set(p.keys_pressed.get() + 1);
            if p.on_key(&name, state) { glib::Propagation::Stop } else { glib::Propagation::Proceed }
        });
        self.view.add_controller(keys);

        self.update_placeholder();
    }

    // ── the typing hooks (shell-design §3.3) ───────────────────────────────────────────────

    fn on_begin_user_action(&self) {
        let snapshot = || self.snapshot();
        self.tracker.borrow_mut().begin(snapshot);
    }

    fn on_end_user_action(&self) {
        let text = self.text();
        let ended = self.tracker.borrow_mut().end(&text);
        self.in_end.set(true);
        match ended {
            Ended::Nothing => {}
            Ended::OwnEdit => self.report(&text),
            Ended::Held => self.bridge(),
            Ended::Judge { before, direction } => self.judge(&before, direction, text),
        }
        self.in_end.set(false);
    }

    /// One change: the typed wrap and the capital decided, the text reported, the answers applied
    /// on the next high-priority idle.
    fn judge(&self, before: &Snapshot, direction: DeleteDirection, now: String) {
        if self.hooks.is_replacing() {
            return;
        }
        let (start, length) = (before.start as isize, before.length as isize);
        // docs/formatting.md §4, asked before the hooks consume the paste flag: off with the
        // setting, in a composition, for a paste and for history.
        let wrap = (self.wrap_selection.get()
            && !self.hooks.is_composing()
            && !self.hooks.is_pasting()
            && !self.hooks.is_history())
        .then(|| formatting_adapter::wrap(&before.text, start, length, &now))
        .flatten()
        .map(|w| formatting_adapter::after_insertion(&w));

        self.hooks.announce_deletion(direction);
        self.hooks.before_text_changing(&before.text, start, length, &now);
        self.hooks.settle_deletion();
        let (insert, bound) = self.marks();
        let (s, l) = offsets::selection_utf16(&now, insert, bound);
        let plan = self.hooks.text_changing(&now, s as isize, l as isize);
        self.report(&now);
        if wrap.is_some() || plan.is_some() {
            self.apply_on_idle(wrap, plan);
        }
    }

    /// The capital or the wrap, after GtkTextHistory has closed the keystroke's own group and
    /// before the next input event — each in its own user action, so each is its own undo step.
    fn apply_on_idle(&self, wrap: Option<FormatEdit>, plan: Option<CapitalizationPlan>) {
        let me = self.me.clone();
        glib::idle_add_local_full(glib::Priority::HIGH, move || {
            let Some(pane) = me.upgrade() else { return glib::ControlFlow::Break };
            if let Some(wrap) = &wrap {
                let text = pane.text();
                let (s, l) = pane.selection_utf16();
                if formatting_adapter::still_applies(wrap, &text, s as isize, l as isize) {
                    pane.apply_format_edit(wrap);
                }
            }
            if let Some(plan) = &plan {
                let text = pane.text();
                let (s, l) = pane.selection_utf16();
                if pane.hooks.still_applies(plan, &text, s as isize, l as isize) {
                    let caret = plan.position + utf16_len(&plan.replacement);
                    pane.hooks.applying(|| {
                        pane.apply_own(plan.position, utf16_len(&plan.inserted), &plan.replacement, caret, 0)
                    });
                }
            }
            glib::ControlFlow::Break
        });
    }

    /// GTK has deleted the selection a typed character replaces and is about to insert it in a user
    /// action of its own (`gtk_text_view_commit_text`). Still inside `end-user-action`, before
    /// GtkTextHistory closes the deletion's group, an outermost user action of the pane's begins:
    /// the tracker adopts the held snapshot, the history's group stays open (its nesting count
    /// never reached zero), and GTK's insertion lands in it as an inner action. The next
    /// high-priority idle — before any further input — ends it: the replacement is one undo step,
    /// as on the Mac and md.win, and the hooks judge it as one change. A deletion nothing follows
    /// (Delete, Cut over a selection) closes the same way, alone.
    fn bridge(&self) {
        if self.bridging.replace(true) {
            return;
        }
        self.buffer.begin_user_action();
        let me = self.me.clone();
        glib::idle_add_local_full(glib::Priority::HIGH, move || {
            if let Some(pane) = me.upgrade() {
                pane.flush_held();
            }
            glib::ControlFlow::Break
        });
    }

    /// Ends an open bridge (its idle, or anything that must not run inside it: a Format command,
    /// Enter, undo, external text); its end judges the replacement.
    fn flush_held(&self) {
        if self.bridging.replace(false) {
            self.buffer.end_user_action();
        }
    }

    fn on_changed(&self) {
        self.update_placeholder();
        if self.normalizing.get() || self.hooks.is_replacing() {
            return;
        }
        if self.tracker.borrow().reports_stray_change() {
            let text = self.text();
            self.report(&text);
        }
    }

    fn history_will_change(&self) {
        self.flush_held();
        *self.history_before.borrow_mut() = Some(self.snapshot());
        self.hooks.announce_history();
        self.tracker.borrow_mut().set_history(true);
    }

    /// After GtkTextHistory applied the step (plain inserts and deletes, no user action): tracked,
    /// never judged (§3.5); the caret collapsed after a restored capital; one report.
    fn history_did_change(&self) {
        self.tracker.borrow_mut().set_history(false);
        let Some(before) = self.history_before.borrow_mut().take() else { return };
        let now = self.text();
        self.hooks.before_text_changing(&before.text, before.start as isize, before.length as isize, &now);
        let (s, l) = self.selection_utf16();
        let _ = self.hooks.text_changing(&now, s as isize, l as isize);
        if let Some(caret) = self.hooks.caret_after_history(&now, s as isize, l as isize) {
            self.select_in(&now, caret, 0);
        }
        // GTK reports a history step as this one signal, so it settles here rather than a turn
        // later (md.win's WinUI may report one step as several changes).
        self.hooks.settle_history();
        self.report(&now);
    }

    fn announce_paste(&self) {
        self.hooks.announce_paste();
        let me = self.me.clone();
        let timer = GlibScheduler.after(
            PASTE_SETTLE,
            Box::new(move || {
                if let Some(pane) = me.upgrade() {
                    // The timer has fired: forget its handle without cancelling it.
                    if let Some(spent) = pane.paste_settle.borrow_mut().take() {
                        spent.detach();
                    }
                    pane.hooks.settle_paste();
                }
            }),
        );
        *self.paste_settle.borrow_mut() = Some(timer);
    }

    fn settle_paste(&self) {
        drop(self.paste_settle.borrow_mut().take());
        self.hooks.settle_paste();
    }

    fn on_key(&self, keyval: &str, state: gtk::gdk::ModifierType) -> bool {
        let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
        let control = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        let alt = state.contains(gtk::gdk::ModifierType::ALT_MASK);
        match typing::enter_action(keyval, shift, control, alt, self.hooks.is_composing()) {
            EnterAction::NotEnter | EnterAction::Pass => false,
            EnterAction::Plain => {
                // Shift+Enter / Ctrl+Enter: a plain newline, typed like any other character.
                self.flush_held();
                self.buffer.begin_user_action();
                self.buffer.delete_selection(true, self.view.is_editable());
                self.buffer.insert_interactive_at_cursor("\n", self.view.is_editable());
                self.buffer.end_user_action();
                self.view.scroll_mark_onscreen(&self.buffer.get_insert());
                true
            }
            EnterAction::Smart => {
                self.flush_held();
                let text = self.text();
                let (s, l) = self.selection_utf16();
                let Some(edit) = self.hooks.enter(&text, s as isize, l as isize) else { return false };
                self.hooks.applying(|| self.apply_own(edit.location, edit.length, &edit.replacement, edit.caret, 0));
                true
            }
        }
    }

    // ── applying md's own edits ────────────────────────────────────────────────────────────

    /// A formatting edit (a Format command, the typed wrap): one replacement under
    /// `external_edit`, the edit's own selection after it.
    fn apply_format_edit(&self, edit: &FormatEdit) {
        let (start, length) = (edit.start, edit.end - edit.start);
        let (sel_start, sel_length) = (edit.selection_start, edit.selection_end - edit.selection_start);
        self.hooks.external_edit(|| self.apply_own(start, length, &edit.text, sel_start, sel_length));
    }

    /// `[start, start + length)` (UTF-16) becomes `replacement` in one user action, then
    /// `[sel_start, sel_start + sel_length)` of the new text is selected. The caller holds the
    /// hooks' `applying` / `external_edit`; the tracker's own flag keeps the edit unjudged, and its
    /// end reports the text. Widened by `typing::undo_unit_edit` so GtkTextHistory never chains it
    /// onto the typing before it.
    fn apply_own(&self, start: usize, length: usize, replacement: &str, sel_start: usize, sel_length: usize) {
        let text = self.text();
        let (start, length, replacement) = typing::undo_unit_edit(&text, start, length, replacement);
        let (from, to) = offsets::range_chars(&text, start, length);
        self.tracker.borrow_mut().own_begin();
        self.buffer.begin_user_action();
        let mut a = self.buffer.iter_at_offset(from as i32);
        let mut b = self.buffer.iter_at_offset(to as i32);
        if from != to {
            self.buffer.delete(&mut a, &mut b);
        }
        if !replacement.is_empty() {
            self.buffer.insert(&mut a, &replacement);
        }
        let now = self.text();
        self.select_in(&now, sel_start, sel_length);
        self.buffer.end_user_action();
        self.tracker.borrow_mut().own_end();
        self.view.scroll_mark_onscreen(&self.buffer.get_insert());
    }

    // ── helpers ────────────────────────────────────────────────────────────────────────────

    fn report(&self, text: &str) {
        self.text_edited.emit(text);
    }

    fn snapshot(&self) -> Snapshot {
        let (insert, bound) = self.marks();
        Snapshot::new(self.text(), insert, bound)
    }

    /// The `insert` and `selection_bound` marks, in scalars.
    fn marks(&self) -> (usize, usize) {
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset().max(0) as usize;
        let bound = self.buffer.iter_at_mark(&self.buffer.selection_bound()).offset().max(0) as usize;
        (insert, bound)
    }

    /// Selects a UTF-16 range of `text` (the buffer's current text): the anchor at its start, the
    /// caret at its end.
    ///
    /// LINUX (X11): a non-empty selection claims the PRIMARY selection, and an emptied one releases
    /// it. GDK's X11 backend (gdkclipboard-x11.c, GTK 4.22) takes the `SelectionClear` the X
    /// server sends for a release as another client's claim unless its server time is *older*
    /// than the latest claim — so a release and a re-claim inside the same millisecond, which is
    /// exactly what replacing a selection and selecting the result does, detaches the new
    /// selection a moment later and collapses it to the caret (measured by the self-test: the
    /// typed wrap, Format commands, Replace). A selection that follows a release by less than
    /// [`PRIMARY_RECLAIM`] is therefore made that much later — unless anything else selected or
    /// typed meanwhile. GDK's Wayland backend ignores such offers while it owns a source.
    fn select_in(&self, text: &str, start: usize, length: usize) {
        let (from, to) = offsets::range_chars(text, start, length);
        let generation = self.select_generation.get() + 1;
        self.select_generation.set(generation);
        let since_release = self.released_at.get().map(|at| at.elapsed());
        match since_release {
            Some(elapsed) if from != to && elapsed < PRIMARY_RECLAIM => {
                // The caret goes to the selection's end now; the anchor follows once the release
                // is a millisecond in the past.
                self.buffer.place_cursor(&self.buffer.iter_at_offset(to as i32));
                let me = self.me.clone();
                let text = text.to_string();
                let keys = self.keys_pressed.get();
                glib::timeout_add_local_once(PRIMARY_RECLAIM - elapsed + Duration::from_millis(1), move || {
                    let Some(pane) = me.upgrade() else { return };
                    if pane.select_generation.get() == generation
                        && pane.keys_pressed.get() == keys
                        && pane.text() == text
                    {
                        pane.select_chars(from, to);
                    }
                });
            }
            _ => self.select_chars(from, to),
        }
    }

    fn select_chars(&self, from: usize, to: usize) {
        let bound = self.buffer.iter_at_offset(from as i32);
        let insert = self.buffer.iter_at_offset(to as i32);
        self.buffer.select_range(&insert, &bound);
    }

    fn scroll_to_selection(&self) {
        self.view.scroll_to_mark(&self.buffer.get_insert(), 0.05, false, 0.0, 0.0);
    }

    fn selection_moved(&self) {
        let insert = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset();
        let bound = self.buffer.iter_at_mark(&self.buffer.selection_bound()).offset();
        if self.last_selection.replace((insert, bound)) != (insert, bound) {
            self.selection_changed.emit(&());
        }
    }

    fn update_placeholder(&self) {
        self.placeholder.set_visible(self.buffer.char_count() == 0);
    }

    fn read_settings(&self) {
        let store = self.settings.borrow().as_ref().map(|(store, _)| Rc::clone(store));
        if let Some(store) = store {
            self.hooks.configure(TypingSettings::read(store.as_ref()));
            self.wrap_selection.set(wrap_selection_setting::read(store.as_ref()));
        }
    }

    // ── the editor half of the scroll sync (shell-design §3.5) ─────────────────────────────

    fn editor_did_scroll(&self, value: f64, scrollable: f64) {
        if self.guard.is_echo() || scrollable <= 0.0 {
            return;
        }
        let fraction = ScrollSync::clamp(value / scrollable);
        self.scrolled_event.emit(&fraction);
        let sync = self.scroll_sync.borrow().clone();
        if let Some(sync) = sync {
            sync.editor_did_scroll(fraction);
        }
    }

    fn apply_scroll_fraction(&self, fraction: f64) {
        let adjustment = self.scrolled.vadjustment();
        let scrollable = adjustment.upper() - adjustment.page_size();
        if scrollable <= 0.0 {
            return;
        }
        let _applying = self.guard.applying();
        adjustment.set_value(ScrollSync::clamp(fraction) * scrollable);
    }
}

impl Drop for EditorPane {
    fn drop(&mut self) {
        if let Some((store, id)) = self.settings.borrow_mut().take() {
            store.changed().disconnect(id);
        }
    }
}
