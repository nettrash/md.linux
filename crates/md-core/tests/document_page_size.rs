//! Port of md.win `PageSizeTests`: the three PageSize cases in md.macOS/mdTests/mdTests.swift plus
//! the exact seven paddings the Kotlin suite pins (the Swift pins A4 and Letter and "strictly
//! smaller" for the four small pages).

use md_core::document::page_size::PageSize;

#[test]
fn page_size_table_has_the_agreed_point_dimensions() {
    fn dims(id: &str) -> (f64, f64) {
        let size = PageSize::named(Some(id));
        (size.width, size.height)
    }

    assert_eq!((595.2, 841.8), dims("a4"));
    assert_eq!((419.5, 595.3), dims("a5"));
    assert_eq!((612.0, 792.0), dims("letter"));
    assert_eq!((612.0, 1008.0), dims("legal"));
    assert_eq!((432.0, 648.0), dims("6x9"));
    assert_eq!((360.0, 576.0), dims("5x8"));
    assert_eq!((396.0, 612.0), dims("5.5x8.5"));

    // The offered list is exactly these seven, A4 first — the default.
    assert_eq!(
        vec!["a4", "a5", "letter", "legal", "6x9", "5x8", "5.5x8.5"],
        PageSize::all().iter().map(|s| s.id).collect::<Vec<_>>()
    );
    assert_eq!(PageSize::A4, PageSize::all()[0]);
    assert!(std::ptr::eq(PageSize::named(Some("a4")), &PageSize::all()[0]));

    // Labels are the menu titles — the imperial ones carry a real × (U+00D7) and a straight inch mark.
    assert_eq!("A4", PageSize::A4.label);
    assert_eq!("US Letter", PageSize::US_LETTER.label);
    assert_eq!("US Legal", PageSize::US_LEGAL.label);
    assert_eq!("6 × 9\"", PageSize::SIX_BY_NINE.label);
    assert_eq!("5 × 8\"", PageSize::FIVE_BY_EIGHT.label);
    assert_eq!("5.5 × 8.5\"", PageSize::DIGEST.label);
}

#[test]
fn page_size_preference_round_trips_and_defaults_to_a4() {
    for size in PageSize::all() {
        assert_eq!(size, PageSize::named(Some(size.id)));
        assert!(std::ptr::eq(size, PageSize::named(Some(size.id))));
    }
    // An empty (first launch), absent or unknown (stale / future-version) key falls back to A4 — never None.
    assert_eq!(&PageSize::A4, PageSize::named(Some("")));
    assert_eq!(&PageSize::A4, PageSize::named(None));
    assert_eq!(&PageSize::A4, PageSize::named(Some("tabloid")));
    assert_eq!(&PageSize::A4, PageSize::named(Some("a6")));
    // Ids are lower-case; the wrong case is unknown (Kotlin pins this).
    assert_eq!(&PageSize::A4, PageSize::named(Some("A4")));
    assert_eq!("a4", PageSize::DEFAULT_ID);
    assert_eq!("md.pdfPageSize", PageSize::PREFERENCE_KEY);
}

#[test]
fn a4_margin_is_unchanged_and_smaller_pages_scale_the_margin_down() {
    // A4 reproduces the historical body margin to the pixel, so an A4 export carries the exact CSS it always did.
    assert_eq!("48px 56px", PageSize::A4.css_padding());

    // Every smaller page gets a strictly smaller margin on both axes.
    for size in [PageSize::A5, PageSize::SIX_BY_NINE, PageSize::FIVE_BY_EIGHT, PageSize::DIGEST] {
        let padding = size.css_padding();
        let parts: Vec<&str> = padding.split(' ').collect();
        assert_eq!(2, parts.len());
        let vertical: i32 = parts[0].strip_suffix("px").unwrap().parse().unwrap();
        let horizontal: i32 = parts[1].strip_suffix("px").unwrap().parse().unwrap();
        assert!(vertical < 48, "{} vertical margin should shrink", size.id);
        assert!(horizontal < 56, "{} horizontal margin should shrink", size.id);
        assert!(vertical > 0);
        assert!(horizontal > 0);
    }
    // Letter is wider than A4, so its horizontal margin grows — proportional per axis, not a blanket cap.
    assert_eq!("45px 58px", PageSize::US_LETTER.css_padding());

    // The exact seven, as the Kotlin suite pins them.
    assert_eq!("34px 39px", PageSize::A5.css_padding());
    assert_eq!("57px 58px", PageSize::US_LEGAL.css_padding());
    assert_eq!("37px 41px", PageSize::SIX_BY_NINE.css_padding());
    assert_eq!("33px 34px", PageSize::FIVE_BY_EIGHT.css_padding());
    assert_eq!("35px 37px", PageSize::DIGEST.css_padding());
}
