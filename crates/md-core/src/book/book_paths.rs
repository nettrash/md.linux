//! Path identity for the book model (md.win `Book/BookPaths.cs`).
//!
//! The Swift side compares `standardizedFileURL.path` strings — case-sensitive, symlinks left
//! alone — because every path it sees comes from its own listing. md.win compares
//! case-insensitively on Windows only, because NTFS does.
//!
//! LINUX DECISION: paths compare byte for byte (`==` on the standardized spelling), as on the Mac
//! and as ext4, btrfs and xfs compare names. Nothing in the book module folds case.
//!
//! Paths are UTF-8 `&str`, as md.win's are `string`s: every path the book handles comes from the
//! listing ([`super::book_model::DirectoryBookListing`] skips a name that is not UTF-8, which the
//! Mac's APFS cannot even hold) or from a file chooser. The separator is `/`.

/// The one directory separator.
pub const SEPARATOR: char = '/';

/// `standardizedFileURL.path` (md.win `Path.TrimEndingDirectorySeparator(Path.GetFullPath(path))`):
/// absolute against the current directory, `.` segments and doubled separators dropped, `..`
/// collapsed lexically (never above the root), no trailing separator — so "/x/" and "/x" name one
/// folder. The root keeps its separator: there is no shorter spelling. Symlinks are NOT resolved,
/// exactly as `standardizedFileURL` and `GetFullPath` leave them.
pub fn standardize(path: &str) -> String {
    let absolute;
    let path = if path.starts_with(SEPARATOR) {
        path
    } else {
        absolute = combine(&current_dir(), path);
        &absolute
    };
    let mut components: Vec<&str> = Vec::new();
    for component in path.split(SEPARATOR) {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            name => components.push(name),
        }
    }
    let mut standardized = String::with_capacity(path.len());
    for component in &components {
        standardized.push(SEPARATOR);
        standardized.push_str(component);
    }
    if standardized.is_empty() {
        standardized.push(SEPARATOR);
    }
    standardized
}

/// Same file, after standardizing both sides. Byte-for-byte on Linux (see the module comment).
pub fn same(a: &str, b: &str) -> bool {
    standardize(a) == standardize(b)
}

/// `lastPathComponent` for a file or a folder — a trailing separator on a folder path is not part
/// of its name.
pub fn name(path: &str) -> &str {
    file_name(trim_ending_separator(path))
}

/// `Path.GetFileName`: everything after the last separator (the whole string when there is none).
pub fn file_name(path: &str) -> &str {
    match path.rfind(SEPARATOR) {
        Some(index) => &path[index + 1..],
        None => path,
    }
}

/// `Path.TrimEndingDirectorySeparator`: one trailing separator off, unless the path is the root.
pub fn trim_ending_separator(path: &str) -> &str {
    if path.len() > 1 && path.ends_with(SEPARATOR) { &path[..path.len() - 1] } else { path }
}

/// `Path.GetDirectoryName` on Unix: the path up to its last separator, trailing separators and
/// doubled ones collapsed; `""` for a bare name; `None` for the root or an empty path.
pub fn directory_name(path: &str) -> Option<String> {
    let root = usize::from(path.starts_with(SEPARATOR));
    let bytes = path.as_bytes();
    let mut end = path.len();
    if end <= root {
        return None;
    }
    loop {
        end -= 1;
        if end <= root || bytes[end] == b'/' {
            break;
        }
    }
    while end > root && bytes[end - 1] == b'/' {
        end -= 1;
    }
    let mut directory = String::with_capacity(end);
    let mut previous_was_separator = false;
    for c in path[..end].chars() {
        if c == SEPARATOR && previous_was_separator {
            continue;
        }
        previous_was_separator = c == SEPARATOR;
        directory.push(c);
    }
    Some(directory)
}

/// `Path.Combine(a, b)`: `b` when it is rooted (so a rooted second argument escapes `a` — the
/// callers that join a typed name guard against that), `a` when `b` is empty, else the two joined
/// by exactly one separator.
pub fn combine(a: &str, b: &str) -> String {
    if b.is_empty() {
        return a.to_string();
    }
    if a.is_empty() || b.starts_with(SEPARATOR) {
        return b.to_string();
    }
    if a.ends_with(SEPARATOR) { format!("{a}{b}") } else { format!("{a}{SEPARATOR}{b}") }
}

/// Foundation's `deletingPathExtension().lastPathComponent` for a file name: the text before the
/// last dot, unless that dot is first or last or everything before it is dots — "README",
/// "notes.", ".hidden", "..md" and "...md" keep their whole name (probed against Foundation on
/// macOS; the two dot-only shapes are what a plain "last dot" rule gets wrong).
pub fn deleting_path_extension(file: &str) -> &str {
    let Some(dot) = file.rfind('.') else {
        return file;
    };
    if dot == 0 || dot == file.len() - 1 {
        return file;
    }
    if file.as_bytes()[..dot].iter().all(|&b| b == b'.') { file } else { &file[..dot] }
}

/// Foundation's `URL.pathExtension` for a file name: the text after the last dot when
/// [`deleting_path_extension`] would remove it, except that CFURL refuses an extension containing
/// a space — U+0020 exactly: a TAB, a no-break space or a newline inside the extension is accepted
/// ("a.b c", "a.md " → ""; "a.b\tc" → "b\tc"; probed on macOS) — while `deletingPathExtension`
/// still strips it. The two Foundation calls the rescue-copy name is built from do not agree with
/// each other, and the port mirrors each. Article extensions never contain a space, so this only
/// shapes names the listing would not have offered.
pub fn path_extension(file: &str) -> &str {
    let stem = deleting_path_extension(file);
    if stem.len() == file.len() {
        return "";
    }
    let ext = &file[stem.len() + 1..];
    if ext.contains(' ') { "" } else { ext }
}

/// Swift's `urlPath.hasPrefix(folderPath + "/")`: `standardized_path` lies strictly inside
/// `standardized_folder`. Both must already be standardized. The root folder already ends with
/// its separator; appending another would make every path fail the test.
pub fn is_inside(standardized_path: &str, standardized_folder: &str) -> bool {
    if standardized_folder.ends_with(SEPARATOR) {
        standardized_path.starts_with(standardized_folder)
    } else {
        standardized_path.len() > standardized_folder.len()
            && standardized_path.starts_with(standardized_folder)
            && standardized_path.as_bytes()[standardized_folder.len()] == b'/'
    }
}

/// The process's current directory, the base a relative path is made absolute against (the root
/// when it cannot be read, or is not UTF-8).
fn current_dir() -> String {
    std::env::current_dir().ok().and_then(|dir| dir.into_os_string().into_string().ok()).unwrap_or_else(|| "/".into())
}
