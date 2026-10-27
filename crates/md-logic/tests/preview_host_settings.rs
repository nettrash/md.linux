//! Port of md.win `tests/Md.App.Logic.Tests/Preview/PreviewHostSettingsTests.cs`. Every assertion
//! there reads the WinUI app's source (`PreviewHost.cs`, `WebViewEnvironment.cs`) for the
//! `CoreWebView2EnvironmentOptions` and `CoreWebView2Settings` lines §4.1 and §4.4 name; none
//! concerns md-logic data. They are kept as ignored stubs so the count matches; the GTK app's
//! `WebKitSettings` (no developer extras in release, `print-backgrounds`, the paper background
//! colour before the first load, one `WebContext`, `web-process-terminated` handling) get their own
//! checks in the app.

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_environment_keeps_timers_running_on_a_hidden_page() {
    // --disable-background-timer-throttling in AdditionalBrowserArguments.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_environment_is_created_once_with_the_documented_options() {
    // AreBrowserExtensionsEnabled / EnableTrackingPrevention false, CreateWithOptionsAsync,
    // `_creating ??=`, no Package.Current.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn every_setting_of_the_table() {
    // The thirteen CoreWebView2Settings assignments of §4.4 (accelerator keys, zoom, pinch, swipe,
    // status bar, script dialogs, autofill, password autosave, host objects, error page, web
    // messages, scripts, context menus).
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn dev_tools_are_only_for_debug_builds() {
    // `#if DEBUG settings.AreDevToolsEnabled = true; #else … false;`.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_context_menu_keeps_only_what_wk_web_view_offers() {
    // ContextMenuRequested keeps "copy", "selectAll".
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn scrollbars_follow_the_app_theme() {
    // Profile.PreferredColorScheme Dark / Light.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_paper_colour_is_painted_before_the_runtime_exists() {
    // ApplyTheme(dark: false) in the constructor before EnsureCoreWebView2Async; DefaultBackgroundColor.
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn only_a_fatal_process_failure_re_attaches_the_host() {
    // ProcessFailed: BrowserProcessExited / RenderProcessExited, never RenderProcessUnresponsive;
    // the attach guarded by ReferenceEquals(core, _attached).
}
