//! Port of md.win `Md.Core.Markdown.PlotMemo`.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::text::utf16::utf16_len;

/// Entries, hits and misses. Read by the tests; nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Statistics {
    pub entries: usize,
    pub hits: usize,
    pub misses: usize,
}

impl Statistics {
    pub const fn new(entries: usize, hits: usize, misses: usize) -> Self {
        Statistics { entries, hits, misses }
    }
}

/// A bounded, thread-safe memo of finished ```plot containers, keyed on the exact fence text that
/// produced them.
///
/// The live preview rebuilds the whole HTML document on every text change, and a plot is the only
/// rich block whose real cost is paid synchronously in that render (every other engine runs
/// asynchronously in the page). Measured on the Mac port: 4.87 ms for the default figure, 187 ms
/// for four eight-series plots.
///
/// Memoising is sound because [`super::render_plot`] is a pure function of one string: no clock,
/// no locale, no theme flag (the ink is `currentColor`, the series colours are baked). A memo of a
/// pure function returns exactly what recomputing would.
///
/// The lock is load-bearing: the export paths render off the UI thread while the preview renders
/// on it. Every critical section is one map probe; the render itself happens outside the lock
/// (two threads racing the same miss both render; the second store simply wins with the same
/// bytes).
///
/// 32 entries, least recently used, the same bound md.vscode's preview uses for its diagram
/// cache.
#[derive(Debug)]
pub struct PlotMemo {
    limit: usize,
    gate: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    entries: HashMap<String, String>,
    /// Keys, oldest first. At 32 entries a linear move costs less than a linked list's
    /// bookkeeping, and it is the same two lines in every port.
    order: Vec<String>,
    hits: usize,
    misses: usize,
}

impl PlotMemo {
    /// The largest container worth remembering, in UTF-16 code units — eight times the default
    /// figure.
    ///
    /// The entry ceiling bounds the count, not the bytes: one fence may legally reach ~1.8 MB
    /// (24 series × `samples: 5000` at 2000×2000), and 32 of those would pin ~57 MB. Such a figure
    /// is also the one a memo helps least. Kotlin, TypeScript and md.win bound in UTF-16 units;
    /// Swift in UTF-8 bytes. The output is the same either way (a container that large is never
    /// memoised by any port); this port counts UTF-16 units, as md.win does.
    pub const LARGEST_MEMOISED_VALUE: usize = 128 * 1024;

    pub fn new(limit: usize) -> Self {
        PlotMemo { limit, gate: Mutex::new(State::default()) }
    }

    /// The lock, even if a panicking thread once held it: every critical section leaves the state
    /// consistent, so a poisoned lock holds nothing torn.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The memoised container for `key`, moved to the young end, or `None`.
    pub fn value(&self, key: &str) -> Option<String> {
        let mut state = self.lock();
        let Some(value) = state.entries.get(key).cloned() else {
            state.misses += 1;
            return None;
        };
        if let Some(index) = state.order.iter().position(|one| one == key) {
            let moved = state.order.remove(index);
            state.order.push(moved);
        }
        state.hits += 1;
        Some(value)
    }

    /// Memoise `value`, evicting the least recently used key past the limit.
    pub fn store(&self, value: &str, key: &str) {
        // Before the lock, deliberately: the size test needs no shared state. A UTF-16 length is
        // never more than the UTF-8 one, so only a long value needs counting.
        if value.len() > Self::LARGEST_MEMOISED_VALUE && utf16_len(value) > Self::LARGEST_MEMOISED_VALUE {
            return;
        }
        let mut state = self.lock();
        let existed = state.entries.insert(key.to_string(), value.to_string()).is_some();
        if existed && let Some(index) = state.order.iter().position(|one| one == key) {
            state.order.remove(index);
        }
        state.order.push(key.to_string());
        while state.order.len() > self.limit {
            let oldest = state.order.remove(0);
            state.entries.remove(&oldest);
        }
    }

    /// The current [`Statistics`]. Named `snapshot`, as md.win names it (C# lets no member share
    /// its name with the nested type it returns); [`super::memo_statistics`] keeps the family's
    /// name on the public surface.
    pub fn snapshot(&self) -> Statistics {
        let state = self.lock();
        Statistics { entries: state.entries.len(), hits: state.hits, misses: state.misses }
    }

    pub fn remove_all(&self) {
        let mut state = self.lock();
        state.entries.clear();
        state.order.clear();
        state.hits = 0;
        state.misses = 0;
    }
}
