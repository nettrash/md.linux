//! `session.json` at `$XDG_STATE_HOME/md/` through md-logic's `windows::SessionStore`
//! (shell-design §1.6): sizes, maximized, mode, Zen — no positions; drafts named by path; written
//! on every close, on Quit and at `query-end`, read on the first activation.
//!
//! The file is the window manager's own state, so it is read and written with md-logic's
//! [`LocalFileSystem`] (the same `std::fs` semantics as the documents' `StdFileSystem`). Which rows
//! a launch brings back is the pure [`restore_plan`], unit-tested with a temp drafts folder: saved
//! documents that still exist, then the drafts the session names, then the drafts it does not name
//! (a crash between a draft write and the session write must never strand text).

use std::path::PathBuf;

use md_logic::documents::LocalFileSystem;
use md_logic::seams::FileSystem;
use md_logic::windows::{SessionState, SessionWindow, session_store};

use crate::windows::drafts::Drafts;

/// `session.json` of this user.
#[derive(Clone, Debug)]
pub struct SessionFile {
    path: PathBuf,
}

impl SessionFile {
    /// `path` is `Paths::session_file()`.
    pub fn new(path: PathBuf) -> SessionFile {
        SessionFile { path }
    }

    #[cfg(test)]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn folder(&self) -> String {
        self.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| String::from("."))
    }

    /// The stored session; empty when the file is missing or unreadable.
    pub fn load(&self) -> SessionState {
        session_store::load(&LocalFileSystem, &self.folder())
    }

    /// Writes `state`, creating the folder on the first write. A session that cannot be recorded is
    /// a lost layout, never a blocked close: the failure is reported on stderr only.
    pub fn save(&self, state: &SessionState) {
        let folder = self.folder();
        if let Err(err) = LocalFileSystem.ensure_directory(&folder) {
            eprintln!("md: cannot keep the session: {err}");
            return;
        }
        if !session_store::save(&LocalFileSystem, &folder, state) {
            eprintln!("md: could not write {}", self.path.display());
        }
    }
}

/// One window a launch brings back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Restore {
    /// A saved document, with its mode, Zen and size.
    Document(SessionWindow),
    /// An untitled document's draft the session names, with its mode, Zen and size.
    Draft(SessionWindow),
    /// A draft no session names: restored dirty, its mode decided by the open rule.
    Orphan(PathBuf),
}

impl Restore {
    /// The file the row restores from.
    pub fn path(&self) -> PathBuf {
        match self {
            Restore::Document(window) | Restore::Draft(window) => PathBuf::from(&window.path),
            Restore::Orphan(path) => path.clone(),
        }
    }
}

/// What a launch restores, in order (§1.6, §1.3): the session's rows whose files exist — saved
/// documents unless `drafts_only` (an `open` as the first activation brings back the drafts only,
/// so unsaved text is never left behind because the user double-clicked another file) — then every
/// draft the session does not name.
pub fn restore_plan(state: &SessionState, drafts: &Drafts, drafts_only: bool) -> Vec<Restore> {
    restore_plan_with(state, drafts, drafts_only, &LocalFileSystem)
}

fn restore_plan_with(state: &SessionState, drafts: &Drafts, drafts_only: bool, fs: &dyn FileSystem) -> Vec<Restore> {
    let mut plan = Vec::new();
    let mut named: Vec<PathBuf> = Vec::new();
    for window in &session_store::restorable(state, fs).windows {
        let path = PathBuf::from(&window.path);
        if drafts.contains(&path) {
            if !named.contains(&path) {
                named.push(path);
                plan.push(Restore::Draft(window.clone()));
            }
        } else if !drafts_only {
            plan.push(Restore::Document(window.clone()));
        }
    }
    plan.extend(drafts.orphans(&named).into_iter().map(Restore::Orphan));
    plan
}

#[cfg(test)]
mod tests {
    use md_core::document::view_mode::ViewMode;
    use md_logic::windows::{SessionBook, WindowRect};

    use std::path::Path;

    use super::*;
    use crate::windows::drafts::tests::TempDir;

    fn row(path: &Path, mode: ViewMode, zen: bool) -> SessionWindow {
        SessionWindow::new(&path.to_string_lossy(), mode, zen, false, WindowRect::new(0, 0, 900, 640), false)
    }

    #[test]
    fn the_file_round_trips_drafts_included_and_creates_its_folder() {
        let temp = TempDir::new("session");
        let file = SessionFile::new(temp.0.join("state/md/session.json"));
        assert_eq!(file.load(), SessionState::EMPTY);
        let drafts = Drafts::new(temp.0.join("share/md/drafts"));
        let draft = drafts.new_path();
        let state = SessionState::new(
            vec![row(&temp.0.join("notes.md"), ViewMode::Preview, true), row(&draft, ViewMode::Edit, false)],
            Some(SessionBook::new(true, WindowRect::new(0, 0, 1000, 700), true)),
        );
        file.save(&state);
        assert!(file.path().is_file());
        assert_eq!(file.load(), state);
        let json = std::fs::read_to_string(file.path()).unwrap();
        assert!(json.contains("\"x\":0,\"y\":0,\"w\":900,\"h\":640"), "{json}");
    }

    #[test]
    fn the_plan_restores_existing_documents_then_named_drafts_then_orphans() {
        let temp = TempDir::new("session");
        let drafts = Drafts::new(temp.0.join("drafts"));
        let saved = temp.0.join("saved.md");
        std::fs::write(&saved, "saved").unwrap();
        let gone = temp.0.join("gone.md");
        let named = drafts.new_path();
        drafts.write(&named, "named draft").unwrap();
        let vanished_draft = drafts.new_path();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let orphan = drafts.new_path();
        drafts.write(&orphan, "orphan").unwrap();

        let state = SessionState::new(
            vec![
                row(&saved, ViewMode::Preview, true),
                row(&gone, ViewMode::Split, false),
                row(&named, ViewMode::Edit, false),
                row(&vanished_draft, ViewMode::Edit, false),
            ],
            None,
        );
        let plan = restore_plan(&state, &drafts, false);
        assert_eq!(
            plan,
            vec![
                Restore::Document(row(&saved, ViewMode::Preview, true)),
                Restore::Draft(row(&named, ViewMode::Edit, false)),
                Restore::Orphan(orphan.clone()),
            ]
        );
        assert_eq!(plan[0].path(), saved);

        let drafts_only = restore_plan(&state, &drafts, true);
        assert_eq!(drafts_only, vec![Restore::Draft(row(&named, ViewMode::Edit, false)), Restore::Orphan(orphan)]);
    }

    #[test]
    fn nothing_to_restore_is_an_empty_plan() {
        let temp = TempDir::new("session");
        let drafts = Drafts::new(temp.0.join("drafts"));
        assert!(restore_plan(&SessionState::EMPTY, &drafts, false).is_empty());
        let state = SessionState::new(vec![row(&temp.0.join("missing.md"), ViewMode::Split, false)], None);
        assert!(restore_plan(&state, &drafts, false).is_empty());
    }

    #[test]
    fn a_draft_named_twice_comes_back_once() {
        let temp = TempDir::new("session");
        let drafts = Drafts::new(temp.0.join("drafts"));
        let draft = drafts.new_path();
        drafts.write(&draft, "x").unwrap();
        let state =
            SessionState::new(vec![row(&draft, ViewMode::Edit, false), row(&draft, ViewMode::Split, false)], None);
        assert_eq!(restore_plan(&state, &drafts, false), vec![Restore::Draft(row(&draft, ViewMode::Edit, false))]);
    }
}
