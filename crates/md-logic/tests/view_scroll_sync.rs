//! Port of md.win `tests/Md.App.Logic.Tests/ScrollSyncTests.cs` (`ScrollSyncTests` and
//! `ScrollSyncGuardTests`): shell-design §3.4 — the pane link and the echo guard behind it.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use md_logic::testing::*;
use md_logic::view::{ScrollSync, ScrollSyncGuard};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn sink() -> (Rc<Cell<Option<f64>>>, Box<dyn Fn(f64)>) {
    let seen = Rc::new(Cell::new(None));
    let s = seen.clone();
    (seen, Box::new(move |f| s.set(Some(f))))
}

#[test]
fn each_side_drives_the_other() {
    let sync = ScrollSync::new();
    let (editor, scroll_editor) = sink();
    let (preview, scroll_preview) = sink();
    sync.set_scroll_editor(Some(scroll_editor));
    sync.set_scroll_preview(Some(scroll_preview));

    sync.editor_did_scroll(0.25);
    sync.preview_did_scroll(0.75);

    assert_eq!(preview.get(), Some(0.25));
    assert_eq!(editor.get(), Some(0.75));
}

#[test]
fn an_unregistered_side_is_simply_not_driven() {
    let sync = ScrollSync::new();

    sync.editor_did_scroll(0.5);
    sync.preview_did_scroll(0.5);
}

#[test]
fn the_fraction_is_clamped_and_never_nan() {
    for (input, expected) in [(-0.5, 0.0), (0.0, 0.0), (1.5, 1.0), (f64::NAN, 0.0), (f64::INFINITY, 1.0)] {
        let sync = ScrollSync::new();
        let (seen, scroll_preview) = sink();
        sync.set_scroll_preview(Some(scroll_preview));

        sync.editor_did_scroll(input);

        assert_eq!(seen.get(), Some(expected), "{input}");
    }
}

// ── ScrollSyncGuardTests ──────────────────────────────────────────────────────────────────

#[test]
fn the_flag_is_set_only_while_the_programmatic_scroll_is_applied() {
    let clock = Rc::new(FakeClock::new());
    let guard = ScrollSyncGuard::new(clock);

    assert!(!guard.is_echo());
    {
        let _scope = guard.applying();
        assert!(guard.is_applying());
        assert!(guard.is_echo());
    }
    assert!(!guard.is_applying());
    assert!(!guard.is_echo());
}

#[test]
fn the_window_keeps_swallowing_echoes_after_the_apply_returns() {
    let clock = Rc::new(FakeClock::new());
    let guard = ScrollSyncGuard::with_window(clock.clone(), ScrollSyncGuard::DEFAULT_WINDOW);
    drop(guard.applying());

    clock.advance(ms(49));
    assert!(guard.is_echo());

    clock.advance(ms(1));
    assert!(!guard.is_echo());
}

#[test]
fn the_window_is_measured_from_the_end_of_the_apply_and_not_its_start() {
    // set_value is not free, and an echo it provokes arrives after it returns — so the 50 ms has
    // to start when the apply finishes. Measured from the start it would already be half spent by
    // the time there is anything to swallow.
    let clock = Rc::new(FakeClock::new());
    let guard = ScrollSyncGuard::with_window(clock.clone(), ScrollSyncGuard::DEFAULT_WINDOW);

    {
        let _scope = guard.applying();
        clock.advance(ms(40));
    }

    clock.advance(ms(49));
    assert!(guard.is_echo());

    clock.advance(ms(1));
    assert!(!guard.is_echo());
}

#[test]
fn the_shipping_default_is_the_plain_flag_with_no_window_at_all() {
    // §3.4: the app ships with no window (the Mac's synchronous notification, and GtkAdjustment's
    // synchronous value-changed); DEFAULT_WINDOW is the contingency, not the default.
    let clock = Rc::new(FakeClock::new());
    let guard = ScrollSyncGuard::new(clock);

    drop(guard.applying());

    assert!(!guard.is_echo());
    assert_eq!(ScrollSyncGuard::DEFAULT_WINDOW, ms(50));
}

#[test]
fn nested_applies_do_not_clear_the_flag_early() {
    let guard = ScrollSyncGuard::new(Rc::new(FakeClock::new()));

    let outer = guard.applying();
    let inner = guard.applying();
    drop(inner);
    assert!(guard.is_applying());
    drop(outer);
    assert!(!guard.is_applying());
}

#[test]
fn disposing_twice_is_harmless() {
    // md.win disposes one scope twice; a Rust scope is dropped exactly once by construction, so
    // what is left to pin is that an early drop leaves nothing behind.
    let guard = ScrollSyncGuard::new(Rc::new(FakeClock::new()));

    let scope = guard.applying();
    drop(scope);

    assert!(!guard.is_applying());
}
