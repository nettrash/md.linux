//! Port of md.win `ExampleLibraryTests`: the Examples menu model (shell.md §4) — the nine bundled
//! names in order, the prefix-stripping rule, the two behaviours the Swift pins that Android does
//! not ("01-" stays; digits are ASCII only), and the natural order — every vector below was
//! produced by Foundation's localizedStandardCompare on macOS (2026-09-06), which is what the Mac
//! sorts the menu with.

mod common;

use std::cmp::Ordering::{self, Equal, Greater, Less};

use md_core::document::example_library as examples;

const MENU_ORDER: [&str; 9] = [
    "01-Welcome.md",
    "02-Formatting.md",
    "03-Tables.md",
    "04-Code.md",
    "05-Images.md",
    "06-Math.md",
    "07-Diagrams.md",
    "08-Plots.md",
    "09-Writer Tools.md",
];

#[test]
fn bundled_examples_list_in_menu_order_with_prefixes_stripped() {
    // The fixture folder is the shipped Examples/ top level; the listing arrives in whatever order the
    // file system gives, so shuffle before asking.
    let mut listing: Vec<String> = std::fs::read_dir(common::fixture_path("examples"))
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".md"))
        .collect();
    listing.sort_by(|a, b| b.cmp(a));
    assert_eq!(9, listing.len());

    let examples = examples::from_listing(&listing);
    assert_eq!(MENU_ORDER.to_vec(), examples.iter().map(|e| e.file_name.as_str()).collect::<Vec<_>>());
    assert_eq!(
        vec!["Welcome", "Formatting", "Tables", "Code", "Images", "Math", "Diagrams", "Plots", "Writer Tools"],
        examples.iter().map(|e| e.name.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn display_name_strips_the_ordering_prefix_only_when_something_remains() {
    assert_eq!("Welcome", examples::display_name("01-Welcome.md"));
    assert_eq!("Writer Tools", examples::display_name("09-Writer Tools.md"));
    assert_eq!("Bond", examples::display_name("007-Bond.md"));
    assert_eq!("2-3", examples::display_name("1-2-3.md"));
    // No prefix, no change.
    assert_eq!("Welcome", examples::display_name("Welcome.md"));
    assert_eq!("01Welcome", examples::display_name("01Welcome.md"));
    assert_eq!("-Welcome", examples::display_name("-Welcome.md"));
    // The Swift guard Android lacks: "01-" stays "01-" rather than becoming "".
    assert_eq!("01-", examples::display_name("01-.md"));
    // Digits are ASCII only (isASCII && isNumber) — full-width and Arabic-Indic digits are not a prefix.
    assert_eq!("０１-Fullwidth", examples::display_name("０１-Fullwidth.md"));
    assert_eq!("١-Arabic", examples::display_name("١-Arabic.md"));
    // A dash fused with a combining mark, a ZWJ or a variation selector is not the Character "-" in
    // Swift, so the prefix stays.
    assert_eq!("01-\u{0301}Acute", examples::display_name("01-\u{0301}Acute.md"));
    assert_eq!("01-\u{200D}Joined", examples::display_name("01-\u{200D}Joined.md"));
    assert_eq!("01-\u{FE0F}Selected", examples::display_name("01-\u{FE0F}Selected.md"));
    // A digit fused with a mark is not an ASCII digit Character either.
    assert_eq!("0\u{0301}-Acute", examples::display_name("0\u{0301}-Acute.md"));
    // Only a real extension is dropped: a bare ".md" has none in Foundation.
    assert_eq!(".md", examples::display_name(".md"));
    assert_eq!("Plain", examples::display_name("Plain"));
}

#[test]
fn listing_keeps_only_markdown_files_and_sorts_naturally() {
    let examples = examples::from_listing([
        "10-Ten.md",
        "2-Two.md",
        "notes.txt",
        "Example Book",
        "README.MD",
        ".md",
        "b.md",
        "A.md",
        "1-One.md",
    ]);
    assert_eq!(
        vec!["1-One.md", "2-Two.md", "10-Ten.md", "A.md", "b.md"],
        examples.iter().map(|e| e.file_name.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(vec!["One", "Two", "Ten", "A", "b"], examples.iter().map(|e| e.name.as_str()).collect::<Vec<_>>());

    assert!(examples::is_example_file("x.md"));
    assert!(!examples::is_example_file(".md"));
    assert!(!examples::is_example_file("x.MD"));
    assert!(!examples::is_example_file("x.markdown"));
}

// MARK: The natural order, against Foundation

/// Each row is `a`, the order `localizedStandardCompare` returned, `b`. Numeric runs by value;
/// letters case-blind; punctuation before digits before letters; a shorter prefix first; then one
/// left-to-right walk over leading zeros (fewer first) and case (lower first), whichever comes
/// first.
const FOUNDATION_PAIRS: &[(&str, Ordering, &str)] = &[
    ("2-Two.md", Less, "10-Ten.md"),
    ("02-b.md", Greater, "2-a.md"),
    ("a.md", Less, "B.md"),
    ("Tables.md", Greater, "Tables (copy).md"),
    ("x.md", Greater, "x (2).md"),
    ("1-x.md", Less, "01-x.md"),
    ("1.md", Less, "01.md"),
    ("00", Greater, "0"),
    ("A.md", Greater, "a.md"),
    ("b.md", Less, "B.md"),
    ("a.md", Less, "a.MD"),
    ("01-Welcome.md", Greater, "01-welcome.md"),
    ("a.md", Greater, "a-b.md"),
    ("a b.md", Less, "a.md"),
    ("a_b.md", Less, "a.md"),
    ("a-1.md", Less, "a.1.md"),
    ("a(1).md", Greater, "a-1.md"),
    ("a1.md", Greater, "a.md"),
    ("ab.md", Greater, "A.md"),
    ("a", Less, "a.md"),
    ("A.md", Less, "b.md"),
    ("Z.md", Greater, "a.md"),
    ("z.md", Greater, "A.md"),
    ("09-Writer Tools.md", Less, "10-X.md"),
    ("Writer Tools.md", Less, "Writer-Tools.md"),
    ("x 1.md", Less, "x1.md"),
    ("x2.md", Less, "x10.md"),
    ("x02.md", Greater, "x2.md"),
    ("a9.md", Less, "a10.md"),
    ("a10b2.md", Less, "a10b10.md"),
    ("01-a.md", Greater, "1-A.md"),
    ("A1.md", Greater, "a01.md"),
    ("x1y.md", Less, "x01Y.md"),
    ("aB.md", Less, "Ab.md"),
    ("1-01.md", Less, "01-1.md"),
    ("01-x.md", Less, "001-x.md"),
    ("1-2.md", Less, "1-02.md"),
    ("01-2.md", Greater, "1-02.md"),
    ("1-b.md", Greater, "01-a.md"),
    ("9999999999999999999999.md", Less, "10000000000000000000000.md"),
    ("1234567890123456789012345678901234567890.md", Greater, "999.md"),
    ("1-x.md", Equal, "1-x.md"),
    ("a.md", Equal, "a.md"),
];

#[test]
fn compare_natural_agrees_with_foundation() {
    assert_eq!(43, FOUNDATION_PAIRS.len());
    for &(a, sign, b) in FOUNDATION_PAIRS {
        assert_eq!(sign, examples::compare_natural(a, b), "{a:?} vs {b:?}");
        assert_eq!(sign.reverse(), examples::compare_natural(b, a), "{b:?} vs {a:?}");
    }
}

#[test]
fn compare_natural_orders_ascii_punctuation_like_foundation() {
    // TAB, space, then the 33 printable non-alphanumerics in root-collation order — not ASCII order —
    // then any digit, then any letter. Exactly Foundation's sequence, on its own and between letters.
    let order = [
        "\t", " ", "_", "-", ",", ";", ":", "!", "?", ".", "'", "\"", "(", ")", "[", "]", "{", "}", "@", "*", "/",
        "\\", "&", "#", "%", "`", "^", "+", "<", "=", ">", "|", "~", "$",
    ];
    assert_eq!(34, order.len());
    let mut distinct = order.to_vec();
    distinct.sort();
    distinct.dedup();
    assert_eq!(34, distinct.len());
    for i in 1..order.len() {
        assert_eq!(Less, examples::compare_natural(order[i - 1], order[i]), "{:?} < {:?}", order[i - 1], order[i]);
        assert_eq!(Less, examples::compare_natural(&format!("a{}b", order[i - 1]), &format!("a{}b", order[i])));
    }
    for punctuation in order {
        assert_eq!(Less, examples::compare_natural(punctuation, "0"));
        assert_eq!(Less, examples::compare_natural(punctuation, "a"));
    }
    assert_eq!(Less, examples::compare_natural("1", "a"));
    assert_eq!(Less, examples::compare_natural("9", "A"));

    let mut shuffled = order.to_vec();
    shuffled.sort_by(|a, b| b.cmp(a));
    shuffled.sort_by(|a, b| examples::compare_natural(a, b));
    assert_eq!(order.to_vec(), shuffled);
}

#[test]
fn compare_natural_sorts_whole_lists_like_foundation() {
    fn sorted(names: &[&'static str]) -> Vec<&'static str> {
        let mut list = names.to_vec();
        list.sort_by(|a, b| b.cmp(a));
        list.sort_by(|a, b| examples::compare_natural(a, b));
        list
    }

    assert_eq!(
        vec![
            "1-One.md",
            "1-x.md",
            "01-x.md",
            "2-Two.md",
            "10-Ten.md",
            "a.md",
            "A.md",
            "b.md",
            "Tables (copy).md",
            "Tables.md"
        ],
        sorted(&[
            "10-Ten.md",
            "2-Two.md",
            "b.md",
            "A.md",
            "1-One.md",
            "Tables.md",
            "Tables (copy).md",
            "a.md",
            "01-x.md",
            "1-x.md"
        ])
    );
    assert_eq!(
        vec!["1-1.md", "1-01.md", "01-1.md", "01-01.md", "001-1.md"],
        sorted(&["1-01.md", "01-1.md", "1-1.md", "01-01.md", "001-1.md"])
    );
    assert_eq!(vec!["a.md", "a.MD", "A.md", "A.MD"], sorted(&["a.md", "A.md", "a.MD", "A.MD"]));
    assert_eq!(vec!["ab.md", "aB.md", "Ab.md", "AB.md"], sorted(&["aB.md", "Ab.md", "ab.md", "AB.md"]));
    assert_eq!(vec!["1-a.md", "1-A.md", "01-a.md", "01-A.md"], sorted(&["01-a.md", "1-A.md", "1-a.md", "01-A.md"]));
}
