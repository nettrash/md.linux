//! Port of md.win `tests/Md.App.Logic.Tests/Export/PrintGeometryTests.cs`.
//!
//! The one piece of arithmetic between the family's shared point table and the print settings,
//! which take inches (on Linux `gtk::PaperSize::new_custom(…, gtk::Unit::Inch)` and
//! `gtk::PageSetup`'s margins) — plus the margin, which is a decision md.win made and the only
//! number in the export path no other port would recognise.

use md_core::document::page_size::PageSize;
use md_logic::testing::PrintGeometry;

fn assert_close(expected: f64, actual: f64) {
    assert!((expected - actual).abs() < 1e-10, "{expected} vs {actual}");
}

#[test]
fn inches_are_points_over_seventy_two_for_every_size_in_the_table() {
    for size in PageSize::all() {
        let geometry = PrintGeometry::for_page(size);
        assert_close(size.width / 72.0, geometry.page_width_in);
        assert_close(size.height / 72.0, geometry.page_height_in);
    }
}

#[test]
fn a4_and_six_by_nine_are_the_numbers_the_self_test_looks_for_in_the_media_box() {
    let a4 = PrintGeometry::for_page(&PageSize::A4);
    assert_close(595.2 / 72.0, a4.page_width_in);
    assert_close(841.8 / 72.0, a4.page_height_in);

    // 6 × 9 inches is exactly that, which is why it is the second size the PDF check uses (and the
    // spike measured it: 432 × 648 pt, docs/linux-facts.md §5).
    assert_eq!(
        PrintGeometry::for_page(&PageSize::SIX_BY_NINE),
        PrintGeometry { page_width_in: 6.0, page_height_in: 9.0, margin_in: 0.5 }
    );
}

#[test]
fn every_side_gets_half_an_inch_and_that_is_the_default() {
    assert_eq!(PrintGeometry::DEFAULT_MARGIN_IN, 0.5);
    for size in PageSize::all() {
        assert_eq!(PrintGeometry::for_page(size).margin_in, 0.5);
    }

    // The constructor's own default agrees, so a geometry built by hand cannot lose the margin.
    assert_eq!(PrintGeometry::new(8.5, 11.0).margin_in, 0.5);
}

#[test]
fn print_is_always_a4_whatever_the_export_page_size_says() {
    assert_eq!(PrintGeometry::paper(), PrintGeometry::for_page(&PageSize::A4));
}

#[test]
#[ignore = "md.win pins ArgumentNullException for a null PageSize; a Rust reference cannot be null"]
fn a_null_size_is_a_refusal_rather_than_a_zero_sized_page() {}
