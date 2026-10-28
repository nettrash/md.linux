//! Helpers the document and book integration tests share: scratch folders that clean up after
//! themselves (unique per test, so the suites run in parallel), md.win's `Path.Combine` /
//! `Path.GetTempPath` spellings, a Windows-1251 encoder that does not go through the codec under
//! test, and the read-only switch the save-failure tests need.
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use md_core::book::book_paths;

/// `Path.GetTempPath()`: the system temporary folder.
pub fn temp_path() -> String {
    std::env::temp_dir().into_os_string().into_string().expect("the temporary folder is UTF-8")
}

/// `Path.Combine(parts…)`.
pub fn combine(parts: &[&str]) -> String {
    parts.iter().fold(String::new(), |path, part| book_paths::combine(&path, part))
}

/// A name no other test in this run uses — md.win's `Guid.NewGuid().ToString("N")`.
pub fn unique(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{prefix}{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// A scratch folder under the system temporary folder, deleted with everything in it on drop.
pub struct TempDir {
    pub path: String,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let path = combine(&[&temp_path(), &unique(prefix)]);
        fs::create_dir_all(&path).expect("create a scratch folder");
        TempDir { path }
    }

    /// `Path.Combine(self, parts…)`.
    pub fn join(&self, parts: &[&str]) -> String {
        let mut all = vec![self.path.as_str()];
        all.extend_from_slice(parts);
        combine(&all)
    }

    /// The entry names directly inside `relative` (the folder itself for ""), sorted ordinally.
    pub fn names(&self, relative: &str) -> Vec<String> {
        let folder = if relative.is_empty() { self.path.clone() } else { self.join(&[relative]) };
        names_in(&folder)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The entry names directly inside `folder`, sorted ordinally.
pub fn names_in(folder: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .expect("list the folder")
        .map(|entry| entry.expect("an entry").file_name().into_string().expect("a UTF-8 name"))
        .collect();
    names.sort();
    names
}

/// Write `data` to `path`, creating its folder.
pub fn write(path: &str, data: &[u8]) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        fs::create_dir_all(parent).expect("create the folder");
    }
    fs::write(path, data).expect("write the file");
}

/// The file as strict UTF-8 (md.win's `new UTF8Encoding(false, throwOnInvalidBytes: true)`).
pub fn read_text(path: &str) -> String {
    String::from_utf8(fs::read(path).expect("read the file")).expect("the file is UTF-8")
}

/// md.win's `File.SetAttributes(…ReadOnly)`: on Linux the write bits.
pub fn set_read_only(path: &str, read_only: bool) {
    let mode = if read_only { 0o444 } else { 0o644 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("change the permissions");
}

/// Make `path` read-only and report whether that actually refuses a write. It does not for root,
/// which ignores permission bits — the tests that need a failing save then skip with a note
/// rather than fail for a reason that has nothing to do with the code under test.
pub fn make_unwritable(path: &str) -> bool {
    set_read_only(path, true);
    let enforced = fs::OpenOptions::new().write(true).open(path).is_err();
    if !enforced {
        eprintln!("skipped: permission bits do not stop this user (root?) from writing {path}");
        set_read_only(path, false);
    }
    enforced
}

/// Windows-1251 bytes for ASCII and Russian text, by table — independent of the codec under test
/// (md.win's tests use .NET's own code page 1251 encoder for the same reason).
pub fn cp1251(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '\u{0000}'..='\u{007F}' => c as u8,
            '\u{0410}'..='\u{044F}' => (c as u32 - 0x0410 + 0xC0) as u8,
            '\u{0401}' => 0xA8,
            '\u{0451}' => 0xB8,
            other => panic!("{other:?} is outside the test's CP1251 table"),
        })
        .collect()
}

/// Latin-1 decoding (md.win's `Encoding.Latin1.GetString`): every byte is its own code point.
pub fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// UTF-16LE bytes, no BOM (md.win's `new UnicodeEncoding(false, false).GetBytes`).
pub fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
