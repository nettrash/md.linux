//! The `privacy` self-test group (shell-design §13.3): after every other group, `OUT/home` holds
//! only `settings.conf`, `session.json`, the drafts and `recently-used.xbel` — nothing under
//! `.cache/md` or `.local/share/md/webkitgtk`.
//!
//! The four are md's whole footprint (§9.2, PRIVACY.md): web data lives in an ephemeral network
//! session, PDF temp files in `$XDG_RUNTIME_DIR` and are deleted after use. Files the system's own
//! libraries keep under `.cache/` (a GL shader cache, fontconfig) are listed in the detail but are
//! not md's; anything else fails the check.

use std::path::{Path, PathBuf};

use crate::selftest::Harness;

/// md's own files, relative to `OUT/home`; the drafts are matched by folder.
const ALLOWED: [&str; 3] =
    [".config/md/settings.conf", ".local/state/md/session.json", ".local/share/recently-used.xbel"];
const DRAFTS: &str = ".local/share/md/drafts";
/// Never: the web engine's stores and an md cache.
const FORBIDDEN: [&str; 5] =
    [".cache/md", ".local/share/md/webkitgtk", ".cache/webkitgtk", ".local/share/webkitgtk", ".config/md/webkitgtk"];

fn files_under(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => files_under(root, &path, out),
            Ok(_) => out.push(path.strip_prefix(root).map(Path::to_path_buf).unwrap_or(path)),
            Err(_) => {}
        }
    }
}

pub async fn run(h: &Harness) {
    let home = h.home();
    let mut files = Vec::new();
    files_under(&home, &home, &mut files);
    files.sort();

    let mut md = Vec::new();
    let mut system = Vec::new();
    let mut unexpected = Vec::new();
    for file in &files {
        let text = file.to_string_lossy().into_owned();
        let draft = file.parent() == Some(Path::new(DRAFTS)) && file.extension().is_some_and(|e| e == "md");
        if FORBIDDEN.iter().any(|f| file.starts_with(f)) {
            unexpected.push(text);
        } else if ALLOWED.contains(&text.as_str()) || draft {
            md.push(text);
        } else if file.starts_with(".cache") {
            system.push(text);
        } else {
            unexpected.push(text);
        }
    }
    // System caches by folder: `.cache/mesa_shader_cache (38 files)`.
    let mut folders: Vec<(String, usize)> = Vec::new();
    for file in &system {
        let folder = file.split('/').take(2).collect::<Vec<_>>().join("/");
        match folders.iter_mut().find(|(f, _)| *f == folder) {
            Some((_, count)) => *count += 1,
            None => folders.push((folder, 1)),
        }
    }
    let system: Vec<String> = folders.iter().map(|(f, n)| format!("{f} ({n} files)")).collect();
    let forbidden_dirs: Vec<&str> = FORBIDDEN.iter().copied().filter(|d| home.join(d).exists()).collect();
    h.check(
        "only_mds_own_files",
        unexpected.is_empty() && forbidden_dirs.is_empty(),
        format!(
            "md: [{}]; system caches: [{}]; unexpected: [{}]{}",
            md.join(", "),
            system.join(", "),
            unexpected.join(", "),
            if forbidden_dirs.is_empty() { String::new() } else { format!("; present: {}", forbidden_dirs.join(", ")) }
        ),
    );

    let runtime = h.shell().paths.runtime_dir().to_path_buf();
    let mut leftovers = Vec::new();
    files_under(&runtime, &runtime, &mut leftovers);
    h.check(
        "no_pdf_temp_files_left",
        leftovers.is_empty(),
        format!("{}: {} file(s)", runtime.display(), leftovers.len()),
    );
}
