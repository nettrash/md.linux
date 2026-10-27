//! The window manager (shell-design §1.2–§1.6): the live windows by `WindowId`, activation routing
//! through md-logic's `ActivationRouter`, open / focus / present / quit, `session.json` and the
//! drafts, the Window menu's rows — and the application's signal handlers, which `app::run`
//! connects to the functions at the bottom of this file.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P1

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use md_logic::seams::WindowId;

use crate::app::Shell;
use crate::book::book_window::BookWindow;
use crate::services::paths::Paths;
use crate::windows::document_window::DocumentWindow;
use crate::windows::host::WindowHost;

/// Every live md window.
pub struct WindowManager {
    session_file: PathBuf,
    drafts_dir: PathBuf,
    windows: RefCell<Vec<Rc<DocumentWindow>>>,
    next_id: Cell<u64>,
}

impl WindowManager {
    pub fn new(paths: &Paths) -> Rc<WindowManager> {
        Rc::new(WindowManager {
            session_file: paths.session_file(),
            drafts_dir: paths.drafts_dir(),
            windows: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
        })
    }

    /// A new untitled document window, with `text` when given (an example, a duplicate, an
    /// import); `dirty` marks it edited.
    pub fn open_untitled(&self, text: Option<String>, dirty: bool) -> WindowId {
        let id = WindowId(self.next_id.get());
        self.next_id.set(id.0 + 1);
        let window = DocumentWindow::new(&Shell::get(), id);
        self.windows.borrow_mut().push(Rc::clone(&window));
        window.window().present();
        id
    }

    /// Opens the document at `path`, or focuses the window that already has it.
    pub fn open_path(&self, path: &str) -> Option<WindowId> {
        if let Some(window) = self.windows.borrow().last() {
            window.window().present();
            return Some(window.id());
        }
        Some(self.open_untitled(None, false))
    }

    pub fn import_text_pack(&self, path: &str) {}

    pub fn import_text_bundle(&self, folder: &str) {}

    pub fn present(&self, id: WindowId) {
        if let Some(host) = self.host(id) {
            host.window().present();
        }
    }

    /// Book ▸ Show Book.
    pub fn show_book(&self) {
        BookWindow::show(&Shell::get());
    }

    pub fn host(&self, id: WindowId) -> Option<Rc<dyn WindowHost>> {
        let window = self.windows.borrow().iter().find(|w| w.id() == id).cloned()?;
        Some(window)
    }

    /// The window with focus, if it is md's.
    pub fn active(&self) -> Option<Rc<dyn WindowHost>> {
        let active = Shell::get().app.active_window()?;
        let window = self.windows.borrow().iter().find(|w| w.window() == active).cloned()?;
        Some(window)
    }

    /// File ▸ Quit: no prompts — flush, drafts, session, then every window goes.
    pub fn quit(&self) {
        Shell::get().app.quit();
    }

    /// A window went away.
    pub fn forget(&self, id: WindowId) {
        self.windows.borrow_mut().retain(|w| w.id() != id);
    }
}

/// `startup`, after the shell's services: actions and accelerators.
pub fn startup(shell: &Rc<Shell>) {
    crate::menus::actions::install_app_actions(shell);
}

/// `activate`: the first launch restores the session (else one untitled window); a later one
/// presents the most recently active window.
pub fn activate(shell: &Rc<Shell>) {
    let last = shell.windows.windows.borrow().last().cloned();
    match last {
        Some(window) => window.window().present(),
        None => {
            shell.windows.open_untitled(None, false);
        }
    }
}

/// `open`: files from the command line, the file manager or a second launch.
pub fn open(shell: &Rc<Shell>, files: &[gio::File], hint: &str) {
    for file in files {
        if let Some(path) = file.path() {
            shell.windows.open_path(&path.to_string_lossy());
        }
    }
}

/// `query-end` (logout, shutdown): Quit's route, synchronously.
pub fn query_end(shell: &Rc<Shell>) {}

/// `handle-local-options`: `--version` prints and exits; `--new-window` hands over to a running md
/// (P1).
pub fn handle_local_options(app: &adw::Application, options: &glib::VariantDict) -> ControlFlow<glib::ExitCode> {
    if options.contains("version") {
        println!("{} {}", md_logic::strings::APP_NAME, env!("CARGO_PKG_VERSION"));
        return ControlFlow::Break(glib::ExitCode::SUCCESS);
    }
    ControlFlow::Continue(())
}
