//! Port of md.win `tests/Md.App.Logic.Tests/DocumentFixtures.cs`, plus the scratch folders the
//! disk tests need.
//!
//! Bytes the document tests need: the legacy encoding a Russian writer's older files are in, by
//! table rather than through the codec under test, and a real `.textpack` built with md-core's own
//! writer so the import path is exercised against an archive md itself could produce.

use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use md_core::export::text_bundle;

/// Windows-1251 bytes for ASCII and Russian text (md.win's `Encoding.GetEncoding(1251)`).
///
/// # Panics
///
/// For a character outside that table — a test bug.
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

/// UTF-16LE bytes, no BOM (md.win's `Encoding.Unicode.GetBytes`).
pub fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// `bytes` with `prefix` in front.
pub fn prefixed(prefix: &[u8], bytes: &[u8]) -> Vec<u8> {
    let mut out = prefix.to_vec();
    out.extend_from_slice(bytes);
    out
}

/// A `.textpack` whose nested bundle folder is `folder_name` and whose `text.md` is `text`.
pub fn text_pack(text: &str, folder_name: &str) -> Vec<u8> {
    text_bundle::bundle_wrapper(text, Vec::new()).to_text_pack(folder_name).expect("md-core writes the pack")
}

/// [`text_pack`] in md.win's default folder, `Notes.textbundle`.
pub fn notes_pack(text: &str) -> Vec<u8> {
    text_pack(text, "Notes.textbundle")
}

/// A name no other test in this run uses — md.win's `Guid.NewGuid().ToString("N")`.
pub fn unique(prefix: &str) -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("{prefix}{}-{}-{nanos}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// A scratch folder under the system temporary folder, unique per test (so the suites run in
/// parallel) and deleted with everything in it on drop. Its path is the *resolved* one
/// (`realpath`), so a canonical path computed under it compares equal on a system whose temporary
/// folder is reached through a link (macOS's `/var` → `/private/var`).
pub struct TempDir {
    pub path: String,
}

impl TempDir {
    pub fn new(prefix: &str) -> Self {
        let raw = std::env::temp_dir().join(unique(prefix));
        fs::create_dir_all(&raw).expect("create a scratch folder");
        let resolved = fs::canonicalize(&raw).expect("resolve the scratch folder");
        TempDir { path: resolved.into_os_string().into_string().expect("the temporary folder is UTF-8") }
    }

    /// The folder joined with `parts`, `/`-separated.
    pub fn join(&self, parts: &[&str]) -> String {
        let mut path = self.path.clone();
        for part in parts {
            path.push('/');
            path.push_str(part);
        }
        path
    }

    /// Write `data` at `relative`, creating its folders.
    pub fn write(&self, relative: &str, data: impl AsRef<[u8]>) -> String {
        let path = self.join(&[relative]);
        if let Some(parent) = std::path::Path::new(&path).parent() {
            fs::create_dir_all(parent).expect("create the folder");
        }
        fs::write(&path, data).expect("write the file");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The repository's root (the workspace folder), for the tests that read a checked-in file.
pub fn repo_root() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::canonicalize(root).expect("the repository root").into_os_string().into_string().expect("a UTF-8 path")
}
