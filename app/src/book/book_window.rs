//! `BookWindow` — the one app-wide Book window (shell-design §8.1, §8.4–§8.6):
//! `adw::ToolbarView` (header bar with the detail toolbar, then the menu bar) → `gtk::Overlay` →
//! `gtk::Paned` (sidebar 200–320 px, detail); the article's `TextFileSession`, navigator, stepper
//! and outputs. Created on demand by Show Book.
//!
//! The brain is md-logic's: `BookNavigatorModel` decides what is selected and what every
//! management operation does to the selection, the `TextFileSession` in its `BookArticle` role
//! decides what the detail pane shows (Editing, Handoff, Unreadable, Empty) and when the file is
//! written, `BookStepper` decides Previous / Next. This file renders their answers (`render`, after
//! md.win's `BookWindow.Render`) and turns gestures and commands into their calls.
//!
//! The detail pane while editing is the document window's composite — `ArticlePanes` (editor, find
//! bar, preview), the conflict / save-error bar and the footer — with the article's path as the
//! preview's document token, a fresh undo history per article, and `md.bookViewMode` (app-wide,
//! never `md.viewModeMemory`) as the mode.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use md_core::book::book_model::Book;
use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_memory::InMemoryViewModeStore;
use md_logic::activation::activation_description::ActivationItem;
use md_logic::activation::{ActivationAction, activation_router};
use md_logic::books::{BookFlushGateBinding, BookNavigatorModel, TextFileSessionArticles};
use md_logic::commands::{
    CommandArgument, CommandDispatcher, CommandId, DiagramRef, ShellSnapshot, WindowTitle, command_table,
};
use md_logic::documents::{SessionRole, Stage, TextFileSession};
use md_logic::exec::{LocalBoxFuture, Spawner};
use md_logic::preview::{PreviewCoordinator, asset_origin};
use md_logic::seams::{Alerts, Clock, DocumentRegistry, OffThread, OffThreadValue, OffThreadWork, Pickers, WindowId};
use md_logic::settings::{SettingsStore, TypingSettings, settings_keys, wrap_selection_setting};
use md_logic::strings::{self, books, buttons, documents};
use md_logic::text::note_preview;
use md_logic::view::{
    DerivedText, DerivedTextScheduler, DocumentWindowState, PaneLayout, SplitLayout, ViewModeController,
};
use md_logic::windows::{WindowSize, window_placement, window_title};
use webkit::prelude::WebViewExt;

use crate::app::Shell;
use crate::book::sidebar::{self, BookSidebar};
use crate::book::{BOOK_WINDOW_ID, BookOutputKind};
use crate::editor::article_panes::{ArticlePanes, PanesRole};
use crate::editor::document_bar::DocumentBar;
use crate::editor::footer::Footer;
use crate::export::window_exports::WindowExports;
use crate::menus::actions::install_window_actions;
use crate::menus::menu_bar::MenuBar;
use crate::services::alerts::AdwAlerts;
use crate::services::file_watcher::GioFileWatcher;
use crate::services::pickers::GtkPickers;
use crate::theme;
use crate::web::preview_host::PreviewHost;
use crate::windows::host::{WindowHost, WindowKind};

thread_local! {
    static BOOK_WINDOW: RefCell<Option<Rc<BookWindow>>> = const { RefCell::new(None) };
}

/// A close waits for an export or print in flight, but never for ever (§1.5).
const OUTPUT_DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// The minimum content size with a book (two columns) and without (a card) — §1.4.
const MINIMUM_WITH_BOOK: (i32, i32) = (700, 400);
const MINIMUM_EMPTY: (i32, i32) = (260, 320);

/// The pages of the content stack and the detail stack.
const PAGE_BOOK: &str = "book";
const PAGE_EMPTY: &str = "empty";
const PAGE_EDITING: &str = "editing";
const PAGE_STATUS: &str = "status";

/// The `book.` actions this file installs (the sidebar's are in `sidebar`).
const JUMP_HEADING: &str = "jump-heading";
const JUMP_NOTE: &str = "jump-note";
const EXPORT_PDF: &str = "export-pdf";
const EXPORT_EPUB: &str = "export-epub";
const EXPORT_LATEX: &str = "export-latex";
const PRINT: &str = "print";
const PDF_PAGE_SIZE: &str = "pdf-page-size";
const OPENS_IN_SEPARATE_WINDOWS: &str = "opens-in-separate-windows";

/// What the bar slot shows; the bar is rebuilt only when this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BarState {
    Hidden,
    Conflict,
    SaveError(String),
}

/// A status page in the paper typography (§10: 17 / 12 pt typewriter): an icon, a title, a
/// message and an optional button.
struct StatusView {
    root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    message: gtk::Label,
    button: gtk::Button,
}

impl StatusView {
    fn new() -> StatusView {
        let icon = gtk::Image::builder().pixel_size(48).build();
        icon.add_css_class("dim-label");
        let title = gtk::Label::builder().wrap(true).justify(gtk::Justification::Center).build();
        title.add_css_class("md-status-title");
        let message = gtk::Label::builder().wrap(true).max_width_chars(48).justify(gtk::Justification::Center).build();
        message.add_css_class("md-status-message");
        message.add_css_class("dim-label");
        let button = gtk::Button::builder().halign(gtk::Align::Center).margin_top(6).build();
        button.add_css_class("pill");
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .margin_start(24)
            .margin_end(24)
            .margin_top(24)
            .margin_bottom(24)
            .build();
        root.append(&icon);
        root.append(&title);
        root.append(&message);
        root.append(&button);
        StatusView { root, icon, title, message, button }
    }

    fn show(&self, icon: &str, title: &str, message: &str, button: Option<&str>) {
        self.icon.set_icon_name(Some(icon));
        self.title.set_label(title);
        self.message.set_label(message);
        self.button.set_visible(button.is_some());
        if let Some(label) = button {
            self.button.set_label(label);
        }
    }
}

/// The detail toolbar in the header bar (§8.4) and the sidebar toggle at its start.
struct Toolbar {
    sidebar_toggle: gtk::ToggleButton,
    modes: [(gtk::ToggleButton, ViewMode); 3],
    previous: gtk::Button,
    next: gtk::Button,
    contents: gtk::MenuButton,
    notes: gtk::MenuButton,
    export: gtk::MenuButton,
    contents_menu: gio::Menu,
    notes_menu: gio::Menu,
    /// Everything above, shown only with a book: No Book Open is a card with nothing to act on
    /// (md.win's empty state hides its command bar with the split view).
    groups: Vec<gtk::Widget>,
}

impl Toolbar {
    fn new(header: &adw::HeaderBar) -> Toolbar {
        let sidebar_toggle = gtk::ToggleButton::builder().icon_name("sidebar-show-symbolic").active(true).build();
        header.pack_start(&sidebar_toggle);

        let mode = |icon: &str, mode: ViewMode| {
            let button = gtk::ToggleButton::builder().icon_name(icon).tooltip_text(books::VIEW_MODE_TOOLTIP).build();
            (button, mode)
        };
        let modes = [
            mode("document-edit-symbolic", ViewMode::Edit),
            mode("view-dual-symbolic", ViewMode::Split),
            mode("view-reveal-symbolic", ViewMode::Preview),
        ];
        modes[1].0.set_group(Some(&modes[0].0));
        modes[2].0.set_group(Some(&modes[0].0));
        let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        mode_box.add_css_class("linked");
        for (button, _) in &modes {
            mode_box.append(button);
        }

        let previous =
            gtk::Button::builder().icon_name("go-up-symbolic").tooltip_text(books::PREVIOUS_ARTICLE_TOOLTIP).build();
        let next =
            gtk::Button::builder().icon_name("go-down-symbolic").tooltip_text(books::NEXT_ARTICLE_TOOLTIP).build();
        let step_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        step_box.add_css_class("linked");
        step_box.append(&previous);
        step_box.append(&next);

        let contents_menu = gio::Menu::new();
        let contents = gtk::MenuButton::builder()
            .icon_name("view-list-bullet-symbolic")
            .tooltip_text(books::CONTENTS_TOOLTIP)
            .menu_model(&contents_menu)
            .build();
        let notes_menu = gio::Menu::new();
        let notes = gtk::MenuButton::builder()
            .icon_name("accessories-text-editor-symbolic")
            .tooltip_text(books::NOTES_TOOLTIP)
            .menu_model(&notes_menu)
            .build();
        let export = gtk::MenuButton::builder()
            .icon_name("document-send-symbolic")
            .tooltip_text(books::SHARE_TOOLTIP)
            .menu_model(&export_menu())
            .build();

        // `pack_end` fills from the far end: the toolbar reads mode, steps, Contents, Notes, Export.
        header.pack_end(&export);
        header.pack_end(&notes);
        header.pack_end(&contents);
        header.pack_end(&step_box);
        header.pack_end(&mode_box);
        let groups = vec![
            sidebar_toggle.clone().upcast(),
            mode_box.upcast(),
            step_box.upcast(),
            contents.clone().upcast(),
            notes.clone().upcast(),
            export.clone().upcast(),
        ];
        Toolbar { sidebar_toggle, modes, previous, next, contents, notes, export, contents_menu, notes_menu, groups }
    }
}

/// The detail toolbar's Export menu (§8.4): Export as PDF…, PDF Page Size ▸ · Export as EPUB…,
/// Export as LaTeX… · Print… · Open Articles in Separate Windows. No "Share as PDF" (§14 row 10).
fn export_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let pdf = gio::Menu::new();
    pdf.append(Some(books::EXPORT_AS_PDF), Some(&format!("book.{EXPORT_PDF}")));
    let sizes = gio::Menu::new();
    for size in PageSize::all() {
        let item = gio::MenuItem::new(Some(size.label), None);
        item.set_action_and_target_value(Some(&format!("book.{PDF_PAGE_SIZE}")), Some(&size.id.to_variant()));
        sizes.append_item(&item);
    }
    pdf.append_submenu(Some(books::PDF_PAGE_SIZE_LABEL), &sizes);
    menu.append_section(None, &pdf);
    let books_section = gio::Menu::new();
    books_section.append(Some(books::EXPORT_AS_EPUB), Some(&format!("book.{EXPORT_EPUB}")));
    books_section.append(Some(books::EXPORT_AS_LATEX), Some(&format!("book.{EXPORT_LATEX}")));
    menu.append_section(None, &books_section);
    let print = gio::Menu::new();
    print.append(Some(books::PRINT_ROW), Some(&format!("book.{PRINT}")));
    menu.append_section(None, &print);
    let separate = gio::Menu::new();
    separate.append(Some(books::OPEN_ARTICLES_IN_SEPARATE_WINDOWS), Some(&format!("book.{OPENS_IN_SEPARATE_WINDOWS}")));
    menu.append_section(None, &separate);
    menu
}

/// The Book window.
pub struct BookWindow {
    me: Weak<BookWindow>,
    shell: Rc<Shell>,
    settings: Rc<dyn SettingsStore>,
    window: adw::ApplicationWindow,
    title: adw::WindowTitle,
    menu_bar: MenuBar,
    overlay: gtk::Overlay,
    content: gtk::Stack,
    paned: gtk::Paned,
    sidebar: Rc<BookSidebar>,
    detail: gtk::Stack,
    stage_view: StatusView,
    empty_view: StatusView,
    toolbar: Toolbar,
    actions: gio::SimpleActionGroup,
    panes: Rc<ArticlePanes>,
    bar: Rc<DocumentBar>,
    footer: Rc<Footer>,
    preview: Rc<PreviewHost>,
    coordinator: Rc<PreviewCoordinator>,
    exports: Rc<WindowExports>,
    alerts: Rc<AdwAlerts>,
    pickers: Rc<GtkPickers>,
    session: Rc<TextFileSession>,
    navigator: Rc<BookNavigatorModel>,
    gate: RefCell<Option<BookFlushGateBinding>>,
    state: Rc<DocumentWindowState>,
    view_modes: ViewModeController,
    derived: Rc<DerivedTextScheduler>,
    dispatcher: Rc<CommandDispatcher>,
    /// The mode last written to `md.bookViewMode`.
    stored_mode: Cell<ViewMode>,
    /// The mode last handed to the panes.
    applied_mode: Cell<Option<ViewMode>>,
    undo_generation: Cell<u64>,
    derived_text: RefCell<Option<String>>,
    layout: Cell<Option<PaneLayout>>,
    bar_state: RefCell<BarState>,
    rendering: Cell<bool>,
    syncing_toolbar: Cell<bool>,
    /// An alert is up in this window: every `win.` command is off until it answers (§2.2).
    dialog_open: Cell<bool>,
    closing: Cell<bool>,
    closed: Cell<bool>,
    outputs_in_flight: Rc<Cell<usize>>,
    /// Alerts raised while the window is closing (a rescue report): the close waits for them.
    close_alerts: RefCell<Vec<LocalBoxFuture<'static, ()>>>,
    /// Undo the subscriptions on app-wide objects, which outlive the window.
    unsubscribe: RefCell<Vec<Box<dyn FnOnce()>>>,
}

impl BookWindow {
    /// Creates the window on first use and presents it.
    pub fn show(shell: &Rc<Shell>) -> Rc<BookWindow> {
        let book = BookWindow::current().unwrap_or_else(|| {
            let book = BookWindow::new(shell);
            BOOK_WINDOW.with(|w| *w.borrow_mut() = Some(Rc::clone(&book)));
            book.open_stored_book();
            book
        });
        book.window.present();
        book
    }

    /// The Book window, while it is open.
    pub fn current() -> Option<Rc<BookWindow>> {
        BOOK_WINDOW.with(|w| w.borrow().clone())
    }

    fn new(shell: &Rc<Shell>) -> Rc<BookWindow> {
        let settings: Rc<dyn SettingsStore> = shell.settings.clone();
        let size = window_placement::parse_size(
            settings.get_string(settings_keys::BOOK_WINDOW_SIZE).as_deref(),
            window_placement::BOOK_DEFAULT,
        );

        let title = adw::WindowTitle::new(strings::BOOK, "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        let toolbar = Toolbar::new(&header);
        let menu_bar = MenuBar::new(shell, WindowKind::Book);

        let overlay = gtk::Overlay::new();
        let toolbar_view = adw::ToolbarView::new();
        toolbar_view.add_top_bar(&header);
        toolbar_view.add_top_bar(&menu_bar.widget());
        toolbar_view.set_content(Some(&overlay));
        let window = adw::ApplicationWindow::builder()
            .application(&shell.app)
            .title(strings::BOOK)
            .default_width(size.width)
            .default_height(size.height)
            .content(&toolbar_view)
            .build();
        let toplevel = window.clone().upcast::<gtk::Window>();
        let alerts = AdwAlerts::new(&toplevel);
        let pickers = GtkPickers::new(&toplevel, alerts.clone());

        // The article session: the book pane never owns a file in the registry (window id None);
        // it only asks who does, which keeps the handoff one-directional (§8.6).
        let session = TextFileSession::new(
            shell.files.clone(),
            GioFileWatcher::new(),
            shell.scheduler.clone(),
            shell.registry.clone(),
            shell.identity.clone(),
            SessionRole::BookArticle,
            None,
        );
        let navigator = Rc::new(BookNavigatorModel::new(
            Rc::new(TextFileSessionArticles::new(Rc::clone(&session))),
            Rc::clone(&settings),
            shell.book.article_opens(),
        ));

        // An article is exempt from the per-file memory both ways, and md.bookViewMode is the one
        // mode the whole book shares (§8.5): the controller's store is provably not
        // md.viewModeMemory, so "books never touch it" holds by construction.
        let state = Rc::new(DocumentWindowState::new());
        state.mark_book_article();
        let stored_mode = ViewMode::from_raw_value(settings.get_string(settings_keys::BOOK_VIEW_MODE).as_deref())
            .unwrap_or(ViewMode::WINDOW_DEFAULT);
        state.set_stored_mode(stored_mode);
        let view_modes = ViewModeController::new(
            Rc::clone(&state),
            Rc::new(InMemoryViewModeStore::new()),
            shell.book.article_opens(),
        );

        let panes = ArticlePanes::new(shell, PanesRole::BookArticle);
        let preview = PreviewHost::new(&shell.web);
        panes.set_preview(&preview);
        let coordinator = PreviewCoordinator::new(
            preview.surface(),
            shell.scheduler.clone(),
            shell.spawner.clone(),
            preview_index_url(&preview),
        );
        let derived = DerivedTextScheduler::new(
            shell.scheduler.clone(),
            shell.spawner.clone(),
            Rc::new(GioBlocking),
            shell.words.clone(),
        );
        let bar = DocumentBar::new();
        let footer = Footer::new();

        let actions = gio::SimpleActionGroup::new();
        window.insert_action_group("book", Some(&actions));
        let sidebar = BookSidebar::new(Rc::clone(&navigator), alerts.clone(), &actions);

        let editing = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let panes_widget = panes.widget();
        panes_widget.set_vexpand(true);
        editing.append(&panes_widget);
        editing.append(&bar.widget());
        editing.append(&footer.widget());
        let stage_view = StatusView::new();
        let detail = gtk::Stack::new();
        detail.set_hexpand(true);
        detail.add_named(&editing, Some(PAGE_EDITING));
        detail.add_named(&stage_view.root, Some(PAGE_STATUS));

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&sidebar.widget())
            .end_child(&detail)
            .position(sidebar::WIDTH_IDEAL)
            .resize_start_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .build();
        let empty_view = StatusView::new();
        empty_view.show(
            "x-office-address-book-symbolic",
            books::NO_BOOK_OPEN,
            books::NO_BOOK_OPEN_MESSAGE,
            Some(buttons::OPEN_BOOK),
        );
        let content = gtk::Stack::new();
        content.add_named(&paned, Some(PAGE_BOOK));
        content.add_named(&empty_view.root, Some(PAGE_EMPTY));
        content.set_visible_child_name(PAGE_EMPTY);
        overlay.set_child(Some(&content));

        let book = Rc::new_cyclic(|me: &Weak<BookWindow>| {
            let host: Weak<dyn WindowHost> = me.clone();
            let exports = WindowExports::new(shell, host);
            let snapshot_of = me.clone();
            let clock: Rc<dyn Clock> = shell.scheduler.clone();
            let dispatcher = Rc::new(CommandDispatcher::new(clock, move || {
                snapshot_of.upgrade().map(|book| book.snapshot()).unwrap_or_else(ShellSnapshot::empty)
            }));
            BookWindow {
                me: me.clone(),
                shell: Rc::clone(shell),
                settings,
                window,
                title,
                menu_bar,
                overlay,
                content,
                paned,
                sidebar,
                detail,
                stage_view,
                empty_view,
                toolbar,
                actions,
                panes,
                bar,
                footer,
                preview,
                coordinator,
                exports,
                alerts,
                pickers,
                session,
                navigator,
                gate: RefCell::new(None),
                state,
                view_modes,
                derived,
                dispatcher,
                stored_mode: Cell::new(stored_mode),
                applied_mode: Cell::new(None),
                undo_generation: Cell::new(0),
                derived_text: RefCell::new(None),
                layout: Cell::new(None),
                bar_state: RefCell::new(BarState::Hidden),
                rendering: Cell::new(false),
                syncing_toolbar: Cell::new(false),
                dialog_open: Cell::new(false),
                closing: Cell::new(false),
                closed: Cell::new(false),
                outputs_in_flight: Rc::new(Cell::new(0)),
                close_alerts: RefCell::new(Vec::new()),
                unsubscribe: RefCell::new(Vec::new()),
            }
        });
        book.wire();
        book.install_actions();
        // md.win's order: the menu is built, the actions installed, the handlers registered, then
        // the menus refreshed from the snapshot.
        install_window_actions(&book.window, WindowKind::Book, &book.dispatcher);
        book.register_commands();
        book.preview.apply_theme(theme::is_dark());
        book.shell.registry.add(BOOK_WINDOW_ID, strings::BOOK);
        book.render();
        book
    }

    // ───────────────────────────── wiring ─────────────────────────────

    fn wire(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let on = move |run: fn(&BookWindow)| {
            let weak = weak.clone();
            move || {
                if let Some(book) = weak.upgrade() {
                    run(&book);
                }
            }
        };

        // The flush gate: every whole-book output saves this article before it reads the folder.
        let session = Rc::downgrade(&self.session);
        *self.gate.borrow_mut() = Some(BookFlushGateBinding::new(&self.shell.book.flush_hub(), move || {
            session.upgrade().is_none_or(|session| session.flush_now(false))
        }));

        // ---- the session ----
        let render = on(BookWindow::render);
        self.session.changed().connect(move |_| render());
        let weak = self.me.clone();
        self.session.text_replaced_externally().connect(move |text: &str| {
            if let Some(book) = weak.upgrade() {
                book.text_replaced(text);
            }
        });
        let weak = self.me.clone();
        self.session.alert_requested().connect(move |(title, message): &(String, String)| {
            if let Some(book) = weak.upgrade() {
                book.warn(title, message);
            }
        });

        // ---- the navigator ----
        let render = on(BookWindow::render);
        self.navigator.changed().connect(move |_| render());
        let state = Rc::downgrade(&self.state);
        self.navigator.selection_changing().connect(move |_| {
            // A one-shot jump must never replay into the next article (book.md §13.7).
            if let Some(state) = state.upgrade() {
                state.set_preview_navigation(None);
                state.set_editor_jump(None);
            }
        });
        let shell = Rc::downgrade(&self.shell);
        self.navigator.open_requested().connect(move |path: &str| {
            if let Some(shell) = shell.upgrade() {
                shell.windows.open_path(path);
            }
        });
        let weak = self.me.clone();
        self.navigator.alert_requested().connect(move |(title, message): &(String, String)| {
            if let Some(book) = weak.upgrade() {
                book.warn(title, message);
            }
        });

        // ---- the view state, the derived text, the panes ----
        let changed = on(BookWindow::state_changed);
        self.state.changed().connect(move |_| changed());
        let weak = self.me.clone();
        self.derived.changed().connect(move |derived: &DerivedText| {
            if let Some(book) = weak.upgrade() {
                book.footer.update(derived);
                book.state.set_derived(derived.clone());
                book.publish();
            }
        });
        let weak = self.me.clone();
        self.panes.layout_changed().connect(move |layout: &PaneLayout| {
            if let Some(book) = weak.upgrade() {
                book.layout.set(Some(*layout));
                book.render();
            }
        });
        let editor = self.panes.editor();
        let session = Rc::downgrade(&self.session);
        editor.text_edited().connect(move |text: &str| {
            if let Some(session) = session.upgrade() {
                session.edit(text);
            }
        });
        let publish = on(BookWindow::publish);
        editor.selection_changed().connect(move |_| publish());
        let publish = on(BookWindow::publish);
        editor.history_changed().connect(move |_| publish());
        editor.use_settings(Some(Rc::clone(&self.settings)));
        let weak = self.me.clone();
        self.alerts.open_changed().connect(move |open: &bool| {
            if let Some(book) = weak.upgrade() {
                book.dialog_open.set(*open);
                book.publish();
            }
        });
        let footer = Rc::downgrade(&self.footer);
        self.exports.busy_changed().connect(move |busy: &bool| {
            if let Some(footer) = footer.upgrade() {
                footer.set_busy(*busy);
            }
        });

        // ---- app-wide objects: subscriptions undone at cleanup ----
        let weak = self.me.clone();
        let id = self.shell.book.changed().connect(move |_| {
            if let Some(book) = weak.upgrade() {
                book.open_stored_book();
            }
        });
        let shell = Rc::clone(&self.shell);
        self.unsubscribe.borrow_mut().push(Box::new(move || shell.book.changed().disconnect(id)));

        let weak = self.me.clone();
        let id = self.settings.changed().connect(move |key: &str| {
            if let Some(book) = weak.upgrade() {
                book.setting_changed(key);
            }
        });
        let settings = Rc::clone(&self.settings);
        self.unsubscribe.borrow_mut().push(Box::new(move || settings.changed().disconnect(id)));

        // Ownership moves when a document window takes or lets go of the article: recheck on the
        // next turn (the registry is mid-update while it notifies).
        let weak = self.me.clone();
        let id = self.shell.registry.changed().connect(move |_| {
            let weak = weak.clone();
            glib::idle_add_local_once(move || {
                if let Some(book) = weak.upgrade() {
                    book.recheck_ownership();
                }
            });
        });
        let shell = Rc::clone(&self.shell);
        self.unsubscribe.borrow_mut().push(Box::new(move || shell.registry.changed().disconnect(id)));

        let publish = on(BookWindow::publish);
        let id = self.shell.recent.changed().connect(move |_| publish());
        let shell = Rc::clone(&self.shell);
        self.unsubscribe.borrow_mut().push(Box::new(move || shell.recent.changed().disconnect(id)));

        let weak = self.me.clone();
        let theme_changed = theme::changed();
        let id = theme_changed.connect(move |dark: &bool| {
            if let Some(book) = weak.upgrade() {
                book.preview.apply_theme(*dark);
                book.render();
            }
        });
        self.unsubscribe.borrow_mut().push(Box::new(move || theme_changed.disconnect(id)));

        // The Mac's didBecomeKeyNotification observer: any md window becoming active rechecks who
        // owns the article (§8.6).
        let weak = self.me.clone();
        let id = self.shell.app.connect_active_window_notify(move |_| {
            if let Some(book) = weak.upgrade() {
                book.recheck_ownership();
            }
        });
        let app = self.shell.app.clone();
        self.unsubscribe.borrow_mut().push(Box::new(move || app.disconnect(id)));

        // Logout (§1.5 route 3): the article is written now, synchronously, rescued on failure.
        let weak = self.me.clone();
        let id = self.shell.app.connect_query_end(move |_| {
            if let Some(book) = weak.upgrade() {
                book.quit_flush();
            }
        });
        let app = self.shell.app.clone();
        self.unsubscribe.borrow_mut().push(Box::new(move || app.disconnect(id)));

        // ---- the window ----
        let weak = self.me.clone();
        self.window.connect_is_active_notify(move |window| {
            // Deactivation saves, as a document window does (§1.5).
            if !window.is_active()
                && let Some(book) = weak.upgrade()
            {
                book.session.flush_now(false);
            }
        });
        let publish = on(BookWindow::publish);
        self.window.connect_fullscreened_notify(move |_| publish());
        let weak = self.me.clone();
        self.window.connect_close_request(move |_| {
            if let Some(book) = weak.upgrade() {
                book.request_close();
            }
            glib::Propagation::Stop
        });
        // GTK 4.22's `gtk_window_destroy` only unrealizes and lets go of GTK's own reference; the
        // `destroy` signal waits for the last reference, which this window holds. So the close
        // route cleans up before it destroys, and a destroy from elsewhere (Quit's window manager)
        // is caught here, at the unrealize every destroy performs.
        let weak = self.me.clone();
        self.window.connect_unrealize(move |_| {
            if let Some(book) = weak.upgrade() {
                book.cleanup();
            }
        });

        // ---- the chrome ----
        let weak = self.me.clone();
        self.empty_view.button.connect_clicked(move |_| {
            if let Some(book) = weak.upgrade() {
                let host: Rc<dyn WindowHost> = book;
                crate::book::open_book(&Shell::get(), Some(host));
            }
        });
        let show_owner = on(BookWindow::show_owner);
        self.stage_view.button.connect_clicked(move |_| show_owner());
        for (button, mode) in &self.toolbar.modes {
            let (weak, mode) = (self.me.clone(), *mode);
            button.connect_toggled(move |button| {
                if let Some(book) = weak.upgrade()
                    && button.is_active()
                    && !book.syncing_toolbar.get()
                {
                    book.view_modes.select(mode);
                }
            });
        }
        let previous = on(|book| book.navigator.step_previous());
        self.toolbar.previous.connect_clicked(move |_| previous());
        let next = on(|book| book.navigator.step_next());
        self.toolbar.next.connect_clicked(move |_| next());
        let weak = self.me.clone();
        self.toolbar.sidebar_toggle.connect_toggled(move |toggle| {
            if let Some(book) = weak.upgrade()
                && !book.syncing_toolbar.get()
            {
                book.set_sidebar_open(toggle.is_active());
            }
        });
        let weak = self.me.clone();
        self.toolbar.contents.set_create_popup_func(move |_| {
            if let Some(book) = weak.upgrade() {
                book.fill_contents_menu();
            }
        });
        let weak = self.me.clone();
        self.toolbar.notes.set_create_popup_func(move |_| {
            if let Some(book) = weak.upgrade() {
                book.fill_notes_menu();
            }
        });

        // The sidebar is resizable within the Mac's 200–320 (md.win could not resize it at all).
        self.paned.connect_position_notify(|paned| {
            let shown = paned.start_child().is_some_and(|child| child.is_visible());
            let position = paned.position();
            let clamped = sidebar::clamp_width(position);
            if shown && position != clamped {
                paned.set_position(clamped);
            }
        });

        // Files dropped on the window open as documents, exactly as from a document window (§6.1):
        // a dropped file is not an article of the book, and nothing is copied into the folder.
        let drop = gtk::DropTarget::new(gtk::gdk::FileList::static_type(), gtk::gdk::DragAction::COPY);
        drop.set_propagation_phase(gtk::PropagationPhase::Capture);
        let shell = Rc::downgrade(&self.shell);
        drop.connect_drop(move |_, value, _, _| {
            let (Some(shell), Ok(files)) = (shell.upgrade(), value.get::<gtk::gdk::FileList>()) else {
                return false;
            };
            for file in files.files() {
                let Some(path) = file.path() else { continue };
                let is_folder = file.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE)
                    == gio::FileType::Directory;
                let item = ActivationItem { path: path.to_string_lossy().into_owned(), is_folder };
                match activation_router::classify(&item) {
                    Some(ActivationAction::OpenPath(path)) => {
                        shell.windows.open_path(&path);
                    }
                    Some(ActivationAction::ImportTextPack(path)) => shell.windows.import_text_pack(&path),
                    Some(ActivationAction::ImportTextBundleFolder(path)) => shell.windows.import_text_bundle(&path),
                    _ => {}
                }
            }
            true
        });
        self.overlay.add_controller(drop);
    }

    /// The `book.` actions of the detail toolbar's menus (the sidebar adds its own).
    fn install_actions(self: &Rc<Self>) {
        let simple = |name: &str, run: fn(&BookWindow)| {
            let action = gio::SimpleAction::new(name, None);
            let weak = self.me.clone();
            action.connect_activate(move |_, _| {
                if let Some(book) = weak.upgrade() {
                    run(&book);
                }
            });
            self.actions.add_action(&action);
        };
        simple(EXPORT_PDF, |book| book.book_output(BookOutputKind::Pdf));
        simple(EXPORT_EPUB, |book| book.book_output(BookOutputKind::Epub));
        simple(EXPORT_LATEX, |book| book.book_output(BookOutputKind::LaTeX));
        simple(PRINT, |book| book.book_output(BookOutputKind::Print));

        let jump = |name: &str, run: fn(&BookWindow, usize)| {
            let action = gio::SimpleAction::new(name, Some(glib::VariantTy::UINT32));
            let weak = self.me.clone();
            action.connect_activate(move |_, target| {
                if let (Some(book), Some(index)) = (weak.upgrade(), target.and_then(|t| t.get::<u32>())) {
                    run(&book, index as usize);
                }
            });
            self.actions.add_action(&action);
        };
        jump(JUMP_HEADING, |book, index| {
            if let Some(entry) = book.state.derived().outline.get(index) {
                book.view_modes.jump_to_heading(entry);
            }
        });
        jump(JUMP_NOTE, |book, index| {
            if let Some(note) = book.state.derived().notes.get(index) {
                book.view_modes.jump_to_note(note);
            }
        });

        // The two settings the Export menu ticks, shared with the menu bar's rows (the same keys).
        let page_size = PageSize::named(self.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref()).id;
        let action =
            gio::SimpleAction::new_stateful(PDF_PAGE_SIZE, Some(glib::VariantTy::STRING), &page_size.to_variant());
        let settings = Rc::clone(&self.settings);
        action.connect_activate(move |_, target| {
            if let Some(id) = target.and_then(|t| t.get::<String>()) {
                settings.set_string(settings_keys::PDF_PAGE_SIZE, &id);
            }
        });
        self.actions.add_action(&action);
        let separate = gio::SimpleAction::new_stateful(
            OPENS_IN_SEPARATE_WINDOWS,
            None,
            &self.navigator.opens_in_separate_windows().to_variant(),
        );
        let navigator = Rc::downgrade(&self.navigator);
        separate.connect_activate(move |_, _| {
            if let Some(navigator) = navigator.upgrade() {
                navigator.set_opens_in_separate_windows(!navigator.opens_in_separate_windows());
            }
        });
        self.actions.add_action(&separate);
    }

    /// Every command the Book window can enable (shell-design §2; md.win's `RegisterCommands` plus
    /// the File / Edit / Window rows md.win's shell package registered on it).
    fn register_commands(self: &Rc<Self>) {
        let d = &self.dispatcher;
        let on = |run: fn(&BookWindow)| {
            let weak = self.me.clone();
            move || {
                if let Some(book) = weak.upgrade() {
                    run(&book);
                }
            }
        };
        let with = |run: fn(&BookWindow, &CommandArgument)| {
            let weak = self.me.clone();
            move |argument: &CommandArgument| {
                if let Some(book) = weak.upgrade() {
                    run(&book, argument);
                }
            }
        };

        // File: the article (book.md §14 — File acts on the article, never on the whole book).
        d.register(CommandId::Close, on(|book| book.request_close()));
        d.register(
            CommandId::Save,
            on(|book| {
                book.session.flush_now(true);
            }),
        );
        d.register(CommandId::Duplicate, on(BookWindow::duplicate));
        for id in [
            CommandId::Print,
            CommandId::ExportPdf,
            CommandId::ExportHtml,
            CommandId::ExportEpub,
            CommandId::ExportLaTeX,
            CommandId::ExportTextBundle,
        ] {
            let weak = self.me.clone();
            d.register(id, move || {
                if let Some(book) = weak.upgrade() {
                    book.article_output(id, None);
                }
            });
        }
        d.register_with_argument(
            CommandId::ExportDiagramSvg,
            with(|book, argument| {
                if let CommandArgument::Diagram(ordinal) = argument {
                    book.article_output(CommandId::ExportDiagramSvg, Some(*ordinal));
                }
            }),
        );
        d.register_with_argument(
            CommandId::PdfPageSize,
            with(|book, argument| {
                if let CommandArgument::Text(id) = argument {
                    book.settings.set_string(settings_keys::PDF_PAGE_SIZE, id);
                }
            }),
        );

        // Edit: the focused widget's (§2.2).
        for id in [
            CommandId::Undo,
            CommandId::Redo,
            CommandId::Cut,
            CommandId::Copy,
            CommandId::Paste,
            CommandId::Delete,
            CommandId::SelectAll,
        ] {
            let weak = self.me.clone();
            d.register(id, move || {
                if let Some(book) = weak.upgrade() {
                    book.edit_command(id);
                }
            });
        }
        // Find… over the preview alone first brings the editor back as a transient nudge, never a
        // stored mode — the Mac's `showEditor` (§2.5).
        d.register(
            CommandId::Find,
            on(|book| {
                book.view_modes.show_editor();
                book.panes.find_bar().show_find();
            }),
        );
        d.register(
            CommandId::Replace,
            on(|book| {
                book.view_modes.show_editor();
                book.panes.find_bar().show_replace();
            }),
        );
        d.register(CommandId::FindNext, on(|book| book.panes.find_bar().find_next(true)));
        d.register(CommandId::FindPrevious, on(|book| book.panes.find_bar().find_next(false)));
        d.register(CommandId::UseSelectionForFind, on(|book| book.panes.find_bar().use_selection()));

        // Format: one edit of the article each.
        for id in [
            CommandId::Bold,
            CommandId::Italic,
            CommandId::InlineCode,
            CommandId::Strikethrough,
            CommandId::Link,
            CommandId::Heading1,
            CommandId::Heading2,
            CommandId::Heading3,
            CommandId::Heading4,
            CommandId::Heading5,
            CommandId::Heading6,
            CommandId::BulletList,
            CommandId::TaskList,
            CommandId::Quote,
            CommandId::Indent,
            CommandId::Outdent,
        ] {
            let weak = self.me.clone();
            d.register(id, move || {
                if let Some(book) = weak.upgrade() {
                    book.panes.editor().apply_format(id);
                }
            });
        }

        // View: md.bookViewMode; full screen is the writing room (no Zen here, §8.1).
        d.register(CommandId::ViewEdit, on(|book| book.view_modes.select(ViewMode::Edit)));
        d.register(CommandId::ViewSplit, on(|book| book.view_modes.select(ViewMode::Split)));
        d.register(CommandId::ViewPreview, on(|book| book.view_modes.select(ViewMode::Preview)));
        d.register(CommandId::ShowSidebar, on(|book| book.set_sidebar_open(!book.sidebar_open())));
        d.register(
            CommandId::FullScreen,
            on(|book| {
                if book.window.is_fullscreen() {
                    book.window.unfullscreen();
                } else {
                    book.window.fullscreen();
                }
            }),
        );

        // Book (app-wide rows; registered here too, as md.win does, for a dispatcher-routed menu).
        d.register(CommandId::NewBook, on(|book| crate::book::new_book(&book.shell, Some(book.host()))));
        d.register(CommandId::OpenBook, on(|book| crate::book::open_book(&book.shell, Some(book.host()))));
        d.register(CommandId::ExampleBook, on(|book| crate::book::unpack_example_book(&book.shell, Some(book.host()))));
        d.register(CommandId::ShowBook, on(|book| book.window.present()));
        d.register(CommandId::CloseBook, on(|book| crate::book::close_book(&book.shell)));
        d.register(CommandId::PrintBook, on(|book| book.book_output(BookOutputKind::Print)));
        d.register(CommandId::ExportBookPdf, on(|book| book.book_output(BookOutputKind::Pdf)));
        d.register(CommandId::ExportBookEpub, on(|book| book.book_output(BookOutputKind::Epub)));
        d.register(CommandId::ExportBookLaTeX, on(|book| book.book_output(BookOutputKind::LaTeX)));

        // Go.
        d.register(CommandId::PreviousArticle, on(|book| book.navigator.step_previous()));
        d.register(CommandId::NextArticle, on(|book| book.navigator.step_next()));
        d.register_with_argument(
            CommandId::Contents,
            with(|book, argument| {
                if let CommandArgument::Outline(entry) = argument {
                    book.view_modes.jump_to_heading(entry);
                }
            }),
        );
        d.register_with_argument(
            CommandId::Notes,
            with(|book, argument| {
                if let CommandArgument::Note(note) = argument {
                    book.view_modes.jump_to_note(note);
                }
            }),
        );

        // Window.
        d.register(CommandId::Minimize, on(|book| book.window.minimize()));
        d.register(
            CommandId::Zoom,
            on(|book| {
                if book.window.is_maximized() {
                    book.window.unmaximize();
                } else {
                    book.window.maximize();
                }
            }),
        );
        d.register_with_argument(
            CommandId::ActivateWindow,
            with(|book, argument| {
                if let CommandArgument::Window(id) = argument {
                    if *id == BOOK_WINDOW_ID {
                        book.window.present();
                    } else {
                        book.shell.windows.present(*id);
                    }
                }
            }),
        );
    }

    // ───────────────────────────── the book ─────────────────────────────

    /// List whatever `md.bookBookmark` says now: open it, or — no book, or a folder out of reach
    /// (the setting kept, the Mac's nil `beginAccess`) — show No Book Open.
    fn open_stored_book(&self) {
        let library = &self.shell.book;
        match library.folder().filter(|_| library.is_accessible()) {
            Some(root) => self.navigator.open_book(&root),
            None if self.navigator.root().is_some() => self.navigator.close_book(),
            None => {}
        }
        self.render();
    }

    fn recheck_ownership(&self) {
        if !self.closed.get() {
            self.session.recheck_ownership();
        }
    }

    /// Handoff's Show Window: bring the document window that owns the article forward.
    fn show_owner(&self) {
        if let Some(id) = self.session.owning_window() {
            self.shell.windows.present(id);
        }
    }

    fn text_replaced(&self, text: &str) {
        let editor = self.panes.editor();
        let generation = self.session.undo_generation();
        if self.undo_generation.replace(generation) != generation {
            // A fresh undo history per article: Ctrl+Z never crosses an article (§8.5).
            editor.reset_history();
        }
        editor.set_text(text);
    }

    fn state_changed(&self) {
        // The single writer of md.bookViewMode: the mode is app-wide, and a nudge stores nothing.
        let mode = self.state.stored_mode();
        if self.stored_mode.replace(mode) != mode {
            self.settings.set_string(settings_keys::BOOK_VIEW_MODE, mode.raw_value());
        }
        self.render();
    }

    fn setting_changed(&self, key: &str) {
        if key == settings_keys::PDF_PAGE_SIZE {
            let id = PageSize::named(self.settings.get_string(key).as_deref()).id;
            if let Some(action) = self.actions.lookup_action(PDF_PAGE_SIZE) {
                action.change_state(&id.to_variant());
            }
            self.publish();
        } else if key == settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS {
            if let Some(action) = self.actions.lookup_action(OPENS_IN_SEPARATE_WINDOWS) {
                action.change_state(&self.navigator.opens_in_separate_windows().to_variant());
            }
            self.render();
        } else if TypingSettings::is_key(key) || wrap_selection_setting::is_key(key) {
            self.publish();
        }
    }

    fn sidebar_open(&self) -> bool {
        self.sidebar.widget().is_visible()
    }

    fn set_sidebar_open(&self, open: bool) {
        self.sidebar.widget().set_visible(open);
        self.render();
    }

    fn host(&self) -> Rc<dyn WindowHost> {
        self.me.upgrade().expect("a live Book window")
    }

    /// File ▸ Duplicate: the article's text in a new untitled document window.
    fn duplicate(&self) {
        if matches!(self.session.stage(), Stage::Editing(_)) {
            self.shell.windows.open_untitled(Some(self.session.text()), true);
        }
    }

    /// File ▸ Print… / Export ▸ … from the Book window: the article being written, saved first.
    fn article_output(&self, id: CommandId, diagram: Option<usize>) {
        let Stage::Editing(path) = self.session.stage() else { return };
        self.session.flush_now(false);
        let (source, title) = (self.session.text(), self.session.title());
        let exports = &self.exports;
        match id {
            CommandId::Print => exports.print(source, title),
            CommandId::ExportPdf => exports.export_pdf(source, title),
            CommandId::ExportHtml => exports.export_html(source, title),
            CommandId::ExportEpub => exports.export_epub(source, title, Some(path)),
            CommandId::ExportLaTeX => exports.export_latex(source, title),
            CommandId::ExportTextBundle => exports.export_text_bundle(source, Some(path), title),
            CommandId::ExportDiagramSvg => {
                let ordinal = diagram.and_then(|d| u32::try_from(d).ok()).unwrap_or(0);
                exports.export_diagram_svg(source, title, ordinal);
            }
            _ => {}
        }
    }

    /// A whole-book output, rendered in this window.
    fn book_output(&self, kind: BookOutputKind) {
        crate::book::run_book_output(&self.shell, Some(self.host()), kind);
    }

    /// This window's exports — the renderer Book outputs use while the window exists (md.win §8.7).
    pub fn exports(&self) -> Rc<WindowExports> {
        Rc::clone(&self.exports)
    }

    /// Edit's clipboard and history rows go to the focused widget (§2.2): a find field edits
    /// itself, the preview copies its own selection, and everything else is the article editor's.
    fn edit_command(&self, id: CommandId) {
        let editor = self.panes.editor();
        if let Some(focus) = GtkWindowExt::focus(&self.window) {
            let editor_widget = editor.widget();
            if focus != editor_widget && !focus.is_ancestor(&editor_widget) {
                if focus.is::<gtk::Text>() {
                    let action = match id {
                        CommandId::Undo => "text.undo",
                        CommandId::Redo => "text.redo",
                        CommandId::Cut => "clipboard.cut",
                        CommandId::Copy => "clipboard.copy",
                        CommandId::Paste => "clipboard.paste",
                        CommandId::Delete => "selection.delete",
                        _ => "selection.select-all",
                    };
                    let _ = focus.activate_action(action, None);
                    return;
                }
                if let Some(view) = focus.downcast_ref::<webkit::WebView>() {
                    match id {
                        CommandId::Copy => view.execute_editing_command("Copy"),
                        CommandId::SelectAll => view.execute_editing_command("SelectAll"),
                        _ => {}
                    }
                    return;
                }
            }
        }
        match id {
            CommandId::Undo => editor.undo(),
            CommandId::Redo => editor.redo(),
            CommandId::Cut => editor.cut(),
            CommandId::Copy => editor.copy(),
            CommandId::Paste => editor.paste(),
            CommandId::Delete => editor.delete_selection(),
            CommandId::SelectAll => editor.select_all(),
            _ => {}
        }
    }

    // ───────────────────────────── rendering ─────────────────────────────

    /// Show what the navigator and the session say. Re-entrant calls (applying the mode may raise
    /// the panes' layout change) are dropped: what they would react to is already being applied.
    fn render(&self) {
        if self.closed.get() || self.rendering.replace(true) {
            return;
        }
        let book = self.navigator.book();
        let has_book = book.is_some();
        self.content.set_visible_child_name(if has_book { PAGE_BOOK } else { PAGE_EMPTY });
        self.render_title(book.as_ref());
        self.sidebar.render(book.as_ref(), self.navigator.selection().as_deref());
        self.render_stage();
        self.render_toolbar(has_book);
        self.render_bar();
        let (width, height) = if has_book { MINIMUM_WITH_BOOK } else { MINIMUM_EMPTY };
        self.window.set_size_request(width, height);
        self.rendering.set(false);
        self.publish();
    }

    fn render_title(&self, book: Option<&Book>) {
        let article = matches!(self.session.stage(), Stage::Editing(_)).then(|| self.session.title());
        let title = window_title::for_book(book.map(Book::name), article.as_deref());
        self.window.set_title(Some(&title));
        self.title.set_title(&title);
        let folder = book.map(|book| display_folder(&book.root, &glib::home_dir().to_string_lossy()));
        self.title.set_subtitle(folder.as_deref().unwrap_or(""));
    }

    fn render_stage(&self) {
        let Stage::Editing(path) = self.session.stage() else {
            self.detail.set_visible_child_name(PAGE_STATUS);
            self.coordinator.hide();
            self.preview.set_shown(false);
            self.derived.cancel();
            *self.derived_text.borrow_mut() = None;
            self.state.set_derived(DerivedText::empty());
            self.footer.update(&DerivedText::empty());
            let title = self.session.title();
            match self.session.stage() {
                Stage::Handoff(_) => self.stage_view.show(
                    "view-restore-symbolic",
                    books::HANDOFF_TITLE,
                    &books::handoff_message(&title),
                    Some(buttons::SHOW_WINDOW),
                ),
                Stage::Unreadable(_) => self.stage_view.show(
                    "dialog-warning-symbolic",
                    books::UNREADABLE_TITLE,
                    &books::unreadable_message(&title),
                    None,
                ),
                _ => self.stage_view.show("document-edit-symbolic", books::EMPTY_TITLE, books::EMPTY_MESSAGE, None),
            }
            return;
        };
        self.detail.set_visible_child_name(PAGE_EDITING);
        let mode = self.state.effective_mode();
        if self.applied_mode.replace(Some(mode)) != Some(mode) {
            self.panes.set_mode(mode);
        }
        let text = self.session.text();
        if self.derived_text.borrow().as_deref() != Some(text.as_str()) {
            *self.derived_text.borrow_mut() = Some(text.clone());
            self.derived.text_changed(&text);
        }
        let shows_preview = SplitLayout::shows_preview(self.current_layout());
        self.preview.set_shown(shows_preview);
        if shows_preview {
            self.coordinator.show();
        } else {
            self.coordinator.hide();
        }
        // The article's path is the document token: another article is a fresh page at the top,
        // never a scroll-preserving reload of the previous one (§8.5).
        self.coordinator.update(&text, &self.session.title(), theme::is_dark(), Some(&path));
        if let Some(jump) = self.state.editor_jump() {
            self.panes.editor().apply_jump(jump.clone());
            self.state.editor_jump_handled(jump.id);
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
    }

    /// The panes' arrangement: the last one they reported when it fits the mode, else the rule's
    /// answer for a wide pane.
    fn current_layout(&self) -> PaneLayout {
        let mode = self.state.effective_mode();
        let fits = |layout: PaneLayout| match mode {
            ViewMode::Edit => layout == PaneLayout::EditorOnly,
            ViewMode::Preview => layout == PaneLayout::PreviewOnly,
            ViewMode::Split => matches!(layout, PaneLayout::SideBySide | PaneLayout::Stacked),
        };
        self.layout
            .get()
            .filter(|layout| fits(*layout))
            .unwrap_or_else(|| SplitLayout::arrange(mode, f64::from(i16::MAX)))
    }

    fn render_toolbar(&self, has_book: bool) {
        let editing = matches!(self.session.stage(), Stage::Editing(_));
        let mode = self.state.effective_mode();
        self.syncing_toolbar.set(true);
        for (button, button_mode) in &self.toolbar.modes {
            button.set_sensitive(editing);
            if *button_mode == mode && !button.is_active() {
                button.set_active(true);
            }
        }
        self.toolbar.sidebar_toggle.set_active(self.sidebar_open());
        self.syncing_toolbar.set(false);
        for widget in &self.toolbar.groups {
            widget.set_visible(has_book);
        }

        let stepper = self.navigator.stepper();
        self.toolbar.previous.set_sensitive(stepper.can_previous());
        self.toolbar.next.set_sensitive(stepper.can_next());
        let derived = self.state.derived();
        self.toolbar.contents.set_sensitive(editing && !derived.outline.is_empty());
        self.toolbar.notes.set_sensitive(editing && !derived.notes.is_empty());
        self.toolbar.export.set_sensitive(has_book);
        for name in [EXPORT_PDF, EXPORT_EPUB, EXPORT_LATEX, PRINT] {
            if let Some(action) = self.actions.lookup_action(name).and_downcast::<gio::SimpleAction>() {
                action.set_enabled(has_book);
            }
        }
    }

    /// The conflict and save-error bar, rebuilt only when what it says changes.
    fn render_bar(&self) {
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
        *self.bar_state.borrow_mut() = wanted.clone();
        let session = Rc::downgrade(&self.session);
        let with_session = move |run: fn(&TextFileSession)| {
            let session = session.clone();
            move || {
                if let Some(session) = session.upgrade() {
                    run(&session);
                }
            }
        };
        match wanted {
            BarState::Hidden => self.bar.hide(),
            BarState::Conflict => self.bar.show_conflict(
                with_session(TextFileSession::resolve_conflict_reloading),
                with_session(TextFileSession::resolve_conflict_keeping_mine),
            ),
            // An article has no Save As (its file IS its place in the book, §8.5): Retry is the
            // way out, and the next keystroke's autosave tries again too.
            BarState::SaveError(error) => self.bar.show_save_error(
                &documents::could_not_save(&error),
                with_session(|session| {
                    session.flush_now(true);
                }),
                || {},
            ),
        }
    }

    fn fill_contents_menu(&self) {
        let menu = &self.toolbar.contents_menu;
        menu.remove_all();
        for (index, entry) in self.state.derived().outline.iter().enumerate() {
            let item = gio::MenuItem::new(Some(&command_table::contents_row_title(entry)), None);
            item.set_action_and_target_value(Some(&format!("book.{JUMP_HEADING}")), Some(&(index as u32).to_variant()));
            menu.append_item(&item);
        }
    }

    fn fill_notes_menu(&self) {
        let menu = &self.toolbar.notes_menu;
        menu.remove_all();
        for (index, note) in self.state.derived().notes.iter().enumerate() {
            let item = gio::MenuItem::new(Some(&note_preview::of(&note.text)), None);
            item.set_action_and_target_value(Some(&format!("book.{JUMP_NOTE}")), Some(&(index as u32).to_variant()));
            menu.append_item(&item);
        }
    }

    /// An alert on this window. While the window is closing the close waits for it, so a rescue
    /// report is read before the window goes.
    fn warn(&self, title: &str, message: &str) {
        let alert = self.alerts.warn(title.to_string(), message.to_string());
        if self.closing.get() {
            self.close_alerts.borrow_mut().push(alert);
        } else {
            glib::spawn_future_local(alert);
        }
    }

    // ───────────────────────────── closing ─────────────────────────────

    /// The close button, Ctrl+W, Close Book: wait for outputs in flight (≤ 10 s), let go of the
    /// article (flushed; a write that will not land is parked in a rescue copy and reported), then
    /// go. The book stays open — Show Book brings it back.
    pub fn request_close(&self) {
        if self.closing.replace(true) {
            return;
        }
        let Some(book) = self.me.upgrade() else { return };
        glib::spawn_future_local(async move {
            let in_flight = Rc::clone(&book.outputs_in_flight);
            let start = std::time::Instant::now();
            while in_flight.get() > 0 && start.elapsed() < OUTPUT_DRAIN_LIMIT {
                glib::timeout_future(Duration::from_millis(50)).await;
            }
            book.navigator.close_book();
            let alerts = std::mem::take(&mut *book.close_alerts.borrow_mut());
            for alert in alerts {
                alert.await;
            }
            book.cleanup();
            book.window.destroy();
        });
    }

    /// File ▸ Quit and logout (§1.5 routes 2 and 3): the article is written now, a failed write
    /// goes to a rescue copy, nothing is asked. The window answers `query-end` itself; Quit's
    /// window manager may call it before destroying the window (the destroy flushes too).
    pub fn quit_flush(&self) {
        self.session.terminate_flush();
    }

    /// The window is going (closed, or destroyed by Quit): undo every app-wide subscription, save
    /// what is unsaved, remember the size, leave the Window menu, and let go of the window — the
    /// last reference, so GTK can finally dispose of it.
    fn cleanup(&self) {
        if self.closed.replace(true) {
            return;
        }
        for undo in self.unsubscribe.take() {
            undo();
        }
        self.panes.editor().use_settings(None);
        self.derived.cancel();
        self.coordinator.hide();
        self.preview.close();
        self.session.terminate_flush();
        self.gate.take();
        self.session.dispose();
        let (width, height) = self.window.default_size();
        if width > 0 && height > 0 {
            self.settings.set_string(
                settings_keys::BOOK_WINDOW_SIZE,
                &window_placement::format_size(WindowSize::new(width, height)),
            );
        }
        self.shell.registry.remove(BOOK_WINDOW_ID);
        BOOK_WINDOW.with(|slot| {
            let mine = slot.borrow().as_ref().is_some_and(|w| std::ptr::eq(Rc::as_ptr(w), self));
            if mine {
                slot.borrow_mut().take();
            }
        });
    }
}

impl WindowHost for BookWindow {
    fn id(&self) -> WindowId {
        BOOK_WINDOW_ID
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
        let in_flight = Rc::clone(&self.outputs_in_flight);
        in_flight.set(in_flight.get() + 1);
        let spawner: &dyn Spawner = &*self.shell.spawner;
        spawner.spawn_local(Box::pin(async move {
            output.await;
            in_flight.set(in_flight.get() - 1);
        }));
    }

    /// The Mac's book pane publishes an active document with a nil file URL: File ▸ Print and
    /// Export act on the article being written, Rename / Move To / Revert are the sidebar's
    /// business (`is_saved` false), and the Go menu's stepper is this window's.
    fn snapshot(&self) -> ShellSnapshot {
        let editing = matches!(self.session.stage(), Stage::Editing(_));
        let stepper = self.navigator.stepper();
        let derived = if editing { self.state.derived() } else { DerivedText::empty() };
        let editor = self.panes.editor();
        let find_bar = self.panes.find_bar();
        let typing = TypingSettings::read(&*self.settings);
        let editor_visible = editing && SplitLayout::shows_editor(self.current_layout());
        ShellSnapshot {
            is_book_window: true,
            is_editing_article: editing,
            is_saved: false,
            is_dirty: self.session.is_dirty(),
            has_book: self.shell.book.has_book(),
            displayed_mode: self.state.effective_mode(),
            diagrams: derived.diagrams.iter().map(|d| DiagramRef::new(d.ordinal, d.menu_title.clone())).collect(),
            outline: derived.outline,
            notes: derived.notes,
            can_previous: stepper.can_previous(),
            can_next: stepper.can_next(),
            editor_visible,
            can_undo: editor_visible && editor.can_undo(),
            can_redo: editor_visible && editor.can_redo(),
            recent_entries: self.shell.recent.entries(),
            window_titles: self
                .shell
                .registry
                .windows()
                .into_iter()
                .map(|(id, title)| WindowTitle::new(id, title, id == BOOK_WINDOW_ID))
                .collect(),
            has_selection: editing && editor.has_selection(),
            has_find_query: find_bar.has_query(),
            pdf_page_size_id: PageSize::named(self.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref())
                .id
                .to_string(),
            sidebar_open: self.sidebar_open(),
            is_full_screen: self.window.is_fullscreen(),
            dialog_open: self.dialog_open.get(),
            find_bar_open: find_bar.is_open(),
            continue_lists: typing.continue_lists,
            capitalize_sentences: typing.capitalize_sentences,
            wrap_selection: wrap_selection_setting::read(&*self.settings),
            ..ShellSnapshot::empty()
        }
    }

    fn publish(&self) {
        // Never after cleanup: the editor raises selection changes while a closed window's tree is
        // torn down (md.win's 2026-09-27 crash).
        if !self.closed.get() {
            self.menu_bar.refresh(&self.snapshot());
        }
    }

    fn is_dark(&self) -> bool {
        theme::is_dark()
    }
}

/// md-logic's `OffThread` over GLib's blocking pool (shell-design §5.5): the derived text's word
/// count and outline are computed there and published back on the main context.
///
/// REQUEST (P1): one shared instance beside the Shell's other GLib seams; the document window
/// needs the same. This private one is removed when that lands.
struct GioBlocking;

impl OffThread for GioBlocking {
    fn spawn_blocking(&self, work: OffThreadWork) -> LocalBoxFuture<'static, Option<OffThreadValue>> {
        let handle = gio::spawn_blocking(work);
        Box::pin(async move { handle.await.ok() })
    }
}

/// The preview's document URL, which the coordinator navigates to.
///
/// MERGE (P4): `PreviewHost` owns its `preview-<n>` host; the frozen interface does not expose
/// it yet (requested: `PreviewHost::index_url()`). At P5's merge this body becomes that call.
/// Until then the S0 stub surface ignores every navigation, so any well-formed URL will do.
fn preview_index_url(_preview: &PreviewHost) -> String {
    asset_origin::index_url(&asset_origin::preview_host(0))
}

/// A folder as the header subtitle shows it: the home folder written `~` (§1.4).
pub fn display_folder(path: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    match path.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// What the self-test reads and drives.
#[cfg(feature = "selftest")]
impl BookWindow {
    pub fn navigator(&self) -> Rc<BookNavigatorModel> {
        Rc::clone(&self.navigator)
    }

    pub fn session(&self) -> Rc<TextFileSession> {
        Rc::clone(&self.session)
    }

    pub fn sidebar(&self) -> Rc<BookSidebar> {
        Rc::clone(&self.sidebar)
    }

    pub fn dispatcher(&self) -> Rc<CommandDispatcher> {
        Rc::clone(&self.dispatcher)
    }

    pub fn gtk_window(&self) -> adw::ApplicationWindow {
        self.window.clone()
    }

    pub fn paned(&self) -> gtk::Paned {
        self.paned.clone()
    }

    /// The content page ("book" / "empty") and the detail page ("editing" / "status").
    pub fn pages(&self) -> (String, String) {
        let name = |stack: &gtk::Stack| stack.visible_child_name().map(|n| n.to_string()).unwrap_or_default();
        (name(&self.content), name(&self.detail))
    }

    /// The status page's title and whether its button shows (the detail's, or No Book Open's).
    pub fn status(&self) -> (String, String, bool) {
        let view = if self.pages().0 == PAGE_EMPTY { &self.empty_view } else { &self.stage_view };
        (view.title.text().to_string(), view.message.text().to_string(), view.button.is_visible())
    }

    /// The status page's button (Open Book…, Show Window).
    pub fn status_button(&self) -> gtk::Button {
        if self.pages().0 == PAGE_EMPTY { self.empty_view.button.clone() } else { self.stage_view.button.clone() }
    }

    /// The header's Previous / Next chevrons.
    pub fn chevrons(&self) -> (gtk::Button, gtk::Button) {
        (self.toolbar.previous.clone(), self.toolbar.next.clone())
    }

    /// The view-mode toggle that is down.
    pub fn toggled_mode(&self) -> Option<ViewMode> {
        self.toolbar.modes.iter().find(|(button, _)| button.is_active()).map(|(_, mode)| *mode)
    }

    pub fn mode_button(&self, mode: ViewMode) -> Option<gtk::ToggleButton> {
        self.toolbar.modes.iter().find(|(_, m)| *m == mode).map(|(button, _)| button.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_is_written_as_a_tilde() {
        assert_eq!(display_folder("/home/ann/Books/Novel", "/home/ann"), "~/Books/Novel");
        assert_eq!(display_folder("/home/ann", "/home/ann/"), "~");
        assert_eq!(display_folder("/home/anna/Book", "/home/ann"), "/home/anna/Book");
        assert_eq!(display_folder("/srv/Книга", "/home/ann"), "/srv/Книга");
        assert_eq!(display_folder("/srv/Book", ""), "/srv/Book");
    }
}
