//! Books (shell-design §8): the library host over md-core's `book_folder`, the one app-wide Book
//! window and its sidebar.
//!
//! | Module | What |
//! | --- | --- |
//! | [`library_host`] | `BookLibraryHost`: the bookmark, new / open / unpack / close, the flush hub |
//! | [`book_window`] | `BookWindow` (`WindowHost`): header toolbar, stages |
//! | [`sidebar`] | `ListBox` rows, context menu, `book.*` actions |
//!
//! The Book menu's rows are `app.` actions (shell-design §2.2), so the window manager's handlers
//! call the functions below: each brings the Book window forward when the result is a book, and
//! every whole-book output goes through [`run_book_output`].
//!
//! Owner: P5.

pub mod book_window;
pub mod library_host;
pub mod sidebar;

use std::rc::Rc;

use md_logic::books::BookOutputs;
use md_logic::seams::WindowId;

use crate::app::Shell;
use crate::book::book_window::BookWindow;
use crate::export::window_exports::WindowExports;
use crate::windows::host::WindowHost;

/// The Book window's id. There is one Book window at a time, and the window manager never hands
/// out 0 (its ids start at 1), so the Window menu's "Book" row and `WindowManager::present` can
/// name it without asking.
pub const BOOK_WINDOW_ID: WindowId = WindowId(0);

/// The four whole-book outputs (§8.4, Book menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BookOutputKind {
    /// Print Book… — always A4.
    Print,
    /// Export Book ▸ PDF… — at `md.pdfPageSize`.
    Pdf,
    Epub,
    LaTeX,
}

/// The window a Book dialog hangs on: the one the command came from, else the Book window.
fn parent_for(shell: &Rc<Shell>, invoking: Option<Rc<dyn WindowHost>>) -> Rc<dyn WindowHost> {
    if let Some(invoking) = invoking {
        return invoking;
    }
    BookWindow::current().unwrap_or_else(|| BookWindow::show(shell))
}

/// Runs a library flow and brings the Book window forward when it ends with a book open.
fn library_flow(shell: &Rc<Shell>, flow: md_logic::exec::LocalBoxFuture<'static, bool>) {
    let shell = Rc::clone(shell);
    glib::spawn_future_local(async move {
        if flow.await {
            BookWindow::show(&shell);
        }
    });
}

/// Book ▸ New Book….
pub fn new_book(shell: &Rc<Shell>, invoking: Option<Rc<dyn WindowHost>>) {
    let parent = parent_for(shell, invoking);
    library_flow(shell, shell.book.new_book(&*parent));
}

/// Book ▸ Open Book… (and the No Book Open page's button).
pub fn open_book(shell: &Rc<Shell>, invoking: Option<Rc<dyn WindowHost>>) {
    let parent = parent_for(shell, invoking);
    library_flow(shell, shell.book.open_book(&*parent));
}

/// File ▸ Examples ▸ Example Book….
pub fn unpack_example_book(shell: &Rc<Shell>, invoking: Option<Rc<dyn WindowHost>>) {
    let parent = parent_for(shell, invoking);
    library_flow(shell, shell.book.unpack_example_book(&*parent));
}

/// Book ▸ Close Book: the bookmark goes and the Book window with it — the Mac's
/// `closeBook(); dismissWindow`. The window starts closing first, so the report of a final save
/// that will not land (the setting's change lets go of the article) is one the close waits for.
/// The folder is untouched.
pub fn close_book(shell: &Rc<Shell>) {
    if let Some(window) = BookWindow::current() {
        window.request_close();
    }
    shell.book.close_book();
}

/// Print Book… / Export Book ▸ …: flush the article (the hub), list and read the book, render it.
/// The render happens in the Book window when it exists, else in `invoking` (md.win §8.7); the
/// output's alerts go to that window, and its close waits for the output.
pub fn run_book_output(shell: &Rc<Shell>, invoking: Option<Rc<dyn WindowHost>>, kind: BookOutputKind) {
    let book_window = BookWindow::current();
    let (host, exports): (Rc<dyn WindowHost>, Rc<WindowExports>) = match (book_window, invoking) {
        (Some(window), _) => {
            let exports = window.exports();
            (window, exports)
        }
        (None, Some(invoking)) => {
            let exports = WindowExports::new(shell, Rc::downgrade(&invoking));
            (invoking, exports)
        }
        (None, None) => {
            let window = BookWindow::show(shell);
            let exports = window.exports();
            (window, exports)
        }
    };
    let Some(sink) = book_sink(&exports) else {
        eprintln!("md: book output skipped: the export renderer is not wired in this build (P4)");
        return;
    };
    let output = shell.book.book_output(sink);
    let alerts = host.alerts();
    output.alert_requested().connect(move |(title, message): &(String, String)| {
        glib::spawn_future_local(alerts.warn(title.clone(), message.clone()));
    });
    let future = match kind {
        BookOutputKind::Print => output.print(),
        BookOutputKind::Pdf => output.export_pdf(),
        BookOutputKind::Epub => output.export_epub(),
        BookOutputKind::LaTeX => output.export_latex(),
    };
    host.track_output(future);
}

/// Whether book outputs reach a renderer in this build (the self-test's question).
#[cfg(feature = "selftest")]
pub fn outputs_wired(shell: &Rc<Shell>) -> bool {
    let exports = BookWindow::current().map(|window| window.exports());
    let exports = exports.unwrap_or_else(|| WindowExports::new(shell, std::rc::Weak::<BookWindow>::new()));
    book_sink(&exports).is_some()
}

/// The md-logic sink a book output renders into: the window's export pipeline.
///
/// MERGE (P4): `WindowExports::book_outputs()` returns the S0 stub's placeholder trait, not
/// md-logic's `books::BookOutputs`; once P4's export half is in, this body is
/// `Some(exports.book_outputs())`. Until then a book output stops here, before the flush.
fn book_sink(exports: &WindowExports) -> Option<Rc<dyn BookOutputs>> {
    let _placeholder = exports.book_outputs();
    None
}
