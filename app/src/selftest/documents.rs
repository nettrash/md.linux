//! The `documents` self-test group (shell-design §13.3, docs/shell-work-packages.md P2): the real
//! `GioFileWatcher` against a temp folder — append in place, a temporary renamed over, GIO's
//! `replace_contents`, delete, rename away, move out, vim's backup dance — each reported as §6.7's
//! table says; in-place writes keep the inode; Rename refuses an existing target; Move To across
//! file systems; TextPack / TextBundle imports and their titles; a real `TextFileSession` over the
//! real services (autosave in place, an outside `echo >>` reloading silently, an outside atomic
//! save under edits conflicting, Revert, Rename / Move To / a rename under it keeping the session,
//! the rescue copy at close); `RecentFiles` against `OUT/home`'s XBEL; the examples and the example
//! book; `AdwAlerts` (defaults, validation, the queue, callers that give up, a window that goes)
//! and `GtkPickers` presented and answered — by `Escape` and `Return` through `xdotool` in the
//! input tier, by closing them without it.
//!
//! Files go under `OUT/documents/`, except the monitor and session rows, which run in the system
//! temp folder (a native file system; see [`run`]) and clean up after themselves, as the move
//! across file systems does. Nothing goes under `OUT/home` beyond what `RecentFiles` and the
//! settings put there — the `privacy` group checks that.
//!
//! The window-level rows of §13.3's `documents` line (each extension through `open`, the second
//! `open` focusing, the conflict bar, drafts) need P1's window manager and are not here yet.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use md_core::export::text_bundle;
use md_logic::activation::{ActivationAction, activation_router};
use md_logic::documents::document_loader::{self, DocumentKind};
use md_logic::documents::{SessionRole, Stage, TextFileSession, file_names};
use md_logic::seams::{Alerts, CloseChoice, FileSystem, FileWatcher, Pickers};
use md_logic::settings::SettingsStore;
use md_logic::settings::settings_keys::RECENT_CLEARED_AT;
use md_logic::strings;
use md_logic::windows::WindowRegistry;

use crate::selftest::Harness;
use crate::services::alerts::AdwAlerts;
use crate::services::document_ops;
use crate::services::file_system::{CanonicalFileIdentity, StdFileSystem};
use crate::services::file_watcher::GioFileWatcher;
use crate::services::glib_seams::GlibScheduler;
use crate::services::pickers::{GtkPickers, finish_save};

/// How long a watcher row waits for the first report, and then for stragglers (the settle plus
/// the coalescing, with room to spare).
const FIRST_REPORT: Duration = Duration::from_secs(2);
const STRAGGLERS: Duration = Duration::from_millis(700);
/// A dialog's appearance or answer.
const DIALOG_LIMIT: Duration = Duration::from_secs(5);
/// Past the whole-second Clear Menu cutoff (`settings::recent_cleared_at`).
const NEXT_SECOND: Duration = Duration::from_millis(1100);

pub async fn run(h: &Harness) {
    let root = h.out().join("documents");
    let _ = std::fs::remove_dir_all(&root);
    if let Err(err) = std::fs::create_dir_all(&root) {
        h.check("folder", false, format!("{}: {err}", root.display()));
        return;
    }
    // The monitor rows run on the system temp folder, a native Linux file system: OUT is a bind
    // mount, and Docker Desktop's mounts replay the host's file events into the container late —
    // the creation of a file shows up after a monitor started on it — which is no Linux desktop's
    // behaviour.
    let scratch = std::env::temp_dir().join(format!("md-selftest-documents-{}", std::process::id()));
    watcher_rows(h, &scratch.join("watch")).await;
    file_rows(h, &root.join("files")).await;
    import_rows(h, &root.join("imports")).await;
    session_rows(h, &scratch.join("session")).await;
    recent_rows(h, &root.join("recent")).await;
    example_rows(h, &root.join("examples")).await;
    dialog_rows(h, &root.join("dialogs")).await;
    let _ = std::fs::remove_dir_all(&scratch);
}

fn string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn fresh(dir: &Path) -> std::io::Result<()> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)
}

// ---- the watcher (§6.7) ----

/// A watcher whose reports are written down as `changed doc.md`, `renamed doc.md → x.md`,
/// `deleted doc.md`.
struct Recording {
    watcher: Rc<GioFileWatcher>,
    log: Rc<RefCell<Vec<String>>>,
}

impl Recording {
    fn new() -> Recording {
        let watcher = GioFileWatcher::new();
        let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let log = Rc::clone(&log);
            watcher.changed().connect(move |path: &str| {
                log.borrow_mut().push(format!("changed {}", file_names::name_of(path)));
            });
        }
        {
            let log = Rc::clone(&log);
            watcher.renamed().connect(move |(old, new): &(String, String)| {
                log.borrow_mut().push(format!(
                    "renamed {} \u{2192} {}",
                    file_names::name_of(old),
                    file_names::name_of(new)
                ));
            });
        }
        {
            let log = Rc::clone(&log);
            watcher.deleted().connect(move |path: &str| {
                log.borrow_mut().push(format!("deleted {}", file_names::name_of(path)));
            });
        }
        Recording { watcher, log }
    }

    fn entries(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

/// Only `changed doc.md`, at least once.
fn only_changes(log: &[String]) -> bool {
    !log.is_empty() && log.iter().all(|e| e == "changed doc.md")
}

/// One §6.7 row: a fresh `dir/doc.md`, watched; `action(dir)`; then what was reported.
async fn watch_case(
    h: &Harness,
    folder: &Path,
    name: &str,
    action: impl FnOnce(&Path) -> std::io::Result<()>,
    expect: impl Fn(&[String]) -> bool,
    expect_text: &str,
) {
    let dir = folder.join(name);
    let doc = dir.join("doc.md");
    if let Err(err) = fresh(&dir).and_then(|()| std::fs::write(&doc, "start\n")) {
        h.check(&format!("watch_{name}"), false, format!("{}: {err}", dir.display()));
        return;
    }
    let recording = Recording::new();
    recording.watcher.watch(&string(&doc));
    h.sleep(Duration::from_millis(150)).await;
    if let Err(err) = action(&dir) {
        recording.watcher.stop();
        h.check(&format!("watch_{name}"), false, format!("the change itself failed: {err}"));
        return;
    }
    h.wait_until(FIRST_REPORT, || !recording.log.borrow().is_empty()).await;
    h.sleep(STRAGGLERS).await;
    recording.watcher.stop();
    let log = recording.entries();
    h.check(&format!("watch_{name}"), expect(&log), format!("reported [{}], expected {expect_text}", log.join(", ")));
}

async fn watcher_rows(h: &Harness, folder: &Path) {
    use std::io::Write as _;

    watch_case(
        h,
        folder,
        "append_in_place",
        |dir| std::fs::OpenOptions::new().append(true).open(dir.join("doc.md"))?.write_all(b"appended\n"),
        only_changes,
        "only changed",
    )
    .await;
    watch_case(
        h,
        folder,
        "temporary_renamed_over",
        |dir| {
            std::fs::write(dir.join(".doc.md.tmp"), "another editor's save\n")?;
            std::fs::rename(dir.join(".doc.md.tmp"), dir.join("doc.md"))
        },
        only_changes,
        "only changed",
    )
    .await;
    watch_case(
        h,
        folder,
        "replace_contents",
        |dir| {
            gio::File::for_path(dir.join("doc.md"))
                .replace_contents(
                    b"g_file_replace_contents\n",
                    None,
                    false,
                    gio::FileCreateFlags::NONE,
                    gio::Cancellable::NONE,
                )
                .map(|_| ())
                .map_err(std::io::Error::other)
        },
        only_changes,
        "only changed",
    )
    .await;
    watch_case(
        h,
        folder,
        "delete",
        |dir| std::fs::remove_file(dir.join("doc.md")),
        |log| log == ["deleted doc.md"],
        "[deleted doc.md]",
    )
    .await;
    watch_case(
        h,
        folder,
        "rename_away",
        |dir| std::fs::rename(dir.join("doc.md"), dir.join("renamed.md")),
        |log| log == ["renamed doc.md \u{2192} renamed.md"],
        "[renamed doc.md \u{2192} renamed.md]",
    )
    .await;
    watch_case(
        h,
        folder,
        "move_out",
        |dir| {
            let elsewhere = dir.with_file_name("move_out-elsewhere");
            fresh(&elsewhere)?;
            std::fs::rename(dir.join("doc.md"), elsewhere.join("doc.md"))
        },
        |log| log == ["deleted doc.md"],
        "[deleted doc.md] (inotify cannot say where)",
    )
    .await;
    watch_case(
        h,
        folder,
        "vim_backup_dance",
        |dir| {
            std::fs::rename(dir.join("doc.md"), dir.join("doc.md~"))?;
            std::fs::write(dir.join("doc.md"), "written anew\n")?;
            std::fs::remove_file(dir.join("doc.md~"))
        },
        only_changes,
        "only changed (never renamed or deleted)",
    )
    .await;
    watch_case(
        h,
        folder,
        "unlink_and_recreate",
        |dir| {
            std::fs::remove_file(dir.join("doc.md"))?;
            std::fs::write(dir.join("doc.md"), "recreated\n")
        },
        only_changes,
        "only changed (the path came back within the settle)",
    )
    .await;
    watch_case(
        h,
        folder,
        "sibling_ignored",
        |dir| std::fs::write(dir.join("sibling.md"), "not ours\n"),
        <[String]>::is_empty,
        "nothing",
    )
    .await;

    // stop() silences the watcher.
    let dir = folder.join("stopped");
    let doc = dir.join("doc.md");
    if fresh(&dir).and_then(|()| std::fs::write(&doc, "start\n")).is_ok() {
        let recording = Recording::new();
        recording.watcher.watch(&string(&doc));
        recording.watcher.stop();
        let appended =
            std::fs::OpenOptions::new().append(true).open(&doc).and_then(|mut f| f.write_all(b"after stop\n"));
        h.sleep(STRAGGLERS).await;
        let log = recording.entries();
        h.check(
            "watch_stop_silences",
            appended.is_ok() && log.is_empty(),
            format!("reported [{}] after stop()", log.join(", ")),
        );
    }
}

// ---- file operations (§6.2, §6.4) ----

async fn file_rows(h: &Harness, folder: &Path) {
    if let Err(err) = fresh(folder) {
        h.check("files", false, format!("{}: {err}", folder.display()));
        return;
    }
    let files = StdFileSystem::new();

    // An in-place write keeps the inode.
    let doc = folder.join("in-place.md");
    let written =
        std::fs::write(&doc, "the first, longer version\n").and_then(|()| std::fs::metadata(&doc)).and_then(|before| {
            files.write_all_bytes_in_place(&string(&doc), b"second\n")?;
            Ok((before.ino(), std::fs::metadata(&doc)?.ino(), std::fs::read(&doc)?))
        });
    match written {
        Ok((before, after, bytes)) => h.check(
            "in_place_write_keeps_the_inode",
            before == after && bytes == b"second\n",
            format!("inode {before} → {after}, {} bytes", bytes.len()),
        ),
        Err(err) => h.check("in_place_write_keeps_the_inode", false, err.to_string()),
    };

    // Rename refuses an existing target, renames otherwise.
    let (a, b) = (folder.join("a.md"), folder.join("b.md"));
    let setup = std::fs::write(&a, "a").and_then(|()| std::fs::write(&b, "b"));
    let refused = document_ops::rename(&string(&a), "b.md");
    let intact = std::fs::read(&a).ok().as_deref() == Some(b"a".as_slice())
        && std::fs::read(&b).ok().as_deref() == Some(b"b".as_slice());
    h.check(
        "rename_refuses_an_existing_target",
        setup.is_ok() && refused.is_err() && intact,
        format!("{refused:?}; both files intact: {intact}"),
    );
    let renamed = document_ops::rename(&string(&a), "c.md");
    h.check(
        "rename_renames",
        renamed.as_deref() == Ok(string(&folder.join("c.md")).as_str()) && !a.exists(),
        format!("{renamed:?}"),
    );

    // Move To across file systems: the system temp folder ↔ OUT (the home folder's file system).
    let temp = std::env::temp_dir().join(format!("md-selftest-move-{}", std::process::id()));
    let target = folder.join("moved");
    let setup = fresh(&temp).and_then(|()| fresh(&target));
    let source = temp.join("travels.md");
    let contents = "Привет — moved across file systems\n";
    let devices = std::fs::metadata(&temp).and_then(|t| std::fs::metadata(&target).map(|o| (t.dev(), o.dev())));
    let across = devices.as_ref().map(|(t, o)| t != o).unwrap_or(false);
    let there = setup
        .and_then(|()| std::fs::write(&source, contents))
        .map_err(|e| e.to_string())
        .and_then(|()| document_ops::move_to(&string(&source), &string(&target)));
    let arrived = there.as_ref().ok().and_then(|p| std::fs::read_to_string(p).ok());
    let back = there.as_ref().map_err(String::clone).and_then(|p| document_ops::move_to(p, &string(&temp)));
    let returned = back.as_ref().ok().and_then(|p| std::fs::read_to_string(p).ok());
    h.check(
        "move_to_across_file_systems",
        arrived.as_deref() == Some(contents)
            && returned.as_deref() == Some(contents)
            && !target.join("travels.md").exists(),
        format!(
            "{} → {} → {}; {}",
            source.display(),
            there.as_deref().unwrap_or("failed"),
            back.as_deref().unwrap_or("failed"),
            if across { "two file systems (copy + delete)" } else { "ONE file system here (rename only)" }
        ),
    );
    let _ = std::fs::remove_dir_all(&temp);
}

// ---- imports (§6.5) ----

/// A session with the real services and no window.
fn session(watcher: Rc<GioFileWatcher>) -> Rc<TextFileSession> {
    TextFileSession::new(
        Rc::new(StdFileSystem::new()),
        watcher,
        Rc::new(GlibScheduler),
        Rc::new(WindowRegistry::new()),
        Rc::new(CanonicalFileIdentity::new()),
        SessionRole::Document,
        None,
    )
}

async fn import_rows(h: &Harness, folder: &Path) {
    if let Err(err) = fresh(folder) {
        h.check("imports", false, format!("{}: {err}", folder.display()));
        return;
    }
    let text = "# Field notes\n\nПривет, мир.\n";
    let wrapper = text_bundle::bundle_wrapper(text, Vec::new());
    let pack = folder.join("Field Notes.textpack");
    let bundle = folder.join("Trip.textbundle");
    let made = wrapper
        .to_text_pack("Field Notes")
        .map_err(|e| format!("{e:?}"))
        .and_then(|bytes| std::fs::write(&pack, bytes).map_err(|e| e.to_string()))
        .and_then(|()| wrapper.write(&bundle).map_err(|e| e.to_string()));
    if let Err(err) = made {
        h.check("imports", false, err);
        return;
    }

    for (row, path, kind, title) in [
        ("textpack_imports_titled", &pack, DocumentKind::TextPack, "Field Notes"),
        ("textbundle_imports_titled", &bundle, DocumentKind::TextBundleFolder, "Trip"),
    ] {
        let path = string(path);
        let loaded = document_ops::import(&path);
        let session = session(GioFileWatcher::new());
        let opened = session.open(&path);
        let ok = loaded.as_ref().is_ok_and(|d| d.kind == kind && d.title == title && d.text == text)
            && opened
            && session.title() == title
            && session.stage() == Stage::Untitled
            && session.imported_from().as_deref() == Some(path.as_str())
            && session.text() == text;
        h.check(
            row,
            ok,
            format!(
                "import: {:?}; session: opened {opened}, title \u{201C}{}\u{201D}, stage {:?}, imported from {:?}",
                loaded.as_ref().map(|d| (&d.title, d.kind)),
                session.title(),
                session.stage(),
                session.imported_from()
            ),
        );
        session.dispose();
    }

    // A .textbundle folder handed over by `open` or a drop routes to the import; a file merely
    // named like one opens as a document.
    let item = document_ops::activation_item(&gio::File::for_path(&bundle));
    let action = item.as_ref().and_then(activation_router::classify);
    let impostor = folder.join("impostor.textbundle");
    let _ = std::fs::write(&impostor, "plain text\n");
    let impostor_action =
        document_ops::activation_item(&gio::File::for_path(&impostor)).as_ref().and_then(activation_router::classify);
    h.check(
        "textbundle_folder_routes_to_import",
        action == Some(ActivationAction::ImportTextBundleFolder(string(&bundle)))
            && impostor_action == Some(ActivationAction::OpenPath(string(&impostor))),
        format!("{action:?}; {impostor_action:?}"),
    );
}

// ---- a real session over the real services (§6.3, §6.4) ----

async fn session_rows(h: &Harness, folder: &Path) {
    if let Err(err) = fresh(folder) {
        h.check("session", false, format!("{}: {err}", folder.display()));
        return;
    }
    let doc = folder.join("doc.md");
    if let Err(err) = std::fs::write(&doc, "one\n") {
        h.check("session", false, err.to_string());
        return;
    }
    let watcher = GioFileWatcher::new();
    let session = session(Rc::clone(&watcher));
    let reloads = Rc::new(Cell::new(0));
    {
        let reloads = Rc::clone(&reloads);
        session.text_replaced_externally().connect(move |_: &str| reloads.set(reloads.get() + 1));
    }
    let alerts: Rc<RefCell<Vec<(String, String)>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let alerts = Rc::clone(&alerts);
        session.alert_requested().connect(move |alert: &(String, String)| alerts.borrow_mut().push(alert.clone()));
    }
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or_default();
    let ino = |path: &Path| std::fs::metadata(path).map(|m| m.ino()).unwrap_or_default();

    let opened = session.open(&string(&doc));
    h.check(
        "session_opens",
        opened && session.stage() == Stage::Editing(string(&doc)) && session.title() == "doc",
        format!("{:?}, title \u{201C}{}\u{201D}", session.stage(), session.title()),
    );
    if !opened {
        session.dispose();
        return;
    }

    // Autosave writes in place a second after the edit, and its own change event is ignored.
    let before = ino(&doc);
    session.edit("one\ntwo\n");
    let saved = h.wait_until(Duration::from_secs(4), || read(&doc) == "one\ntwo\n").await;
    h.sleep(STRAGGLERS).await;
    h.check(
        "autosave_writes_in_place",
        saved && ino(&doc) == before && !session.conflicted() && !session.has_unsaved_changes() && session.is_dirty(),
        format!(
            "on disk {:?}, inode {before} → {}, conflicted {}, unsaved {}, edited {}",
            read(&doc),
            ino(&doc),
            session.conflicted(),
            session.has_unsaved_changes(),
            session.is_dirty()
        ),
    );

    // An outside `echo >>` reloads a clean buffer silently.
    let reloads_before = reloads.get();
    let (appended, output) = h.run(&["sh", "-c", "echo three >> \"$1\"", "sh", &string(&doc)]).await;
    let reloaded = h.wait_until(Duration::from_secs(3), || session.text() == "one\ntwo\nthree\n").await;
    h.check(
        "outside_append_reloads_silently",
        appended && reloaded && reloads.get() > reloads_before && !session.conflicted(),
        format!("text {:?}, conflicted {}, {output}", session.text(), session.conflicted()),
    );

    // Another editor's atomic save under unsaved edits conflicts, and the autosave stays off it.
    session.edit("one\ntwo\nthree\nmine\n");
    let replaced = std::fs::write(folder.join(".doc.md.tmp"), "theirs\n")
        .and_then(|()| std::fs::rename(folder.join(".doc.md.tmp"), &doc));
    let conflicted = h.wait_until(Duration::from_secs(3), || session.conflicted()).await;
    h.sleep(TextFileSession::AUTOSAVE_DELAY + Duration::from_millis(500)).await;
    h.check(
        "outside_atomic_save_under_edits_conflicts",
        replaced.is_ok() && conflicted && read(&doc) == "theirs\n",
        format!("conflicted {}, on disk {:?}", session.conflicted(), read(&doc)),
    );
    session.resolve_conflict_reloading();
    h.check(
        "reload_from_disk_resolves",
        !session.conflicted() && session.text() == "theirs\n",
        format!("text {:?}, conflicted {}", session.text(), session.conflicted()),
    );

    // Revert to Saved goes back to the last explicit save, on disk too.
    session.edit("saved\n");
    let flushed = session.flush_now(true);
    session.edit("saved\nscratch\n");
    h.wait_until(Duration::from_secs(4), || read(&doc) == "saved\nscratch\n").await;
    session.revert_to_saved();
    h.sleep(STRAGGLERS).await;
    h.check(
        "revert_to_saved",
        flushed && session.text() == "saved\n" && read(&doc) == "saved\n" && !session.conflicted(),
        format!("text {:?}, on disk {:?}", session.text(), read(&doc)),
    );

    // Rename… keeps the session, and the watcher follows the new name.
    let renamed = document_ops::rename(&string(&doc), "renamed.md");
    if let Ok(path) = &renamed {
        session.retarget(path);
    }
    let renamed_path = folder.join("renamed.md");
    let _ = h.run(&["sh", "-c", "echo after-rename >> \"$1\"", "sh", &string(&renamed_path)]).await;
    let followed = h.wait_until(Duration::from_secs(3), || session.text().ends_with("after-rename\n")).await;
    h.check(
        "rename_keeps_the_session",
        renamed.is_ok()
            && session.stage() == Stage::Editing(string(&renamed_path))
            && session.title() == "renamed"
            && followed,
        format!("{renamed:?}, {:?}, text {:?}", session.stage(), session.text()),
    );

    // Move To… the same.
    let sub = folder.join("sub");
    let _ = std::fs::create_dir_all(&sub);
    let moved = document_ops::move_to(&string(&renamed_path), &string(&sub));
    if let Ok(path) = &moved {
        session.retarget(path);
    }
    let moved_path = sub.join("renamed.md");
    let _ = h.run(&["sh", "-c", "echo after-move >> \"$1\"", "sh", &string(&moved_path)]).await;
    let followed = h.wait_until(Duration::from_secs(3), || session.text().ends_with("after-move\n")).await;
    h.check(
        "move_to_keeps_the_session",
        moved.is_ok() && session.stage() == Stage::Editing(string(&moved_path)) && followed,
        format!("{moved:?}, {:?}", session.stage()),
    );

    // A rename by another program, in the same folder: the session follows it.
    let away = sub.join("renamed elsewhere.md");
    let (mv, output) = h.run(&["mv", &string(&moved_path), &string(&away)]).await;
    let followed = h.wait_until(Duration::from_secs(3), || session.stage() == Stage::Editing(string(&away))).await;
    h.check(
        "outside_rename_is_followed",
        mv && followed && session.title() == "renamed elsewhere",
        format!("{:?}, title \u{201C}{}\u{201D} {output}", session.stage(), session.title()),
    );

    // The rescue copy at close: the write is refused, the text goes to a sibling.
    let path = away.clone();
    session.edit("rescue me\n");
    let refusal = refuse_writes(&path);
    let reason = match &refusal {
        Refusal::ReadOnly => "the file made read-only".to_string(),
        Refusal::Conflict => {
            // Root writes through any mode bits: refuse by conflict instead (an outside rewrite).
            let _ = std::fs::write(&path, "outside version\n");
            h.wait_until(Duration::from_secs(3), || session.conflicted()).await;
            "a conflict (this process writes through read-only modes)".to_string()
        }
    };
    let expected_original = read(&path);
    session.close_flush();
    let rescued = sub.join("renamed elsewhere (rescued).md");
    let alert = alerts.borrow().last().cloned();
    h.check(
        "rescue_copy_at_close",
        read(&rescued) == "rescue me\n"
            && read(&path) == expected_original
            && alert.as_ref().is_some_and(|(title, message)| {
                *title == strings::documents::rescue_failed_title("renamed elsewhere")
                    && *message == strings::documents::rescue_kept_message("renamed elsewhere (rescued).md")
            }),
        format!("refused by {reason}; rescue {:?}; alert {alert:?}", read(&rescued)),
    );
    if matches!(refusal, Refusal::ReadOnly) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
    }
    session.dispose();
    watcher.stop();
}

enum Refusal {
    ReadOnly,
    Conflict,
}

/// Makes writes to `path` fail: read-only when that stops this process, else (root) not at all —
/// the caller then refuses by conflict.
fn refuse_writes(path: &Path) -> Refusal {
    use std::os::unix::fs::PermissionsExt;
    if std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444)).is_err() {
        return Refusal::Conflict;
    }
    if std::fs::OpenOptions::new().write(true).open(path).is_err() {
        return Refusal::ReadOnly;
    }
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
    Refusal::Conflict
}

// ---- recent documents (§6.6) ----

async fn recent_rows(h: &Harness, folder: &Path) {
    let make = || -> std::io::Result<()> {
        fresh(folder)?;
        for dir in ["x", "y"] {
            std::fs::create_dir_all(folder.join(dir))?;
            std::fs::write(folder.join(dir).join("notes.md"), "notes\n")?;
        }
        std::fs::write(folder.join("a.md"), "a\n")?;
        std::fs::write(folder.join("b.md"), "b\n")
    };
    if let Err(err) = make() {
        h.check("recent", false, err.to_string());
        return;
    }
    let shell = h.shell();
    let recent = Rc::clone(&shell.recent);
    let manager = gtk::RecentManager::default();
    let uri = |path: &Path| gio::File::for_path(path).uri().to_string();
    let (a, b) = (folder.join("a.md"), folder.join("b.md"));
    let names = |recent: &crate::services::recent::RecentFiles| -> Vec<String> {
        recent.entries().into_iter().map(|e| e.name).collect()
    };
    let changes = Rc::new(Cell::new(0));
    let subscription = {
        let changes = Rc::clone(&changes);
        recent.changed().connect(move |_: &()| changes.set(changes.get() + 1))
    };

    // A clean start: whatever an earlier group opened is hidden. The cutoff is a whole second,
    // so the next additions wait for the next one.
    recent.clear();
    h.sleep(NEXT_SECOND).await;
    recent.add(&string(&a));
    h.sleep(Duration::from_millis(5)).await;
    recent.add(&string(&b));
    let listed = names(&recent);
    let entries = recent.entries();
    let xbel = h.home().join(".local/share/recently-used.xbel");
    let written = h
        .wait_until(Duration::from_secs(3), || {
            std::fs::read_to_string(&xbel).is_ok_and(|x| x.contains(&uri(&a)) && x.contains(&uri(&b)))
        })
        .await;
    let notified = h.wait_until(Duration::from_secs(3), || changes.get() >= 2).await;
    h.check(
        "recent_add_and_list",
        listed == ["b.md", "a.md"] && entries.first().is_some_and(|e| e.token == uri(&b)) && written && notified,
        format!("{listed:?}; {} written: {written}; changed ×{}", xbel.display(), changes.get()),
    );

    // Two rows with one name carry their folders.
    recent.add(&string(&folder.join("x/notes.md")));
    h.sleep(Duration::from_millis(5)).await;
    recent.add(&string(&folder.join("y/notes.md")));
    let listed = names(&recent);
    let separator = " \u{2014} ";
    let told_apart = listed.iter().any(|n| n.starts_with("notes.md \u{2014} ") && n.ends_with("/x"))
        && listed.iter().any(|n| n.starts_with("notes.md \u{2014} ") && n.ends_with("/y"))
        && listed.contains(&"a.md".to_string());
    h.check("recent_same_names_show_folders", told_apart, format!("{listed:?} (separator {separator:?})"));

    // Clear Menu hides md's rows and deletes nothing from the shared list.
    recent.clear();
    let cleared = names(&recent);
    let kept = manager.has_item(&uri(&a)) && manager.has_item(&uri(&b));
    let stamp = shell.settings.get_string(RECENT_CLEARED_AT);
    h.check(
        "recent_clear_hides_and_keeps_the_shared_list",
        cleared.is_empty() && kept && stamp.is_some(),
        format!("after Clear Menu {cleared:?}; still in the XBEL: {kept}; {RECENT_CLEARED_AT} = {stamp:?}"),
    );
    h.sleep(NEXT_SECOND).await;
    recent.add(&string(&a));
    let again = names(&recent);
    h.check("recent_reopened_after_clear_shows", again == ["a.md"], format!("{again:?}"));

    // A file that is gone: hidden from the menu, forgotten when its row is chosen.
    recent.add(&string(&b));
    let _ = std::fs::remove_file(&b);
    let hidden = !names(&recent).contains(&"b.md".to_string());
    let resolved_gone = recent.resolve(&uri(&b));
    let forgotten = !manager.has_item(&uri(&b));
    let resolved_here = recent.resolve(&uri(&a));
    h.check(
        "recent_missing_file_hidden_then_forgotten",
        hidden && resolved_gone.is_none() && forgotten && resolved_here.as_deref() == Some(string(&a).as_str()),
        format!(
            "hidden {hidden}; resolve(gone) {resolved_gone:?}; forgotten {forgotten}; resolve(a) {resolved_here:?}"
        ),
    );
    recent.changed().disconnect(subscription);
}

// ---- examples (§2.3) ----

async fn example_rows(h: &Harness, folder: &Path) {
    let shell = h.shell();
    let names: Vec<String> = shell.examples.examples().into_iter().map(|e| e.name).collect();
    let expected = ["Welcome", "Formatting", "Tables", "Code", "Images", "Math", "Diagrams", "Plots", "Writer Tools"];
    let welcome = shell.examples.read("01-Welcome.md");
    h.check(
        "examples_listed_in_menu_order",
        names == expected && welcome.as_deref().is_some_and(|t| t.starts_with('#')),
        format!("{names:?}; welcome {} bytes", welcome.as_ref().map_or(0, String::len)),
    );
    if let Err(err) = fresh(folder) {
        h.check("example_book_unpacks", false, err.to_string());
        return;
    }
    let first = shell.examples.unpack_book(&string(folder));
    let second = shell.examples.unpack_book(&string(folder));
    let source = shell.examples.book_folder();
    let same_tree =
        first.as_ref().is_ok_and(|copy| tree(Path::new(copy)) == tree(&source) && !tree(&source).is_empty());
    h.check(
        "example_book_unpacks_without_replacing",
        same_tree && second.as_deref() == Ok(string(&folder.join("Example Book 2")).as_str()),
        format!("{first:?}, then {second:?}; {} files", tree(&source).len()),
    );
}

/// Every file under `root`, relative, with its bytes.
fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let (Ok(relative), Ok(bytes)) = (path.strip_prefix(root), std::fs::read(&path)) {
                found.push((relative.to_path_buf(), bytes));
            }
        }
    }
    found.sort();
    found
}

// ---- dialogs (§6.1, §7.8) ----

/// How a dialog is dismissed: the key (input tier) or, without input, by closing it.
#[derive(Clone, Copy)]
enum Answer {
    Escape,
    Return,
}

async fn dialog_rows(h: &Harness, folder: &Path) {
    if let Err(err) = fresh(folder) {
        h.check("dialogs", false, err.to_string());
        return;
    }
    let shell = h.shell();
    let window = adw::ApplicationWindow::builder()
        .application(&shell.app)
        .title("md documents self-test")
        .default_width(900)
        .default_height(640)
        .build();
    window.present();
    h.next_frame(&window).await;
    let toplevel: gtk::Window = window.clone().upcast();
    let alerts = AdwAlerts::new(&toplevel);
    let opened: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let opened = Rc::clone(&opened);
        alerts.open_changed().connect(move |open: &bool| opened.borrow_mut().push(*open));
    }
    let seam: Rc<dyn Alerts> = alerts.clone();

    alert_rows(h, &window, &seam, &opened).await;
    picker_rows(h, &window, Rc::clone(&seam), folder).await;
    window.destroy();

    // A window that goes away answers its dialogs — the one up and the ones in line — with the
    // cancelling answer, so no caller hangs on it.
    let doomed = adw::ApplicationWindow::builder()
        .application(&shell.app)
        .title("md documents self-test (closing)")
        .default_width(640)
        .default_height(480)
        .build();
    doomed.present();
    h.next_frame(&doomed).await;
    let doomed_alerts: Rc<dyn Alerts> = AdwAlerts::new(doomed.upcast_ref());
    let up = start(doomed_alerts.confirm_delete(strings::books::delete_question("Chapter"), String::new()));
    let queued = start(doomed_alerts.ask_save_changes(strings::documents::save_changes_question("Untitled")));
    let shown = h.wait_until(DIALOG_LIMIT, || doomed.visible_dialog().is_some()).await;
    doomed.destroy();
    let answered = h.wait_until(DIALOG_LIMIT, || up.borrow().is_some() && queued.borrow().is_some()).await;
    let late = doomed_alerts.warn("After the window".into(), String::new());
    let late = start(late);
    let late_answered = h.wait_until(DIALOG_LIMIT, || late.borrow().is_some()).await;
    h.check(
        "alerts_answered_when_the_window_goes",
        shown
            && answered
            && *up.borrow() == Some(false)
            && *queued.borrow() == Some(CloseChoice::Cancel)
            && late_answered,
        format!("up {:?}, in line {:?}, after {:?}", up.borrow(), queued.borrow(), late.borrow()),
    );
}

/// Starts `future` on the main context; its answer arrives in the returned cell.
fn start<T: 'static>(future: impl Future<Output = T> + 'static) -> Rc<RefCell<Option<T>>> {
    let answer = Rc::new(RefCell::new(None));
    let slot = Rc::clone(&answer);
    glib::spawn_future_local(async move {
        *slot.borrow_mut() = Some(future.await);
    });
    answer
}

/// Waits for an alert on `window`, dismisses it as `how` says, and waits for the answer. The
/// detail says how far it got.
async fn answer_alert<T: Clone + 'static>(
    h: &Harness,
    window: &adw::ApplicationWindow,
    answer: &Rc<RefCell<Option<T>>>,
    how: Answer,
    shot: &str,
) -> Result<T, String> {
    if !h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await {
        return Err("no dialog was presented".to_string());
    }
    h.screenshot(shot).await;
    let dialog = window.visible_dialog().ok_or("the dialog went away by itself")?;
    if h.input() {
        let key = match how {
            Answer::Escape => "Escape",
            Answer::Return => "Return",
        };
        if !h.focus(window.upcast_ref()).await || !h.key(key).await {
            return Err(format!("xdotool could not press {key}"));
        }
    } else {
        dialog.close();
    }
    if !h.wait_until(DIALOG_LIMIT, || answer.borrow().is_some()).await {
        return Err("the dialog never answered".to_string());
    }
    // Gone — another queued alert may already have taken its place.
    if !h.wait_until(DIALOG_LIMIT, || window.visible_dialog().as_ref() != Some(&dialog)).await {
        return Err("the dialog stayed up after answering".to_string());
    }
    answer.borrow().clone().ok_or_else(|| "no answer".to_string())
}

async fn alert_rows(
    h: &Harness,
    window: &adw::ApplicationWindow,
    alerts: &Rc<dyn Alerts>,
    opened: &Rc<RefCell<Vec<bool>>>,
) {
    let how = if h.input() { "Escape through xdotool" } else { "closed (no input)" };

    // Escape (or closing) is always the cancelling answer.
    let answer = start(alerts.warn(strings::documents::COULD_NOT_OPEN.into(), "A message.".into()));
    let got = answer_alert(h, window, &answer, Answer::Escape, "alert-warn").await;
    h.check("alert_warn_dismissed", got.is_ok(), format!("{how}: {got:?}"));

    let answer = start(alerts.prompt_name(
        strings::documents::RENAME_TITLE.into(),
        strings::documents::RENAME_MESSAGE.into(),
        "Draft".into(),
        strings::documents::RENAME_TITLE.into(),
    ));
    let got = answer_alert(h, window, &answer, Answer::Escape, "alert-prompt").await;
    h.check("alert_prompt_name_dismissed", got == Ok(None), format!("{how}: {got:?}"));

    let answer = start(
        alerts
            .confirm_delete(strings::books::delete_question("Chapter"), strings::books::DELETE_CHAPTER_MESSAGE.into()),
    );
    let got = answer_alert(h, window, &answer, Answer::Escape, "alert-delete").await;
    h.check("alert_confirm_delete_dismissed", got == Ok(false), format!("{how}: {got:?}"));

    let answer = start(alerts.ask_save_changes(strings::documents::save_changes_question("Untitled")));
    let got = answer_alert(h, window, &answer, Answer::Escape, "alert-save-changes").await;
    h.check("alert_ask_save_changes_dismissed", got == Ok(CloseChoice::Cancel), format!("{how}: {got:?}"));

    let answer = start(alerts.confirm_replace(strings::exports::folder_exists("Notes.textbundle")));
    let got = answer_alert(h, window, &answer, Answer::Escape, "alert-replace").await;
    h.check("alert_confirm_replace_dismissed", got == Ok(false), format!("{how}: {got:?}"));

    // The name prompt's validation: blank turns the accept button off, a refused character marks
    // the entry, a good name is accepted with Return.
    let answer = start(alerts.prompt_name(
        strings::books::NEW_CHAPTER_TITLE.into(),
        strings::books::NEW_CHAPTER_MESSAGE.into(),
        "Draft".into(),
        strings::buttons::CREATE.into(),
    ));
    if h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await {
        let dialog = window.visible_dialog().and_downcast::<adw::AlertDialog>();
        let entry = dialog.as_ref().and_then(|d| d.extra_child()).and_downcast::<gtk::Entry>();
        let judged = match (&dialog, &entry) {
            (Some(dialog), Some(entry)) => {
                let initially_on = dialog.is_response_enabled("accept");
                entry.set_text("   ");
                let blank_off = !dialog.is_response_enabled("accept");
                entry.set_text("a/b");
                let refused_marked = dialog.is_response_enabled("accept") && entry.has_css_class("error");
                entry.set_text("Final");
                let fine = dialog.is_response_enabled("accept") && !entry.has_css_class("error");
                let detail = format!(
                    "\"Draft\" on {initially_on}, blank off {blank_off}, \"a/b\" marked {refused_marked}, \"Final\" fine {fine}"
                );
                if initially_on && blank_off && refused_marked && fine { Ok(detail) } else { Err(detail) }
            }
            _ => Err("not an alert dialog with an entry".to_string()),
        };
        h.check("alert_prompt_name_validates", judged.is_ok(), judged.clone().unwrap_or_else(|e| e));
        if h.input() {
            let got = answer_alert(h, window, &answer, Answer::Return, "alert-prompt-accept").await;
            h.check("alert_prompt_name_return_accepts", got == Ok(Some("Final".to_string())), format!("{got:?}"));
        } else {
            let _ = answer_alert(h, window, &answer, Answer::Escape, "alert-prompt-closed").await;
        }
    } else {
        h.check("alert_prompt_name_validates", false, "no dialog was presented");
    }

    // Return takes the default: Cancel for Delete and Replace, Save for the close question.
    if h.input() {
        let answer =
            start(alerts.confirm_delete(
                strings::books::delete_question("Chapter"),
                strings::books::DELETE_CHAPTER_MESSAGE.into(),
            ));
        let got = answer_alert(h, window, &answer, Answer::Return, "alert-delete-return").await;
        h.check("alert_confirm_delete_defaults_to_cancel", got == Ok(false), format!("{got:?}"));
        let answer = start(alerts.confirm_replace(strings::exports::folder_exists("Notes.textbundle")));
        let got = answer_alert(h, window, &answer, Answer::Return, "alert-replace-return").await;
        h.check("alert_confirm_replace_defaults_to_cancel", got == Ok(false), format!("{got:?}"));
        let answer = start(alerts.ask_save_changes(strings::documents::save_changes_question("Untitled")));
        let got = answer_alert(h, window, &answer, Answer::Return, "alert-save-return").await;
        h.check("alert_ask_save_changes_defaults_to_save", got == Ok(CloseChoice::Save), format!("{got:?}"));
    }

    // One at a time: the second waits for the first.
    opened.borrow_mut().clear();
    let first = start(alerts.warn("First".into(), String::new()));
    let second = start(alerts.warn("Second".into(), String::new()));
    let one_up = h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await;
    h.sleep(Duration::from_millis(200)).await;
    let count_first = window.dialogs().n_items();
    let got_first = answer_alert(h, window, &first, Answer::Escape, "alert-queue-first").await;
    let count_second = if h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await {
        window.dialogs().n_items()
    } else {
        0
    };
    let second_waited = second.borrow().is_none();
    let got_second = answer_alert(h, window, &second, Answer::Escape, "alert-queue-second").await;
    let flags = opened.borrow().clone();
    h.check(
        "alerts_one_at_a_time",
        one_up
            && count_first == 1
            && got_first.is_ok()
            && count_second == 1
            && second_waited
            && got_second.is_ok()
            && flags == [true, false],
        format!("dialogs up: {count_first} then {count_second}; second waited {second_waited}; open_changed {flags:?}"),
    );

    // A caller that gives up — while in line, or with its dialog up — never blocks the window.
    opened.borrow_mut().clear();
    let shown = start(alerts.warn("Shown".into(), String::new()));
    let up = h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await;
    let in_line = glib::spawn_future_local(alerts.warn("Gives up in line".into(), String::new()));
    h.sleep(Duration::from_millis(100)).await;
    // Aborted and released: GLib drops the future once nothing holds its source.
    in_line.abort();
    drop(in_line);
    let got_shown = answer_alert(h, window, &shown, Answer::Escape, "alert-dropped-first").await;
    let after_line = h.wait_until(Duration::from_millis(500), || window.visible_dialog().is_some()).await;
    let mid_dialog = glib::spawn_future_local(alerts.warn("Gives up mid-dialog".into(), String::new()));
    let mid_up = h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await;
    mid_dialog.abort();
    drop(mid_dialog);
    let mid_gone = h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_none()).await;
    let last = start(alerts.warn("Still answered".into(), String::new()));
    let got_last = answer_alert(h, window, &last, Answer::Escape, "alert-dropped-last").await;
    let flags = opened.borrow().clone();
    h.check(
        "alerts_survive_a_caller_that_gives_up",
        up && got_shown.is_ok()
            && !after_line
            && mid_up
            && mid_gone
            && got_last.is_ok()
            && flags == [true, false, true, false, true, false],
        format!(
            "the abandoned one never showed: {}; mid-dialog closed: {mid_gone}; next answered: {got_last:?}; open_changed {flags:?}",
            !after_line
        ),
    );
}

/// The file chooser GTK opened for `parent` (its own dialog under Xvfb: no portal here).
fn chooser_of(parent: &adw::ApplicationWindow) -> Option<gtk::Window> {
    let parent: &gtk::Window = parent.upcast_ref();
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.is_visible() && w.transient_for().as_ref() == Some(parent))
}

/// Waits for the chooser, then presses `keys` in it (input tier) or closes it (no input); `typed`
/// goes into the field that has the focus after the first key.
async fn drive_chooser(
    h: &Harness,
    parent: &adw::ApplicationWindow,
    keys: &[&str],
    typed: Option<&str>,
    shot: &str,
) -> Result<(), String> {
    if !h.wait_until(DIALOG_LIMIT, || chooser_of(parent).is_some()).await {
        return Err("no file chooser appeared".to_string());
    }
    h.sleep(Duration::from_millis(500)).await;
    h.screenshot(shot).await;
    let chooser = chooser_of(parent).ok_or("the chooser went away by itself")?;
    if !h.input() {
        chooser.close();
        return Ok(());
    }
    if !h.focus(&chooser).await {
        return Err(format!("xdotool could not focus \u{201C}{}\u{201D}", chooser.title().unwrap_or_default()));
    }
    for (n, key) in keys.iter().enumerate() {
        // Into the focused location entry at once: GTK's file-name entry completes inline on an
        // idle, and keys arriving one by one get typed into its completions.
        if n == 1
            && let Some(text) = typed
        {
            let editable = gtk::prelude::RootExt::focus(&chooser).and_then(|w| w.downcast::<gtk::Text>().ok());
            let Some(editable) = editable else { return Err("no text field has the focus".to_string()) };
            editable.set_text(text);
            editable.set_position(-1);
            h.sleep(Duration::from_millis(300)).await;
        }
        if n + 1 == keys.len() && n > 0 {
            h.screenshot(&format!("{shot}-typed")).await;
        }
        if !h.key(key).await {
            return Err(format!("xdotool could not press {key}"));
        }
        h.sleep(Duration::from_millis(300)).await;
    }
    Ok(())
}

async fn picker_rows(h: &Harness, window: &adw::ApplicationWindow, alerts: Rc<dyn Alerts>, folder: &Path) {
    let toplevel: gtk::Window = window.clone().upcast();
    let pickers = GtkPickers::new(&toplevel, Rc::clone(&alerts));
    pickers.set_document_folder(Some(&string(folder)));
    let flags: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let flags = Rc::clone(&flags);
        pickers.open_changed().connect(move |open: &bool| flags.borrow_mut().push(*open));
    }
    let seam: Rc<dyn Pickers> = pickers.clone();
    let how = if h.input() { "Escape through xdotool" } else { "closed (no input)" };

    // Dismissed: the seam's "cancelled".
    let answer = start(seam.open_files(document_loader::open_extensions()));
    let driven = drive_chooser(h, window, &["Escape"], None, "picker-open").await;
    let got = await_answer(h, &answer).await;
    close_leftover_chooser(h, window).await;
    h.check(
        "picker_open_dismissed",
        driven.is_ok() && got.as_ref().is_some_and(Vec::is_empty),
        format!("{how}: {driven:?}, {got:?}"),
    );

    let answer = start(seam.save_file("Draft".into(), document_loader::save_file_type_choices(), ".md".into()));
    let driven = drive_chooser(h, window, &["Escape"], None, "picker-save").await;
    let got = await_answer(h, &answer).await;
    close_leftover_chooser(h, window).await;
    h.check("picker_save_dismissed", driven.is_ok() && got == Some(None), format!("{how}: {driven:?}, {got:?}"));

    let title = strings::books::CHOOSE_BOOK_FOLDER;
    let answer = start(seam.pick_folder(Some(title.into())));
    let titled =
        h.wait_until(DIALOG_LIMIT, || chooser_of(window).is_some_and(|c| c.title().as_deref() == Some(title))).await;
    let driven = drive_chooser(h, window, &["Escape"], None, "picker-folder").await;
    let got = await_answer(h, &answer).await;
    close_leftover_chooser(h, window).await;
    h.check(
        "picker_folder_dismissed",
        titled && driven.is_ok() && got == Some(None),
        format!("{how}: titled {titled}, {driven:?}, {got:?}"),
    );
    h.check(
        "pickers_raise_open_changed",
        *flags.borrow() == [true, false, true, false, true, false],
        format!("{:?}", flags.borrow()),
    );

    // A bare name GTK returned is completed with the default extension, and when that name is
    // taken md asks first (GTK asked only about the name as typed) — through the real alerts.
    // GTK's own name entry offers "Draft.md" as a completion while it exists, so the chooser
    // half of this is the row below, with the name free.
    let draft = folder.join("Draft.md");
    let _ = std::fs::write(&draft, "draft\n");
    let answer = start(finish_save(Rc::clone(&alerts), string(&folder.join("Draft")), ".md".into()));
    let asked = if h.wait_until(DIALOG_LIMIT, || window.visible_dialog().is_some()).await {
        window.visible_dialog().and_downcast::<adw::AlertDialog>().map(|d| d.heading().map(|t| t.to_string()))
    } else {
        None
    };
    let got = answer_alert(h, window, &answer, Answer::Escape, "picker-save-replace").await;
    let question = md_logic::strings::documents::file_exists_question("Draft.md");
    h.check(
        "save_asks_before_replacing_a_completed_name",
        asked == Some(Some(question.clone()))
            && got == Ok(None)
            && std::fs::read(&draft).ok().as_deref() == Some(b"draft\n".as_slice()),
        format!("asked {asked:?}; {how}: {got:?}"),
    );

    if !h.input() {
        return;
    }

    // Chosen through the chooser itself: Ctrl+L, a location, Return.
    let answer = start(seam.open_files(document_loader::open_extensions()));
    let driven = drive_chooser(h, window, &["ctrl+l", "Return"], Some(&string(&draft)), "picker-open-typed").await;
    let got = await_answer(h, &answer).await;
    close_leftover_chooser(h, window).await;
    h.check(
        "picker_open_returns_the_chosen_file",
        got.as_deref() == Some([string(&draft)].as_slice()),
        format!("{driven:?}, {got:?}"),
    );

    // Return in the save dialog takes the suggested bare name; md completes it.
    let _ = std::fs::remove_file(&draft);
    let answer = start(seam.save_file("Draft".into(), document_loader::save_file_type_choices(), ".md".into()));
    let driven = drive_chooser(h, window, &["Return"], None, "picker-save-completed").await;
    let got = await_answer(h, &answer).await;
    close_leftover_chooser(h, window).await;
    h.check("picker_save_completes_a_bare_name", got == Some(Some(string(&draft))), format!("{driven:?}, {got:?}"));
}

/// A chooser a failed row left up is closed, so it cannot take the next row's keys.
async fn close_leftover_chooser(h: &Harness, window: &adw::ApplicationWindow) {
    if let Some(chooser) = chooser_of(window) {
        chooser.close();
        h.wait_until(DIALOG_LIMIT, || chooser_of(window).is_none()).await;
    }
}

/// The answer of a picker, or `None` when it never came.
async fn await_answer<T: Clone + 'static>(h: &Harness, answer: &Rc<RefCell<Option<T>>>) -> Option<T> {
    h.wait_until(DIALOG_LIMIT, || answer.borrow().is_some()).await;
    answer.borrow().clone()
}
