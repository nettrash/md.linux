//! Port of md.win `Md.App.Logic.Documents`: the document state machine and everything it reads
//! and writes with.
//!
//! | Module | md.win source |
//! | --- | --- |
//! | [`text_file_session`] | `TextFileSession.cs` (`Stage`, `SessionRole`, `TextFileSession`) |
//! | [`document_loader`] | `DocumentLoader.cs` |
//! | [`text_file_dressing`] | `TextFileDressing.cs` |
//! | [`line_endings`] | `LineEndings.cs` |
//! | [`line_offsets`] | `LineOffsets.cs` |
//! | [`editor_text`] | `EditorText.cs` |
//! | [`file_names`] | `FileNames.cs` |
//! | [`file_identity`] | `FileIdentity.cs` (`LocalFileIdentity`) |
//! | [`rescue_copy`] | `RescueCopy.cs` (with `FileFailure`) |
//! | [`text_search`] | `TextSearch.cs` |
//! | [`find_as_you_type`] | `FindAsYouType.cs` |
//! | [`asset_reader`] | `AssetReader.cs` |
//! | [`local_file_system`] | `SystemIoFileSystem.cs` (`LocalFileSystem`) |
//!
//! md.win's `FileSystemWatcherAdapter` has no counterpart here: the GTK app implements the
//! [`FileWatcher`](crate::seams::FileWatcher) seam over `GFileMonitor`.
//!
//! The units every editor offset in this module is in — selections, line starts, search hits —
//! are UTF-16 code units, as in md.win and md-core's editing rules; the GTK shell converts its
//! character offsets at the boundary with md-core's `text::utf16` helpers.

pub mod asset_reader;
pub mod document_loader;
pub mod editor_text;
pub mod file_identity;
pub mod file_names;
pub mod find_as_you_type;
pub mod line_endings;
pub mod line_offsets;
pub mod local_file_system;
pub mod rescue_copy;
pub mod text_file_dressing;
pub mod text_file_session;
pub mod text_search;

pub use document_loader::{DocumentKind, DocumentLoad, LoadFailure, LoadedDocument};
pub use file_identity::LocalFileIdentity;
pub use find_as_you_type::FindAsYouType;
pub use line_endings::NewLine;
pub use local_file_system::LocalFileSystem;
pub use text_file_dressing::TextFileDressing;
pub use text_file_session::{SessionRole, Stage, TextFileSession};
