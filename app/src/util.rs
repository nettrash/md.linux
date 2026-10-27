//! Small async helpers over the GLib main loop: a single-threaded one-shot channel, JavaScript
//! evaluation that yields JSON, and awaiting a child process.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use webkit::prelude::*;

struct Shared<T> {
    value: Option<T>,
    sent: bool,
    waker: Option<Waker>,
}

/// The sending half. `send` takes `&self` and only the first value counts, so it can sit in a
/// `Fn` signal handler that may fire more than once (`finished` after `failed`, say).
pub struct Sender<T>(Rc<RefCell<Shared<T>>>);

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Sender(Rc::clone(&self.0))
    }
}

pub struct Receiver<T>(Rc<RefCell<Shared<T>>>);

pub fn oneshot<T>() -> (Sender<T>, Receiver<T>) {
    let shared = Rc::new(RefCell::new(Shared { value: None, sent: false, waker: None }));
    (Sender(Rc::clone(&shared)), Receiver(shared))
}

impl<T> Sender<T> {
    pub fn send(&self, value: T) {
        let waker = {
            let mut shared = self.0.borrow_mut();
            if shared.sent {
                return;
            }
            shared.sent = true;
            shared.value = Some(value);
            shared.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> Future for Receiver<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let mut shared = self.0.borrow_mut();
        match shared.value.take() {
            Some(value) => Poll::Ready(value),
            None => {
                shared.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Awaits `fut` for at most `limit`; `None` on timeout.
pub async fn with_timeout<T>(limit: Duration, fut: impl Future<Output = T>) -> Option<T> {
    glib::future_with_timeout(limit, fut).await.ok()
}

pub async fn sleep_ms(ms: u64) {
    glib::timeout_future(Duration::from_millis(ms)).await;
}

/// Evaluates `script` in the page and returns the result as JSON text (`"1"` with its quotes,
/// `null`, `[1,2]`). `webkit_web_view_evaluate_javascript` hands back a `JSCValue`;
/// `jsc_value_to_json` serialises it. A thrown exception arrives as `Err`.
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

/// Runs a program to completion without blocking the main loop; returns (success, stdout).
pub async fn run(argv: &[&str]) -> (bool, String) {
    let os: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let flags = gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE;
    let proc = match gio::Subprocess::newv(&os, flags) {
        Ok(proc) => proc,
        Err(err) => return (false, err.to_string()),
    };
    match proc.communicate_utf8_future(None).await {
        Ok((out, _)) => (proc.is_successful(), out.map(|s| s.to_string()).unwrap_or_default()),
        Err(err) => (false, err.to_string()),
    }
}

/// A JSON string literal for `s` (enough for log values and JS string arguments).
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

    #[test]
    fn oneshot_keeps_the_first_value() {
        let (tx, rx) = oneshot();
        tx.send(1);
        tx.clone().send(2);
        let ctx = glib::MainContext::new();
        assert_eq!(ctx.block_on(rx), 1);
    }
}
