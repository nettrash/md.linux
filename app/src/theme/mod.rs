//! The paper theme (shell-design §10): one `gtk::CssProvider` at application priority whose text
//! [`css`] generates from md-logic's [`Palette`], reloaded when libadwaita's dark flag flips, and
//! the [`changed`] event every window subscribes to for what CSS cannot reach (a `WebView`'s
//! background, the preview's `data-md-dark` re-render). md follows the system colour scheme; there
//! is no in-app appearance switch.
//!
//! The style classes the CSS defines, for the packages that build the widgets:
//!
//! | Class | On | Owner |
//! | --- | --- | --- |
//! | `md-editor` | the editor `GtkTextView` (typewriter 15 pt, ink, accent caret, selection) | P3 |
//! | `md-placeholder` | the editor's placeholder label (ink 40 %) | P3 |
//! | `md-divider-right`, `md-divider-bottom` | the editor column in Split (1 px, ink 16 %) | P3 |
//! | `md-find-field` | the find bar's two fields (13 pt) | P3 |
//! | `md-footer` | the footer bar (11 pt tabular, ink 60 %, paper-secondary) | P3 |
//! | `md-bar` | the conflict / save-error bar (paper-secondary) | P3 |
//! | `md-zen-capsule`, `md-zen-active` | the Zen capsule (80 % paper-secondary) and its active switch | P3 |
//! | `md-sidebar-row`, `md-chapter-header`, `md-new-article` | Book sidebar rows (13 / 13 / 12 pt; regular weight inside a button, which Adwaita sets bold) | P5 |
//! | `md-status-title`, `md-status-message` | Book status pages' text (17 / 12 pt) | P5 |

use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::Rc;

use md_logic::event::Event;
use md_logic::settings::Palette;

/// The typewriter face Ubuntu ships (URW's Courier), then the metric-compatible mono, then any.
const TYPEWRITER: &str = "\"Nimbus Mono PS\", \"Liberation Mono\", monospace";

thread_local! {
    static PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static CHANGED: Rc<Event<bool>> = Rc::new(Event::new());
}

/// Raised after the CSS was reloaded for the other scheme. Args: whether the theme is now dark.
pub fn changed() -> Rc<Event<bool>> {
    CHANGED.with(Rc::clone)
}

/// libadwaita's answer: the system preference (a portal on the desktop, `ADW_DEBUG_COLOR_SCHEME`
/// in a test) as the application follows it.
pub fn is_dark() -> bool {
    adw::StyleManager::default().is_dark()
}

/// The palette for the current scheme.
pub fn palette() -> Palette {
    Palette::for_dark(is_dark())
}

/// `startup`: installs the provider on the default display and follows the dark flag. Idempotent.
pub fn install() {
    if PROVIDER.with(|p| p.borrow().is_some()) {
        return;
    }
    let Some(display) = gtk::gdk::Display::default() else {
        eprintln!("md: no display; the paper theme is not installed");
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css(&palette()));
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    PROVIDER.with(|p| *p.borrow_mut() = Some(provider));
    adw::StyleManager::default().connect_dark_notify(|_| reload());
}

/// The dark flag flipped: new CSS first, then the windows.
fn reload() {
    let dark = is_dark();
    PROVIDER.with(|p| {
        if let Some(provider) = p.borrow().as_ref() {
            provider.load_from_string(&css(&Palette::for_dark(dark)));
        }
    });
    changed().emit(&dark);
}

/// `rgba(r, g, b, a)` for an ARGB value, alpha to three decimals.
fn rgba(argb: u32) -> String {
    let (a, r, g, b) = Palette::channels(argb);
    let alpha = (f64::from(a) / 255.0 * 1000.0).round() / 1000.0;
    format!("rgba({r}, {g}, {b}, {alpha})")
}

/// The whole stylesheet for one palette: libadwaita's colour variables on `:root` (window, view,
/// header bar, popover, dialog and sidebar are paper; the accent replaces Yaru's) and md's own
/// classes (see the module table).
pub fn css(palette: &Palette) -> String {
    let paper = Palette::hex(palette.paper);
    let secondary = Palette::hex(palette.paper_secondary);
    let ink = Palette::hex(palette.ink);
    let accent = Palette::hex(palette.accent);
    let ink_secondary = rgba(palette.ink_secondary());
    let ink_tertiary = rgba(palette.ink_tertiary());
    let border = rgba(palette.border());
    let capsule = rgba(Palette::with_opacity(palette.paper_secondary, 0.8));
    let selection = rgba(Palette::with_opacity(palette.accent, 0.35));

    let mut out = String::new();
    let _ = write!(
        out,
        ":root {{
  --window-bg-color: {paper};
  --window-fg-color: {ink};
  --view-bg-color: {paper};
  --view-fg-color: {ink};
  --headerbar-bg-color: {paper};
  --headerbar-fg-color: {ink};
  --popover-bg-color: {secondary};
  --popover-fg-color: {ink};
  --dialog-bg-color: {secondary};
  --dialog-fg-color: {ink};
  --sidebar-bg-color: {secondary};
  --sidebar-fg-color: {ink};
  --accent-bg-color: {accent};
  --accent-color: {accent};
  --yaru-accent-bg-color: {accent};
}}
textview.md-editor {{
  font-family: {TYPEWRITER};
  font-size: 15pt;
}}
textview.md-editor text {{
  color: {ink};
  caret-color: {accent};
  background-color: transparent;
}}
textview.md-editor text selection {{
  background-color: {selection};
}}
.md-placeholder {{
  font-family: {TYPEWRITER};
  font-size: 15pt;
  color: {ink_tertiary};
}}
.md-divider-right {{
  border-right: 1px solid {border};
}}
.md-divider-bottom {{
  border-bottom: 1px solid {border};
}}
.md-find-field {{
  font-family: {TYPEWRITER};
  font-size: 13pt;
}}
.md-footer {{
  font-family: {TYPEWRITER};
  font-size: 11pt;
  font-feature-settings: \"tnum\";
  color: {ink_secondary};
  background-color: {secondary};
  padding: 5px 12px;
}}
.md-bar {{
  background-color: {secondary};
}}
.md-zen-capsule {{
  background-color: {capsule};
  border-radius: 20px;
  padding: 6px;
}}
.md-zen-capsule .md-zen-active {{
  color: {accent};
}}
.md-sidebar-row, .md-chapter-header {{
  font-family: {TYPEWRITER};
  font-size: 13pt;
}}
.md-new-article {{
  font-family: {TYPEWRITER};
  font-size: 12pt;
}}
button .md-sidebar-row, button .md-new-article {{
  font-weight: normal;
}}
.md-status-title {{
  font-family: {TYPEWRITER};
  font-size: 17pt;
}}
.md-status-message {{
  font-family: {TYPEWRITER};
  font-size: 12pt;
}}
"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_sheet_is_paper_ink_and_the_accent() {
        let css = css(&Palette::LIGHT);
        assert!(css.contains("--window-bg-color: #F4EFE2;"));
        assert!(css.contains("--headerbar-bg-color: #F4EFE2;"));
        assert!(css.contains("--window-fg-color: #2B2620;"));
        assert!(css.contains("--popover-bg-color: #EAE2CF;"));
        assert!(css.contains("--accent-bg-color: #9C6B2E;"));
        assert!(css.contains("--yaru-accent-bg-color: #9C6B2E;"));
        assert!(css.contains("caret-color: #9C6B2E;"));
        // Ink at 60 / 40 / 16 %, the capsule at 80 %, the selection at 35 %.
        assert!(css.contains("rgba(43, 38, 32, 0.6)"));
        assert!(css.contains("rgba(43, 38, 32, 0.4)"));
        assert!(css.contains("border-right: 1px solid rgba(43, 38, 32, 0.161)"));
        assert!(css.contains("rgba(234, 226, 207, 0.8)"));
        assert!(css.contains("rgba(156, 107, 46, 0.349)"));
    }

    #[test]
    fn the_dark_sheet_is_carbon_paper() {
        let css = css(&Palette::DARK);
        assert!(css.contains("--window-bg-color: #241E18;"));
        assert!(css.contains("--view-fg-color: #E7DBC2;"));
        assert!(css.contains("--sidebar-bg-color: #2F2820;"));
        assert!(css.contains("--accent-color: #C99A55;"));
        assert!(!css.contains("#F4EFE2"));
    }

    #[test]
    fn every_class_the_packages_use_is_styled_and_the_braces_balance() {
        let css = css(&Palette::LIGHT);
        for class in [
            "textview.md-editor",
            ".md-placeholder",
            ".md-divider-right",
            ".md-divider-bottom",
            ".md-find-field",
            ".md-footer",
            ".md-bar",
            ".md-zen-capsule",
            ".md-zen-active",
            ".md-sidebar-row",
            ".md-chapter-header",
            ".md-new-article",
            ".md-status-title",
            ".md-status-message",
        ] {
            assert!(css.contains(class), "{class}");
        }
        assert_eq!(css.matches('{').count(), css.matches('}').count());
        assert!(css.contains("font-family: \"Nimbus Mono PS\", \"Liberation Mono\", monospace;"));
        // Adwaita sets button labels bold; the sidebar's New Chapter… / New Article… are rows.
        assert!(css.contains("button .md-sidebar-row, button .md-new-article {\n  font-weight: normal;\n}"));
    }
}
