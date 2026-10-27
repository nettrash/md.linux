//! Where md reads its bundled data and keeps its state (shell-design §9.3, §9.2).
//!
//! Install data: `$MD_DATA_DIR`, else `/usr/share/md`, else — debug builds only — the source tree,
//! so `scripts/dev.sh run` works from a checkout. The per-user folders come from GLib
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
    /// The folders of this process: `$MD_DATA_DIR` and the XDG variables as GLib reads them.
    pub fn new() -> Paths {
        Paths {
            data_dir: data_dir(std::env::var_os("MD_DATA_DIR").map(PathBuf::from)),
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

/// `$MD_DATA_DIR` when set and not empty; else the installed folder when it exists; else, in a
/// debug build, the source tree (the workspace root holds `rich/` and `examples/`); else the
/// installed folder anyway, so a broken install reports the missing files by their real path.
fn data_dir(overridden: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = overridden.filter(|dir| !dir.as_os_str().is_empty()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_data_dir_wins_and_an_empty_one_does_not() {
        assert_eq!(data_dir(Some(PathBuf::from("/opt/md"))), PathBuf::from("/opt/md"));
        let fallback = data_dir(Some(PathBuf::new()));
        assert_eq!(fallback, data_dir(None));
    }

    #[test]
    fn a_debug_build_without_an_install_finds_the_source_tree() {
        if Path::new(INSTALLED_DATA_DIR).is_dir() {
            return; // an installed md is preferred; nothing to check here
        }
        let dir = data_dir(None);
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
}
