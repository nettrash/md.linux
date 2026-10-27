//! Port of md.win `Documents/FileNames.cs`: the Rename name rule (§6.4) and the path arithmetic
//! that goes with it.
//!
//! md.win's helpers split on both separators and fold case, because the same suite runs on Windows
//! and on a Linux CI leg and every path a test feeds it is a Windows path.
//!
//! LINUX DECISIONS:
//! - `/` is the only separator. A `\` is an ordinary character in a Linux file name, so
//!   `images\logo.png` names one file, never `images/logo.png`.
//! - Names compare byte for byte ([`same_path`]): ext4, btrfs and xfs are case-sensitive, and so is
//!   the Mac's `standardizedFileURL` comparison.
//! - The name rule ([`validate`]) is the Mac's, as md-core's `book_folder::is_valid_name` applies
//!   it to the book prompts: `/` and `:` (and NUL, `.`, `..`) are refused — not Win32's nine
//!   characters, control characters, trailing dot or space, or the device names `CON`…`LPT9`,
//!   which are ordinary names on Linux and on the Mac.
//! - A rooted reference is one that starts with `/`: there are no drive letters or UNC heads, and a
//!   `C:\x` is a relative name.

use md_core::book::{book_folder, book_paths};
use md_core::text::unicode;

const SEPARATOR: char = '/';

/// The characters [`validate`] refuses that a writer can type — the two the dialog's message
/// (`strings::documents::INVALID_NAME_MESSAGE`) names. NUL is refused too, but no prompt can
/// carry one.
pub const INVALID_CHARACTERS: &str = "/:";

/// Whether the Rename dialog may accept `name`: not missing, empty or blank (every character
/// .NET's `char.IsWhiteSpace` — the prompt trims, so a blank name would rename to nothing), and
/// legal by `book_folder::is_valid_name` — no `/`, `:` or NUL, and neither `.` nor `..`. The
/// message for a rejection is `strings::documents::INVALID_NAME_MESSAGE`.
pub fn validate(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    if is_blank(name) {
        return false;
    }
    book_folder::is_valid_name(name)
}

/// A name safe to hand a save dialog: every character [`validate`] refuses (`/`, `:`, NUL)
/// becomes `-`, a blank name and the two names every folder already holds (`.` and `..`) become
/// `"Document"`. md.win's `ForWindows`; md-core's `export_file_names::sanitized` is the shared
/// export rule, this is only what a Linux file name cannot hold plus the Mac's `:`.
///
/// LINUX DECISION: md.win also trims trailing dots and spaces and prefixes a reserved device name
/// with `_`; neither is a rule on Linux, so `"trailing. "`, `"CON"` and `"..."` keep their names.
pub fn for_linux(name: Option<&str>) -> String {
    const FALLBACK: &str = "Document";
    let Some(name) = name else { return FALLBACK.to_string() };
    if is_blank(name) {
        return FALLBACK.to_string();
    }
    let replaced: String = name.chars().map(|c| if matches!(c, '/' | ':' | '\0') { '-' } else { c }).collect();
    if replaced == "." || replaced == ".." { FALLBACK.to_string() } else { replaced }
}

/// Rename keeps the extension (§6.4): the new stem with the old file's extension re-attached.
/// Foundation's split, so `".hidden"` and `"notes."` keep their whole name as the stem and gain no
/// extension — the same rule the rescue-copy name is built from.
pub fn with_extension_of(original_file_name: &str, new_stem: &str) -> String {
    let ext = book_paths::path_extension(name_of(original_file_name));
    if ext.is_empty() { new_stem.to_string() } else { format!("{new_stem}.{ext}") }
}

// ---- path arithmetic ----

/// The last `/`-separated component; a trailing separator is not part of a name.
pub fn name_of(path: &str) -> &str {
    let head = path.trim_end_matches(SEPARATOR);
    match head.rfind(SEPARATOR) {
        Some(cut) => &head[cut + 1..],
        None => head,
    }
}

/// Everything before [`name_of`], without its trailing separator (the root keeps its one). Empty
/// when the path has no directory part.
pub fn directory_of(path: &str) -> &str {
    let head = path.trim_end_matches(SEPARATOR);
    match head.rfind(SEPARATOR) {
        None => "",
        Some(0) => &path[..1],
        Some(cut) => &path[..cut],
    }
}

/// The file's stem, Foundation's way (`README`, `notes.`, `.hidden` keep their whole name).
pub fn stem_of(path: &str) -> &str {
    book_paths::deleting_path_extension(name_of(path))
}

/// The extension with its dot, or `""` — the `Path.GetExtension` shape the rescue name wants.
pub fn extension_of(path: &str) -> String {
    let ext = book_paths::path_extension(name_of(path));
    if ext.is_empty() { String::new() } else { format!(".{ext}") }
}

/// Join with exactly one separator (`name` when there is no directory).
pub fn combine(directory: &str, name: &str) -> String {
    if directory.is_empty() {
        return name.to_string();
    }
    if directory.ends_with(SEPARATOR) { format!("{directory}{name}") } else { format!("{directory}/{name}") }
}

/// Whether a reference is rooted rather than relative: on Linux, a leading `/`.
pub fn is_rooted(reference: &str) -> bool {
    reference.starts_with(SEPARATOR)
}

/// `relative` resolved against `directory` with `.` and `..` collapsed. `None` when the reference
/// is rooted or climbs out of the directory's root: neither can be an asset beside the document.
pub fn resolve_relative(directory: &str, relative: &str) -> Option<String> {
    if relative.is_empty() || is_rooted(relative) {
        return None;
    }
    let root = root_of(directory);
    let mut segments: Vec<&str> = split(&directory[root.len()..]).collect();
    for part in split(relative) {
        match part {
            "." => {}
            ".." => {
                // Out of the root: not ours to read.
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    if segments.is_empty() {
        return None;
    }
    Some(format!("{root}{}", segments.join("/")))
}

/// `standardizedFileURL.path` for a rooted path: dot segments and doubled separators collapsed,
/// no trailing separator. A relative path is returned unchanged — the app only ever holds absolute
/// paths (a picker, an activation, a book listing), and inventing a working directory for one
/// would be a guess.
pub fn standardize(path: &str) -> String {
    let root = root_of(path);
    if root.is_empty() {
        return path.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    for part in split(&path[root.len()..]) {
        match part {
            "." => {}
            // ".." at the root is the root, as GetFullPath and realpath have it.
            ".." => {
                kept.pop();
            }
            name => kept.push(name),
        }
    }
    format!("{root}{}", kept.join("/"))
}

/// Whether two paths name one file, lexically: standardized, then compared segment by segment,
/// byte for byte.
///
/// LINUX DECISION: md.win compares `OrdinalIgnoreCase` with `/` and `\` as one separator —
/// Windows' rule on every OS. Here case matters and `\` is a character. The root is compared too,
/// so a relative spelling never equals an absolute one (md.win's segment split let `a` equal `/a`).
pub fn same_path(a: &str, b: &str) -> bool {
    let (a, b) = (standardize(a), standardize(b));
    root_of(&a) == root_of(&b) && split(&a).eq(split(&b))
}

/// The un-removable head of a path: `"/"`, or `""` for a relative one.
pub(crate) fn root_of(path: &str) -> &str {
    if path.starts_with(SEPARATOR) { &path[..1] } else { "" }
}

/// The non-empty `/`-separated segments.
pub(crate) fn split(path: &str) -> impl Iterator<Item = &str> {
    path.split(SEPARATOR).filter(|part| !part.is_empty())
}

/// .NET's `string.IsNullOrWhiteSpace` for a present string: empty, or every character
/// `char.IsWhiteSpace`.
pub(crate) fn is_blank(text: &str) -> bool {
    text.chars().all(unicode::is_dotnet_white_space)
}
