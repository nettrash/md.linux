//! The live preview: one `webkit::WebView` over the `md-asset:` origin, re-rendered on a 350 ms
//! trailing debounce with its scroll position carried across the reload (the Mac policy, as
//! md.win's `PreviewCoordinator` spells it), plus the link policy and the render-complete
//! signal the export pipeline waits on.

use crate::assets::{AssetHost, SCHEME};
use crate::util::{self, Sender};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};
use webkit::prelude::*;

pub const DEBOUNCE: Duration = Duration::from_millis(350);

/// Registered at document start in every preview (md.win §4.6). `javascript:` URLs never reach
/// `decide-policy` on WebKitGTK (measured, docs/linux-facts.md §3), so the guard is what stops
/// them; http(s) clicks are left to the policy handler, which hands them to the browser.
pub const LINK_GUARD: &str = r#"(function () {
  function guard(e) {
    var a = e.target && e.target.closest ? e.target.closest('a[href]') : null;
    if (!a) return;
    var href = (a.getAttribute('href') || '').trim().toLowerCase();
    if (href.charAt(0) === '#') return;
    if (href.indexOf('http://') === 0 || href.indexOf('https://') === 0) return;
    e.preventDefault(); e.stopImmediatePropagation();
  }
  document.addEventListener('click', guard, true);
  document.addEventListener('auxclick', guard, true);
})();"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkDecision {
    Allow,
    Cancel,
    OpenExternally,
}

/// md.win's `LinkPolicy.Decide`: our own page loads; any other page on our origin is
/// cancelled; http(s) elsewhere opens in the browser; every other scheme is cancelled.
pub fn decide_link(uri: &str, index_url: &str) -> LinkDecision {
    let without_fragment = uri.split('#').next().unwrap_or(uri);
    if without_fragment == index_url {
        return LinkDecision::Allow;
    }
    let lower = uri.to_ascii_lowercase();
    if lower.starts_with(&format!("{SCHEME}:")) {
        return LinkDecision::Cancel;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return LinkDecision::OpenExternally;
    }
    LinkDecision::Cancel
}

#[derive(Debug, Default)]
pub struct PreviewStats {
    /// Debounced reloads started (not counting first loads / new documents).
    pub reloads: u32,
    pub loads_finished: u32,
    pub last_update: Option<Instant>,
    pub last_reload_started: Option<Instant>,
    /// `window.scrollY` read just before the last debounced reload.
    pub saved_scroll: f64,
    /// `window.scrollY` at `LoadEvent::Finished`, before our restore ran.
    pub scroll_at_finished: f64,
    /// Every navigation `decide-policy` saw: (uri, decision type, user gesture, our decision).
    pub decisions: Vec<(String, String, bool, LinkDecision)>,
    /// `mdRender` script messages (md-init.js posts 'complete' through
    /// `window.webkit.messageHandlers.mdRender`, exactly as on the Mac).
    pub render_messages: Vec<Instant>,
    /// `web-process-terminated` signals (a crashed or killed web process).
    pub terminations: Vec<String>,
}

pub struct Preview {
    pub view: webkit::WebView,
    pub host: Rc<AssetHost>,
    pub key: String,
    pub ucm: webkit::UserContentManager,
    debounce: RefCell<Option<glib::SourceId>>,
    loaded_once: Cell<bool>,
    restore: Cell<f64>,
    pub stats: RefCell<PreviewStats>,
    load_waiters: RefCell<Vec<Sender<()>>>,
    /// Launch http(s) links in the browser (off in the spike: there is no browser to start).
    pub launch_external: Cell<bool>,
}

pub fn settings() -> webkit::Settings {
    let settings = webkit::Settings::new();
    settings.set_enable_developer_extras(cfg!(debug_assertions));
    settings.set_enable_write_console_messages_to_stdout(true);
    settings.set_javascript_can_open_windows_automatically(false);
    settings.set_enable_back_forward_navigation_gestures(false);
    settings.set_enable_webaudio(false);
    settings.set_enable_webrtc(false);
    settings.set_enable_media_stream(false);
    settings.set_print_backgrounds(true);
    settings
}

impl Preview {
    pub fn new(ctx: &webkit::WebContext, host: Rc<AssetHost>, key: &str, guard: bool) -> Rc<Preview> {
        let ucm = webkit::UserContentManager::new();
        if guard {
            ucm.add_script(&link_guard_script());
        }
        ucm.register_script_message_handler("mdRender", None);
        let view = webkit::WebView::builder().web_context(ctx).user_content_manager(&ucm).settings(&settings()).build();
        view.set_hexpand(true);
        view.set_vexpand(true);
        let preview = Rc::new(Preview {
            view,
            host,
            key: key.to_owned(),
            ucm,
            debounce: RefCell::new(None),
            loaded_once: Cell::new(false),
            restore: Cell::new(0.0),
            stats: RefCell::new(PreviewStats::default()),
            load_waiters: RefCell::new(Vec::new()),
            launch_external: Cell::new(true),
        });
        preview.connect();
        preview
    }

    pub fn index_url(&self) -> String {
        AssetHost::index_url(&self.key)
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.ucm.connect_script_message_received(Some("mdRender"), move |_, _value| {
            if let Some(p) = weak.upgrade() {
                p.stats.borrow_mut().render_messages.push(Instant::now());
            }
        });

        let weak = Rc::downgrade(self);
        self.view.connect_load_changed(move |view, event| {
            let Some(p) = weak.upgrade() else { return };
            if event != webkit::LoadEvent::Finished {
                return;
            }
            p.stats.borrow_mut().loads_finished += 1;
            let restore = p.restore.replace(0.0);
            let view = view.clone();
            let weak = Rc::downgrade(&p);
            glib::spawn_future_local(async move {
                let at_finished = util::eval_number(&view, "window.scrollY").await;
                if let Some(p) = weak.upgrade() {
                    p.stats.borrow_mut().scroll_at_finished = at_finished;
                }
                if restore > 0.0 {
                    let _ = util::eval_json(&view, &format!("window.scrollTo(0, {restore}); window.scrollY")).await;
                }
                if let Some(p) = weak.upgrade() {
                    for waiter in p.load_waiters.borrow_mut().drain(..) {
                        waiter.send(());
                    }
                }
            });
        });

        let weak = Rc::downgrade(self);
        self.view.connect_decide_policy(move |_, decision, kind| {
            let Some(p) = weak.upgrade() else { return false };
            p.decide(decision, kind)
        });

        // A link with target=_blank or window.open: never a new web view.
        self.view.connect_create(|_, _| None);

        let weak = Rc::downgrade(self);
        self.view.connect_web_process_terminated(move |_, reason| {
            let Some(p) = weak.upgrade() else { return };
            let count = {
                let mut stats = p.stats.borrow_mut();
                stats.terminations.push(format!("{reason:?}"));
                stats.terminations.len()
            };
            // Reload after a crash, but not forever: a web process that cannot start at all
            // (no sandbox, no GPU) would otherwise loop.
            eprintln!("md: preview web process terminated ({reason:?}), #{count}");
            if count <= 3 {
                p.view.load_uri(&p.index_url());
            }
        });
    }

    fn decide(&self, decision: &webkit::PolicyDecision, kind: webkit::PolicyDecisionType) -> bool {
        let kind_name = match kind {
            webkit::PolicyDecisionType::NavigationAction => "navigation",
            webkit::PolicyDecisionType::NewWindowAction => "new-window",
            _ => return false, // responses: default handling
        };
        let Some(nav) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = nav.navigation_action() else { return false };
        let uri = action.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
        let verdict = decide_link(&uri, &self.index_url());
        self.stats.borrow_mut().decisions.push((uri.clone(), kind_name.to_owned(), action.is_user_gesture(), verdict));
        match verdict {
            LinkDecision::Allow if kind == webkit::PolicyDecisionType::NavigationAction => decision.use_(),
            LinkDecision::OpenExternally => {
                decision.ignore();
                if self.launch_external.get() {
                    let root = self.view.root().and_downcast::<gtk::Window>();
                    gtk::UriLauncher::new(&uri).launch(root.as_ref(), gio::Cancellable::NONE, |res| {
                        if let Err(err) = res {
                            eprintln!("md: could not open link: {err}");
                        }
                    });
                }
            }
            _ => decision.ignore(),
        }
        true
    }

    /// A promise for the next `LoadEvent::Finished` (after any scroll restore). Take it before
    /// starting the load.
    pub fn next_load(&self) -> util::Receiver<()> {
        let (tx, rx) = util::oneshot();
        self.load_waiters.borrow_mut().push(tx);
        rx
    }

    /// New text for the page. A first load or a new document navigates now; an edit to the same
    /// document restarts the 350 ms debounce, and the reload keeps the scroll position.
    pub fn update(self: &Rc<Self>, html: String, new_document: bool) {
        self.host.set_page(&self.key, html);
        self.stats.borrow_mut().last_update = Some(Instant::now());
        if let Some(id) = self.debounce.borrow_mut().take() {
            id.remove();
        }
        if !self.loaded_once.get() || new_document {
            self.loaded_once.set(true);
            self.restore.set(0.0);
            self.view.load_uri(&self.index_url());
            return;
        }
        let weak: Weak<Preview> = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            let Some(p) = weak.upgrade() else { return };
            p.debounce.borrow_mut().take();
            glib::spawn_future_local(async move { p.reload_preserving_scroll().await });
        });
        *self.debounce.borrow_mut() = Some(id);
    }

    async fn reload_preserving_scroll(self: Rc<Self>) {
        let y = util::eval_number(&self.view, "window.scrollY").await;
        let y = if y.is_finite() { y } else { 0.0 };
        {
            let mut stats = self.stats.borrow_mut();
            stats.reloads += 1;
            stats.last_reload_started = Some(Instant::now());
            stats.saved_scroll = y;
        }
        self.restore.set(y);
        self.view.reload();
    }

    /// Polls md-init.js's `<html data-md-render-complete="1">` every 250 ms (md.win §4.8:
    /// 480 attempts, timeout counts as success). Returns the time waited and whether the flag
    /// was seen.
    pub async fn wait_render_complete(&self, limit: Duration) -> (Duration, bool) {
        let start = Instant::now();
        loop {
            let flag = util::eval_json(&self.view, "document.documentElement.getAttribute('data-md-render-complete')")
                .await
                .unwrap_or_default();
            if flag == "\"1\"" {
                return (start.elapsed(), true);
            }
            if start.elapsed() >= limit {
                return (start.elapsed(), false);
            }
            util::sleep_ms(250).await;
        }
    }
}

pub fn link_guard_script() -> webkit::UserScript {
    webkit::UserScript::new(
        LINK_GUARD,
        webkit::UserContentInjectedFrames::TopFrame,
        webkit::UserScriptInjectionTime::Start,
        &[],
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_matrix() {
        let index = "md-asset://preview-1/index.html";
        assert_eq!(decide_link(index, index), LinkDecision::Allow);
        assert_eq!(decide_link("md-asset://preview-1/index.html#slug", index), LinkDecision::Allow);
        assert_eq!(decide_link("md-asset://preview-1/other.md", index), LinkDecision::Cancel);
        assert_eq!(decide_link("md-asset://export-2/index.html", index), LinkDecision::Cancel);
        assert_eq!(decide_link("https://example.com/", index), LinkDecision::OpenExternally);
        assert_eq!(decide_link("HTTP://example.com/", index), LinkDecision::OpenExternally);
        assert_eq!(decide_link("javascript:alert(1)", index), LinkDecision::Cancel);
        assert_eq!(decide_link("file:///etc/passwd", index), LinkDecision::Cancel);
        assert_eq!(decide_link("mailto:a@b.c", index), LinkDecision::Cancel);
    }
}
