//! Port of md.win `tests/Md.App.Logic.Tests/SplitLayoutTests.cs`: shell-design §5.3 — the mode
//! picks the layout and Split picks on width alone at 640 (GTK logical pixels here).

use md_core::document::view_mode::ViewMode;
use md_logic::view::{PaneLayout, SplitLayout};

#[test]
fn edit_is_always_editor_only() {
    for width in [0.0, 639.9, 1400.0] {
        assert_eq!(SplitLayout::arrange(ViewMode::Edit, width), PaneLayout::EditorOnly);
    }
}

#[test]
fn preview_is_always_preview_only() {
    for width in [0.0, 639.9, 1400.0] {
        assert_eq!(SplitLayout::arrange(ViewMode::Preview, width), PaneLayout::PreviewOnly);
    }
}

#[test]
fn split_turns_side_by_side_at_six_hundred_and_forty() {
    for (width, expected) in [
        (639.9, PaneLayout::Stacked),
        (640.0, PaneLayout::SideBySide),
        (640.1, PaneLayout::SideBySide),
        (0.0, PaneLayout::Stacked),
    ] {
        assert_eq!(SplitLayout::arrange(ViewMode::Split, width), expected, "{width}");
    }
}

#[test]
fn a_width_that_is_not_yet_known_stacks() {
    assert_eq!(SplitLayout::arrange(ViewMode::Split, f64::NAN), PaneLayout::Stacked);
    assert_eq!(SplitLayout::arrange(ViewMode::Split, -1.0), PaneLayout::Stacked);
}

#[test]
fn only_the_collapsed_pane_is_hidden() {
    assert!(SplitLayout::shows_editor(PaneLayout::EditorOnly));
    assert!(!SplitLayout::shows_preview(PaneLayout::EditorOnly));
    assert!(!SplitLayout::shows_editor(PaneLayout::PreviewOnly));
    assert!(SplitLayout::shows_preview(PaneLayout::PreviewOnly));

    for split in [PaneLayout::SideBySide, PaneLayout::Stacked] {
        assert!(SplitLayout::shows_editor(split));
        assert!(SplitLayout::shows_preview(split));
    }
}

#[test]
fn the_divider_is_drawn_only_between_two_visible_panes() {
    assert!(!SplitLayout::shows_divider(PaneLayout::EditorOnly));
    assert!(!SplitLayout::shows_divider(PaneLayout::PreviewOnly));
    assert!(SplitLayout::shows_divider(PaneLayout::SideBySide));
    assert!(SplitLayout::shows_divider(PaneLayout::Stacked));
}
