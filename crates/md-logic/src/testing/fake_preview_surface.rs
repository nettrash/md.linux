//! Port of md.win `FakePreviewSurface`.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use super::answer_queue::AnswerQueue;
use crate::event::Event;
use crate::exec::{Completion, LocalBoxFuture, ready};
use crate::seams::PreviewSurface;

/// Recording [`PreviewSurface`]: every HTML assignment, navigation, reload and script is kept;
/// [`PreviewSurface::is_shown`] is settable (collapsed pane); script results come from
/// [`FakePreviewSurface::set_eval_handler`], else the queue ([`FakePreviewSurface::queue_eval_result`],
/// or a held [`FakePreviewSurface::hold_eval`]), else the JSON `null`;
/// [`FakePreviewSurface::complete_navigation`] raises the navigation event.
pub struct FakePreviewSurface {
    html: RefCell<String>,
    is_shown: Cell<bool>,
    html_history: RefCell<Vec<String>>,
    navigations: RefCell<Vec<String>>,
    reload_count: Cell<usize>,
    evals: RefCell<Vec<String>>,
    eval_results: AnswerQueue<String>,
    eval_handler: RefCell<Option<Rc<dyn Fn(&str) -> String>>>,
    navigation_completed: Event<bool>,
}

impl Default for FakePreviewSurface {
    fn default() -> Self {
        FakePreviewSurface {
            html: RefCell::new(String::new()),
            is_shown: Cell::new(true),
            html_history: RefCell::new(Vec::new()),
            navigations: RefCell::new(Vec::new()),
            reload_count: Cell::new(0),
            evals: RefCell::new(Vec::new()),
            eval_results: AnswerQueue::new(),
            eval_handler: RefCell::new(None),
            navigation_completed: Event::new(),
        }
    }
}

impl fmt::Debug for FakePreviewSurface {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakePreviewSurface")
            .field("is_shown", &self.is_shown.get())
            .field("html_history", &self.html_history.borrow().len())
            .field("navigations", &self.navigations.borrow())
            .field("reload_count", &self.reload_count.get())
            .field("evals", &self.evals.borrow())
            .finish()
    }
}

impl FakePreviewSurface {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shown (true, the default) or collapsed.
    pub fn set_is_shown(&self, shown: bool) {
        self.is_shown.set(shown);
    }

    /// Every value [`PreviewSurface::set_html`] was given, in order.
    pub fn html_history(&self) -> Vec<String> {
        self.html_history.borrow().clone()
    }

    pub fn navigations(&self) -> Vec<String> {
        self.navigations.borrow().clone()
    }

    pub fn reload_count(&self) -> usize {
        self.reload_count.get()
    }

    /// Every script evaluated, in order.
    pub fn evals(&self) -> Vec<String> {
        self.evals.borrow().clone()
    }

    /// The next script (with no handler set) returns `json`.
    pub fn queue_eval_result(&self, json: &str) {
        self.eval_results.push(json.to_string());
    }

    /// The next script (with no handler set) stays pending until the completion is completed.
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

    /// Raises [`PreviewSurface::navigation_completed`] (md.win's default is success: pass true).
    pub fn complete_navigation(&self, is_success: bool) {
        self.navigation_completed.emit(&is_success);
    }
}

impl PreviewSurface for FakePreviewSurface {
    fn html(&self) -> String {
        self.html.borrow().clone()
    }

    fn set_html(&self, html: String) {
        self.html_history.borrow_mut().push(html.clone());
        *self.html.borrow_mut() = html;
    }

    fn is_shown(&self) -> bool {
        self.is_shown.get()
    }

    fn navigate(&self, url: &str) {
        self.navigations.borrow_mut().push(url.to_string());
    }

    fn reload(&self) {
        self.reload_count.set(self.reload_count.get() + 1);
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, String> {
        self.evals.borrow_mut().push(script.clone());
        let handler = self.eval_handler.borrow().clone();
        match handler {
            Some(handler) => ready(handler(&script)),
            None => self.eval_results.next_or(|| "null".to_string()),
        }
    }

    fn navigation_completed(&self) -> &Event<bool> {
        &self.navigation_completed
    }
}
