//! Port of md.win `tests/Md.App.Logic.Tests/WindowPlacementTests.cs`.
//!
//! Sizes, the "WxH" round trip, the cascade and the clamp (shell-design.md §1.3, §9).

use md_logic::settings::settings_keys;
use md_logic::windows::window_placement::{self, BOOK_DEFAULT, DOCUMENT_DEFAULT, MINIMUM};
use md_logic::windows::{WindowRect, WindowSize};

const SCREEN: WindowRect = WindowRect::new(0, 0, 1920, 1080);

#[test]
fn the_defaults_are_the_designs_and_match_the_settings_keys_defaults() {
    assert_eq!(DOCUMENT_DEFAULT, WindowSize::new(900, 640));
    assert_eq!(BOOK_DEFAULT, WindowSize::new(1000, 700));
    assert_eq!(MINIMUM, WindowSize::new(480, 320));
    assert_eq!(window_placement::CASCADE_STEP, 24);

    assert_eq!(window_placement::format_size(DOCUMENT_DEFAULT), settings_keys::DOCUMENT_WINDOW_SIZE_DEFAULT);
    assert_eq!(window_placement::format_size(BOOK_DEFAULT), settings_keys::BOOK_WINDOW_SIZE_DEFAULT);
}

#[test]
fn sizes_round_trip_through_the_stored_spelling() {
    for (stored, width, height) in [("900x640", 900, 640), ("1000x700", 1000, 700), ("1920x1080", 1920, 1080)] {
        let size = window_placement::parse_size(Some(stored), DOCUMENT_DEFAULT);
        assert_eq!(size, WindowSize::new(width, height));
        assert_eq!(window_placement::format_size(size), stored);
    }
}

#[test]
fn an_unreadable_stored_size_falls_back_rather_than_making_a_windowless_app() {
    let rows = [
        None,
        Some(""),
        Some("900"),
        Some("900x"),
        Some("x640"),
        Some("900X640"), // the codec is ordinal: an upper-case X is not the spelling we write
        Some("900 x 640"),
        Some("-900x640"),
        Some("0x640"),
        Some("nine hundred"),
        Some("900x640x480"),
        // Rust's integer parser takes a leading '+'; .NET's NumberStyles.None does not.
        Some("+900x640"),
        Some("99999999999x640"),
    ];
    for stored in rows {
        assert_eq!(window_placement::parse_size(stored, DOCUMENT_DEFAULT), DOCUMENT_DEFAULT, "{stored:?}");
    }
}

#[test]
fn a_stored_size_below_the_minimum_is_raised_to_it() {
    assert_eq!(window_placement::parse_size(Some("100x100"), DOCUMENT_DEFAULT), MINIMUM);
}

#[test]
fn the_first_window_of_a_run_is_centred() {
    let rect = window_placement::cascade(None, DOCUMENT_DEFAULT, SCREEN);
    assert_eq!(rect, WindowRect::new((1920 - 900) / 2, (1080 - 640) / 2, 900, 640));
}

#[test]
fn each_next_window_steps_twenty_four_right_and_down() {
    let first = window_placement::cascade(None, DOCUMENT_DEFAULT, SCREEN);
    let second = window_placement::cascade(Some(first), DOCUMENT_DEFAULT, SCREEN);

    assert_eq!(second.x, first.x + 24);
    assert_eq!(second.y, first.y + 24);
    assert_eq!(second.size(), first.size());
}

#[test]
fn the_cascade_restarts_at_the_base_when_the_next_step_would_leave_the_work_area() {
    let almost_off = WindowRect::new(1000, 420, 900, 640); // +24 would cross the right edge
    let next = window_placement::cascade(Some(almost_off), DOCUMENT_DEFAULT, SCREEN);
    assert_eq!(next, window_placement::cascade(None, DOCUMENT_DEFAULT, SCREEN));
}

#[test]
fn the_cascade_wraps_only_when_the_step_would_actually_cross_the_edge() {
    // The boundary itself, both sides of it. A window whose stepped frame ends flush with the work
    // area is still inside it and must NOT restart the cascade — an off-by-one here throws the user
    // back to the centre one window early, every time, on every screen size.
    let flush = WindowRect::new(1920 - 900 - 24, 1080 - 640 - 24, 900, 640);
    let stepped = window_placement::cascade(Some(flush), DOCUMENT_DEFAULT, SCREEN);
    assert_eq!(stepped, WindowRect::new(1920 - 900, 1080 - 640, 900, 640));
    assert_eq!(stepped.right(), SCREEN.right());
    assert_eq!(stepped.bottom(), SCREEN.bottom());

    // One pixel further and the step would cross: now it wraps.
    let over_by_one = WindowRect::new(flush.x + 1, flush.y, 900, 640);
    assert_eq!(
        window_placement::cascade(Some(over_by_one), DOCUMENT_DEFAULT, SCREEN),
        window_placement::cascade(None, DOCUMENT_DEFAULT, SCREEN)
    );
}

#[test]
fn a_restored_frame_from_a_monitor_that_is_gone_is_brought_back_on_screen() {
    // Saved on a second monitor to the right that is no longer attached.
    let offscreen = WindowRect::new(3200, 200, 900, 640);
    assert_eq!(window_placement::clamp(offscreen, SCREEN), WindowRect::new(1920 - 900, 200, 900, 640));
}

#[test]
fn a_frame_larger_than_the_work_area_shrinks_to_it_and_sits_at_its_origin() {
    let work = WindowRect::new(0, 0, 800, 600);
    assert_eq!(window_placement::clamp(WindowRect::new(-100, -100, 2000, 1500), work), WindowRect::new(0, 0, 800, 600));
}

#[test]
fn clamping_respects_a_work_area_that_does_not_start_at_the_origin() {
    // A panel on the left, or a second monitor whose origin is negative.
    let work = WindowRect::new(-1920, 40, 1920, 1000);
    assert_eq!(
        window_placement::clamp(WindowRect::new(-4000, -500, 900, 640), work),
        WindowRect::new(-1920, 40, 900, 640)
    );
    assert_eq!(
        window_placement::clamp(WindowRect::new(9000, 9000, 900, 640), work),
        WindowRect::new(-1920 + 1920 - 900, 40 + 1000 - 640, 900, 640)
    );
}

#[test]
fn a_cascaded_window_is_always_inside_the_work_area() {
    let work = WindowRect::new(0, 40, 1280, 960);
    let mut previous = None;
    for i in 0..40 {
        let rect = window_placement::cascade(previous, DOCUMENT_DEFAULT, work);
        assert!(
            rect.x >= work.x && rect.y >= work.y && rect.right() <= work.right() && rect.bottom() <= work.bottom(),
            "window {i} at {rect:?} left {work:?}"
        );
        previous = Some(rect);
    }
}
