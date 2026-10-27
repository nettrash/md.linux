//! Port of md.win `tests/Md.App.Logic.Tests/Books/BookFixture.cs` (`BookFixture`,
//! `FakeArticleSession`) and the `FakeBookOutputs` of `BookOutputTests.cs`.
//!
//! A real book folder in a temporary directory: articles are files, chapters are subfolders. The
//! book tests run against a disk on purpose — the navigator's operations are renames, deletes and
//! two-phase reorders, and a fake file system would pin the fake's behaviour, not Linux's. Each
//! fixture is its own uniquely named folder (deleted on drop), so the suites run in parallel.

use std::cell::{Cell, RefCell};
use std::fs;
use std::rc::Rc;

use md_core::book::book_article_session::BookFlushHub;
use md_core::book::book_model::{self, Book};
use md_core::book::structured_book::StructuredBook;
use md_core::document::book_article_opens::BookArticleOpens;
use md_core::document::page_size::PageSize;
use md_logic::books::{BookArticleSession, BookNavigatorModel, BookOutputs};
use md_logic::testing::*;

use super::document_fixtures::TempDir;

pub struct BookFixture {
    dir: TempDir,
}

impl BookFixture {
    pub fn new() -> Self {
        BookFixture { dir: TempDir::new("md-linux-book-") }
    }

    /// The book's root folder (resolved, so it is the spelling the listing produces).
    pub fn root(&self) -> &str {
        &self.dir.path
    }

    /// The root joined with `parts`.
    pub fn at(&self, parts: &[&str]) -> String {
        self.dir.join(parts)
    }

    /// Write an article; intermediate chapter folders are created.
    pub fn article(&self, relative: &str, text: &str) -> String {
        self.dir.write(relative, text)
    }

    /// [`BookFixture::article`] with md.win's default text, `# Scene\n`.
    pub fn scene(&self, relative: &str) -> String {
        self.article(relative, "# Scene\n")
    }

    pub fn chapter(&self, name: &str) -> String {
        let path = self.at(&[name]);
        fs::create_dir_all(&path).expect("create the chapter");
        path
    }

    /// The names of everything directly in `folder` (the root by default) that does not start with
    /// a dot, sorted ordinally — what a rename or a reorder is checked against.
    pub fn names_in(&self, folder: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .expect("list the folder")
            .map(|entry| entry.expect("an entry").file_name().into_string().expect("a UTF-8 name"))
            .filter(|name| !name.starts_with('.'))
            .collect();
        names.sort();
        names
    }

    pub fn names(&self) -> Vec<String> {
        self.names_in(self.root())
    }

    /// The book as md-core lists it.
    pub fn load(&self) -> Book {
        book_model::load(self.root()).expect("the fixture lists")
    }

    pub fn text(&self, path: &str) -> String {
        fs::read_to_string(path).expect("read the article")
    }
}

/// [`BookArticleSession`] without a file system: it records what it was asked to do and can be told
/// to refuse. "Refuse" is the state that matters — a failed flush must abort a selection change and
/// a whole managed operation, and that cannot be provoked through a disk in a test.
#[derive(Default)]
pub struct FakeArticleSession {
    editing_path: RefCell<Option<String>>,
    /// While true every `select` and `hand_off_for_external_open` fails, as an unsaveable article
    /// does.
    pub flush_fails: Cell<bool>,
    /// Every path the session was asked to hold, in order; `None` is a deselect.
    pub selected: RefCell<Vec<Option<String>>>,
    pub hand_offs: Cell<usize>,
    /// Every `detach` and whether it was asked to report a failure.
    pub detaches: RefCell<Vec<bool>>,
}

impl FakeArticleSession {
    pub fn editing(&self) -> Option<String> {
        self.editing_path.borrow().clone()
    }

    pub fn selected(&self) -> Vec<Option<String>> {
        self.selected.borrow().clone()
    }

    pub fn detaches(&self) -> Vec<bool> {
        self.detaches.borrow().clone()
    }
}

impl BookArticleSession for FakeArticleSession {
    fn editing_path(&self) -> Option<String> {
        self.editing()
    }

    fn select(&self, path: Option<&str>) -> bool {
        if self.flush_fails.get() {
            return false;
        }
        self.selected.borrow_mut().push(path.map(str::to_string));
        *self.editing_path.borrow_mut() = path.map(str::to_string);
        true
    }

    fn detach(&self, report_failure: bool) {
        self.detaches.borrow_mut().push(report_failure);
        *self.editing_path.borrow_mut() = None;
    }

    fn hand_off_for_external_open(&self) -> bool {
        self.hand_offs.set(self.hand_offs.get() + 1);
        if self.flush_fails.get() {
            return false;
        }
        *self.editing_path.borrow_mut() = None;
        true
    }
}

/// A navigator over a fake session and fresh settings, with its own [`BookArticleOpens`].
pub struct Navigator {
    pub model: BookNavigatorModel,
    pub session: Rc<FakeArticleSession>,
    pub settings: Rc<FakeSettingsStore>,
    pub opens: Rc<RefCell<BookArticleOpens>>,
    pub alerts: Rc<RefCell<Vec<(String, String)>>>,
    pub opened: Rc<RefCell<Vec<String>>>,
}

impl Navigator {
    /// md.win's `new BookNavigatorModel(session, settings)` before `OpenBook`.
    pub fn with_settings(settings: Rc<FakeSettingsStore>) -> Self {
        let session = Rc::new(FakeArticleSession::default());
        let opens = Rc::new(RefCell::new(BookArticleOpens::new()));
        let model = BookNavigatorModel::new(session.clone(), settings.clone(), opens.clone());
        let alerts: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let alerts = alerts.clone();
            model.alert_requested().connect(move |alert: &(String, String)| alerts.borrow_mut().push(alert.clone()));
        }
        let opened: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let opened = opened.clone();
            model.open_requested().connect(move |path: &str| opened.borrow_mut().push(path.to_string()));
        }
        Navigator { model, session, settings, opens, alerts, opened }
    }

    /// md.win's `Open(fixture)`: a fresh session and settings, the book opened.
    pub fn open(fixture: &BookFixture) -> Self {
        let navigator = Self::with_settings(Rc::new(FakeSettingsStore::new()));
        navigator.model.open_book(fixture.root());
        navigator
    }

    pub fn alerts(&self) -> Vec<(String, String)> {
        self.alerts.borrow().clone()
    }

    pub fn opened(&self) -> Vec<String> {
        self.opened.borrow().clone()
    }

    pub fn selection(&self) -> Option<String> {
        self.model.selection()
    }

    /// The selected article's file name.
    pub fn selected_name(&self) -> Option<String> {
        self.model.selection().map(|path| file_name(&path).to_string())
    }
}

/// The last `/`-separated component — `Path.GetFileName`.
pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Records what the export half was asked for; the pipeline itself is tested on its own.
#[derive(Default)]
pub struct FakeBookOutputs {
    pub pdfs: RefCell<Vec<(String, String, String)>>,
    pub prints: RefCell<Vec<(String, String)>>,
    pub epubs: RefCell<Vec<StructuredBook>>,
    pub latex: RefCell<Vec<StructuredBook>>,
}

impl BookOutputs for FakeBookOutputs {
    fn export_pdf(&self, source: String, title: String, page_size: PageSize) -> LocalBoxFuture<'static, ()> {
        self.pdfs.borrow_mut().push((source, title, page_size.id.to_string()));
        ready(())
    }

    fn print(&self, source: String, title: String) -> LocalBoxFuture<'static, ()> {
        self.prints.borrow_mut().push((source, title));
        ready(())
    }

    fn export_epub(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.epubs.borrow_mut().push(book);
        ready(())
    }

    fn export_latex(&self, book: StructuredBook) -> LocalBoxFuture<'static, ()> {
        self.latex.borrow_mut().push(book);
        ready(())
    }
}

/// A hub of the test's own — md.win's static `BookFlushGate`, which forced its suites into one
/// collection; here every test has its own and they run in parallel.
pub fn hub() -> Rc<BookFlushHub> {
    Rc::new(BookFlushHub::new())
}
