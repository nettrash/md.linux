//! `BookWindow` — the one app-wide Book window (shell-design §8.1, §8.4–§8.6):
//! `adw::ToolbarView` (header bar with the detail toolbar, then the menu bar) → `gtk::Overlay` →
//! `gtk::Paned` (sidebar 200–320 px, detail); the article's `TextFileSession`, navigator, stepper
//! and outputs. Created on demand by Show Book.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P5

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use md_logic::commands::ShellSnapshot;
use md_logic::exec::{LocalBoxFuture, Spawner};
use md_logic::seams::{Alerts, Pickers, WindowId};
use md_logic::windows::window_placement;

use crate::app::Shell;
use crate::services::alerts::AdwAlerts;
use crate::services::pickers::GtkPickers;
use crate::windows::host::{WindowHost, WindowKind};

thread_local! {
    static BOOK_WINDOW: RefCell<Option<Rc<BookWindow>>> = const { RefCell::new(None) };
}

/// The Book window.
pub struct BookWindow {
    window: adw::ApplicationWindow,
    overlay: gtk::Overlay,
    alerts: Rc<AdwAlerts>,
    pickers: Rc<GtkPickers>,
}

impl BookWindow {
    /// Creates the window on first use and presents it.
    pub fn show(shell: &Rc<Shell>) -> Rc<BookWindow> {
        let existing = BOOK_WINDOW.with(|w| w.borrow().clone());
        let book = existing.unwrap_or_else(|| {
            let book = BookWindow::new(shell);
            BOOK_WINDOW.with(|w| *w.borrow_mut() = Some(Rc::clone(&book)));
            book
        });
        book.window.present();
        book
    }

    fn new(shell: &Rc<Shell>) -> Rc<BookWindow> {
        let size = window_placement::BOOK_DEFAULT;
        let title = md_logic::strings::BOOK;
        let header = adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new(title, "")).build();
        let overlay = gtk::Overlay::new();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&overlay));
        let window = adw::ApplicationWindow::builder()
            .application(&shell.app)
            .title(title)
            .default_width(size.width)
            .default_height(size.height)
            .content(&toolbar)
            .build();
        window.connect_close_request(|_| {
            BOOK_WINDOW.with(|w| w.borrow_mut().take());
            glib::Propagation::Proceed
        });
        let toplevel = window.clone().upcast::<gtk::Window>();
        let alerts = AdwAlerts::new(&toplevel);
        let pickers = GtkPickers::new(&toplevel, alerts.clone());
        Rc::new(BookWindow { window, overlay, alerts, pickers })
    }
}

impl WindowHost for BookWindow {
    /// S0 stub: id 0, which the window manager never hands out.
    fn id(&self) -> WindowId {
        WindowId(0)
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Book
    }

    fn window(&self) -> gtk::Window {
        self.window.clone().upcast()
    }

    fn overlay(&self) -> gtk::Overlay {
        self.overlay.clone()
    }

    fn alerts(&self) -> Rc<dyn Alerts> {
        self.alerts.clone()
    }

    fn pickers(&self) -> Rc<dyn Pickers> {
        self.pickers.clone()
    }

    fn track_output(&self, output: LocalBoxFuture<'static, ()>) {
        Shell::get().spawner.spawn_local(output);
    }

    fn snapshot(&self) -> ShellSnapshot {
        ShellSnapshot { is_book_window: true, ..ShellSnapshot::empty() }
    }

    fn publish(&self) {}

    fn is_dark(&self) -> bool {
        crate::theme::is_dark()
    }
}
