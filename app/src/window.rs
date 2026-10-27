//! A document window: libadwaita window, header bar, classic menu bar, and a `GtkPaned` with
//! the `GtkTextView` editor beside the `WebKitWebView` preview.

use crate::assets::{AssetHost, SchemeSecurity};
use crate::preview::Preview;
use crate::{menu, standin};
use adw::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;
use webkit::prelude::*;

/// What the preview is showing.
#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    /// The editor text through the stand-in renderer.
    Text,
    /// golden/document.html (math, Mermaid, PlantUML, code).
    Golden,
    /// golden 07-Diagrams body (Mermaid, Graphviz, PlantUML).
    Diagrams,
    /// Arbitrary HTML (font specimen, link probe).
    Html(String),
}

pub struct DocWindow {
    pub window: adw::ApplicationWindow,
    pub editor: gtk::TextView,
    pub editor_scroll: gtk::ScrolledWindow,
    pub overlay: gtk::Overlay,
    pub preview: Rc<Preview>,
    pub context: webkit::WebContext,
    pub host: Rc<AssetHost>,
    pub page: RefCell<Page>,
    pub dark: Cell<bool>,
    suppress_render: Cell<bool>,
    pub last_edit: Cell<Option<Instant>>,
    /// Every `win.view-mode` activation (mode, when) — the accelerator double-fire check.
    pub mode_log: RefCell<Vec<(String, Instant)>>,
    /// Paths that arrived through `GApplication::open`.
    pub opened: RefCell<Vec<String>>,
    /// Outcomes of the file dialogs (`Ok(path)` / `Err(message)`).
    pub dialog_results: RefCell<Vec<String>>,
}

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<DocWindow>>> = const { RefCell::new(Vec::new()) };
    static SHARED: RefCell<Option<(webkit::WebContext, Rc<AssetHost>)>> = const { RefCell::new(None) };
}

/// The one web context and asset host every document window shares.
pub fn shared_context() -> (webkit::WebContext, Rc<AssetHost>) {
    SHARED.with(|shared| {
        shared
            .borrow_mut()
            .get_or_insert_with(|| {
                let ctx = webkit::WebContext::default().unwrap_or_default();
                let host = AssetHost::new(AssetHost::default_rich_dir());
                host.attach(&ctx, SchemeSecurity::SHELL);
                (ctx, host)
            })
            .clone()
    })
}

pub fn first_window() -> Option<Rc<DocWindow>> {
    WINDOWS.with(|w| w.borrow().first().cloned())
}

const EDITOR_CSS: &str = "
textview.md-editor { font-family: \"Nimbus Mono PS\", \"Courier Prime\", \"Liberation Mono\", monospace; font-size: 15pt; }
textview.md-editor text { background-color: #F4EFE2; color: #2B2620; }
textview.md-editor.dark text { background-color: #241E18; color: #E7DBC2; }
";

fn install_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(EDITOR_CSS);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}

fn paper(dark: bool) -> gtk::gdk::RGBA {
    if dark {
        gtk::gdk::RGBA::new(0x24 as f32 / 255.0, 0x1E as f32 / 255.0, 0x18 as f32 / 255.0, 1.0)
    } else {
        gtk::gdk::RGBA::new(0xF4 as f32 / 255.0, 0xEF as f32 / 255.0, 0xE2 as f32 / 255.0, 1.0)
    }
}

static PREVIEW_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

impl DocWindow {
    pub fn open(app: &adw::Application) -> Rc<DocWindow> {
        if WINDOWS.with(|w| w.borrow().is_empty()) {
            install_css();
        }
        let (context, host) = shared_context();
        let key = format!("preview-{}", PREVIEW_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        let preview = Preview::new(&context, Rc::clone(&host), &key, true);

        let editor = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .left_margin(24)
            .right_margin(24)
            .top_margin(24)
            .bottom_margin(24)
            .monospace(false)
            .build();
        editor.add_css_class("md-editor");
        let editor_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&editor)
            .hexpand(true)
            .vexpand(true)
            .build();

        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Horizontal)
            .start_child(&editor_scroll)
            .end_child(&preview.view)
            .resize_start_child(true)
            .resize_end_child(true)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .position(560)
            .build();

        let overlay = gtk::Overlay::builder().child(&paned).build();

        let header = adw::HeaderBar::builder().title_widget(&adw::WindowTitle::new("md spike", "test.md")).build();
        let menubar = gtk::PopoverMenuBar::from_model(Some(&menu::model()));
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.add_top_bar(&menubar);
        toolbar.set_content(Some(&overlay));

        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("md spike")
            .default_width(1280)
            .default_height(820)
            .content(&toolbar)
            .build();

        let dark = adw::StyleManager::default().is_dark();
        preview.view.set_background_color(&paper(dark));

        let win = Rc::new(DocWindow {
            window,
            editor,
            editor_scroll,
            overlay,
            preview,
            context,
            host,
            page: RefCell::new(Page::Text),
            dark: Cell::new(dark),
            suppress_render: Cell::new(false),
            last_edit: Cell::new(None),
            mode_log: RefCell::new(Vec::new()),
            opened: RefCell::new(Vec::new()),
            dialog_results: RefCell::new(Vec::new()),
        });
        win.install_actions();
        win.connect_signals();
        win.set_text(&standin::test_markdown());
        win.window.present();
        WINDOWS.with(|w| w.borrow_mut().push(Rc::clone(&win)));
        win
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.editor.buffer().connect_changed(move |_| {
            let Some(win) = weak.upgrade() else { return };
            if win.suppress_render.get() {
                return;
            }
            win.last_edit.set(Some(Instant::now()));
            *win.page.borrow_mut() = Page::Text;
            win.preview.update(standin::render(&win.text(), win.dark.get()), false);
        });

        let weak = Rc::downgrade(self);
        adw::StyleManager::default().connect_dark_notify(move |sm| {
            let Some(win) = weak.upgrade() else { return };
            win.set_dark(sm.is_dark());
        });
    }

    pub fn text(&self) -> String {
        let buffer = self.editor.buffer();
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), false).to_string()
    }

    /// Replaces the editor text without a debounced render, then renders it as a new document.
    pub fn set_text(&self, text: &str) {
        self.suppress_render.set(true);
        self.editor.buffer().set_text(text);
        self.suppress_render.set(false);
        self.show(Page::Text);
    }

    /// Shows `page` as a new document (immediate load, scroll reset).
    pub fn show(&self, page: Page) {
        let dark = self.dark.get();
        let html = match &page {
            Page::Text => standin::render(&self.text(), dark),
            Page::Golden => standin::golden_document(dark),
            Page::Diagrams => standin::golden_diagrams(dark),
            Page::Html(html) => html.clone(),
        };
        *self.page.borrow_mut() = page;
        let preview = Rc::clone(&self.preview);
        preview.update(html, true);
    }

    fn set_dark(&self, dark: bool) {
        if self.dark.replace(dark) == dark {
            return;
        }
        if dark {
            self.editor.add_css_class("dark");
        } else {
            self.editor.remove_css_class("dark");
        }
        self.preview.view.set_background_color(&paper(dark));
        let page = self.page.borrow().clone();
        self.show(page);
    }

    pub fn mode(&self) -> String {
        self.window
            .lookup_action("view-mode")
            .and_then(|a| a.state())
            .and_then(|s| s.get::<String>())
            .unwrap_or_default()
    }

    fn apply_mode(&self, mode: &str) {
        let (editor, preview) = match mode {
            "edit" => (true, false),
            "preview" => (false, true),
            _ => (true, true),
        };
        self.editor_scroll.set_visible(editor);
        self.preview.view.set_visible(preview);
    }

    fn install_actions(self: &Rc<Self>) {
        let mode = gio::SimpleAction::new_stateful("view-mode", Some(glib::VariantTy::STRING), &"split".to_variant());
        let weak = Rc::downgrade(self);
        mode.connect_activate(move |action, target| {
            let Some(value) = target.and_then(|t| t.get::<String>()) else { return };
            let Some(win) = weak.upgrade() else { return };
            win.mode_log.borrow_mut().push((value.clone(), Instant::now()));
            action.set_state(&value.to_variant());
            win.apply_mode(&value);
        });
        self.window.add_action(&mode);

        let open = gio::SimpleAction::new("open", None);
        let weak = Rc::downgrade(self);
        open.connect_activate(move |_, _| {
            let Some(win) = weak.upgrade() else { return };
            glib::spawn_future_local(async move { win.run_open_dialog().await });
        });
        self.window.add_action(&open);

        let save_as = gio::SimpleAction::new("save-as", None);
        let weak = Rc::downgrade(self);
        save_as.connect_activate(move |_, _| {
            let Some(win) = weak.upgrade() else { return };
            glib::spawn_future_local(async move { win.run_save_dialog().await });
        });
        self.window.add_action(&save_as);

        let close = gio::SimpleAction::new("close", None);
        let weak = Rc::downgrade(self);
        close.connect_activate(move |_, _| {
            if let Some(win) = weak.upgrade() {
                win.window.close();
            }
        });
        self.window.add_action(&close);

        let minimize = gio::SimpleAction::new("minimize", None);
        let weak = Rc::downgrade(self);
        minimize.connect_activate(move |_, _| {
            if let Some(win) = weak.upgrade() {
                win.window.minimize();
            }
        });
        self.window.add_action(&minimize);

        for name in [
            "save",
            "export-pdf",
            "print",
            "undo",
            "redo",
            "cut",
            "copy",
            "paste",
            "find",
            "bold",
            "italic",
            "code",
            "contents",
            "notes",
        ] {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate(|a, _| eprintln!("md: win.{} (not in the spike)", a.name()));
            self.window.add_action(&action);
        }
    }

    /// File ▸ Open…: `GtkFileDialog::open_text_file` (GTK 4.18+) returns the file *and* the
    /// encoding the user picked in the dialog's encoding choice.
    pub async fn run_open_dialog(&self) {
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        let markdown = gtk::FileFilter::new();
        markdown.set_name(Some("Markdown"));
        markdown.add_suffix("md");
        markdown.add_suffix("markdown");
        markdown.add_mime_type("text/markdown");
        filters.append(&markdown);
        let dialog = gtk::FileDialog::builder().title("Open").modal(true).filters(&filters).build();
        let result = match dialog.open_text_file_future(Some(&self.window)).await {
            Ok((file, encoding)) => format!("open: Ok({:?}, encoding {encoding})", file.path()),
            Err(err) => format!("open: Err({:?}: {})", err.kind::<gtk::DialogError>(), err.message()),
        };
        self.dialog_results.borrow_mut().push(result);
    }

    pub async fn run_save_dialog(&self) {
        let dialog = gtk::FileDialog::builder().title("Save As").modal(true).initial_name("Untitled.md").build();
        let result = match dialog.save_future(Some(&self.window)).await {
            Ok(file) => format!("save: Ok({:?})", file.path()),
            Err(err) => format!("save: Err({:?}: {})", err.kind::<gtk::DialogError>(), err.message()),
        };
        self.dialog_results.borrow_mut().push(result);
    }
}

/// `GApplication::open` — a file from this launch's command line or from a second launch.
pub fn open_file(app: &adw::Application, file: &gio::File) {
    let win = first_window().unwrap_or_else(|| DocWindow::open(app));
    let path = file.path().map(|p| p.display().to_string()).unwrap_or_else(|| file.uri().to_string());
    win.opened.borrow_mut().push(path);
    match file.load_contents(gio::Cancellable::NONE) {
        Ok((bytes, _etag)) => win.set_text(&String::from_utf8_lossy(&bytes)),
        Err(err) => eprintln!("md: could not read {}: {err}", file.uri()),
    }
    win.window.present();
}
