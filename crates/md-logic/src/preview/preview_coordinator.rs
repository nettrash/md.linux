//! Port of md.win `Preview/PreviewCoordinator.cs`: the Mac's re-render policy (§4.5), the one
//! piece of preview behaviour that is pure policy, so it lives here and the GTK preview only owns
//! the `WebKitWebView`.

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use super::json_script;
use super::preview_navigation::{JumpId, PreviewNavigation};
use super::screen_html;
use super::scripts;
use crate::event::HandlerId;
use crate::exec::{LocalBoxFuture, Spawner};
use crate::seams::{PreviewSurface, Scheduler, TimerHandle};

/// md-core's HTML writer as the coordinator needs it: the whole page, md-core's bytes,
/// `export: false`; the Linux font style is added by the coordinator. A seam so a coordinator test
/// can hand in its own writer and assert what was rendered.
pub trait DocumentHtml {
    fn document(&self, source: &str, title: &str, dark: bool) -> String;
}

/// The app's writer: `md_core::markdown::html::document(source, title, dark, false)`.
///
/// LINUX DECISION: md.win keeps a process-wide `DocumentHtml.Default` that throws until the app
/// sets it at start-up (its logic library could not reach the Core writer when the coordinator was
/// written). md-core's writer is here, so the plain constructor takes it directly and there is no
/// unwired state to report.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreDocumentHtml;

impl DocumentHtml for CoreDocumentHtml {
    fn document(&self, source: &str, title: &str, dark: bool) -> String {
        md_core::markdown::html::document(source, title, dark, false)
    }
}

/// Run once a heading jump has been performed, with its id — the owner clears its request then.
pub type NavigationHandled = Box<dyn FnOnce(JumpId)>;

#[derive(Default)]
struct State {
    last_key: Option<(bool, String, String)>,
    last_token: Option<String>,
    last_navigation_id: Option<JumpId>,
    parked: Option<(PreviewNavigation, Option<NavigationHandled>)>,
    debounce: Option<TimerHandle>,
    loaded_once: bool,
    loading: bool,
    stale: bool,
    saved_scroll_y: f64,
}

/// The Mac coordinator, verbatim (§4.5).
///
/// The shape: the whole page is regenerated on the host for every change (no DOM patching, ever),
/// the first load and a change of document are immediate, everything else is coalesced by a 350 ms
/// trailing debounce into one `reload()` that first reads `window.scrollY` and restores it when the
/// new page has loaded. That debounce is also what keeps Mermaid, Graphviz and PlantUML from
/// re-running on every keystroke.
///
/// Two additions to the Mac's four methods, md.win's (from Android's coordinator):
/// [`show`](Self::show) / [`hide`](Self::hide) carry "stale while collapsed" — in Edit mode the web
/// view stays in the tree, the coordinator only records that the page is older than
/// [`PreviewSurface::html`], and the next show loads once — and a navigation that arrives mid-load
/// is parked and performed when the load finishes.
///
/// Built as an `Rc` (it hands `Weak` references of itself to the surface's event, the debounce
/// timer and the reload); dropping the last `Rc` disconnects it and cancels a pending reload.
pub struct PreviewCoordinator {
    surface: Rc<dyn PreviewSurface>,
    scheduler: Rc<dyn Scheduler>,
    spawner: Rc<dyn Spawner>,
    html: Rc<dyn DocumentHtml>,
    index_url: String,
    me: Weak<PreviewCoordinator>,
    handler: Option<HandlerId>,
    state: RefCell<State>,
}

impl fmt::Debug for PreviewCoordinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("PreviewCoordinator")
            .field("index_url", &self.index_url)
            .field("loaded_once", &state.loaded_once)
            .field("loading", &state.loading)
            .field("stale", &state.stale)
            .field("saved_scroll_y", &state.saved_scroll_y)
            .finish()
    }
}

impl PreviewCoordinator {
    /// The Mac's 0.35 s.
    pub const DEBOUNCE_DELAY: Duration = Duration::from_millis(350);

    /// A coordinator over md-core's writer ([`CoreDocumentHtml`]). `index_url` is the surface's own
    /// document URL ([`asset_origin::index_url`](super::asset_origin::index_url) of its host);
    /// `spawner` drives the script evaluations (GLib's main context in the app).
    pub fn new(
        surface: Rc<dyn PreviewSurface>,
        scheduler: Rc<dyn Scheduler>,
        spawner: Rc<dyn Spawner>,
        index_url: impl Into<String>,
    ) -> Rc<PreviewCoordinator> {
        Self::with_html(surface, scheduler, spawner, Rc::new(CoreDocumentHtml), index_url)
    }

    /// A coordinator over its own writer (tests).
    pub fn with_html(
        surface: Rc<dyn PreviewSurface>,
        scheduler: Rc<dyn Scheduler>,
        spawner: Rc<dyn Spawner>,
        html: Rc<dyn DocumentHtml>,
        index_url: impl Into<String>,
    ) -> Rc<PreviewCoordinator> {
        let index_url = index_url.into();
        Rc::new_cyclic(|me: &Weak<PreviewCoordinator>| {
            let weak = me.clone();
            let handler = surface.navigation_completed().connect(move |&is_success| {
                if let Some(coordinator) = weak.upgrade() {
                    coordinator.on_navigation_completed(is_success);
                }
            });
            PreviewCoordinator {
                surface,
                scheduler,
                spawner,
                html,
                index_url,
                me: me.clone(),
                handler: Some(handler),
                state: RefCell::new(State::default()),
            }
        })
    }

    /// The document URL this coordinator loads.
    pub fn index_url(&self) -> &str {
        &self.index_url
    }

    /// The position the next successful load restores, in CSS pixels; 0 means "do not restore".
    pub fn saved_scroll_y(&self) -> f64 {
        self.state.borrow().saved_scroll_y
    }

    /// True while the served HTML is newer than the page — the pane was collapsed when it changed.
    pub fn is_stale(&self) -> bool {
        self.state.borrow().stale
    }

    /// New text, title or appearance. `token` identifies the document: document windows pass
    /// `None`, the Book pane passes the article's path, and a change of it is a fresh page at the
    /// top rather than a scroll-preserving reload.
    pub fn update(&self, text: &str, title: &str, dark: bool, token: Option<&str>) {
        let new_document = {
            let state = self.state.borrow();
            let new_document = token != state.last_token.as_deref();
            // The Mac's "\(dark)|\(title)|\(text)" dedupe: a re-render that would produce the same
            // page is not one. A new document reloads even when the text is identical, because the
            // token moved.
            if !new_document
                && let Some((last_dark, last_title, last_text)) = &state.last_key
                && *last_dark == dark
                && last_title == title
                && last_text == text
            {
                return;
            }
            new_document
        };

        // Write first, remember after. If the writer panics on some input, a key committed up front
        // would make the identical retry a no-op and that document would never preview again. No
        // borrow is held while it runs.
        let html = screen_html::with_linux_fonts(&self.html.document(text, title, dark)).into_owned();

        {
            let mut state = self.state.borrow_mut();
            state.last_token = token.map(str::to_string);
            state.last_key = Some((dark, title.to_string(), text.to_string()));
        }
        self.surface.set_html(html);
        self.cancel_debounce();

        // Collapsed pane: the HTML is current, the page is not. show() loads it once.
        if !self.surface.is_shown() {
            self.state.borrow_mut().stale = true;
            return;
        }

        let first_load = {
            let mut state = self.state.borrow_mut();
            if !state.loaded_once || new_document {
                state.loaded_once = true;
                state.saved_scroll_y = 0.0;
                true
            } else {
                false
            }
        };
        if first_load {
            self.load_index();
            return;
        }

        let me = self.me.clone();
        let timer = self.scheduler.after(
            Self::DEBOUNCE_DELAY,
            Box::new(move || {
                if let Some(coordinator) = me.upgrade() {
                    coordinator.reload_preserving_scroll();
                }
            }),
        );
        self.state.borrow_mut().debounce = Some(timer);
    }

    /// The pane became visible again: load once if the page is behind the HTML.
    pub fn show(&self) {
        {
            let mut state = self.state.borrow_mut();
            if !state.stale {
                return;
            }
            state.loaded_once = true;
            state.saved_scroll_y = 0.0;
        }
        self.load_index();
    }

    /// The pane was collapsed. A reload that was still waiting is dropped — reloading a view nobody
    /// can see costs a full engine pass — and the page is left marked stale so
    /// [`show`](Self::show) does it once, later.
    pub fn hide(&self) {
        if self.state.borrow().debounce.is_none() {
            return;
        }
        self.cancel_debounce();
        self.state.borrow_mut().stale = true;
    }

    /// Scroll to a heading. Performed once per [`PreviewNavigation::id`]; a request that arrives
    /// while a load is in flight is parked and performed when that load finishes, because a script
    /// evaluated against the outgoing page scrolls nothing. `on_handled` is posted (never called
    /// inline) once the jump has been performed.
    pub fn navigate(&self, navigation: PreviewNavigation, on_handled: Option<NavigationHandled>) {
        {
            let mut state = self.state.borrow_mut();
            if state.last_navigation_id == Some(navigation.id) {
                return;
            }
            state.last_navigation_id = Some(navigation.id);
            if state.loading {
                state.parked = Some((navigation, on_handled));
                return;
            }
        }
        self.perform(navigation, on_handled);
    }

    fn perform(&self, navigation: PreviewNavigation, on_handled: Option<NavigationHandled>) {
        self.fire_and_forget(scripts::jump_to_slug(&navigation.slug));
        if let Some(on_handled) = on_handled {
            let id = navigation.id;
            self.scheduler.post(Box::new(move || on_handled(id)));
        }
    }

    fn load_index(&self) {
        self.begin_load();
        self.surface.navigate(&self.index_url);
    }

    /// The page is about to become the served HTML, so it is no longer behind it.
    fn begin_load(&self) {
        let mut state = self.state.borrow_mut();
        state.loading = true;
        state.stale = false;
    }

    /// The debounce fired: read the reader's place, then reload. The evaluation is awaited where it
    /// stands — synchronously when the surface answers at once, as C#'s `await` continues on a
    /// completed task, and on the spawner otherwise.
    fn reload_preserving_scroll(&self) {
        // The timer has fired; dropping its handle is a no-op.
        self.state.borrow_mut().debounce = None;
        let me = self.me.clone();
        run_now_or_spawn(&*self.spawner, self.surface.eval(scripts::SCROLL_Y.to_string()), move |json| {
            let Some(coordinator) = me.upgrade() else { return };
            // A page that cannot answer has no position worth keeping; reload anyway, as the Mac
            // does when evaluateJavaScript hands back an error.
            let y = json_script::number(&json).unwrap_or(0.0);
            coordinator.state.borrow_mut().saved_scroll_y = y;
            coordinator.begin_load();
            coordinator.surface.reload();
        });
    }

    fn on_navigation_completed(&self, is_success: bool) {
        let (restore, parked) = {
            let mut state = self.state.borrow_mut();
            state.loading = false;
            // A failed load has no DOM to scroll and no page to jump into; the parked request stays
            // parked for the load that succeeds.
            if !is_success {
                return;
            }
            (state.saved_scroll_y, state.parked.take())
        };

        if restore > 0.0 {
            self.fire_and_forget(scripts::scroll_to(restore));
        }
        // Straight to perform: the id was recorded when the request was parked.
        if let Some((navigation, on_handled)) = parked {
            self.perform(navigation, on_handled);
        }
    }

    fn cancel_debounce(&self) {
        // Taken out first and dropped after the borrow ends: dropping the handle cancels the timer.
        let timer = self.state.borrow_mut().debounce.take();
        drop(timer);
    }

    /// An evaluation whose answer nobody needs, driven to completion on the spawner (a lazily
    /// started evaluation must still run).
    fn fire_and_forget(&self, script: String) {
        let evaluation = self.surface.eval(script);
        self.spawner.spawn_local(Box::pin(async move {
            evaluation.await;
        }));
    }
}

impl Drop for PreviewCoordinator {
    fn drop(&mut self) {
        if let Some(handler) = self.handler.take() {
            self.surface.navigation_completed().disconnect(handler);
        }
    }
}

/// Polls `future` once; when it is already finished, `then` runs now, otherwise the rest is spawned.
fn run_now_or_spawn<T: 'static>(
    spawner: &dyn Spawner,
    future: LocalBoxFuture<'static, T>,
    then: impl FnOnce(T) + 'static,
) {
    let mut future = future;
    let ready = {
        let mut context = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    };
    match ready {
        Some(value) => then(value),
        None => spawner.spawn_local(Box::pin(async move { then(future.await) })),
    }
}
