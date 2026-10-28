//! Remembers, per file, the raw mode it was last shown in (md.win `ViewModeMemory`,
//! `IViewModeStore` and `InMemoryViewModeStore`, part of `Document/ViewMode.cs`).
//!
//! One string under [`SETTINGS_KEY`]: a `v1` header line, then one `<16 hex> <token>` line per
//! file, newest first, no trailing newline, at most [`MAX_ENTRIES`] entries. Not JSON on purpose
//! (the Android suite cannot run org.json), and the ports must agree byte for byte.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;

use super::sha256;
use super::view_mode::ViewMode;
use crate::book::book_paths;
use crate::text::{graphemes, unicode, whitespace};

pub const SETTINGS_KEY: &str = "md.viewModeMemory";
pub const HEADER: &str = "v1";
pub const MAX_ENTRIES: usize = 200;

/// One remembered file: its identity hash and the raw mode it was last shown in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub identity: String,
    pub mode: ViewMode,
}

impl Entry {
    pub fn new(identity: impl Into<String>, mode: ViewMode) -> Self {
        Entry { identity: identity.into(), mode }
    }
}

/// Where the one `md.viewModeMemory` string lives. md.win backs it with
/// `ApplicationData.Current.LocalSettings`; the Linux app backs it with its settings store, tests
/// with [`InMemoryViewModeStore`]. `&self` on both so a store can be shared behind an `Rc`.
pub trait ViewModeStore {
    fn load(&self) -> Option<String>;
    fn save(&self, value: &str);
}

/// A [`ViewModeStore`] in memory that counts its reads and writes.
#[derive(Debug, Default)]
pub struct InMemoryViewModeStore {
    value: RefCell<Option<String>>,
    reads: Cell<usize>,
    writes: Cell<usize>,
}

impl InMemoryViewModeStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store already holding `value`.
    pub fn with_value(value: impl Into<String>) -> Self {
        let store = Self::default();
        *store.value.borrow_mut() = Some(value.into());
        store
    }

    /// What the store holds, without counting a read.
    pub fn value(&self) -> Option<String> {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, value: Option<String>) {
        *self.value.borrow_mut() = value;
    }

    pub fn reads(&self) -> usize {
        self.reads.get()
    }

    pub fn writes(&self) -> usize {
        self.writes.get()
    }
}

impl ViewModeStore for InMemoryViewModeStore {
    fn load(&self) -> Option<String> {
        self.reads.set(self.reads.get() + 1);
        self.value.borrow().clone()
    }

    fn save(&self, value: &str) {
        self.writes.set(self.writes.get() + 1);
        *self.value.borrow_mut() = Some(value.to_string());
    }
}

// MARK: Identity

/// The stable identity of a file: [`sha256_prefix`] of `"file:" + canonical_path(path)`. Apple
/// hashes `"file:" + url.resolvingSymlinksInPath().standardizedFileURL.path`; Android hashes
/// `"saf:" + authority + ":" + documentId`. The prefix is per platform — the store never syncs
/// between devices — so only the two ASCII vectors in the tests must agree across ports. A rename
/// or move outside the app changes the path and loses the memory. Accepted.
pub fn identity_for(path: &str) -> String {
    sha256_prefix(&format!("file:{}", canonical_path(path)))
}

/// The platform rule: Linux never folds case.
///
/// LINUX DECISION (md.win folds on Windows because NTFS compares names case-insensitively):
/// ext4, btrfs and xfs compare names byte for byte, so `A.md` and `a.md` are two files and must
/// hold two memories — the Mac's rule too, which hashes the resolved path exactly as spelled.
pub fn canonical_path(path: &str) -> String {
    canonical_path_with(path, false)
}

/// The bytes that get hashed (after the `"file:"` prefix):
/// 1. [`book_paths::standardize`] — absolute, `.`/`..` collapsed lexically, trailing separator
///    dropped (the Swift `standardizedFileURL`, md.win's `Path.GetFullPath`);
/// 2. symlinks resolved on every component (`realpath`), but ONLY when the whole path names
///    something on disk — Foundation's `resolvingSymlinksInPath()` is a no-op for a missing path,
///    and an identity is only ever computed for a document the app has open. A link cycle names
///    nothing (`stat` fails with `ELOOP`), so it ends in "the path as given", never in a hang;
///    a resolved path that is not UTF-8 is not used either;
/// 3. when `fold_case`: `ToUpperInvariant` ([`unicode::upper_invariant`]). Kept for parity with
///    md.win's pinned two-argument form; no Linux caller asks for it.
pub fn canonical_path_with(path: &str, fold_case: bool) -> String {
    let full = resolve_links(book_paths::standardize(path));
    if fold_case { unicode::upper_invariant(&full) } else { full }
}

/// First 16 hex digits (8 bytes) of SHA-256 over the UTF-8 bytes of `value`. A Rust string is
/// always valid UTF-8, so the lone-surrogate case md.win maps to U+FFFD cannot arise.
pub fn sha256_prefix(value: &str) -> String {
    sha256::hex_lower(&sha256::digest(value.as_bytes())[..8])
}

fn resolve_links(full: String) -> String {
    // `exists()` follows links, as .NET's File.Exists / Directory.Exists and Foundation's
    // fileExists do: a dangling or looping link names nothing and is left as given.
    if !Path::new(&full).exists() {
        return full;
    }
    match std::fs::canonicalize(&full) {
        Ok(resolved) => resolved.into_os_string().into_string().unwrap_or(full),
        Err(_) => full,
    }
}

// MARK: Tokens

/// The stored spelling. Written out by hand — never the enum name — so a rename cannot re-spell
/// the disk.
pub fn token(mode: ViewMode) -> &'static str {
    match mode {
        ViewMode::Edit => "edit",
        ViewMode::Split => "split",
        ViewMode::Preview => "preview",
    }
}

/// The mode a token names, or `None`. Surrounding Foundation whitespace (Zs + TAB + U+200B, no
/// newlines — Swift `.whitespaces`, [`whitespace::trim_ws`]) is forgiven and ASCII case is folded.
/// ASCII-only on purpose: Swift's `lowercased()` is a full mapping, so no non-ASCII letter can
/// ever produce one of these three words there (İ → i̇, ı stays ı); a Unicode-aware fold would let
/// `EDİT` and `edıt` through.
pub fn mode_for_token(token: &str) -> Option<ViewMode> {
    let trimmed = whitespace::trim_ws(token);
    if trimmed.eq_ignore_ascii_case("edit") {
        Some(ViewMode::Edit)
    } else if trimmed.eq_ignore_ascii_case("split") {
        Some(ViewMode::Split)
    } else if trimmed.eq_ignore_ascii_case("preview") {
        Some(ViewMode::Preview)
    } else {
        None
    }
}

// MARK: Codec

/// Parse a stored value. A missing or wrong first line means "absent" — the whole value is
/// discarded. Past that a line that does not parse is skipped, never fatal, and the first entry
/// for an identity wins. Lines split on LF (empty lines kept), fields on U+0020 (empty fields
/// dropped) — both the way Swift's `split(separator:)` does it, over Characters
/// (`split_characters`), so a CRLF value reads as absent here exactly as on the Mac, wherever
/// the CR sits. Kotlin splits on code units and `trim()`s each line, which reads a CRLF value as
/// present; the ports agree on every value any of them writes, and the Mac is the source of truth
/// for the rest.
pub fn decode(text: Option<&str>) -> Vec<Entry> {
    let Some(text) = text else {
        return Vec::new();
    };
    let lines = split_characters(text, "\n", false);
    if lines[0] != HEADER {
        return Vec::new();
    }

    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for line in &lines[1..] {
        let fields = split_characters(line, " ", true);
        // The mode is parsed BEFORE the identity is marked seen: an unreadable token must not
        // consume that file's one slot and hide a good line for the same file further down.
        if fields.len() != 2 {
            continue;
        }
        let Some(mode) = mode_for_token(fields[1]) else {
            continue;
        };
        let identity = fields[0];
        if !is_identity(identity) || !seen.insert(identity) {
            continue;
        }
        entries.push(Entry::new(identity, mode));
    }
    entries
}

/// Swift `String.split(separator:)`: the separator is a Character, so one fused into a larger
/// extended grapheme cluster — `"\r\n"`, or a space wearing a combining mark — is not a
/// separator. The walk is over UAX #29 clusters ([`graphemes::clusters`]), so this is the Swift
/// call, not a code-unit approximation of it. Returns `[""]` for an empty string when empties are
/// kept.
fn split_characters<'a>(text: &'a str, separator: &str, omit_empty: bool) -> Vec<&'a str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 0;
    for cluster in graphemes::clusters(text) {
        if cluster == separator {
            if !omit_empty || i > start {
                parts.push(&text[start..i]);
            }
            start = i + cluster.len();
        }
        i += cluster.len();
    }
    if !omit_empty || start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

/// Render entries back. No trailing newline, so the codec round-trips exactly; `encode(&[]) == "v1"`.
pub fn encode(entries: &[Entry]) -> String {
    let mut out = String::from(HEADER);
    for entry in entries {
        out.push('\n');
        out.push_str(&entry.identity);
        out.push(' ');
        out.push_str(token(entry.mode));
    }
    out
}

/// Exactly 16 characters, each in `0123456789abcdef`. Uppercase is rejected — no port writes it,
/// so accepting it would let one file hold two disagreeing entries. ASCII membership, so
/// full-width digits are out; the byte length equals md.win's UTF-16 length and Swift's grapheme
/// count here because any non-ASCII fails the hex test anyway.
pub fn is_identity(candidate: &str) -> bool {
    candidate.len() == 16 && candidate.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// MARK: The MRU list

/// `entries` with `identity` at the front as `mode`, the oldest past [`MAX_ENTRIES`] dropped.
pub fn touched(entries: &[Entry], identity: &str, mode: ViewMode) -> Vec<Entry> {
    let mut updated = Vec::with_capacity(entries.len() + 1);
    updated.push(Entry::new(identity, mode));
    updated.extend(entries.iter().filter(|entry| entry.identity != identity).cloned());
    updated.truncate(MAX_ENTRIES);
    updated
}

// MARK: Lookup / store

/// Everything remembered, newest first. The one read path, so an absent or unreadable value
/// becomes "nothing" in one place.
pub fn entries(store: &dyn ViewModeStore) -> Vec<Entry> {
    decode(store.load().as_deref())
}

pub fn lookup(identity: &str, store: &dyn ViewModeStore) -> Option<ViewMode> {
    entries(store).into_iter().find(|entry| entry.identity == identity).map(|entry| entry.mode)
}

/// `mode` must be the RAW preference — never `view_mode_rule::effective_mode`'s output.
pub fn remember(mode: ViewMode, identity: &str, store: &dyn ViewModeStore) {
    store.save(&encode(&touched(&entries(store), identity, mode)));
}
