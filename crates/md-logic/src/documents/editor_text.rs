//! Port of md.win `Documents/EditorText.cs`.
//!
//! The adapter between the editor widget and the session (§3.2). md.win's is for a WinUI
//! `TextBox`, which reports every line break as `\r` whatever was assigned.
//!
//! LINUX DECISION: the names say "editor", not "text box". A `GtkTextBuffer` keeps exactly the
//! characters it was given, so an LF model assigned to it reads back as LF — but a paste or a drop
//! can bring a CR or a CRLF in, and the buffer keeps that too. The model is therefore still
//! normalised on the way in ([`from_editor`]) and left alone on the way out ([`to_editor`]), which
//! is correct for either widget. Selection offsets here are UTF-16 units, as everywhere in
//! md-logic (see [`super::line_offsets`]).

use super::line_endings;

/// What the editor reports, as the model wants it: LF only.
pub fn from_editor(editor_text: &str) -> String {
    line_endings::normalize(editor_text)
}

/// What to assign to the editor. The identity: a round trip through [`from_editor`] gives the
/// model back unchanged. Named so a reader of the pane sees both directions and neither is "just
/// the string".
pub fn to_editor(model_text: &str) -> &str {
    model_text
}

/// The selection to restore after an external replace (Revert, Reload from Disk, an example
/// load), clamped to the new text the Mac's way: the caret first, then what is left of the length.
/// Never panics for a caret past the end of a shorter document. (md.win throws for a negative text
/// length; a `usize` cannot be one.)
pub fn clamp_selection(start: i64, length: i64, text_length: usize) -> (usize, usize) {
    let total = i64::try_from(text_length).unwrap_or(i64::MAX);
    let s = start.clamp(0, total);
    let l = length.clamp(0, total - s);
    (s as usize, l as usize)
}
