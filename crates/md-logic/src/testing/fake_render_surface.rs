//! Port of md.win `FakeRenderSurface` and `FakeRenderSurfaceFactory`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use super::answer_queue::AnswerQueue;
use crate::exec::{CancelToken, Completion, LocalBoxFuture, ready};
use crate::seams::{PrintGeometry, RectD, RenderError, RenderKind, RenderSurface, RenderSurfaceFactory};

/// Recording [`RenderSurface`]: the HTML loaded, scripts, captures, heights and PDF geometries are
/// kept; results are scripted ([`FakeRenderSurface::set_eval_handler`] /
/// [`FakeRenderSurface::queue_eval_result`], [`FakeRenderSurface::set_png_bytes`],
/// [`FakeRenderSurface::set_pdf_bytes`]); [`FakeRenderSurface::set_load_failure`] and
/// [`FakeRenderSurface::set_pdf_failure`] make the corresponding call fail;
/// [`FakeRenderSurface::hold_load`] keeps the next load pending (a render that has not finished).
/// [`RenderSurface::close`] is observable ([`FakeRenderSurface::is_closed`], md.win's
/// `IsDisposed`).
pub struct FakeRenderSurface {
    kind: RenderKind,
    loaded_html: RefCell<Vec<String>>,
    evals: RefCell<Vec<String>>,
    captures: RefCell<Vec<(RectD, f64)>>,
    heights: RefCell<Vec<f64>>,
    pdf_requests: RefCell<Vec<PrintGeometry>>,
    is_closed: Cell<bool>,
    eval_results: AnswerQueue<String>,
    eval_handler: RefCell<Option<Rc<dyn Fn(&str) -> String>>>,
    load_answers: AnswerQueue<Result<(), RenderError>>,
    png_bytes: RefCell<Vec<u8>>,
    pdf_bytes: RefCell<Vec<u8>>,
    load_failure: RefCell<Option<RenderError>>,
    pdf_failure: RefCell<Option<RenderError>>,
}

impl Default for FakeRenderSurface {
    /// A [`RenderKind::Screen`] surface (C#'s default enum value).
    fn default() -> Self {
        Self::new(RenderKind::Screen)
    }
}

impl fmt::Debug for FakeRenderSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeRenderSurface")
            .field("kind", &self.kind)
            .field("loaded_html", &self.loaded_html.borrow().len())
            .field("evals", &self.evals.borrow())
            .field("is_closed", &self.is_closed.get())
            .finish()
    }
}

impl FakeRenderSurface {
    /// The PNG signature — what a capture returns until [`FakeRenderSurface::set_png_bytes`].
    pub const DEFAULT_PNG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    /// What [`RenderSurface::pdf`] returns until [`FakeRenderSurface::set_pdf_bytes`].
    pub const DEFAULT_PDF: &[u8] = b"%PDF-1.7\n%fake\n";

    pub fn new(kind: RenderKind) -> Self {
        FakeRenderSurface {
            kind,
            loaded_html: RefCell::new(Vec::new()),
            evals: RefCell::new(Vec::new()),
            captures: RefCell::new(Vec::new()),
            heights: RefCell::new(Vec::new()),
            pdf_requests: RefCell::new(Vec::new()),
            is_closed: Cell::new(false),
            eval_results: AnswerQueue::new(),
            eval_handler: RefCell::new(None),
            load_answers: AnswerQueue::new(),
            png_bytes: RefCell::new(Self::DEFAULT_PNG.to_vec()),
            pdf_bytes: RefCell::new(Self::DEFAULT_PDF.to_vec()),
            load_failure: RefCell::new(None),
            pdf_failure: RefCell::new(None),
        }
    }

    pub fn kind(&self) -> RenderKind {
        self.kind
    }

    pub fn loaded_html(&self) -> Vec<String> {
        self.loaded_html.borrow().clone()
    }

    pub fn evals(&self) -> Vec<String> {
        self.evals.borrow().clone()
    }

    /// Every [`RenderSurface::capture_region_png`] call: (rect, scale).
    pub fn captures(&self) -> Vec<(RectD, f64)> {
        self.captures.borrow().clone()
    }

    pub fn heights(&self) -> Vec<f64> {
        self.heights.borrow().clone()
    }

    pub fn pdf_requests(&self) -> Vec<PrintGeometry> {
        self.pdf_requests.borrow().clone()
    }

    pub fn is_closed(&self) -> bool {
        self.is_closed.get()
    }

    pub fn queue_eval_result(&self, json: &str) {
        self.eval_results.push(json.to_string());
    }

    pub fn hold_eval(&self) -> Completion<String> {
        self.eval_results.hold()
    }

    /// Every script's result from now on; the queue is ignored while a handler is set.
    pub fn set_eval_handler(&self, handler: impl Fn(&str) -> String + 'static) {
        *self.eval_handler.borrow_mut() = Some(Rc::new(handler));
    }

    pub fn clear_eval_handler(&self) {
        *self.eval_handler.borrow_mut() = None;
    }

    /// What every capture returns (a copy each time).
    pub fn png_bytes(&self) -> Vec<u8> {
        self.png_bytes.borrow().clone()
    }

    pub fn set_png_bytes(&self, bytes: &[u8]) {
        *self.png_bytes.borrow_mut() = bytes.to_vec();
    }

    pub fn pdf_bytes(&self) -> Vec<u8> {
        self.pdf_bytes.borrow().clone()
    }

    pub fn set_pdf_bytes(&self, bytes: &[u8]) {
        *self.pdf_bytes.borrow_mut() = bytes.to_vec();
    }

    /// Every load (after being recorded) fails with this, until cleared with `None`.
    pub fn set_load_failure(&self, failure: Option<RenderError>) {
        *self.load_failure.borrow_mut() = failure;
    }

    /// Every PDF request (after being recorded) fails with this, until cleared with `None`.
    pub fn set_pdf_failure(&self, failure: Option<RenderError>) {
        *self.pdf_failure.borrow_mut() = failure;
    }

    /// The next load (recorded as usual) stays pending until the completion is completed with
    /// its outcome. A held load takes precedence over [`FakeRenderSurface::set_load_failure`].
    pub fn hold_load(&self) -> Completion<Result<(), RenderError>> {
        self.load_answers.hold()
    }
}

impl RenderSurface for FakeRenderSurface {
    fn load(&self, html: String, cancel: CancelToken) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        // md.win: `ct.ThrowIfCancellationRequested()` before anything is recorded.
        if cancel.is_cancelled() {
            return ready(Err(RenderError::Cancelled));
        }
        self.loaded_html.borrow_mut().push(html);
        let failure = self.load_failure.borrow().clone();
        self.load_answers.next_or(move || match failure {
            Some(error) => Err(error),
            None => Ok(()),
        })
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, Result<String, RenderError>> {
        self.evals.borrow_mut().push(script.clone());
        let handler = self.eval_handler.borrow().clone();
        match handler {
            Some(handler) => ready(Ok(handler(&script))),
            None => {
                let result = self.eval_results.next_or(|| "null".to_string());
                Box::pin(async move { Ok(result.await) })
            }
        }
    }

    fn capture_region_png(&self, css_rect: RectD, scale: f64) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        self.captures.borrow_mut().push((css_rect, scale));
        ready(Ok(self.png_bytes()))
    }

    fn set_height(&self, css_px: f64) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        self.heights.borrow_mut().push(css_px);
        ready(Ok(()))
    }

    fn pdf(&self, geometry: PrintGeometry) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        self.pdf_requests.borrow_mut().push(geometry);
        let failure = self.pdf_failure.borrow().clone();
        match failure {
            Some(error) => ready(Err(error)),
            None => ready(Ok(self.pdf_bytes())),
        }
    }

    fn close(&self) -> LocalBoxFuture<'static, ()> {
        self.is_closed.set(true);
        ready(())
    }
}

/// The factory hands the logic a `Box<dyn RenderSurface>` while the test keeps the `Rc`: the box
/// is this shared handle.
impl RenderSurface for Rc<FakeRenderSurface> {
    fn load(&self, html: String, cancel: CancelToken) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        (**self).load(html, cancel)
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, Result<String, RenderError>> {
        (**self).eval(script)
    }

    fn capture_region_png(&self, css_rect: RectD, scale: f64) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        (**self).capture_region_png(css_rect, scale)
    }

    fn set_height(&self, css_px: f64) -> LocalBoxFuture<'static, Result<(), RenderError>> {
        (**self).set_height(css_px)
    }

    fn pdf(&self, geometry: PrintGeometry) -> LocalBoxFuture<'static, Result<Vec<u8>, RenderError>> {
        (**self).pdf(geometry)
    }

    fn close(&self) -> LocalBoxFuture<'static, ()> {
        (**self).close()
    }
}

/// [`RenderSurfaceFactory`] handing out [`FakeRenderSurface`]s (a fresh one per call, or whatever
/// [`FakeRenderSurfaceFactory::set_create`] returns) and recording the [`RenderKind`] asked for —
/// how a test pins "HTML export renders as Export, PDF as Paper". [`FakeRenderSurfaceFactory::created`]
/// gives the test the very surfaces the logic was handed. [`FakeRenderSurfaceFactory::set_failure`]
/// makes every creation fail (a renderer that could not start).
#[derive(Default)]
pub struct FakeRenderSurfaceFactory {
    kinds: RefCell<Vec<RenderKind>>,
    created: RefCell<Vec<Rc<FakeRenderSurface>>>,
    create: RefCell<Option<Rc<dyn Fn(RenderKind) -> Rc<FakeRenderSurface>>>>,
    failure: RefCell<Option<RenderError>>,
}

impl fmt::Debug for FakeRenderSurfaceFactory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeRenderSurfaceFactory").field("kinds", &self.kinds.borrow()).finish()
    }
}

impl FakeRenderSurfaceFactory {
    pub fn new() -> Self {
        Self::default()
    }

    /// The kind of every creation asked for, in order (failed ones included).
    pub fn kinds(&self) -> Vec<RenderKind> {
        self.kinds.borrow().clone()
    }

    /// Every surface handed out, in order.
    pub fn created(&self) -> Vec<Rc<FakeRenderSurface>> {
        self.created.borrow().clone()
    }

    /// The last surface handed out.
    ///
    /// # Panics
    ///
    /// If none was.
    pub fn last(&self) -> Rc<FakeRenderSurface> {
        self.created.borrow().last().cloned().expect("FakeRenderSurfaceFactory: no surface was created")
    }

    /// Makes the surfaces from now on (md.win's `Create` property).
    pub fn set_create(&self, create: impl Fn(RenderKind) -> Rc<FakeRenderSurface> + 'static) {
        *self.create.borrow_mut() = Some(Rc::new(create));
    }

    /// Every creation fails with this, until cleared with `None`.
    pub fn set_failure(&self, failure: Option<RenderError>) {
        *self.failure.borrow_mut() = failure;
    }
}

impl RenderSurfaceFactory for FakeRenderSurfaceFactory {
    fn create(&self, kind: RenderKind) -> LocalBoxFuture<'static, Result<Box<dyn RenderSurface>, RenderError>> {
        self.kinds.borrow_mut().push(kind);
        if let Some(error) = self.failure.borrow().clone() {
            return ready(Err(error));
        }
        let create = self.create.borrow().clone();
        let surface = match create {
            Some(create) => create(kind),
            None => Rc::new(FakeRenderSurface::new(kind)),
        };
        self.created.borrow_mut().push(surface.clone());
        ready(Ok(Box::new(surface) as Box<dyn RenderSurface>))
    }
}
