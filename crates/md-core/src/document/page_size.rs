//! A named PDF page ("trim") size (md.win `Document/PageSize.cs`).

/// A named PDF page size in PostScript points, 1 in = 72 pt, portrait. Port of the table in
/// md.macOS/md/DocumentExport.swift, copied verbatim by every port — a drift here paginates the
/// same document differently on one platform. A4 keeps the historical 595.2 × 841.8 (210 × 297 mm
/// to a tenth of a point, what the app paginated to before trim sizes existed) so the A4 export is
/// byte-for-byte what it always was; do not "fix" it to 595.28 × 841.89.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    pub id: &'static str,
    pub label: &'static str,
    pub width: f64,
    pub height: f64,
}

/// Every offered size, in menu order — A4 first, since it is the default.
static ALL: [PageSize; 7] = [
    PageSize::A4,
    PageSize::A5,
    PageSize::US_LETTER,
    PageSize::US_LEGAL,
    PageSize::SIX_BY_NINE,
    PageSize::FIVE_BY_EIGHT,
    PageSize::DIGEST,
];

impl PageSize {
    // Labels carry the real multiplication sign U+00D7 and a straight inch mark.
    pub const A4: PageSize = PageSize { id: "a4", label: "A4", width: 595.2, height: 841.8 };
    pub const A5: PageSize = PageSize { id: "a5", label: "A5", width: 419.5, height: 595.3 };
    pub const US_LETTER: PageSize = PageSize { id: "letter", label: "US Letter", width: 612.0, height: 792.0 };
    pub const US_LEGAL: PageSize = PageSize { id: "legal", label: "US Legal", width: 612.0, height: 1008.0 };
    pub const SIX_BY_NINE: PageSize = PageSize { id: "6x9", label: "6 \u{00D7} 9\"", width: 432.0, height: 648.0 };
    pub const FIVE_BY_EIGHT: PageSize = PageSize { id: "5x8", label: "5 \u{00D7} 8\"", width: 360.0, height: 576.0 };
    pub const DIGEST: PageSize = PageSize { id: "5.5x8.5", label: "5.5 \u{00D7} 8.5\"", width: 396.0, height: 612.0 };

    /// The app-wide setting (`@AppStorage("md.pdfPageSize")`). Holds an [`PageSize::id`].
    pub const PREFERENCE_KEY: &'static str = "md.pdfPageSize";

    /// First-launch value: `"a4"` (A4's id).
    pub const DEFAULT_ID: &'static str = PageSize::A4.id;

    /// Every offered size, in menu order — A4 first, since it is the default. The same statics
    /// [`PageSize::named`] hands out, so a size can be compared by address as well as by value.
    pub fn all() -> &'static [PageSize; 7] {
        &ALL
    }

    /// The size stored under `id`, A4 for an empty, unknown or absent key (first launch, or a
    /// preference from a future build). Exact, case-sensitive match like the Swift — `"A4"` is
    /// unknown (the Kotlin suite pins that too).
    pub fn named(id: Option<&str>) -> &'static PageSize {
        ALL.iter().find(|size| Some(size.id) == id).unwrap_or(&ALL[0])
    }

    /// The export body margin (CSS `padding`) scaled per axis from A4's `48px 56px`, so a 6 × 9"
    /// booklet does not wear A4 margins and Letter, wider than A4, grows horizontally. Whole
    /// pixels, rounded half away from zero (Swift `.rounded()`, and Rust's `f64::round`; .NET's
    /// default would be banker's) — the integer string is what keeps the A4 case byte-identical to
    /// the historical CSS.
    pub fn css_padding(&self) -> String {
        let vertical = (48.0 * self.height / PageSize::A4.height).round() as i64;
        let horizontal = (56.0 * self.width / PageSize::A4.width).round() as i64;
        format!("{vertical}px {horizontal}px")
    }
}
