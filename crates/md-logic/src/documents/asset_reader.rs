//! Port of md.win `Documents/AssetReader.cs`.
//!
//! The one place a document's own folder is read (§4.9, §7.7). The live preview never resolves a
//! relative image — a `photo.png` is fetched from the app's asset scheme and 404s, as on every
//! port — so this exists only for Export ▸ TextBundle…, which copies the findable images into
//! `assets/` (md-core's `text_bundle::export_rewriting` takes the resolver this returns).
//!
//! Containment is the whole point: a document is a text file from anywhere, and its
//! `![](../../../etc/shadow)` must resolve to nothing rather than to a file the export would then
//! copy out. The reference must be relative (a leading `/` is refused) and the resolved path must
//! lie *strictly inside* the document's folder after links are resolved on both sides, which is
//! what [`FileIdentity`] is for: a symlink beside the document pointing at `/` resolves out of the
//! folder and fails the test.
//!
//! LINUX DECISION: md.win also refuses a drive letter and a UNC head (its documented Windows
//! deviation) and compares ignoring case with either separator. On Linux `C:\x` and `\\server\x`
//! are relative *names* — they can only ever name a file inside the folder, which the containment
//! test already allows — and names compare byte for byte with `/` as the only separator.

use std::rc::Rc;

use super::file_names;
use crate::seams::{FileIdentity, FileSystem};

/// The `resolve_asset` md-core's `text_bundle::export_rewriting` takes: a relative reference in,
/// the file's bytes out, `None` for "not found" (the reference then stays as written).
pub type AssetResolver = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

/// [`beside_capped`] with no size cap.
pub fn beside(
    document_path: Option<&str>,
    file_system: Rc<dyn FileSystem>,
    identity: Rc<dyn FileIdentity>,
) -> AssetResolver {
    beside_capped(document_path, file_system, identity, u64::MAX)
}

/// The resolver for the document at `document_path`. An unsaved document (`None` or empty)
/// resolves nothing, so it exports with an empty `assets/`. A file longer than `max_bytes` — the
/// EPUB cover's cap — is not found, and is never read. An unreadable file is "not found" too.
pub fn beside_capped(
    document_path: Option<&str>,
    file_system: Rc<dyn FileSystem>,
    identity: Rc<dyn FileIdentity>,
    max_bytes: u64,
) -> AssetResolver {
    let Some(document_path) = document_path.filter(|p| !p.is_empty()) else { return Box::new(|_| None) };
    let folder = file_names::directory_of(document_path).to_string();
    if folder.is_empty() {
        return Box::new(|_| None);
    }
    let canonical_folder = identity.canonical(&folder);

    Box::new(move |relative: &str| {
        let path = resolve(&folder, &canonical_folder, relative, &*identity)?;
        if !file_system.file_exists(&path) {
            return None;
        }
        if file_system.stamp(&path).is_some_and(|stamp| stamp.size > max_bytes) {
            return None;
        }
        // Unreadable is "not found": the reference stays as written.
        file_system.read_all_bytes(&path).ok()
    })
}

/// The path a relative reference names inside `folder`, or `None` when it is rooted, climbs out
/// lexically, or lands outside once links are resolved. Public so the containment rule can be
/// pinned on its own, without a file system.
pub fn resolve(folder: &str, canonical_folder: &str, relative: &str, identity: &dyn FileIdentity) -> Option<String> {
    if relative.is_empty() || file_names::is_rooted(relative) {
        return None;
    }
    let candidate = file_names::resolve_relative(folder, relative)?;
    is_strictly_inside(&identity.canonical(&candidate), canonical_folder).then_some(candidate)
}

/// Whether `path` sits under `folder` and is not the folder itself, compared segment by segment —
/// a plain `starts_with(folder)` would let `/Docs2/x.png` through for `/Docs`, and a doubled
/// separator would defeat `starts_with(folder + "/")`. Byte for byte (see the module comment).
pub fn is_strictly_inside(path: &str, folder: &str) -> bool {
    let inner: Vec<&str> = file_names::split(path).collect();
    let outer: Vec<&str> = file_names::split(folder).collect();
    inner.len() > outer.len() && inner.iter().zip(&outer).all(|(a, b)| a == b)
}
