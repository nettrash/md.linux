//! JavaScript over a `webkit::WebView` (shell-design §4.4, Appendix B): evaluation that yields JSON
//! text, the two-frame wait, and the snapshot scroll — the two scripts the shell keeps of its own;
//! every other script is md-logic's `preview::scripts`.
//!
//! The spike's `util.rs` JSON helpers, kept (docs/linux-facts.md §4): `evaluate_javascript_future`
//! hands back a `javascriptcore::Value` and `to_json(0)` serialises it — `"1"` with its quotes for a
//! string, `null` as four letters — which is exactly the text md-logic's `json_script` decodes.
// P4 → P1 / P5: until the windows construct previews and exports, only the self-test reaches this;
// removed at the integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use md_logic::preview::scripts::invariant_number;
use webkit::prelude::*;

/// The two-frame wait (Appendix B), as the body of an async function: resolves after the page has
/// produced two animation frames, so a scroll or a zoom has been painted.
pub const TWO_FRAMES: &str = "await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));";

/// Evaluates `script` and returns the result as JSON text. `undefined` (a void script) is `null`, as
/// md-logic's seams expect; a thrown exception is `Err` with WebKit's message.
pub async fn eval_json(view: &webkit::WebView, script: &str) -> Result<String, String> {
    match view.evaluate_javascript_future(script, None, None).await {
        Ok(value) if value.is_undefined() => Ok("null".to_owned()),
        Ok(value) => Ok(value.to_json(0).map(|s| s.to_string()).unwrap_or_else(|| "null".to_owned())),
        Err(err) => Err(err.message().to_owned()),
    }
}

/// [`TWO_FRAMES`] through `call_async_javascript_function_future` (an evaluation would hand back the
/// promise unresolved).
pub async fn two_frames(view: &webkit::WebView) -> Result<(), String> {
    view.call_async_javascript_function_future(TWO_FRAMES, None, None, None)
        .await
        .map(|_| ())
        .map_err(|err| err.message().to_owned())
}

/// The snapshot scroll (Appendix B): go to `y` and answer where the page really is — the end of the
/// document clamps it.
pub fn snapshot_scroll(y: f64) -> String {
    format!("window.scrollTo(0, {}); window.scrollY", invariant_number(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_scroll_reads_back_where_it_landed() {
        assert_eq!(snapshot_scroll(0.0), "window.scrollTo(0, 0); window.scrollY");
        assert_eq!(snapshot_scroll(1234.5), "window.scrollTo(0, 1234.5); window.scrollY");
    }

    #[test]
    fn the_two_frame_wait_is_an_async_body() {
        assert!(TWO_FRAMES.starts_with("await "));
        assert_eq!(TWO_FRAMES.matches("requestAnimationFrame").count(), 2);
    }
}
