//! Port of md.win `tests/Md.App.Logic.Tests/Preview/EditorJumpTests.cs`: the one-shot rule both
//! halves of a Contents / Notes pick obey (§3.3, §5.6) — the request is the identity, not the
//! destination, so the same heading or note can be picked twice in a row, and a pane rebuilt by
//! Split → Edit → Split never replays the last one.

use md_logic::preview::{EditorJump, EditorJumpTracker, JumpId};

#[test]
fn a_jump_is_claimed_once() {
    let mut tracker = EditorJumpTracker::new();
    let jump = EditorJump::new(JumpId::next(), 12);

    assert!(tracker.claim(Some(&jump)));
    assert!(!tracker.claim(Some(&jump)));
    assert!(!tracker.claim(Some(&EditorJump::new(jump.id, 99)))); // the id is the identity
}

#[test]
fn the_same_line_twice_is_two_jumps() {
    let mut tracker = EditorJumpTracker::new();

    assert!(tracker.claim(Some(&EditorJump::new(JumpId::next(), 12))));
    assert!(tracker.claim(Some(&EditorJump::new(JumpId::next(), 12))));
}

#[test]
fn no_request_is_never_a_jump() {
    let mut tracker = EditorJumpTracker::new();
    assert!(!tracker.claim(None));
}

#[test]
fn a_rebuilt_tracker_starts_clean() {
    // The counterpart of the coordinator's dedupe: a request the owner has not cleared is performed
    // again by a fresh pane, which is why the owner clears it once the jump is handled.
    let jump = EditorJump::new(JumpId::next(), 3);
    assert!(EditorJumpTracker::new().claim(Some(&jump)));
    assert!(EditorJumpTracker::new().claim(Some(&jump)));
}

#[test]
fn jump_ids_are_fresh() {
    // md.win's Guid.NewGuid(); the tests above rely on two calls never colliding.
    let (a, b) = (JumpId::next(), JumpId::next());
    assert_ne!(a, b);
}
