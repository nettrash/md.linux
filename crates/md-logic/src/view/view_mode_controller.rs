//! Port of md.win `View/ViewModeController.cs`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use md_core::document::book_article_opens::BookArticleOpens;
use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_memory::{self, ViewModeStore};
use md_core::document::view_mode_rule;
use md_core::markdown::block::{NoteEntry, OutlineEntry};

use super::document_window_state::{DocumentWindowState, IdentityMemo};
use crate::preview::{EditorJump, JumpId, PreviewNavigation};

/// The Mac's `setMode` / `applyViewModeMemory` and the Contents / Notes jumps (shell-design §5.2,
/// §5.6; macOS §6.4, §6.5, §6.9), line for line. The single writer of a mode preference and the
/// only caller of `view_mode_memory::remember`.
///
/// The identity of the open file is `view_mode_memory::identity_for(path)` — md-core's own
/// canonicalisation (absolute, `.`/`..` collapsed, links resolved; case kept, as Linux file
/// systems compare names byte for byte). One function, because `BookArticleOpens::mark` /
/// `claim_open` take a path and hash it with exactly this one: canonicalising the two halves
/// differently would silently break the book exemption whenever the navigator and the window
/// spelled the same path differently. [`FileIdentity`](crate::seams::FileIdentity) stays what it is
/// for: which window owns which file.
///
/// LINUX DECISION: md.win's `BookArticleOpens` is a process-wide static; md-core's is a value the
/// app owns ONE of, shared here as `Rc<RefCell<BookArticleOpens>>` by the book navigator that marks
/// and every document window's controller that claims.
pub struct ViewModeController {
    state: Rc<DocumentWindowState>,
    memory: Rc<dyn ViewModeStore>,
    book_opens: Rc<RefCell<BookArticleOpens>>,
    new_id: Box<dyn Fn() -> JumpId>,
    /// The identity of the file the window is on — Swift's `fileURL.map(identity)`, cached rather
    /// than re-hashed on every menu pick. That makes [`ViewModeController::apply_memory`]'s
    /// contract load-bearing: it must be called on **every** change of the document's file, which
    /// is exactly when the Mac's `.onChange(of: fileURL)` fires. Skip one and
    /// [`ViewModeController::set_mode`] would remember under the previous file.
    identity: RefCell<Option<String>>,
}

impl fmt::Debug for ViewModeController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewModeController").field("identity", &self.identity.borrow()).finish()
    }
}

impl ViewModeController {
    pub fn new(
        state: Rc<DocumentWindowState>,
        memory: Rc<dyn ViewModeStore>,
        book_opens: Rc<RefCell<BookArticleOpens>>,
    ) -> Self {
        Self::with_ids(state, memory, book_opens, JumpId::next)
    }

    /// `new_id` stamps each jump request — injectable so a test can pin "performed once per id"
    /// and "a newer request replaces the older" without guessing ids.
    pub fn with_ids(
        state: Rc<DocumentWindowState>,
        memory: Rc<dyn ViewModeStore>,
        book_opens: Rc<RefCell<BookArticleOpens>>,
        new_id: impl Fn() -> JumpId + 'static,
    ) -> Self {
        ViewModeController { state, memory, book_opens, new_id: Box::new(new_id), identity: RefCell::new(None) }
    }

    /// The View menu and Ctrl+1/2/3. In Zen the three commands drive Zen's own write/read switch
    /// instead and store **nothing** — not the mode, not the memory (§5.4).
    pub fn select(&self, mode: ViewMode) {
        if self.state.zen_active() {
            self.state.set_zen_reading(mode == ViewMode::Preview);
            return;
        }
        self.set_mode(mode);
    }

    /// The single writer of a preference: drop any nudge, store the RAW mode, and remember it for
    /// this file unless the window is a book article. Called by [`ViewModeController::select`] and
    /// by [`ViewModeController::apply_memory`]'s open-rule branch; never by Zen and never by a
    /// nudge.
    pub fn set_mode(&self, mode: ViewMode) {
        self.state.set_navigation_mode(None);
        self.state.set_stored_mode(mode);
        if self.state.is_book_article() {
            return;
        }
        let identity = self.identity.borrow().clone();
        if let Some(identity) = identity {
            view_mode_memory::remember(mode, &identity, self.memory.as_ref());
        }
    }

    /// Runs on window appearance and on every identity change (Save of an untitled document, Save
    /// As, Rename, Move To) — the Mac's `.onChange(of: fileURL, initial: true)`.
    ///
    /// `path` is the document's file, or `None` while it has none (untitled, or an Example).
    /// `text` is the document text, for the open rule's `is_empty_document` — the *text*, never
    /// "has no file": an Example has content and no file and must open in Split.
    pub fn apply_memory(&self, path: Option<&str>, text: &str) {
        let identity = path.map(view_mode_memory::identity_for);
        *self.identity.borrow_mut() = identity.clone();

        // Claimed BEFORE the early return: the mark is consumed on the first appearance of the
        // window that the book opened, whatever the window decides afterwards.
        if let Some(path) = path {
            let claimed = self.book_opens.borrow_mut().claim_open(path);
            if claimed {
                self.state.mark_book_article();
            }
        }
        if self.state.is_book_article() {
            self.state.set_last_identity(IdentityMemo::of(identity));
            return;
        }

        // Same file as last time (a Save over the same path): nothing to decide.
        let last = self.state.last_identity();
        if last.ran() && last.value() == identity.as_deref() {
            return;
        }

        let had_no_identity = last.ran_with_no_identity();
        self.state.set_last_identity(IdentityMemo::of(identity.clone()));

        // First save of an untitled document: MIGRATE the mode the writer is already in to the new
        // identity, raw and uncoerced. Re-deciding here would drop them into Split mid-sentence.
        if let (true, Some(identity)) = (had_no_identity, identity.as_deref()) {
            view_mode_memory::remember(self.state.stored_mode(), identity, self.memory.as_ref());
            return;
        }

        // A different document (including Save As / Rename / Move To of a saved one): drop any
        // nudge and re-run the open rule for the new identity.
        self.state.set_navigation_mode(None);
        let remembered = identity.as_deref().and_then(|id| view_mode_memory::lookup(id, self.memory.as_ref()));
        self.set_mode(view_mode_rule::open_view_mode(
            remembered,
            text.is_empty(),
            identity.is_some(),
            DocumentWindowState::IS_WIDE,
        ));
    }

    /// Contents: Split moves both panes, Edit only the caret, Preview only the preview — a reader
    /// is never dropped into the source, so a heading NEVER nudges (§5.6).
    pub fn jump_to_heading(&self, entry: &OutlineEntry) {
        let displayed = self.state.effective_mode();
        if displayed != ViewMode::Edit {
            self.state.set_preview_navigation(Some(PreviewNavigation::new((self.new_id)(), entry.slug.clone())));
        }
        if displayed != ViewMode::Preview {
            self.state.set_editor_jump(Some(EditorJump::new((self.new_id)(), entry.line)));
        }
    }

    /// Notes: a note never renders, so the editor must be visible — nudge Preview → Edit (never
    /// from Split or Edit). The nudge is assigned only when there is one: writing `None` back would
    /// cancel the nudge a previous note jump set.
    pub fn jump_to_note(&self, note: &NoteEntry) {
        if let Some(nudge) = view_mode_rule::navigation_nudge(self.state.effective_mode(), ViewMode::Edit) {
            self.state.set_navigation_mode(Some(nudge));
        }
        self.state.set_editor_jump(Some(EditorJump::new((self.new_id)(), note.line)));
    }
}
