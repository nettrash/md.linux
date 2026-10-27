//! Port of md.win `Preview/EditorJump.cs`.

use super::preview_navigation::JumpId;

/// The editor half of a Contents or Notes pick (§3.3): put the caret at the start of a line and let
/// the text view scroll it into view. Same one-shot shape as
/// [`PreviewNavigation`](super::PreviewNavigation) — a fresh id per request, performed once — so
/// picking the same note twice works.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EditorJump {
    /// Fresh per request.
    pub id: JumpId,
    /// 0-based line index into the editor's text (`GtkTextBuffer::iter_at_line`).
    pub line: usize,
}

impl EditorJump {
    pub fn new(id: JumpId, line: usize) -> Self {
        EditorJump { id, line }
    }
}

/// "Performed once per id", the rule both jump kinds share, kept out of the GTK code so it is
/// testable. The editor pane claims a jump and then does the GTK half (focus, place the cursor,
/// `scroll_to_mark` — one main-loop turn later, since a Notes jump may have just switched Preview
/// → Edit and the pane is not allocated yet); a pane rebuilt by a Split → Edit → Split round-trip
/// must not replay the last request, which is what [`EditorJumpTracker::claim`] prevents.
#[derive(Clone, Debug, Default)]
pub struct EditorJumpTracker {
    last: Option<JumpId>,
}

impl EditorJumpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// True the first time an id is seen, false ever after. No request is never claimed.
    pub fn claim(&mut self, jump: Option<&EditorJump>) -> bool {
        match jump {
            Some(jump) if self.last != Some(jump.id) => {
                self.last = Some(jump.id);
                true
            }
            _ => false,
        }
    }
}
