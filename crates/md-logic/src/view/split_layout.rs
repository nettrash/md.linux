//! Port of md.win `View/SplitLayout.cs`.

use md_core::document::view_mode::ViewMode;

/// How the two panes are laid out in the content row (shell-design §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneLayout {
    /// Edit: the preview widget is hidden.
    EditorOnly,
    /// Preview: the editor widget is hidden.
    PreviewOnly,
    /// Split with room: two equal columns and a 1 px divider between them (a horizontal
    /// `GtkPaned`).
    SideBySide,
    /// Split without room: editor above preview, equal rows, the same divider (a vertical
    /// `GtkPaned`).
    Stacked,
}

/// The Mac's `content` switch (§5.3, macOS §6.7): the mode picks the layout, and Split picks
/// between side-by-side and stacked on the content width alone. Pure — the GTK code turns the
/// answer into the paned's orientation and its children's visibility.
pub struct SplitLayout;

impl SplitLayout {
    /// 640 of content width, the Mac's 640 pt threshold kept as a number rather than converted:
    /// SwiftUI's points, XAML's effective pixels and GTK's logical (application) pixels are all
    /// the scale-independent unit the layout is written in, so the pane splits at the same
    /// apparent width on every platform.
    pub const SIDE_BY_SIDE_MINIMUM_WIDTH: f64 = 640.0;

    /// `content_width` is the content row's allocated width in GTK logical pixels. NaN or a
    /// negative width (a pass before the first allocation) answers [`PaneLayout::Stacked`] — every
    /// comparison with NaN is false, and stacking is the layout that survives any width.
    pub fn arrange(mode: ViewMode, content_width: f64) -> PaneLayout {
        match mode {
            ViewMode::Edit => PaneLayout::EditorOnly,
            ViewMode::Preview => PaneLayout::PreviewOnly,
            ViewMode::Split if content_width >= Self::SIDE_BY_SIDE_MINIMUM_WIDTH => PaneLayout::SideBySide,
            ViewMode::Split => PaneLayout::Stacked,
        }
    }

    /// The editor widget is visible (Preview hides it).
    pub fn shows_editor(layout: PaneLayout) -> bool {
        layout != PaneLayout::PreviewOnly
    }

    /// The preview widget is visible (Edit hides it — and a hidden preview only records "stale",
    /// §4.5).
    pub fn shows_preview(layout: PaneLayout) -> bool {
        layout != PaneLayout::EditorOnly
    }

    /// The divider is drawn only between two visible panes.
    pub fn shows_divider(layout: PaneLayout) -> bool {
        matches!(layout, PaneLayout::SideBySide | PaneLayout::Stacked)
    }
}
