//! `BookLibraryHost` — the open book (shell-design §8.2): `md.bookBookmark` holds its folder path
//! (no sandbox, so the path is the whole record); New Book…, Open Book…, Example Book…, Close
//! Book; `changed` when the book opens, closes or becomes (in)accessible.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P5

use std::rc::Rc;

use md_logic::event::Event;
use md_logic::exec::{LocalBoxFuture, ready};
use md_logic::settings::SettingsStore;

use crate::windows::host::WindowHost;

/// The app's one book, if any.
pub struct BookLibraryHost {
    settings: Rc<dyn SettingsStore>,
    changed: Event<()>,
}

impl BookLibraryHost {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Rc<BookLibraryHost> {
        Rc::new(BookLibraryHost { settings, changed: Event::new() })
    }

    pub fn has_book(&self) -> bool {
        false
    }

    /// The book folder's path.
    pub fn folder(&self) -> Option<String> {
        None
    }

    /// Folder dialog + name prompt → a new book folder; true when a book is now open.
    pub fn new_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        ready(false)
    }

    pub fn open_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        ready(false)
    }

    /// Copies the bundled `Example Book` into a folder the user picks and opens it.
    pub fn unpack_example_book(&self, parent: &dyn WindowHost) -> LocalBoxFuture<'static, bool> {
        ready(false)
    }

    /// Removes `md.bookBookmark`; the folder is untouched.
    pub fn close_book(&self) {}

    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }
}
