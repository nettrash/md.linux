//! Visible-region snapshots of the EPUB's rich elements (shell-design §7.5) — per element, never a
//! full-document texture (a hundred-page document at 2× would be a 200-Mpx texture):
//!
//! 1. `zoom = scale / d`, where `d` is the window's surface scale (fractional on Wayland); the view
//!    becomes `⌈595 × zoom⌉ × ⌈max(842, rect.height) × zoom⌉` logical px, so the CSS viewport stays
//!    595 px wide and the rects keep their CSS values, and the element fits the visible region;
//! 2. read the element's rect again in the zoomed layout — zoom moves text by a few per cent (see
//!    [`matching`]) — then scroll to it and read back where the page really is (the end of the
//!    document clamps it);
//! 3. two frames — the 300 ms settle only when the zoom or the size changed;
//! 4. `snapshot(Visible)`; the factor is measured (`texture width ÷ CSS viewport width`), not
//!    assumed; crop `(x, y − scrollY, w, h) × factor` with `gdk::TextureDownloader`; PNG.
//!
//! The spike proved the zoom-and-crop mechanism on a full-document texture (966 × 96 px for a
//! 483 × 47.9 px formula, docs/linux-facts.md §6); the arithmetic is pure here and unit-tested.

use std::time::Duration;

use gtk::gdk;
use md_logic::preview::json_script;
use md_logic::seams::RectD;
use webkit::prelude::*;

use crate::web::js;

/// The export renderer's CSS viewport: A4's 595 × 842 (md-logic's `RichSnapshotter` measures in it).
pub const VIEWPORT_WIDTH: f64 = 595.0;
pub const VIEWPORT_HEIGHT: f64 = 842.0;

/// The settle after a zoom or a resize (the Mac's repaint wait).
const SETTLE: Duration = Duration::from_millis(300);

/// How long the two-frame wait may take before the capture goes on without it.
const FRAME_LIMIT: Duration = Duration::from_millis(500);

/// How long one snapshot may take.
const SNAPSHOT_LIMIT: Duration = Duration::from_secs(20);

/// How long GTK may take to give the view its new size.
const ALLOCATION_LIMIT: Duration = Duration::from_secs(3);

/// The page zoom that makes one CSS px `scale` device px on a surface of scale `device`.
pub fn zoom_for(scale: f64, device: f64) -> f64 {
    let device = if device.is_finite() && device > 0.0 { device } else { 1.0 };
    scale / device
}

/// The view's size request, in logical px, for an element `rect_height` CSS px tall at `zoom`: the
/// viewport stays 595 CSS px wide and at least 842 tall, and the whole element fits.
pub fn view_size(rect_height: f64, zoom: f64) -> (i32, i32) {
    let height = VIEWPORT_HEIGHT.max(rect_height);
    ((VIEWPORT_WIDTH * zoom).ceil() as i32, (height * zoom).ceil() as i32)
}

/// The crop, in texture pixels, of `rect` (page coordinates in CSS px) from a visible-region texture
/// of `texture` size whose top is at `scroll_y` CSS px and which holds `factor` px per CSS px —
/// clamped to the texture, never smaller than 1 × 1. `None` when the element is wholly outside it.
pub fn crop_rect(rect: RectD, scroll_y: f64, factor: f64, texture: (i32, i32)) -> Option<(i32, i32, i32, i32)> {
    let (tw, th) = texture;
    let x = (rect.x * factor).round() as i32;
    let y = ((rect.y - scroll_y) * factor).round() as i32;
    let w = ((rect.width * factor).round() as i32).max(1);
    let h = ((rect.height * factor).round() as i32).max(1);
    let (left, top) = (x.max(0), y.max(0));
    let (right, bottom) = ((x + w).min(tw), (y + h).min(th));
    if right <= left || bottom <= top {
        return None;
    }
    Some((left, top, right - left, bottom - top))
}

/// Which of `measured` (the page's rich-element rects, in DOM order) is `rect` — the one md-logic
/// measured and asks for, clamped to at least 1 × 1 — or `None` when it is none of them.
///
/// Measured under Xvfb: page zoom changes the layout a little (text metrics at the zoomed size
/// break lines differently), so a 40-page document is ~2 % taller at zoom 2 and its last formula
/// ~750 CSS px lower. The rect md-logic measured at zoom 1 is therefore looked up by its position
/// in that layout and re-read in the zoomed one.
pub fn matching(rect: RectD, measured: &[RectD]) -> Option<usize> {
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    measured.iter().position(|m| {
        near(m.x, rect.x)
            && near(m.y, rect.y)
            && near(m.width.max(1.0), rect.width)
            && near(m.height.max(1.0), rect.height)
    })
}

/// Copies a sub-rectangle of `texture` into a new `GdkMemoryTexture` (RGBA8).
pub fn crop_texture(texture: &gdk::Texture, x: i32, y: i32, w: i32, h: i32) -> gdk::Texture {
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = downloader.download_bytes();
    let row = w as usize * 4;
    let mut out = Vec::with_capacity(row * h as usize);
    for r in 0..h as usize {
        let start = (y as usize + r) * stride + x as usize * 4;
        out.extend_from_slice(&bytes[start..start + row]);
    }
    let bytes = glib::Bytes::from_owned(out);
    gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &bytes, row).upcast()
}

/// The zoom and size the view was last set to, so the settle is paid only on a change.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub zoom: f64,
    pub size: (i32, i32),
}

impl ViewState {
    /// The renderer as created: zoom 1, 595 × 842.
    pub const INITIAL: ViewState = ViewState { zoom: 1.0, size: (VIEWPORT_WIDTH as i32, VIEWPORT_HEIGHT as i32) };
}

/// Puts `view` into `state`: zoom, size request, then waits until GTK has allocated the size and
/// the page has settled. Returns at once when it is already there.
pub async fn apply_state(view: &webkit::WebView, current: ViewState, wanted: ViewState) {
    if current == wanted {
        return;
    }
    if (current.zoom - wanted.zoom).abs() > f64::EPSILON {
        view.set_zoom_level(wanted.zoom);
    }
    if current.size != wanted.size {
        view.set_size_request(wanted.size.0, wanted.size.1);
        let start = std::time::Instant::now();
        while (view.width(), view.height()) != wanted.size && start.elapsed() < ALLOCATION_LIMIT {
            glib::timeout_future(Duration::from_millis(16)).await;
        }
    }
    glib::timeout_future(SETTLE).await;
}

/// A PNG of `rect` (page coordinates, CSS px) at `scale` device px per CSS px, from the view's
/// visible region. The view must already be at the zoom and size [`view_size`] gives for `rect`.
pub async fn capture(view: &webkit::WebView, rect: RectD, zoom: f64) -> Result<Vec<u8>, String> {
    let scroll_y = json_script::number(&js::eval_json(view, &js::snapshot_scroll(rect.y)).await?)
        .ok_or("the page did not say where it scrolled")?;
    // Two frames, so the scroll has been painted on screen too; the snapshot itself is painted by
    // the web process from the page, so a frame clock that is idle does not hold it up.
    if let Ok(frames) = glib::future_with_timeout(FRAME_LIMIT, js::two_frames(view)).await {
        frames?;
    }
    let texture = glib::future_with_timeout(
        SNAPSHOT_LIMIT,
        view.snapshot_future(webkit::SnapshotRegion::Visible, webkit::SnapshotOptions::NONE),
    )
    .await
    .map_err(|_| format!("no snapshot in {} s", SNAPSHOT_LIMIT.as_secs()))?
    .map_err(|e| e.message().to_owned())?;
    let css_width = f64::from(view.width()) / zoom;
    if css_width <= 0.0 {
        return Err("the export renderer has no width".to_owned());
    }
    let factor = f64::from(texture.width()) / css_width;
    let (x, y, w, h) = crop_rect(rect, scroll_y, factor, (texture.width(), texture.height())).ok_or_else(|| {
        format!(
            "the element at {},{} ({} × {}) is outside the {} × {} snapshot scrolled to {scroll_y}",
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            texture.width(),
            texture.height()
        )
    })?;
    Ok(crop_texture(&texture, x, y, w, h).save_to_png_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
        RectD { x, y, width, height }
    }

    #[test]
    fn zoom_divides_the_scale_by_the_surface_scale() {
        assert_eq!(zoom_for(2.0, 1.0), 2.0);
        assert_eq!(zoom_for(2.0, 2.0), 1.0);
        assert_eq!(zoom_for(2.0, 1.25), 1.6);
        assert_eq!(zoom_for(1.5, 2.0), 0.75);
        assert_eq!(zoom_for(2.0, 0.0), 2.0, "an unknown surface scale is 1");
        assert_eq!(zoom_for(2.0, f64::NAN), 2.0);
    }

    #[test]
    fn the_view_keeps_595_css_px_and_fits_the_element() {
        assert_eq!(view_size(100.0, 1.0), (595, 842));
        assert_eq!(view_size(100.0, 2.0), (1190, 1684));
        assert_eq!(view_size(1000.0, 2.0), (1190, 2000));
        assert_eq!(view_size(47.9, 1.6), (952, 1348));
    }

    #[test]
    fn the_crop_is_the_rect_times_the_factor_below_the_scroll() {
        // The spike's formula: 483 × 47.9 CSS px at 2× → 966 × 96.
        assert_eq!(crop_rect(rect(56.0, 1200.0, 483.0, 47.9), 1180.0, 2.0, (1190, 1684)), Some((112, 40, 966, 96)));
        assert_eq!(crop_rect(rect(10.0, 20.0, 30.0, 40.0), 0.0, 1.0, (595, 842)), Some((10, 20, 30, 40)));
    }

    #[test]
    fn a_clamped_scroll_moves_the_element_down_the_texture() {
        // The last element: the page could only scroll to 3000, not to 3500.
        assert_eq!(crop_rect(rect(0.0, 3500.0, 100.0, 50.0), 3000.0, 2.0, (1190, 1684)), Some((0, 1000, 200, 100)));
    }

    #[test]
    fn the_crop_is_clamped_to_the_texture_and_never_empty() {
        assert_eq!(crop_rect(rect(-5.0, 0.0, 700.0, 10.0), 0.0, 1.0, (595, 842)), Some((0, 0, 595, 10)));
        assert_eq!(crop_rect(rect(0.0, 800.0, 10.0, 100.0), 0.0, 1.0, (595, 842)), Some((0, 800, 10, 42)));
        assert_eq!(crop_rect(rect(3.0, 4.0, 0.0, 0.0), 0.0, 1.0, (595, 842)), Some((3, 4, 1, 1)));
        assert_eq!(crop_rect(rect(0.0, 900.0, 10.0, 10.0), 0.0, 1.0, (595, 842)), None);
        assert_eq!(crop_rect(rect(600.0, 0.0, 10.0, 10.0), 0.0, 1.0, (595, 842)), None);
    }

    #[test]
    fn a_rect_is_found_in_the_layout_it_was_measured_in() {
        let layout = [rect(56.0, 100.0, 68.2, 18.0), rect(56.0, 300.0, 483.0, 0.0), rect(56.0, 900.0, 483.0, 47.9)];
        assert_eq!(matching(rect(56.0, 100.0, 68.2, 18.0), &layout), Some(0));
        // md-logic clamps an empty element to 1 × 1 before asking.
        assert_eq!(matching(rect(56.0, 300.0, 483.0, 1.0), &layout), Some(1));
        assert_eq!(matching(rect(56.0, 900.2, 483.0, 47.9), &layout), Some(2));
        assert_eq!(matching(rect(56.0, 950.0, 483.0, 47.9), &layout), None);
        assert_eq!(matching(rect(0.0, 0.0, 1.0, 1.0), &[]), None);
    }

    #[test]
    fn a_hidpi_texture_doubles_without_a_zoom() {
        // GDK_SCALE=2: zoom 1, a 595-px-wide view, a 1190-px texture → factor 2.
        let zoom = zoom_for(2.0, 2.0);
        let (w, _) = view_size(50.0, zoom);
        let factor = 1190.0 / (f64::from(w) / zoom);
        assert_eq!(factor, 2.0);
        assert_eq!(crop_rect(rect(56.0, 100.0, 483.0, 47.9), 100.0, factor, (1190, 1684)), Some((112, 0, 966, 96)));
    }
}
