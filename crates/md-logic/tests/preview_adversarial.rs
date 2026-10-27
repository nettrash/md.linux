//! Port of md.win `tests/Md.App.Logic.Tests/Preview/PreviewAdversarialTests.cs`: the traps aimed
//! at this package — a script result that is a number no script can be handed back, a link that
//! spoofs our own host, a render-complete poll that must not mistake a timeout for success, an HTML
//! writer that panics, and a document swap behind a collapsed pane that must not remember the old
//! article's scroll position.
//!
//! LINUX: md.win's `https://md.assets/…` is `md-asset://preview-1/…`; a C# exception from the writer
//! is a Rust panic, caught with `catch_unwind`.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use md_logic::preview::asset_origin::{index_url, preview_host};
use md_logic::preview::json_script;
use md_logic::preview::link_policy::{LinkDecision, decide};
use md_logic::preview::scripts::{self, RENDER_COMPLETE_RESULT, SCROLL_Y};
use md_logic::preview::{DocumentHtml, PreviewCoordinator};
use md_logic::testing::*;

#[derive(Default)]
struct StubHtml {
    calls: Cell<usize>,
    panics: Cell<bool>,
}

impl DocumentHtml for StubHtml {
    fn document(&self, source: &str, title: &str, _dark: bool) -> String {
        self.calls.set(self.calls.get() + 1);
        if self.panics.get() {
            panic!("no writer");
        }
        format!("<html><head><title>{title}</title></head><body>{source}</body></html>")
    }
}

fn index() -> String {
    index_url(&preview_host(1))
}

fn coordinator(
    surface: &Rc<FakePreviewSurface>,
    scheduler: &Rc<FakeScheduler>,
    html: &Rc<StubHtml>,
) -> Rc<PreviewCoordinator> {
    PreviewCoordinator::with_html(
        surface.clone(),
        scheduler.clone(),
        Rc::new(LocalExecutor::new()),
        html.clone(),
        index(),
    )
}

// ───────────────── a number that is not a number ─────────────────

#[test]
fn an_infinite_scroll_position_is_no_answer() {
    for json in ["1e999", "-1e999"] {
        // JSON has no infinity literal, but the double parse of that many zeroes overflows to one.
        // The finiteness guard is what stops it reaching a script: .NET spells infinity "Infinity"
        // (so does the port's formatter), which either scrolls to the bottom or is a syntax error,
        // and both fail in silence.
        assert_eq!(json_script::number(json), None, "{json}");
    }
    let would_have_been_emitted = scripts::scroll_to(f64::INFINITY);
    assert!(
        would_have_been_emitted.contains("Infinity") || would_have_been_emitted.contains('\u{221E}'),
        "infinity is not a usable scroll target, however it is spelled: {would_have_been_emitted}"
    );
}

#[test]
fn a_finite_but_enormous_scroll_position_still_decodes() {
    assert_eq!(json_script::number("1e308"), Some(1e308));
}

// ───────────────── the render-complete flag (the export poller's contract) ─────────────────

#[test]
fn only_the_quoted_one_is_render_complete() {
    for result in [
        "null",  // the script failed, or the page went away mid-poll
        "1",     // the JSON number one, not the attribute
        "\"0\"", // md-init.js has not finished
        "\"\"",
        "undefined", // not JSON at all
    ] {
        assert_ne!(RENDER_COMPLETE_RESULT, result);
        assert_ne!(json_script::string(result).as_deref(), Some("1"), "{result}");
    }
}

#[test]
fn the_render_complete_flag_decodes_to_the_string_one() {
    // A poller that compares the raw JSON must compare against RENDER_COMPLETE_RESULT; one that
    // decodes must compare against "1". Both are pinned so a timeout stays distinguishable from
    // success — §4.8 makes a timeout succeed anyway, which is exactly why a false *positive*
    // (proceeding while a diagram is still drawing) would never be noticed.
    assert_eq!(json_script::string(RENDER_COMPLETE_RESULT).as_deref(), Some("1"));
    assert_eq!(RENDER_COMPLETE_RESULT, "\"1\"");
}

// ───────────────── links that spoof our own origin ─────────────────

#[test]
fn an_uppercased_script_scheme_is_still_cancelled() {
    for uri in ["JAVASCRIPT:alert(1)", "jAvAsCrIpT:alert(document.cookie)", "vbscript:msgbox(1)"] {
        assert_eq!(decide(uri, true, &index()), LinkDecision::Cancel, "{uri}");
    }
}

#[test]
fn userinfo_that_looks_like_our_host_does_not_buy_trust() {
    // https://preview-1@evil.com/ has host "evil.com" and userinfo "preview-1". Treating it as ours
    // would let a rendered document navigate the preview to a stranger's page in-pane.
    assert_eq!(decide("https://preview-1@evil.com/", true, &index()), LinkDecision::OpenExternally);
    assert_eq!(decide("md-asset://preview-1@evil.com/index.html", true, &index()), LinkDecision::Cancel);
}

#[test]
fn a_lookalike_host_goes_to_the_browser_and_never_into_the_pane() {
    for uri in [
        "https://preview-1.evil.com/index.html", // suffix, not our host
        "https://notpreview-1/index.html",       // prefix, not our host
    ] {
        assert_eq!(decide(uri, true, &index()), LinkDecision::OpenExternally, "{uri}");
    }
}

#[test]
fn our_host_on_another_port_is_still_cancelled_not_launched() {
    assert_eq!(decide("md-asset://preview-1:8443/other.md", true, &index()), LinkDecision::Cancel);
    assert_eq!(decide("https://preview-1:8443/other.md", true, &index()), LinkDecision::Cancel);
}

#[test]
fn the_default_port_spelling_of_our_own_document_is_still_ours() {
    // md.win: https://md.assets:443/index.html is the same document as https://md.assets/index.html.
    // LINUX: md-asset has no default port, so a port spelling is not our document — but it is our
    // host, so it is cancelled, never launched. The http(s) default-port rule itself still holds
    // for an https index.
    assert_eq!(decide("md-asset://preview-1:443/index.html", false, &index()), LinkDecision::Cancel);
    assert_eq!(decide("https://md.assets:443/index.html", false, "https://md.assets/index.html"), LinkDecision::Allow);
}

// ───────────────── an HTML writer that panics ─────────────────

#[test]
fn a_writer_that_threw_does_not_poison_the_dedupe_key() {
    // The one update that panics must not be remembered as rendered: md-core's writer could panic
    // on its own, and the identical retry has to produce a page rather than be deduped into silence.
    let surface = Rc::new(FakePreviewSurface::new());
    let html = Rc::new(StubHtml { panics: Cell::new(true), ..StubHtml::default() });
    let c = coordinator(&surface, &Rc::new(FakeScheduler::new()), &html);

    assert!(catch_unwind(AssertUnwindSafe(|| c.update("a", "Doc", false, None))).is_err());
    assert!(surface.html_history().is_empty());
    assert!(surface.navigations().is_empty());

    html.panics.set(false);
    c.update("a", "Doc", false, None);

    assert!(surface.html().contains('a'));
    assert_eq!(surface.navigations(), vec![index()]);
}

#[test]
fn a_writer_that_threw_does_not_poison_the_token_either() {
    let surface = Rc::new(FakePreviewSurface::new());
    let html = Rc::new(StubHtml { panics: Cell::new(true), ..StubHtml::default() });
    let c = coordinator(&surface, &Rc::new(FakeScheduler::new()), &html);

    assert!(catch_unwind(AssertUnwindSafe(|| c.update("a", "Doc", false, Some("one.md")))).is_err());

    html.panics.set(false);
    c.update("a", "Doc", false, Some("one.md"));
    surface.complete_navigation(true);

    // Still the first document as far as the coordinator is concerned: the next article is new.
    c.update("b", "Doc", false, Some("two.md"));
    assert_eq!(surface.navigations().len(), 2);
}

// ───────────────── a document swap behind a collapsed pane ─────────────────

#[test]
fn switching_article_while_collapsed_forgets_the_old_ones_scroll_position() {
    let surface = Rc::new(FakePreviewSurface::new());
    let scheduler = Rc::new(FakeScheduler::new());
    let c = coordinator(&surface, &scheduler, &Rc::new(StubHtml::default()));
    surface.set_eval_handler(|script| if script == SCROLL_Y { "900".to_string() } else { "null".to_string() });

    c.update("a", "One", false, Some("one.md"));
    surface.complete_navigation(true);
    c.update("ab", "One", false, Some("one.md"));
    scheduler.advance(PreviewCoordinator::DEBOUNCE_DELAY);
    surface.complete_navigation(true);
    assert_eq!(c.saved_scroll_y(), 900.0);

    // Edit mode, then the sidebar picks another article while nobody is looking.
    surface.set_is_shown(false);
    c.hide();
    c.update("b", "Two", false, Some("two.md"));
    assert!(c.is_stale());

    surface.set_is_shown(true);
    let evals_before_show = surface.evals().len();
    c.show();

    assert_eq!(c.saved_scroll_y(), 0.0); // the other article's place is not this one's
    assert_eq!(surface.navigations().len(), 2); // exactly one load on return
    assert!(!c.is_stale());

    surface.complete_navigation(true);
    assert!(!surface.evals()[evals_before_show..].iter().any(|e| e.starts_with("window.__mdScrollTo")));
}

#[test]
fn changing_theme_while_collapsed_costs_one_load_not_two() {
    // The theme change calls update with the other flag; if that happens in Edit mode the whole
    // point of "stale while collapsed" is that Mermaid and PlantUML do not redraw twice.
    let surface = Rc::new(FakePreviewSurface::new());
    surface.set_is_shown(false);
    let scheduler = Rc::new(FakeScheduler::new());
    let c = coordinator(&surface, &scheduler, &Rc::new(StubHtml::default()));

    c.update("a", "Doc", false, None);
    c.update("a", "Doc", true, None);
    assert!(surface.navigations().is_empty());
    assert_eq!(scheduler.pending_timers(), 0);

    surface.set_is_shown(true);
    c.show();
    assert_eq!(surface.navigations().len(), 1);
    assert_eq!(surface.reload_count(), 0);
}
