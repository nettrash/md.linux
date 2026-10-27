//! Port of md.win `FakeWordCounter`.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use md_core::text::unicode;

use crate::seams::WordCounter;

/// [`WordCounter`]: by default the number of whitespace-separated runs that contain a letter or
/// digit (enough for scheduler and footer tests; the real counter has its own vectors);
/// [`FakeWordCounter::set_override`] scripts any other answer. Calls are recorded.
///
/// Whitespace is .NET's `char.IsWhiteSpace` and letter-or-digit `Rune.IsLetterOrDigit`
/// (md-core `unicode`). md.win tests each UTF-16 unit, so a word made only of astral letters
/// ("𝐀") counts here and not there — no test of a fake depends on it.
///
/// Thread-safe (its state behind mutexes), because the derived text counts words off the UI thread
/// ([`OffThread`](crate::seams::OffThread)) and a test may run that work on a real thread.
#[derive(Default)]
pub struct FakeWordCounter {
    calls: Mutex<Vec<String>>,
    override_count: Mutex<Option<Arc<dyn Fn(&str) -> usize + Send + Sync>>>,
}

impl fmt::Debug for FakeWordCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeWordCounter").field("calls", &lock(&self.calls).len()).finish()
    }
}

/// A test that panicked while holding the lock has already failed; the data is still usable.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeWordCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every text counted, in order.
    pub fn calls(&self) -> Vec<String> {
        lock(&self.calls).clone()
    }

    /// Answers with `count` from now on.
    pub fn set_override(&self, count: impl Fn(&str) -> usize + Send + Sync + 'static) {
        *lock(&self.override_count) = Some(Arc::new(count));
    }

    /// Back to the default count.
    pub fn clear_override(&self) {
        *lock(&self.override_count) = None;
    }

    /// The default count, without recording a call.
    pub fn default_count(text: &str) -> usize {
        text.split(unicode::is_dotnet_white_space)
            .filter(|word| !word.is_empty() && word.chars().any(unicode::is_letter_or_digit))
            .count()
    }
}

impl WordCounter for FakeWordCounter {
    fn count(&self, text: &str) -> usize {
        lock(&self.calls).push(text.to_string());
        let custom = lock(&self.override_count).clone();
        match custom {
            Some(f) => f(text),
            None => Self::default_count(text),
        }
    }
}
