//! Port of md.win `InMemorySettingsStore`.

use std::cell::RefCell;

use super::settings_store::{SettingValue, SettingsStore};
use crate::event::Event;

/// [`SettingsStore`] over a list of pairs — md-logic's store for tests and for any code path that
/// runs before the app's persistent store exists. The app's store must keep these semantics: a
/// string read of a bool (or the reverse) is "absent"; [`SettingsStore::changed`] fires only when
/// the stored value differs from what was there.
#[derive(Default)]
pub struct InMemorySettingsStore {
    /// In the order each key was first stored (a rewrite keeps its place; a removal drops it).
    values: RefCell<Vec<(String, SettingValue)>>,
    changed: Event<str>,
}

impl InMemorySettingsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The keys currently stored, in the order they were first stored — for tests and for
    /// PRIVACY's "exactly these" check.
    pub fn keys(&self) -> Vec<String> {
        self.values.borrow().iter().map(|(key, _)| key.clone()).collect()
    }

    /// The stored value under `key`, of either type.
    pub fn value(&self, key: &str) -> Option<SettingValue> {
        self.values.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn set(&self, key: &str, value: SettingValue) {
        {
            let mut values = self.values.borrow_mut();
            match values.iter_mut().find(|(k, _)| k == key) {
                Some((_, old)) if *old == value => return,
                Some((_, old)) => *old = value,
                None => values.push((key.to_string(), value)),
            }
        }
        // The borrow is released: a handler may read (or write) the store.
        self.changed.emit(key);
    }
}

impl SettingsStore for InMemorySettingsStore {
    fn get_string(&self, key: &str) -> Option<String> {
        match self.value(key) {
            Some(SettingValue::String(s)) => Some(s),
            _ => None,
        }
    }

    // md.win throws on a null value; a Rust `&str` cannot be one.
    fn set_string(&self, key: &str, value: &str) {
        self.set(key, SettingValue::String(value.to_string()));
    }

    fn get_bool(&self, key: &str, fallback: bool) -> bool {
        match self.value(key) {
            Some(SettingValue::Bool(b)) => b,
            _ => fallback,
        }
    }

    fn set_bool(&self, key: &str, value: bool) {
        self.set(key, SettingValue::Bool(value));
    }

    fn remove(&self, key: &str) {
        let removed = {
            let mut values = self.values.borrow_mut();
            let before = values.len();
            values.retain(|(k, _)| k != key);
            values.len() != before
        };
        if removed {
            self.changed.emit(key);
        }
    }

    fn changed(&self) -> &Event<str> {
        &self.changed
    }
}
