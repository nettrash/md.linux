//! The web layer (shell-design §4): one `WebContext` and ephemeral `NetworkSession` per process,
//! the `md-asset:` origin, the live preview host.
//!
//! | Module | What |
//! | --- | --- |
//! | [`context`] | `WebServices`: context, ephemeral session, settings factory, sandbox preflight |
//! | [`asset_host`] | the `md-asset:` handler + CSP (from the spike's `assets.rs`) |
//! | [`preview_host`] | `PreviewHost` + its `PreviewSurface`, policy, menus, messages |
//! | [`retry_policy`] | pure: the Mac's `PreviewRetryPolicy` (+ its tests) |
//! | [`js`] | eval → JSON, the two-frame wait, the snapshot scroll scripts |
//!
//! Owner: P4.

pub mod asset_host;
pub mod context;
pub mod js;
pub mod preview_host;
pub mod retry_policy;
