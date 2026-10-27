//! Port of md.win `Documents/SystemIoFileSystem.cs` — the app's [`FileSystem`] seam over `std::fs`
//! (§6.2).
//!
//! Every error carries the path it is about (`"{path}: {OS message}"`, md-core's shape): Rust's
//! `io::Error` names only the failure, where .NET's message — which md.win shows in its save-error
//! bar and alerts — names the file too.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::time::UNIX_EPOCH;

use super::file_names;
use crate::seams::{FileStamp, FileSystem};

/// [`FileSystem`] with Linux semantics. Stateless; share one behind an `Rc`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalFileSystem;

impl LocalFileSystem {
    /// The shared instance (md.win's `SystemIoFileSystem.Instance`).
    pub const INSTANCE: LocalFileSystem = LocalFileSystem;

    /// Creates `directory` and every missing folder above it. Not part of the seam: the app calls
    /// it once for the XDG folder `session.json` lives in, which — unlike md.win's always-present
    /// `LocalFolder` — does not exist on a first run.
    pub fn ensure_directory(&self, directory: &str) -> io::Result<()> {
        fs::create_dir_all(directory).map_err(|e| with_path(e, directory))
    }
}

impl FileSystem for LocalFileSystem {
    /// .NET's `File.Exists`: anything that is not a folder, links followed.
    fn file_exists(&self, path: &str) -> bool {
        fs::metadata(path).is_ok_and(|m| !m.is_dir())
    }

    fn directory_exists(&self, path: &str) -> bool {
        fs::metadata(path).is_ok_and(|m| m.is_dir())
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(path).map_err(|e| with_path(e, path))
    }

    /// `open(O_WRONLY | O_CREAT | O_TRUNC)` over the existing file, never write-then-rename: the
    /// file keeps its inode, permissions, owner, hard links and extended attributes, and a
    /// `GFileMonitor` sees one change that the session's stamp guard recognises as ours. No
    /// `fsync`: an autosave every second must not stall on a slow or synced disk, and the 1 s
    /// window is what bounds the loss either way (§6.3).
    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let mut file =
            OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|e| with_path(e, path))?;
        file.write_all(bytes).map_err(|e| with_path(e, path))
    }

    /// A fresh `stat` every time — never a cached attribute set, which is the whole point of the
    /// guard. A folder has no stamp (.NET's `FileInfo.Exists` is false for one).
    fn stamp(&self, path: &str) -> Option<FileStamp> {
        let metadata = fs::metadata(path).ok()?;
        if metadata.is_dir() {
            return None;
        }
        Some(FileStamp { modified: metadata.modified().unwrap_or(UNIX_EPOCH), size: metadata.len() })
    }

    /// A file or a folder, never onto anything that exists.
    ///
    /// LINUX DECISION: `rename(2)` silently replaces an existing file (and an empty folder), where
    /// md.win's `File.Move(…, overwrite: false)` / `Directory.Move` refuse. The destination is
    /// therefore checked first and an existing one — a dangling link included — is
    /// `AlreadyExists`. (A race with another process creating the name in between remains: std
    /// has no `RENAME_NOREPLACE`, and md-logic has no `unsafe` to call `renameat2`.) A file moved
    /// to another file system (`EXDEV`) is copied and the original removed, as .NET's `File.Move`
    /// does; a folder is not, as `Directory.Move` does not.
    fn move_file(&self, from: &str, to: &str) -> io::Result<()> {
        if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{to}: File exists (os error 17)")));
        }
        let source = fs::symlink_metadata(from).map_err(|e| with_path(e, from))?;
        match fs::rename(from, to) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices && source.is_file() => {
                fs::copy(from, to).map_err(|e| with_path(e, to))?;
                fs::remove_file(from).map_err(|e| with_path(e, from))
            }
            Err(e) => Err(with_path(e, from)),
        }
    }

    /// A folder with everything in it, or a file; a link is removed, never followed. Deleting
    /// nothing is not an error (.NET's `File.Delete`).
    fn delete(&self, path: &str) -> io::Result<()> {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(with_path(e, path)),
            Ok(m) if m.is_dir() => fs::remove_dir_all(path).map_err(|e| with_path(e, path)),
            Ok(_) => fs::remove_file(path).map_err(|e| with_path(e, path)),
        }
    }

    /// Every entry's full path (the folder as given, joined with the name), sorted ordinally so a
    /// listing is deterministic. A name that is not UTF-8 is skipped: md-logic's paths are
    /// strings, and md-core's book listing skips the same names.
    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(directory).map_err(|e| with_path(e, directory))? {
            let entry = entry.map_err(|e| with_path(e, directory))?;
            if let Ok(name) = entry.file_name().into_string() {
                entries.push(file_names::combine(directory, &name));
            }
        }
        entries.sort();
        Ok(entries)
    }
}

/// The OS error with the path it is about.
fn with_path(error: io::Error, path: &str) -> io::Error {
    io::Error::new(error.kind(), format!("{path}: {error}"))
}
