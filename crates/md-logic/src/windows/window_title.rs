//! Port of md.win `Windows/WindowTitle.cs` (`WindowTitle` and `UntitledNames`).
//!
//! What a window is called (shell-design.md §6.7, §1.3). The Mac's title is the document's display
//! name with an "— Edited" proxy while the buffer differs from the last *explicit* save; an
//! autosave never clears it, only a Save does — which is why the flag the caller passes must be
//! `TextFileSession::is_dirty` and never `has_unsaved_changes`. In GTK the string goes to the
//! window's title (`adw::WindowTitle` in the header bar), which is also what the Window menu and
//! the desktop's window list show.

use std::collections::BTreeSet;

use crate::strings;

/// " — Edited" (U+2014), the Mac's own suffix.
pub const EDITED_SUFFIX: &str = strings::EDITED_SUFFIX;

/// A document window: `"{name}"` or `"{name} — Edited"`. (md.win's `WindowTitle.For`.)
pub fn for_document(display_name: &str, is_dirty: bool) -> String {
    if is_dirty { format!("{display_name}{EDITED_SUFFIX}") } else { display_name.to_string() }
}

/// The Book window (§1.3): the book's folder name, or "Book" when none is open, plus
/// `" — {article}"` while an article is being edited. The Book window carries no Edited proxy — a
/// book article's save is its save.
pub fn for_book(book_name: Option<&str>, article_title: Option<&str>) -> String {
    let name = book_name.filter(|n| !n.is_empty()).unwrap_or(strings::BOOK);
    match article_title.filter(|t| !t.is_empty()) {
        Some(article) => format!("{name} {} {article}", strings::EM_DASH),
        None => name.to_string(),
    }
}

/// "Untitled", "Untitled 2", … — the display name, before the Edited suffix.
pub fn untitled(ordinal: usize) -> String {
    strings::untitled_numbered(ordinal)
}

/// The per-process numbering of untitled windows (§6.7). The lowest free ordinal is handed out, so
/// closing "Untitled 2" and choosing File ▸ New gives "Untitled 2" again — NSDocumentController's
/// behaviour, and the reason this is a live set rather than a counter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UntitledNames {
    taken: BTreeSet<usize>,
}

impl UntitledNames {
    pub fn new() -> Self {
        Self::default()
    }

    /// The lowest ordinal not in use. 1 is "Untitled".
    pub fn take(&mut self) -> usize {
        let mut ordinal = 1;
        while !self.taken.insert(ordinal) {
            ordinal += 1;
        }
        ordinal
    }

    /// The window closed (or was saved and is no longer untitled): its number is free again.
    pub fn release(&mut self, ordinal: usize) {
        self.taken.remove(&ordinal);
    }

    /// [`UntitledNames::take`] and the name in one step: (name, ordinal).
    pub fn take_name(&mut self) -> (String, usize) {
        let ordinal = self.take();
        (untitled(ordinal), ordinal)
    }
}
