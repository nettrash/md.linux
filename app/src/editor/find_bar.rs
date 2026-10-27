//! `FindBar` — md's find & replace bar at the top of the editor column (shell-design §3.6): a
//! `gtk::Revealer` with the query `SearchEntry`, the tally, Previous / Next, Done, and the replace
//! row; every decision md-logic's `TextSearch` over UTF-16 offsets.
//!
//! Behaviour is md.win §3.5's (its `FindBar` plus the window's handlers, which on Linux live here
//! because the bar belongs to `ArticlePanes` in both window kinds): find as you type from the
//! anchor (`FindAsYouType`); Enter / Shift+Enter in the query = next / previous with the focus
//! kept; Ctrl+G from the editor selects the hit and keeps the focus in the editor; Enter in the
//! replacement = Replace (focus stays), Shift+Enter = Replace All (focus back to the editor with
//! the rewritten span selected — so the Ctrl+Z that follows reaches the document, one step); Escape
//! (`stop-search`, or the replacement's own key) and Done close the bar and return the focus.
//!
//! **The hit the bar stands on.** GTK keeps one PRIMARY selection per display: a find field that
//! selects its own text (Ctrl+H over an earlier replacement, the query's select-all on focus)
//! takes it, and the editor's selection — the hit — collapses to its caret (`GtkTextBuffer`'s
//! clipboard detach). Replace would then see no hit and act as Find Next. The bar therefore
//! remembers the hit it last selected and, while the editor's caret sits collapsed at that hit's
//! end with the hit's text still there, treats the hit as the selection — for Replace, for Find
//! Previous and for the tally. Any other caret, selection or edit forgets it.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::prelude::*;
use md_core::text::utf16::utf16_len;
use md_logic::documents::FindAsYouType;
use md_logic::documents::text_search::{self, Match};
use md_logic::event::Event;
use md_logic::strings::find;

use super::editor_pane::EditorPane;

/// The bar of one editor column.
pub struct FindBar {
    me: Weak<FindBar>,
    editor: Rc<EditorPane>,
    revealer: gtk::Revealer,
    query: gtk::SearchEntry,
    replacement: gtk::Entry,
    tally: gtk::Label,
    toggle: gtk::ToggleButton,
    previous: gtk::Button,
    next: gtk::Button,
    done: gtk::Button,
    replace_row: gtk::Box,
    replace: gtk::Button,
    replace_all: gtk::Button,
    anchor: RefCell<FindAsYouType>,
    /// Up while the bar writes its own query (a seeded selection): no search as you type.
    seeding: Cell<bool>,
    /// The hit the bar last selected (see the module comment).
    last_hit: Cell<Option<Match>>,
    /// Up while the bar's own Replace edits the text.
    replacing: Cell<bool>,
    changed: Event<()>,
}

impl FindBar {
    pub fn new(editor: &Rc<EditorPane>) -> Rc<FindBar> {
        let query =
            gtk::SearchEntry::builder().placeholder_text(find::QUERY_PLACEHOLDER).search_delay(0).hexpand(true).build();
        query.add_css_class("md-find-field");
        let replacement = gtk::Entry::builder().placeholder_text(find::REPLACEMENT_PLACEHOLDER).hexpand(true).build();
        replacement.add_css_class("md-find-field");
        let tally = gtk::Label::builder().width_chars(10).xalign(1.0).build();
        tally.add_css_class("dim-label");
        let toggle =
            gtk::ToggleButton::builder().icon_name("pan-end-symbolic").tooltip_text(find::TOGGLE_REPLACE).build();
        toggle.add_css_class("flat");
        let previous = gtk::Button::builder().icon_name("go-up-symbolic").tooltip_text(find::PREVIOUS).build();
        previous.add_css_class("flat");
        let next = gtk::Button::builder().icon_name("go-down-symbolic").tooltip_text(find::NEXT).build();
        next.add_css_class("flat");
        let done = gtk::Button::with_label(find::DONE);
        let replace = gtk::Button::with_label(find::REPLACE);
        let replace_all = gtk::Button::with_label(find::REPLACE_ALL);

        let find_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for widget in [toggle.upcast_ref::<gtk::Widget>(), query.upcast_ref(), tally.upcast_ref()] {
            find_row.append(widget);
        }
        for widget in [previous.upcast_ref::<gtk::Widget>(), next.upcast_ref(), done.upcast_ref()] {
            find_row.append(widget);
        }
        let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        // Under the query, past the chevron.
        replace_row.set_margin_start(38);
        replace_row.append(&replacement);
        replace_row.append(&replace);
        replace_row.append(&replace_all);
        replace_row.set_visible(false);

        let rows = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(8)
            .margin_end(8)
            .build();
        rows.append(&find_row);
        rows.append(&replace_row);
        // The bars' paper-secondary (theme `md-bar`), the margins inside it.
        let padded = gtk::Box::new(gtk::Orientation::Vertical, 0);
        padded.add_css_class("md-bar");
        padded.append(&rows);

        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .child(&padded)
            .reveal_child(false)
            .build();
        // Hidden (not only folded) while closed, so the column's height is the editor's.
        revealer.set_visible(false);

        let bar = Rc::new_cyclic(|me| FindBar {
            me: me.clone(),
            editor: Rc::clone(editor),
            revealer,
            query,
            replacement,
            tally,
            toggle,
            previous,
            next,
            done,
            replace_row,
            replace,
            replace_all,
            anchor: RefCell::new(FindAsYouType::new()),
            seeding: Cell::new(false),
            last_hit: Cell::new(None),
            replacing: Cell::new(false),
            changed: Event::new(),
        });
        bar.wire();
        bar
    }

    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    /// The query field (the self-test types into it).
    #[cfg(feature = "selftest")]
    pub fn query_entry(&self) -> gtk::SearchEntry {
        self.query.clone()
    }

    /// The replacement field.
    #[cfg(feature = "selftest")]
    pub fn replacement_entry(&self) -> gtk::Entry {
        self.replacement.clone()
    }

    /// "3 of 12", "No results", or "".
    #[cfg(feature = "selftest")]
    pub fn tally_text(&self) -> String {
        self.tally.text().to_string()
    }

    /// Edit ▸ Find ▸ Find… (Ctrl+F): the bar without the replace row, the query focused.
    pub fn show_find(&self) {
        self.open(false);
        self.query.grab_focus();
        self.query.select_region(0, -1);
    }

    /// Find and Replace… (Ctrl+H): the replace row shown, the replacement focused.
    pub fn show_replace(&self) {
        self.open(true);
        self.replacement.grab_focus();
        self.replacement.select_region(0, -1);
    }

    /// Find Next (`forward`) / Find Previous: the hit selected in the editor. From the bar the
    /// focus stays in the bar; from anywhere else it lands in the editor.
    pub fn find_next(&self, forward: bool) {
        let query = self.query_text();
        if query.is_empty() {
            return;
        }
        let text = self.editor.text();
        let (start, length) = self.selection(&text, &query);
        let hit = if forward {
            text_search::next(&text, &query, (start + length) as i64)
        } else {
            text_search::previous(&text, &query, start as i64)
        };
        let Some(hit) = hit else { return };
        self.anchor.borrow_mut().stepped(hit);
        self.last_hit.set(Some(hit));
        if self.has_focus_within() {
            self.editor.select_utf16(hit.index, hit.length);
        } else {
            self.editor.select_range(hit.index, hit.length);
        }
        self.update_tally();
    }

    /// Use Selection for Find (Ctrl+E): the selection becomes the query; the focus stays put.
    pub fn use_selection(&self) {
        let selected = self.editor.selected_text();
        if selected.is_empty() {
            return;
        }
        self.reveal(true);
        self.anchor.borrow_mut().focused(self.editor.selection_utf16().0 as i64);
        self.set_query(&selected);
        self.update_tally();
        self.changed.emit(&());
    }

    pub fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }

    pub fn has_query(&self) -> bool {
        !self.query_text().is_empty()
    }

    /// Done / Escape: closes and returns focus to the editor.
    pub fn close(&self) {
        if !self.is_open() {
            return;
        }
        self.reveal(false);
        self.last_hit.set(None);
        self.editor.focus();
        self.changed.emit(&());
    }

    /// Raised when the bar opens or closes and when its query changes — what the window's
    /// snapshot publishes (`ShellSnapshot`'s find flags).
    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }

    /// Replace: the hit the editor stands on, then the next one selected; focus stays here.
    pub fn replace_once(&self) {
        let query = self.query_text();
        if query.is_empty() {
            return;
        }
        let replacement = self.replacement.text().to_string();
        let text = self.editor.text();
        let (start, length) = self.selection(&text, &query);
        let step = text_search::replace(&text, &query, &replacement, start as i64, length as i64);
        if let Some(edit) = &step.apply {
            self.replacing.set(true);
            self.editor.replace_range_utf16(edit.start, edit.length, &edit.text);
            self.replacing.set(false);
        }
        // Re-read: `search_from` is an offset into the text the edit left.
        if let Some(hit) = text_search::next(&self.editor.text(), &query, step.search_from as i64) {
            self.anchor.borrow_mut().stepped(hit);
            self.last_hit.set(Some(hit));
            self.editor.select_utf16(hit.index, hit.length);
        }
        self.update_tally();
    }

    /// Replace All: every hit as the ONE edit `TextSearch` plans — one undo step — then the focus
    /// back in the editor with the rewritten span selected.
    pub fn replace_everything(&self) {
        let query = self.query_text();
        if query.is_empty() {
            return;
        }
        let replacement = self.replacement.text().to_string();
        let plan = text_search::replace_all(&self.editor.text(), &query, &replacement);
        if plan.count == 0 {
            return;
        }
        self.replacing.set(true);
        self.editor.replace_range_utf16(plan.apply.start, plan.apply.length, &plan.apply.text);
        self.replacing.set(false);
        self.last_hit.set(None);
        self.editor.select_range(plan.apply.start, utf16_len(&plan.apply.text));
        self.update_tally();
    }

    // ── internals ──────────────────────────────────────────────────────────────────────────

    fn wire(self: &Rc<Self>) {
        let me = self.me.clone();
        let with = move |f: &dyn Fn(&FindBar)| {
            if let Some(bar) = me.upgrade() {
                f(&bar);
            }
        };

        let w = with.clone();
        self.query.connect_search_changed(move |_| w(&|b| b.query_changed()));
        let w = with.clone();
        self.query.connect_activate(move |_| w(&|b| b.find_next(true)));
        let w = with.clone();
        self.query.connect_next_match(move |_| w(&|b| b.find_next(true)));
        let w = with.clone();
        self.query.connect_previous_match(move |_| w(&|b| b.find_next(false)));
        let w = with.clone();
        self.query.connect_stop_search(move |_| w(&|b| b.close()));
        let focus = gtk::EventControllerFocus::new();
        let w = with.clone();
        focus.connect_enter(move |_| {
            w(&|b| b.anchor.borrow_mut().focused(b.editor.selection_utf16().0 as i64));
        });
        self.query.add_controller(focus);
        self.query.add_controller(self.shift_enter(true));

        let w = with.clone();
        self.replacement.connect_activate(move |_| w(&|b| b.replace_once()));
        self.replacement.add_controller(self.shift_enter(false));

        let w = with.clone();
        self.previous.connect_clicked(move |_| w(&|b| b.find_next(false)));
        let w = with.clone();
        self.next.connect_clicked(move |_| w(&|b| b.find_next(true)));
        let w = with.clone();
        self.done.connect_clicked(move |_| w(&|b| b.close()));
        let w = with.clone();
        self.replace.connect_clicked(move |_| {
            w(&|b| {
                b.replace_once();
                b.replacement.grab_focus();
            })
        });
        let w = with.clone();
        self.replace_all.connect_clicked(move |_| w(&|b| b.replace_everything()));
        let w = with.clone();
        self.toggle.connect_toggled(move |toggle| {
            let shown = toggle.is_active();
            w(&|b| {
                b.show_replace_row(shown);
                if shown {
                    b.replacement.grab_focus();
                }
            })
        });

        // The tally follows the text and the selection while the bar is up; the remembered hit is
        // forgotten by any edit but the bar's own and by any caret or selection but its own.
        let w = with.clone();
        self.editor.selection_changed().connect(move |_| {
            w(&|b| {
                if let Some(hit) = b.last_hit.get() {
                    let (start, length) = b.editor.selection_utf16();
                    let on_hit =
                        (start, length) == (hit.index, hit.length) || (length == 0 && start == hit.index + hit.length);
                    if !on_hit {
                        b.last_hit.set(None);
                    }
                }
                b.update_tally();
            })
        });
        let w = with;
        self.editor.text_edited().connect(move |_| {
            w(&|b| {
                if !b.replacing.get() {
                    b.last_hit.set(None);
                }
                b.update_tally();
            })
        });
    }

    /// Shift+Enter in the query is Find Previous; in the replacement, Replace All. Plain Enter is
    /// each field's `activate`. Capture phase, so the entry's own Enter never sees the Shift.
    fn shift_enter(&self, in_query: bool) -> gtk::EventControllerKey {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = self.me.clone();
        keys.connect_key_pressed(move |_, keyval, _, state| {
            let Some(bar) = me.upgrade() else { return glib::Propagation::Proceed };
            let name = keyval.name().map(|n| n.to_string()).unwrap_or_default();
            let enter = matches!(name.as_str(), "Return" | "KP_Enter" | "ISO_Enter");
            if !in_query && name == "Escape" {
                bar.close();
                return glib::Propagation::Stop;
            }
            let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
            let others = state.intersects(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::ALT_MASK);
            if !enter || !shift || others {
                return glib::Propagation::Proceed;
            }
            if in_query {
                bar.find_next(false);
            } else {
                bar.replace_everything();
            }
            glib::Propagation::Stop
        });
        keys
    }

    fn open(&self, with_replace: bool) {
        self.reveal(true);
        self.anchor.borrow_mut().focused(self.editor.selection_utf16().0 as i64);
        self.show_replace_row(with_replace);
        let selected = self.editor.selected_text();
        if !selected.is_empty() {
            self.set_query(&selected);
        }
        self.update_tally();
        self.changed.emit(&());
    }

    fn reveal(&self, open: bool) {
        if open {
            self.revealer.set_visible(true);
            self.revealer.set_reveal_child(true);
        } else {
            self.revealer.set_reveal_child(false);
            self.revealer.set_visible(false);
        }
    }

    fn show_replace_row(&self, shown: bool) {
        self.replace_row.set_visible(shown);
        if self.toggle.is_active() != shown {
            self.toggle.set_active(shown);
        }
        self.toggle.set_icon_name(if shown { "pan-down-symbolic" } else { "pan-end-symbolic" });
    }

    fn set_query(&self, query: &str) {
        self.seeding.set(true);
        self.query.set_text(query);
        self.query.set_position(-1);
        self.seeding.set(false);
    }

    fn query_text(&self) -> String {
        self.query.text().to_string()
    }

    /// Find as you type — only while the writer is in the bar; a query set from code moves nothing.
    fn query_changed(&self) {
        if self.is_open() && !self.seeding.get() && self.has_focus_within() {
            let query = self.query_text();
            let hit = self.anchor.borrow().search(&self.editor.text(), &query);
            self.last_hit.set(hit);
            match hit {
                Some(hit) => self.editor.select_utf16(hit.index, hit.length),
                None => self.editor.select_utf16(self.anchor.borrow().anchor(), 0),
            }
        }
        self.update_tally();
        self.changed.emit(&());
    }

    fn update_tally(&self) {
        if !self.is_open() {
            return;
        }
        let query = self.query_text();
        let text = self.editor.text();
        let (start, length) = self.selection(&text, &query);
        let tally = text_search::count(&text, &query, start as i64, length as i64);
        self.tally.set_text(&find::tally(tally.current, tally.total, !query.is_empty()));
    }

    /// The editor's selection — or the remembered hit, while the editor's caret sits collapsed at
    /// its end and the hit's text is still a match (see the module comment).
    fn selection(&self, text: &str, query: &str) -> (usize, usize) {
        let (start, length) = self.editor.selection_utf16();
        if length == 0
            && let Some(hit) = self.last_hit.get()
            && start == hit.index + hit.length
            && text_search::selection_is_match(text, query, hit.index as i64, hit.length as i64)
        {
            return (hit.index, hit.length);
        }
        (start, length)
    }

    /// Whether the keyboard focus is in the bar (either field or a button).
    fn has_focus_within(&self) -> bool {
        let Some(root) = self.revealer.root() else { return false };
        let Some(focus) = root.focus() else { return false };
        focus.is_ancestor(&self.revealer)
    }
}
