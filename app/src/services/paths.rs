//! Where md reads its bundled data and keeps its state (shell-design §9.3, §9.2).
//!
//! Install data: `$MD_DATA_DIR`; else the folder beside the executable — `<prefix>/share/md` for
//! `<prefix>/bin/md`, or the checkout for a binary cargo built in it (`target/release/md`); else
//! `/usr/share/md`; else — debug builds only — the source tree, so `scripts/dev.sh run` works from
//! a checkout whose target directory is elsewhere. The per-user folders come from GLib
//! (`glib::user_config_dir()` and friends, which honour the XDG variables) and are created on first
//! write, never here.

use std::path::{Path, PathBuf};

use md_logic::settings::settings_keys;

/// md's folder under each XDG base directory.
const APP_FOLDER: &str = "md";
/// The settings file under `$XDG_CONFIG_HOME/md` (§9.1).
const SETTINGS_FILE_NAME: &str = "settings.conf";
/// The installed data folder.
const INSTALLED_DATA_DIR: &str = "/usr/share/md";

/// The install folder and the per-user folders, resolved once in `startup`.
#[derive(Clone, Debug)]
pub struct Paths {
    data_dir: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    user_data_dir: PathBuf,
    runtime_dir: PathBuf,
}

impl Paths {
    /// The folders of this process: `$MD_DATA_DIR`, the executable's place, and the XDG variables
    /// as GLib reads them.
    pub fn new() -> Paths {
        let exe = std::env::current_exe().ok();
        Paths {
            data_dir: data_dir(std::env::var_os("MD_DATA_DIR").map(PathBuf::from), exe.as_deref()),
            config_dir: glib::user_config_dir().join(APP_FOLDER),
            state_dir: glib::user_state_dir().join(APP_FOLDER),
            user_data_dir: glib::user_data_dir().join(APP_FOLDER),
            runtime_dir: glib::user_runtime_dir().join(APP_FOLDER),
        }
    }

    /// The bundled engines (`rich/`): KaTeX, Mermaid, Viz.js, PlantUML, highlight.js, md-init.js.
    pub fn rich_dir(&self) -> PathBuf {
        self.data_dir.join("rich")
    }

    /// The bundled examples (`examples/`, with `Example Book/` inside).
    pub fn examples_dir(&self) -> PathBuf {
        self.data_dir.join("examples")
    }

    /// `$XDG_CONFIG_HOME/md/settings.conf` (§9.1).
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join(SETTINGS_FILE_NAME)
    }

    /// `$XDG_STATE_HOME/md/session.json` (§1.6).
    pub fn session_file(&self) -> PathBuf {
        self.state_dir.join(settings_keys::SESSION_FILE_NAME)
    }

    /// `$XDG_DATA_HOME/md/drafts` — untitled documents' text (§1.6; created `0700` by its owner).
    pub fn drafts_dir(&self) -> PathBuf {
        self.user_data_dir.join("drafts")
    }

    /// `$XDG_RUNTIME_DIR/md` — the PDF temp files (§7.3), deleted after use.
    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }
}

/// `$MD_DATA_DIR` when set and not empty; else the data beside the executable `exe`
/// ([`beside_executable`]); else the installed folder when it exists; else, in a debug build, the
/// source tree (the workspace root holds `rich/` and `examples/`); else the installed folder
/// anyway, so a broken install reports the missing files by their real path.
fn data_dir(overridden: Option<PathBuf>, exe: Option<&Path>) -> PathBuf {
    if let Some(dir) = overridden.filter(|dir| !dir.as_os_str().is_empty()) {
        return dir;
    }
    if let Some(dir) = exe.and_then(beside_executable) {
        return dir;
    }
    let installed = Path::new(INSTALLED_DATA_DIR);
    if installed.is_dir() {
        return installed.to_path_buf();
    }
    // `#[cfg]`, not `cfg!`: a release binary must not carry the build machine's path.
    #[cfg(debug_assertions)]
    {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }
    #[cfg(not(debug_assertions))]
    {
        installed.to_path_buf()
    }
}

/// The data an executable at `exe` carries with it: `<prefix>/share/md` for `<prefix>/bin/md`
/// (`/usr/bin/md` finds `/usr/share/md`), else the checkout for `<checkout>/target/<profile>/md`,
/// so a release build run from the tree finds its examples and engines. Each only when it holds
/// both; `None` otherwise.
fn beside_executable(exe: &Path) -> Option<PathBuf> {
    let holds_data = |dir: &Path| dir.join("rich").join("md-init.js").is_file() && dir.join("examples").is_dir();
    let prefix = exe.parent()?.parent()?;
    let shared = prefix.join("share").join(APP_FOLDER);
    if holds_data(&shared) {
        return Some(shared);
    }
    let checkout = prefix.parent()?;
    holds_data(checkout).then(|| checkout.to_path_buf())
}

/// `folder` with the home folder written `~` — how the header bar's subtitle and Open Recent's
/// rows name a folder (shell-design §1.4, §6.6). Pure: `home` is passed in.
pub fn tilde_folder(folder: &str, home: &str) -> String {
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return folder.to_string();
    }
    if folder == home {
        return "~".to_string();
    }
    match folder.strip_prefix(home) {
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => folder.to_string(),
    }
}

/// [`tilde_folder`] under this user's home folder.
pub fn display_folder(folder: &str) -> String {
    tilde_folder(folder, &glib::home_dir().to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder under the temp directory, removed on drop.
    struct TempFolder(PathBuf);

    impl TempFolder {
        fn new(tag: &str) -> TempFolder {
            let path = std::env::temp_dir().join(format!("md-paths-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            TempFolder(path)
        }

        /// `rich/md-init.js` and `examples/` under `dir`.
        fn with_data(&self, dir: &str) -> PathBuf {
            let dir = self.0.join(dir);
            std::fs::create_dir_all(dir.join("rich")).unwrap();
            std::fs::create_dir_all(dir.join("examples")).unwrap();
            std::fs::write(dir.join("rich/md-init.js"), "").unwrap();
            dir
        }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn md_data_dir_wins_and_an_empty_one_does_not() {
        let temp = TempFolder::new("override");
        let exe = temp.with_data("checkout").join("target/release/md");
        assert_eq!(data_dir(Some(PathBuf::from("/opt/md")), Some(&exe)), PathBuf::from("/opt/md"));
        let fallback = data_dir(Some(PathBuf::new()), None);
        assert_eq!(fallback, data_dir(None, None));
    }

    #[test]
    fn a_release_build_in_a_checkout_finds_the_checkout() {
        let temp = TempFolder::new("checkout");
        let checkout = temp.with_data("md.linux");
        assert_eq!(data_dir(None, Some(&checkout.join("target/release/md"))), checkout);
        assert_eq!(data_dir(None, Some(&checkout.join("target/debug/md"))), checkout);
    }

    #[test]
    fn a_prefix_install_finds_its_share_folder() {
        let temp = TempFolder::new("prefix");
        let shared = temp.with_data("opt/share/md");
        assert_eq!(beside_executable(&temp.0.join("opt/bin/md")), Some(shared));
    }

    #[test]
    fn a_folder_without_both_parts_is_not_data() {
        let temp = TempFolder::new("partial");
        std::fs::create_dir_all(temp.0.join("tree/examples")).unwrap();
        assert_eq!(beside_executable(&temp.0.join("tree/target/release/md")), None);
        std::fs::create_dir_all(temp.0.join("tree/rich")).unwrap();
        assert_eq!(beside_executable(&temp.0.join("tree/target/release/md")), None);
        std::fs::write(temp.0.join("tree/rich/md-init.js"), "").unwrap();
        assert_eq!(beside_executable(&temp.0.join("tree/target/release/md")), Some(temp.0.join("tree")));
        assert_eq!(beside_executable(Path::new("/md")), None);
        assert_eq!(beside_executable(Path::new("md")), None);
    }

    #[test]
    fn a_debug_build_without_an_install_finds_the_source_tree() {
        if Path::new(INSTALLED_DATA_DIR).is_dir() {
            return; // an installed md is preferred; nothing to check here
        }
        let dir = data_dir(None, None);
        assert!(dir.join("rich/md-init.js").is_file(), "{}", dir.display());
        assert!(dir.join("examples").is_dir(), "{}", dir.display());
    }

    #[test]
    fn the_state_files_sit_in_their_xdg_folders() {
        let paths = Paths {
            data_dir: PathBuf::from("/usr/share/md"),
            config_dir: PathBuf::from("/h/.config/md"),
            state_dir: PathBuf::from("/h/.local/state/md"),
            user_data_dir: PathBuf::from("/h/.local/share/md"),
            runtime_dir: PathBuf::from("/run/user/1000/md"),
        };
        assert_eq!(paths.rich_dir(), PathBuf::from("/usr/share/md/rich"));
        assert_eq!(paths.examples_dir(), PathBuf::from("/usr/share/md/examples"));
        assert_eq!(paths.settings_file(), PathBuf::from("/h/.config/md/settings.conf"));
        assert_eq!(paths.session_file(), PathBuf::from("/h/.local/state/md/session.json"));
        assert_eq!(paths.drafts_dir(), PathBuf::from("/h/.local/share/md/drafts"));
        assert_eq!(paths.runtime_dir(), Path::new("/run/user/1000/md"));
    }

    #[test]
    fn folders_under_home_are_written_with_a_tilde() {
        const HOME: &str = "/home/me";
        assert_eq!(tilde_folder("/home/me", HOME), "~");
        assert_eq!(tilde_folder("/home/me", "/home/me/"), "~");
        assert_eq!(tilde_folder("/home/me/Documents/Книги", HOME), "~/Documents/Книги");
        assert_eq!(tilde_folder("/home/me/x", "/home/me/"), "~/x");
        assert_eq!(tilde_folder("/home/meadow", HOME), "/home/meadow");
        assert_eq!(tilde_folder("/srv/Book", HOME), "/srv/Book");
        assert_eq!(tilde_folder("/srv/Book", ""), "/srv/Book");
        let home = glib::home_dir().to_string_lossy().into_owned();
        assert_eq!(display_folder(&format!("{home}/notes")), tilde_folder(&format!("{home}/notes"), &home));
    }
}
