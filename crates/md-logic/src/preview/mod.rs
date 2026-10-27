//! Port of md.win `Md.App.Logic.Preview`: the live preview's policy and every string the web view
//! is handed — the `md-asset:` origin, the MIME table, the link policy, the injected and evaluated
//! scripts, the JSON their answers come back as, the Linux typography graft, and the Mac's
//! re-render coordinator.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`asset_mime`] | `AssetMime.cs` |
//! | [`asset_origin`] | `AssetOrigin.cs` (+ the spike's `split_uri` / `is_contained`) |
//! | [`editor_jump`] | `EditorJump.cs` (`EditorJump`, `EditorJumpTracker`) |
//! | [`json_script`] | `JsonScript.cs` (without `KeyMessage`) |
//! | [`link_policy`] | `LinkPolicy.cs` |
//! | [`preview_coordinator`] | `PreviewCoordinator.cs` (`IDocumentHtml` → [`DocumentHtml`]) |
//! | [`preview_navigation`] | `PreviewNavigation.cs` (+ [`JumpId`], md.win's `Guid`) |
//! | [`screen_html`] | `ScreenHtml.cs` |
//! | [`scripts`] | `Scripts.cs` (without `KeyForward`) |

pub mod asset_mime;
pub mod asset_origin;
pub mod editor_jump;
pub mod json_script;
pub mod link_policy;
pub mod preview_coordinator;
pub mod preview_navigation;
pub mod screen_html;
pub mod scripts;

pub use editor_jump::{EditorJump, EditorJumpTracker};
pub use json_script::ScrollMessage;
pub use link_policy::LinkDecision;
pub use preview_coordinator::{CoreDocumentHtml, DocumentHtml, NavigationHandled, PreviewCoordinator};
pub use preview_navigation::{JumpId, PreviewNavigation};
