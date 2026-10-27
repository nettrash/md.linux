//! Adversarial review of the ```plot port. Port of md.win
//! `tests/Md.Core.Tests/PlotAdversarialTests.cs`.
//!
//! Every expectation here was derived from the Swift source (`md.macOS/md/Plot.swift`:
//! scalar-by-scalar scanning, the grammar's own whitespace, the 4096-node / 128-depth budgets, the
//! 0.6 em legend estimate) or recomputed with an independent oracle (IEEE doubles in Python,
//! formatted from the exact decimal expansion, ties to even) — never read off any port's output.
//! Each test names the mutation it exists to kill.

mod common;
#[path = "common/plot.rs"]
mod plot_common;

use md_core::plot::{self, PlotMemo, Statistics};
use plot_common::*;

/// The first `x,y` pair of the first polyline.
fn first_pair(svg: &str) -> String {
    let at = svg.find("<polyline points=\"").unwrap_or_else(|| panic!("no polyline in: {}", clip(svg)));
    let start = at + "<polyline points=\"".len();
    let end = svg[start..].find([' ', '"']).map_or(svg.len(), |offset| start + offset);
    svg[start..end].to_string()
}

/// The last `x,y` pair of the last polyline.
fn last_pair(svg: &str) -> String {
    let at = svg.rfind("<polyline points=\"").unwrap_or_else(|| panic!("no polyline in: {}", clip(svg)));
    let open = at + "<polyline points=\"".len();
    let close = svg[open..].find('"').map_or(svg.len(), |offset| open + offset);
    let space = svg[..close].rfind(' ').map_or(0, |offset| offset + 1);
    svg[space..close].to_string()
}

/// Run `body` on a thread with exactly this much stack, so the result does not depend on the test
/// harness's thread.
fn on_thread(stack_kb: usize, body: impl FnOnce() -> String + Send + 'static) -> String {
    std::thread::Builder::new()
        .stack_size(stack_kb * 1024)
        .spawn(body)
        .expect("a thread")
        .join()
        .unwrap_or_else(|_| panic!("the body panicked on a {stack_kb} KB thread"))
}

// Kills: resolve_y's 0.05 → 0.1. The suite's only `y: auto` coordinate check is a flat series,
// whose padding is symmetric and so invisible.
//
// x over [-1, 1]: low −1, high 1, span 2, pad 0.1 → window [−1.1, 1.1] (both exact doublings of
// 0.05 and 0.1, so 1 + 0.1 rounds to the 1.1 double and the width is the 2.2 double). Default
// canvas, one unlabelled series → no legend: left 40, top 40, plot 520 × 320. sy(−1) = 40 +
// (2.1/2.2)·320 = 345.4545… → "345.45" at two places, "345.5" at one; sy(1) = 40 + (0.1/2.2)·320 =
// 54.5454… → "54.55" / "54.5". niceStep(2.2): rough 0.275, decade 0.1, norm 2.75 → step 0.2;
// ticks −1.0 … 1.0, eleven of them.
#[test]
fn y_auto_pads_five_percent_of_the_span_on_each_side() {
    let drawn = svg("x: -1..1\nx");
    assert_eq!("40.00,345.45", first_pair(&drawn));
    assert_eq!("560.00,54.55", last_pair(&drawn));
    has(&drawn, "<line x1=\"40.0\" y1=\"345.5\" x2=\"560.0\" y2=\"345.5\"/>", "the grid line of the tick at -1");
    has(&drawn, "<line x1=\"40.0\" y1=\"54.5\" x2=\"560.0\" y2=\"54.5\"/>", "the grid line of the tick at 1");
    has(&drawn, "y=\"345.5\" text-anchor=\"end\" dominant-baseline=\"middle\">-1</text>", "");
    has(&drawn, "y=\"54.5\" text-anchor=\"end\" dominant-baseline=\"middle\">1</text>", "");
    assert_eq!(11, count(&drawn, "dominant-baseline=\"middle\">"));
    // The parsed range is unpadded and the pad is applied only when drawing.
    assert_eq!(None, plot::parse_plot("x: -1..1\nx").expect("parses").y_min);
}

// Kills: the unary operand parsed at precedence 1 instead of the `^` level. Every unary minus in
// the existing suite precedes a `^`, a `/`, a `%` or a lone operand, where −(a op b) and
// (−a) op b coincide. `unary()` parses its operand with `expression(POWER_PRECEDENCE)`, so it
// binds tighter than every additive and multiplicative operator and looser than `^` alone.
#[test]
fn unary_operators_bind_tighter_than_the_binary_ones_except_power() {
    assert_eq!(-2.0, v("-x+1", 3.0)); // (−x) + 1, not −(x + 1) = −4
    assert_eq!(-4.0, v("-x-1", 3.0)); // (−x) − 1, not −(x − 1) = −2
    assert_eq!(2.0, v("!x+1", 0.0)); // (!0) + 1, not !(0 + 1) = 0
    assert_eq!(4.0, v("+x+1", 3.0));
    assert_eq!(-6.0, v("-x*2", 3.0));
    assert_eq!(-6.0, v("2*-x", 3.0));
    assert_eq!(0.5, v("2^-x", 1.0));
    assert_eq!(-8.0, v("-x^2+1", 3.0)); // (−(x²)) + 1
    assert_eq!(1.0, v("-x<0", 3.0)); // (−x) < 0
    assert_eq!(0.0, v("-(x<0)", 3.0));
}

// Kills: `xMin + i * dx` in sample_series. The recorded figure and the existing test check only
// the first and last point of the run, and the Python oracle (same IEEE doubles, exact decimal
// rounding) shows the two formulas agree there and differ on 44 interior samples of this fence —
// the first at sample 455: 348.47 against 348.48.
#[test]
fn samples_with_the_divide_first_formula_in_the_middle_of_the_run_too() {
    let drawn = svg("x: -6.3..0.7999999999999998\ny: -3.3..3.1\nx");
    has(&drawn, " 276.60,348.47 ", "sample 455");
    has(&drawn, " 282.84,344.21 ", "sample 467");
    has(&drawn, " 293.24,337.12 ", "sample 487");
    lacks(&drawn, "276.60,348.48", "what xMin + i * dx draws at sample 455");
    lacks(&drawn, "282.84,344.22", "");
    lacks(&drawn, "293.24,337.11", "");
}

// Kills: count_characters → a UTF-16 (or UTF-8) length. The existing emoji test compares a
// one-emoji label with a one-letter label, and both fall under the 72 px legend minimum, so the
// gutter is 72 either way. Twelve code points at 0.6 em of 11 px are 79.2 → ceil 80 + 28 = 108 →
// a 412 px plot area for emoji and letters alike; twenty-four UTF-16 units would be 158.4 → 159 +
// 28 = 187 → 333 px, which is exactly what 24 letters get.
#[test]
fn legend_gutter_counts_code_points_above_the_minimum_width() {
    let twelve_emoji = "\u{1F600}".repeat(12);
    let emoji = svg(&format!("x: -1..1\n{twelve_emoji} = x"));
    let letters = svg("x: -1..1\nabcdefghijkl = x");
    let twenty_four = svg("x: -1..1\nabcdefghijklmnopqrstuvwx = x");
    assert_eq!("<rect x=\"40.0\" y=\"40.0\" width=\"412.0\" height=\"320.0\"", rect_value(&letters));
    assert_eq!("<rect x=\"40.0\" y=\"40.0\" width=\"412.0\" height=\"320.0\"", rect_value(&emoji));
    assert_eq!("<rect x=\"40.0\" y=\"40.0\" width=\"333.0\" height=\"320.0\"", rect_value(&twenty_four));
    has(&emoji, &format!(">{twelve_emoji}</text>"), "");
    // A supplementary-plane character outside a label is named whole in the message (Swift's
    // bytes), and travels whole through a title.
    has(&plot::render_plot("x + \u{1D465}"), "plot: unexpected character '\u{1D465}'\nx + \u{1D465}", "");
    has(&svg("title: \u{1D465}\nx"), "<title>\u{1D465}</title>", "");
    assert_eq!("\u{1D465}", plot::parse_plot("title: \u{1D465}\nx").expect("parses").title);
}

// The scanner walks code points; a combining mark is a character of its own and never part of the
// `=`, `..`, `#` or `:` next to it. In Swift the source is `Array(source.unicodeScalars)`, so
// `=\u{0301}` is two scalars there too.
#[test]
fn combining_marks_next_to_delimiters_are_characters_not_delimiters() {
    // `a =\u{0301} x`: the `=` is still the label separator; the mark heads the body.
    assert_eq!(
        "<div class=\"plot\"><pre>plot: unexpected character '\u{0301}'\na =\u{0301} x</pre></div>",
        plot::render_plot("a =\u{0301} x")
    );
    // A mark on the range's last digit is inside the constant expression.
    has(&plot::render_plot("x: -1..1\u{0301}\nx"), "plot: unexpected character '\u{0301}'", "");
    // `#` followed by a mark is still a comment line: the first character decides.
    assert_eq!("<div class=\"plot\"></div>", plot::render_plot("#\u{0301} sin(x)"));
    // A mark right after the directive colon is part of the value, and a title keeps it.
    assert_eq!("\u{0301}T", plot::parse_plot("title:\u{0301}T\nx").expect("parses").title);
    has(&svg("title: T\u{0301}\nx"), "<title>T\u{0301}</title>", "");
    // NBSP alone is a series line, not blank: space and tab are the only whitespace.
    assert_eq!(
        "<div class=\"plot\"><pre>plot: unexpected character '\u{00A0}'\n\u{00A0}</pre></div>",
        plot::render_plot("\u{00A0}")
    );
    // A mark glued to a name splits the name: `sin\u{0301}(x)` is `sin` then a stray mark.
    has(&plot::render_plot("sin\u{0301}(x)"), "plot: unexpected character '\u{0301}'", "");
}

// CR, LF and CRLF are the parser's three line breaks and nothing else is; the figure a fence draws
// cannot depend on which one the editor saved.
#[test]
fn cr_crlf_and_lf_fences_draw_the_same_figure_but_are_different_memo_keys() {
    let lf = "x: -1..1\ntitle: T\nsin(x)";
    let crlf = "x: -1..1\r\ntitle: T\r\nsin(x)";
    let cr = "x: -1..1\rtitle: T\rsin(x)";
    let drawn = svg(lf);
    assert_eq!(drawn, svg(crlf));
    assert_eq!(drawn, svg(cr));
    assert_eq!(drawn, svg(&format!("{crlf}\r\n")));
    has(&drawn, "<title>T</title>", "");
    // Blank input in every spelling is the empty container, and never an error.
    for blank in ["", "\n", "\r", "\r\n", "\r\n\r\n", "\t\r\n \r", " \t"] {
        assert_eq!("<div class=\"plot\"></div>", plot::render_plot(blank), "{blank:?}");
    }
    // Mixed breaks in one fence, and a trailing one, are three series.
    assert_eq!(3, plot::parse_plot("sin(x)\rcos(x)\r\ntan(x)\n").expect("parses").series.len());
    // A tab inside a value survives; only the ends are trimmed. `\r` alone ends the title, so
    // "A\rB" is a title and a series `B`.
    assert_eq!("A\tB", plot::parse_plot("title:\tA\tB\t\nx").expect("parses").title);
    assert_eq!("A", plot::parse_plot("title: A\rB = x").expect("parses").title);
    assert_eq!(Some("B"), plot::parse_plot("title: A\rB = x").expect("parses").series[0].label.as_deref());
    // The memo is keyed on the exact text: three spellings are three entries.
    let memo = PlotMemo::new(32);
    let _ = plot::render_plot_with_memo(lf, &memo);
    let _ = plot::render_plot_with_memo(crlf, &memo);
    let _ = plot::render_plot_with_memo(cr, &memo);
    assert_eq!(Statistics::new(3, 0, 3), memo.snapshot());
}

// The budgets, at their exact boundaries. `count()` charges one node per binary and unary node, so
// `x` + 4096 × `+x` is exactly 4096 nodes and legal; `expression()` charges one depth level per
// call, so 127 parentheses reach depth 128 and 128 reach 129. Run on a 16 MB thread so the
// contract, not the host thread, decides.
#[test]
fn the_node_and_depth_budgets_are_exact_at_their_boundaries() {
    let results = on_thread(16 * 1024, || {
        let mut out = String::new();
        let draws = |source: String| plot::render_plot(&source).contains("<polyline");
        let says = |source: String, message: &str| plot::render_plot(&source).contains(message);
        out.push_str(if draws(format!("x: -1..1\nx{}", "+x".repeat(4096))) { "4096:draws;" } else { "4096:no;" });
        out.push_str(if says(format!("x: -1..1\nx{}", "+x".repeat(4097)), "plot: expression too large") {
            "4097:large;"
        } else {
            "4097:no;"
        });
        out.push_str(if draws(format!("x: -1..1\n{}x{}", "(".repeat(127), ")".repeat(127))) {
            "127:draws;"
        } else {
            "127:no;"
        });
        out.push_str(
            if says(format!("x: -1..1\n{}x{}", "(".repeat(128), ")".repeat(128)), "plot: expression nested too deeply")
            {
                "128:deep;"
            } else {
                "128:no;"
            },
        );
        out.push_str(if draws(format!("x: -1..1\n{}x", "-".repeat(127))) { "-127:draws;" } else { "-127:no;" });
        out.push_str(if says(format!("x: -1..1\n{}x", "-".repeat(128)), "plot: expression nested too deeply") {
            "-128:deep"
        } else {
            "-128:no"
        });
        out
    });
    assert_eq!("4096:draws;4097:large;127:draws;128:deep;-127:draws;-128:deep", results);
    // 127 minuses is an odd number of them, so the curve is -x: it climbs out of the auto window's
    // top-left corner and ends bottom-right.
    let odd = on_thread(16 * 1024, || svg(&format!("x: -1..1\ny: -1..1\n{}x", "-".repeat(127))));
    assert_eq!("40.00,40.00", first_pair(&odd));
    assert_eq!("560.00,360.00", last_pair(&odd));
}

// A legal fence must never be a dead process. The 4096-node chain is 4096 levels deep, which md.win
// evaluates recursively and a 1 MB Windows thread does not reliably hold, so md.win answers with
// its `plot:` line on a starved thread. This port walks the tree (and drops it) with an explicit
// stack, so on any thread the answer is the drawing — and the C# assertion, "a drawing or the
// fallback, never anything else", holds as written.
#[test]
fn the_legal_chain_on_a_starved_thread_is_a_container_not_a_crash() {
    let source = format!("x: -1..1\nx{}", "+x".repeat(4096));
    let fallback = format!("plot: expression too large\n{source}");
    let starved = on_thread(256, move || plot::render_plot_with_memo(&source, &PlotMemo::new(32)));
    assert!(starved.starts_with("<div class=\"plot\">"));
    assert!(starved.ends_with("</div>"));
    assert!(
        starved.contains("<polyline") || starved.contains(&fallback),
        "a drawing or the fallback, never anything else: {}",
        clip(&starved)
    );
    // On a 1 MB thread the parser's 128 levels fit, so the depth guard speaks first.
    let deep = on_thread(1024, || {
        plot::render_plot_with_memo(&format!("x: -1..1\n{}x{}", "(".repeat(5000), ")".repeat(5000)), &PlotMemo::new(32))
    });
    has(&deep, "plot: expression nested too deeply", "");
}

// The largest fence the clamps allow: 24 series, 5000 samples, 2000 × 2000. It renders (every
// constant is at its ceiling, none over), and it is the container the memo refuses — 24 polylines
// of 5001 points are far past 128 × 1024 code units.
#[test]
fn the_largest_legal_fence_renders_and_is_not_memoised() {
    let mut lines = vec!["x: -1..1".to_string(), "width: 2000".into(), "height: 2000".into(), "samples: 5000".into()];
    for index in 0..24 {
        lines.push(format!("x*{index}"));
    }
    let source = lines.join("\n");
    let spec = plot::parse_plot(&source).expect("parses");
    assert_eq!(24, spec.series.len());
    assert_eq!((2000, 2000, 5000), (spec.width, spec.height, spec.samples));
    let memo = PlotMemo::new(32);
    let html = plot::render_plot_with_memo(&source, &memo);
    lacks(&html, "plot:", "");
    has(&html, "viewBox=\"0 0 2000 2000\"", "");
    assert_eq!(24, count(&html, "<polyline "));
    assert!(html.encode_utf16().count() > PlotMemo::LARGEST_MEMOISED_VALUE, "length {}", html.len());
    assert_eq!(Statistics::new(0, 0, 1), memo.snapshot());
    // One more line is the 25th series, and the error names the limit.
    has(&plot::render_plot(&format!("{source}\nx*24")), "plot: too many series (limit 24)", "");
}

// This port's, not md.win's: the parser still recurses (only the evaluator and the tree's drop
// were made iterative), bounded by the 128-level guard. Every deepest-legal shape must parse on a
// half-megabyte thread even unoptimised — a regression in the parser's frame size fails here
// before it can abort the app on a smaller thread.
#[test]
fn the_deepest_legal_fence_of_every_shape_parses_on_a_half_megabyte_thread() {
    let shapes = [
        format!("x: -1..1\n{}x{}", "(".repeat(127), ")".repeat(127)),
        format!("x: -1..1\n{}x", "-".repeat(127)),
        format!("x: -1..1\n{}x", "2^".repeat(127)),
        format!("x: -1..1\n{}x{}", "sin(".repeat(127), ")".repeat(127)),
        format!("x: -1..1\n{}x{}", "pow(1,".repeat(127), ")".repeat(127)),
        format!("x: -1..1\nx{}", "+x".repeat(4096)),
    ];
    for source in shapes {
        let label = clip(&source);
        let html = on_thread(512, move || plot::render_plot_with_memo(&source, &PlotMemo::new(32)));
        // A figure, not the `plot:` fallback (the power tower is ∞ everywhere, so it is axes only).
        assert!(html.starts_with("<div class=\"plot\"><svg ") && !html.contains("plot:"), "{label} → {}", clip(&html));
    }
}
