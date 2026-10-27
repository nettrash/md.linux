//! Port of md.win `Documents/RescueCopy.cs` (with its `FileFailure` helper).
//!
//! The last-resort save (§6.3): when the real write is refused right as a window closes —
//! read-only file, conflict, vanished folder, full disk — the keystrokes go to a fresh sibling
//! instead of nowhere. A rescue is a copy, not a save: the buffer stays dirty and the original is
//! never touched.

use std::io;

use super::file_names;
use crate::seams::FileSystem;
use crate::strings;

/// Names tried before giving up; the Mac's hundred.
pub const MAX_ATTEMPTS: usize = 100;

/// `"01-Scene (rescued).md"`, then `"(rescued 2)"`, `"(rescued 3)"`, … — never `"(rescued 1)"`. A
/// file with no extension is rescued as `.md`, as the Mac does (`url.pathExtension.isEmpty ? "md"
/// : …`), and the two Foundation splits the Mac builds this from disagree on odd names —
/// [`file_names`] mirrors each.
///
/// # Panics
///
/// For `attempt == 0` (md.win throws `ArgumentOutOfRangeException`).
pub fn name_for(file_name: &str, attempt: usize) -> String {
    assert!(attempt >= 1, "rescue attempts count from 1");
    let name = file_names::name_of(file_name);
    let mut extension = file_names::extension_of(name);
    if extension.is_empty() {
        extension = ".md".to_string();
    }
    strings::documents::rescue_copy_name(file_names::stem_of(name), &extension, attempt)
}

/// Write `data` beside `path` under the first free rescue name. Returns the path written, or
/// `None` when the write failed (immediately — a failing folder will fail a hundred times too) or
/// a hundred names were taken. Never overwrites: a directory squatting on the name counts as
/// taken, exactly as Foundation's `fileExists(atPath:)` does.
pub fn write(file_system: &dyn FileSystem, path: &str, data: &[u8]) -> Option<String> {
    let folder = file_names::directory_of(path);
    for attempt in 1..=MAX_ATTEMPTS {
        let candidate = file_names::combine(folder, &name_for(path, attempt));
        if exists(file_system, &candidate) {
            continue;
        }
        return file_system.write_all_bytes_in_place(&candidate, data).ok().map(|()| candidate);
    }
    None
}

fn exists(file_system: &dyn FileSystem, candidate: &str) -> bool {
    file_system.file_exists(candidate) || file_system.directory_exists(candidate)
}

/// md.win's `FileFailure.Message`: the message to show, never empty (an empty one would render as
/// a blank bar). Every `io::Error` is a file failure here — C#'s exception filter has no Rust
/// counterpart: a bug panics instead of arriving as an error value.
pub(crate) fn failure_message(error: &io::Error, fallback: &str) -> String {
    let message = error.to_string();
    if message.is_empty() { fallback.to_string() } else { message }
}
