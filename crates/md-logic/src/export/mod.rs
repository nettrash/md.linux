//! Port of md.win `Md.App.Logic.Export`: every export and print flow (§7), driven through the
//! [`RenderSurface`](crate::seams::RenderSurface) / [`RenderSurfaceFactory`](crate::seams::RenderSurfaceFactory)
//! seams, plus the policy around them — the footer ring, the render-complete wait, the EPUB's
//! photographs and its cover.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`export_pipeline`] | `ExportPipeline.cs` |
//! | [`busy_ring`] | `BusyRing.cs` |
//! | [`cover_reader`] | `CoverReader.cs` (+ the [`CoverFetcher`] seam for `HttpClient`) |
//! | [`export_delay`] | `ExportDelay.cs` |
//! | [`export_error`] | `ExportException.cs` (`ExportException`, `PdfPaginationException`) |
//! | [`export_names`] | `ExportNames.cs` |
//! | [`print_geometry`] | `PrintGeometry.cs` (an `impl` of the seams' [`PrintGeometry`](crate::seams::PrintGeometry)) |
//! | [`render_complete_poller`] | `RenderCompletePoller.cs` |
//! | [`rich_rects`] | `RichRects.cs` |
//! | [`rich_snapshotter`] | `RichSnapshotter.cs` |
//!
//! What the GTK app's [`RenderSurface`](crate::seams::RenderSurface) must do — the behaviour the
//! pipeline relies on (docs/linux-facts.md §§3–6):
//!
//! - **`create(kind)`**: a fresh `webkit6::WebView` on the one registered `md-asset:` context, as
//!   an invisible overlay child of the requesting window (opacity 0, not targetable or focusable,
//!   `set_size_request(595 × zoom, 842 × zoom)` so the CSS viewport is 595 × 842): mapped, so the
//!   page is "visible" and its timers are not throttled. `kind` picks the HTML it serves — the
//!   pipeline hands it the right page already, so the kind is the surface's host name and log
//!   label. No scroll-sync or link-guard scripts.
//! - **`load(html, cancel)`**: serve `html` as `md-asset://<surface>/index.html`, `load_uri` it,
//!   await `LoadEvent::Finished` (a `load-failed` is `RenderError::Failed(PAGE_NOT_CAPTURED)`),
//!   then [`RenderCompletePoller::wait`] on itself — a two-minute timeout is success. A `cancel`
//!   already set is `RenderError::Cancelled`; it may be loaded again (the EPUB reloads it per unit).
//! - **`eval(script)`**: `evaluate_javascript_future`, the result as `to_json(0)` text (`"1"` with
//!   its quotes, `null` as four letters); an exception is `Err`.
//! - **`set_height(css_px)`**: the layout viewport becomes `css_px` CSS px tall
//!   (`set_size_request(width, css_px × zoom)`) and the call completes once GTK has allocated it.
//! - **`capture_region_png(rect, scale)`**: page zoom `scale`, `snapshot(FullDocument)`, crop to
//!   `rect` (page coordinates in CSS px; the pipeline has already clamped it to ≥ 1 × 1) times the
//!   texture-width / `scrollWidth` factor, PNG-encode. `scale` is
//!   [`RichSnapshotter::scale_for`] — 2, or less to stay inside 16 Mpx.
//! - **`pdf(geometry)`**: `webkit::PrintOperation` with `gtk::PrintSettings` aimed at the localised
//!   "Print to File" printer (`dgettext("gtk40", …)`), output a temporary `.pdf`, paper
//!   `geometry.page_width_in × page_height_in` inches portrait, all four margins
//!   `geometry.margin_in`, backgrounds printed; await `finished` (or `failed` → `Err`), read the
//!   file back, delete it, return the bytes. Never a dialog.
//! - **`close()`**: remove the view from the overlay and drop it. The pipeline calls it on every
//!   path out; dropping the surface must release the view too.

pub mod busy_ring;
pub mod cover_reader;
pub mod export_delay;
pub mod export_error;
pub mod export_names;
pub mod export_pipeline;
pub mod print_geometry;
pub mod render_complete_poller;
pub mod rich_rects;
pub mod rich_snapshotter;

pub use busy_ring::BusyRing;
pub use cover_reader::{CoverBody, CoverFetcher, CoverResponse};
pub use export_error::ExportError;
pub use export_pipeline::{BundleWriter, ExportPipeline};
pub use render_complete_poller::RenderCompletePoller;
pub use rich_rects::RichRect;
pub use rich_snapshotter::RichSnapshotter;
