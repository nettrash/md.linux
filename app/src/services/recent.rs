//! `RecentFiles` — File ▸ Open Recent over `gtk::RecentManager` (shell-design §6.6): the desktop's
//! shared `recently-used.xbel`, md's ten newest items newer than `md.linux.recentClearedAt`, Clear
//! Menu hides rather than deletes. The filter is pure and unit-tested over plain records.
//!
//! - **Add** (open, Save As, Rename, Move To — the session's `identity_changed`): `add_full` with
//!   application `"md"`, exec `"md %f"`, the file's MIME type and name. GTK registers md on the
//!   item (or bumps md's stamp on it) and writes the XBEL a moment later; GNOME Files shows the
//!   same list under Recent. Untitled documents and bundle imports are never added (the callers'
//!   rule: there is no file to come back to).
//! - **List**: the items md registered (`has_application("md")`), whose md stamp — the time md
//!   last registered them, not another application's — is newer than the Clear Menu cutoff, whose
//!   local file still exists, newest first, at most [`MAX_ENTRIES`] (NSDocumentController's ten).
//!   A file that is missing right now (an unplugged drive) is hidden, not forgotten.
//! - **Rows**: [`RecentEntry::name`] is the row's whole title — the file name, or, when two rows
//!   share one, `"notes.md — ~/a"` (a `GMenu` row has no tooltip to tell them apart);
//!   [`RecentEntry::folder`] is the folder with the home folder written `~`; the token is the URI.
//! - **Resolve** (a row clicked): the path, or — the file is gone — the item is removed from the
//!   shared list (it no longer exists anywhere) and `None` tells the caller to alert
//!   `strings::documents::file_not_found(name)`.
//! - **Clear Menu** writes the cutoff, `md.linux.recentClearedAt`, through md-logic's
//!   `settings::recent_cleared_at` (whole seconds since the epoch; an entry is listed when md's
//!   stamp is strictly later, `recent_cleared_at::is_listed`): md's menu empties while every other
//!   application keeps its history. A file md registers within the same second as the clear stays
//!   hidden until it is opened again — the price of a whole-second cutoff.

use std::rc::Rc;

use gtk::prelude::*;
use md_logic::commands::RecentEntry;
use md_logic::documents::file_names;
use md_logic::event::Event;
use md_logic::settings::SettingsStore;
use md_logic::settings::recent_cleared_at;

use crate::services::paths::tilde_folder;

/// The application name md registers items under, and the command line Files would reopen them
/// with (§6.6).
const APP_NAME: &str = "md";
const APP_EXEC: &str = "md %f";

/// NSDocumentController's default, the Mac's Open Recent length.
pub const MAX_ENTRIES: usize = 10;

/// Between a file name and its folder when two rows share a name (`notes.md — ~/a`).
const NAME_FOLDER_SEPARATOR: &str = " \u{2014} ";

/// The fallback MIME type for a file the shared-mime-info database does not know.
const FALLBACK_MIME: &str = "text/plain";

/// md's view of the desktop's recent list.
pub struct RecentFiles {
    manager: gtk::RecentManager,
    /// For `md.linux.recentClearedAt`.
    settings: Rc<dyn SettingsStore>,
    changed: Rc<Event<()>>,
}

impl RecentFiles {
    pub fn new(settings: Rc<dyn SettingsStore>) -> Rc<RecentFiles> {
        let manager = gtk::RecentManager::default();
        let changed: Rc<Event<()>> = Rc::new(Event::new());
        {
            // GTK's own `changed`: our adds and removes once written, and every other
            // application's (GTK monitors the file).
            let changed = Rc::downgrade(&changed);
            manager.connect_changed(move |_| {
                if let Some(changed) = changed.upgrade() {
                    changed.emit(&());
                }
            });
        }
        Rc::new(RecentFiles { manager, settings, changed })
    }

    /// At most ten, newest first.
    pub fn entries(&self) -> Vec<RecentEntry> {
        let cutoff = recent_cleared_at::read(&*self.settings);
        let home = glib::home_dir().into_os_string().into_string().unwrap_or_default();
        let records = self.manager.items().iter().map(record_of).collect();
        entries(records, cutoff, &home)
    }

    /// Registers `path` as opened by md now. Ignored for anything but an absolute path.
    pub fn add(&self, path: &str) {
        if !file_names::is_rooted(path) {
            return;
        }
        let file = gio::File::for_path(path);
        let (content_type, _) = gio::content_type_guess(Some(path), None::<&[u8]>);
        let mime = gio::content_type_get_mime_type(&content_type);
        let mime = mime.as_deref().unwrap_or(FALLBACK_MIME);
        let name = file_names::name_of(path);
        let data = gtk::RecentData::new(Some(name), None, mime, APP_NAME, APP_EXEC, &[], false);
        if !self.manager.add_full(&file.uri(), &data) {
            eprintln!("md: {path} could not be added to the recent documents");
        }
    }

    /// Clear Menu: hides every current item from md's menu; the shared list is untouched.
    pub fn clear(&self) {
        let now = glib::DateTime::now_utc().map(|now| now.to_unix()).unwrap_or_default();
        recent_cleared_at::clear(&*self.settings, now);
        // The setting is md's alone: GTK will not say anything changed.
        self.changed.emit(&());
    }

    /// The path behind a row's token; `None` → the file is gone: forgotten here, alerted by the
    /// caller.
    pub fn resolve(&self, token: &str) -> Option<String> {
        let path = gio::File::for_uri(token).path().and_then(|p| p.into_os_string().into_string().ok());
        if let Some(path) = path.filter(|p| std::path::Path::new(p).is_file()) {
            return Some(path);
        }
        // Gone: no one can reopen it from any application's list. An error means it is not in
        // the list (any more): nothing to forget.
        let _ = self.manager.remove_item(token);
        self.changed.emit(&());
        None
    }

    pub fn changed(&self) -> &Event<()> {
        &self.changed
    }
}

/// One item of the shared list, reduced to what the filter needs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RecentRecord {
    /// The token.
    uri: String,
    /// The local path; `None` for a non-local URI (md never adds one) or a non-UTF-8 name.
    path: Option<String>,
    /// md's own registration stamp, microseconds since the epoch (the order of the rows; the
    /// cutoff compares its whole seconds); `None` when md never registered the item.
    md_stamp: Option<i64>,
    /// Whether the local file exists now.
    exists: bool,
}

fn record_of(info: &gtk::RecentInfo) -> RecentRecord {
    let uri = info.uri().to_string();
    let path = gio::File::for_uri(&uri).path().and_then(|p| p.into_os_string().into_string().ok());
    let md_stamp = if info.has_application(APP_NAME) {
        info.application_info(APP_NAME).map(|(_, _, stamp)| stamp.to_unix_usec())
    } else {
        None
    };
    let exists = path.as_deref().is_some_and(|p| std::path::Path::new(p).is_file());
    RecentRecord { uri, path, md_stamp, exists }
}

/// The filter (§6.6): md's items, newer than the cutoff, still there, newest first, ten at most,
/// one row per path, and same-named rows told apart by their folders.
fn entries(records: Vec<RecentRecord>, cleared_at: Option<i64>, home: &str) -> Vec<RecentEntry> {
    let mut kept: Vec<(i64, String, String)> = records
        .into_iter()
        .filter_map(|record| {
            let stamp = record.md_stamp?;
            let path = record.path?;
            let listed = recent_cleared_at::is_listed(stamp.div_euclid(1_000_000), cleared_at);
            (record.exists && listed).then_some((stamp, record.uri, path))
        })
        .collect();
    // Newest first; equal stamps in a fixed order, so the menu does not shuffle.
    kept.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    let mut seen: Vec<String> = Vec::new();
    kept.retain(|(_, _, path)| {
        if seen.contains(path) {
            false
        } else {
            seen.push(path.clone());
            true
        }
    });
    kept.truncate(MAX_ENTRIES);

    let names: Vec<&str> = kept.iter().map(|(_, _, path)| file_names::name_of(path)).collect();
    kept.iter()
        .map(|(_, uri, path)| {
            let name = file_names::name_of(path);
            let folder = tilde_folder(file_names::directory_of(path), home);
            let title = if names.iter().filter(|n| **n == name).count() > 1 {
                format!("{name}{NAME_FOLDER_SEPARATOR}{folder}")
            } else {
                name.to_string()
            };
            RecentEntry::new(uri.clone(), title, folder)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/me";

    fn record(path: &str, md_stamp: Option<i64>, exists: bool) -> RecentRecord {
        RecentRecord { uri: format!("file://{path}"), path: Some(path.to_string()), md_stamp, exists }
    }

    fn names(entries: &[RecentEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn only_mds_items_newest_first() {
        let records = vec![
            record("/home/me/a.md", Some(10), true),
            record("/home/me/b.md", None, true), // another application's
            record("/home/me/c.md", Some(30), true),
            record("/home/me/d.md", Some(20), true),
        ];
        let listed = entries(records, None, HOME);
        assert_eq!(names(&listed), ["c.md", "d.md", "a.md"]);
        assert_eq!(listed[0].token, "file:///home/me/c.md");
        assert_eq!(listed[0].folder, "~");
    }

    #[test]
    fn the_cutoff_hides_what_md_registered_up_to_its_second() {
        let records = vec![
            record("/home/me/old.md", Some(1_000_000), true),
            record("/home/me/same-second.md", Some(1_999_999), true),
            record("/home/me/new.md", Some(2_000_000), true),
        ];
        assert_eq!(names(&entries(records.clone(), Some(1), HOME)), ["new.md"]);
        assert_eq!(names(&entries(records.clone(), Some(2), HOME)), Vec::<&str>::new());
        assert_eq!(names(&entries(records, Some(0), HOME)), ["new.md", "same-second.md", "old.md"]);
    }

    #[test]
    fn missing_files_are_hidden_and_non_local_items_skipped() {
        let mut remote = record("/x/remote.md", Some(5), true);
        remote.path = None;
        let records =
            vec![record("/home/me/gone.md", Some(9), false), record("/home/me/here.md", Some(1), true), remote];
        assert_eq!(names(&entries(records, None, HOME)), ["here.md"]);
    }

    #[test]
    fn at_most_ten() {
        let records: Vec<RecentRecord> =
            (0..14).map(|n| record(&format!("/home/me/{n:02}.md"), Some(n), true)).collect();
        let listed = entries(records, None, HOME);
        assert_eq!(listed.len(), MAX_ENTRIES);
        assert_eq!(listed[0].name, "13.md");
        assert_eq!(listed[9].name, "04.md");
    }

    #[test]
    fn duplicate_names_carry_their_folders() {
        let records = vec![
            record("/home/me/a/notes.md", Some(3), true),
            record("/home/me/b/notes.md", Some(2), true),
            record("/srv/notes.md", Some(1), true),
            record("/home/me/a/other.md", Some(0), true),
        ];
        let listed = entries(records, None, HOME);
        assert_eq!(
            names(&listed),
            ["notes.md \u{2014} ~/a", "notes.md \u{2014} ~/b", "notes.md \u{2014} /srv", "other.md"]
        );
        assert_eq!(listed[3].folder, "~/a");
        // A name pushed out of the ten does not make its twin carry a folder.
        let mut many: Vec<RecentRecord> =
            (0..10).map(|n| record(&format!("/home/me/{n}.md"), Some(100 + n), true)).collect();
        many.push(record("/home/me/x/0.md", Some(1), true));
        assert!(names(&entries(many, None, HOME)).contains(&"0.md"));
    }

    #[test]
    fn one_row_per_path() {
        let records = vec![record("/home/me/a.md", Some(5), true), record("/home/me/a.md", Some(7), true)];
        let listed = entries(records, None, HOME);
        assert_eq!(names(&listed), ["a.md"]);
    }
}
