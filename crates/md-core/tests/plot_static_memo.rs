//! The shared memo behind `render_plot`, from md.win's PlotTests §11.
//!
//! md.win pins it with the weakest assertions that survive another xUnit class rendering a plot
//! at the same time. Rust runs every test of a file on parallel threads, so this one lives alone
//! in its own test binary — its own process, its own static — and nothing can render a plot
//! between its steps. Keep it the only test here.

use md_core::plot::{self, Statistics};

#[test]
fn memo_behind_the_static_surface_is_the_same_memo() {
    plot::clear_memo();
    let source = "x: -10..10\ntitle: static memo\nsin(x)";
    let first = plot::render_plot(source);
    let after = plot::memo_statistics();
    assert!(after.entries >= 1 && after.misses >= 1, "{after:?}");
    assert_eq!(first, plot::render_plot(source));
    assert!(
        plot::memo_statistics().hits > after.hits || plot::memo_statistics().entries == 0,
        "a repeat render is served from the memo unless another test cleared it meanwhile"
    );
    // Alone in this process, the statistics are exact as well.
    assert_eq!(Statistics::new(1, 0, 1), after);
    assert_eq!(Statistics::new(1, 1, 1), plot::memo_statistics());
    plot::clear_memo();
    assert_eq!(Statistics::new(0, 0, 0), plot::memo_statistics());
    assert_eq!(first, plot::render_plot(source));
}
