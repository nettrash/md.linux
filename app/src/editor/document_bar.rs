//! `DocumentBar` — the conflict and save-error bar above the footer (shell-design §6.3): one
//! `gtk::Revealer` holding a `gtk::Box` (class `md-bar`) with the message and two buttons.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P3

use std::rc::Rc;

/// One window's bar slot.
pub struct DocumentBar {
    revealer: gtk::Revealer,
}

impl DocumentBar {
    pub fn new() -> Rc<DocumentBar> {
        Rc::new(DocumentBar { revealer: gtk::Revealer::new() })
    }

    pub fn widget(&self) -> gtk::Widget {
        self.revealer.clone().into()
    }

    /// "The file changed on disk." — Reload from Disk / Keep My Version.
    pub fn show_conflict(&self, on_reload: impl Fn() + 'static, on_keep: impl Fn() + 'static) {}

    /// `could_not_save(err)` — Retry / Save As….
    pub fn show_save_error(&self, text: &str, on_retry: impl Fn() + 'static, on_save_as: impl Fn() + 'static) {}

    pub fn hide(&self) {}
}
