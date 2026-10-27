//! The document window (shell-design §1.4, §5.1, §6.3, §6.4, §7): `adw::ToolbarView` with the
//! header bar and the menu bar on top, content `gtk::Overlay` → the panes, the bar slot and the
//! footer; its `TextFileSession`, `DocumentWindowState`, `CommandDispatcher`, preview coordinator
//! and exports.
//!
//! Wiring order (md.win's): build the menu, install the actions, register the handlers, then
//! publish the first snapshot. Everything the window shows is derived from md-logic's objects:
//!
//! - the **session** owns the text, the file, dirty / conflicted / save error; the title, the bars
//!   and the draft follow its events;
//! - the **state** owns the mode, Zen and the jump requests; [`DocumentWindow::apply_layout`]
//!   turns it into the panes' mode, the preview's shown flag and the chrome;
//! - the **snapshot** ([`DocumentWindow::snapshot`]) is what the menus and accelerators read, and
//!   [`WindowHost::publish`] hands it to the menu bar when it changed.
//!
//! Untitled documents whose text is dirty keep a **draft** (§1.6), written 1 s after the last edit,
//! on deactivation and at Quit, deleted on Save / Save As and on Don't Save.

use std::cell::{Cell, RefCell};
#[cfg(feature = "selftest")]
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use adw::prelude::*;
use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_logic::commands::{CommandArgument, CommandDispatcher, CommandId, DiagramRef, ShellSnapshot};
use md_logic::documents::{SessionRole, TextFileSession, document_loader, file_names};
use md_logic::exec::{Completion, LocalBoxFuture, Spawner};
use md_logic::preview::PreviewCoordinator;
use md_logic::seams::{Alerts, CloseChoice, FileWatcher, Pickers, Scheduler, TimerHandle, WindowId};
use md_logic::settings::{SettingsStore, TypingSettings, settings_keys, wrap_selection_setting};
use md_logic::strings;
use md_logic::view::{
    DerivedText, DerivedTextScheduler, DocumentWindowState, SettingsViewModeStore, ViewModeController, ZenController,
};
use md_logic::windows::{SessionWindow, WindowRect, window_placement, window_title};
use webkit::prelude::*;

use crate::app::Shell;
use crate::editor::article_panes::{ArticlePanes, PanesRole};
use crate::editor::document_bar::DocumentBar;
use crate::editor::footer::Footer;
use crate::editor::zen::ZenCapsule;
use crate::export::window_exports::WindowExports;
use crate::menus::actions;
use crate::menus::menu_bar::MenuBar;
use crate::services::alerts::AdwAlerts;
use crate::services::document_ops;
use crate::services::file_watcher::GioFileWatcher;
use crate::services::paths::display_folder;
use crate::services::pickers::GtkPickers;
use crate::theme;
use crate::web::preview_host::PreviewHost;
use crate::windows::host::{WindowHost, WindowKind};

/// How long a closing window waits for an export or print in flight (§1.5).
pub const OUTPUT_WAIT: Duration = Duration::from_secs(10);

/// The exports and prints running for a window, so a close can wait for them (§1.5, §7.1).
#[derive(Default)]
struct Outputs {
    running: Cell<usize>,
    idle: RefCell<Vec<Completion<()>>>,
}

impl Outputs {
    fn track(self: &Rc<Self>, output: LocalBoxFuture<'static, ()>) -> LocalBoxFuture<'static, ()> {
        self.running.set(self.running.get() + 1);
        let me = Rc::clone(self);
        Box::pin(async move {
            output.await;
            me.running.set(me.running.get().saturating_sub(1));
            if me.running.get() == 0 {
                for waiter in me.idle.take() {
                    waiter.complete(());
                }
            }
        })
    }

    /// Resolves when nothing runs, or at `deadline`.
    async fn wait(&self, deadline: Instant) {
        if self.running.get() == 0 {
            return;
        }
        let done: Completion<()> = Completion::new();
        self.idle.borrow_mut().push(done.clone());
        let left = deadline.saturating_duration_since(Instant::now());
        let _ = glib::future_with_timeout(left, done.wait()).await;
    }
}

/// What the bar slot shows.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BarState {
    Hidden,
    Conflict,
    SaveError(String),
}

/// One document's window.
pub struct DocumentWindow {
    me: Weak<DocumentWindow>,
    id: WindowId,
    window: adw::ApplicationWindow,
    toolbar: adw::ToolbarView,
    title: adw::WindowTitle,
    menu_bar: MenuBar,
    overlay: gtk::Overlay,
    panes: Rc<ArticlePanes>,
    preview: Rc<PreviewHost>,
    coordinator: Rc<PreviewCoordinator>,
    bar: Rc<DocumentBar>,
    footer: Rc<Footer>,
    capsule: Rc<ZenCapsule>,
    alerts: Rc<AdwAlerts>,
    pickers: Rc<GtkPickers>,
    exports: Rc<WindowExports>,
    watcher: Rc<GioFileWatcher>,
    session: Rc<TextFileSession>,
    state: Rc<DocumentWindowState>,
    modes: ViewModeController,
    zen: Rc<ZenController>,
    derived: Rc<DerivedTextScheduler>,
    dispatcher: Rc<CommandDispatcher>,
    /// Every command this window registered a handler for (the integration gate's rule).
    #[cfg(feature = "selftest")]
    wired: RefCell<HashSet<CommandId>>,
    /// The `UntitledNames` ordinal this window holds while it is "Untitled N".
    untitled: Cell<Option<usize>>,
    draft: RefCell<Option<PathBuf>>,
    draft_timer: RefCell<Option<TimerHandle>>,
    outputs: Rc<Outputs>,
    bar_state: RefCell<BarState>,
    /// A file dialog of this window is up (`GtkPickers::open_changed`); alerts and About are the
    /// window's visible dialog. Either turns every `win.` command off (§2.2).
    picker_open: Cell<bool>,
    applied: Cell<Option<(ViewMode, bool, bool)>>,
    undo_generation: Cell<u64>,
    closing: Cell<bool>,
    torn_down: Cell<bool>,
    /// While the window collects its session's alerts instead of showing them (an open that may
    /// fail, the close flush).
    collected: RefCell<Option<Vec<(String, String)>>>,
    last_snapshot: RefCell<Option<ShellSnapshot>>,
}

impl DocumentWindow {
    pub fn new(shell: &Rc<Shell>, id: WindowId) -> Rc<DocumentWindow> {
        let size = window_placement::parse_size(
            shell.settings.get_string(settings_keys::DOCUMENT_WINDOW_SIZE).as_deref(),
            window_placement::DOCUMENT_DEFAULT,
        );
        let title = adw::WindowTitle::new(strings::UNTITLED, "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        let menu_bar = MenuBar::new(shell, WindowKind::Document);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&menu_bar.widget());

        let panes = ArticlePanes::new(shell, PanesRole::Document);
        let preview = PreviewHost::new(&shell.web);
        panes.set_preview(&preview);
        let bar = DocumentBar::new();
        let footer = Footer::new();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let panes_widget = panes.widget();
        panes_widget.set_vexpand(true);
        panes_widget.set_hexpand(true);
        content.append(&panes_widget);
        content.append(&bar.widget());
        content.append(&footer.widget());
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&content));
        toolbar.set_content(Some(&overlay));

        let window = adw::ApplicationWindow::builder()
            .application(&shell.app)
            .title(strings::UNTITLED)
            .default_width(size.width)
            .default_height(size.height)
            .content(&toolbar)
            .build();
        window.set_size_request(window_placement::MINIMUM.width, window_placement::MINIMUM.height);
        let toplevel: gtk::Window = window.clone().upcast();
        let alerts = AdwAlerts::new(&toplevel);
        let pickers = GtkPickers::new(&toplevel, alerts.clone());

        let watcher = GioFileWatcher::new();
        let session = TextFileSession::new(
            shell.files.clone(),
            watcher.clone(),
            shell.scheduler.clone(),
            shell.registry.clone(),
            shell.identity.clone(),
            SessionRole::Document,
            Some(id),
        );
        let state = Rc::new(DocumentWindowState::new());
        let modes = ViewModeController::new(
            state.clone(),
            Rc::new(SettingsViewModeStore::new(shell.settings.clone())),
            shell.book.article_opens(),
        );
        let zen = ZenController::new(state.clone(), Some(shell.scheduler.clone()));
        let capsule = ZenCapsule::new(&overlay, zen.clone());
        let derived = DerivedTextScheduler::new(
            shell.scheduler.clone(),
            shell.spawner.clone(),
            shell.off_thread.clone(),
            shell.words.clone(),
        );
        let coordinator = preview.coordinator(shell);

        let window_rc = Rc::new_cyclic(|me: &Weak<DocumentWindow>| {
            let host: Weak<dyn WindowHost> = me.clone();
            let exports = WindowExports::new(shell, host);
            let source = me.clone();
            let dispatcher = Rc::new(CommandDispatcher::new(shell.scheduler.clone(), move || {
                source.upgrade().map(|w| w.snapshot()).unwrap_or_default()
            }));
            DocumentWindow {
                me: me.clone(),
                id,
                window,
                toolbar,
                title,
                menu_bar,
                overlay,
                panes,
                preview,
                coordinator,
                bar,
                footer,
                capsule,
                alerts,
                pickers,
                exports,
                watcher,
                session,
                state,
                modes,
                zen,
                derived,
                dispatcher,
                #[cfg(feature = "selftest")]
                wired: RefCell::new(HashSet::new()),
                untitled: Cell::new(None),
                draft: RefCell::new(None),
                draft_timer: RefCell::new(None),
                outputs: Rc::new(Outputs::default()),
                bar_state: RefCell::new(BarState::Hidden),
                picker_open: Cell::new(false),
                applied: Cell::new(None),
                undo_generation: Cell::new(0),
                closing: Cell::new(false),
                torn_down: Cell::new(false),
                collected: RefCell::new(None),
                last_snapshot: RefCell::new(None),
            }
        });
        window_rc.wire();
        window_rc
    }

    pub fn id(&self) -> WindowId {
        self.id
    }

    /// The file this window edits, when it has one.
    #[cfg(feature = "selftest")]
    pub fn path(&self) -> Option<String> {
        self.session.editing_path()
    }

    /// The document's text (LF), as the session holds it.
    #[cfg(feature = "selftest")]
    pub fn text(&self) -> String {
        self.session.text()
    }

    fn shell() -> Rc<Shell> {
        Shell::get()
    }

    fn spawn(&self, future: impl std::future::Future<Output = ()> + 'static) {
        Self::shell().spawner.spawn_local(Box::pin(future));
    }

    // ---- wiring --------------------------------------------------------------------------------

    fn wire(self: &Rc<Self>) {
        actions::install_window_actions(&self.window, WindowKind::Document, &self.dispatcher);
        self.register_handlers();

        let me = self.me.clone();
        self.session.changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.on_session_changed();
            }
        });
        let me = self.me.clone();
        self.session.text_replaced_externally().connect(move |text: &str| {
            if let Some(w) = me.upgrade() {
                w.panes.editor().set_text(text);
                w.on_text_changed();
            }
        });
        let me = self.me.clone();
        self.session.identity_changed().connect(move |path: &Option<String>| {
            if let Some(w) = me.upgrade() {
                w.on_identity_changed(path.as_deref());
            }
        });
        let me = self.me.clone();
        self.session.alert_requested().connect(move |(title, message): &(String, String)| {
            if let Some(w) = me.upgrade() {
                w.alert(title.clone(), message.clone());
            }
        });

        let editor = self.panes.editor();
        let me = self.me.clone();
        editor.text_edited().connect(move |text: &str| {
            if let Some(w) = me.upgrade()
                && text != w.session.text()
            {
                w.session.edit(text);
                w.on_text_changed();
            }
        });
        let me = self.me.clone();
        editor.selection_changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });
        let me = self.me.clone();
        editor.history_changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });

        let me = self.me.clone();
        self.state.changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.apply_layout();
                // The capsule's switch moves with Ctrl+1 / Ctrl+3 too (P3's contract).
                w.capsule.refresh();
                w.publish();
            }
        });
        let me = self.me.clone();
        self.derived.changed().connect(move |derived: &DerivedText| {
            if let Some(w) = me.upgrade() {
                w.footer.update(derived);
                w.state.set_derived(derived.clone());
            }
        });
        let footer = Rc::downgrade(&self.footer);
        self.exports.busy_changed().connect(move |busy| {
            if let Some(footer) = footer.upgrade() {
                footer.set_busy(*busy);
            }
        });

        // Zen ↔ full screen (§5.3): our request goes out, the notify is the only truth back.
        let window = self.window.downgrade();
        self.zen.full_screen_requested().connect(move |&on| {
            if let Some(window) = window.upgrade() {
                if on { window.fullscreen() } else { window.unfullscreen() }
            }
        });
        let me = self.me.clone();
        self.window.connect_fullscreened_notify(move |window| {
            if let Some(w) = me.upgrade() {
                w.zen.presenter_changed(window.is_fullscreen());
                w.publish();
            }
        });
        let me = self.me.clone();
        self.window.connect_is_active_notify(move |window| {
            if let Some(w) = me.upgrade()
                && !window.is_active()
            {
                w.flush_on_deactivate();
            }
        });
        let me = self.me.clone();
        self.window.connect_close_request(move |_| {
            if let Some(w) = me.upgrade() {
                w.request_close();
            }
            glib::Propagation::Stop
        });
        // The find bar's query and visibility (Find Next lights up as soon as a query is typed),
        // the focus (the editing rows follow the focused widget), the dialogs (§2.2).
        let me = self.me.clone();
        self.panes.find_bar().changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });
        let me = self.me.clone();
        self.window.connect_focus_widget_notify(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });
        let me = self.me.clone();
        self.window.connect_visible_dialog_notify(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });
        let me = self.me.clone();
        self.alerts.open_changed().connect(move |_| {
            if let Some(w) = me.upgrade() {
                w.publish();
            }
        });
        let me = self.me.clone();
        self.pickers.open_changed().connect(move |open| {
            if let Some(w) = me.upgrade() {
                w.picker_open.set(*open);
                w.publish();
            }
        });

        // Files dropped on the window open as documents (§6.1); text drags go on to the editor.
        let drop = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);
        drop.set_propagation_phase(gtk::PropagationPhase::Capture);
        drop.connect_drop(|_, value, _, _| match value.get::<gtk::gdk::FileList>() {
            Ok(list) => {
                Shell::get().windows.open_files(&list.files());
                true
            }
            Err(_) => false,
        });
        self.overlay.add_controller(drop);

        self.preview.apply_theme(theme::is_dark());
        self.on_session_changed();
    }

    /// Registers `id` on the dispatcher, the handler holding the window weakly.
    fn handle(&self, id: CommandId, run: impl Fn(&Rc<DocumentWindow>) + 'static) {
        let me = self.me.clone();
        self.dispatcher.register(id, move || {
            if let Some(w) = me.upgrade() {
                run(&w);
            }
        });
        #[cfg(feature = "selftest")]
        self.wired.borrow_mut().insert(id);
    }

    fn handle_with(&self, id: CommandId, run: impl Fn(&Rc<DocumentWindow>, &CommandArgument) + 'static) {
        let me = self.me.clone();
        self.dispatcher.register_with_argument(id, move |argument| {
            if let Some(w) = me.upgrade() {
                run(&w, argument);
            }
        });
        #[cfg(feature = "selftest")]
        self.wired.borrow_mut().insert(id);
    }

    fn register_handlers(&self) {
        use CommandId as C;
        // File
        self.handle(C::Close, |w| w.window.close());
        self.handle(C::Save, |w| w.save());
        self.handle(C::SaveAs, |w| {
            let flow = Rc::clone(w);
            w.spawn(async move {
                flow.save_as_flow().await;
            });
        });
        self.handle(C::Duplicate, |w| {
            Shell::get().windows.duplicate(w.session.text(), &w.session.title());
        });
        self.handle(C::Rename, |w| {
            let flow = Rc::clone(w);
            w.spawn(async move { flow.rename_flow().await });
        });
        self.handle(C::MoveTo, |w| {
            let flow = Rc::clone(w);
            w.spawn(async move { flow.move_flow().await });
        });
        self.handle(C::RevertToSaved, |w| w.session.revert_to_saved());
        self.handle(C::Print, |w| {
            w.before_output();
            w.exports.print(w.session.text(), w.session.title());
        });
        self.handle(C::ExportPdf, |w| {
            w.before_output();
            w.exports.export_pdf(w.session.text(), w.session.title());
        });
        self.handle(C::ExportHtml, |w| {
            w.before_output();
            w.exports.export_html(w.session.text(), w.session.title());
        });
        self.handle(C::ExportEpub, |w| {
            w.before_output();
            w.exports.export_epub(w.session.text(), w.session.title(), w.session.editing_path());
        });
        self.handle(C::ExportLaTeX, |w| {
            w.before_output();
            w.exports.export_latex(w.session.text(), w.session.title());
        });
        self.handle(C::ExportTextBundle, |w| {
            w.before_output();
            w.exports.export_text_bundle(w.session.text(), w.session.editing_path(), w.session.title());
        });
        self.handle_with(C::ExportDiagramSvg, |w, argument| {
            if let CommandArgument::Diagram(ordinal) = argument {
                w.before_output();
                w.exports.export_diagram_svg(w.session.text(), w.session.title(), *ordinal as u32);
            }
        });

        // Edit: the focused widget's (§2.2).
        for id in [C::Undo, C::Redo, C::Cut, C::Copy, C::Paste, C::Delete, C::SelectAll] {
            self.handle(id, move |w| w.edit_in_focus(id));
        }
        self.handle(C::Find, |w| {
            w.show_editor();
            w.panes.find_bar().show_find();
        });
        self.handle(C::Replace, |w| {
            w.show_editor();
            w.panes.find_bar().show_replace();
        });
        self.handle(C::FindNext, |w| w.panes.find_bar().find_next(true));
        self.handle(C::FindPrevious, |w| w.panes.find_bar().find_next(false));
        self.handle(C::UseSelectionForFind, |w| w.panes.find_bar().use_selection());

        // Format: one undoable edit each, focus back in the editor.
        for id in [
            C::Bold,
            C::Italic,
            C::InlineCode,
            C::Strikethrough,
            C::Link,
            C::Heading1,
            C::Heading2,
            C::Heading3,
            C::Heading4,
            C::Heading5,
            C::Heading6,
            C::BulletList,
            C::TaskList,
            C::Quote,
            C::Indent,
            C::Outdent,
        ] {
            self.handle(id, move |w| w.panes.editor().apply_format(id));
        }

        // View
        self.handle(C::ViewEdit, |w| w.modes.select(ViewMode::Edit));
        self.handle(C::ViewSplit, |w| w.modes.select(ViewMode::Split));
        self.handle(C::ViewPreview, |w| w.modes.select(ViewMode::Preview));
        self.handle(C::ZenMode, |w| w.zen.toggle());
        self.handle(C::FullScreen, |w| {
            if w.window.is_fullscreen() { w.window.unfullscreen() } else { w.window.fullscreen() }
        });

        // Go
        self.handle_with(C::Contents, |w, argument| {
            if let CommandArgument::Outline(entry) = argument {
                w.modes.jump_to_heading(entry);
            }
        });
        self.handle_with(C::Notes, |w, argument| {
            if let CommandArgument::Note(note) = argument {
                w.modes.jump_to_note(note);
            }
        });

        // Window
        self.handle(C::Minimize, |w| w.window.minimize());
        self.handle(C::Zoom, |w| if w.window.is_maximized() { w.window.unmaximize() } else { w.window.maximize() });
        self.handle_with(C::ActivateWindow, |_, argument| {
            if let CommandArgument::Window(id) = argument {
                Shell::get().windows.present(*id);
            }
        });

        // Escape (the window key controller, §2.2): Zen first, then the find bar.
        self.handle(C::Escape, |w| {
            if w.zen.is_active() {
                w.zen.set_active(false);
            } else {
                w.panes.find_bar().close();
            }
        });
    }

    // ---- content -------------------------------------------------------------------------------

    /// A document with no file: New, an example, a duplicate, a restored draft.
    pub fn open_untitled(&self, text: &str, title: &str, ordinal: Option<usize>, dirty: bool) {
        self.untitled.set(ordinal);
        self.session.open_untitled(text, Some(title), dirty);
    }

    /// A restored draft: the untitled document keeps writing to the same draft file.
    pub fn open_draft(&self, path: PathBuf, text: &str, title: &str, ordinal: Option<usize>) {
        *self.draft.borrow_mut() = Some(path);
        self.open_untitled(text, title, ordinal, true);
    }

    /// Opens `path` (a document, or a bundle to import). On failure the session's alerts are
    /// returned for the caller to show — this window is discarded.
    pub fn open_file(&self, path: &str) -> Result<(), Vec<(String, String)>> {
        *self.collected.borrow_mut() = Some(Vec::new());
        let opened = self.session.open(path);
        let alerts = self.collected.borrow_mut().take().unwrap_or_default();
        if opened {
            for (title, message) in alerts {
                self.alert(title, message);
            }
            Ok(())
        } else {
            Err(alerts)
        }
    }

    /// Puts the session's text into the view and lets the per-file memory decide the mode — the
    /// Mac's `onAppear`.
    pub fn load_into_view(&self) {
        let text = self.session.text();
        self.panes.editor().set_text(&text);
        self.undo_generation.set(self.session.undo_generation());
        self.modes.apply_memory(self.session.editing_path().as_deref(), &text);
    }

    /// A `session.json` row's view state: size, maximized, mode, Zen (§1.6). Positions are the
    /// compositor's.
    pub fn restore_view(&self, row: &SessionWindow) {
        if row.placement.width > 0 && row.placement.height > 0 {
            let size = window_placement::at_least_minimum(row.placement.size());
            self.window.set_default_size(size.width, size.height);
        }
        if row.maximized {
            self.window.maximize();
        }
        self.state.set_navigation_mode(None);
        self.state.set_stored_mode(row.mode);
        self.state.set_zen_reading(row.zen_reading);
        if row.zen {
            self.zen.set_active(true);
        }
    }

    /// Lays out, renders and presents the window.
    pub fn show(&self) {
        self.apply_layout();
        self.on_text_changed();
        self.on_session_changed();
        self.window.present();
    }

    // ---- events --------------------------------------------------------------------------------

    fn on_session_changed(&self) {
        let session = &self.session;
        let title = window_title::for_document(&session.title(), session.is_dirty());
        if self.window.title().as_deref() != Some(title.as_str()) {
            self.window.set_title(Some(&title));
            self.title.set_title(&title);
            Self::shell().registry.set_title(self.id, &title);
        }
        let folder = session.editing_path().map(|path| file_names::directory_of(&path).to_string());
        // Save As…, Move To… and Open… start beside the document (§6.1).
        self.pickers.set_document_folder(folder.as_deref());
        let subtitle = folder.as_deref().map(display_folder).unwrap_or_default();
        if self.title.subtitle().as_str() != subtitle {
            self.title.set_subtitle(&subtitle);
        }
        let generation = session.undo_generation();
        if generation != self.undo_generation.replace(generation) {
            self.panes.editor().reset_history();
        }
        self.update_bar();
        self.update_preview();
        self.publish();
    }

    fn update_bar(&self) {
        let wanted = if self.session.conflicted() {
            BarState::Conflict
        } else if let Some(error) = self.session.save_error_text() {
            BarState::SaveError(error)
        } else {
            BarState::Hidden
        };
        if *self.bar_state.borrow() == wanted {
            return;
        }
        match &wanted {
            BarState::Hidden => self.bar.hide(),
            BarState::Conflict => {
                let (reload, keep) = (self.me.clone(), self.me.clone());
                self.bar.show_conflict(
                    move || {
                        if let Some(w) = reload.upgrade() {
                            w.session.resolve_conflict_reloading();
                        }
                    },
                    move || {
                        if let Some(w) = keep.upgrade() {
                            w.session.resolve_conflict_keeping_mine();
                        }
                    },
                );
            }
            BarState::SaveError(error) => {
                let (retry, save_as) = (self.me.clone(), self.me.clone());
                self.bar.show_save_error(
                    &strings::documents::could_not_save(error),
                    move || {
                        if let Some(w) = retry.upgrade() {
                            w.session.flush_now(true);
                        }
                    },
                    move || {
                        if let Some(w) = save_as.upgrade() {
                            let flow = Rc::clone(&w);
                            w.spawn(async move {
                                flow.save_as_flow().await;
                            });
                        }
                    },
                );
            }
        }
        *self.bar_state.borrow_mut() = wanted;
    }

    fn on_identity_changed(&self, path: Option<&str>) {
        if let Some(path) = path {
            let shell = Self::shell();
            shell.recent.add(path);
            // No longer untitled: the draft goes, the "Untitled N" name is free again.
            self.delete_draft();
            if let Some(ordinal) = self.untitled.take() {
                shell.windows.release_untitled(ordinal);
            }
        }
        self.modes.apply_memory(path, &self.session.text());
    }

    /// The text changed (typed, replaced, opened): the preview, the derived text, the draft.
    fn on_text_changed(&self) {
        self.update_preview();
        self.derived.text_changed(&self.session.text());
        self.schedule_draft();
    }

    fn update_preview(&self) {
        self.coordinator.update(&self.session.text(), &self.session.title(), theme::is_dark(), None);
    }

    /// The system colour scheme flipped: the paper background first, then the page (§4.8).
    pub fn on_theme(&self, dark: bool) {
        self.preview.apply_theme(dark);
        self.coordinator.update(&self.session.text(), &self.session.title(), dark, None);
    }

    fn alert(&self, title: String, message: String) {
        if let Some(collected) = self.collected.borrow_mut().as_mut() {
            collected.push((title, message));
            return;
        }
        let shown = self.alerts.warn(title, message);
        self.spawn(shown);
    }

    /// The mode, Zen and the jump requests → the panes, the preview's shown flag and the chrome.
    fn apply_layout(&self) {
        let zen = self.state.zen_active();
        let reading = self.state.zen_reading();
        let mode = self.state.effective_mode();
        if self.applied.get() != Some((mode, zen, reading)) {
            self.applied.set(Some((mode, zen, reading)));
            let shows_preview = if zen { reading } else { mode != ViewMode::Edit };
            // The shown flag before the coordinator hears about it: `is_shown` decides between a
            // reload and "stale" (§4.4, md.win's `ShowPreview`).
            self.preview.set_shown(shows_preview);
            self.panes.set_mode(mode);
            self.panes.set_zen(zen.then_some(reading));
            self.toolbar.set_reveal_top_bars(!zen);
            self.footer.widget().set_visible(!zen);
            self.bar.widget().set_visible(!zen);
            if zen {
                self.window.set_size_request(-1, -1);
            } else {
                self.window.set_size_request(window_placement::MINIMUM.width, window_placement::MINIMUM.height);
            }
            if shows_preview {
                self.coordinator.show();
            } else {
                self.coordinator.hide();
            }
        }
        if let Some(navigation) = self.state.preview_navigation() {
            let state = Rc::downgrade(&self.state);
            self.coordinator.navigate(
                navigation,
                Some(Box::new(move |id| {
                    if let Some(state) = state.upgrade() {
                        state.preview_navigation_handled(id);
                    }
                })),
            );
        }
        if let Some(jump) = self.state.editor_jump() {
            self.panes.editor().apply_jump(jump.clone());
            self.state.editor_jump_handled(jump.id);
        }
    }

    /// Find from a window showing only the preview brings the editor back as a nudge, never a
    /// stored mode (§2.5, the Mac's `showEditor`).
    fn show_editor(&self) {
        if !self.editor_visible() {
            self.modes.show_editor();
        }
    }

    fn editor_visible(&self) -> bool {
        if self.state.zen_active() {
            !self.state.zen_reading()
        } else {
            self.state.effective_mode() != ViewMode::Preview
        }
    }

    // ---- commands ------------------------------------------------------------------------------

    /// Undo … Select All act on whatever holds the keyboard focus (§2.2): a `gtk::Text` (a find
    /// field, a dialog's entry), the preview, or the editor.
    fn edit_in_focus(&self, id: CommandId) {
        let editor = self.panes.editor();
        let focus = gtk::prelude::GtkWindowExt::focus(&self.window);
        let editor_widget = editor.widget();
        let in_editor = focus.as_ref().is_some_and(|f| f == &editor_widget || f.is_ancestor(&editor_widget));
        if !in_editor && let Some(focus) = focus {
            if let Some(text) = focus.downcast_ref::<gtk::Text>() {
                let action = match id {
                    CommandId::Undo => "text.undo",
                    CommandId::Redo => "text.redo",
                    CommandId::Cut => "clipboard.cut",
                    CommandId::Copy => "clipboard.copy",
                    CommandId::Paste => "clipboard.paste",
                    CommandId::Delete => "selection.delete",
                    _ => "selection.select-all",
                };
                let _ = WidgetExt::activate_action(text, action, None);
                return;
            }
            let web = focus
                .downcast_ref::<webkit::WebView>()
                .cloned()
                .or_else(|| focus.ancestor(webkit::WebView::static_type()).and_downcast());
            if let Some(web) = web {
                match id {
                    CommandId::Copy => web.execute_editing_command("Copy"),
                    CommandId::SelectAll => web.execute_editing_command("SelectAll"),
                    _ => {}
                }
                return;
            }
        }
        match id {
            CommandId::Undo => editor.undo(),
            CommandId::Redo => editor.redo(),
            CommandId::Cut => editor.cut(),
            CommandId::Copy => editor.copy(),
            CommandId::Paste => editor.paste(),
            CommandId::Delete => editor.delete_selection(),
            _ => editor.select_all(),
        }
    }

    /// Ctrl+S: an untitled document (or an import) goes through Save As.
    fn save(self: &Rc<Self>) {
        if self.session.has_file_identity() {
            self.session.flush_now(true);
        } else {
            let w = Rc::clone(self);
            self.spawn(async move {
                w.save_as_flow().await;
            });
        }
    }

    /// Save As… (§6.4): the save dialog, then the session follows the new file. True when saved.
    async fn save_as_flow(self: Rc<Self>) -> bool {
        let current = self.session.editing_path();
        let extension = document_loader::default_extension_for(current.as_deref());
        let suggested = match &current {
            Some(path) => file_names::name_of(path).to_string(),
            None => format!("{}{extension}", self.session.title()),
        };
        let chosen = self.pickers.save_file(suggested, document_loader::save_file_type_choices(), extension).await;
        match chosen {
            Some(path) => self.session.save_as(&path),
            None => false,
        }
    }

    /// Rename… (§6.4): a name prompt, the Mac's name rule, the extension kept; the session follows
    /// the file.
    async fn rename_flow(self: Rc<Self>) {
        let Some(path) = self.session.editing_path() else { return };
        let session = Rc::clone(&self.session);
        let flush = move || {
            session.flush_now(false);
        };
        if let Some(new_path) = document_ops::rename_with_prompt(self.alerts.clone(), path, flush).await {
            self.session.retarget(&new_path);
        }
    }

    /// Move To… (§6.4): a folder dialog, then the same follow-up as Rename.
    async fn move_flow(self: Rc<Self>) {
        let Some(path) = self.session.editing_path() else { return };
        let session = Rc::clone(&self.session);
        let flush = move || {
            session.flush_now(false);
        };
        let moved = document_ops::move_with_picker(self.pickers.clone(), self.alerts.clone(), path, flush).await;
        if let Some(new_path) = moved {
            self.session.retarget(&new_path);
        }
    }

    /// Every print and export reads the saved bytes' text; a saved document is flushed first.
    fn before_output(&self) {
        if self.session.has_file_identity() {
            self.session.flush_now(false);
        }
    }

    // ---- drafts --------------------------------------------------------------------------------

    fn needs_draft(&self) -> bool {
        !self.session.has_file_identity() && self.session.is_dirty()
    }

    fn schedule_draft(&self) {
        if !self.needs_draft() {
            return;
        }
        let me = self.me.clone();
        let timer = Self::shell().scheduler.after(
            TextFileSession::AUTOSAVE_DELAY,
            Box::new(move || {
                if let Some(w) = me.upgrade() {
                    w.write_draft();
                }
            }),
        );
        *self.draft_timer.borrow_mut() = Some(timer);
    }

    /// Writes the draft now (an untitled, dirty document only); its path.
    pub fn write_draft(&self) -> Option<PathBuf> {
        let pending = self.draft_timer.borrow_mut().take();
        if let Some(pending) = pending {
            pending.detach(); // fired, or superseded by this write
        }
        if !self.needs_draft() {
            return None;
        }
        let shell = Self::shell();
        let drafts = shell.windows.drafts();
        let path = self.draft.borrow().clone().unwrap_or_else(|| drafts.new_path());
        match drafts.write(&path, &self.session.text()) {
            Ok(()) => {
                *self.draft.borrow_mut() = Some(path.clone());
                Some(path)
            }
            Err(err) => {
                eprintln!("md: could not write the draft {}: {err}", path.display());
                None
            }
        }
    }

    fn delete_draft(&self) {
        drop(self.draft_timer.borrow_mut().take());
        let draft = self.draft.borrow_mut().take();
        if let Some(path) = draft {
            Self::shell().windows.drafts().delete(&path);
        }
    }

    /// The draft file, while there is one.
    #[cfg(feature = "selftest")]
    pub fn draft_path(&self) -> Option<PathBuf> {
        self.draft.borrow().clone()
    }

    fn flush_on_deactivate(&self) {
        if self.torn_down.get() {
            return;
        }
        if self.session.has_file_identity() {
            self.session.flush_now(false);
        } else {
            self.write_draft();
        }
    }

    // ---- session.json and Quit -----------------------------------------------------------------

    /// This window's `session.json` row: a saved document's path, or an untitled dirty document's
    /// draft (written now); `None` for an untouched untitled window, which nothing would restore.
    pub fn session_entry(&self) -> Option<SessionWindow> {
        let path = match self.session.editing_path() {
            Some(path) => path,
            None => self.write_draft()?.to_string_lossy().into_owned(),
        };
        let (width, height) = self.window.default_size();
        Some(SessionWindow::new(
            &path,
            self.state.stored_mode(),
            self.state.zen_active(),
            self.state.zen_reading(),
            WindowRect::new(0, 0, width, height),
            self.window.is_maximized(),
        ))
    }

    /// Quit's flush, synchronous (§1.5 route 2): a saved document is written (a rescue copy when the
    /// write is refused), an untitled dirty one goes to its draft. Nothing is asked.
    pub fn quit_flush(&self) {
        if self.session.has_file_identity() {
            self.session.terminate_flush();
        } else {
            self.write_draft();
        }
    }

    /// Resolves when no export or print runs, or at `deadline`.
    pub async fn wait_outputs(&self, deadline: Instant) {
        self.outputs.wait(deadline).await;
    }

    // ---- closing -------------------------------------------------------------------------------

    fn request_close(self: &Rc<Self>) {
        if self.closing.replace(true) {
            return;
        }
        let w = Rc::clone(self);
        self.spawn(async move {
            if w.close_flow().await {
                Shell::get().windows.close_window(&w);
            } else {
                w.closing.set(false);
            }
        });
    }

    /// The close policy (§1.5 route 1). False when the user cancelled.
    async fn close_flow(self: &Rc<Self>) -> bool {
        if !self.session.has_file_identity() && self.session.is_dirty() {
            match self.alerts.ask_save_changes(self.session.title()).await {
                CloseChoice::Save => {
                    if !Rc::clone(self).save_as_flow().await {
                        return false;
                    }
                }
                CloseChoice::DontSave => self.delete_draft(),
                CloseChoice::Cancel => return false,
            }
        } else {
            // A refused flush parks the text in a rescue copy and says so before the window goes.
            *self.collected.borrow_mut() = Some(Vec::new());
            self.session.close_flush();
            let alerts = self.collected.borrow_mut().take().unwrap_or_default();
            for (title, message) in alerts {
                self.alerts.warn(title, message).await;
            }
        }
        self.outputs.wait(Instant::now() + OUTPUT_WAIT).await;
        true
    }

    /// Lets go of everything: timers, the session's file and registry entry, the web view; records
    /// the size for the next window. Idempotent. The caller destroys the window.
    pub fn teardown(&self) {
        if self.torn_down.replace(true) {
            return;
        }
        self.closing.set(true);
        drop(self.draft_timer.borrow_mut().take());
        // Whatever still renders after the close's wait stops quietly (§1.5).
        self.exports.cancel();
        self.derived.cancel();
        self.session.dispose();
        self.watcher.stop();
        self.preview.close();
        let shell = Self::shell();
        if !self.window.is_maximized() && !self.window.is_fullscreen() {
            let (width, height) = self.window.default_size();
            if width > 0 && height > 0 {
                let size = window_placement::format_size(window_placement::WindowSize::new(width, height));
                shell.settings.set_string(settings_keys::DOCUMENT_WINDOW_SIZE, &size);
            }
        }
        if let Some(ordinal) = self.untitled.take() {
            shell.windows.release_untitled(ordinal);
        }
        shell.registry.remove(self.id);
    }

    /// The toplevel, for the manager.
    pub fn adw_window(&self) -> &adw::ApplicationWindow {
        &self.window
    }

    /// This window's exports — Print Book and Export Book render in them when the Book window is
    /// closed (md.win §8.7).
    pub fn exports(&self) -> Rc<WindowExports> {
        Rc::clone(&self.exports)
    }

    // ---- the snapshot --------------------------------------------------------------------------

    fn compute_snapshot(&self) -> ShellSnapshot {
        let shell = Self::shell();
        let derived = self.state.derived();
        let zen = self.state.zen_active();
        let editor = self.panes.editor();
        let find = self.panes.find_bar();
        let typing = TypingSettings::read(shell.settings.as_ref());
        ShellSnapshot {
            has_document: true,
            is_book_window: false,
            is_editing_article: false,
            is_saved: self.session.has_file_identity(),
            is_dirty: self.session.is_dirty(),
            has_book: shell.book.has_book(),
            displayed_mode: self.state.effective_mode(),
            zen_active: zen,
            zen_reading: self.state.zen_reading(),
            outline: derived.outline,
            notes: derived.notes,
            diagrams: derived.diagrams.iter().map(|d| DiagramRef::new(d.ordinal, d.menu_title.clone())).collect(),
            can_previous: false,
            can_next: false,
            editor_visible: self.editor_visible(),
            can_undo: editor.can_undo(),
            can_redo: editor.can_redo(),
            recent_entries: shell.windows.recent_entries(),
            window_titles: shell.windows.window_titles(self.id),
            has_selection: editor.has_selection(),
            has_find_query: find.has_query(),
            pdf_page_size_id: PageSize::named(shell.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref())
                .id
                .to_string(),
            sidebar_open: false,
            is_full_screen: self.window.is_fullscreen(),
            dialog_open: self.window.visible_dialog().is_some() || self.alerts.is_open() || self.picker_open.get(),
            find_bar_open: find.is_open(),
            continue_lists: typing.continue_lists,
            capitalize_sentences: typing.capitalize_sentences,
            wrap_selection: wrap_selection_setting::read(shell.settings.as_ref()),
        }
    }

    /// Commands this window registered a handler for.
    #[cfg(feature = "selftest")]
    pub fn wired(&self) -> HashSet<CommandId> {
        self.wired.borrow().clone()
    }

    /// The stored mode and Zen, as `session.json` would record them.
    #[cfg(feature = "selftest")]
    pub fn view_state(&self) -> (ViewMode, bool, bool) {
        (self.state.stored_mode(), self.state.zen_active(), self.state.zen_reading())
    }

    /// The header bar, menu bar and footer are on screen (off in Zen).
    #[cfg(feature = "selftest")]
    pub fn chrome_shown(&self) -> bool {
        self.toolbar.reveals_top_bars() && self.footer.widget().is_visible()
    }

    /// The window's menu model.
    #[cfg(feature = "selftest")]
    pub fn menu_model(&self) -> Option<gio::MenuModel> {
        self.menu_bar.model()
    }

    /// The conflict / save-error bar.
    #[cfg(feature = "selftest")]
    pub fn bar(&self) -> Rc<DocumentBar> {
        Rc::clone(&self.bar)
    }

    /// The editor pane (the self-test probes whether the real one is in).
    #[cfg(feature = "selftest")]
    pub fn editor(&self) -> Rc<crate::editor::editor_pane::EditorPane> {
        self.panes.editor()
    }
}

impl WindowHost for DocumentWindow {
    fn id(&self) -> WindowId {
        self.id
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
        Self::shell().spawner.spawn_local(self.outputs.track(output));
    }

    fn snapshot(&self) -> ShellSnapshot {
        self.compute_snapshot()
    }

    fn publish(&self) {
        if self.torn_down.get() {
            return;
        }
        let snapshot = self.compute_snapshot();
        if self.last_snapshot.borrow().as_ref() == Some(&snapshot) {
            return;
        }
        self.menu_bar.refresh(&snapshot);
        *self.last_snapshot.borrow_mut() = Some(snapshot);
    }
}
