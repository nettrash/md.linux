//! Port of md.win `Activation/ActivationRouter.cs`.
//!
//! shell-design.md §1.2's table as code: an [`ActivationDescription`] in, an ordered list of
//! [`ActivationAction`]s out. Pure — [`route`] reads no disk, creates no window and asks no
//! registry, so every row of the table is a unit test.
//!
//! The "a path already open activates that window instead of opening a second one" rule is the
//! second half, [`resolve`]: it needs the live registry, so it is a separate call the window
//! manager makes with the batch [`route`] produced.

use std::collections::HashSet;

use md_core::export::text_bundle;

use super::activation_action::ActivationAction;
use super::activation_description::{ActivationDescription, ActivationItem, ActivationKind};
use crate::documents::document_loader::ends_with_ignoring_ascii_case;
use crate::documents::file_names;
use crate::seams::{DocumentRegistry, FileIdentity, WindowId};

/// The §1.2 table. File activations classify each item; every other kind — Launch, Other — routes
/// as a launch, which is the table's last row.
pub fn route(description: &ActivationDescription) -> Vec<ActivationAction> {
    if description.kind == ActivationKind::File {
        let actions: Vec<ActivationAction> = description.items.iter().filter_map(classify).collect();
        // Every item was a folder we do not open (§1.2: "other folders ignored"). A forwarded
        // activation that asked for nothing does nothing; the first activation still has to
        // produce a window, or the process would run with none.
        if !actions.is_empty() {
            return actions;
        }
        return if description.is_first { launch(description) } else { Vec::new() };
    }
    launch(description)
}

/// One item of a file activation, or one command-line path: a `.textbundle` folder imports, any
/// other folder is ignored, a `.textpack` imports, anything else opens as a document (§1.2, §6.5).
///
/// [`ActivationItem::is_folder`] is the file system's own answer and it decides first: a bundle is
/// a folder, so a plain FILE that happens to be called `notes.textbundle` is a document and opens
/// as one — feeding it to the folder importer could only ever fail. The suffixes match ignoring
/// ASCII case, on the name only.
pub fn classify(item: &ActivationItem) -> Option<ActivationAction> {
    let path = &item.path;
    if file_names::is_blank(path) {
        return None;
    }
    // The name, not the whole path: a document inside /Books/My.textbundle/ is still a document.
    let name = file_names::name_of(path);
    if item.is_folder {
        return ends_with_ignoring_ascii_case(name, text_bundle::BUNDLE_EXTENSION)
            .then(|| ActivationAction::ImportTextBundleFolder(path.clone())); // §1.2: other folders ignored
    }
    if ends_with_ignoring_ascii_case(name, text_bundle::PACK_EXTENSION) {
        return Some(ActivationAction::ImportTextPack(path.clone()));
    }
    Some(ActivationAction::OpenPath(path.clone()))
}

/// The same question where nobody has told us what the path is — a command-line argument, which
/// is a string and not a `GFile` the app could ask. The name is then the only evidence there is,
/// so a `.textbundle` is taken at its word.
fn from_command_line(path: &str) -> ActivationItem {
    if ends_with_ignoring_ascii_case(file_names::name_of(path), text_bundle::BUNDLE_EXTENSION) {
        ActivationItem::folder(path)
    } else {
        ActivationItem::file(path)
    }
}

/// Turn "open this file" into "activate the window that already has it" where one does, and drop a
/// file named twice in one activation (a multi-select can repeat a file through two spellings; two
/// windows on one file would give it two writers). Imports are untitled documents and are never
/// owned, so the same `.textpack` twice really does give two windows — the Mac's behaviour for the
/// same Example opened twice.
///
/// LINUX DECISION: the paths compare exactly after canonicalising (md.win: `OrdinalIgnoreCase`).
pub fn resolve(
    actions: impl IntoIterator<Item = ActivationAction>,
    registry: &dyn DocumentRegistry,
    identity: &dyn FileIdentity,
) -> Vec<ActivationAction> {
    let mut resolved = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut seen_windows: HashSet<WindowId> = HashSet::new();
    for action in actions {
        let ActivationAction::OpenPath(path) = &action else {
            resolved.push(action);
            continue;
        };
        let canonical = identity.canonical(path);
        if let Some(owner) = registry.owning(&canonical) {
            if seen_windows.insert(owner) {
                resolved.push(ActivationAction::Focus(owner));
            }
            continue;
        }
        if seen_paths.insert(canonical) {
            resolved.push(action);
        }
    }
    resolved
}

fn launch(description: &ActivationDescription) -> Vec<ActivationAction> {
    // Classified, not blindly opened: a .textpack on a command line is the same import a
    // double-click gives (§6.5). A folder cannot be told from a file without a disk, so a
    // command-line path is a file unless its name says .textbundle.
    let actions: Vec<ActivationAction> =
        description.argument_paths().iter().filter_map(|path| classify(&from_command_line(path))).collect();
    if !actions.is_empty() {
        return actions;
    }
    // A forwarded launch with nothing to open is "the user asked for md again": a new untitled
    // window, which is what clicking the Dock icon gives on the Mac (⌘N). Only the first
    // activation of the process may bring the last session back.
    if description.is_first && description.has_restorable_session {
        vec![ActivationAction::RestoreSession]
    } else {
        vec![ActivationAction::OpenUntitled]
    }
}
