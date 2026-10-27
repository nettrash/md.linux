//! `ExampleLibrary` — File ▸ Examples over the bundled `examples/` folder (shell-design §2.3):
//! the top level's `.md` files named and ordered by md-core's `example_library`, and the
//! `Example Book` folder Example Book… unpacks.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P2

use std::path::PathBuf;
use std::rc::Rc;

use md_core::document::example_library::Example;

use crate::services::paths::Paths;

/// The bundled examples.
pub struct ExampleLibrary {
    folder: PathBuf,
}

impl ExampleLibrary {
    pub fn new(paths: &Paths) -> Rc<ExampleLibrary> {
        Rc::new(ExampleLibrary { folder: paths.examples_dir() })
    }

    /// In menu order.
    pub fn examples(&self) -> Vec<Example> {
        Vec::new()
    }

    /// An example's text (UTF-8); `None` when it cannot be read.
    pub fn read(&self, file_name: &str) -> Option<String> {
        None
    }

    /// `examples/Example Book`.
    pub fn book_folder(&self) -> PathBuf {
        self.folder.join("Example Book")
    }
}
