//! Pure: md-logic `Chord` → GTK accelerator strings (`<Control>1`, `<Shift><Control>s`, `F11`,
//! `<Alt>Up` …) for every row of shell-design §2.9, the table unit-tested; the
//! `gtk::accelerator_parse` round trip is a self-test check (`shell.accels_parse`).
//!
//! The spelling is md-logic's ([`Chord::accelerator`], `gtk_accelerator_name`'s canonical form) and
//! so is the list a row registers ([`CommandSpec::accelerators`]: the shown chord, then the
//! secondary one). This module pins that list against the design's table, and spells chords for
//! `xdotool` so the self-test can press them.

#[cfg(test)]
use md_logic::commands::Chord;
use md_logic::commands::CommandSpec;

/// The accelerator string GTK parses for `chord`.
#[cfg(test)]
pub fn gtk_accelerator(chord: &Chord) -> String {
    chord.accelerator()
}

/// Every accelerator GTK registers for `spec`, the displayed one first; empty for a row without a
/// chord and for Escape (a window key controller on Linux, §2.2).
pub fn accelerators(spec: &CommandSpec) -> Vec<String> {
    spec.accelerators()
}

/// xdotool's spelling of an accelerator (`<Shift><Control>s` → `shift+ctrl+s`), for the self-test's
/// keystrokes.
#[cfg(test)]
pub fn xdotool_chord(accelerator: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = accelerator;
    while let Some(stripped) = rest.strip_prefix('<') {
        let Some(end) = stripped.find('>') else { break };
        parts.push(
            match &stripped[..end] {
                "Shift" => "shift",
                "Control" | "Primary" => "ctrl",
                "Alt" => "alt",
                "Super" => "super",
                other => other,
            }
            .to_string(),
        );
        rest = &stripped[end + 1..];
    }
    parts.push(rest.to_string());
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use md_logic::commands::{CommandId, CommandKind, command_table};

    use super::*;

    fn registered(id: CommandId) -> Vec<String> {
        accelerators(command_table::spec(id))
    }

    #[test]
    fn the_linux_table_spells_every_chord_the_gtk_way() {
        let expected: [(CommandId, &[&str]); 43] = [
            (CommandId::New, &["<Control>n"]),
            (CommandId::Open, &["<Control>o"]),
            (CommandId::Close, &["<Control>w"]),
            (CommandId::Save, &["<Control>s"]),
            (CommandId::SaveAs, &["<Shift><Control>s"]),
            (CommandId::Print, &["<Control>p"]),
            (CommandId::Quit, &["<Control>q"]),
            (CommandId::Undo, &["<Control>z"]),
            (CommandId::Redo, &["<Shift><Control>z", "<Control>y"]),
            (CommandId::Cut, &["<Control>x"]),
            (CommandId::Copy, &["<Control>c"]),
            (CommandId::Paste, &["<Control>v"]),
            (CommandId::SelectAll, &["<Control>a"]),
            (CommandId::Find, &["<Control>f"]),
            (CommandId::Replace, &["<Control>h"]),
            (CommandId::FindNext, &["<Control>g"]),
            (CommandId::FindPrevious, &["<Shift><Control>g"]),
            (CommandId::UseSelectionForFind, &["<Control>e"]),
            (CommandId::Bold, &["<Control>b"]),
            (CommandId::Italic, &["<Control>i"]),
            (CommandId::InlineCode, &["<Shift><Control>c"]),
            (CommandId::Strikethrough, &["<Shift><Control>x"]),
            (CommandId::Link, &["<Control>k"]),
            (CommandId::Heading1, &["<Control><Alt>1"]),
            (CommandId::Heading2, &["<Control><Alt>2"]),
            (CommandId::Heading3, &["<Control><Alt>3"]),
            (CommandId::Heading4, &["<Control><Alt>4"]),
            (CommandId::Heading5, &["<Control><Alt>5"]),
            (CommandId::Heading6, &["<Control><Alt>6"]),
            (CommandId::BulletList, &["<Shift><Control>l"]),
            (CommandId::TaskList, &["<Shift><Control>t"]),
            (CommandId::Quote, &["<Shift><Control>q"]),
            (CommandId::Indent, &["<Control>bracketright"]),
            (CommandId::Outdent, &["<Control>bracketleft"]),
            (CommandId::ViewEdit, &["<Control>1"]),
            (CommandId::ViewSplit, &["<Control>2"]),
            (CommandId::ViewPreview, &["<Control>3"]),
            (CommandId::ZenMode, &["<Shift><Control>Return"]),
            (CommandId::FullScreen, &["F11"]),
            (CommandId::ShowBook, &["<Shift><Control>b"]),
            (CommandId::PreviousArticle, &["<Alt>Up", "<Control><Alt>Up"]),
            (CommandId::NextArticle, &["<Alt>Down", "<Control><Alt>Down"]),
            (CommandId::Help, &["F1"]),
        ];
        for (id, accels) in expected {
            assert_eq!(registered(id), accels.iter().map(|a| a.to_string()).collect::<Vec<_>>(), "{id:?}");
        }
    }

    #[test]
    fn escape_and_delete_are_never_accelerators() {
        let escape = command_table::spec(CommandId::Escape);
        assert_eq!(escape.kind, CommandKind::WindowKey);
        assert!(accelerators(escape).is_empty());
        assert!(registered(CommandId::Delete).is_empty(), "the Delete key stays with every entry");
    }

    #[test]
    fn rows_without_a_chord_register_nothing() {
        for id in [CommandId::Duplicate, CommandId::Rename, CommandId::Minimize, CommandId::About] {
            assert!(registered(id).is_empty(), "{id:?}");
        }
    }

    #[test]
    fn no_two_commands_share_an_accelerator() {
        let mut seen: HashMap<String, CommandId> = HashMap::new();
        for spec in command_table::all() {
            for accel in accelerators(spec) {
                if let Some(other) = seen.insert(accel.clone(), spec.id) {
                    panic!("{accel} is both {other:?} and {:?}", spec.id);
                }
            }
        }
        assert!(seen.len() >= 46);
    }

    #[test]
    fn the_shown_chord_is_the_spec_chord() {
        for spec in command_table::all() {
            if let (Some(chord), Some(first)) = (spec.chord, accelerators(spec).first()) {
                assert_eq!(&gtk_accelerator(&chord), first, "{:?}", spec.id);
            }
        }
    }

    #[test]
    fn xdotool_spells_the_same_chords() {
        assert_eq!(xdotool_chord("<Shift><Control>s"), "shift+ctrl+s");
        assert_eq!(xdotool_chord("<Control><Alt>2"), "ctrl+alt+2");
        assert_eq!(xdotool_chord("<Control>bracketright"), "ctrl+bracketright");
        assert_eq!(xdotool_chord("F11"), "F11");
        assert_eq!(xdotool_chord("<Alt>Up"), "alt+Up");
    }
}
