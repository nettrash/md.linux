//! `ExampleLibrary` — File ▸ Examples over the bundled `examples/` folder (shell-design §2.3):
//! the top level's `.md` files named and ordered by md-core's `example_library`, and the
//! `Example Book` folder Example Book… unpacks.
//!
//! The nine files ship byte-identical with the other md apps in `/usr/share/md/examples` (the
//! source tree's `examples/` in a debug build, `Paths`). The listing is the folder's top level
//! only: `Example Book/` is not a row, it unpacks whole. Opening an example is the window manager's
//! job — an untitled, dirty window holding the text (§2.3); a missing or unreadable example does
//! nothing, as on the Mac.
#![allow(dead_code)] // S0 stub: removed at the integration gate (P1) — this file's callers are P1's and P5's stubs until then

use std::cell::OnceCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use md_core::document::example_library::{self, Example};
use md_logic::books::book_library_model::BookLibraryModel;
use md_logic::documents::file_names;
use md_logic::strings;

use crate::services::paths::Paths;

/// The example book's folder name inside `examples/`, and the name it unpacks under.
const BOOK_FOLDER_NAME: &str = "Example Book";

/// The bundled examples.
pub struct ExampleLibrary {
    folder: PathBuf,
    /// Listed once: the install does not change while md runs.
    listing: OnceCell<Vec<Example>>,
}

impl ExampleLibrary {
    pub fn new(paths: &Paths) -> Rc<ExampleLibrary> {
        Rc::new(ExampleLibrary::at(paths.examples_dir()))
    }

    fn at(folder: PathBuf) -> ExampleLibrary {
        ExampleLibrary { folder, listing: OnceCell::new() }
    }

    /// In menu order. Empty when the folder is missing — a broken install shows no Examples
    /// rather than failing at start-up.
    pub fn examples(&self) -> Vec<Example> {
        self.listing.get_or_init(|| list(&self.folder)).clone()
    }

    /// An example's text (UTF-8); `None` when it cannot be read — or is not one of the listed
    /// examples: a name is never a path into the install.
    pub fn read(&self, file_name: &str) -> Option<String> {
        if !self.examples().iter().any(|e| e.file_name == file_name) {
            return None;
        }
        String::from_utf8(fs::read(self.folder.join(file_name)).ok()?).ok()
    }

    /// `examples/Example Book`.
    pub fn book_folder(&self) -> PathBuf {
        self.folder.join(BOOK_FOLDER_NAME)
    }

    /// Example Book…'s copy (md.win's `UnpackExampleBookAsync` after its picker): the bundled book
    /// copied into `parent` as `Example Book`, or `Example Book 2`, `… 3` — a folder dialog has no
    /// Replace step, so an existing book is never written over. A copy that fails half way is
    /// removed. Returns the new book folder; the error is the message for an alert titled
    /// `strings::books::COULD_NOT_UNPACK_EXAMPLE_BOOK`.
    pub fn unpack_book(&self, parent: &str) -> Result<String, String> {
        let source = self.book_folder();
        let name = BookLibraryModel::deduped_name(
            BOOK_FOLDER_NAME,
            |candidate| fs::symlink_metadata(Path::new(parent).join(candidate)).is_ok(),
            BookLibraryModel::DEFAULT_MAX_ATTEMPTS,
        )
        .ok_or_else(|| strings::exports::folder_exists(BOOK_FOLDER_NAME))?;
        let destination = file_names::combine(parent, &name);
        match copy_folder(&source, Path::new(&destination)) {
            Ok(()) => Ok(destination),
            Err(err) => {
                // The alert names the copy's failure, not the clean-up's.
                if fs::symlink_metadata(&destination).is_ok_and(|m| m.is_dir()) {
                    let _ = fs::remove_dir_all(&destination);
                }
                Err(err.to_string())
            }
        }
    }
}

fn list(folder: &Path) -> Vec<Example> {
    let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
    let names = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok());
    example_library::from_listing(names)
}

/// Copies `source` into a new folder `destination` (which must not exist): regular files and
/// folders, recursively; anything else (a link, a socket) is not part of the book and is skipped.
/// Never overwrites. Errors carry the path they are about.
fn copy_folder(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination).map_err(|e| with_path(e, destination))?;
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(source)
        .map_err(|e| with_path(e, source))?
        .collect::<io::Result<_>>()
        .map_err(|e| with_path(e, source))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|e| with_path(e, &entry.path()))?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            let bytes = fs::read(entry.path()).map_err(|e| with_path(e, &entry.path()))?;
            let mut file =
                fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| with_path(e, &target))?;
            io::Write::write_all(&mut file, &bytes).map_err(|e| with_path(e, &target))?;
        }
    }
    Ok(())
}

fn with_path(error: io::Error, path: &Path) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::file_system::tests::TempFolder;

    /// The source tree's `examples/`, what a debug build without an install reads.
    fn bundled() -> ExampleLibrary {
        ExampleLibrary::at(Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples"))
    }

    #[test]
    fn the_nine_examples_in_menu_order() {
        let names: Vec<String> = bundled().examples().into_iter().map(|e| e.name).collect();
        assert_eq!(
            names,
            ["Welcome", "Formatting", "Tables", "Code", "Images", "Math", "Diagrams", "Plots", "Writer Tools"]
        );
    }

    #[test]
    fn an_example_reads_and_a_path_does_not() {
        let library = bundled();
        let welcome = library.read("01-Welcome.md").expect("the welcome example");
        assert!(welcome.starts_with('#'), "{welcome}");
        assert_eq!(library.read("../README.md"), None);
        assert_eq!(library.read("Example Book"), None);
        assert_eq!(library.read("missing.md"), None);
    }

    #[test]
    fn a_missing_install_lists_nothing() {
        let library = ExampleLibrary::at(PathBuf::from("/nonexistent/md/examples"));
        assert!(library.examples().is_empty());
        assert_eq!(library.read("01-Welcome.md"), None);
    }

    #[test]
    fn the_book_unpacks_whole_and_never_over_another() {
        let temp = TempFolder::new("unpack");
        let library = bundled();
        let parent = temp.0.to_string_lossy().into_owned();
        let first = library.unpack_book(&parent).expect("first unpack");
        assert_eq!(first, temp.path("Example Book"));
        let second = library.unpack_book(&parent).expect("second unpack");
        assert_eq!(second, temp.path("Example Book 2"));
        // A file squatting on the next name is skipped too.
        std::fs::write(temp.path("Example Book 3"), "").unwrap();
        assert_eq!(library.unpack_book(&parent).unwrap(), temp.path("Example Book 4"));

        let listing = |root: &str| -> Vec<String> {
            let mut found = Vec::new();
            let mut stack = vec![PathBuf::from(root)];
            while let Some(dir) = stack.pop() {
                for entry in std::fs::read_dir(&dir).unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_dir() {
                        stack.push(entry.path());
                    } else {
                        let relative = entry.path().strip_prefix(root).unwrap().to_string_lossy().into_owned();
                        found.push(relative);
                    }
                }
            }
            found.sort();
            found
        };
        let source = library.book_folder().to_string_lossy().into_owned();
        assert!(!listing(&source).is_empty());
        assert_eq!(listing(&first), listing(&source));
        for file in listing(&source) {
            assert_eq!(
                std::fs::read(Path::new(&first).join(&file)).unwrap(),
                std::fs::read(Path::new(&source).join(&file)).unwrap(),
                "{file}"
            );
        }
    }

    #[test]
    fn a_failed_unpack_leaves_nothing_behind() {
        let temp = TempFolder::new("unpack-fail");
        let library = ExampleLibrary::at(temp.0.join("no-examples-here"));
        let parent = temp.0.to_string_lossy().into_owned();
        let message = library.unpack_book(&parent).unwrap_err();
        assert!(message.contains("Example Book"), "{message}");
        assert!(!Path::new(&temp.path("Example Book")).exists());
        // A parent that is not a folder.
        let file = temp.path("plain-file");
        std::fs::write(&file, "").unwrap();
        assert!(bundled().unpack_book(&file).is_err());
    }
}
