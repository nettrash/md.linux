//! Port of md.win `Activation/ActivationAction.cs`.

use crate::seams::WindowId;

/// One thing the window manager must do for an activation (shell-design.md §1.2). The router
/// produces an ordered list of these and nothing else — no window is created, no file is read and
/// no registry is consulted while the list is being built, which is what makes every row of the
/// §1.2 table a test.
///
/// Closed: these six are every outcome an activation can have.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ActivationAction {
    /// Open this file as a document; the window manager turns it into [`ActivationAction::Focus`]
    /// when a window already owns it.
    OpenPath(String),
    /// Import a `.textpack` archive as an untitled, read-only document titled after the bundle
    /// (§6.5).
    ImportTextPack(String),
    /// Import a `.textbundle` *folder* the same way (§6.5) — a folder is never a file association,
    /// so it only ever arrives by drop, by a file manager's "Open With", by the command line or by
    /// File ▸ Open TextBundle Folder….
    ImportTextBundleFolder(String),
    /// The file is already open: activate that window instead of opening a second one
    /// (NSDocumentController's rule, §1.2).
    Focus(WindowId),
    /// One new untitled document window — File ▸ New, and what a second plain launch means (the
    /// Mac's Dock-icon ⌘N).
    OpenUntitled,
    /// Reopen the saved documents `session.json` lists, with their modes, Zen state and placement
    /// (§1.6).
    RestoreSession,
}
