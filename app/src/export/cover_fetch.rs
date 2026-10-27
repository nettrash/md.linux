//! The EPUB cover's fetch (shell-design §7.5) over libsoup 3 — WebKitGTK's own HTTP stack,
//! re-exported by `webkit6`, so no new crate: md-logic's `export::CoverFetcher` seam, which is
//! **one GET of one URL without following a redirect**, answering the status, the `Location`, the
//! declared length and a body read chunk by chunk. Everything that is policy — https only (the first
//! URL and every redirect target, checked before it is requested), at most five redirects followed
//! by hand, the 20 MiB cap, the 15 s deadline over every hop and chunk — is md-logic's
//! `cover_reader`, tested there with a fake.
//!
//! A `soup::Message` flagged `NO_REDIRECT`, `send_future` for the headers, `read_bytes_future` for
//! each chunk. Both are `GioFuture`s, whose drop cancels their `gio::Cancellable` — which is how
//! md-logic abandons a request (a redirect, a refused length, the cap, the deadline): by dropping the
//! future or the body. Dropping the body closes its stream, which closes the connection mid-transfer.
//! The session has no cookie jar, no cache and no persistent HSTS store: nothing reaches the disk.
// P4 → P1 / P5: until the windows construct exports, only the self-test reaches this; removed at the
// integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use md_logic::exec::LocalBoxFuture;
use md_logic::export::{CoverBody, CoverFetcher, CoverResponse};
use webkit::soup;
use webkit::soup::prelude::*;

/// How much of the body one read asks for.
const CHUNK: usize = 64 * 1024;

/// [`CoverFetcher`] over a libsoup session of its own.
pub struct SoupCoverFetcher {
    session: soup::Session,
}

impl SoupCoverFetcher {
    pub fn new() -> SoupCoverFetcher {
        SoupCoverFetcher { session: soup::Session::new() }
    }
}

impl CoverFetcher for SoupCoverFetcher {
    fn get(&self, url: String) -> LocalBoxFuture<'static, Result<CoverResponse, String>> {
        let session = self.session.clone();
        Box::pin(async move {
            let message = soup::Message::new("GET", &url).map_err(|err| format!("{url}: {err}"))?;
            message.set_flags(soup::MessageFlags::NO_REDIRECT);
            let stream = session
                .send_future(&message, glib::Priority::DEFAULT)
                .await
                .map_err(|err| format!("{url}: {}", err.message()))?;
            let status = u16::try_from(message.status_code()).unwrap_or(0);
            let headers = message.response_headers();
            let location = headers.as_ref().and_then(|h| h.one("Location")).map(|l| l.to_string());
            let content_length = headers
                .as_ref()
                .filter(|h| h.encoding() == soup::Encoding::ContentLength)
                .and_then(|h| u64::try_from(h.content_length()).ok());
            Ok(CoverResponse { status, location, content_length, body: Box::new(SoupBody { stream }) })
        })
    }
}

/// A response body: its stream, read on demand, closed when dropped.
struct SoupBody {
    stream: gio::InputStream,
}

impl CoverBody for SoupBody {
    fn next_chunk(&self) -> LocalBoxFuture<'static, Result<Option<Vec<u8>>, String>> {
        let stream = self.stream.clone();
        Box::pin(async move {
            let bytes = stream
                .read_bytes_future(CHUNK, glib::Priority::DEFAULT)
                .await
                .map_err(|err| err.message().to_owned())?;
            Ok((!bytes.is_empty()).then(|| bytes.to_vec()))
        })
    }
}

impl Drop for SoupBody {
    fn drop(&mut self) {
        // Abandoned or finished: the transfer stops here. Asynchronous, so a slow peer cannot hold
        // the main loop; the callback keeps the stream alive until it is closed.
        if !self.stream.is_closed() {
            self.stream.close_async(glib::Priority::DEFAULT, gio::Cancellable::NONE, |_| {});
        }
    }
}
