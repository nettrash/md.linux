//! Port of md.win `tests/Md.App.Logic.Tests/WordCounterTests.cs` — shell-design §5.5, the
//! footer's word count. The five vectors are pinned for every counter the app can end up running,
//! because the number is user-visible and gets compared between a writer's machines. Non-ASCII
//! vectors are escapes so nothing can normalise them away.
//!
//! LINUX: md.win's `SimpleWordCounter` is `WritingStatsWordCounter` here, and its in-box ICU counter
//! has no counterpart (see `text::word_counter`): the ICU-only tests are ignored, and the "which
//! counter runs" tests pin that the one counter is md-core's.

use md_logic::seams::WordCounter;
use md_logic::text::WritingStatsWordCounter;
use md_logic::text::word_counter;

/// "" is 0, whitespace is 0, punctuation is not a word, "it's" is one, newlines separate.
const VECTORS: [(&str, usize); 5] =
    [("", 0), ("   \n\n", 0), ("Hello, world!", 2), ("it's \u{2014} done", 2), ("One\ntwo\n\nthree", 3)];

#[test]
fn the_simple_counter_passes_the_five_vectors() {
    for (text, expected) in VECTORS {
        assert_eq!(WritingStatsWordCounter.count(text), expected, "{text:?}");
    }
}

#[test]
fn whichever_counter_this_machine_gets_passes_the_five_vectors() {
    let counter = word_counter::create();
    for (text, expected) in VECTORS {
        assert_eq!(counter.count(text), expected, "{text:?}");
    }
}

#[test]
fn the_simple_counter_is_cores_uax_twenty_nine_counter_and_not_a_word_approximation() {
    // A "runs of letters and digits" counter answers 1 for the first and 0 for the last two;
    // UAX #29 with ICU's root tailorings answers what macOS and Android answer. This is the
    // assertion that fails the day the counter is swapped for a regex.
    assert_eq!(WritingStatsWordCounter.count("a:b"), 2);
    assert_eq!(WritingStatsWordCounter.count("\u{FF11}\u{FF12}\u{FF13}"), 1); // full-width digits: one number
    assert_eq!(WritingStatsWordCounter.count("\u{216B}"), 1); // ROMAN NUMERAL TWELVE
}

/// LINUX: md.win's "OffWindowsThereIsNoIcuAndTheFallbackIsWhatRuns" asserts the factory hands back
/// the fallback instance. There is no other counter here: what `create` returns counts exactly as
/// md-core does, CJK included.
#[test]
fn off_windows_there_is_no_icu_and_the_fallback_is_what_runs() {
    let created = word_counter::create();
    for text in ["", "Hello, world!", "\u{5317}\u{4EAC}\u{5927}\u{5B66}", "a:b", "\u{0E01}1"] {
        assert_eq!(created.count(text), md_core::book::writing_stats::words(text), "{text:?}");
    }
}

#[test]
#[ignore = "ICU through icu.dll is Windows' in-box library; md-logic is md-core + std, and Linux runs md-core's \
            UAX #29 counter only"]
fn icu_passes_the_five_vectors_too() {
    // md.win: IcuWordCounter.TryCreate()?.Count(text) == expected for the five vectors.
}

/// md.win pins the gap as a relation: Core's counter makes one word per ideograph (4) where ICU's
/// dictionary finds 1–3. The first half is the Linux behaviour and stays pinned; the ICU half has no
/// counterpart here.
#[test]
fn icu_segments_cjk_with_its_dictionary_and_the_fallback_cannot() {
    const UNIVERSITY: &str = "\u{5317}\u{4EAC}\u{5927}\u{5B66}"; // "Peking University", two words to ICU
    assert_eq!(WritingStatsWordCounter.count(UNIVERSITY), 4);
    assert_eq!(word_counter::create().count(UNIVERSITY), 4);
}

#[test]
fn create_never_returns_null() {
    // An `Rc<dyn WordCounter>` cannot be null; what remains is that it answers.
    assert_eq!(word_counter::create().count("md test"), 2);
}
