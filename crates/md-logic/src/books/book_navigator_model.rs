//! Port of md.win `Books/BookNavigatorModel.cs`: the book workspace's brain (§8.5, book.md §13.7
//! verbatim) — what is selected, what a click / rename / move / delete does to the selection, and
//! what gets remembered. Every rule that can be tested without a window lives here; the GTK book
//! window only renders the answers.
//!
//! The Swift original is a SwiftUI view whose `selection` is `@State` with an `.onChange` handler,
//! and two of its behaviours are consequences of that and not of the algorithm:
//! - onChange fires on the NET change of a synchronous body — `performManaged` assigns nil and then
//!   the new selection, and only one handler call follows. That is why the assignments below are
//!   silent and each public entry point dispatches once at the end.
//! - a revert inside the handler (`selection = session.editingURL`) fires onChange a second time,
//!   whose only lasting effect is re-writing `md.bookLastArticle` with the path it already holds.
//!   Not reproduced: the value would not change.
//!
//! Rust shape: every method takes `&self` (state in cells) and no borrow is held while the session,
//! the settings store or an event handler runs, so a `changed` handler may read the model back.
//! md.win's static `BookArticleOpens` is md-core's value the app owns one of, shared here as an
//! `Rc<RefCell<…>>` with the document windows that claim the mark.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use md_core::book::book_folder::{self, BookFolderFileSystem, LocalBookFolderFileSystem, RenameOutcome};
use md_core::book::book_model::{self, Book, BookArticle, BookListing, DirectoryBookListing};
use md_core::book::renumber_plan::{self, RenamePair};
use md_core::book::{book_naming, book_paths, destination};
use md_core::document::book_article_opens::BookArticleOpens;

use super::book_sidebar_model;
use super::book_stepper::BookStepper;
use crate::documents::{TextFileSession, file_names};
use crate::event::Event;
use crate::settings::{SettingsStore, settings_keys};
use crate::strings::{books, documents};

/// The half of the in-place editor the navigator drives (md.win's `IBookArticleSession`).
/// [`TextFileSession`] in the app (through [`TextFileSessionArticles`]); a fake in the tests,
/// because the navigator's contract is "a failed flush aborts the operation" and that has to be
/// provable without a disk that refuses writes.
///
/// Not md-core's `book_article_session::BookArticleSession` struct (the core state machine), which
/// this trait is named after in md.win's spelling.
pub trait BookArticleSession {
    /// The article being edited in place, or `None` (handoff, unreadable and empty all answer
    /// `None`).
    fn editing_path(&self) -> Option<String>;

    /// Flush the outgoing article, then load `path` (`None` deselects). False = the flush failed
    /// and nothing moved.
    fn select(&self, path: Option<&str>) -> bool;

    /// Let go of the article unconditionally — the book was closed. Unlike
    /// [`BookArticleSession::select`] this always completes: a failed flush parks the buffer in a
    /// rescue copy and reports it rather than keeping an article open in a book that is gone.
    fn detach(&self, report_failure: bool);

    /// Save and step aside before a document window opens the article. False = the save failed
    /// and the open must be abandoned.
    fn hand_off_for_external_open(&self) -> bool;
}

/// [`BookArticleSession`] over the documents module's session in
/// [`SessionRole::BookArticle`](crate::documents::SessionRole::BookArticle) mode.
pub struct TextFileSessionArticles {
    session: Rc<TextFileSession>,
}

impl TextFileSessionArticles {
    pub fn new(session: Rc<TextFileSession>) -> Self {
        TextFileSessionArticles { session }
    }
}

impl fmt::Debug for TextFileSessionArticles {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TextFileSessionArticles").field(&self.session).finish()
    }
}

impl BookArticleSession for TextFileSessionArticles {
    fn editing_path(&self) -> Option<String> {
        self.session.editing_path()
    }

    fn select(&self, path: Option<&str>) -> bool {
        self.session.select(path)
    }

    fn detach(&self, report_failure: bool) {
        self.session.detach(report_failure);
    }

    fn hand_off_for_external_open(&self) -> bool {
        self.session.hand_off_for_external_open()
    }
}

/// Selection, listing and the four management operations of the book window.
///
/// The invariants worth naming, because each is a promise the tests pin: a failed flush never loses
/// text — it aborts the selection change and the whole managed operation, and the sidebar goes back
/// to the article that would not save; the selection follows its article through a rename or a
/// reorder (`destination::of`) and falls to a neighbour when it is deleted
/// (`book_folder::deletion_neighbor`, computed while the row still exists); the last article
/// written in is remembered relative to the root, so a moved book still reopens where the writer
/// left off; and while articles open in their own windows the sidebar is a launcher with no
/// selection at all.
pub struct BookNavigatorModel {
    session: Rc<dyn BookArticleSession>,
    settings: Rc<dyn SettingsStore>,
    opens: Rc<RefCell<BookArticleOpens>>,
    files: Rc<dyn BookFolderFileSystem>,
    listing: Rc<dyn BookListing>,
    root: RefCell<Option<String>>,
    book: RefCell<Option<Book>>,
    selection: RefCell<Option<String>>,
    changed: Event<()>,
    selection_changing: Event<()>,
    open_requested: Event<str>,
    alert_requested: Event<(String, String)>,
}

impl fmt::Debug for BookNavigatorModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BookNavigatorModel")
            .field("root", &self.root.borrow())
            .field("selection", &self.selection.borrow())
            .finish()
    }
}

impl BookNavigatorModel {
    /// A navigator with no book open, over the real folder (`std::fs`). `opens` is the app's one
    /// [`BookArticleOpens`], shared with the windows that claim the mark.
    pub fn new(
        session: Rc<dyn BookArticleSession>,
        settings: Rc<dyn SettingsStore>,
        opens: Rc<RefCell<BookArticleOpens>>,
    ) -> Self {
        BookNavigatorModel {
            session,
            settings,
            opens,
            files: Rc::new(LocalBookFolderFileSystem),
            listing: Rc::new(DirectoryBookListing),
            root: RefCell::new(None),
            book: RefCell::new(None),
            selection: RefCell::new(None),
            changed: Event::new(),
            selection_changing: Event::new(),
            open_requested: Event::new(),
            alert_requested: Event::new(),
        }
    }

    /// The folder operations go through `files` instead of `std::fs`.
    pub fn with_files(mut self, files: Rc<dyn BookFolderFileSystem>) -> Self {
        self.files = files;
        self
    }

    /// The listing goes through `listing` instead of `std::fs`.
    pub fn with_listing(mut self, listing: Rc<dyn BookListing>) -> Self {
        self.listing = listing;
        self
    }

    // ---- state ----

    /// The open book's root folder (standardized), or `None`.
    pub fn root(&self) -> Option<String> {
        self.root.borrow().clone()
    }

    /// The last listing. `None` when there is no book or the folder could not be read — both show
    /// the empty state.
    pub fn book(&self) -> Option<Book> {
        self.book.borrow().clone()
    }

    /// The selected article, or `None`. Always a path as the listing spells it.
    pub fn selection(&self) -> Option<String> {
        self.selection.borrow().clone()
    }

    /// Raised after any change to the book or the selection: the window re-renders the sidebar and
    /// the detail pane.
    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }

    /// Raised at the top of every selection change, before the session moves. The window clears
    /// the one-shot Contents / Notes jump requests here, so a jump never replays into the next
    /// article.
    pub fn selection_changing(&self) -> &Event<()> {
        &self.selection_changing
    }

    /// An article should be opened in its own document window (after the handoff has happened).
    pub fn open_requested(&self) -> &Event<str> {
        &self.open_requested
    }

    /// (title, message) for the modal alert — "Could not rename", "Could not reorder", "Could not
    /// delete".
    pub fn alert_requested(&self) -> &Event<(String, String)> {
        &self.alert_requested
    }

    /// The app-wide "Open Articles in Separate Windows" preference (§8.4).
    pub fn opens_in_separate_windows(&self) -> bool {
        self.settings.get_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, false)
    }

    /// Writing it re-lists, because the sidebar stops holding a selection the moment it goes on.
    pub fn set_opens_in_separate_windows(&self, value: bool) {
        if self.opens_in_separate_windows() == value {
            return;
        }
        self.settings.set_bool(settings_keys::BOOK_OPENS_IN_SEPARATE_WINDOWS, value);
        self.reload(None);
    }

    /// The remembered article, `/`-separated and relative to the root (§9); "" when nothing is
    /// remembered.
    pub fn last_article_path(&self) -> String {
        self.settings.get_string(settings_keys::BOOK_LAST_ARTICLE).unwrap_or_default()
    }

    /// Previous / Next over the current listing and selection.
    pub fn stepper(&self) -> BookStepper {
        let book = self.book.borrow();
        BookStepper::for_book(book.as_ref(), self.selection.borrow().as_deref(), self.opens_in_separate_windows())
    }

    // ---- book lifecycle ----

    /// Open the book at `root`: list it, pick a selection and hand it to the session. The dispatch
    /// is unconditional, which is Swift's `onAppear` calling `selectionChanged(selection)` by hand —
    /// a restored selection that happens to equal the one the reload computed would otherwise never
    /// reach the session.
    pub fn open_book(&self, root: &str) {
        *self.root.borrow_mut() = Some(book_paths::standardize(root));
        self.reload_core(None);
        let selection = self.selection();
        self.selection_changed(selection.as_deref());
        self.changed.emit(&());
    }

    /// Close the book: let go of the article, forget the listing. The folder is untouched. The
    /// detach reports a failed final save (Swift's `closeBook` is `detach(reportFailure: true)`).
    pub fn close_book(&self) {
        self.selection_changing.emit(&());
        self.session.detach(true);
        *self.selection.borrow_mut() = None;
        *self.book.borrow_mut() = None;
        *self.root.borrow_mut() = None;
        self.changed.emit(&());
    }

    // ---- listing and selection ----

    /// Re-list the book and re-point the selection: the caller's preference if it still exists,
    /// else the current selection re-spelled as the listing spells it, else the remembered article,
    /// else the first article in reading order. Dispatches to the session only when the net
    /// selection moved — the SwiftUI onChange rule.
    pub fn reload(&self, preferring: Option<&str>) {
        let before = self.selection();
        self.reload_core(preferring);
        let after = self.selection();
        if before != after {
            self.selection_changed(after.as_deref());
        }
        self.changed.emit(&());
    }

    fn reload_core(&self, preferring: Option<&str>) {
        let root = self.root();
        let book = root.and_then(|root| book_model::load_with(&root, &*self.listing));
        *self.book.borrow_mut() = book.clone();
        let Some(book) = book else {
            *self.selection.borrow_mut() = None;
            return;
        };
        // A launcher never auto-selects: clicking a row opens a window, so a highlighted row would
        // be a lie about where the writing goes.
        if self.opens_in_separate_windows() {
            *self.selection.borrow_mut() = None;
            return;
        }
        let order = book_model::reading_order(&book);
        let current = self.selection();
        let chosen = existing(&order, preferring).or_else(|| existing(&order, current.as_deref())).or_else(|| {
            let last = self.last_article_path();
            let remembered = if last.is_empty() {
                None
            } else {
                existing(&order, book_folder::path_in(&book.root, &last).as_deref())
            };
            remembered.or_else(|| order.first().map(|article| article.path.clone()))
        });
        *self.selection.borrow_mut() = chosen;
    }

    /// A row was clicked (or a step landed): move the selection there. A no-op when the value has
    /// not changed, because a SwiftUI List writes its binding only on a real change and the session
    /// is idempotent anyway.
    pub fn select(&self, path: Option<&str>) {
        if self.selection.borrow().as_deref() == path {
            return;
        }
        *self.selection.borrow_mut() = path.map(str::to_string);
        self.selection_changed(path);
        self.changed.emit(&());
    }

    /// The selection handler itself (book.md §13.7). Public because [`BookNavigatorModel::open_book`]
    /// and [`BookNavigatorModel::perform_managed`] call it by hand where SwiftUI's onChange would not
    /// fire.
    pub fn selection_changed(&self, path: Option<&str>) {
        self.selection_changing.emit(&());
        let Some(path) = path else {
            // A failed flush on a deselect keeps the unsaved article on screen with its error.
            if !self.session.select(None) {
                *self.selection.borrow_mut() = self.session.editing_path();
            }
            return;
        };
        if self.opens_in_separate_windows() {
            // The click is a gesture, not a selection: the highlight clears and a window opens.
            *self.selection.borrow_mut() = None;
            self.open_in_window(path);
            return;
        }
        if self.session.select(Some(path)) {
            self.settings.set_string(settings_keys::BOOK_LAST_ARTICLE, &self.relative_path(path));
        } else {
            *self.selection.borrow_mut() = self.session.editing_path();
        }
    }

    /// `path` relative to the book root, `/`-separated — the Mac's spelling of
    /// `md.bookLastArticle`, kept because it is the one form that survives the book folder being
    /// moved or renamed. "" when there is no book or the path is outside it.
    pub fn relative_path(&self, path: &str) -> String {
        match self.root() {
            Some(root) => book_folder::relative_path(&root, path),
            None => String::new(),
        }
    }

    // ---- stepping ----

    /// Alt+Up / the up chevron.
    pub fn step_previous(&self) {
        let target = self.stepper().previous().map(str::to_string);
        if let Some(target) = target {
            self.select(Some(&target));
        }
    }

    /// Alt+Down / the down chevron.
    pub fn step_next(&self) {
        let target = self.stepper().next().map(str::to_string);
        if let Some(target) = target {
            self.select(Some(&target));
        }
    }

    // ---- managed operations ----

    /// The frame every mutation runs in: detach the session first (so nothing is being written
    /// while files move under it), run the operation, re-list preferring whatever it points at,
    /// then dispatch the selection once. A failed flush aborts before anything is touched — the
    /// promise that "the article that will not save is never left behind".
    ///
    /// `operation` takes the selection as it was and returns where the selection should end up
    /// (`None` = let the reload decide). Returning the input is how a failed operation leaves
    /// everything where it was.
    pub fn perform_managed(&self, operation: impl FnOnce(Option<&str>) -> Option<String>) -> bool {
        let previous = self.selection();
        self.selection_changing.emit(&());
        if !self.session.select(None) {
            *self.selection.borrow_mut() = self.session.editing_path();
            self.changed.emit(&());
            return false;
        }
        *self.selection.borrow_mut() = None;
        let preferring = operation(previous.as_deref());
        self.reload_core(preferring.as_deref());
        let selection = self.selection();
        self.selection_changed(selection.as_deref());
        self.changed.emit(&());
        true
    }

    /// New Chapter…: a folder in the root. It does not detach the session — no article moves. A
    /// collision stays quiet, as Swift is; an unusable *name* does not, because §8.3 asks all three
    /// name prompts to vet with [`file_names::validate`] and say so — a silent no-op on a refused
    /// name reads as a broken button rather than as a refusal.
    pub fn create_chapter(&self, typed_name: &str) {
        let Some(root) = self.root() else { return };
        let name = book_folder::trimmed_prompt_name(typed_name);
        if name.is_empty() {
            return;
        }
        // The name that lands on disk is exactly what was typed — that is what md-core vets too.
        if !self.vet(name) {
            return;
        }
        book_folder::create_chapter_with(&root, name, &*self.files);
        self.reload(None);
    }

    /// New Article…: a seeded Markdown file in `folder`, selected when it lands.
    pub fn create_article(&self, typed_name: &str, folder: &str) {
        let name = book_folder::trimmed_prompt_name(typed_name);
        if name.is_empty() {
            return;
        }
        // The FILE NAME is vetted, never the typed stem: md-core's create_article vets the same
        // string.
        if !self.vet(&format!("{name}.md")) {
            return;
        }
        self.perform_managed(|previous| {
            book_folder::create_article_with(folder, name, &*self.files).or_else(|| previous.map(str::to_string))
        });
    }

    /// False (and an alert already raised) when `file_name` is one Linux will not hold (§8.3, §6.4).
    fn vet(&self, file_name: &str) -> bool {
        if file_names::validate(Some(file_name)) {
            return true;
        }
        self.alert(documents::COULD_NOT_RENAME, documents::INVALID_NAME_MESSAGE);
        false
    }

    /// Move Up / Move Down for the item at `path`; `delta` is -1 or +1 (md.win's `Move(path,
    /// delta)`).
    pub fn move_by(&self, path: &str, delta: isize) {
        let Some(book) = self.book() else { return };
        let Some(commands) = book_sidebar_model::commands_for(Some(&book), Some(path)) else { return };
        let from = commands.index as isize;
        self.move_to(from, from + delta, &commands.siblings, &commands.folder);
    }

    /// Renumber a sibling group so the item at `from` lands at `to` (md.win's `Move(from, to,
    /// siblings, folder)`). The plan is materialised as renames and applied two-phase, so a swap
    /// ("01-a" ↔ "02-a") cannot collide midway; the selection is remapped through the same plan.
    pub fn move_to(&self, from: isize, to: isize, siblings: &[String], folder: &str) {
        self.perform_managed(|previous| {
            let plan = renumber_plan::plan(siblings, from, to);
            if let Err(failure) = renumber_plan::apply_with(&plan, folder, &*self.files) {
                self.alert(books::COULD_NOT_REORDER, &failure);
                return previous.map(str::to_string);
            }
            previous.map(|previous| destination::of(previous, folder, &plan))
        });
    }

    /// Rename…: only the readable half of the name changes — the ordering prefix and the extension
    /// are kept. An invalid name and a collision both come back as an alert, and the selection
    /// stays where it was.
    pub fn perform_rename(&self, path: &str, typed_stem: &str) {
        let stem = book_folder::trimmed_prompt_name(typed_stem);
        if stem.is_empty() {
            return;
        }
        self.perform_managed(|previous| {
            let name = book_paths::name(path).to_string();
            match book_folder::rename_item_with(path, stem, &*self.files) {
                RenameOutcome::InvalidName => {
                    self.alert(documents::COULD_NOT_RENAME, documents::INVALID_NAME_MESSAGE);
                    return previous.map(str::to_string);
                }
                RenameOutcome::Failed(failure) => {
                    self.alert(documents::COULD_NOT_RENAME, &failure);
                    return previous.map(str::to_string);
                }
                RenameOutcome::Renamed | RenameOutcome::Unchanged => {}
            }
            let previous = previous?;
            let Some(folder) = book_paths::directory_name(book_paths::trim_ending_separator(path)) else {
                return Some(previous.to_string());
            };
            let plan = [RenamePair::new(name.clone(), book_naming::renamed_name(&name, stem))];
            Some(destination::of(previous, &folder, &plan))
        });
    }

    /// Delete…: the file, or the chapter folder with everything in it. The neighbour the selection
    /// falls to is computed *before* the delete, while the book snapshot still has the row.
    pub fn perform_delete(&self, path: &str) {
        self.perform_managed(|previous| {
            let neighbor = self.book().and_then(|book| book_folder::deletion_neighbor(&book, path));
            if let Err(failure) = book_folder::delete_item_with(path, &*self.files) {
                self.alert(books::COULD_NOT_DELETE, &failure);
                return previous.map(str::to_string);
            }
            let previous = previous?;
            let deleted = book_paths::standardize(path);
            let standing = book_paths::standardize(previous);
            let died = standing == deleted || book_paths::is_inside(&standing, &deleted);
            if died { neighbor } else { Some(previous.to_string()) }
        });
    }

    // ---- separate windows ----

    /// Open `path` in its own document window. When it is the article being edited here, the
    /// session saves and steps aside *first* — there is never a moment with two writers — and a
    /// failed save abandons the open. The mark is what makes the window that lands on the file
    /// exempt from the per-file view-mode memory (md-core's `BookArticleOpens`).
    pub fn open_in_window(&self, path: &str) -> bool {
        if let Some(editing) = self.session.editing_path()
            && file_names::same_path(&editing, path)
            && !self.session.hand_off_for_external_open()
        {
            return false;
        }
        self.opens.borrow_mut().mark(path);
        self.open_requested.emit(path);
        true
    }

    // ---- plumbing ----

    fn alert(&self, title: &str, message: &str) {
        self.alert_requested.emit(&(title.to_string(), message.to_string()));
    }
}

/// The listing's own spelling of `path` when it is in `order`.
fn existing(order: &[BookArticle], path: Option<&str>) -> Option<String> {
    book_folder::order_index(order, path).map(|index| order[index].path.clone())
}
