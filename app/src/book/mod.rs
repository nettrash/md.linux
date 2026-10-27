//! Books (shell-design §8): the library host over md-core's `book_folder`, the one app-wide Book
//! window and its sidebar.
//!
//! | Module | What |
//! | --- | --- |
//! | [`library_host`] | `BookLibraryHost`: the bookmark, new / open / unpack / close |
//! | [`book_window`] | `BookWindow` (`WindowHost`): header toolbar, stages |
//! | [`sidebar`] | `ListBox` rows, context menu, `book.*` actions |
//!
//! Owner: P5.

pub mod book_window;
pub mod library_host;
pub mod sidebar;
