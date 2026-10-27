//! Port of md.win `TypingSettings`.

use super::settings_keys;
use super::settings_store::SettingsStore;

/// The two smart-typing preferences (docs/smart-typing.md §3.1) as the editor and the menus read
/// them: `md.continueLists` and `md.capitalizeSentences`, both bool, both default true, both the
/// same key on every md port. A function whose setting is off is never called (the editor checks
/// the bool before it asks `smart_typing`), and a toggle takes effect in every open editor at once
/// through [`SettingsStore::changed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypingSettings {
    pub continue_lists: bool,
    pub capitalize_sentences: bool,
}

impl TypingSettings {
    /// Both on — what a fresh install and a store with neither key reads as.
    pub const DEFAULTS: TypingSettings =
        TypingSettings::new(settings_keys::CONTINUE_LISTS_DEFAULT, settings_keys::CAPITALIZE_SENTENCES_DEFAULT);

    /// md.win's positional record constructor.
    pub const fn new(continue_lists: bool, capitalize_sentences: bool) -> TypingSettings {
        TypingSettings { continue_lists, capitalize_sentences }
    }

    /// The current values; an absent key (or one of the wrong type) reads as its default.
    pub fn read(settings: &(impl SettingsStore + ?Sized)) -> TypingSettings {
        TypingSettings::new(
            settings.get_bool(settings_keys::CONTINUE_LISTS, settings_keys::CONTINUE_LISTS_DEFAULT),
            settings.get_bool(settings_keys::CAPITALIZE_SENTENCES, settings_keys::CAPITALIZE_SENTENCES_DEFAULT),
        )
    }

    /// Whether a [`SettingsStore::changed`] key is one of the two — what the editor and the menus
    /// re-read on.
    pub fn is_key(key: &str) -> bool {
        key == settings_keys::CONTINUE_LISTS || key == settings_keys::CAPITALIZE_SENTENCES
    }

    /// The Edit menu's two toggles: flip the stored value (absent counts as the default, so the
    /// first click on a fresh install turns the feature *off*).
    ///
    /// # Panics
    ///
    /// For anything but the two keys (md.win throws `ArgumentOutOfRangeException`) — a menu row
    /// wired to the wrong key is a programming error, not a silent write.
    pub fn toggle(settings: &(impl SettingsStore + ?Sized), key: &str) {
        assert!(TypingSettings::is_key(key), "Not a typing setting: {key}");
        let fallback = if key == settings_keys::CONTINUE_LISTS {
            settings_keys::CONTINUE_LISTS_DEFAULT
        } else {
            settings_keys::CAPITALIZE_SENTENCES_DEFAULT
        };
        settings.set_bool(key, !settings.get_bool(key, fallback));
    }
}

impl Default for TypingSettings {
    fn default() -> Self {
        TypingSettings::DEFAULTS
    }
}
