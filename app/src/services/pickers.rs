//! `GtkPickers` — md-logic's `Pickers` over `gtk::FileDialog` (shell-design §6.1): Open with one
//! filter (never the `*_text_file` variants: md's codec decides the encoding), Save with the
//! `DocumentLoader` choices and the default-extension rule, folders for books, bundles and Move To.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::rc::Rc;

use md_logic::exec::{LocalBoxFuture, ready};
use md_logic::seams::{Alerts, FileTypeChoice, Pickers};

/// The dialogs of one window (modal to it).
pub struct GtkPickers {
    window: gtk::Window,
    /// For the replace question GTK does not ask when md appends the default extension.
    alerts: Rc<dyn Alerts>,
}

impl GtkPickers {
    pub fn new(window: &gtk::Window, alerts: Rc<dyn Alerts>) -> Rc<GtkPickers> {
        Rc::new(GtkPickers { window: window.clone(), alerts })
    }
}

impl Pickers for GtkPickers {
    fn open_files(&self, extensions: Vec<String>) -> LocalBoxFuture<'static, Vec<String>> {
        ready(Vec::new())
    }

    fn save_file(
        &self,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        default_extension: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        ready(None)
    }

    fn pick_folder(&self, title: Option<String>) -> LocalBoxFuture<'static, Option<String>> {
        ready(None)
    }
}
