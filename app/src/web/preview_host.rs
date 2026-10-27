//! `PreviewHost` — the live preview (shell-design §4.4–§4.9): one `webkit::WebView` on a
//! `preview-<n>` host of the `md-asset:` origin and md-logic's `PreviewSurface` over it, for the
//! `PreviewCoordinator` that owns the re-render policy (350 ms debounce, scroll kept, first load and
//! a new document navigate).
//!
//! What the host adds around the view:
//! - **Injected scripts** (document start, top frame, not part of the DOM so no export carries
//!   them): the Mac's scroll-sync script byte for byte (`scripts::SCROLL_SYNC`) and md.win's
//!   capture-phase link guard (`scripts::LINK_GUARD`) — WebKitGTK runs a clicked `javascript:` href
//!   in-page without asking `decide-policy`, so the guard is what stops it, and the CSP on
//!   `index.html` stops inline handlers.
//! - **Messages**: `mdScroll` → `json_script::scroll_message` → echoes dropped → [`did_scroll`];
//!   `mdRender` (md-init.js's `'complete'`, the Mac's event path, facts §4) → the retry policy.
//! - **Navigation policy**: `decide-policy` for navigation and new-window actions through
//!   md-logic's `link_policy::decide`; http(s) the user clicked opens in the browser
//!   (`gtk::UriLauncher`); nothing ever opens a new web view.
//! - The context menu keeps Copy and Select All; the zoom is pinned at 1 (WebKitGTK zooms on a
//!   touchpad pinch, WKWebView does not); the paper colour is the view's background, so a reload
//!   never flashes white.
//! - **The web process dying** (§4.9): `Crashed` and `ExceededMemoryLimit` go to
//!   [`RetryPolicy`] — reload once, give up with the Mac's quiet line on the second death in a row,
//!   an edit buys one more attempt, a completed render resets the count. `TerminatedByApi` is md's
//!   own doing and "unresponsive" is never acted on.
//!
//! [`did_scroll`]: PreviewHost::did_scroll
// P4 → P1 / P5: until the document window and the book pane construct previews, only the self-test
// does; removed at the integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use gtk::gdk;
use md_logic::event::Event;
use md_logic::exec::LocalBoxFuture;
use md_logic::preview::{
    LinkDecision, PreviewCoordinator, asset_origin, json_script, link_policy, screen_html, scripts,
};
use md_logic::seams::{PreviewSurface, Scheduler};
use md_logic::settings::Palette;
use webkit::prelude::*;

use crate::app::Shell;
use crate::web::context::WebServices;
use crate::web::js;
use crate::web::retry_policy::{NOTICE_SOURCE, RetryPolicy, RetryResponse};

/// One `decide-policy` verdict, for the self-test: the URI, whether the user started it, and what
/// md-logic's policy said.
#[cfg(feature = "selftest")]
#[derive(Clone, Debug)]
pub struct Decision {
    pub uri: String,
    pub user_gesture: bool,
    pub new_window: bool,
    pub decision: LinkDecision,
}

/// One window's (or the Book article's) live preview.
pub struct PreviewHost {
    me: Weak<PreviewHost>,
    web: Rc<WebServices>,
    view: webkit::WebView,
    user_content: webkit::UserContentManager,
    host_name: String,
    index_url: String,
    shown: Cell<bool>,
    closed: Cell<bool>,
    /// The last document's title and theme, read off its page — what the give-up notice is
    /// rendered with, so it lands on the same paper.
    title: RefCell<String>,
    dark: Cell<bool>,
    retry: Cell<RetryPolicy>,
    /// The pane shows the give-up notice: its own render is not the document's.
    showing_notice: Cell<bool>,
    /// The load in flight failed: its `Finished` is not a success.
    load_failed: Cell<bool>,
    did_scroll: Event<f64>,
    navigation_completed: Event<bool>,
    #[cfg(feature = "selftest")]
    probe: Probe,
}

/// What the self-test watches.
#[cfg(feature = "selftest")]
#[derive(Default)]
pub struct Probe {
    pub decisions: RefCell<Vec<Decision>>,
    /// `mdRender` messages received (the notice's own included).
    pub renders: Cell<u32>,
    pub loads_started: Cell<u32>,
    pub loads_finished: Cell<u32>,
    /// What each counted web-process death led to.
    pub terminations: RefCell<Vec<RetryResponse>>,
    /// Items the last context menu was left with.
    pub menu_items: RefCell<Option<u32>>,
}

impl PreviewHost {
    pub fn new(web: &Rc<WebServices>) -> Rc<PreviewHost> {
        let user_content = webkit::UserContentManager::new();
        user_content.add_script(&document_start_script(scripts::SCROLL_SYNC));
        user_content.add_script(&document_start_script(scripts::LINK_GUARD));
        user_content.register_script_message_handler(scripts::SCROLL_MESSAGE_HANDLER, None);
        user_content.register_script_message_handler(scripts::RENDER_MESSAGE_HANDLER, None);
        let view = web.new_view(&user_content);
        view.set_hexpand(true);
        view.set_vexpand(true);
        let host_name = web.next_preview_host();
        let index_url = asset_origin::index_url(&host_name);
        let host = Rc::new_cyclic(|me| PreviewHost {
            me: me.clone(),
            web: Rc::clone(web),
            view,
            user_content,
            host_name,
            index_url,
            shown: Cell::new(true),
            closed: Cell::new(false),
            title: RefCell::new(String::new()),
            dark: Cell::new(false),
            retry: Cell::new(RetryPolicy::new()),
            showing_notice: Cell::new(false),
            load_failed: Cell::new(false),
            did_scroll: Event::new(),
            navigation_completed: Event::new(),
            #[cfg(feature = "selftest")]
            probe: Probe::default(),
        });
        host.apply_theme(crate::theme::is_dark());
        host.connect();
        host
    }

    pub fn widget(&self) -> gtk::Widget {
        self.view.clone().upcast()
    }

    /// md-logic's seam over this preview, for the `PreviewCoordinator`.
    pub fn surface(self: &Rc<Self>) -> Rc<dyn PreviewSurface> {
        self.clone()
    }

    /// This surface's document URL, `md-asset://preview-<n>/index.html` — the coordinator's
    /// `index_url`.
    pub fn index_url(&self) -> &str {
        &self.index_url
    }

    /// md-logic's coordinator over this preview on the shell's scheduler and spawner — what the
    /// document window and the book pane drive (`update`, `show`, `hide`, `navigate`).
    pub fn coordinator(self: &Rc<Self>, shell: &Shell) -> Rc<PreviewCoordinator> {
        let scheduler: Rc<dyn Scheduler> = shell.scheduler.clone();
        PreviewCoordinator::new(self.surface(), scheduler, shell.spawner.clone(), self.index_url.clone())
    }

    /// The paper colour behind the page, set before the re-render so it never flashes (§4.8). The
    /// page's own theme comes from the next `coordinator.update(…, dark, …)`.
    pub fn apply_theme(&self, dark: bool) {
        let (a, r, g, b) = Palette::channels(Palette::for_dark(dark).paper);
        let channel = |c: u8| f32::from(c) / 255.0;
        self.view.set_background_color(&gdk::RGBA::new(channel(r), channel(g), channel(b), channel(a)));
    }

    /// Editor → preview: `__mdSyncScrollTo(f)`, marked programmatic by the script so its echo does
    /// not come back.
    pub fn apply_scroll_fraction(&self, fraction: f64) {
        if self.closed.get() || !fraction.is_finite() {
            return;
        }
        let view = self.view.clone();
        glib::spawn_future_local(async move {
            let _ = js::eval_json(&view, &scripts::sync_scroll_to(fraction)).await;
        });
    }

    /// Args: the preview's scroll fraction when the reader scrolled it (echoes of the editor's own
    /// sets, heading jumps and reload restores are dropped).
    pub fn did_scroll(&self) -> &Event<f64> {
        &self.did_scroll
    }

    /// The pane's own visibility, which `is_shown` reports (the coordinator only marks the page
    /// stale while it is false).
    pub fn set_shown(&self, shown: bool) {
        self.shown.set(shown);
    }

    /// The pane is going: nothing loads any more and its host serves nothing.
    pub fn close(&self) {
        if self.closed.replace(true) {
            return;
        }
        self.view.stop_loading();
        self.web.asset_host().remove_page(&self.host_name);
    }

    #[cfg(feature = "selftest")]
    pub fn view(&self) -> &webkit::WebView {
        &self.view
    }

    #[cfg(feature = "selftest")]
    pub fn host_name(&self) -> &str {
        &self.host_name
    }

    #[cfg(feature = "selftest")]
    pub fn probe(&self) -> &Probe {
        &self.probe
    }

    #[cfg(feature = "selftest")]
    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry.get()
    }

    fn connect(&self) {
        let me = self.me.clone();
        self.user_content.connect_script_message_received(Some(scripts::SCROLL_MESSAGE_HANDLER), move |_, value| {
            let Some(host) = me.upgrade() else { return };
            let json = value.to_json(0).map(|s| s.to_string()).unwrap_or_default();
            if let Some(message) = json_script::scroll_message(&json)
                && !message.echo
            {
                host.did_scroll.emit(&message.fraction);
            }
        });

        let me = self.me.clone();
        self.user_content.connect_script_message_received(Some(scripts::RENDER_MESSAGE_HANDLER), move |_, _| {
            if let Some(host) = me.upgrade() {
                host.render_did_complete();
            }
        });

        let me = self.me.clone();
        self.view.connect_load_changed(move |_, event| {
            let Some(host) = me.upgrade() else { return };
            match event {
                webkit::LoadEvent::Started => {
                    host.load_failed.set(false);
                    #[cfg(feature = "selftest")]
                    host.probe.loads_started.set(host.probe.loads_started.get() + 1);
                }
                webkit::LoadEvent::Finished => {
                    // A failed load is followed by `Finished` too; it has already been reported.
                    if !host.load_failed.replace(false) {
                        #[cfg(feature = "selftest")]
                        host.probe.loads_finished.set(host.probe.loads_finished.get() + 1);
                        host.navigation_completed.emit(&true);
                    }
                }
                _ => {}
            }
        });

        let me = self.me.clone();
        self.view.connect_load_failed(move |_, _, uri, error| {
            let Some(host) = me.upgrade() else { return false };
            if !error.matches(webkit::NetworkError::Cancelled) {
                eprintln!("md: the preview could not load {uri}: {}", error.message());
            }
            host.load_failed.set(true);
            host.navigation_completed.emit(&false);
            // No error page: the pane keeps what it showed, as the Mac's does.
            true
        });

        let me = self.me.clone();
        self.view.connect_decide_policy(move |_, decision, kind| match me.upgrade() {
            Some(host) => host.decide(decision, kind),
            None => false,
        });

        // A link with target=_blank or window.open(): never a new web view.
        self.view.connect_create(|_, _| None);

        // WKWebView's menu has Copy (and the reader can Select All); Reload, Back and Inspect go.
        #[cfg(feature = "selftest")]
        let me = self.me.clone();
        self.view.connect_context_menu(move |_, menu, _| {
            for item in menu.items() {
                if !matches!(
                    item.stock_action(),
                    webkit::ContextMenuAction::Copy | webkit::ContextMenuAction::SelectAll
                ) {
                    menu.remove(&item);
                }
            }
            #[cfg(feature = "selftest")]
            if let Some(host) = me.upgrade() {
                host.probe.menu_items.replace(Some(menu.n_items()));
            }
            // An empty menu is not shown at all.
            menu.n_items() == 0
        });

        self.view.connect_zoom_level_notify(|view| {
            if (view.zoom_level() - 1.0).abs() > f64::EPSILON {
                view.set_zoom_level(1.0);
            }
        });

        let me = self.me.clone();
        self.view.connect_web_process_terminated(move |_, reason| {
            if let Some(host) = me.upgrade() {
                host.process_terminated(reason);
            }
        });
    }

    fn decide(&self, decision: &webkit::PolicyDecision, kind: webkit::PolicyDecisionType) -> bool {
        let new_window = match kind {
            webkit::PolicyDecisionType::NavigationAction => false,
            webkit::PolicyDecisionType::NewWindowAction => true,
            // Responses: default handling.
            _ => return false,
        };
        let Some(navigation) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else { return false };
        let Some(action) = navigation.navigation_action() else { return false };
        let uri = action.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
        let user_gesture = action.is_user_gesture();
        let verdict = link_policy::decide(&uri, user_gesture, &self.index_url);
        #[cfg(feature = "selftest")]
        self.probe.decisions.borrow_mut().push(Decision {
            uri: uri.clone(),
            user_gesture,
            new_window,
            decision: verdict,
        });
        match verdict {
            LinkDecision::Allow if !new_window => decision.use_(),
            LinkDecision::OpenExternally => {
                decision.ignore();
                let root = self.view.root().and_downcast::<gtk::Window>();
                gtk::UriLauncher::new(&uri).launch(root.as_ref(), gio::Cancellable::NONE, |result| {
                    if let Err(err) = result {
                        eprintln!("md: could not open the link: {err}");
                    }
                });
            }
            _ => decision.ignore(),
        }
        true
    }

    /// md-init.js finished: math, highlighting and every engine ran and the page survived them —
    /// a rendered document, unless it is the notice's own page.
    fn render_did_complete(&self) {
        #[cfg(feature = "selftest")]
        self.probe.renders.set(self.probe.renders.get() + 1);
        if self.showing_notice.get() {
            return;
        }
        let mut retry = self.retry.get();
        retry.render_did_finish();
        self.retry.set(retry);
    }

    fn process_terminated(&self, reason: webkit::WebProcessTerminationReason) {
        match reason {
            webkit::WebProcessTerminationReason::Crashed | webkit::WebProcessTerminationReason::ExceededMemoryLimit => {
            }
            // TerminatedByApi is md's own doing; anything newer is not ours to guess at.
            _ => return,
        }
        if self.closed.get() {
            return;
        }
        let mut retry = self.retry.get();
        let response = retry.content_process_did_terminate();
        self.retry.set(retry);
        eprintln!("md: the preview's web process ended ({reason:?}): {response:?}");
        #[cfg(feature = "selftest")]
        self.probe.terminations.borrow_mut().push(response);
        match response {
            // `reload()` is not dependable on a view whose process has gone; a fresh load is.
            RetryResponse::Reload => self.load_index(),
            RetryResponse::GiveUp => {
                self.showing_notice.set(true);
                let notice =
                    md_core::markdown::html::document(NOTICE_SOURCE, &self.title.borrow(), self.dark.get(), false);
                self.web.asset_host().set_page(&self.host_name, screen_html::with_linux_fonts(&notice).into_owned());
                self.load_index();
            }
            RetryResponse::Ignore => {}
        }
    }

    fn load_index(&self) {
        if self.closed.get() || !self.web.can_load() {
            return;
        }
        self.view.load_uri(&self.index_url);
    }
}

impl PreviewSurface for PreviewHost {
    fn html(&self) -> String {
        self.web.asset_host().page(&self.host_name).unwrap_or_default()
    }

    /// A new page — a change of text, title, theme or article: the load that follows is one more
    /// attempt for the retry policy, and the pane stops showing the notice.
    fn set_html(&self, html: String) {
        *self.title.borrow_mut() = page_title(&html);
        self.dark.set(page_is_dark(&html));
        let mut retry = self.retry.get();
        retry.document_did_change();
        self.retry.set(retry);
        self.showing_notice.set(false);
        self.web.asset_host().set_page(&self.host_name, html);
    }

    fn is_shown(&self) -> bool {
        self.shown.get()
    }

    fn navigate(&self, url: &str) {
        if self.closed.get() || !self.web.can_load() {
            return;
        }
        self.view.load_uri(url);
    }

    fn reload(&self) {
        if self.closed.get() || !self.web.can_load() {
            return;
        }
        self.view.reload();
    }

    fn eval(&self, script: String) -> LocalBoxFuture<'static, String> {
        let view = self.view.clone();
        Box::pin(async move { js::eval_json(&view, &script).await.unwrap_or_else(|_| "null".to_owned()) })
    }

    fn navigation_completed(&self) -> &Event<bool> {
        &self.navigation_completed
    }
}

/// A document-start user script for the top frame.
fn document_start_script(source: &str) -> webkit::UserScript {
    webkit::UserScript::new(
        source,
        webkit::UserContentInjectedFrames::TopFrame,
        webkit::UserScriptInjectionTime::Start,
        &[],
        &[],
    )
}

/// The `<title>` md-core wrote, unescaped (md-core escapes `&`, `<`, `>` and `"`); empty when there
/// is none.
fn page_title(html: &str) -> String {
    let Some(start) = html.find("<title>").map(|at| at + "<title>".len()) else { return String::new() };
    let Some(length) = html[start..].find("</title>") else { return String::new() };
    html[start..start + length].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// Whether md-core wrote the page dark (`<body data-md-dark="1">`).
fn page_is_dark(html: &str) -> bool {
    html.contains("<body data-md-dark=\"1\">")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_keeps_the_documents_title_and_theme() {
        let title = "Fish & <Chips> \"daily\"";
        let html = md_core::markdown::html::document("# Hi", title, true, false);
        assert_eq!(page_title(&html), title);
        assert!(page_is_dark(&html));
        let light = md_core::markdown::html::document("# Hi", "Plain", false, false);
        assert_eq!(page_title(&light), "Plain");
        assert!(!page_is_dark(&light));
        assert_eq!(page_title("<p>no head</p>"), "");
    }

    #[test]
    fn the_link_policy_is_md_logics() {
        let index = asset_origin::index_url("preview-1");
        let decide = |uri: &str, gesture: bool| link_policy::decide(uri, gesture, &index);
        assert_eq!(decide(&index, false), LinkDecision::Allow);
        assert_eq!(decide("md-asset://preview-1/index.html#slug", true), LinkDecision::Allow);
        assert_eq!(decide("md-asset://preview-1/other.md", true), LinkDecision::Cancel);
        assert_eq!(decide("md-asset://export-2/index.html", true), LinkDecision::Cancel);
        assert_eq!(decide("https://example.com/", true), LinkDecision::OpenExternally);
        assert_eq!(decide("https://example.com/", false), LinkDecision::Cancel);
        assert_eq!(decide("javascript:alert(1)", true), LinkDecision::Cancel);
        assert_eq!(decide("file:///etc/passwd", true), LinkDecision::Cancel);
        assert_eq!(decide("mailto:a@b.c", true), LinkDecision::Cancel);
    }
}
