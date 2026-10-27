//! Port of md.win `Md.App.Logic.View`: everything a document window remembers about how it shows
//! its document, and the rules that change it (shell-design §3.4, §5).
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`document_window_state`] | `DocumentWindowState.cs` (`IdentityMemo`, `DocumentWindowState`) |
//! | [`view_mode_controller`] | `ViewModeController.cs` |
//! | [`zen_controller`] | `ZenController.cs` |
//! | [`scroll_sync`] | `ScrollSync.cs` |
//! | [`scroll_sync_guard`] | `ScrollSyncGuard.cs` |
//! | [`split_layout`] | `SplitLayout.cs` (`PaneLayout`, `SplitLayout`) |
//! | [`derived_text`] | `DerivedText.cs` |
//! | [`derived_text_scheduler`] | `DerivedTextScheduler.cs` |
//! | [`settings_view_mode_store`] | `SettingsViewModeStore.cs` |

pub mod derived_text;
pub mod derived_text_scheduler;
pub mod document_window_state;
pub mod scroll_sync;
pub mod scroll_sync_guard;
pub mod settings_view_mode_store;
pub mod split_layout;
pub mod view_mode_controller;
pub mod zen_controller;

pub use derived_text::DerivedText;
pub use derived_text_scheduler::{DerivedTextScheduler, RunOffThread};
pub use document_window_state::{DocumentWindowState, IdentityMemo};
pub use scroll_sync::ScrollSync;
pub use scroll_sync_guard::{ApplyingScope, ScrollSyncGuard};
pub use settings_view_mode_store::SettingsViewModeStore;
pub use split_layout::{PaneLayout, SplitLayout};
pub use view_mode_controller::ViewModeController;
pub use zen_controller::ZenController;
