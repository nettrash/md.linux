//! `WindowExports` — one window's exports and print (shell-design §7): md-logic's `ExportPipeline`
//! over this window's overlay renderers, pickers and alerts, the file system and identity of the
//! shell, the bundled `rich/` assets and the libsoup cover fetch. Every flow's future is handed to
//! the window's `track_output`, so a closing window waits for it; the footer ring follows
//! [`WindowExports::busy_changed`] (md-logic's `BusyRing`: shown only once an export has run
//! 500 ms); and [`WindowExports::book_outputs`] is the sink md-logic's `BookOutput` renders a book
//! into (md-logic's `PipelineBookOutputs` over the same pipeline).
//!
//! The window is reached through a `Weak` and only when a flow runs, never at construction, so the
//! window can build its exports inside its own `Rc::new_cyclic`. A window that has gone answers the
//! pipeline like a cancelled dialog: silence.
//!
//! Print hands md-logic's paper HTML to a Paper renderer and runs GTK's print dialog over it
//! (`pdf::run_print_dialog`); the renderer stays in the overlay until the dialog — and any job it
//! started — is over, then is closed. Failures are swallowed, as on the Mac.
// P4 → P1 / P5: until the windows construct exports, only the self-test reaches this; removed at the
// integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::rc::{Rc, Weak};

use md_core::document::page_size::PageSize;
use md_logic::books::{BookOutputs, PipelineBookOutputs};
use md_logic::event::Event;
use md_logic::exec::{CancelToken, LocalBoxFuture, ready};
use md_logic::export::{BusyRing, ExportPipeline};
use md_logic::seams::{
    Alerts, CloseChoice, FileIdentity, FileSystem, FileTypeChoice, Pickers, RenderError, RenderKind, RenderSurface,
    RenderSurfaceFactory, Scheduler, UiThread,
};
use md_logic::settings::{SettingsStore, settings_keys};

use crate::app::Shell;
use crate::export::cover_fetch::SoupCoverFetcher;
use crate::export::pdf::{self, PrintOutcome};
use crate::export::renderer::{self, OverlayRendererFactory};
use crate::web::context::WebServices;
use crate::windows::host::WindowHost;

type ShowPrint = Rc<dyn Fn(String) -> LocalBoxFuture<'static, Result<(), RenderError>>>;

/// The exports of one window.
pub struct WindowExports {
    host: Weak<dyn WindowHost>,
    settings: Rc<dyn SettingsStore>,
    pipeline: Rc<ExportPipeline>,
    show_print: ShowPrint,
    ring: BusyRing,
    busy_changed: Event<bool>,
    #[cfg(feature = "selftest")]
    print_outcomes: Rc<std::cell::RefCell<Vec<PrintOutcome>>>,
}

impl WindowExports {
    pub fn new(shell: &Rc<Shell>, host: Weak<dyn WindowHost>) -> Rc<WindowExports> {
        Self::build(shell, host, shell.files.clone(), shell.identity.clone())
    }

    /// The self-test's exports: the pipeline writes through `files` and resolves links through
    /// `identity` (md-logic's `LocalFileSystem` / `LocalFileIdentity`) instead of the shell's.
    #[cfg(feature = "selftest")]
    pub fn with_files(
        shell: &Rc<Shell>,
        host: Weak<dyn WindowHost>,
        files: Rc<dyn FileSystem>,
        identity: Rc<dyn FileIdentity>,
    ) -> Rc<WindowExports> {
        Self::build(shell, host, files, identity)
    }

    fn build(
        shell: &Rc<Shell>,
        host: Weak<dyn WindowHost>,
        files: Rc<dyn FileSystem>,
        identity: Rc<dyn FileIdentity>,
    ) -> Rc<WindowExports> {
        let web = Rc::clone(&shell.web);
        let scheduler: Rc<dyn Scheduler> = shell.scheduler.clone();
        let ui: Rc<dyn UiThread> = shell.ui.clone();
        let rich = Rc::clone(&web);
        let pipeline = Rc::new(ExportPipeline::new(
            Rc::new(HostRenderers { host: host.clone(), web: Rc::clone(&web) }),
            Rc::new(HostPickers(host.clone())),
            Rc::new(HostAlerts(host.clone())),
            files,
            move |key: &str| rich.read_rich_asset(key),
            Rc::clone(&scheduler),
            identity,
            Rc::new(SoupCoverFetcher::new()),
        ));
        #[cfg(feature = "selftest")]
        let print_outcomes = Rc::new(std::cell::RefCell::new(Vec::new()));
        let show_print = show_print(
            host.clone(),
            web,
            #[cfg(feature = "selftest")]
            Rc::clone(&print_outcomes),
        );
        let settings: Rc<dyn SettingsStore> = shell.settings.clone();
        Rc::new_cyclic(|me: &Weak<WindowExports>| {
            let shown = me.clone();
            let ring = BusyRing::new(scheduler, ui, move |busy| {
                if let Some(this) = shown.upgrade() {
                    this.busy_changed.emit(&busy);
                }
            });
            let busy = me.clone();
            pipeline.busy_changed().connect(move |&running| {
                if let Some(this) = busy.upgrade() {
                    this.ring.busy_changed(running);
                }
            });
            WindowExports {
                host,
                settings,
                pipeline,
                show_print,
                ring,
                busy_changed: Event::new(),
                #[cfg(feature = "selftest")]
                print_outcomes,
            }
        })
    }

    /// File ▸ Print…: GTK's print dialog over the paper HTML (A4, 0.5 in, preset).
    pub fn print(&self, source: String, title: String) {
        let show_print = Rc::clone(&self.show_print);
        self.track(self.pipeline.print(&source, &title, move |html| show_print(html)));
    }

    /// File ▸ Export ▸ PDF… at `md.pdfPageSize` (an unknown id is A4).
    pub fn export_pdf(&self, source: String, title: String) {
        let size = PageSize::named(self.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref());
        self.track(self.pipeline.export_pdf(&source, &title, size));
    }

    pub fn export_html(&self, source: String, title: String) {
        self.track(self.pipeline.export_html(&source, &title));
    }

    /// `file_name` titles the book when the front matter does not; `document_path` lets a `cover:`
    /// beside the document be read.
    pub fn export_epub(&self, source: String, file_name: String, document_path: Option<String>) {
        self.track(self.pipeline.export_epub(&source, &file_name, document_path.as_deref()));
    }

    pub fn export_latex(&self, source: String, title: String) {
        self.track(self.pipeline.export_latex(&source, &title));
    }

    /// Images are copied only from beside a saved document (`document_path`).
    pub fn export_text_bundle(&self, source: String, document_path: Option<String>, title: String) {
        self.track(self.pipeline.export_text_bundle(&source, document_path.as_deref(), &title));
    }

    /// The diagram's `ordinal`; the pipeline re-resolves it from the current text and does nothing
    /// when that fence is gone.
    pub fn export_diagram_svg(&self, source: String, title: String, ordinal: u32) {
        self.track(self.pipeline.export_diagram_svg_at(&source, &title, ordinal as usize));
    }

    /// md-logic `BookOutput`'s sink, rendering in this window.
    pub fn book_outputs(&self) -> Rc<dyn BookOutputs> {
        let show_print = Rc::clone(&self.show_print);
        Rc::new(PipelineBookOutputs::new(Rc::clone(&self.pipeline), move |html| show_print(html)))
    }

    /// Args: whether the footer's ring shows — true once an export has run 500 ms, false when it
    /// ends (md-logic's `BusyRing`; Print raises neither).
    pub fn busy_changed(&self) -> &Event<bool> {
        &self.busy_changed
    }

    /// The window is closing: whatever renders stops quietly and the ring is forgotten.
    pub fn cancel(&self) {
        self.pipeline.cancel();
        self.ring.cancel();
    }

    /// How each Print… ended, oldest first.
    #[cfg(feature = "selftest")]
    pub fn print_outcomes(&self) -> Vec<PrintOutcome> {
        self.print_outcomes.borrow().clone()
    }

    fn track(&self, output: LocalBoxFuture<'static, ()>) {
        match self.host.upgrade() {
            Some(host) => host.track_output(output),
            None => {
                glib::spawn_future_local(output);
            }
        }
    }
}

/// Print's `show_print`: a Paper renderer in the window's overlay, GTK's dialog over it, closed once
/// the dialog and its job are over.
fn show_print(
    host: Weak<dyn WindowHost>,
    web: Rc<WebServices>,
    #[cfg(feature = "selftest")] outcomes: Rc<std::cell::RefCell<Vec<PrintOutcome>>>,
) -> ShowPrint {
    Rc::new(move |html: String| -> LocalBoxFuture<'static, Result<(), RenderError>> {
        let Some(host) = host.upgrade() else { return ready(Err(RenderError::Cancelled)) };
        let (overlay, window) = (host.overlay(), host.window());
        let created = renderer::create_in(&overlay, &web, RenderKind::Paper);
        #[cfg(feature = "selftest")]
        let outcomes = Rc::clone(&outcomes);
        Box::pin(async move {
            let renderer = created.await;
            let outcome = match renderer.load(html, CancelToken::new()).await {
                Ok(()) => pdf::run_print_dialog(renderer.view(), &window).await,
                Err(error) => PrintOutcome::Failed(error.to_string()),
            };
            renderer.close().await;
            #[cfg(feature = "selftest")]
            outcomes.borrow_mut().push(outcome.clone());
            match outcome {
                PrintOutcome::Failed(message) => {
                    eprintln!("md: Print: {message}");
                    Err(RenderError::Failed(message))
                }
                PrintOutcome::Cancelled | PrintOutcome::Printed => Ok(()),
            }
        })
    })
}

/// The window's overlay, looked up when a renderer is wanted.
struct HostRenderers {
    host: Weak<dyn WindowHost>,
    web: Rc<WebServices>,
}

impl RenderSurfaceFactory for HostRenderers {
    fn create(&self, kind: RenderKind) -> LocalBoxFuture<'static, Result<Box<dyn RenderSurface>, RenderError>> {
        match self.host.upgrade() {
            Some(host) => OverlayRendererFactory::new(&host.overlay(), &self.web).create(kind),
            None => ready(Err(RenderError::Cancelled)),
        }
    }
}

/// The window's file dialogs, looked up when one is wanted; a window that has gone cancels.
struct HostPickers(Weak<dyn WindowHost>);

impl Pickers for HostPickers {
    fn open_files(&self, extensions: Vec<String>) -> LocalBoxFuture<'static, Vec<String>> {
        match self.0.upgrade() {
            Some(host) => host.pickers().open_files(extensions),
            None => ready(Vec::new()),
        }
    }

    fn save_file(
        &self,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        default_extension: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        match self.0.upgrade() {
            Some(host) => host.pickers().save_file(suggested_name, choices, default_extension),
            None => ready(None),
        }
    }

    fn pick_folder(&self, title: Option<String>) -> LocalBoxFuture<'static, Option<String>> {
        match self.0.upgrade() {
            Some(host) => host.pickers().pick_folder(title),
            None => ready(None),
        }
    }
}

/// The window's alerts, looked up when one is wanted; a window that has gone says nothing.
struct HostAlerts(Weak<dyn WindowHost>);

impl Alerts for HostAlerts {
    fn warn(&self, title: String, message: String) -> LocalBoxFuture<'static, ()> {
        match self.0.upgrade() {
            Some(host) => host.alerts().warn(title, message),
            None => ready(()),
        }
    }

    fn prompt_name(
        &self,
        title: String,
        message: String,
        initial: String,
        accept_label: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        match self.0.upgrade() {
            Some(host) => host.alerts().prompt_name(title, message, initial, accept_label),
            None => ready(None),
        }
    }

    fn confirm_delete(&self, title: String, message: String) -> LocalBoxFuture<'static, bool> {
        match self.0.upgrade() {
            Some(host) => host.alerts().confirm_delete(title, message),
            None => ready(false),
        }
    }

    fn ask_save_changes(&self, title: String) -> LocalBoxFuture<'static, CloseChoice> {
        match self.0.upgrade() {
            Some(host) => host.alerts().ask_save_changes(title),
            None => ready(CloseChoice::Cancel),
        }
    }

    fn confirm_replace(&self, message: String) -> LocalBoxFuture<'static, bool> {
        match self.0.upgrade() {
            Some(host) => host.alerts().confirm_replace(message),
            None => ready(false),
        }
    }
}
