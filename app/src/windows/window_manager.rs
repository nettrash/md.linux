//! The window manager (shell-design §1.2–§1.6): the live windows by `WindowId`, activation routing
//! through md-logic's `ActivationRouter`, open / focus / present / quit, `session.json` and the
//! drafts, the Window menu's rows — and the application's signal handlers, which `app::run`
//! connects to the functions at the bottom of this file, and the `app.` commands
//! ([`run_app_command`]).
//!
//! Three routes end a window, one policy (§1.5): a window's own close (its close policy, then
//! [`WindowManager::close_window`]); Quit ([`WindowManager::quit`]: nothing asked — saved documents
//! flushed, untitled dirty ones kept as drafts, `session.json` written, then every window goes);
//! and the session's end (`query-end`: the same flush and session write, synchronously).

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use adw::prelude::*;
use md_logic::activation::{ActivationAction, ActivationDescription, ActivationItem, activation_router};
use md_logic::commands::{CommandArgument, CommandId, RecentEntry, WindowTitle};
use md_logic::documents::document_loader;
use md_logic::exec::Spawner;
use md_logic::seams::{DocumentRegistry, WindowId};
use md_logic::settings::{SettingsStore, TypingSettings, settings_keys, wrap_selection_setting};
use md_logic::strings;
use md_logic::windows::{SessionBook, SessionState, UntitledNames, WindowRect};

use crate::app::Shell;
use crate::book::book_window::BookWindow;
use crate::book::{self, BookOutputKind};
use crate::menus::{about, actions};
use crate::services::document_ops;
use crate::services::paths::Paths;
use crate::theme;
use crate::windows::document_window::{DocumentWindow, OUTPUT_WAIT};
use crate::windows::drafts::Drafts;
use crate::windows::host::WindowHost;
use crate::windows::session::{Restore, SessionFile, restore_plan};

/// Every live md window.
pub struct WindowManager {
    session_file: SessionFile,
    drafts: Drafts,
    windows: RefCell<Vec<Rc<DocumentWindow>>>,
    next_id: Cell<u64>,
    untitled: RefCell<UntitledNames>,
    recent: RefCell<Vec<RecentEntry>>,
    /// No activation has been handled yet: the next one is the process's first (§1.3).
    first: Cell<bool>,
    /// `--new-window` on the primary: the first activation adds an untitled window.
    new_window: Cell<bool>,
    quitting: Cell<bool>,
}

impl WindowManager {
    pub fn new(paths: &Paths) -> Rc<WindowManager> {
        Rc::new(WindowManager {
            session_file: SessionFile::new(paths.session_file()),
            drafts: Drafts::new(paths.drafts_dir()),
            windows: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
            untitled: RefCell::new(UntitledNames::new()),
            recent: RefCell::new(Vec::new()),
            first: Cell::new(true),
            new_window: Cell::new(false),
            quitting: Cell::new(false),
        })
    }

    // ---- the frozen interface (docs/shell-work-packages.md §3.2) -------------------------------

    /// A new untitled document window, with `text` when given (an example, a duplicate, an
    /// import); `dirty` marks it edited.
    pub fn open_untitled(&self, text: Option<String>, dirty: bool) -> WindowId {
        self.open_untitled_titled(text, dirty, None)
    }

    /// Opens the document at `path`, or focuses the window that already has it.
    pub fn open_path(&self, path: &str) -> Option<WindowId> {
        let shell = Shell::get();
        if let Some(id) = shell.registry.find_by_path(path, shell.identity.as_ref()) {
            self.present(id);
            return Some(id);
        }
        self.open_in_new_window(path)
    }

    /// A `.textpack`: an untitled document titled after the pack (§6.5).
    pub fn import_text_pack(&self, path: &str) {
        self.open_in_new_window(path);
    }

    /// A `.textbundle` folder: an untitled document titled after the bundle (§6.5).
    pub fn import_text_bundle(&self, folder: &str) {
        self.open_in_new_window(folder);
    }

    pub fn present(&self, id: WindowId) {
        #[cfg(feature = "selftest")]
        probe::log(format!("Present {}", id.0));
        if let Some(host) = self.host(id) {
            host.window().present();
        }
    }

    /// Book ▸ Show Book.
    pub fn show_book(&self) {
        BookWindow::show(&Shell::get());
    }

    /// `WindowId(0)` is the Book window ([`book::BOOK_WINDOW_ID`]); document windows count from 1.
    pub fn host(&self, id: WindowId) -> Option<Rc<dyn WindowHost>> {
        if id == book::BOOK_WINDOW_ID {
            return self.book_host();
        }
        let window = self.windows.borrow().iter().find(|w| w.id() == id).cloned()?;
        Some(window)
    }

    /// The window with focus, if it is md's.
    pub fn active(&self) -> Option<Rc<dyn WindowHost>> {
        let active = Shell::get().app.active_window()?;
        self.host_for(&active)
    }

    /// File ▸ Quit: no prompts — flush, drafts, session, then every window goes (§1.5 route 2).
    pub fn quit(&self) {
        if self.quitting.replace(true) {
            return;
        }
        self.prepare_quit();
        let windows = self.documents();
        let book = BookWindow::current();
        let shell = Shell::get();
        let app = shell.app.clone();
        shell.spawner.spawn_local(Box::pin(async move {
            let deadline = Instant::now() + OUTPUT_WAIT;
            for window in &windows {
                window.wait_outputs(deadline).await;
            }
            if let Some(book) = &book {
                book.wait_outputs(deadline).await;
            }
            for window in windows {
                window.teardown();
                window.adw_window().destroy();
            }
            if let Some(book) = book {
                book.window().destroy();
            }
            app.quit();
        }));
    }

    // ---- the rest of the manager ---------------------------------------------------------------

    pub fn drafts(&self) -> &Drafts {
        &self.drafts
    }

    /// The live document windows, in creation order.
    pub fn documents(&self) -> Vec<Rc<DocumentWindow>> {
        self.windows.borrow().clone()
    }

    /// The document window `id`, while it is open.
    pub fn document(&self, id: WindowId) -> Option<Rc<DocumentWindow>> {
        self.windows.borrow().iter().find(|w| w.id() == id).cloned()
    }

    /// The md window behind a toplevel.
    pub fn host_for(&self, window: &gtk::Window) -> Option<Rc<dyn WindowHost>> {
        if let Some(document) =
            self.windows.borrow().iter().find(|w| w.adw_window().upcast_ref::<gtk::Window>() == window).cloned()
        {
            return Some(document);
        }
        self.book_host().filter(|book| &book.window() == window)
    }

    /// The Book window while it is open — however it was shown (Show Book, a library flow, a
    /// restored session).
    fn book_host(&self) -> Option<Rc<dyn WindowHost>> {
        BookWindow::current().map(|book| book as Rc<dyn WindowHost>)
    }

    /// The window an app command's dialogs belong to: the focused md window, else the one most
    /// recently focused.
    fn parent(&self) -> Option<Rc<dyn WindowHost>> {
        self.active().or_else(|| Shell::get().app.windows().iter().find_map(|w| self.host_for(w)))
    }

    /// An untitled window titled `title` ("{name} copy" for a duplicate), else the next
    /// "Untitled N".
    pub fn open_untitled_titled(&self, text: Option<String>, dirty: bool, title: Option<String>) -> WindowId {
        let window = self.create();
        let (title, ordinal) = match title {
            Some(title) => (title, None),
            None => {
                let (name, ordinal) = self.untitled.borrow_mut().take_name();
                (name, Some(ordinal))
            }
        };
        window.open_untitled(&text.unwrap_or_default(), &title, ordinal, dirty);
        window.load_into_view();
        window.show();
        window.id()
    }

    /// File ▸ Duplicate: a new untitled, dirty window holding `text`, titled "{name} copy" after
    /// the document it came from (§6.4) — from a document window or the Book window's article.
    pub fn duplicate(&self, text: String, name: &str) -> WindowId {
        self.open_untitled_titled(Some(text), true, Some(strings::duplicate_title(name)))
    }

    /// "Untitled N" is free again.
    pub fn release_untitled(&self, ordinal: usize) {
        self.untitled.borrow_mut().release(ordinal);
    }

    fn create(&self) -> Rc<DocumentWindow> {
        let id = WindowId(self.next_id.get());
        self.next_id.set(id.0 + 1);
        let shell = Shell::get();
        let window = DocumentWindow::new(&shell, id);
        self.windows.borrow_mut().push(Rc::clone(&window));
        shell.registry.add(id, strings::UNTITLED);
        window
    }

    /// A window that never showed (its open failed) goes again without a trace.
    fn discard(&self, window: &Rc<DocumentWindow>) {
        window.teardown();
        self.windows.borrow_mut().retain(|w| w.id() != window.id());
        window.adw_window().destroy();
    }

    fn open_in_new_window(&self, path: &str) -> Option<WindowId> {
        let window = self.create();
        match window.open_file(path) {
            Ok(()) => {
                window.load_into_view();
                window.show();
                Some(window.id())
            }
            Err(alerts) => {
                self.discard(&window);
                for (title, message) in alerts {
                    self.warn(title, message);
                }
                None
            }
        }
    }

    /// An alert with no window of its own: on the most recent md window, or — when there is none,
    /// as on a first launch whose only file failed — on a fresh untitled window, so the process
    /// never vanishes without a word.
    fn warn(&self, title: String, message: String) {
        let parent = match self.parent() {
            Some(parent) => parent,
            None => {
                let id = self.open_untitled(None, false);
                match self.host(id) {
                    Some(host) => host,
                    None => return,
                }
            }
        };
        let shown = parent.alerts().warn(title, message);
        Shell::get().spawner.spawn_local(shown);
    }

    /// A document window's close policy finished: forget it, record the session, destroy it.
    pub fn close_window(&self, window: &Rc<DocumentWindow>) {
        window.teardown();
        self.windows.borrow_mut().retain(|w| w.id() != window.id());
        if !self.quitting.get() {
            self.write_session();
        }
        window.adw_window().destroy();
    }

    /// Quit's and `query-end`'s synchronous half: every document flushed or kept as a draft, and
    /// `session.json` naming them all.
    pub fn prepare_quit(&self) {
        for window in self.documents() {
            window.quit_flush();
        }
        if let Some(book) = BookWindow::current() {
            book.quit_flush();
        }
        self.write_session();
    }

    /// `session.json` for the windows open now (§1.6).
    pub fn write_session(&self) {
        let windows = self.documents().iter().filter_map(|w| w.session_entry()).collect();
        let book = self.book_host().map(|book| {
            let window = book.window();
            let (width, height) = window.default_size();
            SessionBook::new(true, WindowRect::new(0, 0, width, height), window.is_maximized())
        });
        self.session_file.save(&SessionState::new(windows, book));
    }

    /// Brings back what `session.json` lists and the drafts it does not (§1.6). `drafts_only` for
    /// an `open` as the first activation. Returns how many windows came back.
    fn restore_session(&self, drafts_only: bool) -> usize {
        let state = self.session_file.load();
        let mut restored = 0;
        for item in restore_plan(&state, &self.drafts, drafts_only) {
            let window = match &item {
                Restore::Document(row) => {
                    let window = self.create();
                    if let Err(alerts) = window.open_file(&row.path) {
                        self.discard(&window);
                        for (title, message) in alerts {
                            eprintln!("md: not restored: {title} {message}");
                        }
                        continue;
                    }
                    window
                }
                Restore::Draft(_) | Restore::Orphan(_) => {
                    let path = item.path();
                    let Some(text) = self.drafts.read(&path) else { continue };
                    let window = self.create();
                    let (title, ordinal) = self.untitled.borrow_mut().take_name();
                    window.open_draft(path, &text, &title, Some(ordinal));
                    window
                }
            };
            window.load_into_view();
            if let Restore::Document(row) | Restore::Draft(row) = &item {
                window.restore_view(row);
            }
            window.show();
            restored += 1;
        }
        if !drafts_only && state.book.is_some_and(|b| b.open) {
            self.show_book();
            restored += 1;
        }
        restored
    }

    /// The actions an activation asked for, in order (§1.3).
    fn execute(&self, actions: Vec<ActivationAction>) {
        for action in actions {
            #[cfg(feature = "selftest")]
            probe::log(format!("{action:?}"));
            match action {
                ActivationAction::OpenPath(path) => {
                    self.open_path(&path);
                }
                ActivationAction::ImportTextPack(path) => self.import_text_pack(&path),
                ActivationAction::ImportTextBundleFolder(path) => self.import_text_bundle(&path),
                ActivationAction::Focus(id) => self.present(id),
                ActivationAction::OpenUntitled => {
                    self.open_untitled(None, false);
                }
                ActivationAction::RestoreSession => {
                    if self.restore_session(false) == 0 {
                        self.open_untitled(None, false);
                    }
                }
            }
        }
    }

    /// Files from the command line, the file manager, a second launch, a drop or Open….
    pub fn open_files(&self, files: &[gio::File]) {
        self.open_files_with(files, false);
    }

    fn open_files_with(&self, files: &[gio::File], is_first: bool) {
        let mut items = Vec::new();
        let mut unreadable = Vec::new();
        for file in files {
            match document_ops::activation_item(file) {
                Some(item) => items.push(item),
                None => unreadable.push(file.uri().to_string()),
            }
        }
        self.route_items(items, is_first);
        for location in unreadable {
            self.warn(strings::documents::COULD_NOT_OPEN.to_string(), location);
        }
    }

    /// Paths from a dialog or the recent list.
    fn open_paths(&self, paths: Vec<String>) {
        let items = paths
            .iter()
            .map(
                |path| if Path::new(path).is_dir() { ActivationItem::folder(path) } else { ActivationItem::file(path) },
            )
            .collect();
        self.route_items(items, false);
    }

    fn route_items(&self, items: Vec<ActivationItem>, is_first: bool) {
        let shell = Shell::get();
        let description = ActivationDescription::for_files(items, is_first, false);
        let actions = activation_router::resolve(
            activation_router::route(&description),
            shell.registry.as_ref(),
            shell.identity.as_ref(),
        );
        self.execute(actions);
    }

    /// Open Recent's rows as of the last `changed` (§6.6).
    pub fn recent_entries(&self) -> Vec<RecentEntry> {
        self.recent.borrow().clone()
    }

    /// The Window menu's rows for window `me` (§2.8): every registry row, then the Book window.
    pub fn window_titles(&self, me: WindowId) -> Vec<WindowTitle> {
        let shell = Shell::get();
        let mut rows: Vec<WindowTitle> =
            shell.registry.windows().into_iter().map(|(id, title)| WindowTitle::new(id, title, id == me)).collect();
        if let Some(book) = self.book_host()
            && !rows.iter().any(|row| row.id == book.id())
        {
            let title = book.window().title().map(|t| t.to_string()).unwrap_or_else(|| strings::BOOK.to_string());
            rows.push(WindowTitle::new(book.id(), title, book.id() == me));
        }
        rows
    }

    /// Every window publishes its snapshot again (an app-wide value changed).
    pub fn publish_all(&self) {
        for window in self.documents() {
            window.publish();
        }
        if let Some(book) = self.book_host() {
            book.publish();
        }
    }

    fn refresh_recent(&self) {
        let entries = Shell::get().recent.entries();
        *self.recent.borrow_mut() = entries;
    }
}

// ---- the application's signals -------------------------------------------------------------------

/// `startup`, after the shell's services: actions and accelerators, and the app-wide events every
/// window's snapshot reads.
pub fn startup(shell: &Rc<Shell>) {
    // A broken install renders text without math or diagrams; say where the engines should be.
    let rich = shell.paths.rich_dir();
    if !rich.join("md-init.js").is_file() {
        eprintln!("md: {} is missing; the preview cannot render math, code or diagrams", rich.display());
    }
    actions::install_app_actions(shell);
    shell.windows.refresh_recent();
    shell.settings.changed().connect(|_| Shell::get().windows.publish_all());
    shell.recent.changed().connect(|_| {
        let windows = &Shell::get().windows;
        windows.refresh_recent();
        windows.publish_all();
    });
    shell.book.changed().connect(|_| Shell::get().windows.publish_all());
    shell.registry.changed().connect(|_| Shell::get().windows.publish_all());
    theme::changed().connect(|dark| {
        for window in Shell::get().windows.documents() {
            window.on_theme(*dark);
        }
    });
}

/// `activate`: the first launch restores the session (else one untitled window); a later one
/// presents the most recently active window (§1.3).
pub fn activate(shell: &Rc<Shell>) {
    let windows = &shell.windows;
    if windows.first.replace(false) {
        let state = windows.session_file.load();
        let restorable = !restore_plan(&state, &windows.drafts, false).is_empty() || state.book.is_some_and(|b| b.open);
        let description = ActivationDescription::for_launch(&[], true, restorable);
        windows.execute(activation_router::route(&description));
        if windows.new_window.replace(false) {
            windows.open_untitled(None, false);
        }
    } else {
        // GTK keeps the list most recently focused first.
        match shell.app.windows().first() {
            Some(window) => {
                #[cfg(feature = "selftest")]
                probe::log(String::from("PresentMostRecent"));
                window.present();
            }
            None => {
                windows.open_untitled(None, false);
            }
        }
    }
    #[cfg(feature = "selftest")]
    probe::schedule();
}

/// `open`: files from the command line, the file manager or a second launch. As the first
/// activation it brings back the drafts (never the saved documents) first, so unsaved text is never
/// left behind because the user double-clicked another file.
pub fn open(shell: &Rc<Shell>, files: &[gio::File], _hint: &str) {
    let windows = &shell.windows;
    let first = windows.first.replace(false);
    if first {
        windows.restore_session(true);
    }
    windows.open_files_with(files, first);
    if shell.app.windows().is_empty() {
        windows.open_untitled(None, false);
    }
    #[cfg(feature = "selftest")]
    probe::schedule();
}

/// `query-end` (logout, shutdown): Quit's route, synchronously — nothing awaited, nothing asked.
pub fn query_end(shell: &Rc<Shell>) {
    shell.windows.prepare_quit();
}

/// `handle-local-options`: `--version` prints and exits; `--new-window` hands over to a running md,
/// or makes the first activation add an untitled window.
pub fn handle_local_options(app: &adw::Application, options: &glib::VariantDict) -> ControlFlow<glib::ExitCode> {
    if options.contains("version") {
        println!("{} {}", strings::APP_NAME, env!("CARGO_PKG_VERSION"));
        return ControlFlow::Break(glib::ExitCode::SUCCESS);
    }
    if options.contains("new-window") {
        if let Err(err) = app.register(gio::Cancellable::NONE) {
            eprintln!("md: {err}");
            return ControlFlow::Break(glib::ExitCode::FAILURE);
        }
        if app.is_remote() {
            gio::prelude::ActionGroupExt::activate_action(app, "new", None);
            // The call is a D-Bus message: send it before this process returns.
            if let Some(connection) = app.dbus_connection() {
                let _ = connection.flush_sync(gio::Cancellable::NONE);
            }
            return ControlFlow::Break(glib::ExitCode::SUCCESS);
        }
        Shell::get().windows.new_window.set(true);
    }
    ControlFlow::Continue(())
}

// ---- the app. commands -----------------------------------------------------------------------------

/// Runs an `app.` command (§2.2): New … About, with the focused md window as the parent of any
/// dialog. The book outputs run in the Book window, by its own dispatcher.
pub fn run_app_command(id: CommandId, argument: CommandArgument) {
    #[cfg(feature = "selftest")]
    {
        probe::log(format!("app.{id:?}"));
        probe::schedule();
    }
    let shell = Shell::get();
    let windows = &shell.windows;
    let parent = windows.parent();
    let settings = shell.settings.as_ref();
    let spawn = |future: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>| shell.spawner.spawn_local(future);
    use CommandId as C;
    match id {
        C::New => {
            windows.open_untitled(None, false);
        }
        C::Open => {
            if let Some(parent) = parent {
                let chosen = parent.pickers().open_files(document_loader::open_extensions());
                spawn(Box::pin(async move {
                    let paths = chosen.await;
                    Shell::get().windows.open_paths(paths);
                }));
            }
        }
        C::OpenRecentEntry => {
            if let CommandArgument::Text(token) = argument {
                match shell.recent.resolve(&token) {
                    Some(path) => windows.open_paths(vec![path]),
                    None => {
                        let name =
                            windows.recent_entries().into_iter().find(|e| e.token == token).map_or(token, |e| e.name);
                        windows.warn(
                            strings::documents::COULD_NOT_OPEN.to_string(),
                            strings::documents::file_not_found(&name),
                        );
                    }
                }
            }
        }
        C::ClearRecent => shell.recent.clear(),
        C::OpenTextBundleFolder => {
            if let Some(parent) = parent {
                let chosen = parent.pickers().pick_folder(None);
                spawn(Box::pin(async move {
                    if let Some(folder) = chosen.await {
                        Shell::get().windows.open_paths(vec![folder]);
                    }
                }));
            }
        }
        C::Example => {
            if let CommandArgument::Text(file_name) = argument
                && let Some(text) = shell.examples.read(&file_name)
            {
                windows.open_untitled(Some(text), true);
            }
        }
        C::ExampleBook => book::unpack_example_book(&shell, parent),
        C::NewBook => book::new_book(&shell, parent),
        C::OpenBook => book::open_book(&shell, parent),
        C::PdfPageSize => {
            if let CommandArgument::Text(page_size) = argument {
                settings.set_string(settings_keys::PDF_PAGE_SIZE, &page_size);
            }
        }
        C::Quit => windows.quit(),
        C::ContinueLists => TypingSettings::toggle(settings, settings_keys::CONTINUE_LISTS),
        C::CapitalizeSentences => TypingSettings::toggle(settings, settings_keys::CAPITALIZE_SENTENCES),
        C::WrapSelection => wrap_selection_setting::toggle(settings),
        C::ShowBook => windows.show_book(),
        C::CloseBook => book::close_book(&shell),
        C::PrintBook => book::run_book_output(&shell, parent, BookOutputKind::Print),
        C::ExportBookPdf => book::run_book_output(&shell, parent, BookOutputKind::Pdf),
        C::ExportBookEpub => book::run_book_output(&shell, parent, BookOutputKind::Epub),
        C::ExportBookLaTeX => book::run_book_output(&shell, parent, BookOutputKind::LaTeX),
        C::Help | C::PrivacyPolicy => {
            let url = if id == C::Help { strings::help::SUPPORT_URL } else { strings::help::PRIVACY_URL };
            let window = parent.map(|p| p.window());
            gtk::UriLauncher::new(url).launch(window.as_ref(), gio::Cancellable::NONE, |result| {
                if let Err(err) = result {
                    eprintln!("md: could not open {err}");
                }
            });
        }
        C::About => {
            if let Some(parent) = parent {
                about::show_about(&parent.window());
            }
        }
        _ => {}
    }
}

/// `MD_SELFTEST_PROBE=FILE` (self-test builds only): after each activation the process writes what
/// it did and which windows it has to FILE, so a self-test can check a second `md` process — a
/// relaunch, a hand-off — from the outside.
#[cfg(feature = "selftest")]
pub mod probe {
    use std::cell::RefCell;
    use std::fmt::Write as _;

    use super::*;

    thread_local! {
        static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    pub fn log(line: String) {
        LOG.with(|log| log.borrow_mut().push(line));
    }

    /// Writes the probe file on the next idle, once the activation's windows exist.
    pub fn schedule() {
        let Some(path) = std::env::var_os("MD_SELFTEST_PROBE") else { return };
        glib::idle_add_local_once(move || {
            let text = report();
            if let Err(err) = std::fs::write(&path, text) {
                eprintln!("md: probe {}: {err}", std::path::Path::new(&path).display());
            }
        });
    }

    fn json(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                c if (c as u32) < 0x20 => {
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    /// `{"pid": …, "activations": [...], "windows": [{"title", "path", "draft", "mode", "zen", "text"}]}`.
    pub fn report() -> String {
        let activations: Vec<String> = LOG.with(|log| log.borrow().iter().map(|l| json(l)).collect());
        let windows: Vec<String> = Shell::get()
            .windows
            .documents()
            .iter()
            .map(|w| {
                let title = w.adw_window().title().map(|t| t.to_string()).unwrap_or_default();
                let (mode, zen, _) = w.view_state();
                format!(
                    "{{\"title\": {}, \"path\": {}, \"draft\": {}, \"mode\": {}, \"zen\": {zen}, \"text\": {}}}",
                    json(&title),
                    json(&w.path().unwrap_or_default()),
                    json(&w.draft_path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
                    json(mode.raw_value()),
                    json(&w.text()),
                )
            })
            .collect();
        format!(
            "{{\"pid\": {}, \"activations\": [{}], \"windows\": [{}]}}\n",
            std::process::id(),
            activations.join(", "),
            windows.join(", ")
        )
    }
}
