//! The paper theme's colours — port of md.win `Palette` / `PaletteTheme`.

/// Light or dark — the two palettes; the app maps libadwaita's `StyleManager::is_dark` onto this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaletteTheme {
    Light,
    Dark,
}

/// The paper theme's colours (shell-design §10) as ARGB, the single source for the GTK app's CSS,
/// the `WebView` background colour and the Zen tint. The values are the shared stylesheet's
/// (md-core `markdown::html::css`): paper `#F4EFE2` / `#241E18`, ink `#2B2620` / `#E7DBC2`,
/// accent `#9C6B2E` / `#C99A55`, secondary paper `#EAE2CF` / `#2F2820`. The three derived colours
/// are ink at 60 %, 40 % and 16 % (the CSS border's `rgba(…, 0.16)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette {
    pub paper: u32,
    pub paper_secondary: u32,
    pub ink: u32,
    pub accent: u32,
}

impl Palette {
    pub const INK_SECONDARY_OPACITY: f64 = 0.6;
    pub const INK_TERTIARY_OPACITY: f64 = 0.4;
    pub const BORDER_OPACITY: f64 = 0.16;

    pub const LIGHT: Palette = Palette::new(0xFFF4EFE2, 0xFFEAE2CF, 0xFF2B2620, 0xFF9C6B2E);
    pub const DARK: Palette = Palette::new(0xFF241E18, 0xFF2F2820, 0xFFE7DBC2, 0xFFC99A55);

    /// md.win's positional record constructor: paper, secondary paper, ink, accent.
    pub const fn new(paper: u32, paper_secondary: u32, ink: u32, accent: u32) -> Palette {
        Palette { paper, paper_secondary, ink, accent }
    }

    /// `Palette.For(PaletteTheme)`.
    pub const fn for_theme(theme: PaletteTheme) -> Palette {
        match theme {
            PaletteTheme::Dark => Palette::DARK,
            PaletteTheme::Light => Palette::LIGHT,
        }
    }

    /// `Palette.For(bool dark)`.
    pub const fn for_dark(dark: bool) -> Palette {
        if dark { Palette::DARK } else { Palette::LIGHT }
    }

    /// Footer, sidebar buttons, placeholder titles — ink at 60 %.
    pub fn ink_secondary(&self) -> u32 {
        Palette::with_opacity(self.ink, Palette::INK_SECONDARY_OPACITY)
    }

    /// The editor's placeholder — ink at 40 %.
    pub fn ink_tertiary(&self) -> u32 {
        Palette::with_opacity(self.ink, Palette::INK_TERTIARY_OPACITY)
    }

    /// Dividers — ink at 16 %, the stylesheet's border.
    pub fn border(&self) -> u32 {
        Palette::with_opacity(self.ink, Palette::BORDER_OPACITY)
    }

    /// The same RGB with alpha = round(255 × opacity), the opacity clamped to [0, 1]. md.win
    /// rounds with `MidpointRounding.AwayFromZero`, which is what `f64::round` does; a NaN
    /// opacity gives alpha 0 in both (.NET's saturating cast, Rust's `as`).
    pub fn with_opacity(argb: u32, opacity: f64) -> u32 {
        let alpha = (255.0 * opacity.clamp(0.0, 1.0)).round() as u32;
        (alpha << 24) | (argb & 0x00FF_FFFF)
    }

    /// The four channels (A, R, G, B), for building a `gdk::RGBA` without arithmetic at the call
    /// site.
    pub fn channels(argb: u32) -> (u8, u8, u8, u8) {
        ((argb >> 24) as u8, (argb >> 16) as u8, (argb >> 8) as u8, argb as u8)
    }

    /// `#RRGGBB`, upper case — the spelling the stylesheet uses; alpha is dropped.
    pub fn hex(argb: u32) -> String {
        format!("#{:06X}", argb & 0x00FF_FFFF)
    }
}
