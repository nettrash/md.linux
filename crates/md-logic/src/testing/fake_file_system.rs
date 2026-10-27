//! Port of md.win `FakeFileSystem`, with Linux path semantics.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::io;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use md_core::text::unicode;

use super::fake_clock::FakeClock;
use crate::seams::{FileStamp, FileSystem};

struct Entry {
    bytes: Vec<u8>,
    modified: SystemTime,
    read_only: bool,
}

/// In-memory [`FileSystem`] with Linux path semantics: case-sensitive, `/` the only separator (a
/// `\` is an ordinary character in a name), a trailing `/` ignored. Stamps behave like ext4's:
/// every write gets a strictly later mtime (the clock's time, nudged by a nanosecond when the
/// clock has not moved), a rename keeps the stamp, and [`FileSystem::stamp`] is `None` for a
/// missing file. The session's (mtime, size) guard can therefore be exercised exactly:
/// [`FakeFileSystem::write_externally`] is another program's edit, [`FakeFileSystem::touch`] an
/// attribute-only touch, [`FakeFileSystem::set_read_only`] the `chmod 444` file whose autosave
/// must fail into the save-error bar, [`FakeFileSystem::set_next_write_failure`] an I/O error for
/// the rescue-copy path. Every write through the seam is logged in [`FakeFileSystem::writes`].
///
/// LINUX DECISION: md.win's fake is case-insensitive and takes either separator, as NTFS does.
/// Errors carry the kind a Linux `std::fs` call would (`NotFound`, `AlreadyExists`,
/// `PermissionDenied`) and a message shaped like md-core's `"{path}: {OS message}"`.
/// [`FileSystem::move_file`] refuses to overwrite an existing target, as md.win's `File.Move`
/// does — note that the app's implementation must check, because `std::fs::rename` replaces
/// silently.
pub struct FakeFileSystem {
    clock: Rc<FakeClock>,
    files: RefCell<HashMap<String, Entry>>,
    dirs: RefCell<BTreeSet<String>>,
    last_stamp: Cell<Option<SystemTime>>,
    writes: RefCell<Vec<String>>,
    reads: RefCell<Vec<String>>,
    next_write_failure: RefCell<Option<io::Error>>,
}

impl Default for FakeFileSystem {
    fn default() -> Self {
        Self::with_clock(Rc::new(FakeClock::new()))
    }
}

impl fmt::Debug for FakeFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut files: Vec<String> = self.files.borrow().keys().cloned().collect();
        files.sort();
        f.debug_struct("FakeFileSystem").field("files", &files).field("dirs", &self.dirs.borrow()).finish()
    }
}

impl FakeFileSystem {
    /// A file system with its own clock.
    pub fn new() -> Self {
        Self::default()
    }

    /// A file system whose stamps follow a clock the test shares.
    pub fn with_clock(clock: Rc<FakeClock>) -> Self {
        FakeFileSystem {
            clock,
            files: RefCell::new(HashMap::new()),
            dirs: RefCell::new(BTreeSet::new()),
            last_stamp: Cell::new(None),
            writes: RefCell::new(Vec::new()),
            reads: RefCell::new(Vec::new()),
            next_write_failure: RefCell::new(None),
        }
    }

    pub fn clock(&self) -> &Rc<FakeClock> {
        &self.clock
    }

    /// Paths written through [`FileSystem::write_all_bytes_in_place`], in order (as given, not
    /// normalised) — attempts that failed included.
    pub fn writes(&self) -> Vec<String> {
        self.writes.borrow().clone()
    }

    /// Every path [`FileSystem::read_all_bytes`] was asked for, in order.
    pub fn reads(&self) -> Vec<String> {
        self.reads.borrow().clone()
    }

    /// Returned once by the next [`FileSystem::write_all_bytes_in_place`] (which still logs the
    /// attempt), then cleared. `None` clears a failure not yet used.
    pub fn set_next_write_failure(&self, failure: Option<io::Error>) {
        *self.next_write_failure.borrow_mut() = failure;
    }

    /// Whether a failure set with [`FakeFileSystem::set_next_write_failure`] is still waiting.
    pub fn has_next_write_failure(&self) -> bool {
        self.next_write_failure.borrow().is_some()
    }

    // ---- set-up and inspection ----

    /// Creates the folder and every missing folder above it.
    pub fn add_directory(&self, path: &str) {
        let key = Self::key(path);
        self.add_directory_key(&key);
    }

    /// Creates (or replaces) a file, creating its folders, stamped with the clock's time.
    pub fn add_file(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let modified = self.next_stamp_time();
        self.add_file_stamped(path, bytes, modified);
    }

    /// [`FakeFileSystem::add_file`] with an explicit mtime, kept as given.
    pub fn add_file_stamped(&self, path: &str, bytes: impl AsRef<[u8]>, modified: SystemTime) {
        let key = Self::key(path);
        self.add_directory_key(&Self::parent(&key));
        self.files.borrow_mut().insert(key, Entry { bytes: bytes.as_ref().to_vec(), modified, read_only: false });
    }

    /// Another program writes the file: new bytes, new mtime, no [`FakeFileSystem::writes`] entry.
    ///
    /// # Panics
    ///
    /// If the file does not exist.
    pub fn write_externally(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let modified = self.next_stamp_time();
        self.with_existing(path, |e| {
            e.bytes = bytes.as_ref().to_vec();
            e.modified = modified;
        });
    }

    /// mtime only — the "attribute-only touch" the stamp guard must still treat as a change.
    ///
    /// # Panics
    ///
    /// If the file does not exist.
    pub fn touch(&self, path: &str) {
        let modified = self.next_stamp_time();
        self.with_existing(path, |e| e.modified = modified);
    }

    /// `chmod 444` (true) or back (false): the in-place write is refused with `PermissionDenied`.
    ///
    /// # Panics
    ///
    /// If the file does not exist.
    pub fn set_read_only(&self, path: &str, read_only: bool) {
        self.with_existing(path, |e| e.read_only = read_only);
    }

    /// The file's bytes, `None` when it does not exist (nothing is logged).
    pub fn bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(&Self::key(path)).map(|e| e.bytes.clone())
    }

    /// The file's bytes as UTF-8 (invalid sequences become U+FFFD, as .NET's decoder does).
    pub fn text(&self, path: &str) -> Option<String> {
        self.bytes(path).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    /// Every file, sorted.
    pub fn file_paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.files.borrow().keys().cloned().collect();
        paths.sort();
        paths
    }

    /// The key a path is stored under: no trailing `/` (the root keeps its one). Nothing else is
    /// normalised — `a//b` and `a/./b` are other keys, as they are other strings to the logic.
    ///
    /// # Panics
    ///
    /// For an empty path (md.win throws `ArgumentException`).
    pub fn key(path: &str) -> String {
        assert!(!path.is_empty(), "FakeFileSystem: empty path");
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() { "/".to_string() } else { trimmed.to_string() }
    }

    // ---- internals ----

    fn parent(key: &str) -> String {
        match key.rfind('/') {
            None => String::new(),
            Some(0) => "/".to_string(),
            Some(cut) => key[..cut].to_string(),
        }
    }

    fn is_under(key: &str, dir: &str) -> bool {
        if key == dir {
            return true;
        }
        if dir.ends_with('/') {
            key.starts_with(dir)
        } else {
            key.starts_with(dir) && key[dir.len()..].starts_with('/')
        }
    }

    fn add_directory_key(&self, key: &str) {
        let mut dirs = self.dirs.borrow_mut();
        let mut d = key.to_string();
        // Walks up to the root, whose parent is itself: the second insert of "/" ends the walk.
        while !d.is_empty() {
            if !dirs.insert(d.clone()) {
                break;
            }
            d = Self::parent(&d);
        }
    }

    fn with_existing(&self, path: &str, change: impl FnOnce(&mut Entry)) {
        let mut files = self.files.borrow_mut();
        let entry = files
            .get_mut(&Self::key(path))
            .unwrap_or_else(|| panic!("FakeFileSystem: no file at {path} (add it first)"));
        change(entry);
    }

    fn next_stamp_time(&self) -> SystemTime {
        let mut t = self.clock.wall_now();
        if let Some(last) = self.last_stamp.get()
            && t <= last
        {
            t = last + Duration::from_nanos(1);
        }
        self.last_stamp.set(Some(t));
        t
    }

    fn not_found(path: &str) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, format!("{path}: No such file or directory (os error 2)"))
    }

    fn already_exists(path: &str) -> io::Error {
        io::Error::new(io::ErrorKind::AlreadyExists, format!("{path}: File exists (os error 17)"))
    }

    fn permission_denied(path: &str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, format!("{path}: Permission denied (os error 13)"))
    }

    /// .NET's `StringComparer.OrdinalIgnoreCase`: UTF-16 units compared after the simple upper
    /// mapping, ties broken ordinally so the order is total.
    fn ordinal_ignore_case(a: &str, b: &str) -> Ordering {
        let fold = |s: &str| -> Vec<u16> {
            let mut units = Vec::with_capacity(s.len());
            let mut buffer = [0u16; 2];
            for c in s.chars() {
                units.extend_from_slice(unicode::to_upper_invariant(c).encode_utf16(&mut buffer));
            }
            units
        };
        fold(a).cmp(&fold(b)).then_with(|| a.cmp(b))
    }
}

impl FileSystem for FakeFileSystem {
    fn file_exists(&self, path: &str) -> bool {
        self.files.borrow().contains_key(&Self::key(path))
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.dirs.borrow().contains(&Self::key(path))
    }

    fn read_all_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        self.reads.borrow_mut().push(path.to_string());
        self.files.borrow().get(&Self::key(path)).map(|e| e.bytes.clone()).ok_or_else(|| Self::not_found(path))
    }

    fn write_all_bytes_in_place(&self, path: &str, bytes: &[u8]) -> io::Result<()> {
        self.writes.borrow_mut().push(path.to_string());
        if let Some(failure) = self.next_write_failure.borrow_mut().take() {
            return Err(failure);
        }
        let key = Self::key(path);
        if !self.dirs.borrow().contains(&Self::parent(&key)) {
            return Err(Self::not_found(path));
        }
        if self.files.borrow().get(&key).is_some_and(|e| e.read_only) {
            return Err(Self::permission_denied(path));
        }
        let modified = self.next_stamp_time();
        let mut files = self.files.borrow_mut();
        let entry = files.entry(key).or_insert_with(|| Entry { bytes: Vec::new(), modified, read_only: false });
        entry.bytes = bytes.to_vec();
        entry.modified = modified;
        Ok(())
    }

    fn stamp(&self, path: &str) -> Option<FileStamp> {
        self.files
            .borrow()
            .get(&Self::key(path))
            .map(|e| FileStamp { modified: e.modified, size: e.bytes.len() as u64 })
    }

    fn move_file(&self, from: &str, to: &str) -> io::Result<()> {
        let src = Self::key(from);
        let dst = Self::key(to);
        if self.files.borrow().contains_key(&src) {
            if self.files.borrow().contains_key(&dst) {
                return Err(Self::already_exists(to));
            }
            if !self.dirs.borrow().contains(&Self::parent(&dst)) {
                return Err(Self::not_found(to));
            }
            let mut files = self.files.borrow_mut();
            if let Some(entry) = files.remove(&src) {
                files.insert(dst, entry); // a rename keeps the stamp, as ext4 does
            }
            return Ok(());
        }
        if self.dirs.borrow().contains(&src) {
            if self.dirs.borrow().contains(&dst) || self.files.borrow().contains_key(&dst) {
                return Err(Self::already_exists(to));
            }
            {
                let mut dirs = self.dirs.borrow_mut();
                let moved: Vec<String> = dirs.iter().filter(|d| Self::is_under(d, &src)).cloned().collect();
                for d in moved {
                    dirs.remove(&d);
                    dirs.insert(format!("{dst}{}", &d[src.len()..]));
                }
            }
            {
                let mut files = self.files.borrow_mut();
                let moved: Vec<String> = files.keys().filter(|k| Self::is_under(k, &src)).cloned().collect();
                for k in moved {
                    if let Some(entry) = files.remove(&k) {
                        files.insert(format!("{dst}{}", &k[src.len()..]), entry);
                    }
                }
            }
            self.add_directory_key(&Self::parent(&dst));
            return Ok(());
        }
        Err(Self::not_found(from))
    }

    fn delete(&self, path: &str) -> io::Result<()> {
        let key = Self::key(path);
        if self.files.borrow_mut().remove(&key).is_some() {
            return Ok(());
        }
        if self.dirs.borrow().contains(&key) {
            self.dirs.borrow_mut().retain(|d| !Self::is_under(d, &key));
            self.files.borrow_mut().retain(|k, _| !Self::is_under(k, &key));
        }
        // md.win's `File.Delete` of a missing file is a no-op; mirror that.
        Ok(())
    }

    fn enumerate_entries(&self, directory: &str) -> io::Result<Vec<String>> {
        let key = Self::key(directory);
        if !self.dirs.borrow().contains(&key) {
            return Err(Self::not_found(directory));
        }
        let mut entries: Vec<String> =
            self.dirs.borrow().iter().filter(|d| Self::parent(d) == key && **d != key).cloned().collect();
        entries.extend(self.files.borrow().keys().filter(|f| Self::parent(f) == key).cloned());
        entries.sort_by(|a, b| Self::ordinal_ignore_case(a, b));
        Ok(entries)
    }
}
