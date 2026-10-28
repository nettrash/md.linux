//! Port of md.win `tests/Md.App.Logic.Tests/PaletteTests.cs` (shell-design §10): the paper
//! palette's numbers, the derived opacities, and the single-source rule. md.win pins the rule
//! against `App.xaml` and the MSIX manifest; md.linux has neither, so the rule is pinned against
//! the shared stylesheet the palette is taken from (md-core `markdown::html::css`), and the
//! WinUI-only tests are kept as ignored stubs.

use md_core::markdown::html;
use md_logic::settings::{Palette, PaletteTheme};

#[test]
fn light_and_dark_are_the_stylesheets_paper_ink_and_accent() {
    assert_eq!(Palette::LIGHT, Palette::new(0xFFF4EFE2, 0xFFEAE2CF, 0xFF2B2620, 0xFF9C6B2E));
    assert_eq!(Palette::DARK, Palette::new(0xFF241E18, 0xFF2F2820, 0xFFE7DBC2, 0xFFC99A55));
    assert_eq!(
        Palette::LIGHT,
        Palette { paper: 0xFFF4EFE2, paper_secondary: 0xFFEAE2CF, ink: 0xFF2B2620, accent: 0xFF9C6B2E }
    );
}

#[test]
fn for_maps_the_theme_and_the_dark_flag() {
    assert_eq!(Palette::for_theme(PaletteTheme::Light), Palette::LIGHT);
    assert_eq!(Palette::for_theme(PaletteTheme::Dark), Palette::DARK);
    assert_eq!(Palette::for_dark(false), Palette::LIGHT);
    assert_eq!(Palette::for_dark(true), Palette::DARK);
}

#[test]
fn with_opacity_rounds_the_alpha_and_keeps_the_rgb() {
    for (opacity, alpha) in [
        (0.6, 0x99),
        (0.4, 0x66),
        (0.16, 0x29),
        (1.0, 0xFF),
        (0.0, 0x00),
        (1.5, 0xFF), // clamped
        (-1.0, 0x00),
    ] {
        let argb = Palette::with_opacity(0xFF2B2620, opacity);
        assert_eq!(argb >> 24, alpha, "{opacity}");
        assert_eq!(argb & 0x00FF_FFFF, 0x2B2620, "{opacity}");
    }
}

#[test]
fn with_opacity_rounds_a_midpoint_away_from_zero() {
    // Rust only: md.win passes MidpointRounding.AwayFromZero, which is f64::round's rule. 255 × 0.3
    // is exactly 76.5 in binary64: away from zero gives 77 where banker's rounding would give 76.
    assert_eq!(Palette::with_opacity(0, 0.5) >> 24, 128);
    assert_eq!(Palette::with_opacity(0, 0.3) >> 24, 77);
    assert_eq!(Palette::with_opacity(0xFF123456, f64::NAN), 0x00123456);
}

#[test]
fn derived_brushes_are_ink_at_60_40_and_16_percent() {
    assert_eq!(Palette::INK_SECONDARY_OPACITY, 0.6);
    assert_eq!(Palette::INK_TERTIARY_OPACITY, 0.4);
    assert_eq!(Palette::BORDER_OPACITY, 0.16);
    assert_eq!(Palette::LIGHT.ink_secondary(), 0x992B2620);
    assert_eq!(Palette::LIGHT.ink_tertiary(), 0x662B2620);
    assert_eq!(Palette::LIGHT.border(), 0x292B2620);
    assert_eq!(Palette::DARK.ink_secondary(), 0x99E7DBC2);
    assert_eq!(Palette::DARK.ink_tertiary(), 0x66E7DBC2);
    assert_eq!(Palette::DARK.border(), 0x29E7DBC2);
}

#[test]
fn channels_split_argb_in_order() {
    assert_eq!(Palette::channels(0xFFF4EFE2), (0xFF, 0xF4, 0xEF, 0xE2));
    assert_eq!(Palette::channels(0x292B2620), (0x29, 0x2B, 0x26, 0x20));
}

#[test]
fn hex_is_upper_case_rgb_without_alpha() {
    assert_eq!(Palette::hex(Palette::LIGHT.paper), "#F4EFE2");
    assert_eq!(Palette::hex(Palette::DARK.paper), "#241E18");
    assert_eq!(Palette::hex(Palette::LIGHT.ink_secondary()), "#2B2620");
    assert_eq!(Palette::hex(0xFF000000), "#000000");
    assert_eq!(Palette::hex(0x000A0B0C), "#0A0B0C");
}

// ---- the single source: the shared stylesheet carries the same numbers ----

#[test]
fn the_stylesheet_light_and_dark_carry_the_palette() {
    // md.win: App_xaml_light_and_dark_dictionaries_carry_the_palette. The preview's stylesheet
    // (export: false) is what the paper colours come from on every port.
    for dark in [false, true] {
        let p = Palette::for_dark(dark);
        let css = html::css(dark, false);
        let has = |needle: String| assert!(css.contains(&needle), "dark={dark}: {needle} not in the stylesheet");
        has(format!("html, body {{ background: {}; }}", Palette::hex(p.paper)));
        has(format!("    color: {};", Palette::hex(p.ink)));
        has(format!("code {{ background: {};", Palette::hex(p.paper_secondary)));
        has(format!("pre {{ background: {};", Palette::hex(p.paper_secondary)));
        has(format!("a {{ color: {}; }}", Palette::hex(p.accent)));
        // The border is ink at BORDER_OPACITY, which the stylesheet spells as a literal rgba().
        let (_, r, g, b) = Palette::channels(p.ink);
        has(format!("rgba({r},{g},{b},{})", Palette::BORDER_OPACITY));
    }
}

// ---- md.win's WinUI-only checks: nothing on Linux to read ----

#[test]
#[ignore = "WinUI only: App.xaml theme dictionaries; the GTK app's CSS is the app's to pin"]
fn every_theme_dictionary_defines_every_paper_key_once() {}

#[test]
#[ignore = "WinUI only: App.xaml's TextBox resource keys; GtkTextView is styled by the app's CSS"]
fn light_and_dark_strip_the_text_box_fill_border_and_focus_underline() {}

#[test]
#[ignore = "WinUI only: App.xaml's HighContrast dictionary; libadwaita has its own high-contrast mode"]
fn high_contrast_keeps_the_system_text_box_and_uses_system_colours() {}

#[test]
#[ignore = "WinUI only: XAML bindings in App.xaml"]
fn app_xaml_uses_no_bindings() {}

#[test]
#[ignore = "WinUI only: the MSIX manifest's tile and splash colours; a .deb has neither"]
fn the_package_manifest_tile_and_splash_use_the_dark_paper() {}
