//! `AdwAlerts` — md-logic's `Alerts` over `adw::AlertDialog` (shell-design §7.8): one dialog at a
//! time per window (a queue), name prompts with an entry as the extra child, and `open_changed`
//! raised while one is up so the window disables its `win.` actions (§2.2).
//!
//! Every title, message and button comes from `md_logic::strings` or from the caller (who takes
//! its own from there). The defaults follow the platform's safety rule, as md.win's: Delete and
//! Replace default to Cancel, Save defaults to Save, a name prompt to its accept button; Escape is
//! always the cancelling answer (`set_close_response`). Response ids are md's own and never shown.
//!
//! A window that goes away (`gtk_window_destroy`) answers its dialogs — the one up and every one
//! in line — with the cancelling answer at once: nobody is left to ask, and a caller must never
//! hang on a dead window. GTK 4 emits `destroy` only once the last reference is gone, which the
//! shell's own structs delay, so "gone" is the window leaving GTK's toplevel list (seen when it is
//! unrealized) or, failing that, `destroy`.
#![allow(dead_code)] // S0 stub: removed at the integration gate (P1) — this file's callers are P1's and P5's stubs until then

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use adw::prelude::*;
use md_core::book::book_folder;
use md_core::text::unicode;
use md_logic::documents::file_names;
use md_logic::event::Event;
use md_logic::exec::{Completion, LocalBoxFuture};
use md_logic::seams::{Alerts, CloseChoice};
use md_logic::strings::buttons;

// Response ids (never shown).
const OK: &str = "ok";
const CANCEL: &str = "cancel";
const ACCEPT: &str = "accept";
const DELETE: &str = "delete";
const REPLACE: &str = "replace";
const SAVE: &str = "save";
const DISCARD: &str = "discard";

/// The alerts of one window.
pub struct AdwAlerts {
    inner: Rc<Inner>,
}

struct Inner {
    /// Weak: the alerts must not keep a closed window alive.
    window: glib::WeakRef<gtk::Window>,
    /// One dialog at a time: true while a [`Turn`] exists.
    busy: Cell<bool>,
    /// The callers waiting their turn, in order.
    waiting: RefCell<VecDeque<Rc<Waiter>>>,
    /// The dialog on screen, if any.
    showing: RefCell<Option<Rc<Shown>>>,
    gone: Cell<bool>,
    open_changed: Event<bool>,
}

/// One caller in line.
struct Waiter {
    /// Set when the turn is handed to it (not when the window's end wakes it).
    granted: Cell<bool>,
    wake: Completion<()>,
}

/// The right to show a dialog on the window; dropping it hands the right on, so a caller that
/// drops its future mid-dialog cannot leave the window's queue stuck.
struct Turn(Rc<Inner>);

impl Drop for Turn {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// A caller waiting in line. Dropped before its turn came, it leaves the line; dropped after the
/// turn was handed to it but before it woke, it hands the turn on.
struct InLine {
    inner: Rc<Inner>,
    waiter: Rc<Waiter>,
    woke: bool,
}

impl Drop for InLine {
    fn drop(&mut self) {
        if self.woke {
            return;
        }
        if self.waiter.granted.get() {
            self.inner.release();
        } else {
            self.inner.waiting.borrow_mut().retain(|w| !Rc::ptr_eq(w, &self.waiter));
        }
    }
}

/// A dialog on screen and its one answer.
struct Shown {
    dialog: adw::AlertDialog,
    answered: Cell<bool>,
    done: Completion<String>,
}

impl Shown {
    /// The first answer wins; later ones (a `closed` after a `response`) are ignored.
    fn answer(&self, response: String) {
        if !self.answered.replace(true) {
            self.done.complete(response);
        }
    }

    /// Answers with the close response and takes the dialog down.
    fn close(&self) {
        if !self.answered.get() {
            self.answer(self.dialog.close_response().to_string());
            self.dialog.force_close();
        }
    }
}

/// The dialog on screen for as long as its caller waits: dropped before the answer (the caller
/// gave up), it closes the dialog.
struct Showing {
    inner: Rc<Inner>,
    shown: Rc<Shown>,
}

impl Drop for Showing {
    fn drop(&mut self) {
        self.inner.showing.borrow_mut().take();
        self.shown.close();
    }
}

impl AdwAlerts {
    pub fn new(window: &gtk::Window) -> Rc<AdwAlerts> {
        let inner = Rc::new(Inner {
            window: window.downgrade(),
            busy: Cell::new(false),
            waiting: RefCell::new(VecDeque::new()),
            showing: RefCell::new(None),
            gone: Cell::new(false),
            open_changed: Event::new(),
        });
        {
            // gtk_window_destroy takes the window out of the toplevel list, then unrealizes it.
            let inner = Rc::downgrade(&inner);
            window.connect_unrealize(move |window| {
                let listed = gtk::Window::list_toplevels().iter().any(|w| w == window.upcast_ref::<gtk::Widget>());
                if let Some(inner) = inner.upgrade().filter(|_| !listed) {
                    inner.window_gone();
                }
            });
        }
        {
            let inner = Rc::downgrade(&inner);
            window.connect_destroy(move |_| {
                if let Some(inner) = inner.upgrade() {
                    inner.window_gone();
                }
            });
        }
        Rc::new(AdwAlerts { inner })
    }

    /// Args: whether a dialog is now showing on this window (true when the first one opens, false
    /// when the last queued one has answered).
    pub fn open_changed(&self) -> &Event<bool> {
        &self.inner.open_changed
    }

    /// Whether a dialog is up (or about to be, for a caller whose turn has come).
    pub fn is_open(&self) -> bool {
        self.inner.busy.get()
    }

    /// Runs `dialog` when this window's turn comes; `None` when the window is gone.
    fn show(&self, dialog: adw::AlertDialog) -> LocalBoxFuture<'static, Option<String>> {
        let inner = Rc::clone(&self.inner);
        Box::pin(async move {
            let turn = Inner::acquire(Rc::clone(&inner)).await?;
            let window = inner.window.upgrade().filter(|_| !inner.gone.get())?;
            let answer = Inner::present(&inner, dialog, &window).await;
            drop(turn);
            Some(answer)
        })
    }
}

impl Inner {
    /// Waits for this caller's turn; `None` when the window went first.
    async fn acquire(inner: Rc<Inner>) -> Option<Turn> {
        if inner.gone.get() {
            return None;
        }
        if !inner.busy.replace(true) {
            inner.open_changed.emit(&true);
            return Some(Turn(inner));
        }
        let waiter = Rc::new(Waiter { granted: Cell::new(false), wake: Completion::new() });
        inner.waiting.borrow_mut().push_back(Rc::clone(&waiter));
        let mut line = InLine { inner: Rc::clone(&inner), waiter: Rc::clone(&waiter), woke: false };
        waiter.wake.wait().await;
        line.woke = true;
        waiter.granted.get().then_some(Turn(inner))
    }

    /// Hands the turn to the next caller in line, or ends the busy spell.
    fn release(&self) {
        let next = self.waiting.borrow_mut().pop_front();
        match next {
            Some(waiter) => {
                waiter.granted.set(true);
                waiter.wake.complete(());
            }
            None => {
                self.busy.set(false);
                self.open_changed.emit(&false);
            }
        }
    }

    /// The window went: the dialog up answers its close response, the callers in line answer
    /// for themselves, and every later call answers at once.
    fn window_gone(&self) {
        if self.gone.replace(true) {
            return;
        }
        let shown = self.showing.borrow_mut().take();
        if let Some(shown) = shown {
            shown.close();
        }
        let waiting: Vec<Rc<Waiter>> = self.waiting.borrow_mut().drain(..).collect();
        for waiter in waiting {
            waiter.wake.complete(());
        }
    }

    /// Presents `dialog` on `window` and answers with the id of the response chosen — the close
    /// response for Escape, or when the dialog is closed any other way and says nothing.
    ///
    /// `adw_alert_dialog_choose` without its `GCancellable`: libadwaita-rs 0.9's `choose`
    /// trampoline asserts that `choose_finish` returned a string, and for a cancelled choice it
    /// returns NULL — so dropping a `choose_future` (which cancels) aborts the whole process. Here
    /// a caller that stops waiting closes the dialog instead ([`Showing`]).
    async fn present(inner: &Rc<Inner>, dialog: adw::AlertDialog, window: &gtk::Window) -> String {
        let shown = Rc::new(Shown { dialog: dialog.clone(), answered: Cell::new(false), done: Completion::new() });
        {
            let shown = Rc::downgrade(&shown);
            dialog.connect_response(None, move |_, response| {
                if let Some(shown) = shown.upgrade() {
                    shown.answer(response.to_string());
                }
            });
        }
        {
            // `response` is emitted from inside the dialog's own `closed` handling, after this
            // handler: only a close that brought no response by the next turn is the close
            // response.
            let shown = Rc::downgrade(&shown);
            dialog.connect_closed(move |dialog| {
                let close_response = dialog.close_response().to_string();
                let shown = shown.clone();
                glib::idle_add_local_once(move || {
                    if let Some(shown) = shown.upgrade() {
                        shown.answer(close_response);
                    }
                });
            });
        }
        *inner.showing.borrow_mut() = Some(Rc::clone(&shown));
        let showing = Showing { inner: Rc::clone(inner), shown: Rc::clone(&shown) };
        dialog.present(Some(window));
        let answer = shown.done.wait().await;
        drop(showing);
        answer
    }
}

/// A dialog with a heading, an optional body (an empty message shows none), and its responses in
/// the order given — GNOME puts the cancelling one first and the default last.
fn dialog(heading: &str, body: &str, responses: &[(&str, &str)]) -> adw::AlertDialog {
    let dialog = adw::AlertDialog::new(Some(heading), (!body.is_empty()).then_some(body));
    for (id, label) in responses {
        dialog.add_response(id, label);
    }
    dialog
}

impl Alerts for AdwAlerts {
    fn warn(&self, title: String, message: String) -> LocalBoxFuture<'static, ()> {
        let alert = dialog(&title, &message, &[(OK, buttons::OK)]);
        alert.set_default_response(Some(OK));
        alert.set_close_response(OK);
        let shown = self.show(alert);
        Box::pin(async move {
            shown.await;
        })
    }

    /// The entry starts with `initial`, selected, so typing replaces it; Enter accepts. The
    /// accept button is off while the name is blank (a blank name can never be accepted), and the
    /// entry turns red while the name holds a character `book_folder::is_valid_name` refuses — the
    /// button stays on there, so the caller's alert can say why
    /// (`strings::documents::INVALID_NAME_MESSAGE`). The text comes back as typed: every caller
    /// trims it (`book_folder::trimmed_prompt_name`).
    fn prompt_name(
        &self,
        title: String,
        message: String,
        initial: String,
        accept_label: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        let alert = dialog(&title, &message, &[(CANCEL, buttons::CANCEL), (ACCEPT, &accept_label)]);
        alert.set_response_appearance(ACCEPT, adw::ResponseAppearance::Suggested);
        alert.set_default_response(Some(ACCEPT));
        alert.set_close_response(CANCEL);
        let entry = gtk::Entry::builder().text(initial.as_str()).activates_default(true).build();
        let judge = {
            let alert = alert.downgrade();
            move |entry: &gtk::Entry| {
                let Some(alert) = alert.upgrade() else { return };
                let text = entry.text();
                let verdict = name_verdict(&text);
                alert.set_response_enabled(ACCEPT, verdict != NameVerdict::Blank);
                if verdict == NameVerdict::Refused {
                    entry.add_css_class("error");
                } else {
                    entry.remove_css_class("error");
                }
            }
        };
        judge(&entry);
        entry.connect_changed(judge);
        alert.set_extra_child(Some(&entry));
        alert.set_focus(Some(&entry));
        let shown = self.show(alert);
        Box::pin(async move {
            match shown.await.as_deref() {
                Some(ACCEPT) => Some(entry.text().to_string()),
                _ => None,
            }
        })
    }

    fn confirm_delete(&self, title: String, message: String) -> LocalBoxFuture<'static, bool> {
        let alert = dialog(&title, &message, &[(CANCEL, buttons::CANCEL), (DELETE, buttons::DELETE)]);
        alert.set_response_appearance(DELETE, adw::ResponseAppearance::Destructive);
        alert.set_default_response(Some(CANCEL));
        alert.set_close_response(CANCEL);
        let shown = self.show(alert);
        Box::pin(async move { shown.await.as_deref() == Some(DELETE) })
    }

    /// `title` is the whole question (`strings::documents::save_changes_question`), as md.win's
    /// caller passes it.
    fn ask_save_changes(&self, title: String) -> LocalBoxFuture<'static, CloseChoice> {
        let alert =
            dialog(&title, "", &[(CANCEL, buttons::CANCEL), (DISCARD, buttons::DONT_SAVE), (SAVE, buttons::SAVE)]);
        alert.set_response_appearance(DISCARD, adw::ResponseAppearance::Destructive);
        alert.set_response_appearance(SAVE, adw::ResponseAppearance::Suggested);
        alert.set_default_response(Some(SAVE));
        alert.set_close_response(CANCEL);
        let shown = self.show(alert);
        Box::pin(async move {
            match shown.await.as_deref() {
                Some(SAVE) => CloseChoice::Save,
                Some(DISCARD) => CloseChoice::DontSave,
                _ => CloseChoice::Cancel,
            }
        })
    }

    fn confirm_replace(&self, message: String) -> LocalBoxFuture<'static, bool> {
        let alert = dialog(&message, "", &[(CANCEL, buttons::CANCEL), (REPLACE, buttons::REPLACE)]);
        alert.set_response_appearance(REPLACE, adw::ResponseAppearance::Destructive);
        alert.set_default_response(Some(CANCEL));
        alert.set_close_response(CANCEL);
        let shown = self.show(alert);
        Box::pin(async move { shown.await.as_deref() == Some(REPLACE) })
    }
}

/// What a name prompt makes of the text typed so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NameVerdict {
    /// Nothing but white space: the accept button is off.
    Blank,
    /// A character (or a whole name) `book_folder::is_valid_name` refuses: the entry is marked.
    Refused,
    Fine,
}

/// The prompt's judgement, by the rule every caller applies after it: the text trimmed as
/// `book_folder::trimmed_prompt_name` trims, then `file_names::validate` (not blank, and
/// `book_folder::is_valid_name`).
fn name_verdict(typed: &str) -> NameVerdict {
    let name = book_folder::trimmed_prompt_name(typed);
    if name.chars().all(unicode::is_dotnet_white_space) {
        NameVerdict::Blank
    } else if file_names::validate(Some(name)) {
        NameVerdict::Fine
    } else {
        NameVerdict::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_prompt_judges_as_its_callers_do() {
        assert_eq!(name_verdict("Chapter One"), NameVerdict::Fine);
        assert_eq!(name_verdict("  Chapter One  "), NameVerdict::Fine);
        assert_eq!(name_verdict("Привет, мир"), NameVerdict::Fine);
        assert_eq!(name_verdict("trailing. "), NameVerdict::Fine);
        assert_eq!(name_verdict(""), NameVerdict::Blank);
        assert_eq!(name_verdict("   "), NameVerdict::Blank);
        assert_eq!(name_verdict("\u{00A0}\t"), NameVerdict::Blank);
        assert_eq!(name_verdict("a/b"), NameVerdict::Refused);
        assert_eq!(name_verdict("a:b"), NameVerdict::Refused);
        assert_eq!(name_verdict(".."), NameVerdict::Refused);
        assert_eq!(name_verdict(" . "), NameVerdict::Refused);
    }
}
