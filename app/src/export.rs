//! PDF without a dialog (`WebKitPrintOperation` + `GtkPrintSettings`/`GtkPageSetup` aimed at
//! GTK's built-in "Print to File" printer) and element snapshots at 2× (page zoom + a
//! full-document snapshot, cropped).

use crate::util;
use gtk::gdk;
use std::path::Path;
use std::time::{Duration, Instant};
use webkit::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum Paper {
    A4,
    /// Width × height in inches, e.g. the 6 × 9 in trade paperback.
    Inches(f64, f64),
}

/// The name of GTK's file printer in the current locale. GTK names it `_("Print to File")` in
/// its `gtk40` gettext domain, and WebKit finds the printer by that name, so a hard-coded
/// English string would miss it under a translated desktop.
pub fn file_printer_name() -> String {
    glib::dgettext(Some("gtk40"), "Print to File").to_string()
}

/// Prints the page in `view` to `path` as PDF — no dialog. `margin_in` on all four sides.
pub async fn print_pdf(view: &webkit::WebView, path: &Path, paper: Paper, margin_in: f64) -> Result<Duration, String> {
    print_pdf_with_printer(view, path, paper, margin_in, &file_printer_name()).await
}

pub async fn print_pdf_with_printer(
    view: &webkit::WebView,
    path: &Path,
    paper: Paper,
    margin_in: f64,
    printer: &str,
) -> Result<Duration, String> {
    let start = Instant::now();
    let uri = gio::File::for_path(path).uri();
    let paper_size = match paper {
        Paper::A4 => gtk::PaperSize::new(Some(gtk::PAPER_NAME_A4)),
        Paper::Inches(w, h) => {
            let name = format!("custom_{w}x{h}in");
            gtk::PaperSize::new_custom(&name, &format!("{w} × {h} in"), w, h, gtk::Unit::Inch)
        }
    };

    let settings = gtk::PrintSettings::new();
    settings.set_printer(printer);
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(uri.as_str()));
    settings.set_paper_size(&paper_size);
    settings.set_orientation(gtk::PageOrientation::Portrait);

    let setup = gtk::PageSetup::new();
    setup.set_paper_size(&paper_size);
    setup.set_orientation(gtk::PageOrientation::Portrait);
    setup.set_top_margin(margin_in, gtk::Unit::Inch);
    setup.set_bottom_margin(margin_in, gtk::Unit::Inch);
    setup.set_left_margin(margin_in, gtk::Unit::Inch);
    setup.set_right_margin(margin_in, gtk::Unit::Inch);

    let op = webkit::PrintOperation::new(view);
    op.set_print_settings(&settings);
    op.set_page_setup(&setup);
    let (tx, rx) = util::oneshot::<Result<(), String>>();
    let ok = tx.clone();
    op.connect_finished(move |_| ok.send(Ok(())));
    op.connect_failed(move |_, err| tx.send(Err(err.message().to_owned())));
    op.print();
    match util::with_timeout(Duration::from_secs(90), rx).await {
        Some(Ok(())) => Ok(start.elapsed()),
        Some(Err(msg)) => Err(msg),
        None => Err("timed out after 90 s".to_owned()),
    }
}

/// `pdfinfo` → (pages, "W x H pts" of page 1).
pub async fn pdf_info(path: &Path) -> Option<(u32, String)> {
    let (ok, out) = util::run(&["pdfinfo", &path.to_string_lossy()]).await;
    if !ok {
        return None;
    }
    let field = |name: &str| {
        out.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim_start_matches(':').trim().to_owned())
    };
    Some((field("Pages")?.parse().ok()?, field("Page size")?))
}

pub struct ElementShot {
    /// The element's rect in CSS px (document coordinates).
    pub css: [f64; 4],
    /// Device pixels per CSS px in the snapshot texture.
    pub factor: f64,
    pub png_size: (i32, i32),
}

/// A PNG of the first element matching `selector`, at `scale` device px per CSS px.
///
/// WebKitGTK's `webkit_web_view_get_snapshot` renders at the view's device scale; page zoom
/// (`set_zoom_level`) is what raises it. Zoom shrinks the CSS viewport, so the caller widens
/// the view by the same factor when the layout width matters (the export renderer does).
pub async fn snapshot_element(
    view: &webkit::WebView,
    selector: &str,
    scale: f64,
    path: &Path,
) -> Result<ElementShot, String> {
    let old_zoom = view.zoom_level();
    view.set_zoom_level(scale);
    util::sleep_ms(400).await; // let the zoomed layout settle before reading rects
    let result = snapshot_inner(view, selector, path).await;
    view.set_zoom_level(old_zoom);
    result
}

async fn snapshot_inner(view: &webkit::WebView, selector: &str, path: &Path) -> Result<ElementShot, String> {
    let script = format!(
        "(function(){{var e=document.querySelector({sel}); if(!e) return null; var r=e.getBoundingClientRect(); \
         return [r.left+window.scrollX, r.top+window.scrollY, r.width, r.height, document.documentElement.scrollWidth];}})()",
        sel = util::json_str(selector)
    );
    let json = util::eval_json(view, &script).await?;
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

/// Copies a sub-rectangle of `texture` into a new `GdkMemoryTexture` (RGBA8, unpremultiplied).
impl ElementShot {
    pub fn describe(&self) -> String {
        let [x, y, w, h] = self.css;
        format!(
            "element {w}x{h} CSS px at ({x}, {y}); {}x{} px PNG = {}x device px per CSS px",
            self.png_size.0, self.png_size.1, self.factor
        )
    }
}

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
