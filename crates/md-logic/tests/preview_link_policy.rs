//! Port of md.win `tests/Md.App.Logic.Tests/Preview/LinkPolicyTests.cs`: the §4.7 matrix, row by
//! row. The row that matters most is the second: two of the three candidate designs tested http(s)
//! before the host and so opened the user's browser at an address made up of our own origin for an
//! ordinary relative link.
//!
//! LINUX: md.win's `https://md.assets/…` is the preview surface's `md-asset://preview-1/…`.

use md_logic::preview::asset_origin::{self, index_url, preview_host};
use md_logic::preview::link_policy::{LinkDecision, decide};

fn index() -> String {
    index_url(&preview_host(1))
}

fn decide_user(uri: &str) -> LinkDecision {
    decide(uri, true, &index())
}

#[test]
fn our_own_document_is_allowed() {
    for uri in [
        "md-asset://preview-1/index.html",           // our own load / reload
        "md-asset://preview-1/index.html#a-heading", // [x](#slug) — WebKitGTK does raise decide-policy for it
        "md-asset://preview-1/index.html#",
    ] {
        assert_eq!(decide_user(uri), LinkDecision::Allow, "{uri}");
    }
}

#[test]
fn every_other_url_on_our_host_is_cancelled() {
    for uri in [
        "md-asset://preview-1/other.md", // [x](other.md) resolved against the origin
        "md-asset://preview-1/",
        "md-asset://preview-1/rich/md-init.js",
        "md-asset://preview-1/index.html?v=2", // same path, different resource
        "https://preview-1/index.html",        // same host, wrong scheme
    ] {
        assert_eq!(decide_user(uri), LinkDecision::Cancel, "{uri}");
    }
}

#[test]
fn http_on_another_host_goes_to_the_browser() {
    for uri in ["https://nettrash.me/", "http://example.com/a?b=c#d"] {
        assert_eq!(decide_user(uri), LinkDecision::OpenExternally, "{uri}");
    }
}

#[test]
fn every_other_scheme_dies() {
    for uri in [
        "javascript:alert(1)",
        "data:text/html,<b>x</b>",
        "file:///C:/Windows/System32/calc.exe",
        "file:///etc/passwd", // LINUX: the same row with a Linux path
        "mailto:nettrash@nettrash.me",
        "ftp://example.com/x",
        "about:blank",
    ] {
        assert_eq!(decide_user(uri), LinkDecision::Cancel, "{uri}");
    }
}

#[test]
fn a_relative_uri_is_cancelled() {
    // Nothing produces one (WebKit hands over absolute URIs), and nothing relative can be ours.
    assert_eq!(decide_user("other.md"), LinkDecision::Cancel);
}

#[test]
fn the_host_is_matched_case_insensitively_like_dns() {
    assert_eq!(decide_user("md-asset://PREVIEW-1/other.md"), LinkDecision::Cancel);
}

#[test]
fn a_navigation_the_user_did_not_start_never_opens_the_browser() {
    // A script assigning location, or a meta refresh, inside the rendered document. The Mac lets
    // those navigate the preview away; here they are dropped (§4.7, the recorded divergence).
    assert_eq!(decide("https://nettrash.me/", false, &index()), LinkDecision::Cancel);
}

#[test]
fn our_own_loads_are_not_user_initiated_and_still_pass() {
    assert_eq!(decide(&index(), false, &index()), LinkDecision::Allow);
}

#[test]
fn the_index_url_is_the_one_the_host_serves() {
    // LINUX: the surface's own URL, on the md-asset scheme.
    assert_eq!(index(), "md-asset://preview-1/index.html");
    assert_eq!(asset_origin::split_uri(&index()), Some(("preview-1", "index.html")));
}

#[test]
fn another_surface_on_the_same_scheme_is_not_ours() {
    // LINUX: one host per surface, so another surface's page is another origin — cancelled, never
    // loaded into this pane.
    assert_eq!(decide_user("md-asset://export-2/index.html"), LinkDecision::Cancel);
    assert_eq!(decide_user("md-asset://preview-2/index.html"), LinkDecision::Cancel);
}

#[test]
fn the_scheme_and_host_of_our_own_document_are_compared_without_case() {
    // LINUX: System.Uri canonicalises both; the hand-written split does the same.
    assert_eq!(decide_user("MD-ASSET://Preview-1/index.html#x"), LinkDecision::Allow);
    assert_eq!(decide_user("HTTP://example.com/"), LinkDecision::OpenExternally);
}
