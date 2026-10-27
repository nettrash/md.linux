//! Port of md.win `Export/RichRects.cs`: the JSON [`scripts::rich_elements`] hands back — an array
//! of five-number rows, in DOM order.
//!
//! Two rules, both of which cost a corrupted book if they are got wrong. Rows that are not five
//! numbers are **dropped** (the Mac's `count != 5` guard — a page that answered with something else
//! is not a measurement). Degenerate rects — zero width, zero height — are **kept**: the list must
//! pair 1:1 with the containers in the markup, and dropping one shifts every later image onto the
//! wrong element. That pairing is then checked against the plan's own count before a single pixel
//! is captured.
//!
//! Nothing panics. A script that failed, a page that went away and a malformed answer are all "no
//! measurements", and the count check turns that into the named error.
//!
//! [`scripts::rich_elements`]: crate::preview::scripts::rich_elements

use crate::preview::json_script::{self, Json};
use crate::seams::RectD;

/// One measured rich element: its page-space rectangle in CSS pixels, and whether it is a formula.
///
/// The fifth column decides the EPUB's `alt` text on macOS; md-core derives it from its own markup
/// scan instead, so this is carried for anyone reading a measurement, not for the file written.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RichRect {
    pub rect: RectD,
    pub is_math: bool,
}

/// The five columns, in the order the script emits them.
pub const COLUMNS: usize = 5;

/// The measurements, or an empty list for anything that is not an array of rows.
pub fn parse(json: Option<&str>) -> Vec<RichRect> {
    let Some(json) = json.filter(|json| !json.is_empty()) else { return Vec::new() };
    let Some(Json::Array(rows)) = json_script::parse(json) else { return Vec::new() };
    rows.iter()
        .filter_map(|row| {
            let Json::Array(cells) = row else { return None };
            if cells.len() != COLUMNS {
                return None;
            }
            let mut values = [0.0; COLUMNS];
            for (value, cell) in values.iter_mut().zip(cells) {
                match cell {
                    // An overflowing literal parses to infinity (JsonElement.TryGetDouble's answer
                    // too) and is no measurement.
                    Json::Number(number) if number.is_finite() => *value = *number,
                    _ => return None,
                }
            }
            Some(RichRect {
                rect: RectD { x: values[0], y: values[1], width: values[2], height: values[3] },
                is_math: values[4] != 0.0,
            })
        })
        .collect()
}
