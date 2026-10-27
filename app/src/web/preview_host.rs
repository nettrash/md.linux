//! `PreviewHost` — the live preview (shell-design §4.4–§4.9): one `webkit::WebView` on a
//! `preview-<n>` host of the `md-asset:` origin, md-logic's `PreviewSurface` for the
//! `PreviewCoordinator`, the injected scroll-sync and link-guard scripts, the `mdScroll` /
//! `mdRender` message handlers, `decide-policy` through md-logic's `LinkPolicy`, the Copy-only
//! context menu, the pinned zoom, the paper background and the retry policy.
//!
//! S0 moved the spike's policy glue here as the starting point (docs/linux-facts.md §3); the
//! 350 ms debounce and the scroll-keeping reload now live in md-logic's `PreviewCoordinator`
//! (`DEBOUNCE` below is the spike's measured value, kept for reference).
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::rc::Rc;
use std::time::Duration;

use md_logic::event::Event;
use md_logic::exec::{LocalBoxFuture, ready};
use md_logic::seams::PreviewSurface;
use webkit::prelude::*;

use crate::web::asset_host::SCHEME;
use crate::web::context::WebServices;

/// The spike's re-render debounce: one reload 353 ms after the last of 17 keystrokes (facts §2).
pub const DEBOUNCE: Duration = Duration::from_millis(350);

/// The spike's capture-phase guard (md.win §4.6), injected at document start. `javascript:` URLs
/// never reach `decide-policy` on WebKitGTK (facts §3), so this is what stops them; http(s) clicks
/// are left to the policy, which hands them to the browser. md-logic's `Scripts::LINK_GUARD` is
/// the shell's source.
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

/// The spike's decision (md-logic's `LinkDecision` is the shell's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkDecision {
    Allow,
    Cancel,
    OpenExternally,
}

/// md.win's `LinkPolicy.Decide` as the spike wrote it: our own page loads (fragment hops too);
/// any other page on our origin is cancelled; http(s) elsewhere opens in the browser; every other
/// scheme is cancelled.
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

/// The spike's `decide-policy` handler: navigation and new-window actions are decided, responses
/// get default handling. Returns whether the decision was taken.
pub fn decide_policy(
    view: &webkit::WebView,
    decision: &webkit::PolicyDecision,
    kind: webkit::PolicyDecisionType,
    index_url: &str,
) -> bool {
    if kind != webkit::PolicyDecisionType::NavigationAction && kind != webkit::PolicyDecisionType::NewWindowAction {
        return false;
    }
    let Some(navigation) = decision.downcast_ref::<webkit::NavigationPolicyDecision>() else {
        return false;
    };
    let Some(action) = navigation.navigation_action() else { return false };
    let uri = action.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
    match decide_link(&uri, index_url) {
        LinkDecision::Allow if kind == webkit::PolicyDecisionType::NavigationAction => decision.use_(),
        LinkDecision::OpenExternally => {
            decision.ignore();
            let root = view.root().and_downcast::<gtk::Window>();
            gtk::UriLauncher::new(&uri).launch(root.as_ref(), gio::Cancellable::NONE, |result| {
                if let Err(err) = result {
                    eprintln!("md: could not open link: {err}");
                }
            });
        }
        _ => decision.ignore(),
    }
    true
}

/// The guard as a document-start user script for the top frame.
pub fn link_guard_script() -> webkit::UserScript {
    webkit::UserScript::new(
        LINK_GUARD,
        webkit::UserContentInjectedFrames::TopFrame,
        webkit::UserScriptInjectionTime::Start,
        &[],
        &[],
    )
}

/// One window's (or the Book article's) live preview.
pub struct PreviewHost {
    web: Rc<WebServices>,
    did_scroll: Event<f64>,
    navigation_completed: Event<bool>,
}

impl PreviewHost {
    pub fn new(web: &Rc<WebServices>) -> Rc<PreviewHost> {
        Rc::new(PreviewHost { web: Rc::clone(web), did_scroll: Event::new(), navigation_completed: Event::new() })
    }

    pub fn widget(&self) -> gtk::Widget {
        gtk::Box::new(gtk::Orientation::Vertical, 0).into()
    }

    /// md-logic's seam over this preview, for the `PreviewCoordinator`.
    pub fn surface(self: &Rc<Self>) -> Rc<dyn PreviewSurface> {
        self.clone()
    }

    /// The paper background first, so the re-render never flashes (§4.8).
    pub fn apply_theme(&self, dark: bool) {}

    /// `__mdSyncScrollTo(f)`.
    pub fn apply_scroll_fraction(&self, fraction: f64) {}

    /// Args: the preview's scroll fraction (`mdScroll` messages, echoes dropped).
    pub fn did_scroll(&self) -> &Event<f64> {
        &self.did_scroll
    }

    /// The pane's own visibility, which `is_shown` reports.
    pub fn set_shown(&self, shown: bool) {}

    pub fn close(&self) {}
}

impl PreviewSurface for PreviewHost {
    fn html(&self) -> String {
        String::new()
    }

    fn set_html(&self, html: String) {}

    fn is_shown(&self) -> bool {
        false
    }

    fn navigate(&self, url: &str) {}

    fn reload(&self) {}

    fn eval(&self, script: String) -> LocalBoxFuture<'static, String> {
        ready(String::from("null"))
    }

    fn navigation_completed(&self) -> &Event<bool> {
        &self.navigation_completed
    }
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
