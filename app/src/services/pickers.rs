//! `GtkPickers` — md-logic's `Pickers` over `gtk::FileDialog` (shell-design §6.1): Open with one
//! filter (never the `*_text_file` variants: md's codec decides the encoding), Save with the
//! `DocumentLoader` choices and the default-extension rule, folders for books, bundles and Move To.
//!
//! Every dialog is modal to its window. GTK decides between its own chooser (what Xvfb shows) and
//! the xdg-desktop-portal one (a desktop); either way no Encoding row appears, because md never
//! calls the `*_text_file` variants. Escape is `DialogError::Dismissed` and comes back as the
//! seam's "cancelled" (an empty list, `None`), like every other failure of the dialog itself.
//!
//! GTK's save dialog does not create the file (unlike WinRT's, which md.win's flow deletes again on
//! a failed write), and it confirms a replacement only for the name the user typed. When md
//! appends the default extension to a bare name, the name it will write is a different one, so md
//! asks about that one itself ([`Alerts::confirm_replace`]).

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use md_logic::documents::file_names;
use md_logic::event::Event;
use md_logic::exec::LocalBoxFuture;
use md_logic::seams::{Alerts, FileTypeChoice, Pickers};
use md_logic::strings::documents::{OPEN_FILTER, file_exists_question};

/// The MIME types the Open filter accepts besides the extensions (§6.1).
const OPEN_MIME_TYPES: [&str; 3] = ["text/markdown", "text/x-markdown", "text/plain"];

/// The dialogs of one window (modal to it).
pub struct GtkPickers {
    window: gtk::Window,
    /// For the replace question GTK does not ask when md appends the default extension.
    alerts: Rc<dyn Alerts>,
    /// Where the dialogs start: the document's folder (§6.1). `None` lets GTK choose.
    folder: RefCell<Option<String>>,
    open_changed: Rc<Event<bool>>,
    /// Self-test builds: answers queued by the self-test, taken by the next dialog instead of
    /// showing it — so a group can drive a whole flow (a book export into a file) through this
    /// window's real pickers. GTK's dialogs themselves are proven by the `documents` group.
    #[cfg(feature = "selftest")]
    answers: RefCell<std::collections::VecDeque<Option<String>>>,
}

impl GtkPickers {
    pub fn new(window: &gtk::Window, alerts: Rc<dyn Alerts>) -> Rc<GtkPickers> {
        Rc::new(GtkPickers {
            window: window.clone(),
            alerts,
            folder: RefCell::new(None),
            open_changed: Rc::new(Event::new()),
            #[cfg(feature = "selftest")]
            answers: RefCell::new(std::collections::VecDeque::new()),
        })
    }

    /// Self-test builds: the next dialog of this window is answered with `answer` (a path, or
    /// `None` for Cancel) without being shown; `open_changed` is raised around it as for a real one.
    #[cfg(feature = "selftest")]
    pub fn queue_answer(&self, answer: Option<&str>) {
        self.answers.borrow_mut().push_back(answer.map(str::to_string));
    }

    /// The queued answer for this dialog, if the self-test left one.
    #[cfg(feature = "selftest")]
    fn queued(&self) -> Option<Option<String>> {
        self.answers.borrow_mut().pop_front()
    }

    /// The folder every dialog of this window starts in — the document's own (§6.1: Save As starts
    /// beside the document). The window sets it whenever its document gains or changes a path;
    /// `None` (an untitled document) leaves the choice to GTK.
    pub fn set_document_folder(&self, folder: Option<&str>) {
        *self.folder.borrow_mut() = folder.filter(|f| !f.is_empty()).map(str::to_string);
    }

    /// Args: whether a file dialog of this window is up — the same role as
    /// `AdwAlerts::open_changed` for the window's `dialog_open` (§2.2).
    pub fn open_changed(&self) -> &Event<bool> {
        &self.open_changed
    }

    fn file_dialog(&self, title: Option<&str>) -> gtk::FileDialog {
        let dialog = gtk::FileDialog::builder().modal(true).build();
        if let Some(title) = title.filter(|t| !t.is_empty()) {
            dialog.set_title(title);
        }
        if let Some(folder) = self.folder.borrow().as_deref() {
            dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
        }
        dialog
    }

    /// Raises `open_changed` around `future`.
    fn bracketed<T: 'static>(
        &self,
        future: impl std::future::Future<Output = T> + 'static,
    ) -> impl std::future::Future<Output = T> + 'static {
        let open_changed = Rc::clone(&self.open_changed);
        async move {
            open_changed.emit(&true);
            let result = future.await;
            open_changed.emit(&false);
            result
        }
    }
}

impl Pickers for GtkPickers {
    /// One filter named [`OPEN_FILTER`]: the suffixes of `extensions` (case-insensitive in GTK)
    /// plus the Markdown and plain-text MIME types (§6.1). GTK hides the filter popup when there
    /// is only one.
    fn open_files(&self, extensions: Vec<String>) -> LocalBoxFuture<'static, Vec<String>> {
        #[cfg(feature = "selftest")]
        if let Some(answer) = self.queued() {
            return Box::pin(self.bracketed(async move { answer.into_iter().collect() }));
        }
        let dialog = self.file_dialog(None);
        if !extensions.is_empty() {
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&open_filter(&extensions));
            dialog.set_filters(Some(&filters));
        }
        let picked = dialog.open_multiple_future(Some(&self.window));
        Box::pin(self.bracketed(async move {
            let Ok(files) = picked.await else { return Vec::new() };
            (0..files.n_items())
                .filter_map(|n| files.item(n).and_downcast::<gio::File>())
                .filter_map(|file| local_path(&file))
                .collect()
        }))
    }

    /// `suggested_name` is a file name; the dialog starts in [`GtkPickers::set_document_folder`]'s
    /// folder. One filter per choice, in the order given (the Mac's); the one holding
    /// `default_extension` is preselected. A name typed without an extension gets
    /// `default_extension`, and when that completed name exists md asks before replacing it.
    fn save_file(
        &self,
        suggested_name: String,
        choices: Vec<FileTypeChoice>,
        default_extension: String,
    ) -> LocalBoxFuture<'static, Option<String>> {
        #[cfg(feature = "selftest")]
        if let Some(answer) = self.queued() {
            let alerts = Rc::clone(&self.alerts);
            return Box::pin(self.bracketed(async move { finish_save(alerts, answer?, default_extension).await }));
        }
        let dialog = self.file_dialog(None);
        dialog.set_initial_name(Some(&suggested_name));
        if !choices.is_empty() {
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            for choice in &choices {
                let filter = choice_filter(choice);
                if choice.extensions.iter().any(|e| e.eq_ignore_ascii_case(&default_extension)) {
                    dialog.set_default_filter(Some(&filter));
                }
                filters.append(&filter);
            }
            dialog.set_filters(Some(&filters));
        }
        let picked = dialog.save_future(Some(&self.window));
        let alerts = Rc::clone(&self.alerts);
        Box::pin(self.bracketed(async move {
            let path = local_path(&picked.await.ok()?)?;
            finish_save(alerts, path, default_extension).await
        }))
    }

    /// `title` (a book or bundle prompt) is the dialog's title; GTK's own when `None`.
    fn pick_folder(&self, title: Option<String>) -> LocalBoxFuture<'static, Option<String>> {
        #[cfg(feature = "selftest")]
        if let Some(answer) = self.queued() {
            return Box::pin(self.bracketed(async move { answer }));
        }
        let dialog = self.file_dialog(title.as_deref());
        let picked = dialog.select_folder_future(Some(&self.window));
        Box::pin(self.bracketed(async move { local_path(&picked.await.ok()?) }))
    }
}

/// What happens to the path GTK's save dialog returned (§6.1): kept when it has an extension,
/// else completed with `default_extension` — and when that completed name exists, md asks
/// (`strings::documents::file_exists_question`) before handing it out; `None` when the writer declines.
pub fn finish_save(
    alerts: Rc<dyn Alerts>,
    chosen: String,
    default_extension: String,
) -> LocalBoxFuture<'static, Option<String>> {
    Box::pin(async move {
        match completed_save_path(&chosen, &default_extension, |p| std::fs::symlink_metadata(p).is_ok()) {
            SaveTarget::AsChosen(path) | SaveTarget::Completed { path, exists: false } => Some(path),
            SaveTarget::Completed { path, exists: true } => {
                let question = file_exists_question(file_names::name_of(&path));
                alerts.confirm_replace(question).await.then_some(path)
            }
        }
    })
}

/// The Open dialog's one filter (§6.1).
fn open_filter(extensions: &[String]) -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(OPEN_FILTER));
    for extension in extensions {
        filter.add_suffix(suffix(extension));
    }
    filter.add_mime_types(&OPEN_MIME_TYPES);
    filter
}

/// A save choice as a filter: its label, its suffixes.
fn choice_filter(choice: &FileTypeChoice) -> gtk::FileFilter {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&choice.label));
    for extension in &choice.extensions {
        filter.add_suffix(suffix(extension));
    }
    filter
}

/// md-logic's extensions carry the dot; GTK's suffixes do not.
fn suffix(extension: &str) -> &str {
    extension.strip_prefix('.').unwrap_or(extension)
}

/// Where a save goes once md has looked at the name the dialog returned.
#[derive(Clone, Debug, PartialEq, Eq)]
enum SaveTarget {
    /// The name had an extension: GTK already confirmed any replacement.
    AsChosen(String),
    /// The name had none and got the default one; `exists` says whether that name is taken (md
    /// must ask, GTK did not).
    Completed { path: String, exists: bool },
}

/// §6.1's rule: append `default_extension` only when the chosen name has no extension (Foundation's
/// split, so `notes.` and `.hidden` count as having none), and report whether the completed name
/// collides.
fn completed_save_path(chosen: &str, default_extension: &str, exists: impl Fn(&str) -> bool) -> SaveTarget {
    if default_extension.is_empty() || !file_names::extension_of(chosen).is_empty() {
        return SaveTarget::AsChosen(chosen.to_string());
    }
    let extension = if default_extension.starts_with('.') {
        default_extension.to_string()
    } else {
        format!(".{default_extension}")
    };
    let path = format!("{chosen}{extension}");
    let exists = exists(&path);
    SaveTarget::Completed { path, exists }
}

/// A `gio::File`'s local path as md-logic's UTF-8 string. A file with no local path (a portal can
/// hand over only local ones, GTK's own chooser too) or a non-UTF-8 name is dropped.
fn local_path(file: &gio::File) -> Option<String> {
    file.path().and_then(|path| path.into_os_string().into_string().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn completed(chosen: &str, default_extension: &str, taken: &[&str]) -> SaveTarget {
        completed_save_path(chosen, default_extension, |p| taken.contains(&p))
    }

    #[test]
    fn a_name_with_an_extension_is_kept_as_chosen() {
        assert_eq!(completed("/d/notes.md", ".md", &[]), SaveTarget::AsChosen("/d/notes.md".into()));
        assert_eq!(completed("/d/notes.txt", ".md", &[]), SaveTarget::AsChosen("/d/notes.txt".into()));
        assert_eq!(completed("/d/archive.tar.gz", ".md", &[]), SaveTarget::AsChosen("/d/archive.tar.gz".into()));
        assert_eq!(completed("/d/Заметки.markdown", ".md", &[]), SaveTarget::AsChosen("/d/Заметки.markdown".into()));
    }

    #[test]
    fn a_bare_name_gets_the_default_extension() {
        assert_eq!(
            completed("/d/notes", ".md", &[]),
            SaveTarget::Completed { path: "/d/notes.md".into(), exists: false }
        );
        assert_eq!(
            completed("/d/diagram", ".puml", &[]),
            SaveTarget::Completed { path: "/d/diagram.puml".into(), exists: false }
        );
        // Foundation's split: neither of these has an extension.
        assert_eq!(
            completed("/d/notes.", ".md", &[]),
            SaveTarget::Completed { path: "/d/notes..md".into(), exists: false }
        );
        assert_eq!(
            completed("/d/.hidden", ".md", &[]),
            SaveTarget::Completed { path: "/d/.hidden.md".into(), exists: false }
        );
        // A dotless default is given its dot.
        assert_eq!(
            completed("/d/notes", "txt", &[]),
            SaveTarget::Completed { path: "/d/notes.txt".into(), exists: false }
        );
        // No default: nothing to append.
        assert_eq!(completed("/d/notes", "", &[]), SaveTarget::AsChosen("/d/notes".into()));
    }

    #[test]
    fn a_completed_name_that_exists_is_reported() {
        assert_eq!(
            completed("/d/notes", ".md", &["/d/notes.md"]),
            SaveTarget::Completed { path: "/d/notes.md".into(), exists: true }
        );
        // The name as typed existing is GTK's business, not ours.
        assert_eq!(completed("/d/notes.md", ".md", &["/d/notes.md"]), SaveTarget::AsChosen("/d/notes.md".into()));
    }

    #[test]
    fn finishing_a_save_asks_only_about_a_completed_name_that_exists() {
        use crate::services::file_system::tests::TempFolder;
        use md_logic::exec::LocalExecutor;
        use md_logic::testing::FakeAlerts;

        let temp = TempFolder::new("finish-save");
        std::fs::write(temp.path("Draft.md"), "taken").unwrap();
        let executor = LocalExecutor::new();
        let alerts = Rc::new(FakeAlerts::new());
        let finish = |chosen: &str| executor.block_on(finish_save(alerts.clone(), chosen.to_string(), ".md".into()));

        // Free names and names with an extension pass without a question.
        assert_eq!(finish(&temp.path("Fresh")), Some(temp.path("Fresh.md")));
        assert_eq!(finish(&temp.path("Draft.md")), Some(temp.path("Draft.md")));
        assert!(alerts.replace_confirmations().is_empty());
        // The completed name is taken: declined (the safe default), then accepted.
        assert_eq!(finish(&temp.path("Draft")), None);
        assert_eq!(alerts.replace_confirmations(), [file_exists_question("Draft.md")]);
        alerts.queue_replace_answer(true);
        assert_eq!(finish(&temp.path("Draft")), Some(temp.path("Draft.md")));
        assert_eq!(std::fs::read(temp.path("Draft.md")).unwrap(), b"taken", "md only answers the path");
    }

    #[test]
    fn suffixes_drop_the_dot() {
        assert_eq!(suffix(".md"), "md");
        assert_eq!(suffix("textpack"), "textpack");
    }

    #[test]
    fn the_replace_question_names_the_file() {
        assert_eq!(
            file_exists_question("notes.md"),
            "\u{201C}notes.md\u{201D} already exists. Do you want to replace it?"
        );
    }
}
