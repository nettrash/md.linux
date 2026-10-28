//! `md.wrapSelection` (docs/formatting.md §4) — port of md.win `WrapSelectionSetting`: typing one
//! of the four marks (`*`, `_`, the backtick, `$`) over a selection wraps it. On by default;
//! Edit ▸ Typing ▸ Wrap Selection When Typing flips it, live in every window through
//! [`SettingsStore::changed`], like the two smart-typing switches beside it. Kept apart from
//! [`TypingSettings`](super::TypingSettings), which is smart typing's own pair.

use super::settings_keys;
use super::settings_store::SettingsStore;

/// The current value; an absent key (or one of the wrong type) reads as on.
pub fn read(settings: &(impl SettingsStore + ?Sized)) -> bool {
    settings.get_bool(settings_keys::WRAP_SELECTION, settings_keys::WRAP_SELECTION_DEFAULT)
}

/// Whether a [`SettingsStore::changed`] key is this one.
pub fn is_key(key: &str) -> bool {
    key == settings_keys::WRAP_SELECTION
}

/// Flip the stored value; absent counts as on, so the first click turns it off.
pub fn toggle(settings: &(impl SettingsStore + ?Sized)) {
    settings.set_bool(settings_keys::WRAP_SELECTION, !read(settings));
}
