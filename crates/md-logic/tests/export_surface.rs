//! Port of md.win `tests/Md.App.Logic.Tests/Export/ExportSurfaceTests.cs`. md.win's suite pins the
//! WinUI half of the export path — `Web/ExportRenderer.cs`, `Web/ExportHostWindow.cs`,
//! `Controls/PrintOverlay.xaml(.cs)`, `Export/ShareBridge.cs` — by searching their *source*. None of
//! that source exists here: the GTK renderer is the app's, with the behaviour `md_logic::export`'s
//! module documentation spells out. So every source pin is ported as an ignored test naming what it
//! checked, and every assertion on md-logic's own data is ported and runs.

use md_logic::export::RichSnapshotter;
use md_logic::preview::scripts;
use md_logic::strings::{buttons, exports};
use md_logic::testing::PrintGeometry;

// ── §7.1 the export renderer ──────────────────────────────────────────────────────────────

/// md.win also pins `PageWidthCssPx = 595`, `PageHeightCssPx = 842`, `OffCanvasLeft = -10000` and
/// their assignments in ExportRenderer.cs (WinUI only; the GTK renderer is an invisible overlay
/// child sized 595 × 842 CSS px, docs/linux-facts.md §6). The md-logic half agrees about the height.
#[test]
fn the_export_renderer_is_exactly_one_page_wide_and_parked_off_every_monitor() {
    assert_eq!(RichSnapshotter::MINIMUM_HEIGHT_CSS_PX, 842.0);
}

#[test]
#[ignore = "checks the WinUI app (WebView2 Visibility.Visible); the GTK overlay renderer is mapped at opacity 0"]
fn the_off_canvas_renderer_is_visible_because_chromium_throttles_a_hidden_page() {}

#[test]
#[ignore = "checks the WinUI app (CDP Emulation.setDeviceMetricsOverride); WebKitGTK sizes by set_size_request and zoom"]
fn the_layout_viewport_is_pinned_with_cdp_so_a_150_percent_monitor_does_not_scale_every_snapshot() {}

#[test]
#[ignore = "checks the WinUI app (CDP Page.captureScreenshot clip); the 1-px clamp is md-logic's and is tested in export_adversarial"]
fn the_snapshot_is_a_cdp_clip_in_page_coordinates_taken_beyond_the_viewport() {}

/// md.win also pins `string.Create(CultureInfo.InvariantCulture, …)` around ExportRenderer's CDP
/// payloads (WinUI only). The md-logic half is the scripts' own spelling of their numbers.
#[test]
fn every_number_the_renderer_writes_into_json_is_spelled_invariantly() {
    let script = scripts::canvas_rasterise(7, 1.25);
    assert!(script.contains("[7]"));
    assert!(script.contains("rect.width * 1.25"));
    assert!(scripts::diagram_svg(1_000_000).contains("nodes[1000000]"));
    assert_eq!(scripts::invariant_number(0.5), "0.5");
}

// ── §7.3 the print settings ───────────────────────────────────────────────────────────────

/// md.win also pins the eleven `CoreWebView2PrintSettings` assignments (WinUI only; on Linux they
/// are `gtk::PrintSettings` / `gtk::PageSetup`, docs/linux-facts.md §5). The number the four
/// margins carry is md-logic's.
#[test]
fn the_print_settings_are_the_designs_eight_lines_with_half_an_inch_on_all_four_sides() {
    assert_eq!(PrintGeometry::DEFAULT_MARGIN_IN, 0.5);
    assert_eq!(PrintGeometry::paper().margin_in, 0.5);
}

#[test]
#[ignore = "checks the WinUI app (PrintToPdfAsync into TemporaryFiles); the GTK surface prints to a temporary file and returns the bytes"]
fn the_pdf_is_written_to_a_temp_file_and_read_back_rather_than_over_the_pickers_file() {}

// ── §7.2 print ────────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "checks the WinUI app (ShowPrintUI Browser vs System); Linux prints through GTK's own dialog"]
fn print_opens_chromiums_browser_preview_and_keeps_the_system_dialog_only_as_the_fallback() {}

/// md.win also pins PrintOverlay's code and XAML (WinUI only — Linux has no overlay: GTK's print
/// dialog is its own window). The two strings it assigns are md-logic's.
#[test]
fn the_print_overlay_hosts_a_visible_renderer_and_offers_print_again_and_done() {
    assert_eq!(exports::PRINT_AGAIN, "Print\u{2026}");
    assert_eq!(buttons::DONE, "Done");
}

// ── §7.1 the written-and-off fallback host ────────────────────────────────────────────────

#[test]
#[ignore = "checks the WinUI app (ExportHostWindow, AppWindow.Show(false)); the GTK renderer has no fallback window"]
fn the_fallback_host_window_is_shown_without_activation_rather_than_never_shown() {}

// ── §7.8 share ────────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "checks the WinUI app (ShareBridge, IDataTransferManagerInterop); Linux has no share sheet"]
fn share_goes_through_the_desktop_interop_and_never_get_for_current_view() {}

// ── §7.1 the renderer is always taken down ────────────────────────────────────────────────

#[test]
#[ignore = "checks the WinUI app (WebView2.Close); that the pipeline closes every surface is tested in export_adversarial"]
fn the_renderer_leaves_no_web_view2_behind_it() {}
