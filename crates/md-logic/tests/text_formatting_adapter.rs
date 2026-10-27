//! Port of md.win `tests/Md.App.Logic.Tests/FormattingAdapterTests.cs`: the editor's half of the
//! formatting commands (1.6; formatting.md) — the command → op map, the buffer's line breaks on
//! both sides of md-core's `markdown_formatting`, the typed-wrap plan and its two-phase
//! application, the setting, and when the Format rows are live. The rules themselves are pinned by
//! the shared vectors in md-core.
//!
//! LINUX: the buffer's line break is LF (md.win's `TextBox` stores CR), so the three CR tests are
//! ported with LF in and LF out; the AltGr guard has no Linux counterpart (see
//! `text::formatting_adapter`).

use md_core::editing::markdown_formatting::{FormatEdit, FormatOp};
use md_logic::commands::{CommandId, KeyModifiers, ShellSnapshot, command_enablement, command_table};
use md_logic::settings::{SettingsStore, TypingSettings, settings_keys, wrap_selection_setting};
use md_logic::testing::FakeSettingsStore;
use md_logic::text::formatting_adapter::{after_insertion, commands, edit, op_for, still_applies, wrap};

fn fe(start: usize, end: usize, text: &str, selection_start: usize, selection_end: usize) -> FormatEdit {
    FormatEdit { start, end, text: text.to_string(), selection_start, selection_end }
}

// ── Commands ───────────────────────────────────────────────────────────────────────────────

#[test]
fn every_format_command_has_its_op_and_nothing_else_has_one() {
    assert_eq!(commands().len(), 16);
    assert_eq!(op_for(CommandId::InlineCode), Some(FormatOp::Code));
    assert_eq!(op_for(CommandId::Heading6), Some(FormatOp::Heading6));
    assert_eq!(op_for(CommandId::Outdent), Some(FormatOp::Outdent));
    // HeadingCycle is the phone bar's; no desktop command runs it.
    assert!(!commands().iter().any(|&id| op_for(id) == Some(FormatOp::HeadingCycle)));
    for id in CommandId::ALL.iter().filter(|id| !commands().contains(id)) {
        assert_eq!(op_for(*id), None, "{id:?}");
    }
}

// ── The buffer's line breaks ───────────────────────────────────────────────────────────────

/// LINUX: md.win's "ALineCommandReadsAndWritesCr" — the line command reads and writes the buffer's
/// own line break, LF.
#[test]
fn a_line_command_reads_and_writes_cr() {
    // "a\nb" selected whole: two lines to the buffer, two lines to the rules.
    let e = edit("a\nb", 0, 3, FormatOp::BulletList).unwrap();
    assert_eq!(e, fe(0, 3, "- a\n- b", 2, 7));
    assert_eq!(e.apply_to("a\nb"), "- a\n- b");
    assert!(!e.text.contains('\r'));
}

/// LINUX: md.win's "AnInlineCommandOverACrKeepsItsOffsets", over LF.
#[test]
fn an_inline_command_over_a_cr_keeps_its_offsets() {
    // The trimmed wrap stops before the line break.
    assert_eq!(edit("aword\nb", 1, 4, FormatOp::Bold), Some(fe(1, 5, "**word**", 3, 7))); // "word"
    assert_eq!(edit("aword\nb", 1, 5, FormatOp::Bold), Some(fe(1, 5, "**word**", 3, 7))); // "word\n"
}

#[test]
fn a_selection_outside_the_box_is_no_edit() {
    assert_eq!(edit("abc", 2, 5, FormatOp::Bold), None);
    assert_eq!(edit("abc", -1, 0, FormatOp::Bold), None);
}

#[test]
fn outdent_with_nothing_to_remove_is_no_edit() {
    assert_eq!(edit("text", 2, 0, FormatOp::Outdent), None);
}

// ── The typed wrap ─────────────────────────────────────────────────────────────────────────

#[test]
fn one_of_the_four_typed_over_a_selection_plans_a_wrap() {
    for typed in ["*", "_", "`", "$"] {
        // "say hello now", "hello" selected, the buffer about to hold "say * now".
        let w = wrap("say hello now", 4, 5, &format!("say {typed} now"));
        assert_eq!(w, Some(fe(4, 9, &format!("{typed}hello{typed}"), 5, 10)), "{typed}");
    }
}

#[test]
fn anything_else_is_not_a_wrap() {
    let rows: [(&str, isize, isize, &str); 5] = [
        ("say hello now", 4, 0, "say *hello now"), // a caret: ordinary typing
        ("say hello now", 4, 5, "say ~ now"),      // another character
        ("say hello now", 4, 5, "say ** now"),     // two units: a paste, not a keystroke
        ("say hello now", 4, 5, "say  now"),       // a deletion
        ("say hello now", 4, 5, "*say hello now"), // a change somewhere else
    ];
    for (text, start, length, new_text) in rows {
        assert_eq!(wrap(text, start, length, new_text), None, "{new_text:?}");
    }
}

/// LINUX: md.win's "AWrapAcrossLinesKeepsTheBoxsCr" — the buffer's LF is kept.
#[test]
fn a_wrap_across_lines_keeps_the_boxs_cr() {
    let w = wrap("a\nb", 0, 3, "`").unwrap();
    assert_eq!(w.text, "`a\nb`");
}

#[test]
fn after_the_control_inserted_the_character_the_wrap_replaces_just_that_character() {
    let w = wrap("say hello now", 4, 5, "say * now").unwrap();
    let pending = after_insertion(&w);
    assert_eq!(pending, fe(4, 5, "*hello*", 5, 10));

    // The buffer now holds the typed star, cursor after it: the plan fits, and lands as the spec says.
    assert!(still_applies(&pending, "say * now", 5, 0));
    assert_eq!(pending.apply_to("say * now"), "say *hello* now");

    // Anything else drops it, leaving the star as typed.
    assert!(!still_applies(&pending, "say * now", 4, 0)); // caret elsewhere
    assert!(!still_applies(&pending, "say * now", 5, 1)); // a selection
    assert!(!still_applies(&pending, "say _ now", 5, 0)); // another character
    assert!(!still_applies(&pending, "say", 5, 0)); // shorter text
}

// ── The AltGr guard ────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "HeadingChordGuard is not ported: GTK delivers AltGr as ISO_Level3_Shift (Mod5), never as Ctrl+Alt, \
            so an AltGr character can never match a <Control><Alt>digit accelerator"]
fn a_heading_chord_lets_alt_gr_type() {
    // md.win: HeadingChordGuard.Formats(layoutTypesCharacter, rightAlt, leftAlt) over six rows —
    // nothing on Linux to call.
}

/// The second half of md.win's test, which is about the table rather than the guard: the six
/// heading chords are the only Ctrl+Alt ones there, so they are the only ones AltGr could ever have
/// collided with — on GTK it cannot (see `text::formatting_adapter`).
#[test]
fn the_guard_looks_only_at_the_headings() {
    let headings = [
        CommandId::Heading1,
        CommandId::Heading2,
        CommandId::Heading3,
        CommandId::Heading4,
        CommandId::Heading5,
        CommandId::Heading6,
    ];
    let ctrl_alt = KeyModifiers::CTRL | KeyModifiers::ALT;
    let with_ctrl_alt: Vec<CommandId> = command_table::all()
        .iter()
        .filter(|spec| spec.chord.is_some_and(|c| c.modifiers == ctrl_alt))
        .map(|spec| spec.id)
        .collect();
    assert_eq!(with_ctrl_alt, headings);
    for (id, digit) in headings.iter().zip(["1", "2", "3", "4", "5", "6"]) {
        assert_eq!(command_table::spec(*id).chord.unwrap().accelerator(), format!("<Control><Alt>{digit}"));
    }
}

// ── The setting ────────────────────────────────────────────────────────────────────────────

#[test]
fn wrap_selection_is_on_until_it_is_turned_off() {
    let store = FakeSettingsStore::new();
    assert!(wrap_selection_setting::read(&store));
    wrap_selection_setting::toggle(&store);
    assert!(!wrap_selection_setting::read(&store));
    assert!(!store.get_bool(settings_keys::WRAP_SELECTION, true));
    wrap_selection_setting::toggle(&store);
    assert!(wrap_selection_setting::read(&store));
    assert!(wrap_selection_setting::is_key("md.wrapSelection"));
    assert!(!wrap_selection_setting::is_key(settings_keys::CONTINUE_LISTS));
    // Not one of SmartTyping's pair: the typing hooks never see it.
    assert!(!TypingSettings::is_key(settings_keys::WRAP_SELECTION));
}

// ── When the rows are live ─────────────────────────────────────────────────────────────────

fn editing() -> ShellSnapshot {
    ShellSnapshot { has_document: true, editor_visible: true, ..ShellSnapshot::empty() }
}

#[test]
fn format_rows_need_an_editor_with_a_document_on_screen() {
    for &id in commands() {
        assert!(command_enablement::is_enabled(id, &editing()), "{id:?}");
        // Preview, Zen reading.
        assert!(!command_enablement::is_enabled(id, &ShellSnapshot { editor_visible: false, ..editing() }), "{id:?}");
        assert!(
            !command_enablement::is_enabled(id, &ShellSnapshot { editor_visible: true, ..ShellSnapshot::empty() }),
            "{id:?}"
        );
        // The Book window: only while an article is open in it.
        let book =
            |editing_article| ShellSnapshot { is_book_window: true, is_editing_article: editing_article, ..editing() };
        assert!(command_enablement::is_enabled(id, &book(true)), "{id:?}");
        assert!(!command_enablement::is_enabled(id, &book(false)), "{id:?}");
    }
}

#[test]
fn the_wrap_toggle_is_always_live_and_ticked_from_the_snapshot() {
    assert!(command_enablement::is_enabled(CommandId::WrapSelection, &ShellSnapshot::empty()));
    assert!(command_enablement::is_checked(CommandId::WrapSelection, &ShellSnapshot::empty()));
    assert!(!command_enablement::is_checked(
        CommandId::WrapSelection,
        &ShellSnapshot { wrap_selection: false, ..ShellSnapshot::empty() }
    ));
}
