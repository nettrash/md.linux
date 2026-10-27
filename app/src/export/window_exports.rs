//! `WindowExports` — one window's exports and print (shell-design §7): md-logic's `ExportPipeline`
//! over this window's overlay renderers, pickers and alerts; outputs handed to the window's
//! `track_output`; the footer spinner through `busy_changed`; and the sink md-logic's `BookOutput`
//! renders a book into.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::rc::{Rc, Weak};

use md_logic::event::Event;

use crate::app::Shell;
use crate::windows::host::WindowHost;

/// md-logic pending: `md_logic::books::BookOutputs` (md.win `IBookOutputs`: export PDF, print,
/// EPUB and LaTeX of a compiled book — no share on Linux). Replaced by md-logic's trait when its
/// `books` module lands.
pub trait BookOutputs {}

/// The exports of one window.
pub struct WindowExports {
    host: Weak<dyn WindowHost>,
    busy_changed: Event<bool>,
}

/// The S0 stub's book sink: does nothing.
struct NoBookOutputs;

impl BookOutputs for NoBookOutputs {}

impl WindowExports {
    pub fn new(shell: &Rc<Shell>, host: Weak<dyn WindowHost>) -> Rc<WindowExports> {
        Rc::new(WindowExports { host, busy_changed: Event::new() })
    }

    /// File ▸ Print…: GTK's print dialog over the paper HTML.
    pub fn print(&self, source: String, title: String) {}

    /// File ▸ Export ▸ PDF… at `md.pdfPageSize`.
    pub fn export_pdf(&self, source: String, title: String) {}

    pub fn export_html(&self, source: String, title: String) {}

    pub fn export_epub(&self, source: String, file_name: String, document_path: Option<String>) {}

    pub fn export_latex(&self, source: String, title: String) {}

    pub fn export_text_bundle(&self, source: String, document_path: Option<String>, title: String) {}

    /// The diagram's `ordinal`; the pipeline re-resolves it from the current text.
    pub fn export_diagram_svg(&self, source: String, title: String, ordinal: u32) {}

    /// md-logic `BookOutput`'s sink, rendering in this window.
    pub fn book_outputs(&self) -> Rc<dyn BookOutputs> {
        Rc::new(NoBookOutputs)
    }

    /// Args: whether an export is running (500 ms in, md-logic's `BusyRing`).
    pub fn busy_changed(&self) -> &Event<bool> {
        &self.busy_changed
    }
}
