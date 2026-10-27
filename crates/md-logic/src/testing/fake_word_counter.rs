//! Port of md.win `FakeWordCounter`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use md_core::text::unicode;

use crate::seams::WordCounter;

/// [`WordCounter`]: by default the number of whitespace-separated runs that contain a letter or
/// digit (enough for scheduler and footer tests; the real counter has its own vectors);
/// [`FakeWordCounter::set_override`] scripts any other answer. Calls are recorded.
///
/// Whitespace is .NET's `char.IsWhiteSpace` and letter-or-digit `Rune.IsLetterOrDigit`
/// (md-core `unicode`). md.win tests each UTF-16 unit, so a word made only of astral letters
/// ("𝐀") counts here and not there — no test of a fake depends on it.
#[derive(Default)]
pub struct FakeWordCounter {
    calls: RefCell<Vec<String>>,
    override_count: RefCell<Option<Rc<dyn Fn(&str) -> usize>>>,
}

impl fmt::Debug for FakeWordCounter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeWordCounter").field("calls", &self.calls.borrow().len()).finish()
    }
}

impl FakeWordCounter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every text counted, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Answers with `count` from now on.
    pub fn set_override(&self, count: impl Fn(&str) -> usize + 'static) {
        *self.override_count.borrow_mut() = Some(Rc::new(count));
    }

    /// Back to the default count.
    pub fn clear_override(&self) {
        *self.override_count.borrow_mut() = None;
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
        self.calls.borrow_mut().push(text.to_string());
        let custom = self.override_count.borrow().clone();
        match custom {
            Some(f) => f(text),
            None => Self::default_count(text),
        }
    }
}
