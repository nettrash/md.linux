//! Port of md.win `tests/Md.App.Logic.Tests/AppSurfaceTests.cs`. md.win's suite pins the WinUI
//! controls' *source* (`src/Md.App/Controls/EditorPane.cs`, `ZenControls.cs`, `FindBar.xaml.cs`,
//! `FooterBar.xaml.cs` and their XAML) against shell-design §3, §5.4, §5.5 and §10. None of that
//! source exists here — the GTK app gets its own surface tests — so every source pin is ported as an
//! ignored test naming what it checked, and the assertions on md-logic's own strings are ported and
//! run.

use md_logic::strings::{self, find, zen};

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_editor_is_lucida_sans_typewriter_at_twenty_effective_pixels_on_a_sixteen_pixel_inset() {
    // md.win: EditorPane's FontFamily / FontSize = 20 / Padding = 16 (§3.1, §10). The GTK editor's
    // face is Nimbus Mono PS at 15 pt (docs/linux-facts.md §7).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_editor_turns_off_both_auto_correct_sources_win_ui_has() {
    // md.win: IsSpellCheckEnabled / IsTextPredictionEnabled = false.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_editor_wraps_accepts_return_and_scrolls_vertically_only() {
    // md.win: AcceptsReturn, TextWrapping.Wrap, no horizontal scroll bar.
}

/// md.win also pins `_box.BorderThickness = 0`, the placeholder assignment and its brush in
/// EditorPane's source (WinUI only); the string is md-logic's and is ported.
#[test]
fn the_editor_has_no_border_and_takes_its_placeholder_from_strings() {
    assert_eq!(strings::EDITOR_PLACEHOLDER, "# Start writing\u{2026}"); // U+2026, one character
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn tab_inserts_a_tab_because_there_is_no_accepts_tab_in_win_ui() {
    // md.win: the PreviewKeyDown Tab handler (GtkTextView inserts a tab itself).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_selection_is_painted_whether_or_not_the_editor_has_focus() {
    // md.win: SelectionHighlightColor and …WhenNotFocused.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_editor_routes_enter_and_the_letter_through_the_hooks_on_the_tab_keys_undo_path() {
    // md.win: EditorPane wires TypingHooks.Enter / BeforeTextChanging / TextChanging.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn a_settings_change_is_read_back_on_the_editors_own_thread() {
    // md.win: Settings.Changed → DispatcherQueue.TryEnqueue(ReadTypingSettings).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_pane_holds_no_typing_state_of_its_own() {
    // md.win: EditorPane keeps no capital / plan fields beside its TypingHooks.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn set_text_asks_the_hooks_before_it_assigns_so_the_echo_leaves_the_override_armed() {
    // md.win: SetText calls ExternalText before Replacing(… Text = …).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn reset_history_tells_the_hooks_before_it_clears_the_controls_history() {
    // md.win: ResetHistory calls _hooks.Reset() before ClearUndoRedoHistory().
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn enter_is_decided_in_preview_key_down_before_the_control_inserts_its_newline() {
    // md.win: the Enter hook runs in PreviewKeyDown.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn backspace_and_delete_are_announced_from_preview_key_down() {
    // md.win: AnnounceDeletion(TypingHooks.DeletionKey(e.Key)) in PreviewKeyDown.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn undo_redo_and_paste_announce_themselves_before_the_control_acts() {
    // md.win: EditorPane.Undo / Redo / Paste announce, then call the box.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_scroller_is_found_by_template_part_name_with_a_first_scroll_viewer_fallback() {
    // md.win: the TextBox's ContentElement ScrollViewer lookup.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_capsule_carries_the_three_segoe_fluent_glyphs_section_10_maps() {
    // md.win: the Write / Read / Exit glyph code points in ZenControls.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_capsule_geometry_is_the_macs() {
    // md.win: spacing 2, corner radius 20, padding 6, top margin 10, the separator and insets.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_capsule_fades_over_three_hundred_milliseconds_and_stops_hit_testing_when_hidden() {
    // md.win: the 300 ms ScalarTransition and IsHitTestVisible = shown.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_capsule_tint_is_acrylic_over_paper_secondary_at_eighty_percent_with_a_fallback() {
    // md.win: AcrylicBrush { TintOpacity = 0.8 } over Palette.PaperSecondary.
}

/// md.win also pins that ZenControls and FindBar read these from `Strings` (WinUI only); the words
/// are md-logic's and are ported.
#[test]
fn the_capsules_tooltips_and_the_find_bars_labels_are_the_macs_words() {
    assert_eq!(zen::WRITE, "Write");
    assert_eq!(zen::READ, "Read");
    assert_eq!(zen::EXIT, "Exit Zen Mode");
    assert_eq!(find::NEXT, "Next");
    assert_eq!(find::PREVIOUS, "Previous");
    assert_eq!(find::DONE, "Done");
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_selected_switch_is_accent_and_the_other_is_secondary_ink() {
    // md.win: the capsule's icon foregrounds follow ZenReading.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_footer_is_eleven_point_right_aligned_with_tabular_figures() {
    // md.win: FooterEpx = 14.7, tabular numerals, padding, Strings.Footer in FooterBar.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_find_bar_is_thirteen_point_and_answers_enter_and_escape() {
    // md.win: SmallEpx = 17.3 and the query box's Enter / Shift+Enter / Escape.
}

/// md.win also pins the replace half's wiring in FindBar's source (WinUI only); the four words are
/// md-logic's and are ported.
#[test]
fn the_find_bars_replace_half_is_labelled_from_strings_and_wired_in_the_code_behind() {
    assert_eq!(find::REPLACE, "Replace");
    assert_eq!(find::REPLACE_ALL, "Replace All");
    assert_eq!(find::QUERY_PLACEHOLDER, "Find");
    assert_eq!(find::REPLACEMENT_PLACEHOLDER, "Replace with");
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn only_replace_keeps_focus_in_the_replacement_box() {
    // md.win: Replace() refocuses ReplaceBox, ReplaceAll() does not.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn replace_goes_in_through_the_selection_as_one_undo_unit_and_is_not_typing() {
    // md.win: ReplaceRange calls _hooks.ExternalEdit before SelectedText = replacement.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_xaml_is_structure_and_names_only_article_panes() {
    // md.win (theory row ArticlePanes.xaml): no bindings, styles or handlers in the XAML.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_xaml_is_structure_and_names_only_find_bar() {
    // md.win (theory row FindBar.xaml).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_xaml_is_structure_and_names_only_footer_bar() {
    // md.win (theory row FooterBar.xaml).
}
