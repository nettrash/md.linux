//! The name every export suggests in its save dialog, from the document's title (md.win
//! `Md.Core.Export.ExportFileNames`).
//!
//! Two layers, deliberately separable. [`portable_stem`] is the family rule every port shares
//! (macOS `DocumentExport.sanitized`, Android `Exporter.sanitized`, md.vscode `sanitized`, md.win
//! `PortableStem`): split on `/ \ : ? % * | " < >`, join with `-`, trim, and fall back to
//! `Document` when nothing is left. It is a *split-join*, so a run of two offending characters
//! becomes **two** dashes, not one — pinned by every port.
//!
//! [`sanitized`] adds what Linux refuses on top — and only that, because the Mac, the family's
//! source of truth, composes `"\(sanitized(title)).<ext>"` with nothing more:
//!
//! - **NUL** (U+0000), the one character besides '/' a Linux file name cannot hold (and one no C
//!   string — GTK's included — can carry). It becomes the same `-` the family set becomes. ('/' is
//!   already in the family set.)
//! - **`.` and `..`**, the two names every directory already holds. Only a title of dots with no
//!   extension can come out as one; it falls back to `Document`.
//!
//! md.win's Windows layer is deliberately *not* ported: control characters other than NUL, a
//! trailing dot or space, and the device names `CON PRN AUX NUL COM1–9 LPT1–9` are all ordinary on
//! Linux (and on the Mac), so `CON`, `Notes...` and a title with a line break keep their names
//! here exactly as they do on macOS. Not handled either: component length (255 bytes) and total
//! path length, which only the caller, holding the folder, can judge.

use crate::text::whitespace;

/// The name every port falls back to when a title sanitises away to nothing.
pub const FALLBACK: &str = "Document";

/// The characters the family replaces, ASCII, in the order the Swift spells them.
const FORBIDDEN: [char; 10] = ['/', '\\', ':', '?', '%', '*', '|', '"', '<', '>'];

/// The family's file-name stem, byte for byte with macOS, Android, md.vscode and md.win: no
/// extension, no platform rules. Use it to compare this port against the others; use
/// [`sanitized`] for a name the save dialog will be given.
pub fn portable_stem(title: &str) -> String {
    let cleaned = cleaned(title, false);
    if cleaned.is_empty() { FALLBACK.to_string() } else { cleaned }
}

/// A file name Linux will take: [`portable_stem`] plus Linux's rules, then `.` and `extension`
/// (given with or without its leading dot; empty for none). The extension gets the stem's own
/// rule — the family characters, NUL, the family trim — and loses any leading dots, so `svg`,
/// `.svg` and `..svg` all mean `.svg` and an extension of only dots or spaces means none.
pub fn sanitized(title: &str, extension: &str) -> String {
    let mut stem = cleaned(title, true);
    if stem.is_empty() {
        stem = FALLBACK.to_string();
    }
    let suffix = cleaned(extension, true);
    let suffix = suffix.trim_start_matches('.');
    let name = if suffix.is_empty() { stem } else { format!("{stem}.{suffix}") };
    if name == "." || name == ".." { FALLBACK.to_string() } else { name }
}

/// The family rule, without the fallback: every forbidden character (and, for Linux, NUL)
/// becomes `-`, then the whitespace-and-newlines set is trimmed. `whitespace::trim_wsnl`, not
/// `str::trim` — the two differ on U+200B, which Foundation's frozen tables still call a space
/// separator, so a title padded with zero-width spaces trims here exactly as it does on macOS.
fn cleaned(text: &str, linux: bool) -> String {
    let replaced: String =
        text.chars().map(|c| if FORBIDDEN.contains(&c) || (linux && c == '\0') { '-' } else { c }).collect();
    whitespace::trim_wsnl(&replaced).to_string()
}
