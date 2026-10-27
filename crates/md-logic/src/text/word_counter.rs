//! The footer's word count (shell-design §5.5) — the port of md.win `Text/SimpleWordCounter.cs`,
//! standing in for `Text/IcuWordCounter.cs` as well.
//!
//! LINUX DECISION: md.win runs two counters — `IcuWordCounter` over Windows' in-box `icu.dll`
//! (the only one with ICU's *dictionary* segmentation, so CJK and Thai count as on macOS and
//! Android) and `SimpleWordCounter` (md-core's UAX #29 counter) as the fallback. md-logic depends on
//! md-core and std only, and Linux has no ICU the app may assume, so there is one counter:
//! md-core's `writing_stats::words` — UAX #29 with ICU's root tailorings, which agrees with
//! Foundation and Android on everything but dictionary segmentation. A CJK ideograph or a Hiragana
//! syllable counts as one word each ("北京大学" is 4 here, 2 on macOS), and an unspaced Thai run is
//! one word; md-core documents the gap.

use std::sync::Arc;

use md_core::book::writing_stats;

use crate::seams::WordCounter;

/// [`WordCounter`] over md-core's `writing_stats::words`. Stateless; [`create`] hands out the one
/// the app runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WritingStatsWordCounter;

impl WordCounter for WritingStatsWordCounter {
    fn count(&self, text: &str) -> usize {
        writing_stats::words(text)
    }
}

/// The counter the app runs — one place, so the footer, the book footer and a self-test cannot end
/// up on different rules (md.win's `WordCounters.Create`). Never fails. An `Arc`, because the
/// derived text takes it off the UI thread ([`OffThread`](crate::seams::OffThread)).
pub fn create() -> Arc<dyn WordCounter> {
    Arc::new(WritingStatsWordCounter)
}
