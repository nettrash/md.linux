//! Untitled drafts in `$XDG_DATA_HOME/md/drafts/` (shell-design §1.6): `<uuid>.md` (`0600`, folder
//! `0700`) written 1 s after the last keystroke and on deactivation, deleted on Save / Don't Save,
//! orphans restored after the session's own; unit-tested in a temp folder.
//!
//! DATA, not STATE: a draft is the user's text and must not be swept with caches. A draft is
//! written in place like a document (truncate + write), so the file keeps its mode; the folder and
//! the file are private to the user because an untitled document may hold anything.

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// The extension every draft carries (Markdown: the drafts are untitled documents).
const EXTENSION: &str = "md";
/// The folder's mode.
const FOLDER_MODE: u32 = 0o700;
/// A draft's mode.
const FILE_MODE: u32 = 0o600;

/// The drafts folder of this user.
#[derive(Clone, Debug)]
pub struct Drafts {
    dir: PathBuf,
}

impl Drafts {
    pub fn new(dir: PathBuf) -> Drafts {
        Drafts { dir }
    }

    /// The folder (`$XDG_DATA_HOME/md/drafts`).
    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A fresh draft path, `<uuid>.md` in the folder; nothing is written yet.
    pub fn new_path(&self) -> PathBuf {
        self.dir.join(format!("{}.{EXTENSION}", glib::uuid_string_random()))
    }

    /// Whether `path` names a draft: a `.md` file directly inside the drafts folder. This is how a
    /// `session.json` row is recognised as a draft rather than a saved document.
    pub fn contains(&self, path: &Path) -> bool {
        path.parent() == Some(self.dir.as_path()) && path.extension().is_some_and(|e| e == EXTENSION)
    }

    /// Writes `text` to the draft at `path` in place, creating the folder (`0700`) and the file
    /// (`0600`) when needed.
    pub fn write(&self, path: &Path, text: &str) -> io::Result<()> {
        if !self.dir.is_dir() {
            DirBuilder::new().recursive(true).mode(FOLDER_MODE).create(&self.dir)?;
            // `mode` applies to every folder the call creates; the drafts folder itself is set
            // explicitly in case it existed under another umask.
            fs::set_permissions(&self.dir, Permissions::from_mode(FOLDER_MODE))?;
        }
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(FILE_MODE).open(path)?;
        file.write_all(text.as_bytes())
    }

    /// A draft's text; `None` when it cannot be read. Drafts are written as UTF-8, so anything else
    /// is decoded lossily rather than lost.
    pub fn read(&self, path: &Path) -> Option<String> {
        let bytes = fs::read(path).ok()?;
        Some(match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
        })
    }

    /// Removes a draft; a draft that is already gone is not an error.
    pub fn delete(&self, path: &Path) {
        if let Err(err) = fs::remove_file(path)
            && err.kind() != io::ErrorKind::NotFound
        {
            eprintln!("md: could not delete the draft {}: {err}", path.display());
        }
    }

    /// Every draft in the folder, oldest first (by modification time, then name), so restored
    /// orphans come back in the order they were written.
    pub fn list(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.dir) else { return Vec::new() };
        let mut drafts: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| self.contains(path) && path.is_file())
            .map(|path| (fs::metadata(&path).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), path))
            .collect();
        drafts.sort();
        drafts.into_iter().map(|(_, path)| path).collect()
    }

    /// The drafts no `named` path accounts for — left by a crash between a draft write and the
    /// session write. They are restored after the named ones, so a crash never strands text.
    pub fn orphans(&self, named: &[PathBuf]) -> Vec<PathBuf> {
        self.list().into_iter().filter(|path| !named.contains(path)).collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh folder under the system temp dir, removed on drop.
    pub(crate) struct TempDir(pub PathBuf);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("md-{tag}-{}", glib::uuid_string_random()));
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_draft_is_created_private_written_in_place_and_read_back() {
        let temp = TempDir::new("drafts");
        let drafts = Drafts::new(temp.0.join("share/md/drafts"));
        let path = drafts.new_path();
        assert!(drafts.contains(&path));
        assert!(!path.exists());

        drafts.write(&path, "# Draft\n\nПривет 😀\n").unwrap();
        assert_eq!(mode(drafts.dir()), 0o700);
        assert_eq!(mode(&path), 0o600);
        assert_eq!(drafts.read(&path).as_deref(), Some("# Draft\n\nПривет 😀\n"));

        use std::os::unix::fs::MetadataExt;
        let inode = fs::metadata(&path).unwrap().ino();
        drafts.write(&path, "shorter").unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode, "in place, never a new file");
        assert_eq!(drafts.read(&path).as_deref(), Some("shorter"));
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn deleting_twice_is_harmless() {
        let temp = TempDir::new("drafts");
        let drafts = Drafts::new(temp.0.clone());
        let path = drafts.new_path();
        drafts.write(&path, "x").unwrap();
        drafts.delete(&path);
        assert!(!path.exists());
        drafts.delete(&path);
        assert_eq!(drafts.read(&path), None);
    }

    #[test]
    fn only_markdown_files_directly_in_the_folder_are_drafts() {
        let temp = TempDir::new("drafts");
        let drafts = Drafts::new(temp.0.join("drafts"));
        assert!(drafts.contains(&temp.0.join("drafts/abc.md")));
        assert!(!drafts.contains(&temp.0.join("drafts/abc.txt")));
        assert!(!drafts.contains(&temp.0.join("drafts/sub/abc.md")));
        assert!(!drafts.contains(&temp.0.join("abc.md")));
        assert!(!drafts.contains(Path::new("/home/me/notes.md")));
    }

    #[test]
    fn orphans_are_the_drafts_no_session_names_oldest_first() {
        let temp = TempDir::new("drafts");
        let drafts = Drafts::new(temp.0.clone());
        assert!(drafts.list().is_empty(), "a missing or empty folder lists nothing");
        let first = drafts.new_path();
        drafts.write(&first, "1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = drafts.new_path();
        drafts.write(&second, "2").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let third = drafts.new_path();
        drafts.write(&third, "3").unwrap();
        fs::write(temp.0.join("notes.txt"), "not a draft").unwrap();
        fs::create_dir(temp.0.join("folder.md")).unwrap();

        assert_eq!(drafts.list(), vec![first.clone(), second.clone(), third.clone()]);
        assert_eq!(drafts.orphans(std::slice::from_ref(&second)), vec![first, third]);
    }

    #[test]
    fn a_missing_folder_is_created_on_the_first_write() {
        let temp = TempDir::new("drafts");
        let drafts = Drafts::new(temp.0.join("a/b/drafts"));
        let path = drafts.new_path();
        drafts.write(&path, "text").unwrap();
        assert!(path.is_file());
    }
}
