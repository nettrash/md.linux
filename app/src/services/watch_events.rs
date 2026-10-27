//! Pure: one `gio::FileMonitor` event → what the document session is told (shell-design §6.7).
//!
//! `(event, file, other, our_path, exists_after_settle)` in, a [`WatchReport`] out — every row of
//! the §6.7 table, unit-tested without a display. [`super::file_watcher::GioFileWatcher`] feeds it
//! from `monitor_file(WATCH_MOVES)` and does the timing (the settle wait and the coalescing); this
//! module only decides.
//!
//! | `gio::FileMonitorEvent` | Reported as |
//! | --- | --- |
//! | `Changed`, `ChangesDoneHint`, `AttributeChanged`, `Created`, `MovedIn` of our file | [`WatchReport::Changed`] |
//! | `Renamed` / `Moved` whose **other** file is ours (another editor's atomic save) | [`WatchReport::Changed`] |
//! | `Renamed` / `Moved` of our file (renamed away) | after the settle: [`WatchReport::Renamed`], or [`WatchReport::Changed`] when our path exists again |
//! | `MovedOut` of our file | after the settle: [`WatchReport::Deleted`] (or `Renamed` when GLib knows where), or `Changed` when our path exists again |
//! | `Deleted` of our file | after the settle: [`WatchReport::Deleted`], or `Changed` when our path exists again |
//! | anything about another file, `PreUnmount`, `Unmounted` | [`WatchReport::Ignore`] |
//!
//! The settle is the design's 250 ms for vim's backup dance (rename the file away, write it anew),
//! applied to the two other ways a file can vanish for a moment as well: `MovedOut` (vim with a
//! `backupdir` elsewhere) and `Deleted` (an editor that unlinks and recreates). A file that is
//! really gone is still reported gone, 250 ms later; one that came back is a change, so a clean
//! document follows the new bytes instead of turning dirty and then conflicted. Events are hints
//! either way: the session's (mtime, size) stamp check before every write is the guard.
#![allow(dead_code)] // S0 stub: removed at the integration gate (P1) — this file's callers are P1's and P5's stubs until then

use std::time::Duration;

use gio::FileMonitorEvent;
use md_logic::documents::file_names;

/// How long a vanished file has to come back before it is reported gone (§6.7: vim's dance).
pub const SETTLE: Duration = Duration::from_millis(250);

/// `changed` is coalesced: at most one per this window (§6.7).
pub const COALESCE: Duration = Duration::from_millis(100);

/// What the watcher tells the session about one monitor event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchReport {
    /// The file's bytes may differ: `FileWatcher::changed(our path)`.
    Changed,
    /// The file now lives at `new`: `FileWatcher::renamed((old, new))`.
    Renamed { old: String, new: String },
    /// The file is gone: `FileWatcher::deleted(our path)`.
    Deleted,
    /// Not about our file, or nothing the session acts on.
    Ignore,
}

/// Whether the event can only be judged after [`SETTLE`]: our file left its path (renamed away,
/// moved out or deleted) and may be back by then. The watcher then waits, looks whether
/// `our_path` exists again, and calls [`interpret`] with the answer.
pub fn needs_settle(event: FileMonitorEvent, file: &str, other: Option<&str>, our_path: &str) -> bool {
    if !is_ours(file, our_path) {
        return false;
    }
    match event {
        FileMonitorEvent::Renamed | FileMonitorEvent::Moved => !other.is_some_and(|other| is_ours(other, our_path)),
        FileMonitorEvent::MovedOut | FileMonitorEvent::Deleted => true,
        _ => false,
    }
}

/// The report for one event. `exists_after_settle` is whether `our_path` existed again
/// [`SETTLE`] after the event; it is read only for the events [`needs_settle`] holds back.
pub fn interpret(
    event: FileMonitorEvent,
    file: &str,
    other: Option<&str>,
    our_path: &str,
    exists_after_settle: bool,
) -> WatchReport {
    let file_is_ours = is_ours(file, our_path);
    let other_is_ours = other.is_some_and(|other| is_ours(other, our_path));
    match event {
        FileMonitorEvent::Changed
        | FileMonitorEvent::ChangesDoneHint
        | FileMonitorEvent::AttributeChanged
        | FileMonitorEvent::Created
        | FileMonitorEvent::MovedIn => {
            if file_is_ours {
                WatchReport::Changed
            } else {
                WatchReport::Ignore
            }
        }
        FileMonitorEvent::Renamed | FileMonitorEvent::Moved => {
            if other_is_ours {
                // Another program's atomic save: a temporary renamed over our file (facts §8).
                WatchReport::Changed
            } else if !file_is_ours {
                WatchReport::Ignore
            } else if exists_after_settle {
                // Renamed away and written anew (vim's backup dance): the same path, new bytes.
                WatchReport::Changed
            } else {
                match other {
                    Some(new) => WatchReport::Renamed { old: our_path.to_string(), new: new.to_string() },
                    None => WatchReport::Deleted,
                }
            }
        }
        FileMonitorEvent::MovedOut => {
            if !file_is_ours {
                WatchReport::Ignore
            } else if exists_after_settle {
                WatchReport::Changed
            } else {
                // inotify cannot say where a file went across folders; when GLib does know, the
                // session can follow it.
                match other {
                    Some(new) if !other_is_ours => {
                        WatchReport::Renamed { old: our_path.to_string(), new: new.to_string() }
                    }
                    _ => WatchReport::Deleted,
                }
            }
        }
        FileMonitorEvent::Deleted => {
            if !file_is_ours {
                WatchReport::Ignore
            } else if exists_after_settle {
                WatchReport::Changed
            } else {
                WatchReport::Deleted
            }
        }
        _ => WatchReport::Ignore,
    }
}

/// Whether a path the monitor reported is the watched one: the same file name in the same folder,
/// compared byte for byte after `.` / `..` and doubled separators are collapsed (GLib builds the
/// event's file from the monitored folder as it was spelled, so no link needs resolving).
fn is_ours(path: &str, our_path: &str) -> bool {
    !path.is_empty() && file_names::same_path(path, our_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gio::FileMonitorEvent as E;

    const OURS: &str = "/home/me/notes/doc.md";

    fn report(event: E, file: &str, other: Option<&str>, exists_after: bool) -> WatchReport {
        interpret(event, file, other, OURS, exists_after)
    }

    #[test]
    fn writes_in_place_are_changes() {
        // append in place: Changed, ChangesDoneHint (facts §8)
        for event in [E::Changed, E::ChangesDoneHint, E::AttributeChanged, E::Created, E::MovedIn] {
            assert!(!needs_settle(event, OURS, None, OURS), "{event:?}");
            assert_eq!(report(event, OURS, None, false), WatchReport::Changed, "{event:?}");
            assert_eq!(report(event, OURS, None, true), WatchReport::Changed, "{event:?}");
        }
    }

    #[test]
    fn an_atomic_save_by_another_editor_is_a_change() {
        // write .doc.md.tmp, rename over: Renamed file=.doc.md.tmp other=doc.md
        let tmp = "/home/me/notes/.doc.md.tmp";
        assert!(!needs_settle(E::Renamed, tmp, Some(OURS), OURS));
        assert_eq!(report(E::Renamed, tmp, Some(OURS), false), WatchReport::Changed);
        // g_file_replace_contents: Renamed .goutputstream-XXXXXX → doc.md
        let stream = "/home/me/notes/.goutputstream-K3Q9ZY";
        assert_eq!(report(E::Renamed, stream, Some(OURS), false), WatchReport::Changed);
        assert_eq!(report(E::Moved, stream, Some(OURS), false), WatchReport::Changed);
    }

    #[test]
    fn a_delete_is_reported_once_the_file_stays_gone() {
        assert!(needs_settle(E::Deleted, OURS, None, OURS));
        assert_eq!(report(E::Deleted, OURS, None, false), WatchReport::Deleted);
        // Unlinked and written anew within the settle: the same file, new bytes.
        assert_eq!(report(E::Deleted, OURS, None, true), WatchReport::Changed);
    }

    #[test]
    fn a_rename_away_in_the_same_folder_is_a_rename() {
        let new = "/home/me/notes/renamed.md";
        assert!(needs_settle(E::Renamed, OURS, Some(new), OURS));
        assert_eq!(
            report(E::Renamed, OURS, Some(new), false),
            WatchReport::Renamed { old: OURS.to_string(), new: new.to_string() }
        );
        // A rename GLib could not pair is a disappearance.
        assert_eq!(report(E::Renamed, OURS, None, false), WatchReport::Deleted);
    }

    #[test]
    fn vims_backup_dance_is_a_change() {
        // :w with backupcopy=no — doc.md → doc.md~, write doc.md, delete doc.md~.
        let backup = "/home/me/notes/doc.md~";
        assert!(needs_settle(E::Renamed, OURS, Some(backup), OURS));
        assert_eq!(report(E::Renamed, OURS, Some(backup), true), WatchReport::Changed);
        assert_eq!(report(E::Created, OURS, None, false), WatchReport::Changed);
        assert!(!needs_settle(E::Deleted, backup, None, OURS));
        assert_eq!(report(E::Deleted, backup, None, false), WatchReport::Ignore);
        // The same with a backupdir in another folder: MovedOut, then Created.
        assert!(needs_settle(E::MovedOut, OURS, None, OURS));
        assert_eq!(report(E::MovedOut, OURS, None, true), WatchReport::Changed);
    }

    #[test]
    fn a_move_to_another_folder_is_a_delete_unless_glib_knows_where() {
        assert_eq!(report(E::MovedOut, OURS, None, false), WatchReport::Deleted);
        let elsewhere = "/home/me/archive/doc.md";
        assert_eq!(
            report(E::MovedOut, OURS, Some(elsewhere), false),
            WatchReport::Renamed { old: OURS.to_string(), new: elsewhere.to_string() }
        );
    }

    #[test]
    fn other_files_and_mount_events_are_ignored() {
        let sibling = "/home/me/notes/other.md";
        for event in [
            E::Changed,
            E::ChangesDoneHint,
            E::AttributeChanged,
            E::Created,
            E::Deleted,
            E::MovedIn,
            E::MovedOut,
            E::Renamed,
            E::Moved,
        ] {
            assert!(!needs_settle(event, sibling, Some("/home/me/notes/x.md"), OURS), "{event:?}");
            assert_eq!(report(event, sibling, Some("/home/me/notes/x.md"), false), WatchReport::Ignore, "{event:?}");
        }
        for event in [E::PreUnmount, E::Unmounted] {
            assert!(!needs_settle(event, OURS, None, OURS));
            assert_eq!(report(event, OURS, None, false), WatchReport::Ignore);
        }
        // A file named like ours in another folder is not ours; case matters on Linux.
        assert_eq!(report(E::Changed, "/home/me/doc.md", None, false), WatchReport::Ignore);
        assert_eq!(report(E::Changed, "/home/me/notes/Doc.md", None, false), WatchReport::Ignore);
        assert_eq!(report(E::Changed, "", None, false), WatchReport::Ignore);
    }

    #[test]
    fn spellings_of_our_path_are_ours() {
        for spelling in ["/home/me/notes/./doc.md", "/home/me//notes/doc.md", "/home/me/x/../notes/doc.md"] {
            assert_eq!(report(E::Changed, spelling, None, false), WatchReport::Changed, "{spelling}");
        }
        assert_eq!(report(E::Renamed, "/tmp/.x", Some("/home/me/notes/./doc.md"), false), WatchReport::Changed);
    }

    #[test]
    fn a_rename_onto_ourselves_is_a_change_not_a_disappearance() {
        assert!(!needs_settle(E::Renamed, OURS, Some(OURS), OURS));
        assert_eq!(report(E::Renamed, OURS, Some(OURS), false), WatchReport::Changed);
        assert_eq!(report(E::MovedOut, OURS, Some(OURS), false), WatchReport::Deleted);
    }
}
