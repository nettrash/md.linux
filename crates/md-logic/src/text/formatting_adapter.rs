//! Port of md.win `Text/FormattingAdapter.cs`: the editor's half of the formatting commands
//! (formatting.md) — which [`FormatOp`] a command runs, the edit it makes of the buffer's text, and
//! whether a keystroke wraps the selection. The pane applies an answer as one replacement in its
//! own user action: one undo unit.
//!
//! LINUX DECISION (line breaks): md.win hands the `TextBox`'s text over with CR swapped for LF and
//! the replacement back with LF swapped for CR, because WinUI stores a line break as U+000D. The
//! `GtkTextBuffer` holds LF, which is what md-core's `markdown_formatting` reads, so the text goes
//! in and the edit comes out unchanged. Offsets are UTF-16 units, selections `isize`, as in
//! [`smart_typing_adapter`](super::smart_typing_adapter).
//!
//! LINUX DECISION (AltGr): md.win's `HeadingChordGuard` is not ported. On Windows AltGr arrives as
//! Ctrl+Alt, so AltGr+2 (² on German) matched the Heading 2 accelerator and needed a guard. GTK
//! reports AltGr as `ISO_Level3_Shift` (the Mod5 / Level 3 modifier), never as Control+Alt, and the
//! keyval it produces is the typed character (`twosuperior`), so it can never match a
//! `<Control><Alt>2` accelerator: the character is typed and no heading is made, with no code.

use std::sync::OnceLock;

use md_core::editing::markdown_formatting::{self, FormatEdit, FormatOp};
use md_core::text::utf16::{Utf16Text, utf16_len};

use super::smart_typing_adapter::{insertion_over_selection, selection_in};
use crate::commands::{CommandId, command_table};

/// The [`FormatOp`] a Format command runs; `None` for any other command.
pub fn op_for(id: CommandId) -> Option<FormatOp> {
    use CommandId as C;
    Some(match id {
        C::Bold => FormatOp::Bold,
        C::Italic => FormatOp::Italic,
        C::InlineCode => FormatOp::Code,
        C::Strikethrough => FormatOp::Strikethrough,
        C::Link => FormatOp::Link,
        C::Heading1 => FormatOp::Heading1,
        C::Heading2 => FormatOp::Heading2,
        C::Heading3 => FormatOp::Heading3,
        C::Heading4 => FormatOp::Heading4,
        C::Heading5 => FormatOp::Heading5,
        C::Heading6 => FormatOp::Heading6,
        C::BulletList => FormatOp::BulletList,
        C::TaskList => FormatOp::TaskList,
        C::Quote => FormatOp::Quote,
        C::Indent => FormatOp::Indent,
        C::Outdent => FormatOp::Outdent,
        _ => return None,
    })
}

/// Every Format command, in table order — what the windows register.
pub fn commands() -> &'static [CommandId] {
    static COMMANDS: OnceLock<Vec<CommandId>> = OnceLock::new();
    COMMANDS
        .get_or_init(|| command_table::all().iter().map(|spec| spec.id).filter(|&id| op_for(id).is_some()).collect())
}

/// The edit `op` makes of the buffer's text and selection, or `None` for none (nothing to push on
/// the undo stack) — including a selection the text cannot hold.
pub fn edit(box_text: &str, selection_start: isize, selection_length: isize, op: FormatOp) -> Option<FormatEdit> {
    selection_in(utf16_len(box_text), selection_start, selection_length)?;
    markdown_formatting::format(box_text, selection_start, selection_start + selection_length, op)
}

/// The question asked before a change lands: is this change one of `* _ ` $` typed over a
/// non-empty selection? Then the wrap it becomes (over the original selection — [`after_insertion`]
/// moves it onto the inserted character), else `None`. Exactly one unit replacing the whole
/// selection — a paste of `*` is the caller's to exclude, as are a composition and history.
pub fn wrap(box_text: &str, selection_start: isize, selection_length: isize, new_text: &str) -> Option<FormatEdit> {
    if selection_length <= 0 {
        return None;
    }
    selection_in(utf16_len(box_text), selection_start, selection_length)?;
    let inserted = insertion_over_selection(box_text, selection_start, selection_length, new_text)?;
    // Asked on every keystroke over a selection: anything but the four is answered before the
    // text is converted.
    if !matches!(inserted.as_str(), "*" | "_" | "`" | "$") {
        return None;
    }
    markdown_formatting::wrap_typed(box_text, selection_start, selection_start + selection_length, &inserted)
}

/// The wrap as the buffer will need it once the text view has inserted the typed character itself:
/// the same replacement and selection, over `[start, start + 1)` — the one character that now
/// stands where the selection was. The view's insertion is the first undo unit, this the second,
/// as with SmartTyping's capital (§3.5): the keystroke is never cancelled, so it can never be lost.
pub fn after_insertion(wrap: &FormatEdit) -> FormatEdit {
    FormatEdit { end: wrap.start + 1, ..wrap.clone() }
}

/// Whether [`after_insertion`]'s edit still fits the buffer: the typed character is at `start` and
/// the cursor is collapsed just after it. Anything else — the input was transformed, another change
/// got in first — and the plan is dropped, leaving the character typed.
pub fn still_applies(pending: &FormatEdit, box_text: &str, selection_start: isize, selection_length: isize) -> bool {
    let units = Utf16Text::new(box_text);
    pending.end == pending.start + 1
        && pending.end <= units.len()
        && pending.text.encode_utf16().next() == Some(units.units()[pending.start])
        && selection_length == 0
        && selection_start == pending.end as isize
}
