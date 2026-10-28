//! Port of md.win `tests/Md.App.Logic.Tests/FindPanelTests.cs`.
//!
//! 2026-09-27, the find panel: the count beside the query and find as you type.

use md_logic::documents::FindAsYouType;
use md_logic::documents::text_search::{self, Match, Tally};
use md_logic::strings;

const TEXT: &str = "alpha beta Alpha gamma alpha";
//                  0          11          23

#[test]
fn the_count_places_the_selected_hit_among_all_of_them() {
    assert_eq!(text_search::count(TEXT, "alpha", 0, 5), Tally::new(1, 3));
    assert_eq!(text_search::count(TEXT, "ALPHA", 11, 5), Tally::new(2, 3));
    assert_eq!(text_search::count(TEXT, "alpha", 23, 5), Tally::new(3, 3));
}

#[test]
fn off_a_hit_the_count_has_no_current() {
    assert_eq!(text_search::count(TEXT, "alpha", 6, 0), Tally::new(0, 3));
    assert_eq!(text_search::count(TEXT, "alpha", 0, 4), Tally::new(0, 3));
    assert_eq!(text_search::count(TEXT, "delta", 0, 0), Tally::new(0, 0));
    assert_eq!(text_search::count(TEXT, "", 0, 0), Tally::new(0, 0));
    assert_eq!(text_search::count("ab", "abc", 0, 0), Tally::new(0, 0));
}

#[test]
fn the_total_is_what_replace_all_would_replace() {
    for (text, query) in [("aaaa", "aa"), ("aaa", "aa"), (TEXT, "a"), ("x", "x"), ("", "x")] {
        assert_eq!(text_search::count(text, query, 0, 0).total, text_search::replace_all(text, query, "-").count);
    }
}

#[test]
fn an_overlapping_hit_is_placed_after_the_counted_ones_before_it() {
    // "aaaa" / "aa" counts 0 and 2; the hit at 1 overlaps the first and is the second.
    assert_eq!(text_search::count("aaaa", "aa", 1, 2), Tally::new(2, 2));
    // …and never past the total: "aaa" counts only the hit at 0, so the one at 1 is "1 of 1", not "2 of 1".
    assert_eq!(text_search::count("aaa", "aa", 1, 2), Tally::new(1, 1));
}

#[test]
fn typing_searches_from_where_the_caret_was() {
    let mut find = FindAsYouType::new();
    find.focused(12);
    assert_eq!(find.search(TEXT, "a"), Some(Match::new(15, 1))); // the last letter of Alpha
    assert_eq!(find.search(TEXT, "alpha"), Some(Match::new(23, 5)));
    assert_eq!(find.search(TEXT, "alph"), Some(Match::new(23, 4))); // Backspace: still from the caret
}

#[test]
fn a_longer_query_keeps_the_hit_it_already_has() {
    let mut find = FindAsYouType::new();
    find.focused(0);
    let hit = text_search::next(TEXT, "al", 1).unwrap(); // Next: the Alpha at 11
    find.stepped(hit);
    assert_eq!(find.search(TEXT, "alp"), Some(Match::new(11, 3)));
    assert_eq!(find.search(TEXT, "alpha"), Some(Match::new(11, 5)));
}

#[test]
fn typing_wraps_and_an_empty_or_absent_query_finds_nothing() {
    let mut find = FindAsYouType::new();
    find.focused(24);
    assert_eq!(find.search(TEXT, "beta"), Some(Match::new(6, 4)));
    assert_eq!(find.search(TEXT, ""), None);
    assert_eq!(find.search(TEXT, "delta"), None);
    find.focused(-3);
    assert_eq!(find.anchor(), 0);
}

#[test]
fn the_count_reads_like_windows() {
    assert_eq!(strings::find::tally(0, 0, false), "");
    assert_eq!(strings::find::tally(0, 0, true), "No results");
    assert_eq!(strings::find::tally(3, 12, true), "3 of 12");
    assert_eq!(strings::find::tally(0, 12, true), "12 results");
    assert_eq!(strings::find::tally(0, 1, true), "1 result");
    assert_eq!(strings::find::tally(1, 1, true), "1 of 1");
}
