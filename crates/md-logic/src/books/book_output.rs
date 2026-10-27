//! Port of md.win `Books/BookOutput.cs`: Compile to PDF, Print Book, Export Book as EPUB / LaTeX
//! (§8.7, book.md §8). Every one of them reads the book off disk, so every one of them first asks
//! the in-place editor to save — through md-core's [`BookFlushHub`], which is synchronous for
//! exactly that reason: the save has happened when [`BookFlushHub::flush_editor`] returns, before
//! a single article is read.
//!
//! LINUX DECISIONS:
//! - No Share Book as PDF: Linux has no share sheet, so md.win's `IBookOutputs.SharePdfAsync` and
//!   `BookOutput.SharePdfAsync` are not ported. Export Book ▸ PDF… writes the same file.
//! - md.win's static `BookFlushGate.Requested` event is md-core's [`BookFlushHub`] value the app
//!   owns one of; [`BookFlushGateBinding`] subscribes to it for as long as it lives.

use std::cell::Cell;
use std::fmt;
use std::rc::{Rc, Weak};

use md_core::book::book_article_session::{BookFlushGate, BookFlushHub, BookFlushSubscriber};
use md_core::book::book_compiler;
use md_core::book::book_folder;
use md_core::book::book_model::Book;
use md_core::book::structured_book::StructuredBook;
use md_core::document::page_size::PageSize;

use crate::event::Event;
use crate::exec::{LocalBoxFuture, ready};
use crate::export::ExportPipeline;
use crate::seams::RenderError;
use crate::settings::{SettingsStore, settings_keys};
use crate::strings::{books, exports};

/// The app's export half, as the book outputs need it (md.win's `IBookOutputs`).
/// [`PipelineBookOutputs`] is the implementation over the window's [`ExportPipeline`]; the trait
/// exists so the book window can be built and tested without one. Every book render is light — a
/// compiled book is paper, whatever the window it was started from looks like.
pub trait BookOutputs {
    /// Export Book ▸ PDF… — render `source` at `page_size`, saved through the dialog.
    fn export_pdf(&self, source: String, title: String, page_size: PageSize) -> LocalBoxFuture<'static, ()>;

    /// Print Book… — always A4: the page size setting is not consulted (core-api.md A4 §4 step 5).
    fn print(&self, source: String, title: String) -> LocalBoxFuture<'static, ()>;

    /// Export Book ▸ EPUB… — the structured book, one file per article.
    fn export_epub(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()>;

    /// Export Book ▸ LaTeX… — the structured book as one `.tex`.
    fn export_latex(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()>;
}

/// [`BookOutputs`] over a window's [`ExportPipeline`] (md.win's `Md.App.Book.BookExportOutputs`):
/// the book's PDF, EPUB and LaTeX are the pipeline's book flows, and Print is the pipeline's print
/// with the app's `show_print` (see [`ExportPipeline::print`]).
pub struct PipelineBookOutputs {
    pipeline: Rc<ExportPipeline>,
    show_print: Rc<dyn Fn(String) -> LocalBoxFuture<'static, Result<(), RenderError>>>,
}

impl PipelineBookOutputs {
    pub fn new(
        pipeline: Rc<ExportPipeline>,
        show_print: impl Fn(String) -> LocalBoxFuture<'static, Result<(), RenderError>> + 'static,
    ) -> Self {
        PipelineBookOutputs { pipeline, show_print: Rc::new(show_print) }
    }
}

impl fmt::Debug for PipelineBookOutputs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineBookOutputs").field("pipeline", &self.pipeline).finish_non_exhaustive()
    }
}

impl BookOutputs for PipelineBookOutputs {
    fn export_pdf(&self, source: String, title: String, page_size: PageSize) -> LocalBoxFuture<'static, ()> {
        self.pipeline.export_pdf(&source, &title, &page_size)
    }

    fn print(&self, source: String, title: String) -> LocalBoxFuture<'static, ()> {
        let show_print = self.show_print.clone();
        self.pipeline.print(&source, &title, move |html| show_print(html))
    }

    fn export_epub(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.pipeline.export_book_epub(book)
    }

    fn export_latex(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.pipeline.export_book_latex(book)
    }
}

struct Binding {
    flush: Box<dyn Fn() -> bool>,
    requests: Cell<usize>,
}

impl BookFlushSubscriber for Binding {
    fn flush_requested(&self, gate: &mut BookFlushGate) {
        self.requests.set(self.requests.get() + 1);
        if !(self.flush)() {
            gate.vetoed = true;
        }
    }
}

/// Holds a subscription to the [`BookFlushHub`] for as long as a book editor exists: every output
/// posts a request before it reads the folder, and this answers it by flushing. A failed flush
/// vetoes, and the output abandons itself silently — the footer already says why, and a second
/// alert on top of it would be noise. No book window this launch means no subscriber, which leaves
/// the gate open, which is exactly right: there is nothing unsaved.
///
/// Dropping the binding (or [`BookFlushGateBinding::dispose`]) unsubscribes: the hub holds it
/// weakly.
pub struct BookFlushGateBinding {
    binding: Rc<Binding>,
}

impl fmt::Debug for BookFlushGateBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BookFlushGateBinding").field("requests", &self.requests()).finish()
    }
}

impl BookFlushGateBinding {
    /// Answer every request posted to `hub` with `flush` (the book window passes its session's
    /// `flush_now(false)`); false vetoes.
    pub fn new(hub: &BookFlushHub, flush: impl Fn() -> bool + 'static) -> Self {
        let binding = Rc::new(Binding { flush: Box::new(flush), requests: Cell::new(0) });
        let weak: Weak<dyn BookFlushSubscriber> = Rc::downgrade(&binding) as Weak<Binding>;
        hub.subscribe(weak);
        BookFlushGateBinding { binding }
    }

    /// How many requests this binding has answered — for the window's own diagnostics and the
    /// tests.
    pub fn requests(&self) -> usize {
        self.binding.requests.get()
    }

    /// Unsubscribe now (md.win's `Dispose`; dropping does the same).
    pub fn dispose(self) {}
}

/// The four whole-book outputs. Each one: flush the editor (abandon on a veto), read the book, then
/// hand the result to [`BookOutputs`]. Reading is where a book fails — no book open, the folder
/// gone, one article unreadable — and each failure is a modal alert with the Mac's wording
/// ([`BookOutput::alert_requested`]), never a half-finished file.
///
/// The work up to the hand-off is synchronous, inside the call; the returned future is the
/// output's own (a finished one when the output abandoned itself).
pub struct BookOutput {
    outputs: Rc<dyn BookOutputs>,
    settings: Rc<dyn SettingsStore>,
    hub: Rc<BookFlushHub>,
    book: Box<dyn Fn() -> Option<Book>>,
    read_article: Box<dyn Fn(&str) -> Option<String>>,
    alert_requested: Event<(String, String)>,
}

impl fmt::Debug for BookOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BookOutput").field("page_size", &self.page_size().id).finish_non_exhaustive()
    }
}

impl BookOutput {
    /// `book` is the current listing — re-read at the moment of the output, never cached from
    /// window construction. Articles are read with md-core's [`book_folder::read_article`], which
    /// decodes through `plain_text_codec`: the recorded divergence from the Mac, whose compile path
    /// decodes UTF-8-then-Latin-1 and turns a CP1251 article that edits correctly into mojibake.
    pub fn new(
        outputs: Rc<dyn BookOutputs>,
        settings: Rc<dyn SettingsStore>,
        hub: Rc<BookFlushHub>,
        book: impl Fn() -> Option<Book> + 'static,
    ) -> Self {
        BookOutput {
            outputs,
            settings,
            hub,
            book: Box::new(book),
            read_article: Box::new(book_folder::read_article),
            alert_requested: Event::new(),
        }
    }

    /// Articles are read with `read_article` instead (`None` = unreadable).
    pub fn with_reader(mut self, read_article: impl Fn(&str) -> Option<String> + 'static) -> Self {
        self.read_article = Box::new(read_article);
        self
    }

    /// (title, message) for the modal alert.
    pub fn alert_requested(&self) -> &Event<(String, String)> {
        &self.alert_requested
    }

    /// The trim size the PDF output uses — `md.pdfPageSize`, shared with the document window's
    /// menu. Unknown ids read as A4.
    pub fn page_size(&self) -> &'static PageSize {
        PageSize::named(self.settings.get_string(settings_keys::PDF_PAGE_SIZE).as_deref())
    }

    pub fn export_pdf(&self) -> LocalBoxFuture<'static, ()> {
        match self.compile() {
            Some((title, source)) => self.outputs.export_pdf(source, title, *self.page_size()),
            None => ready(()),
        }
    }

    pub fn print(&self) -> LocalBoxFuture<'static, ()> {
        match self.compile() {
            Some((title, source)) => self.outputs.print(source, title),
            None => ready(()),
        }
    }

    pub fn export_epub(&self) -> LocalBoxFuture<'static, ()> {
        match self.structured(exports::COULD_NOT_EXPORT_EPUB) {
            Some(book) => self.outputs.export_epub(book),
            None => ready(()),
        }
    }

    pub fn export_latex(&self) -> LocalBoxFuture<'static, ()> {
        match self.structured(exports::COULD_NOT_EXPORT_LATEX) {
            Some(book) => self.outputs.export_latex(book),
            None => ready(()),
        }
    }

    /// The whole book as one Markdown source — a title page, then every chapter heading and article
    /// on a page of its own. `None` when the gate vetoed (silently) or the read failed (with an
    /// alert).
    fn compile(&self) -> Option<(String, String)> {
        let current = self.ready(None)?;
        match book_compiler::compile_book_source(&current, |path| (self.read_article)(path)) {
            Ok(source) => Some((book_compiler::title(&current), source)),
            Err(unreadable) => {
                self.alert(books::COULD_NOT_COMPILE_BOOK, &books::article_unreadable(&unreadable));
                None
            }
        }
    }

    /// The book with its chapter and article boundaries kept — what EPUB and LaTeX need.
    /// `failure_title` is the caller's alert title, as on the Mac.
    fn structured(&self, failure_title: &str) -> Option<StructuredBook> {
        let current = self.ready(Some(failure_title))?;
        match book_compiler::read_structured_book(&current, |path| (self.read_article)(path)) {
            Ok(book) => Some(book),
            Err(unreadable) => {
                self.alert(failure_title, &books::article_unreadable(&unreadable));
                None
            }
        }
    }

    /// The gate first, then the listing: a veto is silent, an inaccessible book is an alert.
    fn ready(&self, failure_title: Option<&str>) -> Option<Book> {
        if !self.hub.flush_editor() {
            return None;
        }
        let current = (self.book)();
        if current.is_none() {
            self.alert(failure_title.unwrap_or(books::COULD_NOT_COMPILE_BOOK), books::NO_BOOK_ACCESSIBLE);
        }
        current
    }

    fn alert(&self, title: &str, message: &str) {
        self.alert_requested.emit(&(title.to_string(), message.to_string()));
    }
}
