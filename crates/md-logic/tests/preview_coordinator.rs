//! Port of md.win `tests/Md.App.Logic.Tests/Preview/PreviewCoordinatorTests.cs`: every branch of
//! §4.5. The coordinator is the Mac's, so the tests are written as the Mac's rules: first load
//! immediate, everything else coalesced into one reload that keeps the reader's place, a new
//! document is a fresh page at the top, and a collapsed pane costs nothing until it is shown.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use md_logic::preview::asset_origin::{index_url, preview_host};
use md_logic::preview::screen_html::FONT_STYLE;
use md_logic::preview::scripts::{self, SCROLL_Y, invariant_number};
use md_logic::preview::{DocumentHtml, JumpId, PreviewCoordinator, PreviewNavigation};
use md_logic::testing::*;

/// Stands in for md-core's HTML writer: enough shape for the font graft to find a head.
#[derive(Default)]
struct StubHtml {
    calls: Cell<usize>,
}

impl DocumentHtml for StubHtml {
    fn document(&self, source: &str, title: &str, dark: bool) -> String {
        self.calls.set(self.calls.get() + 1);
        format!(
            "<html><head><title>{title}</title></head><body data-md-dark=\"{}\">{source}</body></html>",
            u8::from(dark)
        )
    }
}

fn index() -> String {
    index_url(&preview_host(1))
}

struct Fixture {
    surface: Rc<FakePreviewSurface>,
    scheduler: Rc<FakeScheduler>,
    executor: LocalExecutor,
    html: Rc<StubHtml>,
    coordinator: Rc<PreviewCoordinator>,
    /// What the page will report for `window.scrollY` on the next reload.
    page_scroll_y: Rc<Cell<f64>>,
}

impl Fixture {
    fn new() -> Fixture {
        let surface = Rc::new(FakePreviewSurface::new());
        let scheduler = Rc::new(FakeScheduler::new());
        let executor = LocalExecutor::new();
        let html = Rc::new(StubHtml::default());
        let coordinator = PreviewCoordinator::with_html(
            surface.clone(),
            scheduler.clone(),
            Rc::new(executor.clone()),
            html.clone(),
            index(),
        );
        Fixture { surface, scheduler, executor, html, coordinator, page_scroll_y: Rc::new(Cell::new(0.0)) }
    }

    fn with_live_page(self) -> Fixture {
        let y = self.page_scroll_y.clone();
        self.surface.set_eval_handler(
            move |script| {
                if script == SCROLL_Y { invariant_number(y.get()) } else { "null".to_string() }
            },
        );
        self
    }

    /// A first update, its load reported complete: the state every later branch starts from.
    fn loaded(self, text: &str, token: Option<&str>) -> Fixture {
        self.coordinator.update(text, "Doc", false, token);
        self.surface.complete_navigation(true);
        self
    }

    fn update(&self, text: &str) {
        self.coordinator.update(text, "Doc", false, None);
    }

    fn settle(&self) {
        self.scheduler.advance(PreviewCoordinator::DEBOUNCE_DELAY);
    }

    fn evals_starting(&self, prefix: &str) -> usize {
        self.surface.evals().iter().filter(|e| e.starts_with(prefix)).count()
    }

    fn jumps(&self, slug: &str) -> usize {
        let script = scripts::jump_to_slug(slug);
        self.surface.evals().iter().filter(|e| **e == script).count()
    }
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn recorder() -> (Rc<RefCell<Vec<JumpId>>>, impl Fn() -> Option<Box<dyn FnOnce(JumpId)>>) {
    let handled = Rc::new(RefCell::new(Vec::new()));
    let sink = handled.clone();
    (handled, move || {
        let sink = sink.clone();
        Some(Box::new(move |id| sink.borrow_mut().push(id)) as Box<dyn FnOnce(JumpId)>)
    })
}

// ───────────────────────────── first load ─────────────────────────────

#[test]
fn the_first_update_loads_immediately() {
    let f = Fixture::new();
    f.update("a");

    assert_eq!(f.surface.navigations(), vec![index()]);
    assert_eq!(f.surface.reload_count(), 0);
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn the_served_html_is_cores_page_with_the_linux_font_style() {
    let f = Fixture::new();
    f.update("a");

    assert!(f.surface.html().contains("<title>Doc</title>"));
    assert!(f.surface.html().contains(FONT_STYLE));
    assert_eq!(f.html.calls.get(), 1);
}

#[test]
fn an_update_that_would_render_the_same_page_does_nothing() {
    let f = Fixture::new().loaded("a", None);
    f.update("a");

    assert_eq!(f.surface.html_history().len(), 1);
    assert_eq!(f.html.calls.get(), 1);
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn title_and_theme_are_part_of_the_key() {
    let f = Fixture::new().with_live_page().loaded("a", None);

    f.coordinator.update("a", "Other", false, None);
    f.settle();
    assert_eq!(f.surface.reload_count(), 1);

    f.surface.complete_navigation(true);
    f.coordinator.update("a", "Other", true, None);
    f.settle();
    assert_eq!(f.surface.reload_count(), 2);
}

// ───────────────────────────── coalescing ─────────────────────────────

#[test]
fn typing_coalesces_into_one_reload_after_350_ms() {
    let f = Fixture::new().with_live_page().loaded("a", None);

    f.update("ab");
    f.scheduler.advance(ms(200));
    f.update("abc");
    f.scheduler.advance(ms(200));

    // The second keystroke restarted the debounce, so 400 ms in nothing has reloaded yet.
    assert_eq!(f.surface.reload_count(), 0);

    f.scheduler.advance(ms(150));
    assert_eq!(f.surface.reload_count(), 1);
    assert!(f.surface.html().contains("abc"));
    assert_eq!(f.surface.navigations().len(), 1); // still the one first load
}

#[test]
fn the_debounce_is_exactly_350_ms() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.update("ab");

    f.scheduler.advance(ms(349));
    assert_eq!(f.surface.reload_count(), 0);

    f.scheduler.advance(ms(1));
    assert_eq!(f.surface.reload_count(), 1);
    assert_eq!(PreviewCoordinator::DEBOUNCE_DELAY, ms(350));
}

#[test]
fn every_change_still_regenerates_the_html_immediately() {
    // The debounce delays the reload, never the HTML: an export or a print taken mid-typing must
    // see what the editor holds.
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.update("ab");

    assert!(f.surface.html().contains("ab"));
    assert_eq!(f.surface.reload_count(), 0);
}

// ───────────────────────────── token change ─────────────────────────────

#[test]
fn a_new_document_loads_immediately_and_forgets_the_scroll_position() {
    let f = Fixture::new().with_live_page().loaded("a", Some("one.md"));
    f.page_scroll_y.set(900.0);
    f.coordinator.update("b", "Doc", false, Some("one.md"));
    f.settle(); // a reload that saved 900
    f.surface.complete_navigation(true);
    assert_eq!(f.coordinator.saved_scroll_y(), 900.0);

    f.coordinator.update("c", "Doc", false, Some("two.md"));

    assert_eq!(f.coordinator.saved_scroll_y(), 0.0);
    assert_eq!(f.surface.navigations().len(), 2);
    assert_eq!(f.surface.reload_count(), 1);
    assert_eq!(f.scheduler.pending_timers(), 0);
}

#[test]
fn the_same_article_with_the_same_text_is_not_a_new_document() {
    let f = Fixture::new().loaded("a", Some("one.md"));
    f.coordinator.update("a", "Doc", false, Some("one.md"));

    assert_eq!(f.surface.navigations().len(), 1);
}

#[test]
fn a_new_document_reloads_even_when_the_text_is_identical() {
    let f = Fixture::new().loaded("same", Some("one.md"));
    f.coordinator.update("same", "Doc", false, Some("two.md"));

    assert_eq!(f.surface.navigations().len(), 2);
}

// ───────────────────────────── scroll restore ─────────────────────────────

#[test]
fn a_reload_saves_the_scroll_position_and_restores_it_when_the_load_finishes() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.page_scroll_y.set(1234.5);

    f.update("ab");
    f.settle();

    assert!(f.surface.evals().contains(&SCROLL_Y.to_string()));
    assert_eq!(f.coordinator.saved_scroll_y(), 1234.5);

    f.surface.complete_navigation(true);
    assert!(f.surface.evals().contains(&"window.__mdScrollTo?.(1234.5)".to_string()));
}

#[test]
fn nothing_is_restored_from_the_top_of_the_page() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.page_scroll_y.set(0.0);

    f.update("ab");
    f.settle();
    f.surface.complete_navigation(true);

    assert_eq!(f.evals_starting("window.__mdScrollTo"), 0);
}

#[test]
fn a_page_that_cannot_answer_reloads_from_the_top() {
    let f = Fixture::new().loaded("a", None); // no eval handler: every script answers JSON null
    f.update("ab");
    f.settle();

    assert_eq!(f.coordinator.saved_scroll_y(), 0.0);
    assert_eq!(f.surface.reload_count(), 1);
}

#[test]
fn a_failed_load_restores_nothing() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.page_scroll_y.set(500.0);
    f.update("ab");
    f.settle();

    f.surface.complete_navigation(false);

    assert_eq!(f.evals_starting("window.__mdScrollTo"), 0);
}

#[test]
fn a_page_that_answers_later_is_reloaded_when_it_does() {
    // LINUX: the evaluation is a future. md.win's fake answers at once; the GTK surface's answer
    // always arrives later. This pins that half: a pending answer is awaited on the spawner and the
    // reload follows it, with the position it brought.
    let f = Fixture::new().loaded("a", None);
    let answer = f.surface.hold_eval();
    f.update("ab");
    f.settle();
    f.executor.run_until_stalled();
    assert_eq!(f.surface.reload_count(), 0);

    answer.complete("321".to_string());
    f.executor.run_until_stalled();
    assert_eq!(f.surface.reload_count(), 1);
    assert_eq!(f.coordinator.saved_scroll_y(), 321.0);
}

// ───────────────────────────── stale while collapsed ─────────────────────────────

#[test]
fn a_collapsed_pane_records_the_change_and_loads_nothing() {
    let f = Fixture::new();
    f.surface.set_is_shown(false);

    f.update("a");

    assert!(f.surface.navigations().is_empty());
    assert_eq!(f.surface.reload_count(), 0);
    assert_eq!(f.scheduler.pending_timers(), 0);
    assert!(f.coordinator.is_stale());
    assert!(f.surface.html().contains('a')); // the HTML is current all the same
}

#[test]
fn showing_the_pane_loads_once_however_many_changes_it_missed() {
    let f = Fixture::new();
    f.surface.set_is_shown(false);
    f.update("a");
    f.update("ab");
    f.update("abc");

    f.surface.set_is_shown(true);
    f.coordinator.show();

    assert_eq!(f.surface.navigations(), vec![index()]);
    assert!(!f.coordinator.is_stale());
    assert!(f.surface.html().contains("abc"));
}

#[test]
fn showing_a_pane_that_missed_nothing_loads_nothing() {
    let f = Fixture::new().loaded("a", None);
    f.coordinator.show();

    assert_eq!(f.surface.navigations().len(), 1);
}

#[test]
fn collapsing_drops_a_waiting_reload_and_the_next_show_does_it() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.update("ab");
    assert_eq!(f.scheduler.pending_timers(), 1);

    f.surface.set_is_shown(false);
    f.coordinator.hide();
    f.settle();

    assert_eq!(f.surface.reload_count(), 0); // nobody was looking
    assert!(f.coordinator.is_stale());

    f.surface.set_is_shown(true);
    f.coordinator.show();
    assert_eq!(f.surface.navigations().len(), 2); // a fresh page, the Mac's look on return
}

#[test]
fn collapsing_with_nothing_pending_leaves_the_page_alone() {
    let f = Fixture::new().loaded("a", None);
    f.coordinator.hide();

    assert!(!f.coordinator.is_stale());
    f.coordinator.show();
    assert_eq!(f.surface.navigations().len(), 1);
}

// ───────────────────────────── heading navigation ─────────────────────────────

#[test]
fn a_heading_jump_evaluates_the_script_and_reports_back_on_the_next_turn() {
    let f = Fixture::new().loaded("a", None);
    let (handled, on_handled) = recorder();
    let jump = PreviewNavigation::new(JumpId::next(), "a-heading");

    f.coordinator.navigate(jump.clone(), on_handled());

    assert!(f.surface.evals().contains(&scripts::jump_to_slug("a-heading")));
    assert!(handled.borrow().is_empty()); // posted, not called
    f.scheduler.run_posted();
    assert_eq!(*handled.borrow(), vec![jump.id]);
}

#[test]
fn the_same_request_is_performed_only_once() {
    let f = Fixture::new().loaded("a", None);
    let jump = PreviewNavigation::new(JumpId::next(), "a-heading");

    f.coordinator.navigate(jump.clone(), None);
    f.coordinator.navigate(jump, None);

    assert_eq!(f.jumps("a-heading"), 1);
}

#[test]
fn picking_the_same_heading_twice_works_because_the_id_is_the_identity() {
    let f = Fixture::new().loaded("a", None);

    f.coordinator.navigate(PreviewNavigation::new(JumpId::next(), "a-heading"), None);
    f.coordinator.navigate(PreviewNavigation::new(JumpId::next(), "a-heading"), None);

    assert_eq!(f.jumps("a-heading"), 2);
}

#[test]
fn a_jump_that_arrives_during_a_load_is_parked_and_done_when_the_load_finishes() {
    let f = Fixture::new();
    let (handled, on_handled) = recorder();
    f.update("a"); // loading
    let jump = PreviewNavigation::new(JumpId::next(), "a-heading");

    f.coordinator.navigate(jump.clone(), on_handled());
    assert!(f.surface.evals().is_empty());

    f.surface.complete_navigation(true);
    assert!(f.surface.evals().contains(&scripts::jump_to_slug("a-heading")));
    f.scheduler.run_posted();
    assert_eq!(*handled.borrow(), vec![jump.id]);
}

#[test]
fn a_parked_jump_is_performed_exactly_once() {
    let f = Fixture::new();
    f.update("a");
    f.coordinator.navigate(PreviewNavigation::new(JumpId::next(), "a-heading"), None);

    f.surface.complete_navigation(true);
    f.surface.complete_navigation(true);

    assert_eq!(f.jumps("a-heading"), 1);
}

#[test]
fn a_jump_parked_by_a_reload_waits_for_that_reload() {
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.update("ab");
    f.settle(); // reload in flight

    f.coordinator.navigate(PreviewNavigation::new(JumpId::next(), "later"), None);
    assert_eq!(f.jumps("later"), 0);

    f.surface.complete_navigation(true);
    assert_eq!(f.jumps("later"), 1);
}

#[test]
fn a_jump_parked_behind_a_failed_load_is_not_thrown_away() {
    let f = Fixture::new();
    f.update("a");
    f.coordinator.navigate(PreviewNavigation::new(JumpId::next(), "a-heading"), None);

    f.surface.complete_navigation(false);
    assert!(f.surface.evals().is_empty());

    f.coordinator.update("b", "Doc", false, Some("next.md")); // a fresh load
    f.surface.complete_navigation(true);
    assert_eq!(f.jumps("a-heading"), 1);
}

// ───────────────────────────── wiring ─────────────────────────────

#[test]
fn the_two_argument_constructor_takes_the_process_wide_writer() {
    // LINUX: there is no process-wide writer to set; the plain constructor takes md-core's own.
    let surface = Rc::new(FakePreviewSurface::new());
    let coordinator =
        PreviewCoordinator::new(surface.clone(), Rc::new(FakeScheduler::new()), Rc::new(LocalExecutor::new()), index());

    coordinator.update("# Hello", "Doc", false, None);

    let html = surface.html();
    assert!(html.contains("<title>Doc</title>"));
    assert!(html.contains("Hello</h1>"));
    assert!(html.contains(FONT_STYLE));
    assert_eq!(
        html,
        md_logic::preview::screen_html::with_linux_fonts(&md_core::markdown::html::document(
            "# Hello", "Doc", false, false
        ))
    );
    assert_eq!(coordinator.index_url(), index());
}

#[test]
#[ignore = "LINUX: no unwired default — md-core's writer exists, so PreviewCoordinator::new always has one (md.win's DocumentHtml.Default throws until Md.App sets it)"]
fn until_it_is_wired_the_default_writer_says_so_rather_than_preview_something_made_up() {
    // md.win: new PreviewCoordinator(surface, scheduler).Update(…) throws InvalidOperationException
    // naming DocumentHtml.Default.
}

#[test]
fn a_dropped_coordinator_lets_go_of_the_surface() {
    // LINUX: the navigation handler holds only a Weak; dropping the coordinator disconnects it and
    // cancels a waiting reload, so a closed window's preview does nothing more.
    let f = Fixture::new().with_live_page().loaded("a", None);
    f.update("ab");
    assert_eq!(f.surface.navigation_completed().handler_count(), 1);
    let Fixture { surface, scheduler, coordinator, .. } = f;
    drop(coordinator);

    assert_eq!(surface.navigation_completed().handler_count(), 0);
    assert_eq!(scheduler.pending_timers(), 0);
    scheduler.advance(PreviewCoordinator::DEBOUNCE_DELAY);
    assert_eq!(surface.reload_count(), 0);
}
