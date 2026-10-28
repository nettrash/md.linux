//! The width rule, the open rule and the display rule (md.win `ViewModeRule`, part of
//! `Document/ViewMode.cs`). Pure functions — no view, no storage, no clock.

use super::view_mode::ViewMode;

const NARROW: [ViewMode; 2] = [ViewMode::Edit, ViewMode::Preview];

/// All three when there is room for Split (a desktop window always), else Edit and Preview.
pub fn available_modes(is_wide: bool) -> &'static [ViewMode] {
    if is_wide { &ViewMode::ALL } else { &NARROW }
}

/// The stored preference coerced to one the width supports: Split collapses to Edit when narrow.
/// Display-only — the caller keeps the raw preference so widening brings Split back.
pub fn effective_mode(stored: ViewMode, is_wide: bool) -> ViewMode {
    if available_modes(is_wide).contains(&stored) {
        return stored;
    }
    if stored == ViewMode::Preview { ViewMode::Preview } else { ViewMode::Edit }
}

/// What the window shows: the navigation nudge when one is in force, else the preference — then
/// coerced by [`effective_mode`]. The nudge is plain view state and is never stored.
pub fn displayed_mode(preferred: ViewMode, navigation: Option<ViewMode>, is_wide: bool) -> ViewMode {
    effective_mode(navigation.unwrap_or(preferred), is_wide)
}

/// The transient mode a jump needs so its destination pane is visible, or `None` when it already
/// is. Split shows both panes, so it never nudges. Asked about the DISPLAYED mode, never the raw
/// one: on a narrow window a raw Split shows as Edit, and the raw mode would answer "no nudge
/// needed".
pub fn navigation_nudge(displayed: ViewMode, wants: ViewMode) -> Option<ViewMode> {
    if displayed == wants || displayed == ViewMode::Split { None } else { Some(wants) }
}

/// The mode a document opens in: remembered (raw, uncoerced) → Edit for an empty document → Split
/// where Split is offered → Preview. `has_file_identity` never selects a branch; it is in the
/// shared signature so every suite can assert it stays that way.
pub fn open_view_mode(
    remembered: Option<ViewMode>,
    is_empty_document: bool,
    has_file_identity: bool,
    is_wide: bool,
) -> ViewMode {
    let _ = has_file_identity;
    if let Some(known) = remembered {
        return known;
    }
    if is_empty_document {
        return ViewMode::Edit;
    }
    if is_wide { ViewMode::Split } else { ViewMode::Preview }
}
