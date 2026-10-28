//! Port of md.win `DestinationTests`: selection remapping after a rename plan (mdTests:
//! testDestinationFollowsARenamedArticle, testDestinationFollowsAnArticleInsideARenamedChapter,
//! testDestinationLeavesUntouchedURLsAlone).

mod book_support;

use book_support::combine;
use md_core::book::book_paths;
use md_core::book::destination;
use md_core::book::renumber_plan::RenamePair;

fn root() -> String {
    book_paths::standardize("/tmp/Book")
}

#[test]
fn destination_follows_a_renamed_article() {
    let root = root();
    let moved = destination::of(
        &combine(&[&root, "02-Draft.md"]),
        &root,
        &[RenamePair::new("02-Draft.md", "01-Draft.md"), RenamePair::new("01-Intro.md", "02-Intro.md")],
    );
    assert_eq!("01-Draft.md", book_paths::file_name(&moved));
    assert_eq!(combine(&[&root, "01-Draft.md"]), moved);
}

#[test]
fn destination_follows_an_article_inside_a_renamed_chapter() {
    let root = root();
    let article = combine(&[&root, "03-Middle", "01-Scene.md"]);
    let moved = destination::of(&article, &root, &[RenamePair::new("03-Middle", "02-Middle")]);
    assert_eq!(combine(&[&root, "02-Middle", "01-Scene.md"]), moved);
}

#[test]
fn destination_leaves_untouched_paths_alone() {
    let root = root();
    let plan = [RenamePair::new("01-a.md", "02-a.md")];
    // A sibling the plan doesn't mention…
    let bystander = combine(&[&root, "03-c.md"]);
    assert_eq!(book_paths::standardize(&bystander), destination::of(&bystander, &root, &plan));
    // …an article whose chapter is not in the plan…
    let nested = combine(&[&root, "02-One", "01-a.md"]);
    assert_eq!(nested, destination::of(&nested, &root, &plan));
    // …and anything outside the folder entirely, returned exactly as given.
    let outside = combine(&[&book_paths::standardize("/tmp/Elsewhere"), "01-a.md"]);
    assert_eq!(outside, destination::of(&outside, &root, &plan));
    // md.win asserts the very same object comes back; a Rust String compares by value, so a
    // non-standard spelling is what shows "exactly as given, not even standardized".
    let odd = format!("{root}/02-One/./01-a.md");
    assert_eq!(odd, destination::of(&odd, &root, &plan));
    let odd_outside = format!("{}//01-a.md", book_paths::standardize("/tmp/Elsewhere"));
    assert_eq!(odd_outside, destination::of(&odd_outside, &root, &plan));
    // A folder with a trailing separator names the same folder.
    assert_eq!(
        combine(&[&root, "02-a.md"]),
        destination::of(&combine(&[&root, "01-a.md"]), &format!("{root}/"), &plan)
    );
}
