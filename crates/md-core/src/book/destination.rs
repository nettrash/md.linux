//! Where a selected path ends up after a plan renames siblings inside a folder (md.win
//! `Book/Destination.cs`, Swift `BookLibrary.destination`).

use super::book_paths;
use super::renumber_plan::RenamePair;

/// The item itself, or the chapter an article lives in, after `plan` renames siblings inside
/// `folder` — the pure half of the workspace's selection remapping. Only the *first* path
/// component under the folder is remapped; paths outside the folder and names the plan does not
/// touch come back exactly as given (not even standardized), as Swift returns the original URL
/// object. Names match byte for byte, as Linux file systems compare them.
pub fn of(path: &str, folder: &str, plan: &[RenamePair]) -> String {
    let folder_path = book_paths::standardize(folder);
    let full_path = book_paths::standardize(path);
    if !book_paths::is_inside(&full_path, &folder_path) {
        return path.to_string();
    }
    let start = if folder_path.ends_with(book_paths::SEPARATOR) { folder_path.len() } else { folder_path.len() + 1 };
    // Swift split(separator: "/") drops empty parts; standardize has already collapsed doubled
    // separators, so filtering them is belt and braces.
    let mut components: Vec<&str> =
        full_path[start..].split(book_paths::SEPARATOR).filter(|part| !part.is_empty()).collect();
    if components.is_empty() {
        return path.to_string();
    }
    let Some(renamed) = plan.iter().find(|pair| pair.from == components[0]) else {
        return path.to_string();
    };
    components[0] = &renamed.to;
    components.iter().fold(folder_path.clone(), |result, component| book_paths::combine(&result, component))
}
