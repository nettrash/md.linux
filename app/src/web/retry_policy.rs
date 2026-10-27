//! Pure: a port of the Mac's `PreviewRetryPolicy` (shell-design §4.9) with its Swift tests — what
//! the live preview does when the web process that renders it dies. The first death reloads, a
//! second with no completed render between them gives up with the quiet line, an edit buys one
//! attempt without touching the count, and an `mdRender` completion of the *document* resets it.
//!
//! Only termination is counted (`Crashed`, `ExceededMemoryLimit`); "unresponsive" never is — a long
//! PlantUML or Graphviz layout looks exactly like a hung web process. `TerminatedByApi` is md's own
//! close and is not counted either. The mapping from WebKit's reason lives in `preview_host`; the
//! counting lives here, away from the `WebView`, so it can be tested.
//!
//! The count belongs to the preview host, which lives as long as its window or book pane: the
//! hidden pane of Edit mode keeps its `WebView` in the tree (shell-design §5.2), so a mode switch is
//! not a new pane and needs none of the Mac's `PreviewRecovery` hand-over.

/// How many terminations in a row the preview absorbs before it stops. The second one is the last:
/// it is the one that shows the notice.
pub const LIMIT: u32 = 2;

/// The one quiet line the pane shows once md has stopped retrying — the Mac's
/// `PreviewRetryPolicy.noticeSource`, byte for byte, from md-logic's strings (shell-design §15),
/// rendered through the ordinary document pipeline.
pub use md_logic::strings::preview::STOPPED_NOTICE_SOURCE as NOTICE_SOURCE;

/// What the pane does about a termination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetryResponse {
    /// First death of this run: load the document again.
    Reload,
    /// Second death in a row: stop retrying and show [`NOTICE_SOURCE`].
    GiveUp,
    /// md has already given up; do nothing at all.
    Ignore,
}

/// The preview's consecutive-failure counter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetryPolicy {
    failures: u32,
    has_given_up: bool,
}

impl RetryPolicy {
    pub fn new() -> RetryPolicy {
        RetryPolicy::default()
    }

    /// Terminations since the last successful render.
    #[cfg(any(test, feature = "selftest"))]
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// True once [`LIMIT`] terminations happened in a row: the pane shows the notice and further
    /// terminations are not retried. Stored, not derived, because an edit clears it while the
    /// count stands.
    #[cfg(any(test, feature = "selftest"))]
    pub fn has_given_up(&self) -> bool {
        self.has_given_up
    }

    /// The web process died.
    pub fn content_process_did_terminate(&mut self) -> RetryResponse {
        if self.has_given_up {
            return RetryResponse::Ignore;
        }
        self.failures += 1;
        if self.failures < LIMIT {
            return RetryResponse::Reload;
        }
        self.has_given_up = true;
        RetryResponse::GiveUp
    }

    /// A render of the *document* finished (md-init.js's `mdRender` message — never the load
    /// event, which arrives before a single engine has run, and never the notice's own render).
    pub fn render_did_finish(&mut self) {
        self.failures = 0;
        self.has_given_up = false;
    }

    /// The document, the theme or the article changed: the load that follows is one more attempt.
    /// The count stands — in Split mode an edit arrives on every keystroke, and zeroing it there
    /// would refill the budget for a document that cannot render.
    pub fn document_did_change(&mut self) {
        self.has_given_up = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The two-strike rule.

    #[test]
    fn first_termination_reloads() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.failures(), 0);
        assert!(!policy.has_given_up());
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        assert_eq!(policy.failures(), 1);
        assert!(!policy.has_given_up(), "one death is bad luck, not a verdict");
    }

    #[test]
    fn second_termination_in_a_row_gives_up() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        assert!(policy.has_given_up());
        assert_eq!(policy.failures(), LIMIT);
    }

    #[test]
    fn after_giving_up_nothing_is_retried_and_nothing_is_shown_twice() {
        let mut policy = RetryPolicy::new();
        let _ = policy.content_process_did_terminate();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Ignore);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Ignore);
        assert_eq!(policy.failures(), LIMIT, "an ignored death is not counted either");
    }

    // What resets the count.

    #[test]
    fn a_render_between_two_deaths_makes_them_both_first_deaths() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        policy.render_did_finish();
        assert_eq!(policy.failures(), 0);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        assert!(!policy.has_given_up());
    }

    #[test]
    fn a_render_after_giving_up_starts_the_count_over() {
        let mut policy = RetryPolicy::new();
        let _ = policy.content_process_did_terminate();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        policy.render_did_finish();
        assert!(!policy.has_given_up());
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
    }

    #[test]
    fn editing_takes_the_policy_out_of_giving_up_but_buys_one_attempt() {
        let mut policy = RetryPolicy::new();
        let _ = policy.content_process_did_terminate();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        assert!(policy.has_given_up());
        policy.document_did_change();
        assert!(!policy.has_given_up());
        assert_eq!(policy.failures(), LIMIT, "nothing rendered, so the run of failures still stands");
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        assert!(policy.has_given_up());
    }

    #[test]
    fn keystrokes_cannot_refill_the_retry_budget() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        for _ in 0..50 {
            policy.document_did_change();
        }
        assert_eq!(policy.failures(), 1);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
    }

    #[test]
    fn resetting_an_untroubled_policy_changes_nothing() {
        let mut policy = RetryPolicy::new();
        policy.render_did_finish();
        policy.document_did_change();
        assert_eq!(policy, RetryPolicy::new());
    }

    // A full run.

    #[test]
    fn the_whole_sequence_of_a_document_that_kills_the_process_every_time() {
        let mut policy = RetryPolicy::new();
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Reload);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::GiveUp);
        assert_eq!(policy.content_process_did_terminate(), RetryResponse::Ignore);
        policy.document_did_change();
        assert!(!policy.has_given_up());
        policy.render_did_finish();
        assert_eq!(policy, RetryPolicy::new(), "back where it started");
    }

    // The notice.

    #[test]
    fn the_notice_is_one_quiet_line() {
        assert!(!NOTICE_SOURCE.is_empty());
        assert!(!NOTICE_SOURCE.contains(['\n', '\r']), "one line, not a paragraph");
        assert!(!NOTICE_SOURCE.contains(['<', '>']), "Markdown, not HTML");
        assert!(NOTICE_SOURCE.contains('\u{2014}'), "the Mac's em dash");
    }

    #[test]
    fn the_notice_renders_as_a_plain_page_with_no_engines() {
        let html = md_core::markdown::html::document(NOTICE_SOURCE, "Untitled", false, false);
        assert!(html.contains("The preview stopped unexpectedly"));
        assert!(html.contains("<em>"), "the emphasis is real markup");
        for engine in [
            "rich/katex.min.js",
            "rich/katex.min.css",
            "rich/mhchem.min.js",
            "rich/mermaid.min.js",
            "rich/viz-global.js",
            "rich/highlight.min.js",
        ] {
            assert!(!html.contains(engine), "the notice page pulled in {engine}");
        }
    }
}
