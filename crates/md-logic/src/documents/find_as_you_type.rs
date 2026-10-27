//! Port of md.win `Documents/FindAsYouType.cs`.

use super::text_search::{self, Match};

/// Find as you type (2026-09-27, the find panel): every change to the query searches again, from
/// the **anchor** — where the caret stood when the writer went to the query box, or the start of
/// the hit the last step landed on. Searching from the hit's start, not its end, is what lets a
/// longer query keep the hit it already has ("ca" on the third `cat`, then "cat", stays on the
/// third) — the way Notepad, Edge and VS Code refine. The matching is [`text_search`]'s one rule;
/// this type only remembers where to start.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FindAsYouType {
    anchor: usize,
}

impl FindAsYouType {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the next as-you-type search starts, in UTF-16 units.
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// The query box got focus: the search starts at the editor's selection as it stands.
    pub fn focused(&mut self, selection_start: i64) {
        self.anchor = usize::try_from(selection_start.max(0)).unwrap_or(usize::MAX);
    }

    /// Next, Previous or Replace landed on `hit`: typing on refines from there.
    pub fn stepped(&mut self, hit: Match) {
        self.anchor = hit.index;
    }

    /// The query changed: the first hit at or after the anchor, wrapping; `None` for an empty query
    /// or none.
    pub fn search(&self, text: &str, query: &str) -> Option<Match> {
        text_search::next(text, query, i64::try_from(self.anchor).unwrap_or(i64::MAX))
    }
}
