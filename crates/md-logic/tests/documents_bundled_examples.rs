//! Port of md.win `tests/Md.App.Logic.Tests/BundledExamplesTests.cs`.
//!
//! The nine files that ship in this repository's `examples/`, through the model the app uses: the
//! File ▸ Examples rows are md-core's `example_library::from_listing` over the folder's top-level
//! listing, so this asserts exactly that pair — it pins the ASSET, which is what a dropped, renamed
//! or renumbered example would break. (md-core's own tests pin the rule over a fixture copy.)

mod common;

use common::document_fixtures::repo_root;
use md_core::document::example_library;

#[test]
fn the_nine_bundled_examples_produce_the_menus_rows_in_order() {
    // Top level only, as the app lists it: "Example Book/" is not a row — it unpacks whole through
    // Examples ▸ Example Book… (§2.3).
    let folder = format!("{}/examples", repo_root());
    let mut listing: Vec<String> = std::fs::read_dir(&folder)
        .expect("the examples folder")
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".md"))
        .collect();
    // The file system's order is not the menu's; from_listing is what sorts.
    listing.sort_by(|a, b| b.cmp(a));

    let examples = example_library::from_listing(&listing);

    assert_eq!(
        examples.iter().map(|e| e.file_name.as_str()).collect::<Vec<_>>(),
        [
            "01-Welcome.md",
            "02-Formatting.md",
            "03-Tables.md",
            "04-Code.md",
            "05-Images.md",
            "06-Math.md",
            "07-Diagrams.md",
            "08-Plots.md",
            "09-Writer Tools.md",
        ]
    );
    assert_eq!(
        examples.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Welcome", "Formatting", "Tables", "Code", "Images", "Math", "Diagrams", "Plots", "Writer Tools"]
    );
}
