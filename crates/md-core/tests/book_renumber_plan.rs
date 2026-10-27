//! Port of md.win `RenumberPlanTests`: Move Up / Move Down planning (mdTests: the five
//! testRenumberPlan* tests) plus the Kotlin planMove / renumberedBookName vectors, and the two-phase
//! executor against a fake mover and against the disk.

mod book_support;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use book_support::{TempDir, combine, names_in, temp_path};
use md_core::book::book_paths;
use md_core::book::renumber_plan::{self, BookFileMover, RenamePair};

fn split(plan: &[RenamePair]) -> (Vec<&str>, Vec<&str>) {
    (plan.iter().map(|p| p.from.as_str()).collect(), plan.iter().map(|p| p.to.as_str()).collect())
}

#[test]
fn renumber_plan_materializes_a_move_down() {
    // a moves below b; c is already right, so only two renames.
    let plan = renumber_plan::plan(&["01-a.md", "02-b.md", "03-c.md"], 0, 1);
    let (from, to) = split(&plan);
    assert_eq!(vec!["02-b.md", "01-a.md"], from);
    assert_eq!(vec!["01-b.md", "02-a.md"], to);
}

#[test]
fn renumber_plan_prefixes_unprefixed_siblings() {
    // Unprefixed and loosely-prefixed names all end up with the canonical zero-padded prefix;
    // display names survive.
    let plan = renumber_plan::plan(&["1. intro.md", "notes.md", "02-end.md"], 2, 1);
    let (from, to) = split(&plan);
    assert_eq!(vec!["1. intro.md", "notes.md"], from);
    assert_eq!(vec!["01-intro.md", "03-notes.md"], to);
}

#[test]
fn renumber_plan_handles_chapter_folders() {
    // Chapters have no extension, and a dot in a folder name is part of the name.
    let plan = renumber_plan::plan(&["Drafts", "01-Final"], 1, 0);
    let (from, to) = split(&plan);
    assert_eq!(vec!["Drafts"], from);
    assert_eq!(vec!["02-Drafts"], to);
}

#[test]
fn renumber_plan_swaps_identical_stems() {
    // Trading places between same-stem names yields renames that pass through each other — the
    // executor stages via temporary names so this never collides.
    let plan = renumber_plan::plan(&["01-a.md", "02-a.md"], 0, 1);
    let (from, to) = split(&plan);
    assert_eq!(vec!["02-a.md", "01-a.md"], from);
    assert_eq!(vec!["01-a.md", "02-a.md"], to);
}

#[test]
fn renumber_plan_rejects_impossible_moves() {
    let none: [&str; 0] = [];
    assert!(renumber_plan::plan(&["01-a.md"], 0, -1).is_empty());
    assert!(renumber_plan::plan(&["01-a.md"], 0, 1).is_empty());
    assert!(renumber_plan::plan(&["01-a.md"], 0, 0).is_empty());
    assert!(renumber_plan::plan(&none, 0, 0).is_empty());
    // Kotlin's variants of the same rule.
    assert!(renumber_plan::plan(&["01-a.md", "02-b.md"], 1, 1).is_empty());
    assert!(renumber_plan::plan(&["01-a.md", "02-b.md"], 0, -1).is_empty());
    assert!(renumber_plan::plan(&["01-a.md", "02-b.md"], 1, 2).is_empty());
    assert!(renumber_plan::plan(&none, 0, 1).is_empty());
}

#[test]
fn kotlin_plan_move_vectors_agree() {
    assert_eq!(
        vec![RenamePair::new("03-c.md", "02-c.md"), RenamePair::new("02-b.md", "03-b.md")],
        renumber_plan::plan(&["01-a.md", "02-b.md", "03-c.md"], 2, 1)
    );
    assert_eq!(
        vec![RenamePair::new("beta.md", "01-beta.md"), RenamePair::new("alpha.md", "02-alpha.md")],
        renumber_plan::plan(&["alpha.md", "beta.md"], 1, 0)
    );
    assert_eq!(
        vec![RenamePair::new("02-b.md", "01-b.md"), RenamePair::new("1. a.md", "02-a.md")],
        renumber_plan::plan(&["1. a.md", "02-b.md"], 0, 1)
    );
    assert_eq!(
        vec![RenamePair::new("v1.2", "01-v1.2"), RenamePair::new("01-Basics", "02-Basics")],
        renumber_plan::plan(&["01-Basics", "v1.2"], 1, 0)
    );
}

#[test]
fn renumbered_name_replaces_or_adds_the_prefix() {
    assert_eq!("03-intro.md", renumber_plan::renumbered_name("intro.md", 3));
    assert_eq!("02-deploy.md", renumber_plan::renumbered_name("10-deploy.md", 2));
    assert_eq!("01-setup", renumber_plan::renumbered_name("2. setup", 1));
    // Three digits still work; the padding is a minimum, not a cap.
    assert_eq!("100-late.md", renumber_plan::renumbered_name("late.md", 100));
    assert_eq!("01-v1.2", renumber_plan::renumbered_name("v1.2", 1));
}

#[test]
fn staging_uses_hidden_temporary_names_inside_the_folder() {
    let folder = combine(&[&temp_path(), "Book"]);
    let plan = renumber_plan::plan(&["01-a.md", "02-a.md"], 0, 1);
    let staged = renumber_plan::stage(&plan, &folder);
    assert_eq!(2, staged.len());
    assert_eq!(combine(&[&folder, "02-a.md"]), staged[0].original);
    assert_eq!(combine(&[&folder, "01-a.md"]), staged[0].target);
    for s in &staged {
        assert_eq!(Some(folder.clone()), book_paths::directory_name(&s.temp));
        assert!(book_paths::file_name(&s.temp).starts_with(".md-reorder-"));
    }
    assert_ne!(staged[0].temp, staged[1].temp);
    let mut counter = 0;
    let deterministic = renumber_plan::stage_with(&plan, &folder, &mut || {
        counter += 1;
        format!(".md-reorder-{counter}")
    });
    assert_eq!(combine(&[&folder, ".md-reorder-1"]), deterministic[0].temp);
}

/// md.win spells the temp name as an upper-case GUID; so does this port, from std alone.
#[test]
fn temp_names_are_upper_case_uuids_and_never_repeat() {
    let names: Vec<String> = (0..1000).map(|_| renumber_plan::temp_name()).collect();
    for name in &names {
        let guid = name.strip_prefix(".md-reorder-").expect("the staging prefix");
        let groups: Vec<&str> = guid.split('-').collect();
        assert_eq!(vec![8, 4, 4, 4, 12], groups.iter().map(|g| g.len()).collect::<Vec<_>>());
        assert!(guid.chars().all(|c| c == '-' || c.is_ascii_digit() || c.is_ascii_uppercase()));
    }
    let mut distinct = names.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(names.len(), distinct.len());
}

/// A folder as a map name → content; a move fails the way a file system does.
#[derive(Default)]
struct MemoryMover {
    files: RefCell<HashMap<String, String>>,
    fail_when_moving_to: Option<String>,
    log: RefCell<Vec<(String, String)>>,
}

impl BookFileMover for MemoryMover {
    fn move_item(&self, from: &str, to: &str) -> io::Result<()> {
        self.log.borrow_mut().push((from.to_string(), to.to_string()));
        if self.fail_when_moving_to.as_deref() == Some(to) {
            return Err(io::Error::other(format!("simulated: {}", book_paths::file_name(to))));
        }
        let mut files = self.files.borrow_mut();
        let Some(content) = files.remove(from) else {
            return Err(io::Error::new(io::ErrorKind::NotFound, from.to_string()));
        };
        if files.contains_key(to) {
            files.insert(from.to_string(), content);
            return Err(io::Error::other(format!("exists: {}", book_paths::file_name(to))));
        }
        files.insert(to.to_string(), content);
        Ok(())
    }
}

#[test]
fn apply_swaps_through_temporary_names_without_colliding() {
    let folder = combine(&[&temp_path(), "Book"]);
    let mover = MemoryMover::default();
    mover.files.borrow_mut().insert(combine(&[&folder, "01-a.md"]), "first".into());
    mover.files.borrow_mut().insert(combine(&[&folder, "02-a.md"]), "second".into());
    let plan = renumber_plan::plan(&["01-a.md", "02-a.md"], 0, 1);

    assert_eq!(Ok(()), renumber_plan::apply_with(&plan, &folder, &mover));
    let files = mover.files.borrow();
    assert_eq!(2, files.len());
    assert_eq!("second", files[&combine(&[&folder, "01-a.md"])]);
    assert_eq!("first", files[&combine(&[&folder, "02-a.md"])]);
    // Two phases: every source parked first, then every temp delivered.
    let log = mover.log.borrow();
    assert_eq!(4, log.len());
    assert!(log[..2].iter().all(|(_, to)| book_paths::file_name(to).starts_with(".md-reorder-")));
    assert!(log[2..].iter().all(|(from, _)| book_paths::file_name(from).starts_with(".md-reorder-")));
}

#[test]
fn apply_rolls_back_undelivered_items_when_a_move_fails() {
    let folder = combine(&[&temp_path(), "Book"]);
    // Plan: 02-b.md -> 01-b.md (delivered first), then 01-a.md -> 02-a.md (refused).
    let mover = MemoryMover { fail_when_moving_to: Some(combine(&[&folder, "02-a.md"])), ..MemoryMover::default() };
    mover.files.borrow_mut().insert(combine(&[&folder, "01-a.md"]), "a".into());
    mover.files.borrow_mut().insert(combine(&[&folder, "02-b.md"]), "b".into());
    mover.files.borrow_mut().insert(combine(&[&folder, "03-c.md"]), "c".into());
    let plan = renumber_plan::plan(&["01-a.md", "02-b.md", "03-c.md"], 0, 1);

    let failure = renumber_plan::apply_with(&plan, &folder, &mover).unwrap_err();
    assert!(failure.contains("02-a.md"), "{failure}");
    // Swift's rollback is "every temp back to its original", best effort: the item that failed and
    // the ones not yet delivered return to their old names, while "02-b.md" — already delivered as
    // "01-b.md" before the failure — stays where phase two put it. Nothing is ever left hidden under
    // a temporary name.
    let files = mover.files.borrow();
    let mut names: Vec<&str> = files.keys().map(|k| book_paths::file_name(k)).collect();
    names.sort();
    assert_eq!(vec!["01-a.md", "01-b.md", "03-c.md"], names);
    assert_eq!("a", files[&combine(&[&folder, "01-a.md"])]);
    assert_eq!("b", files[&combine(&[&folder, "01-b.md"])]);
    assert!(!files.keys().any(|k| book_paths::file_name(k).starts_with(".md-reorder-")));
}

#[test]
fn apply_of_an_empty_plan_touches_nothing() {
    let mover = MemoryMover::default();
    assert_eq!(Ok(()), renumber_plan::apply_with(&[], &temp_path(), &mover));
    assert!(mover.log.borrow().is_empty());
}

#[test]
fn apply_on_disk_swaps_files_and_renames_folders() {
    let root = TempDir::new("md-reorder-");
    fs::write(root.join(&["01-a.md"]), "first").unwrap();
    fs::write(root.join(&["02-a.md"]), "second").unwrap();
    fs::create_dir(root.join(&["Drafts"])).unwrap();
    fs::create_dir(root.join(&["01-Final"])).unwrap();

    assert_eq!(Ok(()), renumber_plan::apply(&renumber_plan::plan(&["01-a.md", "02-a.md"], 0, 1), &root.path));
    assert_eq!("second", fs::read_to_string(root.join(&["01-a.md"])).unwrap());
    assert_eq!("first", fs::read_to_string(root.join(&["02-a.md"])).unwrap());

    assert_eq!(Ok(()), renumber_plan::apply(&renumber_plan::plan(&["Drafts", "01-Final"], 1, 0), &root.path));
    assert!(Path::new(&root.join(&["02-Drafts"])).is_dir());
    assert!(Path::new(&root.join(&["01-Final"])).is_dir());
    assert!(!names_in(&root.path).iter().any(|name| name.starts_with(".md-reorder-")));
}

/// LINUX DECISION, pinned: `rename(2)` would silently replace an existing file, where Foundation's
/// `moveItem` and .NET's `File.Move` refuse. The local mover refuses too, so a plan whose target is
/// held by something outside the sibling list (here a stray file the plan does not know about)
/// fails and rolls back instead of eating it.
#[test]
fn the_local_mover_never_replaces_an_existing_file() {
    let root = TempDir::new("md-reorder-");
    fs::write(root.join(&["a.md"]), "mine").unwrap();
    fs::write(root.join(&["01-a.md"]), "a stranger").unwrap();

    let failure = renumber_plan::apply(&[RenamePair::new("a.md", "01-a.md")], &root.path).unwrap_err();
    assert!(failure.contains("01-a.md"), "{failure}");
    assert_eq!("mine", fs::read_to_string(root.join(&["a.md"])).unwrap());
    assert_eq!("a stranger", fs::read_to_string(root.join(&["01-a.md"])).unwrap());
    assert_eq!(vec!["01-a.md", "a.md"], names_in(&root.path));

    assert!(renumber_plan::LocalBookFileMover.move_item(&root.join(&["a.md"]), &root.join(&["01-a.md"])).is_err());
    assert_eq!("mine", fs::read_to_string(root.join(&["a.md"])).unwrap());
}
