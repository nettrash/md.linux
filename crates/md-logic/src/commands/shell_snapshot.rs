//! Port of md.win `Commands/ShellSnapshot.cs`: what a window publishes about itself so the menus
//! can be enabled, ticked and filled.

use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_core::markdown::block::{NoteEntry, OutlineEntry};

use crate::seams::WindowId;

/// One row of File ▸ Open Recent (§2.2): the token that reopens the file, the file name the row
/// shows, and the folder the row's tooltip shows. On Linux the token is whatever the shell's
/// recent-files store hands back to reopen the entry (a `GtkRecentManager` URI).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecentEntry {
    pub token: String,
    pub name: String,
    pub folder: String,
}

impl RecentEntry {
    pub fn new(token: impl Into<String>, name: impl Into<String>, folder: impl Into<String>) -> Self {
        RecentEntry { token: token.into(), name: name.into(), folder: folder.into() }
    }
}

/// One row of File ▸ Export ▸ Diagram as SVG (§2.2): the 0-based ordinal that identifies the
/// diagram and the title the row shows — the two fields of md-core's
/// [`Diagram`](md_core::export::diagram_svg::Diagram) that reach the menu. The producer maps
/// `diagram_svg::diagrams(text)` → `DiagramRef::new(d.ordinal, d.menu_title)`, and dispatch carries
/// the ordinal back so the export can re-resolve the real diagram from the source.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiagramRef {
    pub ordinal: usize,
    pub menu_title: String,
}

impl DiagramRef {
    pub fn new(ordinal: usize, menu_title: impl Into<String>) -> Self {
        DiagramRef { ordinal, menu_title: menu_title.into() }
    }
}

/// One row of the Window menu: a window, its title, and whether it is the window publishing the
/// snapshot (md.win's `(Guid Id, string Title, bool IsThis)`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct WindowTitle {
    pub id: WindowId,
    pub title: String,
    pub is_this: bool,
}

impl WindowTitle {
    pub fn new(id: WindowId, title: impl Into<String>, is_this: bool) -> Self {
        WindowTitle { id, title: title.into(), is_this }
    }
}

/// What a window publishes about itself so the menus can be enabled, ticked and filled — the
/// desktop shape of the Mac's five focused-scene values (§2.1, §2.9). One snapshot per window; the
/// menu bar and the accelerators of that window read this one and nothing else.
///
/// Equality is structural over the five lists as well as the scalars, because the Mac's focused
/// values compare their outline and notes by value and that comparison is what decides when the Go
/// and Diagram submenus rebuild (§2.9): a recomputed but identical outline must not rebuild an open
/// menu. md.win writes that `Equals` by hand; here the derived `PartialEq` already compares a
/// `Vec` element by element, and a field added to the struct is part of it automatically.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ShellSnapshot {
    /// The window publishes an active document. Document windows always; the Book window never (it
    /// publishes `is_editing_article` instead).
    pub has_document: bool,
    /// This is the one app-wide Book window.
    pub is_book_window: bool,
    /// Book window only: the detail session is in its Editing stage, so the article is a document
    /// for the File menu's purposes.
    pub is_editing_article: bool,
    /// The document has a path (Rename / Move To / Revert).
    pub is_saved: bool,
    /// The document differs from its last explicit save or open.
    pub is_dirty: bool,
    /// A book is open — `md.bookBookmark` is non-empty.
    pub has_book: bool,
    /// The mode the window shows *outside* Zen; the View ticks apply the Zen rule of §2.5 on top.
    pub displayed_mode: ViewMode,
    /// Zen is on (document windows only).
    pub zen_active: bool,
    /// Zen is showing the preview rather than the editor.
    pub zen_reading: bool,
    /// Go ▸ Contents' rows, in document order.
    pub outline: Vec<OutlineEntry>,
    /// Go ▸ Notes' rows, in document order.
    pub notes: Vec<NoteEntry>,
    /// File ▸ Export ▸ Diagram as SVG's rows; arrives with the 250 ms derived-text tick (§5.5).
    pub diagrams: Vec<DiagramRef>,
    /// Book window only: the article stepper can step back.
    pub can_previous: bool,
    /// Book window only: the article stepper can step forward.
    pub can_next: bool,
    /// An editor pane is on screen (Edit, Split, or Zen writing) — the Edit menu's gate.
    pub editor_visible: bool,
    /// The editor's own undo stack is non-empty (`GtkTextBuffer:can-undo`).
    pub can_undo: bool,
    /// The editor's own redo stack is non-empty (`GtkTextBuffer:can-redo`).
    pub can_redo: bool,
    /// File ▸ Open Recent's rows, newest first.
    pub recent_entries: Vec<RecentEntry>,
    /// The Window menu's rows: every open window, and which one is this one.
    pub window_titles: Vec<WindowTitle>,
    /// The editor has a non-empty selection (Use Selection for Find).
    pub has_selection: bool,
    /// The find bar holds a query (Find Next / Find Previous).
    pub has_find_query: bool,
    /// The `md.pdfPageSize` value; ticks one PDF Page Size radio.
    pub pdf_page_size_id: String,
    /// Book window only: the sidebar is showing (View ▸ Show Sidebar's tick).
    pub sidebar_open: bool,
    /// The window is full screen (`GtkWindow:fullscreened`) — View's row reads "Exit Full Screen".
    pub is_full_screen: bool,
    /// (Linux) An alert is up in this window. `adw::AlertDialog` is presented *inside* the window,
    /// so the window's shortcuts would still reach the window behind it; while this is set every
    /// [`ActionScope::Window`](super::ActionScope::Window) command is disabled, as the Mac's sheet
    /// blocks the window's menu items (shell-design §2.2, §7.8). The application's own commands
    /// stay live.
    pub dialog_open: bool,
    /// The find bar is showing (a document window's or the Book window's); with `zen_active` it is
    /// what makes Esc a command (§2.9).
    pub find_bar_open: bool,
    /// The `md.continueLists` setting (Edit ▸ Typing ▸ Continue Lists and Tables' tick). Default
    /// true.
    pub continue_lists: bool,
    /// The `md.capitalizeSentences` setting (Edit ▸ Typing ▸ Capitalize Sentences' tick). Default
    /// true.
    pub capitalize_sentences: bool,
    /// The `md.wrapSelection` setting (Edit ▸ Typing ▸ Wrap Selection When Typing's tick). Default
    /// true.
    pub wrap_selection: bool,
}

impl ShellSnapshot {
    /// Nothing published: no document, no book, no rows. The base every window's snapshot is built
    /// from with struct-update syntax, and what a window reports before its content exists — every
    /// command that needs a document is disabled against it, and New / Open / Show Book still work.
    pub fn empty() -> ShellSnapshot {
        ShellSnapshot {
            has_document: false,
            is_book_window: false,
            is_editing_article: false,
            is_saved: false,
            is_dirty: false,
            has_book: false,
            displayed_mode: ViewMode::WINDOW_DEFAULT,
            zen_active: false,
            zen_reading: false,
            outline: Vec::new(),
            notes: Vec::new(),
            diagrams: Vec::new(),
            can_previous: false,
            can_next: false,
            editor_visible: false,
            can_undo: false,
            can_redo: false,
            recent_entries: Vec::new(),
            window_titles: Vec::new(),
            has_selection: false,
            has_find_query: false,
            pdf_page_size_id: PageSize::DEFAULT_ID.to_string(),
            sidebar_open: false,
            is_full_screen: false,
            dialog_open: false,
            find_bar_open: false,
            continue_lists: true,
            capitalize_sentences: true,
            wrap_selection: true,
        }
    }

    /// The mode the three View ticks read: outside Zen the displayed mode; in Zen the Mac's
    /// published mode, `zenReading ? .preview : .edit` — which is why Ctrl+1 and Ctrl+2 both mean
    /// "write", Ctrl+3 means "read", and Split is never ticked in Zen (§2.5).
    pub fn published_mode(&self) -> ViewMode {
        if self.zen_active {
            if self.zen_reading { ViewMode::Preview } else { ViewMode::Edit }
        } else {
            self.displayed_mode
        }
    }
}

impl Default for ShellSnapshot {
    fn default() -> Self {
        ShellSnapshot::empty()
    }
}
