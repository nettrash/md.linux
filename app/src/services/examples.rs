//! `ExampleLibrary` — File ▸ Examples over the bundled `examples/` folder (shell-design §2.3):
//! the top level's `.md` files named and ordered by md-core's `example_library`, and the
//! `Example Book` folder Example Book… copies (`BookLibraryHost::unpack_example_book_into`).
//!
//! The nine files ship byte-identical with the other md apps in `/usr/share/md/examples` (the
//! source tree's `examples/` in a debug build, `Paths`). The listing is the folder's top level
//! only: `Example Book/` is not a row, it unpacks whole. Opening an example is the window manager's
//! job — an untitled, dirty window holding the text (§2.3); a missing or unreadable example does
//! nothing, as on the Mac.

use std::cell::OnceCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use md_core::document::example_library::{self, Example};

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
}

fn list(folder: &Path) -> Vec<Example> {
    let Ok(entries) = fs::read_dir(folder) else { return Vec::new() };
    let names = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|entry| entry.file_name().into_string().ok());
    example_library::from_listing(names)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
