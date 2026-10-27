//! Port of md.win `Export/PrintGeometry.cs`: the other half of the [`PrintGeometry`] the seams
//! froze — the conversion that needs md-core's [`PageSize`]. Kept apart so the seam stays free of
//! md-core types (md.win's partial record; here an inherent `impl` in this module).

use md_core::document::page_size::PageSize;

use crate::seams::PrintGeometry;

impl PrintGeometry {
    /// PostScript points to the inch. The whole size table is in points; the print settings are in
    /// inches.
    pub const POINTS_PER_INCH: f64 = 72.0;

    /// The margin decision, all four sides (§7.3, §14 row 12). The Mac inherits Page Setup, Android
    /// sets 0.5 in, md.vscode zero — md.win follows Android, for Android's reason: the body's CSS
    /// padding wraps the *whole document*, so with a zero printer margin pages 2…n would touch the
    /// paper edge. This port keeps md.win's number; on Linux it reaches
    /// `gtk::PageSetup::set_{top,bottom,left,right}_margin(0.5, gtk::Unit::Inch)` (measured in the
    /// spike: the first word's `xMin` is 36 pt + the body padding, docs/linux-facts.md §5).
    pub const DEFAULT_MARGIN_IN: f64 = 0.5;

    /// A page of `width_in` × `height_in` with the default margin — md.win's positional record
    /// constructor with `MarginIn = 0.5` defaulted, so a geometry built by hand cannot lose it.
    pub fn new(page_width_in: f64, page_height_in: f64) -> Self {
        PrintGeometry { page_width_in, page_height_in, margin_in: Self::DEFAULT_MARGIN_IN }
    }

    /// The paper a [`PageSize`] asks for, in inches (md.win's `For`). The size table is the
    /// family's, to a tenth of a point (A4 stays 595.2 × 841.8), so the division is the only
    /// arithmetic between the shared table and the print settings.
    pub fn for_page(size: &PageSize) -> Self {
        PrintGeometry {
            page_width_in: size.width / Self::POINTS_PER_INCH,
            page_height_in: size.height / Self::POINTS_PER_INCH,
            margin_in: Self::DEFAULT_MARGIN_IN,
        }
    }

    /// The paper Print means: always A4, never `md.pdfPageSize` — the trim size is an export-file
    /// concern, and macOS, Android and md.win agree on that.
    ///
    /// On Linux too nothing passes this to a printer: GTK's print dialog
    /// (`webkit::PrintOperation::run_dialog`) holds the reader's own paper, and the app hands it no
    /// page setup. This is the written statement of the default.
    pub fn paper() -> Self {
        Self::for_page(&PageSize::A4)
    }
}
