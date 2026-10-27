//! `BookLibraryHost` — the open book (shell-design §8.2): `md.bookBookmark` holds its folder path
//! (no sandbox, so the path is the whole record); New Book…, Open Book…, Example Book…, Close
//! Book; `changed` when the book opens, closes or is chosen again.
//!
//! It also owns the two values the whole app shares about books (md-logic's LINUX DECISIONS: md.win
//! keeps them static): the [`BookFlushHub`] every whole-book output posts to before it reads the
//! folder, which the Book window's article answers; and the [`BookArticleOpens`] marks the Book
//! window sets before it opens an article in a document window, which that window's view-mode
//! controller claims.
//!
//! | Mac | Linux |
//! | --- | --- |
//! | security-scoped bookmark | `md.bookBookmark` = the folder path |
//! | `beginAccess()` | [`BookLibraryHost::is_accessible`]: the folder exists; else the window shows no book and the setting is kept |
//! | `closeBook()` | [`BookLibraryHost::close_book`]: the setting goes, the folder is untouched |
//! | `newBook()` save panel | folder dialog + name prompt → one `mkdir` ([`BookLibraryHost::create_book`]) |
//! | `unpackExampleBook()` | folder dialog → a deduped copy of the bundled book ([`BookLibraryHost::unpack_example_book_into`]) |
//! | `chooseBook()` | folder dialog |
//!
//! Each dialog flow is a thin shell around a method that takes the answers as arguments, so the
//! self-test drives the same code with a folder of its own.

use std::cell::{Cell, RefCell};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use md_core::book::book_article_session::BookFlushHub;
use md_core::book::book_model;
use md_core::book::{book_folder, book_paths};
use md_core::document::book_article_opens::BookArticleOpens;
use md_logic::books::{BookLibraryModel, BookOutput, BookOutputs};
use md_logic::event::{Event, HandlerId};
use md_logic::exec::LocalBoxFuture;
use md_logic::settings::{SettingsStore, settings_keys};
use md_logic::strings::{books, buttons, documents, exports};

use crate::app::Shell;
use crate::windows::host::WindowHost;

/// The app's one book, if any.
pub struct BookLibraryHost {
    me: Weak<BookLibraryHost>,
    settings: Rc<dyn SettingsStore>,
    model: BookLibraryModel,
    hub: Rc<BookFlushHub>,
    opens: Rc<RefCell<BookArticleOpens>>,
    changed: Event<()>,
    subscription: Cell<Option<HandlerId>>,
}

impl BookLibraryHost {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Rc<BookLibraryHost> {
        let host = Rc::new_cyclic(|me: &Weak<BookLibraryHost>| BookLibraryHost {
            me: me.clone(),
            model: BookLibraryModel::new(Rc::clone(&settings)),
            settings: Rc::clone(&settings),
            hub: Rc::new(BookFlushHub::new()),
            opens: Rc::new(RefCell::new(BookArticleOpens::new())),
            changed: Event::new(),
            subscription: Cell::new(None),
        });
        // Every window watches the key (md-logic BookLibraryModel): a book opened or closed
        // anywhere — this host, another window, a hand-edited settings file — re-lists the Book
        // window.
        let weak = Rc::downgrade(&host);
        let id = settings.changed().connect(move |key: &str| {
            if key == settings_keys::BOOK_BOOKMARK
                && let Some(host) = weak.upgrade()
            {
                host.changed.emit(&());
            }
        });
        host.subscription.set(Some(id));
        host
    }

    /// A book is open: `md.bookBookmark` is non-empty (the Book menu's `book` enablement). The
    /// folder may still be out of reach; see [`BookLibraryHost::is_accessible`].
    pub fn has_book(&self) -> bool {
        self.model.has_book()
    }

    /// The book folder's path.
    pub fn folder(&self) -> Option<String> {
        self.model.book_path()
    }

    /// The Mac's `beginAccess()`: the stored folder is there to be listed. False for no book, and
    /// for a book on a drive that is gone — whose setting is kept, so it comes back with the drive.
    pub fn is_accessible(&self) -> bool {
        self.folder().is_some_and(|folder| Path::new(&folder).is_dir())
    }

    /// Folder dialog + name prompt → a new book folder; true when a book is now open.
    pub fn new_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        let (pickers, alerts, me) = (parent.pickers(), parent.alerts(), self.me.clone());
        Box::pin(async move {
            // md.win's order: where first, then the name (a folder dialog cannot ask for a name,
            // and a save dialog cannot make a folder).
            let Some(folder) = pickers.pick_folder(Some(books::NEW_BOOK_MESSAGE.to_string())).await else {
                return false;
            };
            let typed = alerts
                .prompt_name(
                    books::NEW_BOOK_TITLE.to_string(),
                    books::NEW_BOOK_MESSAGE.to_string(),
                    books::NEW_BOOK_DEFAULT_NAME.to_string(),
                    buttons::CREATE.to_string(),
                )
                .await;
            let (Some(typed), Some(host)) = (typed, me.upgrade()) else { return false };
            match host.create_book(&folder, &typed) {
                Ok(Some(path)) => {
                    host.open_folder(&path);
                    true
                }
                Ok(None) => false,
                Err(message) => {
                    alerts.warn(books::COULD_NOT_CREATE_BOOK.to_string(), message).await;
                    false
                }
            }
        })
    }

    /// Book ▸ Open Book…: a folder dialog; true when a book is now open.
    pub fn open_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        let (pickers, me) = (parent.pickers(), self.me.clone());
        Box::pin(async move {
            let Some(folder) = pickers.pick_folder(Some(books::CHOOSE_BOOK_FOLDER.to_string())).await else {
                return false;
            };
            let Some(host) = me.upgrade() else { return false };
            host.open_folder(&folder);
            true
        })
    }

    /// Copies the bundled `Example Book` into a folder the user picks and opens it.
    pub fn unpack_example_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        let (pickers, alerts, me) = (parent.pickers(), parent.alerts(), self.me.clone());
        Box::pin(async move {
            let Some(folder) = pickers.pick_folder(Some(books::CHOOSE_EXAMPLE_BOOK_FOLDER.to_string())).await else {
                return false;
            };
            let Some(host) = me.upgrade() else { return false };
            match host.unpack_example_book_into(&folder) {
                Ok(path) => {
                    host.open_folder(&path);
                    true
                }
                Err(message) => {
                    alerts.warn(books::COULD_NOT_UNPACK_EXAMPLE_BOOK.to_string(), message).await;
                    false
                }
            }
        })
    }

    /// Removes `md.bookBookmark`; the folder is untouched.
    pub fn close_book(&self) {
        self.model.close_book();
    }

    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }

    // ---- the dialog flows' cores ----

    /// Remember `path` as the open book. Choosing the book that is already open still re-lists it
    /// (the folder may have come back): the setting does not change then, so `changed` is raised
    /// here instead.
    pub fn open_folder(&self, path: &str) {
        let unchanged = self.folder().as_deref() == Some(path);
        self.model.store(path);
        if unchanged {
            self.changed.emit(&());
        }
    }

    /// New Book…'s `mkdir`: the folder `typed` (trimmed as every prompt is) inside `parent`.
    /// `Ok(None)` when nothing was typed; `Err` is the alert's message — the name rule's, "a folder
    /// of that name exists" (Foundation's `withIntermediateDirectories: false` refuses one, and so
    /// does this; an existing folder is never adopted silently), or the file system's own words.
    pub fn create_book(&self, parent: &str, typed: &str) -> Result<Option<String>, String> {
        create_book_folder(parent, typed)
    }

    /// Example Book…'s copy: the bundled book into `parent`, as "Example Book", else "Example Book
    /// 2", … (a folder dialog cannot ask "Replace?"). A copy that fails half way is removed. The
    /// new book's path, or the alert's message.
    pub fn unpack_example_book_into(&self, parent: &str) -> Result<String, String> {
        let source = Shell::get().examples.book_folder();
        unpack_book(&source, Path::new(parent)).map(|path| path.to_string_lossy().into_owned())
    }

    // ---- what the app shares about books ----

    /// The app's one flush hub: every whole-book output posts here before it reads the folder.
    pub fn flush_hub(&self) -> Rc<BookFlushHub> {
        Rc::clone(&self.hub)
    }

    /// The app's one set of book-open marks: the Book window marks, the document window that lands
    /// on the article claims (md-logic `ViewModeController::new`).
    pub fn article_opens(&self) -> Rc<RefCell<BookArticleOpens>> {
        Rc::clone(&self.opens)
    }

    /// The four whole-book outputs over `sink` (the rendering window's export pipeline). The book is
    /// listed afresh at the moment of each output, after the flush, never cached.
    pub fn book_output(&self, sink: Rc<dyn BookOutputs>) -> BookOutput {
        let settings = Rc::clone(&self.settings);
        BookOutput::new(sink, Rc::clone(&self.settings), Rc::clone(&self.hub), move || {
            BookLibraryModel::new(Rc::clone(&settings)).book_path().and_then(|folder| book_model::load(&folder))
        })
    }
}

impl Drop for BookLibraryHost {
    fn drop(&mut self) {
        if let Some(id) = self.subscription.take() {
            self.settings.changed().disconnect(id);
        }
    }
}

/// [`BookLibraryHost::create_book`].
fn create_book_folder(parent: &str, typed: &str) -> Result<Option<String>, String> {
    let name = book_folder::trimmed_prompt_name(typed);
    if name.is_empty() {
        return Ok(None);
    }
    if !book_folder::is_valid_name(name) {
        return Err(documents::INVALID_NAME_MESSAGE.to_string());
    }
    let path = book_paths::combine(parent, name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(exports::folder_exists(name));
    }
    fs::create_dir(&path).map_err(|error| format!("{path}: {error}"))?;
    Ok(Some(path))
}

/// [`BookLibraryHost::unpack_example_book_into`] with the source given.
fn unpack_book(source: &Path, parent: &Path) -> Result<PathBuf, String> {
    let base = source.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let name = BookLibraryModel::deduped_name(
        &base,
        |candidate| fs::symlink_metadata(parent.join(candidate)).is_ok(),
        BookLibraryModel::DEFAULT_MAX_ATTEMPTS,
    )
    .ok_or_else(|| exports::folder_exists(&base))?;
    let destination = parent.join(name);
    if let Err(error) = copy_folder(source, &destination) {
        // The alert names the copy's failure, not the clean-up's.
        if fs::symlink_metadata(&destination).is_ok() {
            let _ = fs::remove_dir_all(&destination);
        }
        return Err(error.to_string());
    }
    Ok(destination)
}

/// `source`'s tree into a new folder `destination`: folders recreated, files copied, nothing
/// written over (the destination must not exist; neither may anything inside it).
fn copy_folder(source: &Path, destination: &Path) -> io::Result<()> {
    let with_path =
        |error: io::Error, path: &Path| io::Error::new(error.kind(), format!("{}: {error}", path.display()));
    // The source is read before anything is made, so a missing one leaves nothing behind.
    let mut entries: Vec<fs::DirEntry> =
        fs::read_dir(source).map_err(|e| with_path(e, source))?.collect::<io::Result<_>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    fs::create_dir(destination).map_err(|e| with_path(e, destination))?;
    for entry in entries {
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if entry.file_type().map_err(|e| with_path(e, &from))?.is_dir() {
            copy_folder(&from, &to)?;
        } else {
            // `create_new`: a name that appeared meanwhile is an error, never overwritten.
            let mut reader = fs::File::open(&from).map_err(|e| with_path(e, &from))?;
            let mut writer =
                fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(|e| with_path(e, &to))?;
            io::copy(&mut reader, &mut writer).map_err(|e| with_path(e, &to))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp folder, removed when dropped.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(tag: &str) -> TempFolder {
            let path = std::env::temp_dir().join(format!("md-library-{tag}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            TempFolder(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn text(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tree(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(folder) = stack.pop() {
            for entry in fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                let relative = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                if path.is_dir() {
                    out.push((relative, None));
                    stack.push(path);
                } else {
                    out.push((relative, Some(fs::read(&path).unwrap())));
                }
            }
        }
        out.sort();
        out
    }

    fn sample_book(root: &Path) -> PathBuf {
        let book = root.join("Example Book");
        fs::create_dir_all(book.join("02-Chapter")).unwrap();
        fs::write(book.join("01-Preface.md"), "# Preface\n").unwrap();
        fs::write(book.join("02-Chapter").join("01-Привет.md"), "# Привет\n").unwrap();
        fs::create_dir(book.join("03-Empty")).unwrap();
        book
    }

    #[test]
    fn the_example_book_is_copied_whole_and_deduped() {
        let source_root = TempFolder::new("source");
        let source = sample_book(source_root.path());
        let target = TempFolder::new("target");

        let first = unpack_book(&source, target.path()).unwrap();
        assert_eq!(first, target.path().join("Example Book"));
        assert_eq!(tree(&first), tree(&source));

        let second = unpack_book(&source, target.path()).unwrap();
        assert_eq!(second, target.path().join("Example Book 2"));
        assert_eq!(tree(&second), tree(&source));

        // A file squatting on the next name is skipped too, never written over.
        fs::write(target.path().join("Example Book 3"), "x").unwrap();
        let fourth = unpack_book(&source, target.path()).unwrap();
        assert_eq!(fourth, target.path().join("Example Book 4"));
        assert_eq!(fs::read(target.path().join("Example Book 3")).unwrap(), b"x");
    }

    #[test]
    fn a_failed_copy_leaves_nothing_behind() {
        let target = TempFolder::new("failed");
        let elsewhere = TempFolder::new("failed-source");
        let missing = elsewhere.path().join("no such book");
        let message = unpack_book(&missing, target.path()).unwrap_err();
        assert!(message.contains("no such book"), "{message}");
        assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_new_book_is_one_new_folder() {
        let parent = TempFolder::new("new");
        let made = create_book_folder(&parent.text(), "  My Book\u{a0}").unwrap().unwrap();
        assert_eq!(made, format!("{}/My Book", parent.text()));
        assert!(Path::new(&made).is_dir());

        assert_eq!(create_book_folder(&parent.text(), " \t "), Ok(None));
        assert_eq!(create_book_folder(&parent.text(), "My Book"), Err(exports::folder_exists("My Book")));
        for bad in ["a/b", "a:b", ".", ".."] {
            assert_eq!(
                create_book_folder(&parent.text(), bad),
                Err(documents::INVALID_NAME_MESSAGE.to_string()),
                "{bad}"
            );
        }
        let missing = format!("{}/missing", parent.text());
        let error = create_book_folder(&missing, "Book").unwrap_err();
        assert!(error.starts_with(&format!("{missing}/Book: ")), "{error}");
    }
}
