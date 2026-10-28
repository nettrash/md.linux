//! The app-wide key/value store (§9) — port of md.win `ISettingsStore`.

use std::fmt;

use crate::event::Event;

/// The app-wide key/value store (§9): the app's persistent store in the GTK shell, an
/// [`InMemorySettingsStore`](super::InMemorySettingsStore) in md-logic and the tests. Keys are the
/// constants in [`settings_keys`](super::settings_keys); values are strings and bools only, and
/// per-window state never goes here (it lives in `session.json`). [`SettingsStore::changed`] is
/// raised with the key when a write actually changes the stored value, so a menu that re-writes
/// the value it just observed cannot loop.
///
/// `&self` throughout, like every seam: one store is shared by every window behind an `Rc`, and
/// implementations keep their state in cells. An implementation must not hold a borrow of that
/// state while it raises [`SettingsStore::changed`] — a handler re-reads the store.
pub trait SettingsStore {
    /// `None` when absent or not a string.
    fn get_string(&self, key: &str) -> Option<String>;

    fn set_string(&self, key: &str, value: &str);

    /// The fallback when absent or not a bool.
    fn get_bool(&self, key: &str, fallback: bool) -> bool;

    fn set_bool(&self, key: &str, value: bool);

    fn remove(&self, key: &str);

    /// Args: the key whose stored value changed.
    fn changed(&self) -> &Event<str>;
}

/// One stored value — C#'s boxed `object` (a `string` or a `bool`). A string `"true"` and the
/// bool `true` are different values: reading one as the other is "absent".
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SettingValue {
    String(String),
    Bool(bool),
}

impl From<&str> for SettingValue {
    fn from(value: &str) -> Self {
        SettingValue::String(value.to_string())
    }
}

impl From<String> for SettingValue {
    fn from(value: String) -> Self {
        SettingValue::String(value)
    }
}

impl From<bool> for SettingValue {
    fn from(value: bool) -> Self {
        SettingValue::Bool(value)
    }
}

impl fmt::Display for SettingValue {
    /// The string itself, or `true` / `false`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SettingValue::String(s) => f.write_str(s),
            SettingValue::Bool(b) => write!(f, "{b}"),
        }
    }
}
