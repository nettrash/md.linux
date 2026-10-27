//! Visible-region snapshots for the EPUB's rich elements (shell-design §7.5): page zoom = scale ÷
//! surface scale, scroll to the element, two frames, `snapshot_future(Visible)`, crop with
//! `gdk::TextureDownloader`, PNG — the crop arithmetic pure and unit-tested.
//!
//! S0 moved the spike's full-document snapshot and crop (`export.rs`, facts §6) here as the
//! starting point.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::path::Path;
use std::time::Duration;

use gtk::gdk;
use webkit::prelude::*;

use crate::web::js::{eval_json, json_str};

pub struct ElementShot {
    /// The element's rect in CSS px (document coordinates).
    pub css: [f64; 4],
    /// Device pixels per CSS px in the snapshot texture.
    pub factor: f64,
    pub png_size: (i32, i32),
}

/// The spike's proof: a PNG of the first element matching `selector` at `scale` device px per CSS
/// px. Page zoom raises the snapshot's scale; it also shrinks the CSS viewport, so the caller widens
/// the view by the same factor when the layout width matters.
pub async fn snapshot_element(
    view: &webkit::WebView,
    selector: &str,
    scale: f64,
    path: &Path,
) -> Result<ElementShot, String> {
    let old_zoom = view.zoom_level();
    view.set_zoom_level(scale);
    glib::timeout_future(Duration::from_millis(400)).await; // let the zoomed layout settle
    let result = snapshot_inner(view, selector, path).await;
    view.set_zoom_level(old_zoom);
    result
}

async fn snapshot_inner(view: &webkit::WebView, selector: &str, path: &Path) -> Result<ElementShot, String> {
    let script = format!(
        "(function(){{var e=document.querySelector({sel}); if(!e) return null; var r=e.getBoundingClientRect(); \
         return [r.left+window.scrollX, r.top+window.scrollY, r.width, r.height, document.documentElement.scrollWidth];}})()",
        sel = json_str(selector)
    );
    let json = eval_json(view, &script).await?;
    let nums: Vec<f64> = json.trim_matches(['[', ']']).split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let [x, y, w, h, doc_w] = nums[..] else {
        return Err(format!("no element for {selector}: {json}"));
    };
    let texture = view
        .snapshot_future(webkit::SnapshotRegion::FullDocument, webkit::SnapshotOptions::NONE)
        .await
        .map_err(|e| e.message().to_owned())?;
    let factor = f64::from(texture.width()) / doc_w;
    let crop = [x, y, w, h].map(|v| (v * factor).round() as i32);
    let png = crop_texture(&texture, crop[0], crop[1], crop[2], crop[3])?;
    png.save_to_png(path).map_err(|e| e.to_string())?;
    Ok(ElementShot { css: [x, y, w, h], factor, png_size: (png.width(), png.height()) })
}

/// Copies a sub-rectangle of `texture` into a new `GdkMemoryTexture` (RGBA8).
pub fn crop_texture(texture: &gdk::Texture, x: i32, y: i32, w: i32, h: i32) -> Result<gdk::Texture, String> {
    let (tw, th) = (texture.width(), texture.height());
    let x = x.clamp(0, tw);
    let y = y.clamp(0, th);
    let w = w.min(tw - x);
    let h = h.min(th - y);
    if w <= 0 || h <= 0 {
        return Err(format!("empty crop {x},{y} {w}x{h} of {tw}x{th}"));
    }
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
    Ok(gdk::MemoryTexture::new(w, h, gdk::MemoryFormat::R8g8b8a8, &bytes, row).upcast())
}
