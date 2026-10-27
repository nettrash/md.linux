//! The document window (shell-design §1.4, §5.1, §6.3, §6.4, §7): `adw::ToolbarView` with the
//! header bar and the menu bar on top, content `gtk::Overlay` → the panes, the bar slot and the
//! footer; its `TextFileSession`, `DocumentWindowState`, `CommandDispatcher`, preview coordinator
//! and exports. The S0 stub is the empty paper window.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use std::rc::Rc;

use adw::prelude::*;
use md_logic::commands::ShellSnapshot;
use md_logic::exec::{LocalBoxFuture, Spawner};
use md_logic::seams::{Alerts, Pickers, WindowId};
use md_logic::settings::{SettingsStore, settings_keys};
use md_logic::windows::window_placement;

use crate::app::Shell;
use crate::services::alerts::AdwAlerts;
use crate::services::pickers::GtkPickers;
use crate::windows::host::{WindowHost, WindowKind};

/// One document's window.
pub struct DocumentWindow {
    id: WindowId,
    window: adw::ApplicationWindow,
    overlay: gtk::Overlay,
    alerts: Rc<AdwAlerts>,
    pickers: Rc<GtkPickers>,
}

impl DocumentWindow {
    pub fn new(shell: &Rc<Shell>, id: WindowId) -> Rc<DocumentWindow> {
        let size = window_placement::parse_size(
            shell.settings.get_string(settings_keys::DOCUMENT_WINDOW_SIZE).as_deref(),
            window_placement::DOCUMENT_DEFAULT,
        );
        let title = md_logic::strings::UNTITLED;
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
        window.connect_close_request(move |_| {
            Shell::get().windows.forget(id);
            glib::Propagation::Proceed
        });
        let toplevel = window.clone().upcast::<gtk::Window>();
        let alerts = AdwAlerts::new(&toplevel);
        let pickers = GtkPickers::new(&toplevel, alerts.clone());
        Rc::new(DocumentWindow { id, window, overlay, alerts, pickers })
    }
}

impl WindowHost for DocumentWindow {
    fn id(&self) -> WindowId {
        self.id
    }

    fn kind(&self) -> WindowKind {
        WindowKind::Document
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
        ShellSnapshot { has_document: true, ..ShellSnapshot::empty() }
    }

    fn publish(&self) {}

    fn is_dark(&self) -> bool {
        crate::theme::is_dark()
    }
}
