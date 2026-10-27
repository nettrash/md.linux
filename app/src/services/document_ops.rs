//! Rename and Move To through `gio::File::move_` (shell-design §6.4) — it refuses an existing
//! target and copies + deletes across file systems — and the TextBundle / TextPack import helpers
//! (§6.5). Errors come back as the message an alert shows.
//!
//! [`rename`] and [`move_to`] are the file operations; [`rename_with_prompt`] and
//! [`move_with_picker`] are md.win's whole `RenameAsync` / `MoveToAsync` around them — the prompt
//! or the folder dialog, the name rule, the flush before the file changes identity under its
//! watcher, the alert on failure — and hand back the new path for the window to
//! `TextFileSession::retarget` to (then recents and `apply_memory` follow from the session's
//! `identity_changed`, §6.4).
#![allow(dead_code)] // S0 stub: removed at the integration gate (P1) — this file's callers are P1's and P5's stubs until then

use gio::prelude::*;
use md_core::book::book_folder;
use md_logic::activation::ActivationItem;
use md_logic::documents::document_loader::{self, DocumentKind, LoadedDocument};
use md_logic::documents::{LocalFileIdentity, file_names};
use md_logic::exec::LocalBoxFuture;
use md_logic::seams::{Alerts, FileIdentity, Pickers};
use md_logic::strings;
use std::rc::Rc;

use crate::services::file_system::StdFileSystem;

/// Renames the file in its folder; the new path, or the message. `new_file_name` is a whole name
/// (extension included — see [`renamed_file_name`]); one the Mac's rule refuses is
/// `strings::documents::INVALID_NAME_MESSAGE`. An existing target is refused, never replaced.
/// Renaming to the current name is a no-op that answers the path.
pub fn rename(path: &str, new_file_name: &str) -> Result<String, String> {
    if !file_names::validate(Some(new_file_name)) {
        return Err(strings::documents::INVALID_NAME_MESSAGE.to_string());
    }
    if new_file_name == file_names::name_of(path) {
        return Ok(path.to_string());
    }
    let target = file_names::combine(file_names::directory_of(path), new_file_name);
    move_file(path, &target)
}

/// Moves the file into `folder` under its own name; the new path, or the message. Across file
/// systems GIO copies and deletes. An existing file of that name in `folder` is refused. A move
/// into the folder the file is already in is a no-op that answers the path.
pub fn move_to(path: &str, folder: &str) -> Result<String, String> {
    let identity = LocalFileIdentity::INSTANCE;
    if identity.canonical(file_names::directory_of(path)) == identity.canonical(folder) {
        return Ok(path.to_string());
    }
    let target = file_names::combine(folder, file_names::name_of(path));
    move_file(path, &target)
}

/// `g_file_move` without `OVERWRITE`: `G_IO_ERROR_EXISTS` rather than a replacement, and the
/// copy + delete fallback when `rename(2)` cannot cross the file systems.
fn move_file(from: &str, to: &str) -> Result<String, String> {
    gio::File::for_path(from)
        .move_(&gio::File::for_path(to), gio::FileCopyFlags::NONE, gio::Cancellable::NONE, None)
        .map(|()| to.to_string())
        .map_err(|err| err.message().to_string())
}

/// Rename's name rule (§6.4): the typed stem, trimmed as every md prompt trims it, with the
/// file's own extension kept (`file_names::with_extension_of`); `INVALID_NAME_MESSAGE` for a
/// blank stem or one the Mac's rule refuses.
pub fn renamed_file_name(path: &str, typed_stem: &str) -> Result<String, String> {
    let stem = book_folder::trimmed_prompt_name(typed_stem);
    let name = file_names::with_extension_of(file_names::name_of(path), stem);
    if !file_names::validate(Some(stem)) || !file_names::validate(Some(&name)) {
        return Err(strings::documents::INVALID_NAME_MESSAGE.to_string());
    }
    Ok(name)
}

/// File ▸ Rename…: the prompt (`RENAME_TITLE` / `RENAME_MESSAGE`, the current stem), the name
/// rule, `flush` (the window's `flush_now(false)`: write first, the file is about to change
/// identity), then [`rename`]. `Some(new path)` for the window to retarget to; `None` when the
/// writer cancelled, kept the name, or an alert (`COULD_NOT_RENAME`) already said why not.
pub fn rename_with_prompt(
    alerts: Rc<dyn Alerts>,
    path: String,
    flush: impl FnOnce() + 'static,
) -> LocalBoxFuture<'static, Option<String>> {
    Box::pin(async move {
        let typed = alerts
            .prompt_name(
                strings::documents::RENAME_TITLE.to_string(),
                strings::documents::RENAME_MESSAGE.to_string(),
                file_names::stem_of(&path).to_string(),
                strings::documents::RENAME_TITLE.to_string(),
            )
            .await?;
        let name = match renamed_file_name(&path, &typed) {
            Ok(name) => name,
            Err(message) => {
                alerts.warn(strings::documents::COULD_NOT_RENAME.to_string(), message).await;
                return None;
            }
        };
        if name == file_names::name_of(&path) {
            return None;
        }
        flush();
        match rename(&path, &name) {
            Ok(new_path) => Some(new_path),
            Err(message) => {
                alerts.warn(strings::documents::COULD_NOT_RENAME.to_string(), message).await;
                None
            }
        }
    })
}

/// File ▸ Move To…: the folder dialog, `flush`, then [`move_to`]. `Some(new path)` for the window
/// to retarget to; `None` when cancelled, when the folder is the file's own, or after the alert
/// (md.win's `COULD_NOT_RENAME`: the family has no move-specific title).
pub fn move_with_picker(
    pickers: Rc<dyn Pickers>,
    alerts: Rc<dyn Alerts>,
    path: String,
    flush: impl FnOnce() + 'static,
) -> LocalBoxFuture<'static, Option<String>> {
    Box::pin(async move {
        let folder = pickers.pick_folder(None).await?;
        flush();
        match move_to(&path, &folder) {
            Ok(new_path) if new_path == path => None,
            Ok(new_path) => Some(new_path),
            Err(message) => {
                alerts.warn(strings::documents::COULD_NOT_RENAME.to_string(), message).await;
                None
            }
        }
    })
}

/// File ▸ Duplicate's window title (§6.4): `"{title} copy"` — the new untitled window holds the
/// same text and is dirty from the start (so it gets a draft at once).
pub fn duplicate_title(title: &str) -> String {
    strings::duplicate_title(title)
}

/// A `.textpack` file or a `.textbundle` folder read for import (§6.5): its text, the encoding
/// it was written in, and its title (the bundle's stem) — what
/// `TextFileSession::import_bundle(path, text, encoding)` takes. The error is (alert title,
/// message): `TEXT_PACK_UNREADABLE` for a bundle that is not one, `COULD_NOT_OPEN` with the
/// system's message for one that cannot be read.
pub fn import(path: &str) -> Result<LoadedDocument, (String, String)> {
    let load = document_loader::load(path, &StdFileSystem::new());
    match load.document {
        Some(document) if document.kind != DocumentKind::PlainText => Ok(document),
        Some(_) => Err((strings::documents::TEXT_PACK_UNREADABLE.to_string(), String::new())),
        None => Err((load.alert_title().unwrap_or(strings::documents::COULD_NOT_OPEN).to_string(), load.error_text)),
    }
}

/// One file handed to md by `open` or a drop, as md-logic's activation router classifies it
/// (`activation_router::classify`: a `.textbundle` folder imports, other folders are ignored, a
/// `.textpack` imports, anything else opens): its local path, and whether it is a folder — the
/// file system's answer, never the name's. `None` for a file with no local path (the caller alerts
/// `COULD_NOT_OPEN`, §1.3) or a name that is not UTF-8.
pub fn activation_item(file: &gio::File) -> Option<ActivationItem> {
    let path = file.path()?.into_os_string().into_string().ok()?;
    let is_folder =
        file.query_file_type(gio::FileQueryInfoFlags::NONE, gio::Cancellable::NONE) == gio::FileType::Directory;
    Some(ActivationItem { path, is_folder })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::file_system::tests::TempFolder;
    use md_core::export::text_bundle;
    use md_logic::exec::LocalExecutor;
    use md_logic::testing::{FakeAlerts, FakePickers};
    use std::cell::Cell;

    #[test]
    fn rename_keeps_the_folder_and_refuses_an_existing_target() {
        let temp = TempFolder::new("rename");
        let (a, b) = (temp.path("a.md"), temp.path("b.md"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let err = rename(&a, "b.md").unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(std::fs::read(&a).unwrap(), b"a");
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(rename(&a, "c.md"), Ok(temp.path("c.md")));
        assert!(!std::path::Path::new(&a).exists());
        assert_eq!(rename(&temp.path("c.md"), "c.md"), Ok(temp.path("c.md")));
        assert_eq!(rename(&temp.path("c.md"), "x/y.md"), Err(strings::documents::INVALID_NAME_MESSAGE.to_string()));
        assert_eq!(rename(&temp.path("c.md"), "x:y.md"), Err(strings::documents::INVALID_NAME_MESSAGE.to_string()));
        assert!(rename(&temp.path("missing.md"), "z.md").is_err());
    }

    #[test]
    fn move_to_refuses_an_existing_name_and_stays_put_in_its_own_folder() {
        let temp = TempFolder::new("move-to");
        std::fs::create_dir(temp.path("sub")).unwrap();
        let doc = temp.path("doc.md");
        std::fs::write(&doc, "text").unwrap();
        assert_eq!(move_to(&doc, &temp.0.to_string_lossy()), Ok(doc.clone()));
        std::fs::write(temp.path("sub/doc.md"), "other").unwrap();
        assert!(move_to(&doc, &temp.path("sub")).is_err());
        assert_eq!(std::fs::read(temp.path("sub/doc.md")).unwrap(), b"other");
        std::fs::remove_file(temp.path("sub/doc.md")).unwrap();
        assert_eq!(move_to(&doc, &temp.path("sub")), Ok(temp.path("sub/doc.md")));
        assert!(!std::path::Path::new(&doc).exists());
        assert_eq!(std::fs::read(temp.path("sub/doc.md")).unwrap(), b"text");
    }

    #[test]
    fn the_rename_rule_keeps_the_extension() {
        assert_eq!(renamed_file_name("/d/notes.md", "ideas"), Ok("ideas.md".to_string()));
        assert_eq!(renamed_file_name("/d/notes.md", "  ideas  "), Ok("ideas.md".to_string()));
        assert_eq!(renamed_file_name("/d/diagram.puml", "flow.v2"), Ok("flow.v2.puml".to_string()));
        assert_eq!(renamed_file_name("/d/README", "readme"), Ok("readme".to_string()));
        assert_eq!(renamed_file_name("/d/Заметки.md", "Идеи"), Ok("Идеи.md".to_string()));
        for bad in ["", "   ", "a/b", "a:b", ".."] {
            assert_eq!(
                renamed_file_name("/d/notes.md", bad),
                Err(strings::documents::INVALID_NAME_MESSAGE.to_string()),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn rename_with_prompt_flushes_renames_and_alerts() {
        let temp = TempFolder::new("rename-prompt");
        let doc = temp.path("notes.md");
        std::fs::write(&doc, "x").unwrap();
        std::fs::write(temp.path("taken.md"), "y").unwrap();
        let executor = LocalExecutor::new();
        let alerts = Rc::new(FakeAlerts::new());
        let flushed = Rc::new(Cell::new(0));
        let flush = {
            let flushed = Rc::clone(&flushed);
            move || flushed.set(flushed.get() + 1)
        };

        // Cancelled: nothing happens.
        alerts.queue_name_answer(None);
        assert_eq!(executor.block_on(rename_with_prompt(alerts.clone(), doc.clone(), flush.clone())), None);
        let prompt = &alerts.name_prompts()[0];
        assert_eq!(prompt.title, strings::documents::RENAME_TITLE);
        assert_eq!(prompt.message, strings::documents::RENAME_MESSAGE);
        assert_eq!(prompt.initial, "notes");
        assert_eq!(prompt.accept_label, strings::documents::RENAME_TITLE);
        // The same name: nothing to do.
        alerts.queue_name_answer(Some(" notes "));
        assert_eq!(executor.block_on(rename_with_prompt(alerts.clone(), doc.clone(), flush.clone())), None);
        assert_eq!(flushed.get(), 0);
        // An invalid name: the alert, no flush.
        alerts.queue_name_answer(Some("a/b"));
        assert_eq!(executor.block_on(rename_with_prompt(alerts.clone(), doc.clone(), flush.clone())), None);
        assert_eq!(flushed.get(), 0);
        let warning = alerts.warnings().last().cloned().unwrap();
        assert_eq!(warning.title, strings::documents::COULD_NOT_RENAME);
        assert_eq!(warning.message, strings::documents::INVALID_NAME_MESSAGE);
        // Taken: flushed, refused, alerted, file untouched.
        alerts.queue_name_answer(Some("taken"));
        assert_eq!(executor.block_on(rename_with_prompt(alerts.clone(), doc.clone(), flush.clone())), None);
        assert_eq!(flushed.get(), 1);
        assert_eq!(alerts.warnings().len(), 2);
        assert!(std::path::Path::new(&doc).exists());
        // Renamed.
        alerts.queue_name_answer(Some("ideas"));
        assert_eq!(
            executor.block_on(rename_with_prompt(alerts.clone(), doc.clone(), flush)),
            Some(temp.path("ideas.md"))
        );
        assert_eq!(flushed.get(), 2);
        assert_eq!(std::fs::read(temp.path("ideas.md")).unwrap(), b"x");
    }

    #[test]
    fn move_with_picker_moves_into_the_chosen_folder() {
        let temp = TempFolder::new("move-picker");
        std::fs::create_dir(temp.path("archive")).unwrap();
        let doc = temp.path("doc.md");
        std::fs::write(&doc, "x").unwrap();
        let executor = LocalExecutor::new();
        let alerts = Rc::new(FakeAlerts::new());
        let pickers = Rc::new(FakePickers::new());
        let flushed = Rc::new(Cell::new(false));
        let flush = {
            let flushed = Rc::clone(&flushed);
            move || flushed.set(true)
        };
        // Cancelled.
        pickers.queue_folder_answer(None);
        assert_eq!(
            executor.block_on(move_with_picker(pickers.clone(), alerts.clone(), doc.clone(), flush.clone())),
            None
        );
        assert!(!flushed.get());
        assert_eq!(pickers.folder_calls(), [None]);
        // Its own folder: nothing moves.
        pickers.queue_folder_answer(Some(&temp.0.to_string_lossy()));
        assert_eq!(
            executor.block_on(move_with_picker(pickers.clone(), alerts.clone(), doc.clone(), flush.clone())),
            None
        );
        assert!(std::path::Path::new(&doc).exists());
        // Moved.
        pickers.queue_folder_answer(Some(&temp.path("archive")));
        assert_eq!(
            executor.block_on(move_with_picker(pickers.clone(), alerts.clone(), doc.clone(), flush)),
            Some(temp.path("archive/doc.md"))
        );
        assert!(flushed.get());
        assert!(alerts.warnings().is_empty());
        // Into a folder that is gone: the alert.
        pickers.queue_folder_answer(Some(&temp.path("gone")));
        let moved = temp.path("archive/doc.md");
        assert_eq!(executor.block_on(move_with_picker(pickers, alerts.clone(), moved, || {})), None);
        assert_eq!(alerts.warnings()[0].title, strings::documents::COULD_NOT_RENAME);
    }

    #[test]
    fn imports_are_titled_after_the_bundle() {
        let temp = TempFolder::new("import");
        let wrapper = text_bundle::bundle_wrapper("# Field notes\n\nПривет.\n", Vec::new());
        let pack = temp.path("Field Notes.textpack");
        std::fs::write(&pack, wrapper.to_text_pack("Field Notes").unwrap()).unwrap();
        let folder = temp.path("Trip.textbundle");
        wrapper.write(std::path::Path::new(&folder)).unwrap();

        let packed = import(&pack).expect("the pack imports");
        assert_eq!(packed.kind, DocumentKind::TextPack);
        assert_eq!(packed.title, "Field Notes");
        assert_eq!(packed.text, "# Field notes\n\nПривет.\n");
        let bundled = import(&folder).expect("the bundle imports");
        assert_eq!(bundled.kind, DocumentKind::TextBundleFolder);
        assert_eq!(bundled.title, "Trip");

        let broken = temp.path("Broken.textpack");
        std::fs::write(&broken, "not a zip").unwrap();
        assert_eq!(import(&broken).unwrap_err().0, strings::documents::TEXT_PACK_UNREADABLE);
        let plain = temp.path("plain.md");
        std::fs::write(&plain, "text").unwrap();
        assert_eq!(import(&plain).unwrap_err().0, strings::documents::TEXT_PACK_UNREADABLE);
        assert_eq!(import(&temp.path("missing.textpack")).unwrap_err().0, strings::documents::COULD_NOT_OPEN);
    }

    #[test]
    fn activation_items_ask_the_file_system() {
        let temp = TempFolder::new("activation");
        let folder = temp.path("Trip.textbundle");
        std::fs::create_dir(&folder).unwrap();
        let file = temp.path("not-a-bundle.textbundle");
        std::fs::write(&file, "").unwrap();
        assert_eq!(activation_item(&gio::File::for_path(&folder)), Some(ActivationItem::folder(&folder)));
        assert_eq!(activation_item(&gio::File::for_path(&file)), Some(ActivationItem::file(&file)));
        assert_eq!(activation_item(&gio::File::for_uri("https://example.com/x.md")), None);
    }

    #[test]
    fn a_duplicate_is_titled_after_its_original() {
        assert_eq!(duplicate_title("notes"), "notes copy");
        assert_eq!(duplicate_title("Untitled 2"), "Untitled 2 copy");
    }
}
