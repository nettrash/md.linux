//! Port of md.win `BookOrderTests`: the reading-order comparator (mdTests:
//! testOrderedPutsNumberedNamesFirstInNumericOrder, testOrderedFallsBackToFinderAlphabetical,
//! testOrderedTreatsOverflowingDigitRunsAsUnnumbered) and the Kotlin BookOrderTest vectors for
//! leadingNumber, naturalCompare and bookNameOrder, which this port reproduces instead of
//! localizedStandardCompare.

use std::cmp::Ordering;

use md_core::book::book_order::{compare, leading_number, natural_compare, ordered};

#[test]
fn ordered_puts_numbered_names_first_in_numeric_order() {
    // Numeric, not lexicographic: 2 before 10; numbered before not.
    assert!(ordered("2. setup", "10-ending"));
    assert!(!ordered("10-ending", "2. setup"));
    assert!(ordered("01-intro", "appendix"));
    assert!(!ordered("appendix", "01-intro"));
}

#[test]
fn ordered_falls_back_to_finder_alphabetical() {
    assert!(ordered("apple", "Banana"));
    assert!(!ordered("Banana", "apple"));
    // Equal leading numbers tie-break alphabetically too.
    assert!(ordered("01-a", "01-b"));
}

#[test]
fn ordered_treats_overflowing_digit_runs_as_unnumbered() {
    // A digit run too long for i64 must not panic — it sorts as unnumbered.
    assert!(ordered("1-x", "99999999999999999999-y"));
    assert!(!ordered("99999999999999999999-y", "1-x"));
}

#[test]
fn ordered_is_strict() {
    assert!(!ordered("draft", "draft"));
    assert_eq!(Ordering::Equal, compare("draft", "draft"));
}

#[test]
fn leading_digits_parse() {
    assert_eq!(Some(1), leading_number("01-intro"));
    assert_eq!(Some(2), leading_number("2. setup"));
    assert_eq!(Some(10), leading_number("10"));
    assert_eq!(Some(7), leading_number("007"));
}

#[test]
fn unnumbered_names_do_not() {
    assert_eq!(None, leading_number("appendix"));
    assert_eq!(None, leading_number("-3 degrees")); // a sign is not a digit
    assert_eq!(None, leading_number(""));
    assert_eq!(None, leading_number("99999999999999999999-overflow"));
    assert_eq!(None, leading_number("\u{0663}-arabic")); // only ASCII '0'..'9'
}

#[test]
fn embedded_numbers_compare_numerically() {
    assert_eq!(Ordering::Less, natural_compare("part2", "part10"));
    assert_eq!(Ordering::Greater, natural_compare("part10", "part2"));
    assert_eq!(Ordering::Less, natural_compare("v1.9", "v1.10"));
}

#[test]
fn natural_compare_is_case_insensitive_with_deterministic_ties() {
    assert_eq!(Ordering::Less, natural_compare("Apple", "banana"));
    assert_eq!(Ordering::Equal, natural_compare("draft", "draft"));
    // Same value, different padding / case: ordered, never "equal".
    assert_ne!(Ordering::Equal, natural_compare("part01", "part1"));
    assert_ne!(Ordering::Equal, natural_compare("Draft", "draft"));
}

#[test]
fn numbers_sort_numerically_not_lexicographically() {
    assert_eq!(vec!["01-intro", "2. setup", "10-deploy"], sorted(&["10-deploy", "01-intro", "2. setup"]));
}

#[test]
fn numbered_come_before_unnumbered() {
    assert_eq!(vec!["01-intro", "2-body", "appendix", "Zebra"], sorted(&["appendix", "01-intro", "Zebra", "2-body"]));
}

#[test]
fn unnumbered_sort_naturally() {
    assert_eq!(vec!["Apple", "banana", "cherry"], sorted(&["banana", "Apple", "cherry"]));
    // The Finder rule reaches embedded numbers too: part2 before part10.
    assert_eq!(vec!["epilogue", "part2", "part10"], sorted(&["part10", "part2", "epilogue"]));
}

#[test]
fn equal_numbers_fall_back_to_natural_order() {
    assert_eq!(vec!["01-a", "1-b"], sorted(&["1-b", "01-a"]));
}

/// LINQ `OrderBy(n => n, BookOrder.Comparer)`: a stable sort by the book order.
fn sorted(names: &[&'static str]) -> Vec<&'static str> {
    let mut list = names.to_vec();
    list.sort_by(|a, b| compare(a, b));
    list
}
