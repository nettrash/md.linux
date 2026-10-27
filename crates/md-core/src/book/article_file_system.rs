//! The file operations the in-place article editor performs (md.win `Book/ArticleFileSystem.cs`).

use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// (modification time, size) of a file — the session's change detector. Size is in the stamp
/// because a FAT time stamp is 2 s coarse and a rewrite inside that window would otherwise pass as
/// unchanged; ext4, btrfs and APFS make the date alone enough, but the pair costs nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub modified: SystemTime,
    pub size: u64,
}

/// The file operations [`super::book_article_session::BookArticleSession`] performs, behind a seam
/// so the state machine is testable against a fake and the app can route through whatever storage
/// API granted the book folder. Read and write fail with the error the session turns into its
/// `save_error_text`; [`ArticleFileSystem::stamp`] returns `None` for a file that is gone. `&self`
/// throughout so one file system can be shared behind an `Rc` (fakes keep their state in cells).
pub trait ArticleFileSystem {
    fn file_exists(&self, path: &str) -> bool;
    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>>;
    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()>;
    fn stamp(&self, path: &str) -> Option<FileStamp>;
}

/// [`ArticleFileSystem`] over `std::fs`. Writes are plain overwrites, like Swift's
/// `data.write(to:)`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LocalArticleFileSystem;

impl ArticleFileSystem for LocalArticleFileSystem {
    /// Swift's `fileExists(atPath:)`: true for a folder too, so a folder squatting on a rescue name
    /// is skipped, never written into. Follows links, as Foundation does.
    fn file_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(path).map_err(|e| with_path(e, path))
    }

    fn write_all_bytes(&self, path: &str, data: &[u8]) -> io::Result<()> {
        fs::write(path, data).map_err(|e| with_path(e, path))
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        local_stamp(path)
    }
}

/// A fresh `stat` every time: never a cached attribute set, which is what Swift's
/// `attributesOfItem` (as opposed to `URL.resourceValues`) buys it. A folder has no stamp, as .NET's
/// `FileInfo.Exists` is false for one.
pub(crate) fn local_stamp(path: &str) -> Option<FileStamp> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.is_dir() {
        return None;
    }
    Some(FileStamp { modified: metadata.modified().unwrap_or(UNIX_EPOCH), size: metadata.len() })
}

/// The OS error with the path it is about: Rust's `io::Error` names only the failure ("Permission
/// denied (os error 13)") where .NET's message, which md.win shows in its alerts, names the file
/// too.
pub(crate) fn with_path(error: io::Error, path: &str) -> io::Error {
    io::Error::new(error.kind(), format!("{path}: {error}"))
}
