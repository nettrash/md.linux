//! Port of md.win `Commands/Chord.cs`: one keyboard chord, as GTK spells it.
//!
//! LINUX DECISION: md.win's chord is a Win32 virtual-key code and a Windows display string. Here
//! the key is a GDK keyval *name* (what `gdk_keyval_name` returns and `gtk_accelerator_parse`
//! reads: `"n"`, `"1"`, `"Return"`, `"bracketright"`) and the chord produces two strings:
//! [`Chord::accelerator`] — the canonical `gtk_accelerator_name` spelling the shell passes to
//! `gtk_application_set_accels_for_action` and to a menu item's `accel` attribute — and
//! [`Chord::display_text`] — the English label GTK itself draws beside the menu row
//! (`gtk_accelerator_get_label`), which is what the tooltips quote. Both were checked against
//! GTK 4.22.4 in the Ubuntu 26.04 image: every chord in the table parses, is
//! `gtk_accelerator_valid`, round-trips through `gtk_accelerator_name` unchanged, and labels as
//! [`Chord::display_text`] says. md-logic stays GTK-free: only the strings cross.

use std::fmt;
use std::ops::BitOr;

/// The modifier set of a [`Chord`]: GDK's `SHIFT_MASK`, `CONTROL_MASK` and `ALT_MASK` (Mod1). No
/// chord in md uses Super, Hyper or Meta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    pub const NONE: KeyModifiers = KeyModifiers(0);
    pub const CTRL: KeyModifiers = KeyModifiers(1);
    pub const SHIFT: KeyModifiers = KeyModifiers(2);
    pub const ALT: KeyModifiers = KeyModifiers(4);

    /// The union, usable in a `const` (the [`BitOr`] impl is not).
    pub const fn with(self, other: KeyModifiers) -> KeyModifiers {
        KeyModifiers(self.0 | other.0)
    }

    /// Every flag of `other` is set here — C#'s `HasFlag`.
    pub const fn contains(self, other: KeyModifiers) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for KeyModifiers {
    type Output = KeyModifiers;

    fn bitor(self, rhs: KeyModifiers) -> KeyModifiers {
        self.with(rhs)
    }
}

/// The GDK keyval names the command table uses — md.win's `VirtualKeys`, by name. Letters are
/// lower case (`gtk_accelerator_parse` lower-cases a letter keyval, so `<Shift><Control>s` is
/// Ctrl+Shift+S), digits are themselves.
pub mod keys {
    pub const BACKSPACE: &str = "BackSpace";
    pub const RETURN: &str = "Return";
    pub const ESCAPE: &str = "Escape";
    pub const DELETE: &str = "Delete";
    /// The keypad's Delete — the same deletion as [`DELETE`] to the text view.
    pub const KP_DELETE: &str = "KP_Delete";
    pub const UP: &str = "Up";
    pub const DOWN: &str = "Down";
    pub const F1: &str = "F1";
    pub const F11: &str = "F11";
    /// The `[` key on a US layout (Outdent).
    pub const BRACKET_LEFT: &str = "bracketleft";
    /// The `]` key on a US layout (Indent).
    pub const BRACKET_RIGHT: &str = "bracketright";
}

/// One keyboard chord: a key and its modifiers (§13.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    /// The GDK keyval name (see [`keys`]).
    pub key: &'static str,
    pub modifiers: KeyModifiers,
}

impl Chord {
    pub const fn new(key: &'static str, modifiers: KeyModifiers) -> Chord {
        Chord { key, modifiers }
    }

    /// A chord with no modifier (`Chord(int virtualKey)` in md.win).
    pub const fn key(key: &'static str) -> Chord {
        Chord { key, modifiers: KeyModifiers::NONE }
    }

    /// The string `gtk_accelerator_parse` reads, in `gtk_accelerator_name`'s canonical form —
    /// modifiers in the order Shift, Control, Alt, then the keyval name: `"<Control>n"`,
    /// `"<Shift><Control>s"`, `"<Control><Alt>1"`, `"<Control>bracketright"`, `"F11"`.
    pub fn accelerator(&self) -> String {
        let mut text = String::new();
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            text.push_str("<Shift>");
        }
        if self.modifiers.contains(KeyModifiers::CTRL) {
            text.push_str("<Control>");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            text.push_str("<Alt>");
        }
        text.push_str(self.key);
        text
    }

    /// The label GTK draws beside the menu row (`gtk_accelerator_get_label`, untranslated):
    /// modifiers in the order Shift, Ctrl, Alt, then the key — `"Shift+Ctrl+S"`, `"Alt+Up"`,
    /// `"Ctrl+]"`. Pure; the one place a chord is spelled for a human (the tooltips quote it).
    pub fn display_text(&self) -> String {
        let mut text = String::new();
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            text.push_str("Shift+");
        }
        if self.modifiers.contains(KeyModifiers::CTRL) {
            text.push_str("Ctrl+");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            text.push_str("Alt+");
        }
        text.push_str(&Chord::key_name(self.key));
        text
    }

    /// The key half of [`Chord::display_text`], as GTK labels it: a letter upper-cased, a digit
    /// or a function key as written, the brackets as their characters, and the named keys by
    /// their keyval names (`Return`, `Escape`, `Delete`, `Up`, `Down` — GTK's own labels; it
    /// spells `Page_Up` as `Page Up`). An unlisted name comes back as itself.
    pub fn key_name(key: &str) -> String {
        match key {
            keys::BRACKET_LEFT => "[".to_string(),
            keys::BRACKET_RIGHT => "]".to_string(),
            "apostrophe" => "'".to_string(),
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) if c.is_ascii_lowercase() => c.to_ascii_uppercase().to_string(),
                    _ => key.replace('_', " "),
                }
            }
        }
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_text())
    }
}
