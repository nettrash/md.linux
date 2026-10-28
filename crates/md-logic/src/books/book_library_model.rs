//! Port of md.win `Books/BookLibraryModel.cs`: the testable half of the book library (§8.2) —
//! which folder is the book, and the one naming rule the example-book unpack needs.
//!
//! LINUX DECISION: md.win keeps the grant in a FutureAccessList token and the path in
//! `md.bookBookmark`. A `.deb` app opens any folder its user can, so the path *is* the whole record
//! (settings_keys says the same): there is no token to take or drop.

use std::rc::Rc;

use crate::settings::{SettingsStore, settings_keys};

/// The open book as a setting. `md.bookBookmark` holds the folder path; a non-empty value means "a
/// book is open", and every window watches the key, so opening or closing a book in one window
/// re-lists the other.
pub struct BookLibraryModel {
    settings: Rc<dyn SettingsStore>,
}

impl std::fmt::Debug for BookLibraryModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BookLibraryModel").field("book_path", &self.book_path()).finish()
    }
}

impl BookLibraryModel {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Self {
        BookLibraryModel { settings }
    }

    /// The open book's folder path, or `None` when no book is open.
    pub fn book_path(&self) -> Option<String> {
        self.settings.get_string(settings_keys::BOOK_BOOKMARK).filter(|stored| !stored.is_empty())
    }

    pub fn has_book(&self) -> bool {
        self.book_path().is_some()
    }

    /// Remember `path` as the open book.
    pub fn store(&self, path: &str) {
        self.settings.set_string(settings_keys::BOOK_BOOKMARK, path);
    }

    /// Close Book: forget the path. The folder is untouched.
    pub fn close_book(&self) {
        self.settings.remove(settings_keys::BOOK_BOOKMARK);
    }

    /// The folder name to unpack the example book under: `base_name`, then `"{base_name} 2"`,
    /// `"{base_name} 3"`, … until one is free. A folder dialog cannot ask "Replace?", so an existing
    /// Example Book is never written over — Kotlin does exactly this. `exists` says whether a folder
    /// or file of that name is already there; `None` when `max_attempts` names were all taken (the
    /// caller then asks for another folder). md.win's default is 100 attempts
    /// ([`BookLibraryModel::DEFAULT_MAX_ATTEMPTS`]).
    pub fn deduped_name(base_name: &str, exists: impl Fn(&str) -> bool, max_attempts: usize) -> Option<String> {
        (1..=max_attempts)
            .map(|attempt| if attempt == 1 { base_name.to_string() } else { format!("{base_name} {attempt}") })
            .find(|candidate| !exists(candidate))
    }

    /// md.win's default for `maxAttempts`.
    pub const DEFAULT_MAX_ATTEMPTS: usize = 100;
}
