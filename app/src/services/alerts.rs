//! `AdwAlerts` — md-logic's `Alerts` over `adw::AlertDialog` (shell-design §7.8): one dialog at a
//! time per window (a queue), name prompts with an entry as the extra child, and `open_changed`
//! raised while one is up so the window disables its `win.` actions (§2.2).
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::rc::Rc;

use md_logic::event::Event;
use md_logic::exec::{LocalBoxFuture, ready};
use md_logic::seams::{Alerts, CloseChoice};

/// The alerts of one window.
pub struct AdwAlerts {
    window: gtk::Window,
    open_changed: Event<bool>,
}

impl AdwAlerts {
    pub fn new(window: &gtk::Window) -> Rc<AdwAlerts> {
        Rc::new(AdwAlerts { window: window.clone(), open_changed: Event::new() })
    }

    /// Args: whether a dialog is now showing on this window.
    pub fn open_changed(&self) -> &Event<bool> {
        &self.open_changed
    }
}

impl Alerts for AdwAlerts {
    fn warn(&self, title: String, message: String) -> LocalBoxFuture<'static, ()> {
        ready(())
    }

    fn prompt_name(
        &self,
        title: String,
        message: String,
        initial: String,
        accept_label: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        ready(None)
    }

    fn confirm_delete(&self, title: String, message: String) -> LocalBoxFuture<'static, bool> {
        ready(false)
    }

    fn ask_save_changes(&self, title: String) -> LocalBoxFuture<'static, CloseChoice> {
        ready(CloseChoice::Cancel)
    }

    fn confirm_replace(&self, message: String) -> LocalBoxFuture<'static, bool> {
        ready(false)
    }
}
