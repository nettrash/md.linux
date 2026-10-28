//! Port of md.win `Books/BookStepper.cs`: Previous / Next Article (§8.4–§8.5, book.md §13.5),
//! walking the whole book in reading order, root articles first, then each chapter's. Pure — a
//! snapshot of the order and where the selection sits in it; the caller applies the answer as a
//! selection change.

use md_core::book::book_folder;
use md_core::book::book_model::{self, Book};

/// The two toolbar chevrons and Alt+Up / Alt+Down (the GNOME-safe chords the command table
/// assigns), as a value. Nothing selected yet (the empty pane, or a book just opened) is a real
/// state and not a dead end: Next enters the book from the front and Previous from the back. While
/// "Open Articles in Separate Windows" is on there is no persistent selection to step from, so both
/// directions are dead — Swift returns an empty stepper there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct BookStepper {
    /// Every article path in reading order.
    pub order: Vec<String>,
    /// Where the selection sits in `order`, or `None` when nothing in the order is selected.
    pub index: Option<usize>,
}

impl BookStepper {
    /// No book, or articles that open in their own windows: both directions dead.
    pub fn disabled() -> Self {
        BookStepper::default()
    }

    /// The stepper for a book snapshot and the current selection (md.win's `For`).
    pub fn for_book(book: Option<&Book>, selection: Option<&str>, opens_in_separate_windows: bool) -> Self {
        let Some(book) = book.filter(|_| !opens_in_separate_windows) else { return Self::disabled() };
        let articles = book_model::reading_order(book);
        let index = book_folder::order_index(&articles, selection);
        BookStepper { order: articles.into_iter().map(|article| article.path).collect(), index }
    }

    /// Swift's `index.map { $0 > 0 } ?? !order.isEmpty`: at the front there is nowhere back; with no
    /// selection there is the back of the book.
    pub fn can_previous(&self) -> bool {
        match self.index {
            Some(index) => index > 0,
            None => !self.order.is_empty(),
        }
    }

    pub fn can_next(&self) -> bool {
        match self.index {
            Some(index) => index + 1 < self.order.len(),
            None => !self.order.is_empty(),
        }
    }

    /// The article to select, or `None` when the step is not possible (the caller then does
    /// nothing at all).
    pub fn previous(&self) -> Option<&str> {
        self.step(false)
    }

    pub fn next(&self) -> Option<&str> {
        self.step(true)
    }

    fn step(&self, forward: bool) -> Option<&str> {
        // Entering from the end the step comes from: Next starts at the first article, Previous at
        // the last — so one keypress with nothing selected puts the writer somewhere sensible.
        let target = match self.index {
            None if forward => 0,
            None => self.order.len().checked_sub(1)?,
            Some(index) if forward => index + 1,
            Some(index) => index.checked_sub(1)?,
        };
        self.order.get(target).map(String::as_str)
    }
}
