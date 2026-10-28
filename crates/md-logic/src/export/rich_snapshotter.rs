//! Port of md.win `Export/RichSnapshotter.cs`: the photographs an EPUB needs (§7.5, core-api.md
//! §A4 "epub"). Every formula and every diagram becomes a PNG — 2×, or less for a block so tall
//! that 2× would pass [`RichSnapshotter::MAX_SNAPSHOT_PIXELS`] ([`RichSnapshotter::scale_for`]) —
//! because an EPUB reader has no KaTeX, no Mermaid and no Graphviz. md-core plans the book and pairs
//! the images with the markup; this drives the browser for the one thing md-core cannot do.
//!
//! Per unit that has rich content — and only those; a heading page's `document` is deliberately
//! empty and never opens a renderer — the sequence is the Mac's: load the unit's own page and wait
//! for render-complete, grow the surface to `max(842, scrollHeight)`, let it repaint, measure,
//! check the count, then capture.
//!
//! The 300 ms repaint wait is **kept**: Mermaid's `max-width: 100%` means its layout is not final
//! until the surface has been resized and laid out again, and a rect read before that is a rect of
//! the old width.
//!
//! The count check is Android's guard, adopted because the alternative is silent corruption: Swift's
//! `zip` pairs up to the shorter list, so one missing measurement shifts every later image onto the
//! wrong element and the book still saves. md-core refuses the same mismatch on the way in
//! ([`md_core::export::EpubSnapshotCountError`]); this catches it one step earlier, before a single
//! pixel is captured, and says so in the user's words.
//!
//! LINUX: the surface's [`RenderSurface::capture_region_png`] is WebKitGTK's
//! `snapshot(FullDocument)` at page zoom `scale` on the overlay renderer, cropped to the rect × the
//! measured texture/scrollWidth factor (docs/linux-facts.md §6) — `scale` is this module's
//! [`RichSnapshotter::scale_for`], so the pixel budget is the app's too.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use md_core::export::{EpubPlan, EpubUnitPlan, RichSnapshot};

use super::export_delay;
use super::export_error::ExportError;
use super::rich_rects;
use crate::exec::CancelToken;
use crate::preview::{json_script, scripts};
use crate::seams::{RectD, RenderKind, RenderSurface, RenderSurfaceFactory, Scheduler};
use crate::strings::exports;

/// The photographer of [`EpubPlan::rich_units`], over one [`RenderSurface`] per export.
pub struct RichSnapshotter {
    renderers: Rc<dyn RenderSurfaceFactory>,
    scheduler: Rc<dyn Scheduler>,
}

impl std::fmt::Debug for RichSnapshotter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichSnapshotter").finish_non_exhaustive()
    }
}

impl RichSnapshotter {
    /// The macOS repaint wait after the surface grows (Android sleeps 250 ms; the Mac is the source
    /// of truth).
    pub const REPAINT_DELAY: Duration = Duration::from_millis(300);

    /// The offscreen layout viewport: 595 × 842 CSS px, and never shorter, whatever the document.
    pub const MINIMUM_HEIGHT_CSS_PX: f64 = 842.0;

    /// The preferred scale: a genuine 2× bitmap, as `WKSnapshotConfiguration.snapshotWidth = width * 2`
    /// gives on the Mac. Every capture takes [`RichSnapshotter::scale_for`], which lowers it past the
    /// pixel budget.
    pub const SCALE: f64 = 2.0;

    /// The most pixels one capture may have: 16 Mpx, about what the old 4096 px PlantUML gate let a
    /// page-wide diagram cost. 1.6 raised the gate and the stylesheet caps only width, so nothing
    /// else bounds a tall diagram's bitmap — the Apple ports' budget, the same number.
    pub const MAX_SNAPSHOT_PIXELS: f64 = 16.0 * 1024.0 * 1024.0;

    /// The scheduler times the repaint wait (required — see [`export_delay`]).
    pub fn new(renderers: Rc<dyn RenderSurfaceFactory>, scheduler: Rc<dyn Scheduler>) -> Self {
        RichSnapshotter { renderers, scheduler }
    }

    /// The capture scale for a block of `width` × `height` CSS px: [`RichSnapshotter::SCALE`] while
    /// the PNG fits [`RichSnapshotter::MAX_SNAPSHOT_PIXELS`], less past it.
    pub fn scale_for(width: f64, height: f64) -> f64 {
        let area = width.max(1.0) * height.max(1.0);
        Self::SCALE.min((Self::MAX_SNAPSHOT_PIXELS / area).sqrt())
    }

    /// One list of PNGs per unit that has rich content, keyed by [`EpubUnitPlan::index`] and in
    /// document order within the unit — exactly what `epub_export::assemble` takes. Empty (and no
    /// renderer at all) for a plan with nothing to photograph.
    ///
    /// Fails with [`exports::RICH_IMAGES_NOT_CAPTURED`] when the DOM measured a different number of
    /// rich elements than the markup holds, or a capture came back empty. The surface is closed on
    /// every path out.
    pub async fn capture(
        &self,
        plan: &EpubPlan,
        cancel: &CancelToken,
    ) -> Result<HashMap<usize, Vec<RichSnapshot>>, ExportError> {
        let mut snapshots = HashMap::new();
        let units = plan.rich_units();
        if units.is_empty() {
            return Ok(snapshots);
        }

        // Export, never Paper: an EPUB is read on someone else's device, so the page must be the
        // pure md-core HTML every port pins byte for byte — no Linux font style anywhere near it.
        let surface = self.renderers.create(RenderKind::Export).await?;
        let result = async {
            for unit in units {
                if cancel.is_cancelled() {
                    return Err(ExportError::Cancelled);
                }
                let taken = self.capture_unit(&*surface, unit, cancel).await?;
                snapshots.insert(unit.index, taken);
            }
            Ok(())
        }
        .await;
        surface.close().await;
        result.map(|()| snapshots)
    }

    async fn capture_unit(
        &self,
        surface: &dyn RenderSurface,
        unit: &EpubUnitPlan,
        cancel: &CancelToken,
    ) -> Result<Vec<RichSnapshot>, ExportError> {
        surface.load(unit.document.clone(), cancel.clone()).await?;

        // Grow to content: the capture is in page coordinates, so every element must lie inside the
        // laid-out area. A page that will not answer keeps the default height rather than shrinking.
        let height = json_script::number(&surface.eval(scripts::SCROLL_HEIGHT.to_string()).await?)
            .unwrap_or(Self::MINIMUM_HEIGHT_CSS_PX);
        surface.set_height(Self::MINIMUM_HEIGHT_CSS_PX.max(height)).await?;
        export_delay::delay(&*self.scheduler, Self::REPAINT_DELAY, cancel).await?;

        let answer = surface.eval(scripts::rich_elements()).await?;
        let rects = rich_rects::parse(Some(&answer));
        if rects.len() != unit.rich_elements.len() {
            return Err(ExportError::Export(exports::RICH_IMAGES_NOT_CAPTURED.to_string()));
        }

        let mut taken = Vec::with_capacity(rects.len());
        for measured in rects {
            if cancel.is_cancelled() {
                return Err(ExportError::Cancelled);
            }
            // Clamped for the capture — a renderer refuses a zero-sized region — but the
            // *unclamped* width is what the <img> carries, exactly as the Mac writes
            // Int(rect.width.rounded()).
            let rect = measured.rect;
            let clip = RectD { x: rect.x, y: rect.y, width: rect.width.max(1.0), height: rect.height.max(1.0) };
            let png = surface.capture_region_png(clip, Self::scale_for(clip.width, clip.height)).await?;
            if png.is_empty() {
                return Err(ExportError::Export(exports::RICH_IMAGES_NOT_CAPTURED.to_string()));
            }
            taken.push(RichSnapshot::new(png, rect.width));
        }
        Ok(taken)
    }
}
