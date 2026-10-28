//! Exports and print (shell-design §7): the invisible overlay renderer md-logic's
//! `ExportPipeline` drives, PDF through `webkit::PrintOperation` to "Print to File", the
//! visible-region snapshots, the cover fetch, and each window's export entry points.
//!
//! | Module | What |
//! | --- | --- |
//! | [`renderer`] | `OverlayRenderer` (`RenderSurface`), `OverlayRendererFactory` |
//! | [`pdf`] | `PrintOperation` to a file, the print dialog |
//! | [`snapshot`] | visible-region capture, crop, PNG |
//! | [`cover_fetch`] | the https fetch over libsoup for `CoverReader` |
//! | [`window_exports`] | `WindowExports`: pipeline, pickers, alerts, busy, `track_output` |
//!
//! Owner: P4.

pub mod cover_fetch;
pub mod pdf;
pub mod renderer;
pub mod snapshot;
pub mod window_exports;
