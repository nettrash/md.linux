//! Port of md.win `Preview/LinkPolicy.cs`: the whole navigation policy, pure (§4.7).

/// What the host does with a navigation the page asks for (§4.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkDecision {
    /// Let it happen: our own load, reload or a fragment hop inside the document
    /// (`PolicyDecision::use_`).
    Allow,
    /// `PolicyDecision::ignore` and nothing else — the Mac's silent drop.
    Cancel,
    /// `PolicyDecision::ignore`, then hand the URI to the desktop (`gtk::UriLauncher`).
    OpenExternally,
}

/// The decision for a navigation to `uri` (the `decide-policy` request URI, for a
/// `NavigationAction` and a `NewWindowAction` alike), started by the user or not
/// (`NavigationAction::is_user_gesture`), from the surface whose document is `index_url`.
///
/// The order of the tests is the point: the *host* is checked before the http(s) branch, so a
/// relative link the page resolved against our own origin (`[x](other.md)` →
/// `md-asset://preview-1/other.md`) is cancelled like the Mac cancels a non-fragment `mdassets` URL,
/// instead of launching the browser at a made-up address.
///
/// `is_user_initiated` only ever narrows the answer: a cross-host http(s) navigation the user did
/// not ask for (a script assigning `location`, a `meta refresh`) is cancelled rather than handed to
/// the browser. The Mac allows those to navigate the preview away, which is worse; this is the one
/// recorded divergence in this table.
///
/// LINUX DECISION: md.win reads `System.Uri`; here the URI is split by hand (`ParsedUri`) the way
/// that class canonicalises the parts this policy reads — scheme and host compared without case,
/// userinfo never part of the host, http's and https's default ports dropped, everything but the
/// fragment compared for the Allow rule. WebKitGTK raises `decide-policy` for same-document fragment
/// hops too (docs/linux-facts.md §3, where WebView2 does not), so `index.html#slug` must be — and
/// is — allowed by the without-fragment rule. A `javascript:` href never reaches here on WebKitGTK
/// either (measured), which is why [`LINK_GUARD`](super::scripts::LINK_GUARD) exists as well.
pub fn decide(uri: &str, is_user_initiated: bool, index_url: &str) -> LinkDecision {
    let target = ParsedUri::parse(uri);
    let index = ParsedUri::parse(index_url);

    // Our own load / reload, and a fragment hop that surfaces as a navigation.
    if without_fragment(uri, target.as_ref()) == without_fragment(index_url, index.as_ref()) {
        return LinkDecision::Allow;
    }

    // Nothing relative can be one of ours.
    let Some(target) = target else { return LinkDecision::Cancel };

    // Anything else on our own host: a relative link that resolved against the origin. Nothing but
    // index.html is a document here, so it can only be a 404 or an asset — never a page.
    if let Some(index) = index
        && target.host == index.host
    {
        return LinkDecision::Cancel;
    }

    let web = target.scheme == "http" || target.scheme == "https";
    if web && is_user_initiated {
        return LinkDecision::OpenExternally;
    }

    // file:, data:, mailto:, dropped files — and a cross-host page the user never clicked.
    LinkDecision::Cancel
}

/// Scheme, host, port, path and query — everything but the fragment, which never changes the
/// decision; a relative URI as given (`Uri.OriginalString`).
fn without_fragment(original: &str, parsed: Option<&ParsedUri>) -> String {
    match parsed {
        Some(uri) => uri.left_part.clone(),
        None => original.to_string(),
    }
}

/// The parts of an absolute URI the policy reads.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ParsedUri {
    /// ASCII lower case.
    scheme: String,
    /// ASCII lower case, userinfo and port removed; empty for a URI with no authority (`mailto:`,
    /// `javascript:`, `file:///…`).
    host: String,
    /// `Uri.GetLeftPart(UriPartial.Query)`: the URI up to its fragment, scheme and host lower-cased,
    /// http/https default port dropped, an empty hierarchical path written as `/`.
    left_part: String,
}

impl ParsedUri {
    /// `None` for a relative reference — no `scheme:` prefix (RFC 3986: a letter, then letters,
    /// digits, `+`, `-`, `.`).
    fn parse(uri: &str) -> Option<ParsedUri> {
        let colon = uri.find(':')?;
        let scheme = &uri[..colon];
        let mut chars = scheme.chars();
        let first = chars.next()?;
        if !first.is_ascii_alphabetic() || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
            return None;
        }
        let scheme = scheme.to_ascii_lowercase();
        let rest = &uri[colon + 1..];
        let rest = match rest.find('#') {
            Some(hash) => &rest[..hash],
            None => rest,
        };

        let Some(hierarchical) = rest.strip_prefix("//") else {
            // An opaque URI (mailto:, javascript:, data:, about:): no host.
            return Some(ParsedUri { left_part: format!("{scheme}:{rest}"), scheme, host: String::new() });
        };

        let authority_end = hierarchical.find(['/', '?']).unwrap_or(hierarchical.len());
        let authority = &hierarchical[..authority_end];
        let mut path_and_query = hierarchical[authority_end..].to_string();
        if !path_and_query.starts_with('/') {
            path_and_query.insert(0, '/');
        }

        // Userinfo is everything up to the LAST '@': `md-asset://preview-1@evil.com/` is evil.com.
        let host_and_port = match authority.rfind('@') {
            Some(at) => &authority[at + 1..],
            None => authority,
        };
        let (host, port) = split_port(host_and_port);
        let host = host.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" => Some("80"),
            "https" => Some("443"),
            _ => None,
        };
        let port = match port {
            Some(port) if Some(port) != default_port => format!(":{port}"),
            _ => String::new(),
        };
        let left_part = format!("{scheme}://{host}{port}{path_and_query}");
        Some(ParsedUri { scheme, host, left_part })
    }
}

/// `host:port` → (`host`, `Some(port)`); a bracketed IPv6 literal keeps its colons.
fn split_port(host_and_port: &str) -> (&str, Option<&str>) {
    let search_from = if host_and_port.starts_with('[') { host_and_port.find(']').unwrap_or(0) } else { 0 };
    match host_and_port[search_from..].rfind(':') {
        Some(colon) => {
            let colon = search_from + colon;
            (&host_and_port[..colon], Some(&host_and_port[colon + 1..]))
        }
        None => (host_and_port, None),
    }
}
