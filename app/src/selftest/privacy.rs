//! The `privacy` self-test group (shell-design §13.3): after every other group, `OUT/home` holds
//! only `settings.conf`, `session.json`, the drafts and `recently-used.xbel` — nothing under
//! `.cache/md` or `.local/share/md/webkitgtk`.
//!
//! The four are md's whole footprint (§9.2, PRIVACY.md): web data lives in an ephemeral network
//! session, PDF temp files in `$XDG_RUNTIME_DIR` and are deleted after use. Files the system's own
//! libraries keep under `.cache/` (a GL shader cache, fontconfig) are listed in the detail but are
//! not md's. One desktop file is allowed by name: `.config/dconf/user`, the desktop's settings
//! database, where GTK's file chooser keeps its own preferences (GSettings
//! `org.gtk.gtk4.Settings.FileChooser`: its size, sort order, hidden files…) whenever md shows it —
//! md's code never writes it, and the check proves the database holds nothing but that schema's
//! keys. PRIVACY.md says so. Anything else fails the check.

use std::path::{Path, PathBuf};

use crate::selftest::Harness;

/// md's own files, relative to `OUT/home`; the drafts are matched by folder.
const ALLOWED: [&str; 3] =
    [".config/md/settings.conf", ".local/state/md/session.json", ".local/share/recently-used.xbel"];
const DRAFTS: &str = ".local/share/md/drafts";
/// The desktop's settings database (dconf), which GTK's file chooser writes, not md.
const DESKTOP_SETTINGS: &str = ".config/dconf/user";
/// The only dconf folders the database may hold: GTK 4's file chooser's settings path,
/// `/org/gtk/gtk4/settings/file-chooser/`.
const FILE_CHOOSER_PATH: [&str; 5] = ["org/", "gtk/", "gtk4/", "settings/", "file-chooser/"];

/// The folder names in a dconf (GVDB) database: every run of `[a-z0-9-]` closed by a `/`. GVDB
/// stores each path component as its own key, so a folder of any other application shows up here.
fn dconf_folders(bytes: &[u8]) -> Vec<String> {
    let mut folders = Vec::new();
    let mut run = String::new();
    for &b in bytes {
        match b {
            b'a'..=b'z' | b'0'..=b'9' | b'-' => run.push(char::from(b)),
            b'/' if !run.is_empty() => {
                run.push('/');
                if !folders.contains(&run) {
                    folders.push(run.clone());
                }
                run.clear();
            }
            _ => run.clear(),
        }
    }
    folders
}

/// Whether every folder of the database is on the file chooser's path. A GVariant type letter
/// may precede a key in the file (`sgtk/`), so a folder matches by its end; the root folder `/`
/// therefore reads as that one letter and a slash (`s/`).
fn only_file_chooser_settings(folders: &[String]) -> bool {
    let allowed = |f: &String| f.len() == 2 || FILE_CHOOSER_PATH.iter().any(|a| f == a || f.ends_with(a));
    folders.iter().any(|f| f.ends_with("file-chooser/")) && folders.iter().all(allowed)
}
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
    let mut desktop = Vec::new();
    let mut unexpected = Vec::new();
    for file in &files {
        let text = file.to_string_lossy().into_owned();
        let draft = file.parent() == Some(Path::new(DRAFTS)) && file.extension().is_some_and(|e| e == "md");
        if FORBIDDEN.iter().any(|f| file.starts_with(f)) {
            unexpected.push(text);
        } else if ALLOWED.contains(&text.as_str()) || draft {
            md.push(text);
        } else if text == DESKTOP_SETTINGS {
            let folders = dconf_folders(&std::fs::read(home.join(file)).unwrap_or_default());
            if only_file_chooser_settings(&folders) {
                desktop.push(format!("{text} (GTK's file chooser: {})", folders.join(" ")));
            } else {
                unexpected.push(format!("{text} with {}", folders.join(" ")));
            }
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
            "md: [{}]; system caches: [{}]; desktop: [{}]; unexpected: [{}]{}",
            md.join(", "),
            system.join(", "),
            desktop.join(", "),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_chooser_database_passes_and_anything_else_does_not() {
        // What GTK 4.22's chooser wrote under Xvfb, as `strings` shows the GVDB file.
        let written = b"GVariant\0type-format\0sgtk/\0s/\0view-type\0sorg/\0(ii)file-chooser/\0gtk4/\0settings/\0";
        let folders = dconf_folders(written);
        assert_eq!(folders, ["sgtk/", "s/", "sorg/", "file-chooser/", "gtk4/", "settings/"]);
        assert!(only_file_chooser_settings(&folders));
        let other = dconf_folders(b"sorg/\0gtk/\0gtk4/\0settings/\0file-chooser/\0md/\0");
        assert!(!only_file_chooser_settings(&other));
        assert!(!only_file_chooser_settings(&dconf_folders(b"org/\0gnome/\0")));
        assert!(!only_file_chooser_settings(&[]));
    }
}
