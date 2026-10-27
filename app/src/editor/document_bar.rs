//! `DocumentBar` — the conflict and save-error bar above the footer (shell-design §6.3): one
//! `gtk::Revealer` holding a `gtk::Box` (class `md-bar`) with the warning icon, the message and two
//! buttons. `GtkInfoBar` is deprecated since GTK 4.10 and `adw::Banner` carries one button only.
//! Routine autosave shows nothing; the window decides when to show or hide it.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use md_logic::strings::{buttons, documents};

type Action = Rc<dyn Fn()>;

/// One window's bar slot.
pub struct DocumentBar {
    revealer: gtk::Revealer,
    message: gtk::Label,
    first: gtk::Button,
    second: gtk::Button,
    actions: Rc<RefCell<(Option<Action>, Option<Action>)>>,
}

impl DocumentBar {
    pub fn new() -> Rc<DocumentBar> {
        let icon = gtk::Image::from_icon_name("dialog-warning-symbolic");
        let message = gtk::Label::builder().hexpand(true).xalign(0.0).wrap(true).build();
        let first = gtk::Button::new();
        let second = gtk::Button::new();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        row.append(&icon);
        row.append(&message);
        row.append(&first);
        row.append(&second);
        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("md-bar");
        bar.append(&row);
        let revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .child(&bar)
            .reveal_child(false)
            .visible(false)
            .build();

        let actions: Rc<RefCell<(Option<Action>, Option<Action>)>> = Rc::new(RefCell::new((None, None)));
        let slots = Rc::clone(&actions);
        first.connect_clicked(move |_| {
            let action = slots.borrow().0.clone();
            if let Some(action) = action {
                action();
            }
        });
        let slots = Rc::clone(&actions);
        second.connect_clicked(move |_| {
            let action = slots.borrow().1.clone();
            if let Some(action) = action {
                action();
            }
        });
        Rc::new(DocumentBar { revealer, message, first, second, actions })
    }

    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().upcast()
    }

    /// "The file changed on disk." — Reload from Disk / Keep My Version.
    pub fn show_conflict(&self, on_reload: impl Fn() + 'static, on_keep: impl Fn() + 'static) {
        self.show(
            documents::FILE_CHANGED_ON_DISK,
            buttons::RELOAD_FROM_DISK,
            on_reload,
            Some((buttons::KEEP_MY_VERSION, Rc::new(on_keep))),
        );
    }

    /// `could_not_save(err)` — Retry / Save As….
    pub fn show_save_error(&self, text: &str, on_retry: impl Fn() + 'static, on_save_as: impl Fn() + 'static) {
        self.show(text, buttons::RETRY, on_retry, Some((buttons::SAVE_AS, Rc::new(on_save_as))));
    }

    /// `could_not_save(err)` with Retry alone — a book article, whose file is its place in the
    /// book, has no Save As (shell-design §8.5).
    pub fn show_save_error_retry_only(&self, text: &str, on_retry: impl Fn() + 'static) {
        self.show(text, buttons::RETRY, on_retry, None);
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.revealer.set_visible(false);
        *self.actions.borrow_mut() = (None, None);
    }

    pub fn is_shown(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// The message and the button labels, as shown ("" for a hidden second button).
    pub fn texts(&self) -> (String, String, String) {
        let second = if self.second.is_visible() {
            self.second.label().map(|l| l.to_string()).unwrap_or_default()
        } else {
            String::new()
        };
        (self.message.text().to_string(), self.first.label().map(|l| l.to_string()).unwrap_or_default(), second)
    }

    /// Clicks the first (`true`) or the second button, as a pointer would.
    pub fn press(&self, first: bool) {
        if first {
            self.first.emit_clicked();
        } else {
            self.second.emit_clicked();
        }
    }

    fn show(&self, message: &str, first: &str, on_first: impl Fn() + 'static, second: Option<(&str, Action)>) {
        self.message.set_text(message);
        self.first.set_label(first);
        let on_second = second.map(|(label, action)| {
            self.second.set_label(label);
            action
        });
        self.second.set_visible(on_second.is_some());
        *self.actions.borrow_mut() = (Some(Rc::new(on_first)), on_second);
        self.revealer.set_visible(true);
        self.revealer.set_reveal_child(true);
    }
}
