//! Port of md.win `View/SettingsViewModeStore.cs`.

use std::rc::Rc;

use md_core::document::view_mode_memory::{SETTINGS_KEY, ViewModeStore};

use crate::settings::SettingsStore;

/// md-core's [`ViewModeStore`] over the app-wide [`SettingsStore`]: the one `md.viewModeMemory`
/// string (shell-design §5.2, §9). Nothing else in the app reads or writes that key —
/// `view_mode_memory` owns the codec and this owns the location.
pub struct SettingsViewModeStore {
    settings: Rc<dyn SettingsStore>,
}

impl SettingsViewModeStore {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Self {
        SettingsViewModeStore { settings }
    }
}

impl std::fmt::Debug for SettingsViewModeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SettingsViewModeStore")
    }
}

impl ViewModeStore for SettingsViewModeStore {
    /// `None` when absent; the codec reads that as "nothing remembered".
    fn load(&self) -> Option<String> {
        self.settings.get_string(SETTINGS_KEY)
    }

    fn save(&self, value: &str) {
        self.settings.set_string(SETTINGS_KEY, value);
    }
}
