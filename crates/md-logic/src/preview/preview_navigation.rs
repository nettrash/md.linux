//! Port of md.win `Preview/PreviewNavigation.cs`, plus the jump-request identity both jump kinds
//! share.

use std::sync::atomic::{AtomicU64, Ordering};

/// The identity of one jump request — md.win's `Guid`. Fresh on every request
/// ([`JumpId::next`]), so two picks of the same heading or note are two jumps; only the request
/// is the identity, never its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JumpId(pub u64);

impl JumpId {
    /// A process-unique id (`Guid.NewGuid()`).
    pub fn next() -> JumpId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        JumpId(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// One request to scroll the preview to a heading (§4.5, §5.6). The id is fresh on every request
/// and the coordinator performs each id exactly once, which is what lets two consecutive picks of
/// the *same* heading both work — the slug is not the identity, the request is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PreviewNavigation {
    /// Fresh per request.
    pub id: JumpId,
    /// The heading's slug, as md-core's parser emits it: letters, digits, `-`, `_`.
    pub slug: String,
}

impl PreviewNavigation {
    pub fn new(id: JumpId, slug: impl Into<String>) -> Self {
        PreviewNavigation { id, slug: slug.into() }
    }
}
