//! `OverlayRenderer` — md-logic's `RenderSurface` as a fresh `webkit::WebView` added to the
//! requesting window's `gtk::Overlay`, mapped and invisible (opacity 0, no input, 595 × 842), on an
//! `export-<n>` host (shell-design §7.1, facts §6); `OverlayRendererFactory` makes one per export.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::rc::Rc;

use md_logic::exec::{CancelToken, LocalBoxFuture, ready};
use md_logic::seams::{PrintGeometry, RectD, RenderError, RenderKind, RenderSurface, RenderSurfaceFactory};

use crate::web::context::WebServices;

/// The S0 stub's answer to every render.
fn unimplemented() -> RenderError {
    RenderError::Failed(String::from("S0 stub: the overlay renderer is work package P4"))
}

/// One export's renderer.
pub struct OverlayRenderer {
    overlay: gtk::Overlay,
    kind: RenderKind,
}

impl RenderSurface for OverlayRenderer {
    fn load(&self, html: String, cancel: CancelToken) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        ready(Err(unimplemented()))
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, Result<String, RenderError>> {
        ready(Err(unimplemented()))
    }

    fn capture_region_png(&self, css_rect: RectD, scale: f64) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        ready(Err(unimplemented()))
    }

    fn set_height(&self, css_px: f64) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        ready(Err(unimplemented()))
    }

    fn pdf(&self, geometry: PrintGeometry) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        ready(Err(unimplemented()))
    }

    fn close(&self) -> LocalBoxFuture<'static, ()> {
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
}

impl RenderSurfaceFactory for OverlayRendererFactory {
    fn create(&self, kind: RenderKind) -> LocalBoxFuture<'static, Result<Box<dyn RenderSurface>, RenderError>> {
        ready(Err(unimplemented()))
    }
}
