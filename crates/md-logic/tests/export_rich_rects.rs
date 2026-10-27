//! Port of md.win `tests/Md.App.Logic.Tests/Export/RichRectsTests.cs`.
//!
//! The measurement the EPUB pairs its images by. The two rules here are the difference between a
//! correct book and one where every figure after the first is on the wrong page.

use md_logic::export::RichRect;
use md_logic::export::rich_rects;
use md_logic::testing::RectD;

fn rect(x: f64, y: f64, width: f64, height: f64) -> RectD {
    RectD { x, y, width, height }
}

#[test]
fn five_number_rows_become_rects_in_the_order_the_page_gave_them() {
    let rects = rich_rects::parse(Some("[[10,20,30,40,1],[50,60,70,80,0]]"));

    assert_eq!(
        rects,
        [
            RichRect { rect: rect(10.0, 20.0, 30.0, 40.0), is_math: true },
            RichRect { rect: rect(50.0, 60.0, 70.0, 80.0), is_math: false },
        ]
    );
}

#[test]
fn a_degenerate_rect_is_kept_because_dropping_it_shifts_every_later_image() {
    let rects = rich_rects::parse(Some("[[0,0,0,0,0],[1,2,3,4,1]]"));

    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0].rect, rect(0.0, 0.0, 0.0, 0.0));
}

#[test]
fn a_row_that_is_not_five_numbers_is_dropped_rather_than_guessed_at() {
    // The Mac's `count != 5` guard: four columns is iOS's shape, six is nobody's.
    assert!(rich_rects::parse(Some("[[1,2,3,4]]")).is_empty());
    assert!(rich_rects::parse(Some("[[1,2,3,4,5,6]]")).is_empty());
    assert!(rich_rects::parse(Some("[[1,2,3,4,\"1\"]]")).is_empty());
    assert!(rich_rects::parse(Some("[[1,2,3,4,null]]")).is_empty());
    assert!(rich_rects::parse(Some("[\"nonsense\"]")).is_empty());
}

#[test]
fn anything_that_is_not_an_array_of_rows_is_no_measurement_at_all() {
    for json in [
        None,
        Some(""),
        Some("null"),         // the page went away, or the script threw
        Some("{\"els\":[]}"), // Android's shape, which this port does not use
        Some("[[1,2,3,"),     // truncated
    ] {
        assert!(rich_rects::parse(json).is_empty(), "{json:?}");
    }
}

#[test]
fn a_non_finite_number_is_not_a_measurement() {
    // JSON has no NaN literal, but a page can still answer with something unusable.
    assert!(rich_rects::parse(Some("[[1,2,3,4,1e400]]")).is_empty());
}

#[test]
fn the_fifth_column_is_whether_it_is_a_formula() {
    let rects = rich_rects::parse(Some("[[0,0,1,1,1],[0,0,1,1,0]]"));

    assert!(rects[0].is_math);
    assert!(!rects[1].is_math);
}
