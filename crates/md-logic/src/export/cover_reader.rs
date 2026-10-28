//! Port of md.win `Export/CoverReader.cs`: reads the image an EPUB's `cover:` names (1.6). md-core
//! classifies the reference and the bytes ([`epub_metadata`]); this is the I/O: a `data:` image is
//! already decoded, a relative path is read beside the document with the TextBundle export's
//! containment ([`asset_reader::beside_capped`](crate::documents::asset_reader::beside_capped)),
//! and an `https:` image is fetched. Anything unreadable — missing, outside the folder, a failed or
//! slow fetch, bytes that are not a PNG, JPEG, GIF or WebP — is `None`, and the book is written
//! without a cover. Nothing fails but the export's own cancellation.
//!
//! LINUX DECISION — the fetch seam. md.win fetches with `HttpClient` (no automatic redirects,
//! 15 s in all, ≤ 5 redirects each checked https *before* it is requested, ≤ 20 MiB). md-logic has
//! no HTTP stack (std only), so the network is one small seam, [`CoverFetcher`]: **one GET of one
//! URL, never following a redirect**, answering the status, the `Location` header, the declared
//! length and a body read chunk by chunk. Everything that is policy stays here and is tested with a
//! fake: the https check of the first URL and of every redirect target (resolved against the
//! current URL, RFC 3986), the redirect count, the success range, the 20 MiB cap (a declared length
//! past it is refused before the body is read; a body is read only until it passes it), and the
//! 15 s overall deadline on the [`Scheduler`] across every hop and every chunk. The GTK app
//! implements the single request with libsoup 3 (WebKitGTK's own dependency): a `soup::Session`
//! with `max-redirects` 0 (or a `soup::Message` flagged `NO_REDIRECT`), `send_future` for the
//! headers, `read_bytes_future` for each chunk — and a `gio::Cancellable` cancelled on drop, because
//! md-logic abandons a request (a redirect, a refused length, the cap, the deadline) by dropping
//! its future or its body.

use std::fmt;
use std::future::Future;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use md_core::export::epub_metadata::{self, MAX_COVER_BYTES};
use md_core::export::{CoverReference, EpubCover};

use super::export_delay;
use crate::exec::{CancelToken, Cancelled, LocalBoxFuture};
use crate::seams::Scheduler;

/// How long a cover fetch may take, every redirect and the whole body included, before the book
/// goes without one.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(15);

/// How many redirects a cover fetch follows before it gives up.
pub const MAX_REDIRECTS: usize = 5;

/// One HTTP GET without redirect following — the only part of the cover fetch the app implements.
pub trait CoverFetcher {
    /// Send one GET for `url` and resolve when the response headers have arrived. A redirect is
    /// answered as itself (its 3xx status and `Location`), never followed. `Err` (any text, for the
    /// log) for a failure of the request itself: DNS, TLS, a refused connection. Dropping the
    /// future must abort the request.
    fn get(&self, url: String) -> LocalBoxFuture<'static, Result<CoverResponse, String>>;
}

/// One response's headers and its body.
pub struct CoverResponse {
    /// The HTTP status code.
    pub status: u16,
    /// The `Location` header as sent (it may be relative), when there is one.
    pub location: Option<String>,
    /// The declared `Content-Length`, when there is one.
    pub content_length: Option<u64>,
    /// The body, read on demand. Dropping it must abort the transfer.
    pub body: Box<dyn CoverBody>,
}

impl fmt::Debug for CoverResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CoverResponse")
            .field("status", &self.status)
            .field("location", &self.location)
            .field("content_length", &self.content_length)
            .finish_non_exhaustive()
    }
}

/// A response body as a stream of chunks.
pub trait CoverBody {
    /// The next chunk; `Ok(None)` at the end of the body; `Err` when the transfer failed.
    fn next_chunk(&self) -> LocalBoxFuture<'static, Result<Option<Vec<u8>>, String>>;
}

/// The cover, or `None`. `reference` is the plan's [`EpubPlan::cover`](md_core::export::EpubPlan);
/// `beside` reads a path relative to the document, contained (`None` when not found);
/// `fetcher` and `scheduler` serve [`fetch`]. Only the export's own cancellation is an error.
pub async fn read(
    reference: Option<&CoverReference>,
    beside: &dyn Fn(&str) -> Option<Vec<u8>>,
    fetcher: &dyn CoverFetcher,
    scheduler: &dyn Scheduler,
    cancel: &CancelToken,
) -> Result<Option<EpubCover>, Cancelled> {
    match reference {
        Some(CoverReference::Inline(data)) => Ok(epub_metadata::recognise(Some(data))),
        Some(CoverReference::Local(path)) => Ok(epub_metadata::recognise(beside(path).as_deref())),
        Some(CoverReference::Https(url)) => {
            let data = fetch(fetcher, scheduler, url, cancel).await?;
            Ok(epub_metadata::recognise(data.as_deref()))
        }
        None => Ok(None),
    }
}

/// GET an https image: [`FETCH_TIMEOUT`] in all, at most [`MAX_COVER_BYTES`], and at most
/// [`MAX_REDIRECTS`] redirects, each of which must stay https — checked before it is requested.
/// `Ok(None)` on any failure; `Err(Cancelled)` only when `cancel` is (seen whenever the fetch is
/// polled — after each answer, chunk or the deadline; see [`export_delay`]).
pub async fn fetch(
    fetcher: &dyn CoverFetcher,
    scheduler: &dyn Scheduler,
    url: &str,
    cancel: &CancelToken,
) -> Result<Option<Vec<u8>>, Cancelled> {
    if cancel.is_cancelled() {
        return Err(Cancelled);
    }
    let Some(first) = parse_https(url) else { return Ok(None) };

    let transfer = async move {
        let mut current = first;
        let mut hop = 0;
        loop {
            let response = fetcher.get(current.clone()).await.ok()?;
            if (300..400).contains(&response.status) {
                if hop >= MAX_REDIRECTS {
                    return None;
                }
                current = followed_redirect(&current, response.location.as_deref())?;
                hop += 1;
                continue;
            }
            if !(200..300).contains(&response.status) {
                return None;
            }
            if response.content_length.is_some_and(|length| length > MAX_COVER_BYTES as u64) {
                return None;
            }
            return read_bounded(&*response.body, MAX_COVER_BYTES).await.ok().flatten();
        }
    };
    // The deadline is the fetch's own: only dropping it (when the transfer wins) stops it.
    let deadline = export_delay::delay(scheduler, FETCH_TIMEOUT, &CancelToken::new());
    race(transfer, deadline, cancel).await
}

/// The transfer, unless the export is cancelled first (an error) or the deadline passes first
/// (`None`). Cancellation wins a tie, as C#'s exception filter lets it.
async fn race(
    transfer: impl Future<Output = Option<Vec<u8>>>,
    deadline: export_delay::Delay,
    cancel: &CancelToken,
) -> Result<Option<Vec<u8>>, Cancelled> {
    let mut transfer = pin!(transfer);
    let mut deadline = pin!(deadline);
    std::future::poll_fn(move |cx| {
        if cancel.is_cancelled() {
            return Poll::Ready(Err(Cancelled));
        }
        if let Poll::Ready(data) = transfer.as_mut().poll(cx) {
            return Poll::Ready(Ok(data));
        }
        if deadline.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Ok(None));
        }
        Poll::Pending
    })
    .await
}

/// The whole body, or `None` once it passes `limit` bytes — the read stops there and the rest is
/// never asked for. `Err` when the transfer failed.
pub async fn read_bounded(body: &dyn CoverBody, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let mut buffer = Vec::new();
    while let Some(chunk) = body.next_chunk().await? {
        if buffer.len() + chunk.len() > limit {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(Some(buffer))
}

/// Where a redirect from `current` goes: `location` resolved against it when relative (RFC 3986
/// §5.2), or `None` when there is none or it is not https — never requested.
pub fn followed_redirect(current: &str, location: Option<&str>) -> Option<String> {
    let location = location?;
    let base = UriParts::split(current);
    base.scheme.as_ref()?;
    let next = base.resolve(&UriParts::split(location)).recompose();
    parse_https(&next)
}

/// `url` normalised (scheme and host in lower case, an empty path as `/`) when it is an absolute
/// https URL with a host — .NET's `Uri.TryCreate(url, Absolute)` + `Scheme == https`; else `None`.
fn parse_https(url: &str) -> Option<String> {
    let parts = UriParts::split(url);
    if !parts.scheme.as_deref()?.eq_ignore_ascii_case("https") {
        return None;
    }
    let authority = parts.authority.as_deref()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    if host.is_empty() || host.starts_with(':') {
        return None;
    }
    Some(parts.recompose())
}

/// The five components of RFC 3986 Appendix B's regular expression,
/// `^(([^:/?#]+):)?(//([^/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?`, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
struct UriParts {
    scheme: Option<String>,
    authority: Option<String>,
    path: String,
    query: Option<String>,
    fragment: Option<String>,
}

impl UriParts {
    fn split(uri: &str) -> UriParts {
        // Every delimiter is ASCII, so every slice below is on a char boundary.
        let (rest, fragment) = match uri.find('#') {
            Some(at) => (&uri[..at], Some(uri[at + 1..].to_string())),
            None => (uri, None),
        };
        let (rest, query) = match rest.find('?') {
            Some(at) => (&rest[..at], Some(rest[at + 1..].to_string())),
            None => (rest, None),
        };
        let (scheme, rest) = match rest.find([':', '/']) {
            Some(at) if at > 0 && rest.as_bytes()[at] == b':' && is_scheme(&rest[..at]) => {
                (Some(rest[..at].to_ascii_lowercase()), &rest[at + 1..])
            }
            _ => (None, rest),
        };
        let (authority, path) = match rest.strip_prefix("//") {
            Some(after) => {
                let end = after.find('/').unwrap_or(after.len());
                (Some(lower_host(&after[..end])), &after[end..])
            }
            None => (None, rest),
        };
        UriParts { scheme, authority, path: path.to_string(), query, fragment }
    }

    /// RFC 3986 §5.2.2, strict (a reference's own scheme always wins).
    fn resolve(&self, reference: &UriParts) -> UriParts {
        let fragment = reference.fragment.clone();
        if reference.scheme.is_some() {
            return UriParts { path: remove_dot_segments(&reference.path), fragment, ..reference.clone() };
        }
        if reference.authority.is_some() {
            return UriParts {
                scheme: self.scheme.clone(),
                authority: reference.authority.clone(),
                path: remove_dot_segments(&reference.path),
                query: reference.query.clone(),
                fragment,
            };
        }
        let (path, query) = if reference.path.is_empty() {
            (self.path.clone(), reference.query.clone().or_else(|| self.query.clone()))
        } else if reference.path.starts_with('/') {
            (remove_dot_segments(&reference.path), reference.query.clone())
        } else {
            (remove_dot_segments(&self.merge(&reference.path)), reference.query.clone())
        };
        UriParts { scheme: self.scheme.clone(), authority: self.authority.clone(), path, query, fragment }
    }

    /// §5.2.3.
    fn merge(&self, reference_path: &str) -> String {
        if self.authority.is_some() && self.path.is_empty() {
            return format!("/{reference_path}");
        }
        match self.path.rfind('/') {
            Some(at) => format!("{}{reference_path}", &self.path[..=at]),
            None => reference_path.to_string(),
        }
    }

    /// §5.3, with .NET's one normalisation: an authority with an empty path gets `/`.
    fn recompose(&self) -> String {
        let mut out = String::new();
        if let Some(scheme) = &self.scheme {
            out.push_str(scheme);
            out.push(':');
        }
        if let Some(authority) = &self.authority {
            out.push_str("//");
            out.push_str(authority);
            if self.path.is_empty() {
                out.push('/');
            }
        }
        out.push_str(&self.path);
        if let Some(query) = &self.query {
            out.push('?');
            out.push_str(query);
        }
        if let Some(fragment) = &self.fragment {
            out.push('#');
            out.push_str(fragment);
        }
        out
    }
}

/// `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`.
fn is_scheme(candidate: &str) -> bool {
    let mut bytes = candidate.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// The host (and port) in lower case; user information is left as written. ASCII only — a host
/// that is not ASCII is the request's problem, not a redirect's.
fn lower_host(authority: &str) -> String {
    match authority.rsplit_once('@') {
        Some((user, host)) => format!("{user}@{}", host.to_ascii_lowercase()),
        None => authority.to_ascii_lowercase(),
    }
}

/// §5.2.4.
fn remove_dot_segments(path: &str) -> String {
    let mut input = path;
    let mut output = String::with_capacity(path.len());
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../").or_else(|| input.strip_prefix("./")) {
            input = rest;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") || input == "/.." {
            input = if input == "/.." { "/" } else { &input[3..] };
            match output.rfind('/') {
                Some(at) => output.truncate(at),
                None => output.clear(),
            }
        } else if input == "." || input == ".." {
            input = "";
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..].find('/').map_or(input.len(), |at| at + start);
            output.push_str(&input[..end]);
            input = &input[end..];
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments_are_removed_as_rfc_3986_section_5_2_4_spells_them() {
        assert_eq!(remove_dot_segments("/a/b/c/./../../g"), "/a/g");
        assert_eq!(remove_dot_segments("mid/content=5/../6"), "mid/6");
        assert_eq!(remove_dot_segments("/../a"), "/a");
        assert_eq!(remove_dot_segments("/a/.."), "/");
    }

    #[test]
    fn the_rfc_3986_section_5_4_examples_resolve() {
        let base = UriParts::split("http://a/b/c/d;p?q");
        let resolved = |r: &str| base.resolve(&UriParts::split(r)).recompose();
        assert_eq!(resolved("g:h"), "g:h");
        assert_eq!(resolved("g"), "http://a/b/c/g");
        assert_eq!(resolved("./g"), "http://a/b/c/g");
        assert_eq!(resolved("g/"), "http://a/b/c/g/");
        assert_eq!(resolved("/g"), "http://a/g");
        assert_eq!(resolved("//g"), "http://g/");
        assert_eq!(resolved("?y"), "http://a/b/c/d;p?y");
        assert_eq!(resolved("g?y"), "http://a/b/c/g?y");
        assert_eq!(resolved("#s"), "http://a/b/c/d;p?q#s");
        assert_eq!(resolved(""), "http://a/b/c/d;p?q");
        assert_eq!(resolved("."), "http://a/b/c/");
        assert_eq!(resolved(".."), "http://a/b/");
        assert_eq!(resolved("../g"), "http://a/b/g");
        assert_eq!(resolved("../../../g"), "http://a/g");
        assert_eq!(resolved("g;x=1/../y"), "http://a/b/c/y");
    }
}
