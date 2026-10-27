//! `OverlayRenderer` — md-logic's `RenderSurface` as a fresh `webkit::WebView` added to the
//! requesting window's `gtk::Overlay`, mapped and invisible ([`OPACITY`] 1/255, no input, no focus,
//! 595 × 842, start-aligned), on an `export-<n>` host (shell-design §7.1, docs/linux-facts.md §6).
//! `OverlayRendererFactory` makes one per export.
//!
//! Mapped, because a never-parented `WebView` lays out at 0 × 0 with `visibilityState` "hidden" —
//! fine for a PDF, useless for a snapshot — while an overlay child gets exactly its size request
//! (the overlay does not measure it, so the window's minimum size is untouched), is "visible" and
//! keeps its timers. No user scripts and no message handlers: an export page is the pure md-core
//! (or paper) HTML md-logic's `ExportPipeline` hands it.
//!
//! What each seam call does is md-logic's contract (its `export` module docs):
//! - `load`: serve the page, `load_uri`, await `Finished` (`load-failed` → "The rendered page could
//!   not be captured."), then md-logic's `RenderCompletePoller` on this surface (250 ms × 480,
//!   timeout = success);
//! - `eval`: JSON text, an exception is an error;
//! - `set_height`: nothing — the visible-region capture sizes the view per element;
//! - `capture_region_png`: `snapshot`'s zoom, scroll, two frames, visible snapshot, crop — with the
//!   element's rect read again in the zoomed layout (`snapshot::matching`);
//! - `pdf`: `pdf::print_to_file` into `$XDG_RUNTIME_DIR/md/<uuid>.pdf`, read back, deleted;
//! - `close`: off the overlay, page forgotten, view dropped — also when the surface is dropped
//!   unclosed.
// P4 → P1 / P5: until the windows construct exports, only the self-test reaches this; removed at the
// integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use md_logic::exec::{CancelToken, Completion, LocalBoxFuture, ready};
use md_logic::export::{RenderCompletePoller, rich_rects};
use md_logic::preview::{asset_origin, scripts};
use md_logic::seams::{PrintGeometry, RectD, RenderError, RenderKind, RenderSurface, RenderSurfaceFactory, Scheduler};
use md_logic::strings::exports;
use webkit::prelude::*;

use crate::export::{pdf, snapshot};
use crate::services::glib_seams::GlibScheduler;
use crate::web::context::WebServices;
use crate::web::js;

/// The renderer's opacity: the least GTK draws, 1/255 (GTK keeps a widget's opacity in 8 bits, so
/// anything smaller rounds to 0). Measured under Xvfb: at 0 GTK does not draw the view and the
/// freshly loaded page gets no animation frames (`requestAnimationFrame` did not run within 3 s);
/// at 1/255 the view is composited every frame — a blend no 8-bit screen can show — and the page
/// runs as a visible one.
pub const OPACITY: f64 = 1.0 / 255.0;

/// How long one page may take to reach `Finished` (the render-complete wait comes after).
const LOAD_LIMIT: Duration = Duration::from_secs(60);

/// How long a new renderer waits to be mapped before it loads anyway.
const MAP_LIMIT: Duration = Duration::from_secs(2);

struct Inner {
    web: Rc<WebServices>,
    overlay: gtk::Overlay,
    view: webkit::WebView,
    host: String,
    index_url: String,
    kind: RenderKind,
    scheduler: Rc<dyn Scheduler>,
    /// The load in flight, completed by `load-changed` / `load-failed`.
    pending: RefCell<Option<Completion<Result<(), String>>>>,
    load_failed: Cell<bool>,
    state: Cell<snapshot::ViewState>,
    /// The rich elements' rects in each page zoom measured since the last load.
    layouts: RefCell<Vec<(f64, Vec<RectD>)>>,
    closed: Cell<bool>,
}

impl Inner {
    fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.view.stop_loading();
        if self.view.parent().as_ref() == Some(self.overlay.upcast_ref()) {
            self.overlay.remove_overlay(&self.view);
        }
        self.web.asset_host().remove_page(&self.host);
        if let Some(pending) = self.pending.borrow_mut().take() {
            pending.complete(Err("the renderer was closed".to_owned()));
        }
    }

    fn finish_load(&self, result: Result<(), String>) {
        if let Some(pending) = self.pending.borrow_mut().take() {
            pending.complete(result);
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.close();
    }
}

/// One export's renderer: an invisible, mapped `WebView` in a window's overlay.
#[derive(Clone)]
pub struct OverlayRenderer {
    inner: Rc<Inner>,
}

impl OverlayRenderer {
    fn new(overlay: &gtk::Overlay, web: &Rc<WebServices>, kind: RenderKind) -> OverlayRenderer {
        let view = web.new_view(&webkit::UserContentManager::new());
        view.set_opacity(OPACITY);
        view.set_can_target(false);
        view.set_can_focus(false);
        view.set_focusable(false);
        view.set_halign(gtk::Align::Start);
        view.set_valign(gtk::Align::Start);
        let initial = snapshot::ViewState::INITIAL;
        view.set_size_request(initial.size.0, initial.size.1);
        let host = web.next_export_host();
        let inner = Rc::new(Inner {
            web: Rc::clone(web),
            overlay: overlay.clone(),
            index_url: asset_origin::index_url(&host),
            host,
            kind,
            view,
            scheduler: Rc::new(GlibScheduler),
            pending: RefCell::new(None),
            load_failed: Cell::new(false),
            state: Cell::new(initial),
            layouts: RefCell::new(Vec::new()),
            closed: Cell::new(false),
        });
        let weak = Rc::downgrade(&inner);
        inner.view.connect_load_changed(move |_, event| {
            let Some(inner) = weak.upgrade() else { return };
            match event {
                webkit::LoadEvent::Started => inner.load_failed.set(false),
                webkit::LoadEvent::Finished => {
                    let failed = inner.load_failed.replace(false);
                    inner.finish_load(if failed { Err("the page failed to load".to_owned()) } else { Ok(()) });
                }
                _ => {}
            }
        });
        let weak = Rc::downgrade(&inner);
        inner.view.connect_load_failed(move |_, _, uri, error| {
            if let Some(inner) = weak.upgrade() {
                eprintln!("md: the export renderer could not load {uri}: {}", error.message());
                inner.load_failed.set(true);
            }
            true
        });
        inner.view.connect_decide_policy(|_, decision, kind| {
            // An export page navigates nowhere but to itself.
            if kind == webkit::PolicyDecisionType::NewWindowAction {
                decision.ignore();
                return true;
            }
            false
        });
        inner.view.connect_create(|_, _| None);
        overlay.add_overlay(&inner.view);
        OverlayRenderer { inner }
    }

    /// The web view, for Print's dialog.
    pub fn view(&self) -> &webkit::WebView {
        &self.inner.view
    }

    /// The surface scale of the window, 1 when it is not realised yet.
    fn device_scale(&self) -> f64 {
        self.inner.overlay.native().and_then(|native| native.surface()).map(|surface| surface.scale()).unwrap_or(1.0)
    }

    async fn load_page(&self, html: String, cancel: CancelToken) -> Result<(), RenderError> {
        let inner = &self.inner;
        if cancel.is_cancelled() {
            return Err(RenderError::Cancelled);
        }
        if inner.closed.get() || !inner.web.can_load() {
            return Err(RenderError::Failed(exports::PAGE_NOT_CAPTURED.to_owned()));
        }
        // Each load starts from the standard viewport, so the rects md-logic measures next are the
        // 595-px layout's.
        snapshot::apply_state(
            &inner.view,
            inner.state.replace(snapshot::ViewState::INITIAL),
            snapshot::ViewState::INITIAL,
        )
        .await;
        inner.layouts.borrow_mut().clear();
        inner.web.asset_host().set_page(&inner.host, html);
        let done: Completion<Result<(), String>> = Completion::new();
        if let Some(previous) = inner.pending.replace(Some(done.clone())) {
            previous.complete(Err("superseded".to_owned()));
        }
        inner.view.load_uri(&inner.index_url);
        let loaded = match glib::future_with_timeout(LOAD_LIMIT, done.wait()).await {
            Ok(result) => result,
            Err(_) => Err(format!("no Finished in {} s", LOAD_LIMIT.as_secs())),
        };
        if let Err(message) = loaded {
            eprintln!("md: export {} ({:?}): {message}", inner.host, inner.kind);
            return Err(RenderError::Failed(exports::PAGE_NOT_CAPTURED.to_owned()));
        }
        RenderCompletePoller::new(Rc::clone(&inner.scheduler)).wait(self, &cancel).await
    }

    /// The rich elements' rects in the layout of `zoom` — measured now when the view is at that
    /// zoom and they have not been yet, else as measured before; `None` when unknown.
    async fn layout(&self, zoom: f64) -> Option<Vec<RectD>> {
        let inner = &self.inner;
        let cached = inner.layouts.borrow().iter().find(|(z, _)| *z == zoom).map(|(_, rects)| rects.clone());
        if cached.is_some() || inner.state.get().zoom != zoom {
            return cached;
        }
        let json = js::eval_json(&inner.view, &scripts::rich_elements()).await.ok();
        let rects: Vec<RectD> = rich_rects::parse(json.as_deref()).into_iter().map(|r| r.rect).collect();
        inner.layouts.borrow_mut().push((zoom, rects.clone()));
        Some(rects)
    }

    async fn capture(&self, rect: RectD, scale: f64) -> Result<Vec<u8>, RenderError> {
        let inner = &self.inner;
        if inner.closed.get() {
            return Err(RenderError::Failed(exports::RICH_IMAGES_NOT_CAPTURED.to_owned()));
        }
        let zoom = snapshot::zoom_for(scale, self.device_scale());
        // md-logic measured `rect` in the layout the page loaded in (zoom 1); find it there before
        // zooming, then read the same element again in the zoomed layout (see `snapshot::matching`).
        let index =
            self.layout(snapshot::ViewState::INITIAL.zoom).await.and_then(|rects| snapshot::matching(rect, &rects));
        let zoomed = snapshot::ViewState { zoom, size: snapshot::view_size(rect.height, zoom) };
        snapshot::apply_state(&inner.view, inner.state.replace(zoomed), zoomed).await;
        let target = match index {
            Some(i) => self.layout(zoom).await.and_then(|rects| rects.get(i).copied()).unwrap_or(rect),
            None => rect,
        };
        let target = RectD { width: target.width.max(1.0), height: target.height.max(1.0), ..target };
        let wanted = snapshot::ViewState { zoom, size: snapshot::view_size(target.height, zoom) };
        snapshot::apply_state(&inner.view, inner.state.replace(wanted), wanted).await;
        snapshot::capture(&inner.view, target, zoom).await.map_err(|message| {
            eprintln!("md: export {}: snapshot of {target:?} at {scale}×: {message}", inner.host);
            RenderError::Failed(exports::RICH_IMAGES_NOT_CAPTURED.to_owned())
        })
    }

    async fn print_pdf(&self, geometry: PrintGeometry) -> Result<Vec<u8>, RenderError> {
        let inner = &self.inner;
        let failed = |message: String| {
            eprintln!("md: export {}: PDF: {message}", inner.host);
            RenderError::Failed(exports::PDF_PAGES_NOT_PRODUCED.to_owned())
        };
        if inner.closed.get() {
            return Err(failed("the renderer was closed".to_owned()));
        }
        let path = inner.web.temp_dir().join(format!("{}.pdf", glib::uuid_string_random()));
        let printed = pdf::print_to_file(&inner.view, &path, &geometry).await;
        let bytes = printed.and_then(|()| std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display())));
        let _ = std::fs::remove_file(&path);
        bytes.map_err(failed)
    }
}

impl RenderSurface for OverlayRenderer {
    fn load(&self, html: String, cancel: CancelToken) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        let this = self.clone();
        Box::pin(async move { this.load_page(html, cancel).await })
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, Result<String, RenderError>> {
        let view = self.inner.view.clone();
        Box::pin(async move { js::eval_json(&view, &script).await.map_err(RenderError::Failed) })
    }

    fn capture_region_png(&self, css_rect: RectD, scale: f64) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        let this = self.clone();
        Box::pin(async move { this.capture(css_rect, scale).await })
    }

    /// md-logic's grow-to-content call. Nothing to do: the capture sizes the view per element so
    /// that each one is inside the visible region, and the rects are page coordinates either way.
    fn set_height(&self, _css_px: f64) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        ready(Ok(()))
    }

    fn pdf(&self, geometry: PrintGeometry) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        let this = self.clone();
        Box::pin(async move { this.print_pdf(geometry).await })
    }

    fn close(&self) -> LocalBoxFuture<'static, ()> {
        self.inner.close();
        ready(())
    }
}

/// Renderers in one window's overlay.
pub struct OverlayRendererFactory {
    overlay: gtk::Overlay,
    web: Rc<WebServices>,
}

impl OverlayRendererFactory {
    pub fn new(overlay: &gtk::Overlay, web: &Rc<WebServices>) -> Rc<OverlayRendererFactory> {
        Rc::new(OverlayRendererFactory { overlay: overlay.clone(), web: Rc::clone(web) })
    }

    /// A renderer of the concrete type (Print needs its view), mapped once the window has drawn it.
    pub fn create_renderer(&self, kind: RenderKind) -> LocalBoxFuture<'static, OverlayRenderer> {
        create_in(&self.overlay, &self.web, kind)
    }
}

/// A renderer in `overlay`, after its first frame (at most [`MAP_LIMIT`]: an unmapped window never
/// draws one).
pub fn create_in(
    overlay: &gtk::Overlay,
    web: &Rc<WebServices>,
    kind: RenderKind,
) -> LocalBoxFuture<'static, OverlayRenderer> {
    let renderer = OverlayRenderer::new(overlay, web, kind);
    Box::pin(async move {
        let drawn: Completion<()> = Completion::new();
        let ticked = drawn.clone();
        renderer.inner.view.add_tick_callback(move |_, _| {
            ticked.complete(());
            glib::ControlFlow::Break
        });
        let _ = glib::future_with_timeout(MAP_LIMIT, drawn.wait()).await;
        renderer
    })
}

impl RenderSurfaceFactory for OverlayRendererFactory {
    fn create(&self, kind: RenderKind) -> LocalBoxFuture<'static, Result<Box<dyn RenderSurface>, RenderError>> {
        let created = self.create_renderer(kind);
        Box::pin(async move { Ok(Box::new(created.await) as Box<dyn RenderSurface>) })
    }
}
