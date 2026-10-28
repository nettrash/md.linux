//! Port of md.win `Md.App.Logic.Windows`: the session file, window sizes and placement, the
//! registry of open windows and their titles.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`session_store`] | `SessionStore.cs` (`SessionWindow`, `SessionBook`, `SessionState`) |
//! | [`window_placement`] | `WindowPlacement.cs` (`WindowSize`, `WindowRect`) |
//! | [`window_registry`] | `WindowRegistry.cs` |
//! | [`window_title`] | `WindowTitle.cs` (`WindowTitle`, `UntitledNames`) |
//!
//! `json` (private) is the std-only JSON `session.json` needs; md.win has `System.Text.Json`.

mod json;
pub mod session_store;
pub mod window_placement;
pub mod window_registry;
pub mod window_title;

pub use session_store::{SessionBook, SessionState, SessionWindow};
pub use window_placement::{WindowRect, WindowSize};
pub use window_registry::WindowRegistry;
pub use window_title::UntitledNames;
