//! The editor's three display modes (md.win `ViewMode` / `ViewModes`, the first part of
//! `Document/ViewMode.cs`).
//!
//! Port of md.macOS/md/ViewMode.swift (byte-identical between the iPhone and Mac apps, mirrored by
//! md.Android/ui/ViewMode.kt). Which of the three display modes a window offers, which one it
//! shows, and which one a *file* opens in. THE ONE INVARIANT: what gets stored is the RAW
//! preference, never `view_mode_rule::effective_mode`'s output — a narrow window coerces a
//! remembered Split to Edit for display without discarding the preference, and a navigation nudge
//! stores nothing at all. The rules are in [`super::view_mode_rule`], the per-file memory in
//! [`super::view_mode_memory`] and the book exemption in [`super::book_article_opens`].

/// The editor's three display modes, in switch order (Swift `DocumentView.Mode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewMode {
    Edit,
    Split,
    Preview,
}

impl ViewMode {
    /// CaseIterable order — also the View-menu order and the Split-offered order (`ViewModes.All`).
    pub const ALL: [ViewMode; 3] = [ViewMode::Edit, ViewMode::Split, ViewMode::Preview];

    /// Per-window state key (`@SceneStorage("md.viewMode")`). Never the per-file memory.
    pub const WINDOW_STATE_KEY: &'static str = "md.viewMode";

    /// The per-window default: `Mode(rawValue: stored) ?? .split`.
    pub const WINDOW_DEFAULT: ViewMode = ViewMode::Split;

    /// The Swift `rawValue`: what `md.viewMode` / `md.bookViewMode` hold. Spelled the same as the
    /// memory tokens but deliberately a separate function — the tokens are on disk in three apps
    /// and must not follow an enum rename; the raw value is the window's own state.
    pub fn raw_value(self) -> &'static str {
        match self {
            ViewMode::Edit => "edit",
            ViewMode::Split => "split",
            ViewMode::Preview => "preview",
        }
    }

    /// `Mode(rawValue:)`: exact, case-sensitive; anything else (including `None`) is `None` so the
    /// caller falls back to [`ViewMode::WINDOW_DEFAULT`].
    pub fn from_raw_value(raw: Option<&str>) -> Option<ViewMode> {
        match raw {
            Some("edit") => Some(ViewMode::Edit),
            Some("split") => Some(ViewMode::Split),
            Some("preview") => Some(ViewMode::Preview),
            _ => None,
        }
    }

    /// Menu title.
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::Edit => "Edit",
            ViewMode::Split => "Split",
            ViewMode::Preview => "Preview",
        }
    }

    /// The digit behind ⌘1 / ⌘2 / ⌘3 (Ctrl+1/2/3 on Linux).
    pub fn command_key(self) -> &'static str {
        match self {
            ViewMode::Edit => "1",
            ViewMode::Split => "2",
            ViewMode::Preview => "3",
        }
    }
}
