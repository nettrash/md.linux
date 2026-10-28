//! Port of md.win `FakeSettingsStore`.

use std::cell::RefCell;
use std::fmt;

use crate::event::Event;
use crate::settings::{InMemorySettingsStore, SettingValue, SettingsStore};

/// [`SettingsStore`] for tests: [`InMemorySettingsStore`]'s semantics plus a log of every write and
/// removal (changing or not), so a test can assert "Zen never touched md.viewModeMemory".
/// [`SettingsStore::changed`] is the inner store's event: it fires only on real changes.
#[derive(Default)]
pub struct FakeSettingsStore {
    inner: InMemorySettingsStore,
    writes: RefCell<Vec<(String, SettingValue)>>,
    removals: RefCell<Vec<String>>,
}

impl fmt::Debug for FakeSettingsStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeSettingsStore")
            .field("keys", &self.inner.keys())
            .field("writes", &self.writes.borrow())
            .field("removals", &self.removals.borrow())
            .finish()
    }
}

impl FakeSettingsStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every `set_*` call, in order.
    pub fn writes(&self) -> Vec<(String, SettingValue)> {
        self.writes.borrow().clone()
    }

    /// Every `remove` call, in order.
    pub fn removals(&self) -> Vec<String> {
        self.removals.borrow().clone()
    }

    /// The keys currently stored, in the order first stored.
    pub fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    /// Whether any write went to `key`.
    pub fn was_written(&self, key: &str) -> bool {
        self.writes.borrow().iter().any(|(k, _)| k == key)
    }
}

impl SettingsStore for FakeSettingsStore {
    fn get_string(&self, key: &str) -> Option<String> {
        self.inner.get_string(key)
    }

    fn set_string(&self, key: &str, value: &str) {
        self.writes.borrow_mut().push((key.to_string(), SettingValue::String(value.to_string())));
        self.inner.set_string(key, value);
    }

    fn get_bool(&self, key: &str, fallback: bool) -> bool {
        self.inner.get_bool(key, fallback)
    }

    fn set_bool(&self, key: &str, value: bool) {
        self.writes.borrow_mut().push((key.to_string(), SettingValue::Bool(value)));
        self.inner.set_bool(key, value);
    }

    fn remove(&self, key: &str) {
        self.removals.borrow_mut().push(key.to_string());
        self.inner.remove(key);
    }

    fn changed(&self) -> &Event<str> {
        self.inner.changed()
    }
}
