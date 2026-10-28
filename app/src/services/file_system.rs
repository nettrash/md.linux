//! `StdFileSystem` and `CanonicalFileIdentity` — md-logic's `FileSystem` and `FileIdentity` seams
//! over `std::fs` (shell-design §6.2): whole-file reads, writes **in place** (truncate + write, never
//! write-then-rename, so the inode, mode, owner, hard links and xattrs survive), `canonicalize`
//! with md-core's lexical fallback for a path that does not exist yet.
//!
//! The semantics are md-logic's own `LocalFileSystem` and `LocalFileIdentity` (the port of md.win's
//! `SystemIoFileSystem` and `FileIdentity`, with their Linux decisions and tests); these are the
//! shell's named handles on them, so the `Shell` and the seams it hands out stay the frozen types
//! (docs/shell-work-packages.md §3.2) while the behaviour has one home.

use std::io;

use md_logic::documents::{LocalFileIdentity, LocalFileSystem};
use md_logic::seams::{FileIdentity, FileStamp, FileSystem};

/// [`FileSystem`] over `std::fs` — md-logic's [`LocalFileSystem`]: in-place writes, a fresh `stat`
/// per stamp, a move that never replaces an existing target (and copies across file systems), and
/// errors that name their path.
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFileSystem {
    inner: LocalFileSystem,
}

impl StdFileSystem {
    pub fn new() -> StdFileSystem {
        StdFileSystem { inner: LocalFileSystem::INSTANCE }
    }
}

impl FileSystem for StdFileSystem {
    fn file_exists(&self, path: &str) -> bool {
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.inner.directory_exists(path)
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        self.inner.read_all_bytes(path)
    }

    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.inner.write_all_bytes_in_place(path, bytes)
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.inner.stamp(path)
    }

    fn move_file(&self, from: &str, to: &str) -> io::Result<()> {
        self.inner.move_file(from, to)
    }

    fn delete(&self, path: &str) -> io::Result<()> {
        self.inner.delete(path)
    }

    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>> {
        self.inner.enumerate_entries(directory)
    }
}

/// [`FileIdentity`]: `realpath` when the path names something, else md-core's lexical
/// normalisation; never case-folded (md-logic's [`LocalFileIdentity`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct CanonicalFileIdentity {
    inner: LocalFileIdentity,
}

impl CanonicalFileIdentity {
    pub fn new() -> CanonicalFileIdentity {
        CanonicalFileIdentity { inner: LocalFileIdentity::INSTANCE }
    }
}

impl FileIdentity for CanonicalFileIdentity {
    fn canonical(&self, path: &str) -> String {
        self.inner.canonical(path)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    /// A fresh folder under the system temp directory, removed when dropped.
    pub(crate) struct TempFolder(pub PathBuf);

    impl TempFolder {
        pub(crate) fn new(name: &str) -> TempFolder {
            let dir = std::env::temp_dir().join(format!("md-p2-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temp folder");
            TempFolder(dir)
        }

        pub(crate) fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into_owned()
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_in_place_write_keeps_the_inode_and_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempFolder::new("in-place");
        let path = temp.path("doc.md");
        std::fs::write(&path, "first version, longer than the second\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let link = temp.path("hard-link.md");
        std::fs::hard_link(&path, &link).unwrap();
        let before = std::fs::metadata(&path).unwrap();

        let files = StdFileSystem::new();
        files.write_all_bytes_in_place(&path, b"second\n").unwrap();

        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(after.ino(), before.ino());
        assert_eq!(after.mode() & 0o777, 0o640);
        assert_eq!(std::fs::read(&path).unwrap(), b"second\n");
        assert_eq!(std::fs::read(&link).unwrap(), b"second\n", "the hard link sees the write");
        assert_eq!(files.stamp(&path).map(|s| s.size), Some(7));
    }

    #[test]
    fn a_new_file_is_created_and_a_folder_has_no_stamp() {
        let temp = TempFolder::new("create");
        let files = StdFileSystem::new();
        let path = temp.path("new.md");
        assert!(!files.file_exists(&path));
        files.write_all_bytes_in_place(&path, b"x").unwrap();
        assert!(files.file_exists(&path));
        assert!(files.directory_exists(&temp.0.to_string_lossy()));
        assert_eq!(files.stamp(&temp.0.to_string_lossy()), None);
        assert_eq!(files.stamp(&temp.path("missing.md")), None);
    }

    #[test]
    fn a_move_never_replaces_and_errors_name_the_path() {
        let temp = TempFolder::new("move");
        let files = StdFileSystem::new();
        let (a, b) = (temp.path("a.md"), temp.path("b.md"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let err = files.move_file(&a, &b).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(err.to_string().contains(&b), "{err}");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        let c = temp.path("c.md");
        files.move_file(&a, &c).unwrap();
        assert!(!files.file_exists(&a));
        assert_eq!(std::fs::read(&c).unwrap(), b"a");
        let missing = temp.path("missing.md");
        assert!(files.read_all_bytes(&missing).unwrap_err().to_string().contains(&missing));
    }

    #[test]
    fn identity_resolves_links_and_keeps_case() {
        let temp = TempFolder::new("identity");
        let real = temp.path("Real.md");
        std::fs::write(&real, "x").unwrap();
        let link = temp.path("link.md");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let identity = CanonicalFileIdentity::new();
        let canonical = identity.canonical(&real);
        assert_eq!(identity.canonical(&link), canonical);
        assert!(canonical.ends_with("/Real.md"), "{canonical}");
        // Not there yet (a Save As target): the lexical half only.
        let dotted = format!("{}/./sub/../New.md", temp.0.to_string_lossy());
        assert!(identity.canonical(&dotted).ends_with("/New.md"));
        assert_eq!(identity.canonical(""), "");
    }
}
