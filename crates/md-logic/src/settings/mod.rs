//! Port of md.win `Md.App.Logic.Settings`: the app-wide key/value store, its keys, the paper
//! palette and the three typing switches.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`settings_store`] | `ISettingsStore.cs` |
//! | [`in_memory_settings_store`] | `InMemorySettingsStore.cs` |
//! | [`settings_keys`] | `SettingsKeys.cs` |
//! | [`palette`] | `Palette.cs` |
//! | [`typing_settings`] | `TypingSettings.cs` |
//! | [`wrap_selection_setting`] | `WrapSelectionSetting.cs` |
//!
//! The types are re-exported here: `settings::SettingsStore`, `settings::Palette`, ….

pub mod in_memory_settings_store;
pub mod palette;
pub mod settings_keys;
pub mod settings_store;
pub mod typing_settings;
pub mod wrap_selection_setting;

pub use in_memory_settings_store::InMemorySettingsStore;
pub use palette::{Palette, PaletteTheme};
pub use settings_store::{SettingValue, SettingsStore};
pub use typing_settings::TypingSettings;
