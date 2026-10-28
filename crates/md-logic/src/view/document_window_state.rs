//! Port of md.win `View/DocumentWindowState.cs`.

use std::cell::{Cell, RefCell};
use std::fmt;

use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_rule;

use super::derived_text::DerivedText;
use crate::event::Event;
use crate::preview::{EditorJump, JumpId, PreviewNavigation};

/// Whether the per-file memory hook has run for this window, and with which identity — the Swift
/// `lastIdentity: String??` (shell-design §5.1). Three states, and all three matter: never ran (a
/// fresh window), ran on a document with no file (untitled, or an Example), and ran on a file. The
/// middle one is what makes the first Ctrl+S a *migration* instead of a re-decision, so it cannot
/// be flattened into a plain `Option<String>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct IdentityMemo {
    ran: bool,
    value: Option<String>,
}

impl IdentityMemo {
    /// The hook has never run for this window (the default, so a new state starts here).
    pub const NEVER_RAN: IdentityMemo = IdentityMemo { ran: false, value: None };

    pub fn of(identity: Option<String>) -> IdentityMemo {
        IdentityMemo { ran: true, value: identity }
    }

    pub fn ran(&self) -> bool {
        self.ran
    }

    /// The identity the hook last saw; `None` when it ran on a document with no file.
    pub fn value(&self) -> Option<&str> {
        self.value.as_deref()
    }

    /// Swift's `lastIdentity == .some(String?.none)` — the untitled-document case.
    pub fn ran_with_no_identity(&self) -> bool {
        self.ran && self.value.is_none()
    }
}

/// Everything one document window remembers about how it is showing its document (shell-design
/// §5.1): the raw mode preference, Zen, whether the per-file memory hook has run, the sticky
/// book-article exemption, the transient navigation nudge, the two one-shot jump requests and the
/// derived text the footer and the Go menu read.
///
/// Persisted only through `session.json` (§1.6) — never through `md.viewModeMemory`, which is per
/// *file*. [`DocumentWindowState::changed`] fires when any of it actually changes, so the window
/// relays out and the menus re-evaluate without polling. Shared (`Rc`) by the window, its
/// [`ViewModeController`](super::ViewModeController) and its [`ZenController`](super::ZenController);
/// every method takes `&self`, and no borrow is held while `changed` is raised.
pub struct DocumentWindowState {
    stored_mode: Cell<ViewMode>,
    zen_active: Cell<bool>,
    zen_reading: Cell<bool>,
    navigation_mode: Cell<Option<ViewMode>>,
    last_identity: RefCell<IdentityMemo>,
    is_book_article: Cell<bool>,
    preview_navigation: RefCell<Option<PreviewNavigation>>,
    editor_jump: RefCell<Option<EditorJump>>,
    derived: RefCell<DerivedText>,
    changed: Event<()>,
}

impl Default for DocumentWindowState {
    fn default() -> Self {
        DocumentWindowState {
            stored_mode: Cell::new(ViewMode::WINDOW_DEFAULT),
            zen_active: Cell::new(false),
            zen_reading: Cell::new(false),
            navigation_mode: Cell::new(None),
            last_identity: RefCell::new(IdentityMemo::NEVER_RAN),
            is_book_article: Cell::new(false),
            preview_navigation: RefCell::new(None),
            editor_jump: RefCell::new(None),
            derived: RefCell::new(DerivedText::empty()),
            changed: Event::new(),
        }
    }
}

impl fmt::Debug for DocumentWindowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentWindowState")
            .field("stored_mode", &self.stored_mode.get())
            .field("zen_active", &self.zen_active.get())
            .field("zen_reading", &self.zen_reading.get())
            .field("navigation_mode", &self.navigation_mode.get())
            .field("is_book_article", &self.is_book_article.get())
            .finish()
    }
}

impl DocumentWindowState {
    /// A desktop window is always wide (§5.1): every rule is asked with `is_wide: true` and Split
    /// handles a narrow window by stacking its two panes (§5.3), exactly as the Mac does.
    pub const IS_WIDE: bool = true;

    pub fn new() -> Self {
        Self::default()
    }

    /// Raised after any real change.
    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }

    fn set_cell<T: Copy + PartialEq>(&self, cell: &Cell<T>, value: T) {
        if cell.get() == value {
            return;
        }
        cell.set(value);
        self.changed.emit(&());
    }

    fn set_ref<T: PartialEq>(&self, cell: &RefCell<T>, value: T) {
        if *cell.borrow() == value {
            return;
        }
        *cell.borrow_mut() = value;
        self.changed.emit(&());
    }

    /// The RAW preference — never `view_mode_rule::effective_mode`'s output. Default Split.
    pub fn stored_mode(&self) -> ViewMode {
        self.stored_mode.get()
    }

    pub fn set_stored_mode(&self, mode: ViewMode) {
        self.set_cell(&self.stored_mode, mode);
    }

    /// Zen is on for this window. Neither this nor [`DocumentWindowState::zen_reading`] ever
    /// reaches the per-file memory.
    pub fn zen_active(&self) -> bool {
        self.zen_active.get()
    }

    pub fn set_zen_active(&self, active: bool) {
        self.set_cell(&self.zen_active, active);
    }

    /// False = writing (the editor), true = reading (the preview). Zen's own switch, stored
    /// nowhere.
    pub fn zen_reading(&self) -> bool {
        self.zen_reading.get()
    }

    pub fn set_zen_reading(&self, reading: bool) {
        self.set_cell(&self.zen_reading, reading);
    }

    /// The transient nudge a Notes jump sets; cleared by any explicit mode pick and never stored.
    pub fn navigation_mode(&self) -> Option<ViewMode> {
        self.navigation_mode.get()
    }

    pub fn set_navigation_mode(&self, mode: Option<ViewMode>) {
        self.set_cell(&self.navigation_mode, mode);
    }

    /// Set by [`ViewModeController::apply_memory`](super::ViewModeController::apply_memory); read
    /// by its `set_mode`. Not a change anyone redraws for.
    pub fn last_identity(&self) -> IdentityMemo {
        self.last_identity.borrow().clone()
    }

    pub fn set_last_identity(&self, memo: IdentityMemo) {
        *self.last_identity.borrow_mut() = memo;
    }

    /// This window is showing a book article, so the per-file memory neither decides its mode nor
    /// records it. Sticky for the window's life — Save As included (the Mac is the source of
    /// truth; Android clears its flag there and is the odd one out).
    pub fn is_book_article(&self) -> bool {
        self.is_book_article.get()
    }

    /// One-way: the exemption is claimed once and never given back.
    pub fn mark_book_article(&self) {
        if self.is_book_article.get() {
            return;
        }
        self.is_book_article.set(true);
        self.changed.emit(&());
    }

    pub fn preview_navigation(&self) -> Option<PreviewNavigation> {
        self.preview_navigation.borrow().clone()
    }

    pub fn set_preview_navigation(&self, navigation: Option<PreviewNavigation>) {
        self.set_ref(&self.preview_navigation, navigation);
    }

    pub fn editor_jump(&self) -> Option<EditorJump> {
        self.editor_jump.borrow().clone()
    }

    pub fn set_editor_jump(&self, jump: Option<EditorJump>) {
        self.set_ref(&self.editor_jump, jump);
    }

    /// Word/character counts, outline, notes and diagrams — recomputed by
    /// [`DerivedTextScheduler`](super::DerivedTextScheduler).
    pub fn derived(&self) -> DerivedText {
        self.derived.borrow().clone()
    }

    /// Stores the value and raises `changed` only when it differs (structurally) from the stored
    /// one — two runs over the same text are silent.
    pub fn set_derived(&self, derived: DerivedText) {
        self.set_ref(&self.derived, derived);
    }

    /// What the window actually shows: the nudge when one is in force, else the preference.
    pub fn effective_mode(&self) -> ViewMode {
        view_mode_rule::displayed_mode(self.stored_mode(), self.navigation_mode(), Self::IS_WIDE)
    }

    /// Clear the request only when it is still the one that was performed — a newer one may have
    /// replaced it.
    pub fn preview_navigation_handled(&self, id: JumpId) {
        let current = self.preview_navigation.borrow().as_ref().map(|n| n.id);
        if current == Some(id) {
            self.set_preview_navigation(None);
        }
    }

    /// Same id guard as [`DocumentWindowState::preview_navigation_handled`]: a pane recreated by a
    /// mode change would otherwise replay a stale jump.
    pub fn editor_jump_handled(&self, id: JumpId) {
        let current = self.editor_jump.borrow().as_ref().map(|j| j.id);
        if current == Some(id) {
            self.set_editor_jump(None);
        }
    }
}
