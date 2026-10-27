//! `StdFileSystem` and `CanonicalFileIdentity` — md-logic's `FileSystem` and `FileIdentity` seams
//! over `std::fs` (shell-design §6.2): whole-file reads, writes **in place** (truncate + write, never
//! write-then-rename, so the inode, mode, owner, hard links and xattrs survive), `canonicalize`
//! with md-core's lexical fallback for a path that does not exist yet.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::io;

use md_logic::seams::{FileIdentity, FileStamp, FileSystem};

/// [`FileSystem`] over `std::fs`.
pub struct StdFileSystem {}

impl StdFileSystem {
    pub fn new() -> StdFileSystem {
        StdFileSystem {}
    }
}

/// The S0 stub's answer to every operation.
fn unimplemented() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "S0 stub: StdFileSystem is work package P2")
}

impl FileSystem for StdFileSystem {
    fn file_exists(&self, path: &str) -> bool {
        false
    }

    fn directory_exists(&self, path: &str) -> bool {
        false
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        Err(unimplemented())
    }

    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        Err(unimplemented())
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        None
    }

    fn move_file(&self, from: &str, to: &str) -> io::Result<()> {
        Err(unimplemented())
    }

    fn delete(&self, path: &str) -> io::Result<()> {
        Err(unimplemented())
    }

    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>> {
        Err(unimplemented())
    }
}

/// [`FileIdentity`]: `std::fs::canonicalize`, else md-core's lexical normalisation.
pub struct CanonicalFileIdentity {}

impl CanonicalFileIdentity {
    pub fn new() -> CanonicalFileIdentity {
        CanonicalFileIdentity {}
    }
}

impl FileIdentity for CanonicalFileIdentity {
    fn canonical(&self, path: &str) -> String {
        path.to_string()
    }
}
