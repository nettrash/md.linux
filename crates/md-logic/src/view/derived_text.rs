//! Port of md.win `View/DerivedText.cs`.

use md_core::export::diagram_svg::Diagram;
use md_core::markdown::block::{NoteEntry, OutlineEntry};

/// Everything the window derives from the document text off the typing path (shell-design §5.5):
/// the footer's two numbers and the Go menu's Contents / Notes / Diagram rows. Recomputed by
/// [`DerivedTextScheduler`](super::DerivedTextScheduler) on a 250 ms trailing debounce, never inside
/// a keystroke.
///
/// Equality is structural, element by element — md.win writes it by hand because a C# record
/// compares its list members by reference, which made every 250 ms tick look like a change; the
/// derived `PartialEq` on `Vec` is already the structural one, which is what
/// [`DocumentWindowState::set_derived`](super::DocumentWindowState::set_derived)'s "only on a real
/// change" guard needs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct DerivedText {
    pub words: usize,
    /// Extended grapheme clusters (md-core `writing_stats::characters`), Swift's `text.count`.
    pub characters: usize,
    pub outline: Vec<OutlineEntry>,
    pub notes: Vec<NoteEntry>,
    pub diagrams: Vec<Diagram>,
}

impl DerivedText {
    pub fn new(
        words: usize,
        characters: usize,
        outline: Vec<OutlineEntry>,
        notes: Vec<NoteEntry>,
        diagrams: Vec<Diagram>,
    ) -> Self {
        DerivedText { words, characters, outline, notes, diagrams }
    }

    /// What a window shows before its first computation lands: "0 words · 0 characters", empty
    /// menus.
    pub fn empty() -> Self {
        Self::default()
    }
}
