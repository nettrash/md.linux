//! TextBundle (`.textbundle`, a folder) and TextPack (`.textpack`, that folder zipped) — the
//! Markdown-with-assets container Ulysses, iA Writer and Bear write (macOS `TextBundle`, md.win
//! `Md.Core.Export.TextBundle`). The document model is a single string, so the round trip here
//! is text, not pictures: import loads `text.md`; export copies the findable local images into
//! `assets/` and rewrites their refs, leaving every unfindable ref exactly as the author wrote it.
//! Bundles are read-only for the document (saving one back would drop its assets); producing one
//! is the explicit Export action.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::zip_reader;
use super::zip_writer::{ZipEntry, ZipError, ZipWriter};
use crate::document::plain_text_codec::{self, DecodedText};
use crate::text::unicode::{self, GeneralCategory};

/// The `info.json` every bundle we write carries — a fixed canonical document (stable key order,
/// no trailing newline, 81 bytes) so it is trivially testable.
pub const INFO_JSON: &str =
    "{\n  \"version\": 2,\n  \"type\": \"net.daringfireball.markdown\",\n  \"transient\": false\n}";

pub const TEXT_FILE_NAME: &str = "text.md";
pub const INFO_FILE_NAME: &str = "info.json";
pub const ASSETS_DIRECTORY_NAME: &str = "assets";
pub const BUNDLE_EXTENSION: &str = ".textbundle";
pub const PACK_EXTENSION: &str = ".textpack";

/// One image copied into a written bundle's `assets/`. Equality is by name and bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Asset {
    pub name: String,
    pub data: Vec<u8>,
}

impl Asset {
    pub fn new(name: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self { name: name.into(), data: data.into() }
    }
}

impl fmt::Debug for Asset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Asset({:?}, {} bytes)", self.name, self.data.len())
    }
}

/// The rewritten document and the assets to write beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    pub text: String,
    pub assets: Vec<Asset>,
}

// MARK: Import

/// The text of a `.textbundle` given as its top-level files (name → bytes), decoded with the same
/// encoding detection a bare file gets so a legacy-encoded bundle round-trips. Prefers `text.md`,
/// then `text.markdown`, then any `text.*` (ordinal-smallest name: Swift's dictionary order is
/// unspecified, so the tie is made deterministic here). `None` when there is no text file.
pub fn text_from_bundle(children: &HashMap<String, Vec<u8>>) -> Option<DecodedText> {
    let name = pick_text_file(children.keys().map(String::as_str))?;
    plain_text_codec::decode(&children[name])
}

/// The same import over a `.textbundle` folder on disk. A missing or unreadable folder, or one
/// without a text file, is `None`, never an error.
pub fn text_from_bundle_directory(bundle_directory: &Path) -> Option<DecodedText> {
    if !bundle_directory.is_dir() {
        return None;
    }
    let mut by_name: HashMap<String, PathBuf> = HashMap::new();
    for entry in fs::read_dir(bundle_directory).ok()? {
        let path = entry.ok()?.path();
        // md.win lists the folder's files: everything that is not a directory, a link to a
        // directory counting as one.
        if fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        let Some(name) = path.file_name() else { continue };
        by_name.entry(name.to_string_lossy().into_owned()).or_insert_with(|| path.clone());
    }
    let name = pick_text_file(by_name.keys().map(String::as_str))?;
    let bytes = fs::read(&by_name[name]).ok()?;
    plain_text_codec::decode(&bytes)
}

/// The text of a `.textpack`. The pack wraps a `.textbundle` folder, so the entry is nested under
/// it and matched by its last path component. Only `text.*` entries are inflated — every asset is
/// listed but never decompressed, so a pack that declares gigabytes of assets costs nothing to
/// open. `None` when the bytes are not a readable zip, carry no text file, or the text entry is
/// empty.
pub fn text_from_pack(archive: &[u8]) -> Option<DecodedText> {
    let is_text = |name: &str| last_component(name).starts_with("text.");
    let entries = zip_reader::entries(archive, Some(&is_text))?;
    let entry = entries
        .iter()
        .find(|entry| last_component(&entry.name) == "text.md")
        .or_else(|| entries.iter().find(|entry| last_component(&entry.name) == "text.markdown"))
        .or_else(|| entries.iter().find(|entry| is_text(&entry.name)))?;
    if entry.data.is_empty() {
        return None;
    }
    plain_text_codec::decode(&entry.data)
}

/// Whether an opened file should be read as a pack — named `.textpack` (ASCII case ignored), or
/// carrying the full four-byte local-header magic `PK\x03\x04` (Android's sniff). The full four
/// bytes, never just "PK": prose that begins with the letters PK is ordinary text.
///
/// md.win compares the extension with `OrdinalIgnoreCase`; folding ASCII only is exact for this
/// suffix, since no other character's simple upper case is one of its letters (pinned by a test).
pub fn looks_like_pack(name: &str, bytes: &[u8]) -> bool {
    let suffix = PACK_EXTENSION.as_bytes();
    let name = name.as_bytes();
    if name.len() >= suffix.len() && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix) {
        return true;
    }
    bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04])
}

// MARK: Export

/// Whether a Markdown image destination names a local file by relative path — the only kind that
/// can be copied into `assets/`. Remote URLs (anything with a scheme), inline `data:` images,
/// absolute paths and bare fragments are left untouched. Ordinal throughout: a combining mark
/// after ':' or '/' is its own scalar and never manufactures a "://". The scheme fold is
/// ASCII-only, which is exact: Swift's and Kotlin's full case mapping never turn a non-ASCII
/// letter into one of these needles' letters (İ becomes "i" + U+0307, not i).
pub fn is_local_relative_reference(url: &str) -> bool {
    if url.is_empty() {
        return false;
    }
    if url.contains("://") {
        return false; // http:, https:, file:, custom
    }
    if starts_with_ascii_ignoring_case(url, "data:") {
        return false; // inline image bytes
    }
    if starts_with_ascii_ignoring_case(url, "mailto:") {
        return false;
    }
    if url.starts_with('/') {
        return false; // absolute path — not "next to" the source
    }
    if url.starts_with('#') {
        return false; // in-document fragment
    }
    true
}

/// Rewrite the document for export into a bundle: every local image ref `resolve_asset` can
/// satisfy is copied into `assets/` and its ref rewritten to `assets/<name>`; refs it cannot
/// satisfy (a file that is not there, a remote URL, an absolute path) stay exactly as written.
/// Only the URL is replaced, so a following "title" survives. The same path used twice becomes
/// one asset; two distinct paths sharing a file name are disambiguated (`logo-2.png`),
/// case-insensitively. An image-looking string inside a code span is also matched — accepted: it
/// is only rewritten when a real file exists, and then still resolves.
pub fn export_rewriting(source: &str, mut resolve_asset: impl FnMut(&str) -> Option<Vec<u8>>) -> Rewrite {
    let mut assets = Vec::new();
    let mut asset_for_path: HashMap<&str, String> = HashMap::new(); // source ref -> asset file name
    let mut used_names: HashSet<String> = HashSet::new(); // lower-cased, for collision-free names
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();

    for destination in image_destinations(source) {
        let url = &source[destination.clone()];
        if !is_local_relative_reference(url) {
            continue;
        }
        if let Some(existing) = asset_for_path.get(url) {
            edits.push((destination, format!("assets/{existing}")));
            continue;
        }
        let Some(data) = resolve_asset(url) else { continue };
        let base_name = last_component(url);
        if base_name.is_empty() {
            continue;
        }

        let mut name = base_name.to_string();
        let mut counter = 2;
        while used_names.contains(&unicode::lower_invariant(&name)) {
            name = disambiguated(base_name, counter);
            counter += 1;
        }
        used_names.insert(unicode::lower_invariant(&name));
        asset_for_path.insert(url, name.clone());
        edits.push((destination, format!("assets/{name}")));
        assets.push(Asset { name, data });
    }

    if edits.is_empty() {
        return Rewrite { text: source.to_string(), assets };
    }
    // Matches are disjoint and in order, so a forward splice equals Swift's back-to-front
    // replacement on the NSMutableString.
    let mut text = String::with_capacity(source.len() + edits.len() * 8);
    let mut cursor = 0;
    for (range, replacement) in edits {
        text.push_str(&source[cursor..range.start]);
        text.push_str(&replacement);
        cursor = range.end;
    }
    text.push_str(&source[cursor..]);
    Rewrite { text, assets }
}

/// Assemble the bundle: `text.md` (UTF-8, no BOM), `info.json` and an `assets/` folder — always
/// present, empty when there are no images.
pub fn bundle_wrapper(text: &str, assets: Vec<Asset>) -> BundleWrapper {
    BundleWrapper { text_bytes: text.as_bytes().to_vec(), info_json_bytes: INFO_JSON.as_bytes().to_vec(), assets }
}

/// Read an image the document references by relative path, if it sits beside the saved document.
/// The resolved path is constrained to the document's own folder: a `../…` that climbs out, or a
/// symlink beside the document pointing anywhere else, is treated as not found (and so left
/// untouched). `..` is folded lexically first (like `standardizedFileURL` and .NET's
/// `GetFullPath`), then links are resolved on both sides before the containment test; the
/// candidate must lie strictly inside the folder, and what is read is the resolved path the test
/// approved. An unsaved document (`None`) finds nothing, so it exports with an empty `assets/`.
/// Names compare byte for byte: Linux file systems are case-sensitive (md.win folds case on
/// Windows, where NTFS does).
pub fn read_asset(relative_path: &str, document_path: Option<&Path>) -> Option<Vec<u8>> {
    let document = lexical_full_path(document_path?).ok()?;
    let folder = resolve_links(document.parent()?, 0).ok()?;
    let candidate = resolve_links(&lexical_full_path(&folder.join(relative_path)).ok()?, 0).ok()?;
    if candidate == folder || !candidate.starts_with(&folder) {
        return None;
    }
    if !fs::metadata(&candidate).is_ok_and(|metadata| metadata.is_file()) {
        return None;
    }
    fs::read(&candidate).ok()
}

/// The assembled `.textbundle` as an in-memory description — what the folder holds — plus the
/// two ways of materialising it: a folder on disk, or the entry set of a `.textpack`.
#[derive(Clone, PartialEq, Eq)]
pub struct BundleWrapper {
    text_bytes: Vec<u8>,
    info_json_bytes: Vec<u8>,
    assets: Vec<Asset>,
}

impl fmt::Debug for BundleWrapper {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BundleWrapper(text.md {} bytes, assets {:?})", self.text_bytes.len(), self.assets)
    }
}

impl BundleWrapper {
    /// `text.md`: the (rewritten) document as UTF-8 without a BOM.
    pub fn text_bytes(&self) -> &[u8] {
        &self.text_bytes
    }

    /// `info.json`: [`INFO_JSON`] as UTF-8.
    pub fn info_json_bytes(&self) -> &[u8] {
        &self.info_json_bytes
    }

    /// The files of `assets/`; the folder exists even when this is empty.
    pub fn assets(&self) -> &[Asset] {
        &self.assets
    }

    /// Write the bundle as a folder at `bundle_directory`. Like FileWrapper's atomic write, an
    /// existing bundle there is replaced whole (stale assets do not linger): the tree is built
    /// beside it and swapped in. An existing symlink at the destination is replaced, never
    /// followed. An asset whose name is not a single plain file name is refused before anything
    /// is written (export only produces last path components; this keeps a hand-built wrapper
    /// from writing outside its `assets/`).
    pub fn write(&self, bundle_directory: &Path) -> io::Result<()> {
        let target = lexical_full_path(bundle_directory)?;
        let (Some(parent), Some(file_name)) = (target.parent(), target.file_name()) else {
            return Err(io::Error::other("A bundle cannot be written at a file-system root."));
        };
        if let Some(asset) = self.assets.iter().find(|asset| !is_plain_file_name(&asset.name)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{:?} is not a file name an asset can have.", asset.name),
            ));
        }
        let staging = create_staging_directory(parent, file_name.to_os_string())?;
        let result = self.fill_and_swap(&staging, &target);
        if fs::symlink_metadata(&staging).is_ok() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    fn fill_and_swap(&self, staging: &Path, target: &Path) -> io::Result<()> {
        fs::write(staging.join(TEXT_FILE_NAME), &self.text_bytes)?;
        fs::write(staging.join(INFO_FILE_NAME), &self.info_json_bytes)?;
        let assets = staging.join(ASSETS_DIRECTORY_NAME);
        fs::create_dir(&assets)?;
        for asset in &self.assets {
            fs::write(assets.join(&asset.name), &asset.data)?;
        }
        match fs::symlink_metadata(target) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(target)?,
            Ok(_) => fs::remove_file(target)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(staging, target)
    }

    /// The archive entries of a `.textpack`, nested under `<folder_name>.textbundle/` (Android's
    /// `bundleEntries`): `info.json`, `text.md`, then the assets — or an explicit empty `assets/`
    /// directory entry, so the folder is always present.
    pub fn pack_entries(&self, folder_name: &str) -> Vec<ZipEntry> {
        let root = format!("{folder_name}{BUNDLE_EXTENSION}/");
        let mut entries = Vec::with_capacity(3 + self.assets.len());
        entries.push(ZipEntry::new(format!("{root}{INFO_FILE_NAME}"), self.info_json_bytes.clone()));
        entries.push(ZipEntry::new(format!("{root}{TEXT_FILE_NAME}"), self.text_bytes.clone()));
        if self.assets.is_empty() {
            entries.push(ZipEntry::new(format!("{root}{ASSETS_DIRECTORY_NAME}/"), Vec::new()));
        } else {
            for asset in &self.assets {
                entries
                    .push(ZipEntry::new(format!("{root}{ASSETS_DIRECTORY_NAME}/{}", asset.name), asset.data.clone()));
            }
        }
        entries
    }

    /// The bundle zipped as a `.textpack` through the app's own STORED writer.
    pub fn to_text_pack(&self, folder_name: &str) -> Result<Vec<u8>, ZipError> {
        ZipWriter::archive(&self.pack_entries(folder_name))
    }
}

// MARK: Helpers

/// Exact-name preference: text.md, text.markdown, then the ordinal-smallest text.*.
fn pick_text_file<'a>(names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut any: Option<&str> = None;
    let mut has_md = false;
    let mut has_markdown = false;
    for name in names {
        if name == "text.md" {
            has_md = true;
        } else if name == "text.markdown" {
            has_markdown = true;
        } else if name.starts_with("text.") && any.is_none_or(|current| name < current) {
            any = Some(name);
        }
    }
    if has_md {
        Some("text.md")
    } else if has_markdown {
        Some("text.markdown")
    } else {
        any
    }
}

/// The last '/'-separated component (empty for a trailing slash, like the Swift split and
/// Kotlin's substringAfterLast).
fn last_component(path: &str) -> &str {
    path.rfind('/').map_or(path, |slash| &path[slash + 1..])
}

/// Prefix test folding only A–Z; `needle` is lower-case ASCII.
fn starts_with_ascii_ignoring_case(text: &str, needle: &str) -> bool {
    text.len() >= needle.len()
        && text.as_bytes().iter().zip(needle.as_bytes()).all(|(&c, &n)| c.to_ascii_lowercase() == n)
}

/// photo.png → photo-2.png; README → README-2. The dot must be past the first character so a
/// dotfile (.gitignore) keeps its whole name as the stem.
fn disambiguated(name: &str, counter: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{}-{counter}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}-{counter}"),
    }
}

/// ICU's `\s` (NSRegularExpression), measured on macOS against every candidate and spelled out
/// in md.win's pattern: TAB, LF, VT, FF, CR, NEL (U+0085) and every Z (Zs, Zl, Zp); not U+200B,
/// not U+FEFF. The HTML writer's image syntax uses the identical class.
fn is_pattern_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r' | '\u{0085}')
        || matches!(
            unicode::category(c),
            GeneralCategory::SpaceSeparator | GeneralCategory::LineSeparator | GeneralCategory::ParagraphSeparator
        )
}

/// The ranges of every image destination in `source`, in order — what md.win's
///
/// `!\[[^\]]*\]\(([^)\s]+)(?:\s+(?:"[^"]*"|'[^']*'))?\)` (with `\s` as [`is_pattern_space`])
///
/// captures as group 1 under `Regex.Matches`: matches are tried at every position left to right
/// and resume after the previous match. The pattern backtracks into nothing: the alt text can
/// only end at the first ']', the destination only at the first ')' or space, and after a
/// destination a space must open a title that closes right before ')' — so one forward pass per
/// candidate "![" decides exactly what the engine decides.
fn image_destinations(source: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = source[from..].find("![") {
        let start = from + offset;
        match image_at(source, start) {
            Some((destination, end)) => {
                found.push(destination);
                from = end;
            }
            None => from = start + 1,
        }
    }
    found
}

/// The pattern anchored at `start` (where "![" is): the destination's range and the match's end.
fn image_at(source: &str, start: usize) -> Option<(Range<usize>, usize)> {
    let bytes = source.as_bytes();
    // `[^\]]*\]` — line breaks included.
    let close = start + 2 + source[start + 2..].find(']')?;
    // `\(`
    if bytes.get(close + 1) != Some(&b'(') {
        return None;
    }
    // `([^)\s]+)`
    let destination_start = close + 2;
    let rest = &source[destination_start..];
    let length = rest.find(|c: char| c == ')' || is_pattern_space(c)).unwrap_or(rest.len());
    if length == 0 {
        return None;
    }
    let destination = destination_start..destination_start + length;
    let after = &source[destination.end..];
    if after.starts_with(')') {
        return Some((destination.clone(), destination.end + 1));
    }
    // `(?:\s+(?:"[^"]*"|'[^']*'))?\)` — reached only at a space (or the end, which fails).
    let quote_at = destination.end + after.find(|c: char| !is_pattern_space(c))?;
    let quote = bytes[quote_at];
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let closing = quote_at + 1 + source[quote_at + 1..].find(char::from(quote))?;
    if bytes.get(closing + 1) != Some(&b')') {
        return None;
    }
    Some((destination, closing + 2))
}

fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// An absolute path with `.` and `..` folded lexically, links untouched — .NET's `GetFullPath`
/// (`std::path::absolute` keeps `..` on Unix).
fn lexical_full_path(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "The path is empty."));
    }
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir()?.join(path) };
    let mut folded = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                folded.pop();
            }
            other => folded.push(other.as_os_str()),
        }
    }
    Ok(folded)
}

/// Resolve every symlink along a full path, the way `resolvingSymlinksInPath` does: a link's
/// target replaces the path up to it and the walk starts over, so a linked folder beside the
/// document cannot pass the containment test while reading elsewhere. Forty links at most,
/// Linux's own limit.
fn resolve_links(full_path: &Path, depth: u32) -> io::Result<PathBuf> {
    if depth > 40 {
        return Err(io::Error::other("Too many levels of symbolic links."));
    }
    let components: Vec<Component> = full_path.components().collect();
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        if !matches!(component, Component::Normal(_)) {
            continue;
        }
        let is_link = fs::symlink_metadata(&current).is_ok_and(|metadata| metadata.file_type().is_symlink());
        if !is_link {
            continue;
        }
        let target = fs::read_link(&current)?;
        let mut next = current.parent().map_or_else(|| PathBuf::from("/"), Path::to_path_buf).join(target);
        for rest in &components[index + 1..] {
            next.push(rest.as_os_str());
        }
        return resolve_links(&lexical_full_path(&next)?, depth + 1);
    }
    Ok(current)
}

/// A fresh, empty, uniquely named folder beside the destination: `.<name>-<32 hex>.tmp`, the
/// shape of md.win's GUID-named staging folder. Created with `create_dir`, so a name another
/// writer holds is never shared.
fn create_staging_directory(parent: &Path, file_name: OsString) -> io::Result<PathBuf> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos() as u64);
    loop {
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(&file_name);
        name.push(format!("-{:08x}{nanos:016x}{:08x}.tmp", std::process::id(), sequence as u32));
        let staging = parent.join(name);
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_other_character_upper_cases_into_the_pack_extension() {
        // looks_like_pack folds ASCII only where md.win folds with OrdinalIgnoreCase (simple
        // upper case per character): exact, because no non-ASCII character upper-cases to a
        // letter of ".textpack".
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32).filter(|c| !c.is_ascii()) {
            let upper = unicode::to_upper_invariant(c);
            assert!(!".TEXTPACK".contains(upper), "{c:?} upper-cases to {upper:?}");
        }
    }

    #[test]
    fn the_destination_matcher_follows_the_engine() {
        let destinations = |source: &str| -> Vec<String> {
            image_destinations(source).into_iter().map(|range| source[range].to_string()).collect()
        };
        // The alt text ends at the FIRST ']' and spans lines; an inner "![" is alt text.
        assert_eq!(destinations("![![a](b.png)"), ["b.png"]);
        assert_eq!(destinations("![a\nb](c.png)"), ["c.png"]);
        // The destination runs to the first ')', taking brackets with it.
        assert_eq!(destinations("![a](![b](c.png))"), ["![b](c.png"]);
        // A title must close right before ')'; otherwise nothing matches at that start.
        assert_eq!(destinations("![a](b.png \"t\" )"), Vec::<String>::new());
        assert_eq!(destinations("![a](b.png \"t)\")"), ["b.png"]);
        assert_eq!(destinations("![a](b.png 'it''s')"), Vec::<String>::new());
        assert_eq!(destinations("![a](b.png  \t'x')![c](d)"), ["b.png", "d"]);
        // No destination, no ')' or no '(' straight after ']': no match.
        assert_eq!(destinations("![a]() ![b] (c) ![d](e"), Vec::<String>::new());
        // A failed start resumes one character later, not after the failure.
        assert_eq!(destinations("![x](a b)![y](c)"), ["c"]);
        assert_eq!(destinations("!![a](b)"), ["b"]);
    }
}
