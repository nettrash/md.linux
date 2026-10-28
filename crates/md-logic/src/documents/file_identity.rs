//! Port of md.win `Documents/FileIdentity.cs` — the app's [`FileIdentity`] seam implementation.

use md_core::document::view_mode_memory;

use crate::seams::FileIdentity;

/// The truest spelling of a path, so two spellings of one file are one identity (§5.2): one entry
/// in `md.viewModeMemory`, one key in the document registry, one answer to "is this file already
/// open?".
///
/// md.win asks Windows itself (`GetFinalPathNameByHandle`), because that is the only call that
/// undoes case, junctions, mapped drives and 8.3 short names, and strips the `\\?\` prefix it
/// answers with.
///
/// LINUX DECISION: md-core's `view_mode_memory::canonical_path` is the whole answer — absolute
/// against the working directory, `.` / `..` and doubled separators collapsed, symlinks resolved
/// on every component (`realpath`) when the path names something, and never case-folded, because
/// ext4 compares names byte for byte and `A.md` and `a.md` are two files. There is no prefix to
/// strip. A path that names nothing yet (a Save As target) gets the lexical half only, exactly as
/// md.win's fallback does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalFileIdentity;

impl LocalFileIdentity {
    /// The shared instance (md.win's `FileIdentity.Instance`); the type has no state.
    pub const INSTANCE: LocalFileIdentity = LocalFileIdentity;

    /// md.win's non-Windows / missing-file leg, exposed so a test can pin that both legs agree:
    /// md-core's `canonical_path`, which folds no case on Linux.
    pub fn lexical(path: &str) -> String {
        view_mode_memory::canonical_path(path)
    }
}

impl FileIdentity for LocalFileIdentity {
    /// An empty path is returned unchanged rather than resolved against the working directory.
    fn canonical(&self, path: &str) -> String {
        if path.is_empty() {
            return String::new();
        }
        Self::lexical(path)
    }
}
