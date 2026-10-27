//! Port of md.win `tests/Md.App.Logic.Tests/TextSearchTests.cs`.
//!
//! §3.5: the find bar wraps, ignores case, and hands back a range the editor can select as is.
//! Offsets are UTF-16 units, as in md.win; [`applied`] does the shell's edit in those units.

use md_logic::documents::text_search::{self, Edit, Match, ReplaceAllPlan};

const TEXT: &str = "alpha beta Alpha gamma alpha";
//                  0          11          23

fn utf16_len(s: &str) -> i64 {
    s.encode_utf16().count() as i64
}

#[test]
fn next_finds_the_first_hit_at_or_after_the_caret() {
    assert_eq!(text_search::next(TEXT, "alpha", 0), Some(Match::new(0, 5)));
    assert_eq!(text_search::next(TEXT, "alpha", 1), Some(Match::new(11, 5)));
    assert_eq!(text_search::next(TEXT, "alpha", 12), Some(Match::new(23, 5)));
}

#[test]
fn next_wraps_to_the_top_when_nothing_follows() {
    assert_eq!(text_search::next(TEXT, "alpha", 23), Some(Match::new(23, 5))); // "at or after"
    assert_eq!(text_search::next(TEXT, "alpha", 24), Some(Match::new(0, 5)));
    assert_eq!(text_search::next(TEXT, "alpha", utf16_len(TEXT)), Some(Match::new(0, 5)));
    assert_eq!(text_search::next(TEXT, "alpha", 9999), Some(Match::new(0, 5)));
}

#[test]
fn previous_finds_the_last_hit_starting_before_the_caret() {
    assert_eq!(text_search::previous(TEXT, "alpha", 22), Some(Match::new(11, 5)));
    assert_eq!(text_search::previous(TEXT, "alpha", 11), Some(Match::new(0, 5)));
    assert_eq!(text_search::previous(TEXT, "alpha", utf16_len(TEXT)), Some(Match::new(23, 5)));
}

#[test]
fn previous_wraps_to_the_bottom_when_nothing_precedes() {
    assert_eq!(text_search::previous(TEXT, "alpha", 0), Some(Match::new(23, 5)));
    assert_eq!(text_search::previous(TEXT, "alpha", -5), Some(Match::new(23, 5)));
}

#[test]
fn matching_is_ordinal_and_case_insensitive() {
    assert_eq!(text_search::next(TEXT, "ALPHA", 1), Some(Match::new(11, 5)));
    // Ordinal, so no culture folds these together and the length always equals the query's.
    assert_eq!(text_search::next("straße", "strasse", 0), None);
    assert_eq!(text_search::next("Straße x", "straße", 0).unwrap().length, 6);
}

#[test]
fn an_empty_or_oversized_query_finds_nothing() {
    assert_eq!(text_search::next(TEXT, "", 0), None);
    assert_eq!(text_search::previous(TEXT, "", 0), None);
    assert_eq!(text_search::next("ab", "abc", 0), None);
    assert_eq!(text_search::next(TEXT, "zeta", 0), None);
    assert_eq!(text_search::previous(TEXT, "zeta", 0), None);
}

#[test]
fn a_single_hit_is_found_from_anywhere_in_both_directions() {
    let one = "xxxNEEDLExxx";
    assert_eq!(text_search::next(one, "needle", 0), Some(Match::new(3, 6)));
    assert_eq!(text_search::next(one, "needle", 5), Some(Match::new(3, 6))); // wraps onto itself
    assert_eq!(text_search::previous(one, "needle", 0), Some(Match::new(3, 6)));
    assert_eq!(text_search::previous(one, "needle", utf16_len(one)), Some(Match::new(3, 6)));
}

#[test]
fn overlapping_hits_are_each_reachable() {
    let aaa = "aaaa";
    assert_eq!(text_search::next(aaa, "aa", 0), Some(Match::new(0, 2)));
    assert_eq!(text_search::next(aaa, "aa", 1), Some(Match::new(1, 2)));
    assert_eq!(text_search::next(aaa, "aa", 2), Some(Match::new(2, 2)));
    assert_eq!(text_search::previous(aaa, "aa", 3), Some(Match::new(2, 2)));
}

// ── §3.5 Replace: the same one matching rule, and nothing decided in the shell ──────────────────

/// The whole of Replace's decision: is the selection the hit we are standing on?
#[test]
fn the_selection_is_the_match_only_when_it_spans_one_hit_exactly() {
    assert!(text_search::selection_is_match(TEXT, "alpha", 0, 5));
    assert!(text_search::selection_is_match(TEXT, "alpha", 11, 5)); // "Alpha" — case-insensitive
    assert!(text_search::selection_is_match(TEXT, "ALPHA", 23, 5));
    assert!(text_search::selection_is_match(TEXT, "alpha", 23, 5)); // the hit at the very end

    assert!(!text_search::selection_is_match(TEXT, "alpha", 0, 0)); // a caret is not a match
    assert!(!text_search::selection_is_match(TEXT, "alpha", 0, 4)); // short of the query
    assert!(!text_search::selection_is_match(TEXT, "alpha", 0, 6)); // past it
    assert!(!text_search::selection_is_match(TEXT, "alpha", 1, 5)); // the nearest hit is elsewhere
    assert!(!text_search::selection_is_match(TEXT, "", 0, 0)); // an empty query matches nothing
    assert!(!text_search::selection_is_match(TEXT, "alpha", -1, 5)); // off either end, rather than a panic
    assert!(!text_search::selection_is_match(TEXT, "alpha", 24, 5));
    assert!(!text_search::selection_is_match("straße", "strasse", 0, 7)); // ordinal, like the search
}

#[test]
fn replace_edits_the_selection_when_it_is_the_hit_and_searches_on_from_after_the_replacement() {
    let step = text_search::replace(TEXT, "alpha", "omega", 11, 5);
    assert_eq!(step.apply, Some(Edit::new(11, 5, "omega")));
    assert_eq!(step.search_from, 16);

    // What the window then does: apply, re-read, Next from search_from.
    let after = applied(TEXT, step.apply.as_ref().unwrap());
    assert_eq!(after, "alpha beta omega gamma alpha");
    assert_eq!(text_search::next(&after, "alpha", step.search_from as i64), Some(Match::new(23, 5)));
}

#[test]
fn replace_is_a_plain_find_next_when_the_selection_is_not_the_hit() {
    // A caret inside the word, a selection of the wrong length, and one over other text.
    for (start, length) in [(2, 0), (0, 4), (6, 4)] {
        let step = text_search::replace(TEXT, "alpha", "omega", start, length);
        assert_eq!(step.apply, None);
        assert_eq!(step.search_from as i64, start + length);
    }

    assert_eq!(text_search::replace(TEXT, "", "omega", 0, 0).apply, None);
    assert_eq!(text_search::replace(TEXT, "zeta", "omega", 0, 4).apply, None);
}

#[test]
fn replace_clamps_a_selection_the_control_could_not_have_had() {
    let step = text_search::replace(TEXT, "alpha", "omega", 9999, 9999);
    assert_eq!(step.apply, None);
    assert_eq!(step.search_from as i64, utf16_len(TEXT));

    let negative = text_search::replace(TEXT, "alpha", "omega", -4, -4);
    assert_eq!(negative.apply, None);
    assert_eq!(negative.search_from, 0);
}

#[test]
fn replace_at_the_very_start_and_the_very_end_are_both_the_hit_they_stand_on() {
    let first = text_search::replace(TEXT, "alpha", "x", 0, 5);
    assert_eq!(first.apply, Some(Edit::new(0, 5, "x")));
    assert_eq!(first.search_from, 1);
    assert_eq!(applied(TEXT, first.apply.as_ref().unwrap()), "x beta Alpha gamma alpha");

    let last = text_search::replace(TEXT, "alpha", "x", 23, 5);
    assert_eq!(last.apply, Some(Edit::new(23, 5, "x")));
    assert_eq!(last.search_from, 24);
    assert_eq!(applied(TEXT, last.apply.as_ref().unwrap()), "alpha beta Alpha gamma x");
}

/// The loop this rule exists to prevent: "a" → "aa" is a replacement that contains the query, and
/// search_from is past what went in, so the very next Next cannot land inside it.
#[test]
fn a_replacement_that_contains_the_query_is_not_searched_again() {
    let step = text_search::replace("aaa", "a", "aa", 0, 1);
    assert_eq!(step.apply, Some(Edit::new(0, 1, "aa")));
    assert_eq!(step.search_from, 2);
    assert_eq!(applied("aaa", step.apply.as_ref().unwrap()), "aaaa");
    assert_eq!(text_search::next("aaaa", "a", step.search_from as i64), Some(Match::new(2, 1)));
}

#[test]
fn a_replacement_equal_to_the_query_still_counts_and_still_moves() {
    let step = text_search::replace(TEXT, "alpha", "alpha", 11, 5);
    assert_eq!(step.apply, Some(Edit::new(11, 5, "alpha")));
    assert_eq!(step.search_from, 16);
    // "Alpha" became "alpha": the text DID change, which is the point of a case-insensitive find.
    assert_eq!(applied(TEXT, step.apply.as_ref().unwrap()), "alpha beta alpha gamma alpha");
}

#[test]
fn an_empty_replacement_is_a_deletion() {
    let step = text_search::replace(TEXT, "alpha ", "", 0, 6);
    assert_eq!(step.apply, Some(Edit::new(0, 6, "")));
    assert_eq!(step.search_from, 0);
    assert_eq!(applied(TEXT, step.apply.as_ref().unwrap()), "beta Alpha gamma alpha");
}

// ── Replace All ─────────────────────────────────────────────────────────────────────────────────

#[test]
fn replace_all_replaces_every_hit_case_insensitively_and_counts_them() {
    let plan = text_search::replace_all(TEXT, "alpha", "omega");
    assert_eq!(plan.count, 3);
    assert_eq!(plan.text, "omega beta omega gamma omega");
    assert_plan_is_one_edit(TEXT, &plan);
}

/// The edit is the span from the first hit to the last, not the whole document: that is what lets
/// the shell put it in as one buffer edit (one undo unit) without rewriting text nobody asked it to
/// touch.
#[test]
fn the_replace_all_plan_is_the_one_span_from_the_first_hit_to_the_last() {
    let text = "head alpha middle alpha tail";
    let plan = text_search::replace_all(text, "alpha", "X");
    assert_eq!(plan.count, 2);
    assert_eq!(plan.text, "head X middle X tail");
    assert_eq!(plan.apply, Edit::new(5, 18, "X middle X"));
    assert_plan_is_one_edit(text, &plan);
}

#[test]
fn replace_all_with_nothing_to_do_leaves_the_text_and_plans_an_empty_edit() {
    let longer = format!("{TEXT} and more");
    for query in ["", "zeta", longer.as_str()] {
        let plan = text_search::replace_all(TEXT, query, "X");
        assert_eq!(plan.count, 0);
        assert_eq!(plan.text, TEXT);
        assert_eq!(plan.apply, Edit::new(0, 0, ""));
        assert_plan_is_one_edit(TEXT, &plan);
    }
}

#[test]
fn replace_all_takes_hits_at_the_very_start_and_the_very_end() {
    let plan = text_search::replace_all("xx middle xx", "xx", "Y");
    assert_eq!(plan.count, 2);
    assert_eq!(plan.text, "Y middle Y");
    assert_eq!(plan.apply, Edit::new(0, 12, "Y middle Y"));
    assert_plan_is_one_edit("xx middle xx", &plan);
}

/// Hits are taken left to right and never overlap: "aa" over "aaaa" is two, over "aaaaa" two with
/// an "a" left over.
#[test]
fn overlapping_candidates_are_taken_left_to_right_without_overlapping() {
    let four = text_search::replace_all("aaaa", "aa", "b");
    assert_eq!(four.count, 2);
    assert_eq!(four.text, "bb");
    assert_plan_is_one_edit("aaaa", &four);

    let five = text_search::replace_all("aaaaa", "aa", "b");
    assert_eq!(five.count, 2);
    assert_eq!(five.text, "bba");
    assert_eq!(five.apply, Edit::new(0, 4, "bb"));
    assert_plan_is_one_edit("aaaaa", &five);
}

/// One pass, left to right, never rescanning what it inserted — or this test never returns.
#[test]
fn a_replacement_containing_the_query_replaces_each_hit_exactly_once() {
    let plan = text_search::replace_all("aaa", "a", "aa");
    assert_eq!(plan.count, 3);
    assert_eq!(plan.text, "aaaaaa");
    assert_plan_is_one_edit("aaa", &plan);

    let longer = text_search::replace_all("one two", "o", "foo");
    assert_eq!(longer.count, 2);
    assert_eq!(longer.text, "foone twfoo");
    assert_plan_is_one_edit("one two", &longer);
}

#[test]
fn a_replacement_equal_to_the_query_only_changes_the_casing_that_differed() {
    let plan = text_search::replace_all(TEXT, "alpha", "alpha");
    assert_eq!(plan.count, 3);
    assert_eq!(plan.text, "alpha beta alpha gamma alpha");
    assert_plan_is_one_edit(TEXT, &plan);

    // Nothing differed: the text comes back byte for byte, and the count still says what happened.
    let same = text_search::replace_all("alpha alpha", "alpha", "alpha");
    assert_eq!(same.count, 2);
    assert_eq!(same.text, "alpha alpha");
    assert_plan_is_one_edit("alpha alpha", &same);
}

/// A pasted CR or CRLF may sit in the editor; the engine is ordinal over UTF-16 units and never
/// treats a line end as anything else — including a query that spans one.
#[test]
fn line_ends_are_ordinary_characters_on_both_sides_of_the_replace() {
    let crlf = "one\r\ntwo\r\nthree";
    let plan = text_search::replace_all(crlf, "\r\n", "\n");
    assert_eq!(plan.count, 2);
    assert_eq!(plan.text, "one\ntwo\nthree");
    assert_plan_is_one_edit(crlf, &plan);

    // A query with a line end inside it, replaced by text with one inside it.
    let joined = text_search::replace_all(crlf, "two\r\n", "TWO\r");
    assert_eq!(joined.count, 1);
    assert_eq!(joined.text, "one\r\nTWO\rthree");
    assert_plan_is_one_edit(crlf, &joined);

    // And the step: a CR is a unit like any other, so the offsets stay the editor's own.
    let step = text_search::replace("a\rb\rc", "\r", "\r\r", 1, 1);
    assert_eq!(step.apply, Some(Edit::new(1, 1, "\r\r")));
    assert_eq!(step.search_from, 3);
    assert_eq!(applied("a\rb\rc", step.apply.as_ref().unwrap()), "a\r\rb\rc");
}

/// Offsets are UTF-16 units (§0.1), so an emoji on either side of the hit moves it by two and the
/// edit must still land on the hit and never inside a pair.
#[test]
fn surrogate_pairs_on_both_sides_of_the_match_are_counted_as_two_units_each() {
    let text = "\u{1F600}needle\u{1F600}needle\u{1F600}";
    assert_eq!(text_search::next(text, "needle", 0).unwrap().index, 2);

    let step = text_search::replace(text, "needle", "\u{1F680}", 2, 6);
    assert_eq!(step.apply, Some(Edit::new(2, 6, "\u{1F680}")));
    assert_eq!(step.search_from, 4);
    assert_eq!(applied(text, step.apply.as_ref().unwrap()), "\u{1F600}\u{1F680}\u{1F600}needle\u{1F600}");

    let plan = text_search::replace_all(text, "needle", "\u{1F680}");
    assert_eq!(plan.count, 2);
    assert_eq!(plan.text, "\u{1F600}\u{1F680}\u{1F600}\u{1F680}\u{1F600}");
    assert_eq!(plan.apply, Edit::new(2, 14, "\u{1F680}\u{1F600}\u{1F680}"));
    assert_plan_is_one_edit(text, &plan);

    // A pair as the query itself: two units in, whatever goes out.
    let emoji = text_search::replace_all(text, "\u{1F600}", "!");
    assert_eq!(emoji.count, 3);
    assert_eq!(emoji.text, "!needle!needle!");
    assert_plan_is_one_edit(text, &emoji);
}

/// md.win pins that a null argument throws instead of being treated as empty. A Rust `&str` cannot
/// be null — the type system refuses the call — so what is left to pin is that the empty strings
/// a caller *can* pass are handled, not invented around.
#[test]
fn replace_and_replace_all_refuse_a_null_argument_rather_than_invent_one() {
    assert_eq!(text_search::replace("", "a", "b", 0, 1).apply, None);
    assert_eq!(text_search::replace("a", "", "b", 0, 1).apply, None);
    assert_eq!(text_search::replace("a", "a", "", 0, 1).apply, Some(Edit::new(0, 1, "")));
    assert_eq!(text_search::replace_all("", "a", "b").count, 0);
    assert_eq!(text_search::replace_all("a", "", "b").count, 0);
    assert_eq!(text_search::replace_all("a", "a", "").text, "");
}

/// What the shell does with one edit: that span replaced, nothing else touched — in UTF-16 units.
fn applied(text: &str, edit: &Edit) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut out: Vec<u16> = units[..edit.start].to_vec();
    out.extend(edit.text.encode_utf16());
    out.extend_from_slice(&units[edit.start + edit.length..]);
    String::from_utf16(&out).expect("an edit never splits a surrogate pair")
}

/// The plan's two halves agree: applying the single edit gives the whole new text. This is the
/// invariant the one-undo-step apply path rests on.
fn assert_plan_is_one_edit(text: &str, plan: &ReplaceAllPlan) {
    assert_eq!(applied(text, &plan.apply), plan.text);
}
