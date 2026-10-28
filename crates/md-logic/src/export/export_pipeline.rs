//! Port of md.win `Export/ExportPipeline.cs`: every export and print flow of §7, end to end and in
//! one place, driving the browser only through [`RenderSurface`](crate::seams::RenderSurface) and the machine only through the
//! seams — so the order of operations, the HTML each flow loads, the suggested names and every
//! alert are testable with no WebKit anywhere in sight. The GTK app supplies the adapters and
//! nothing else.
//!
//! **The order matters and differs per flow, deliberately.** PDF renders first and opens the save
//! dialog afterwards (the Mac's order: the pages exist before the user is asked where to put them).
//! The document EPUB works out its title first, opens the dialog *before* building, and only then
//! photographs — because photographing a book of diagrams takes a while and the user may cancel.
//! LaTeX and TextBundle never open a renderer at all. These are pinned by tests, not by comments.
//!
//! **Which HTML each flow loads is the typography decision (§4.3).** Print and PDF load
//! [`RenderKind::Paper`] — md-core's export HTML plus the app's typewriter style
//! ([`screen_html::with_linux_fonts`]), because paper a reader holds should not be Liberation Mono.
//! HTML, EPUB and SVG load [`RenderKind::Export`], the pure md-core bytes, because those files are
//! pinned byte for byte against the other ports and nothing Linux-specific may reach them.
//!
//! Exports are serialised per pipeline (one per window): two at once would share the window's
//! export renderer and its alerts. A cancelled dialog is silence, never an alert; every real
//! failure is one [`Alerts::warn`] whose title is fixed by §7.9 and whose body is the error's own
//! message ([`ExportError::message`]).
//!
//! LINUX DECISIONS (each also noted where it applies):
//! - No Share ▸ Rendered PDF… and no Share ▸ Source…: Linux has no system share sheet, so md.win's
//!   `SharePdfAsync` / `ShareSourceAsync` and their temporary folder are not ported. Export ▸ PDF…
//!   writes the same file.
//! - GTK's save dialog does not create the file it returns (WinRT's does, empty). Rendering before
//!   asking is still the order, for the Mac's reason; a failed write leaves whatever the OS left.
//! - The PDF is WebKitGTK's `PrintOperation` to GTK's "Print to File" printer
//!   (docs/linux-facts.md §5); the surface reads the file back and returns the bytes, and the
//!   pipeline writes them over the chosen path. Zero bytes is [`ExportError::PdfPagination`].
//! - Print hands the paper HTML to the app's `show_print`, which loads it in a renderer and runs
//!   `webkit::PrintOperation::run_dialog` — GTK's own print dialog, a separate window that needs no
//!   visible web view, so there is no in-window overlay and no Done button (md.win's
//!   `PrintOverlay`). The future it returns completes when the dialog (and any printing it
//!   started) is over.
//! - The scheduler and the cover fetcher are required ([`export_delay`](super::export_delay),
//!   [`cover_reader`]).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::io;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use md_core::book::structured_book::StructuredBook;
use md_core::document::page_size::PageSize;
use md_core::export::epub_metadata::MAX_COVER_BYTES;
use md_core::export::{BundleWrapper, Diagram, EpubPlan};
use md_core::export::{diagram_svg, epub_export, html_export, latex_export, pdf_export, text_bundle};
use md_core::markdown::html;

use super::cover_reader::{self, CoverFetcher};
use super::export_error::ExportError;
use super::export_names;
use super::rich_snapshotter::RichSnapshotter;
use crate::documents::{asset_reader, file_names};
use crate::event::Event;
use crate::exec::{CancelToken, LocalBoxFuture, ready};
use crate::preview::{json_script, screen_html, scripts};
use crate::seams::{
    Alerts, FileIdentity, FileSystem, FileTypeChoice, Pickers, PrintGeometry, RenderError, RenderKind,
    RenderSurfaceFactory, Scheduler,
};
use crate::strings::exports;

/// How a TextBundle folder is written: md-core's [`BundleWrapper::write`] (staging folder, atomic
/// swap) unless a test replaces it.
pub type BundleWriter = Rc<dyn Fn(&BundleWrapper, &str) -> io::Result<()>>;

/// One window's exports. Cheap to hold; every flow returns a `'static` future the app spawns on
/// GLib's main context.
pub struct ExportPipeline {
    inner: Rc<Inner>,
}

struct Inner {
    renderers: Rc<dyn RenderSurfaceFactory>,
    pickers: Rc<dyn Pickers>,
    alerts: Rc<dyn Alerts>,
    file_system: Rc<dyn FileSystem>,
    read_rich_asset: Rc<dyn Fn(&str) -> Option<Vec<u8>>>,
    scheduler: Rc<dyn Scheduler>,
    identity: Rc<dyn FileIdentity>,
    cover_fetcher: Rc<dyn CoverFetcher>,
    write_bundle: RefCell<BundleWriter>,
    snapshotter: RichSnapshotter,
    gate: Rc<Gate>,
    cancellation: CancelToken,
    busy_changed: Event<bool>,
}

impl fmt::Debug for ExportPipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportPipeline")
            .field("cancelled", &self.inner.cancellation.is_cancelled())
            .field("busy", &self.inner.gate.is_held())
            .finish()
    }
}

impl ExportPipeline {
    /// How long an export must still be running before the footer shows its progress ring (§7.1).
    /// The number lives here, beside the event that starts the clock, rather than as a literal in
    /// the window that draws the ring. Most exports — LaTeX, TextBundle — finish inside it and show
    /// nothing at all, which is the point.
    pub const BUSY_RING_DELAY: Duration = Duration::from_millis(500);

    /// - `renderers`: one offscreen surface per export (a fresh `webkit6::WebView` in the app).
    /// - `pickers`: the save and folder dialogs; a `None` answer is a cancel.
    /// - `alerts`: §7.9's dialogs.
    /// - `file_system`: where the finished bytes go.
    /// - `read_rich_asset`: the bundled engine assets by slash-separated key (`rich/katex.min.css`,
    ///   `rich/fonts/<face>.woff2`); `None` when the file is not installed. Never panics.
    /// - `scheduler`: the UI-thread timer the render-complete poll, the repaint wait and the cover
    ///   deadline use.
    /// - `identity`: link resolution for the TextBundle's and the cover's containment.
    /// - `cover_fetcher`: one HTTP GET for an EPUB's `cover: https://…` ([`cover_reader`]).
    pub fn new(
        renderers: Rc<dyn RenderSurfaceFactory>,
        pickers: Rc<dyn Pickers>,
        alerts: Rc<dyn Alerts>,
        file_system: Rc<dyn FileSystem>,
        read_rich_asset: impl Fn(&str) -> Option<Vec<u8>> + 'static,
        scheduler: Rc<dyn Scheduler>,
        identity: Rc<dyn FileIdentity>,
        cover_fetcher: Rc<dyn CoverFetcher>,
    ) -> Self {
        let snapshotter = RichSnapshotter::new(renderers.clone(), scheduler.clone());
        let write_bundle: BundleWriter =
            Rc::new(|wrapper: &BundleWrapper, folder: &str| wrapper.write(Path::new(folder)));
        ExportPipeline {
            inner: Rc::new(Inner {
                renderers,
                pickers,
                alerts,
                file_system,
                read_rich_asset: Rc::new(read_rich_asset),
                scheduler,
                identity,
                cover_fetcher,
                write_bundle: RefCell::new(write_bundle),
                snapshotter,
                gate: Rc::new(Gate::default()),
                cancellation: CancelToken::new(),
                busy_changed: Event::new(),
            }),
        }
    }

    /// Replaces how a TextBundle folder is written — a test seam, so a test can watch it happen
    /// without touching a disk.
    pub fn with_bundle_writer(self, write: impl Fn(&BundleWrapper, &str) -> io::Result<()> + 'static) -> Self {
        *self.inner.write_bundle.borrow_mut() = Rc::new(write);
        self
    }

    /// Raised with `true` when an export takes the pipeline and `false` when it lets go (Print
    /// raises neither). The footer shows its ring [`ExportPipeline::BUSY_RING_DELAY`] after `true`
    /// ([`BusyRing`](super::BusyRing)).
    pub fn busy_changed(&self) -> &Event<bool> {
        &self.inner.busy_changed
    }

    /// The window is closing: whatever is rendering stops rather than talking to a dead window, and
    /// says nothing. The pipeline is finished after this — a window gets one.
    pub fn cancel(&self) {
        self.inner.cancellation.cancel();
    }

    /// The paper page: md-core's export HTML (light, `export: true`) with the app's typewriter
    /// style — screen and paper only, never a file (§4.3).
    pub fn paper_html(source: &str, title: &str) -> String {
        screen_html::with_linux_fonts(&export_document_html(source, title)).into_owned()
    }

    // ─────────────────────────────── Print (§7.2) ───────────────────────────────

    /// Print… — the paper HTML handed to `show_print` (see the module's Linux decisions). No
    /// page-size rewrite: Print is always A4 and paper is the printer's business, as on macOS and
    /// Android. Raises no busy signal — the print dialog is up for as long as it runs, and a footer
    /// ring behind it would say nothing. A failure shows nothing: there is nothing actionable to
    /// say, and the Mac swallows it too.
    pub fn print(
        &self,
        source: &str,
        title: &str,
        show_print: impl FnOnce(String) -> LocalBoxFuture<'static, Result<(), RenderError>> + 'static,
    ) -> LocalBoxFuture<'static, ()> {
        let (source, title) = (source.to_string(), title.to_string());
        serialised(self.inner.clone(), false, async move {
            // Swallowed, as on the Mac: the dialog is gone and there is no failed file to explain.
            let _ = show_print(Self::paper_html(&source, &title)).await;
        })
    }

    // ──────────────────────────────── PDF (§7.3) ────────────────────────────────

    /// Export ▸ PDF… (and Export Book ▸ PDF…) — render, then ask where to put it (the Mac's order).
    pub fn export_pdf(&self, source: &str, title: &str, size: &PageSize) -> LocalBoxFuture<'static, ()> {
        let (source, title, size) = (source.to_string(), title.to_string(), *size);
        self.guarded(exports::COULD_NOT_EXPORT_PDF, move |inner, cancel| async move {
            let bytes = inner.pdf_bytes(&source, &title, &size, &cancel).await?;
            let Some(picked) =
                inner.save(export_names::pdf(&title), export_names::pdf_choices(), export_names::PDF_EXTENSION).await
            else {
                return Ok(());
            };
            inner.file_system.write_all_bytes_in_place(&picked, &bytes)?;
            Ok(())
        })
    }

    // ─────────────────────────────── HTML (§7.4) ────────────────────────────────

    /// Export ▸ HTML… — one file that opens anywhere with no engines and no network: the rendered
    /// DOM with its scripts and stylesheet links stripped, KaTeX's CSS and faces inlined when the
    /// document has math, and the licence notices that inlining obliges.
    pub fn export_html(&self, source: &str, title: &str) -> LocalBoxFuture<'static, ()> {
        let (source, title) = (source.to_string(), title.to_string());
        self.guarded(exports::COULD_NOT_EXPORT_HTML, move |inner, cancel| async move {
            // export_document, not html::document: it swaps the page-break rule for the visible one,
            // and loading the wrong string loses the author's page breaks silently.
            let document = html_export::export_document(&source, &title);
            let answer = inner
                .load_and_eval(RenderKind::Export, document.clone(), html_export::CAPTURE_SCRIPT.to_string(), &cancel)
                .await?;
            let captured = json_script::string(&answer).unwrap_or_default();
            if captured.is_empty() {
                return Err(ExportError::Export(exports::PAGE_NOT_CAPTURED.to_string()));
            }

            // The same document as the math gate: the <link> to katex.min.css is already gone from
            // the capture, so asking the captured page would always answer "no maths".
            let page = html_export::prepare_page_for_document(&captured, &document, &*inner.read_rich_asset)
                .map_err(|empty| ExportError::Export(empty.to_string()))?;

            let Some(picked) = inner
                .save(export_names::html(&title), export_names::html_choices(), export_names::HTML_EXTENSION)
                .await
            else {
                return Ok(());
            };
            inner.write_text(&picked, &page)
        })
    }

    // ─────────────────────────────── EPUB (§7.5) ────────────────────────────────

    /// Export ▸ EPUB… for one document. The title is worked out first so the suggested file name
    /// matches it, the dialog opens before anything is photographed, and the plan is fixed before
    /// the browser is involved at all.
    ///
    /// `document_path`: the document's file, so a `cover:` naming an image beside it can be read
    /// (contained, as the TextBundle export reads images); `None` for an unsaved document, which
    /// then finds none.
    pub fn export_epub(
        &self,
        source: &str,
        file_name: &str,
        document_path: Option<&str>,
    ) -> LocalBoxFuture<'static, ()> {
        let (source, file_name) = (source.to_string(), file_name.to_string());
        let document_path = document_path.map(str::to_string);
        self.guarded(exports::COULD_NOT_EXPORT_EPUB, move |inner, cancel| async move {
            let title = epub_export::document_title(&source, &file_name);
            let Some(picked) = inner
                .save(export_names::epub(&title), export_names::epub_choices(), export_names::EPUB_EXTENSION)
                .await
            else {
                return Ok(());
            };
            inner
                .build_epub(epub_export::plan_document(&source, &title), &picked, &cancel, document_path.as_deref())
                .await
        })
    }

    /// Export Book ▸ EPUB… — the dialog first, then build: photographing a book of diagrams takes a
    /// while and the user may cancel. The caller ([`BookOutput`](crate::books::BookOutput)) has
    /// already flushed the article being edited.
    pub fn export_book_epub(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.guarded(exports::COULD_NOT_EXPORT_EPUB, move |inner, cancel| async move {
            let Some(picked) = inner
                .save(export_names::epub(&book.title), export_names::epub_choices(), export_names::EPUB_EXTENSION)
                .await
            else {
                return Ok(());
            };
            inner.build_epub(epub_export::plan_book(&book), &picked, &cancel, None).await
        })
    }

    // ────────────────────────── LaTeX and SVG (§7.6) ────────────────────────────

    /// Export ▸ LaTeX… — pure md-core, no browser: the source is transformed, not rendered.
    pub fn export_latex(&self, source: &str, title: &str) -> LocalBoxFuture<'static, ()> {
        let (source, title) = (source.to_string(), title.to_string());
        self.guarded(exports::COULD_NOT_EXPORT_LATEX, move |inner, _| async move {
            let document = latex_export::document(&source, &title);
            inner
                .save_text(
                    &document,
                    export_names::latex(&title),
                    export_names::latex_choices(),
                    export_names::LATEX_EXTENSION,
                )
                .await
        })
    }

    /// Export Book ▸ LaTeX… — the structured book, likewise with no browser.
    pub fn export_book_latex(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.guarded(exports::COULD_NOT_EXPORT_LATEX, move |inner, _| async move {
            let document = latex_export::book(&book);
            inner
                .save_text(
                    &document,
                    export_names::latex(&book.title),
                    export_names::latex_choices(),
                    export_names::LATEX_EXTENSION,
                )
                .await
        })
    }

    /// Export ▸ Diagram as SVG ▸ … from the menu's own argument, the ordinal: the row is resolved
    /// against the text as it is *now*. The menu was built from a 250 ms-old snapshot and the writer
    /// may have deleted that fence in the meantime, in which case the command quietly does nothing
    /// — no renderer, no dialog, no busy signal — rather than exporting the neighbour.
    pub fn export_diagram_svg_at(&self, source: &str, title: &str, ordinal: usize) -> LocalBoxFuture<'static, ()> {
        match diagram_svg::diagrams(source).into_iter().find(|candidate| candidate.ordinal == ordinal) {
            Some(diagram) => self.export_diagram_svg(source, title, &diagram),
            None => ready(()),
        }
    }

    /// Export ▸ Diagram as SVG ▸ … — one rendered diagram, lifted out of the DOM as it was drawn and
    /// made a standalone file. Light, always: an `.svg` has no theme, Mermaid bakes its colours in
    /// and a plot draws with `currentColor`.
    pub fn export_diagram_svg(&self, source: &str, title: &str, diagram: &Diagram) -> LocalBoxFuture<'static, ()> {
        let (source, title, ordinal) = (source.to_string(), title.to_string(), diagram.ordinal);
        self.guarded(exports::COULD_NOT_EXPORT_SVG, move |inner, cancel| async move {
            let answer = inner
                .load_and_eval(
                    RenderKind::Export,
                    export_document_html(&source, &title),
                    scripts::diagram_svg(ordinal),
                    &cancel,
                )
                .await?;
            // null means the engine threw or timed out and its source text is all that is there.
            // Writing an empty .svg would be worse than saying so.
            let svg = json_script::string(&answer).filter(|svg| !svg.is_empty());
            let Some(svg) = svg else {
                return Err(ExportError::Export(exports::DIAGRAM_NOT_CAPTURED.to_string()));
            };
            let page = diagram_svg::standalone_document(&svg);
            inner
                .save_text(
                    &page,
                    export_names::diagram_svg(&title, ordinal),
                    export_names::svg_choices(),
                    export_names::SVG_EXTENSION,
                )
                .await
        })
    }

    // ───────────────────────────── TextBundle (§7.7) ────────────────────────────

    /// Export ▸ TextBundle… — a `.textbundle` *folder*, which is what the product is on every other
    /// port. The folder dialog picks the parent, and the Replace question is asked here — for a
    /// folder *or* a plain file already holding the name.
    ///
    /// `document_path`: the saved document's path, or `None`. Images are copied only from the
    /// document's own folder, so an unsaved document exports with an empty `assets/` and every
    /// reference left as written.
    pub fn export_text_bundle(
        &self,
        source: &str,
        document_path: Option<&str>,
        title: &str,
    ) -> LocalBoxFuture<'static, ()> {
        let (source, title) = (source.to_string(), title.to_string());
        let document_path = document_path.map(str::to_string);
        self.guarded(exports::COULD_NOT_EXPORT_TEXT_BUNDLE, move |inner, _| async move {
            let Some(parent) = inner.pickers.pick_folder(Some(exports::CHOOSE_TEXT_BUNDLE_FOLDER.to_string())).await
            else {
                return Ok(());
            };
            let name = export_names::text_bundle_folder(&title);
            let target = file_names::combine(&parent, &name);

            if (inner.file_system.directory_exists(&target) || inner.file_system.file_exists(&target))
                && !inner.alerts.confirm_replace(exports::folder_exists(&name)).await
            {
                return Ok(());
            }

            let resolve =
                asset_reader::beside(document_path.as_deref(), inner.file_system.clone(), inner.identity.clone());
            let rewrite = text_bundle::export_rewriting(&source, |reference| resolve(reference));
            let wrapper = text_bundle::bundle_wrapper(&rewrite.text, rewrite.assets);
            let write = inner.write_bundle.borrow().clone();
            write(&wrapper, &target)?;
            Ok(())
        })
    }

    // ──────────────────────────────── plumbing ──────────────────────────────────

    fn guarded<F, Fut>(&self, alert_title: &'static str, body: F) -> LocalBoxFuture<'static, ()>
    where
        F: FnOnce(Rc<Inner>, CancelToken) -> Fut + 'static,
        Fut: Future<Output = Result<(), ExportError>> + 'static,
    {
        let inner = self.inner.clone();
        serialised(self.inner.clone(), true, async move {
            match body(inner.clone(), inner.cancellation.clone()).await {
                // The window closed under the export; there is no one left to tell.
                Ok(()) | Err(ExportError::Cancelled) => {}
                Err(error) => inner.alerts.warn(alert_title.to_string(), error.message()).await,
            }
        })
    }
}

/// md-core's export page: light, `export: true`, and pure — the base of every rendered flow.
fn export_document_html(source: &str, title: &str) -> String {
    html::document(source, title, false, true)
}

/// `body` behind the pipeline's gate, announcing busy around it when asked — md.win's `Serialised`.
fn serialised(
    inner: Rc<Inner>,
    announce_busy: bool,
    body: impl Future<Output = ()> + 'static,
) -> LocalBoxFuture<'static, ()> {
    Box::pin(async move {
        let gate = inner.gate.acquire().await;
        if announce_busy {
            inner.busy_changed.emit(&true);
        }
        // The `finally`: busy goes false (then the gate opens) however the body ends — dropped
        // mid-way included.
        let _held = Held { inner: inner.clone(), announce_busy, _gate: gate };
        body.await;
    })
}

struct Held {
    inner: Rc<Inner>,
    announce_busy: bool,
    _gate: GateGuard,
}

impl Drop for Held {
    fn drop(&mut self) {
        if self.announce_busy {
            self.inner.busy_changed.emit(&false);
        }
        // `_gate` is dropped after this: the gate opens once busy has gone false, as in md.win.
    }
}

impl Inner {
    async fn save(&self, suggested_name: String, choices: Vec<FileTypeChoice>, extension: &str) -> Option<String> {
        self.pickers.save_file(suggested_name, choices, extension.to_string()).await
    }

    async fn save_text(
        &self,
        text: &str,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        extension: &str,
    ) -> Result<(), ExportError> {
        let Some(picked) = self.save(suggested_name, choices, extension).await else { return Ok(()) };
        self.write_text(&picked, text)
    }

    /// UTF-8 without a BOM, everywhere — what every other port writes, and what a browser, a TeX
    /// distribution and an SVG parser all expect. (A Rust string has no BOM unless it holds one.)
    fn write_text(&self, path: &str, text: &str) -> Result<(), ExportError> {
        self.file_system.write_all_bytes_in_place(path, text.as_bytes())?;
        Ok(())
    }

    /// Load `html` on a fresh surface of `kind`, evaluate `script`, close the surface (always), and
    /// hand back the raw JSON answer.
    async fn load_and_eval(
        &self,
        kind: RenderKind,
        html: String,
        script: String,
        cancel: &CancelToken,
    ) -> Result<String, ExportError> {
        let surface = self.renderers.create(kind).await?;
        let result = async {
            surface.load(html, cancel.clone()).await?;
            Ok::<_, ExportError>(surface.eval(script).await?)
        }
        .await;
        surface.close().await;
        result
    }

    async fn pdf_bytes(
        &self,
        source: &str,
        title: &str,
        size: &PageSize,
        cancel: &CancelToken,
    ) -> Result<Vec<u8>, ExportError> {
        // styled_for_export rewrites one declaration — the body padding — and nothing else; the
        // paper geometry itself lives in the print settings, never in the document.
        let document = export_document_html(source, title);
        let styled = pdf_export::styled_for_export(&document, size);
        let paper = screen_html::with_linux_fonts(&styled).into_owned();

        let surface = self.renderers.create(RenderKind::Paper).await?;
        let result = async {
            surface.load(paper, cancel.clone()).await?;
            Ok::<_, ExportError>(surface.pdf(PrintGeometry::for_page(size)).await?)
        }
        .await;
        surface.close().await;
        let bytes = result?;
        if bytes.is_empty() {
            return Err(ExportError::PdfPagination);
        }
        Ok(bytes)
    }

    async fn build_epub(
        &self,
        plan: EpubPlan,
        path: &str,
        cancel: &CancelToken,
        document_path: Option<&str>,
    ) -> Result<(), ExportError> {
        let snapshots = self.snapshotter.capture(&plan, cancel).await?;
        let beside = asset_reader::beside_capped(
            document_path,
            self.file_system.clone(),
            self.identity.clone(),
            MAX_COVER_BYTES as u64,
        );
        let cover =
            cover_reader::read(plan.cover.as_ref(), &*beside, &*self.cover_fetcher, &*self.scheduler, cancel).await?;
        let bytes = epub_export::assemble(&plan, Some(&snapshots), None, None, cover.as_ref())?;
        self.file_system.write_all_bytes_in_place(path, &bytes)?;
        Ok(())
    }
}

// ─────────────────────────────── the gate ───────────────────────────────────

/// md.win's `SemaphoreSlim(1, 1)` on one thread: a FIFO async lock. A waiter that is dropped
/// before its turn gives the turn on; one dropped after being granted releases it.
#[derive(Default)]
struct Gate {
    held: Cell<bool>,
    queue: RefCell<VecDeque<Rc<Waiter>>>,
}

#[derive(Default)]
struct Waiter {
    granted: Cell<bool>,
    abandoned: Cell<bool>,
    waker: RefCell<Option<Waker>>,
}

impl Gate {
    fn is_held(&self) -> bool {
        self.held.get()
    }

    fn acquire(self: &Rc<Self>) -> Acquire {
        Acquire { gate: self.clone(), waiter: None, done: false }
    }

    /// Hands the gate to the next live waiter, or opens it.
    fn release(&self) {
        loop {
            let next = self.queue.borrow_mut().pop_front();
            let Some(waiter) = next else {
                self.held.set(false);
                return;
            };
            if waiter.abandoned.get() {
                continue;
            }
            waiter.granted.set(true);
            let waker = waiter.waker.borrow_mut().take();
            if let Some(waker) = waker {
                waker.wake();
            }
            return;
        }
    }
}

struct Acquire {
    gate: Rc<Gate>,
    waiter: Option<Rc<Waiter>>,
    done: bool,
}

impl Future for Acquire {
    type Output = GateGuard;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<GateGuard> {
        let this = &mut *self;
        match &this.waiter {
            None => {
                if !this.gate.held.get() && this.gate.queue.borrow().is_empty() {
                    this.gate.held.set(true);
                    this.done = true;
                    return Poll::Ready(GateGuard { gate: this.gate.clone() });
                }
                let waiter = Rc::new(Waiter::default());
                *waiter.waker.borrow_mut() = Some(cx.waker().clone());
                this.gate.queue.borrow_mut().push_back(waiter.clone());
                this.waiter = Some(waiter);
                Poll::Pending
            }
            Some(waiter) => {
                if waiter.granted.get() {
                    this.done = true;
                    return Poll::Ready(GateGuard { gate: this.gate.clone() });
                }
                *waiter.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Acquire {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        if let Some(waiter) = &self.waiter {
            if waiter.granted.get() {
                self.gate.release();
            } else {
                waiter.abandoned.set(true);
            }
        }
    }
}

struct GateGuard {
    gate: Rc<Gate>,
}

impl Drop for GateGuard {
    fn drop(&mut self) {
        self.gate.release();
    }
}
