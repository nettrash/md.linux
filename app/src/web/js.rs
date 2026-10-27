//! JavaScript helpers over a `webkit::WebView` (shell-design §4.4, Appendix B): evaluation that
//! yields JSON text, the two-frame wait, the snapshot scroll script.
//!
//! S0 moved the spike's `util.rs` JSON helpers here as the starting point.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use webkit::prelude::*;

/// Evaluates `script` and returns the result as JSON text (`"1"` with its quotes, `null`, `[1,2]`):
/// `webkit_web_view_evaluate_javascript` hands back a `JSCValue`, `jsc_value_to_json` serialises
/// it. A thrown exception arrives as `Err`.
pub async fn eval_json(view: &webkit::WebView, script: &str) -> Result<String, String> {
    match view.evaluate_javascript_future(script, None, None).await {
        Ok(value) => {
            if value.is_undefined() {
                Ok("undefined".to_owned())
            } else {
                Ok(value.to_json(0).map(|s| s.to_string()).unwrap_or_else(|| "null".to_owned()))
            }
        }
        Err(err) => Err(err.message().to_owned()),
    }
}

/// Evaluates `script` and reads the result as a number (NaN when it is not one).
pub async fn eval_number(view: &webkit::WebView, script: &str) -> f64 {
    match view.evaluate_javascript_future(script, None, None).await {
        Ok(value) if value.is_number() => value.to_double(),
        _ => f64::NAN,
    }
}

/// A JSON string literal for `s` (enough for JS string arguments).
pub fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_str_escapes_quotes_controls_and_keeps_unicode() {
        assert_eq!(json_str("a\"b\\c\nd\u{1}é"), "\"a\\\"b\\\\c\\nd\\u0001é\"");
    }
}
