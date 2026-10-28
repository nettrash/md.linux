//! Port of md.win `tests/Md.App.Logic.Tests/Preview/AssetOriginTests.cs`: the origin constants
//! (§4.2) — on Linux the `md-asset:` scheme, one host per surface — plus the scheme handler's two
//! pure helpers, which the spike app carried untested-by-md-logic until now.

use md_logic::preview::asset_origin::{
    CONTENT_SECURITY_POLICY, INDEX_PATH, RICH_FOLDER_NAME, SCHEME, export_host, index_url, is_contained, preview_host,
    split_uri,
};

#[test]
fn the_constants_are_the_ones_the_design_fixes() {
    // LINUX: md.win pins "md.assets", "web", "rich" and https://md.assets/index.html.
    assert_eq!(SCHEME, "md-asset");
    assert_eq!(INDEX_PATH, "index.html");
    assert_eq!(RICH_FOLDER_NAME, "rich");
    assert_eq!(index_url("preview-1"), "md-asset://preview-1/index.html");
    assert_eq!(preview_host(1), "preview-1");
    assert_eq!(export_host(2), "export-2");
}

#[test]
fn the_index_url_is_built_on_the_host_and_is_https() {
    // LINUX: built on the surface's own host, on the md-asset scheme (a secure context because the
    // shell registers the scheme secure — docs/linux-facts.md §3 measured isSecureContext true even
    // unregistered).
    let host = preview_host(7);
    assert_eq!(index_url(&host), format!("{SCHEME}://{host}/{INDEX_PATH}"));
    assert_eq!(split_uri(&index_url(&host)), Some((host.as_str(), INDEX_PATH)));
    // Every surface is its own origin.
    assert_ne!(index_url(&preview_host(1)), index_url(&export_host(1)));
}

#[test]
#[ignore = "checks the WinUI app; the GTK app gets its own"]
fn the_csproj_packages_the_engines_where_the_virtual_host_looks_for_them() {
    // md.win: Md.App.csproj's <Content Include="rich\**\*" Link="web\rich\…"/> matches
    // AssetOrigin.WebFolderName / RichFolderName. Linux has no mapped folder; the .deb's install of
    // rich/ to /usr/share/md/rich is the packaging's own check.
}

#[test]
fn the_mapped_folder_is_not_the_install_root() {
    // LINUX: nothing maps a folder; the handler serves `rich/…` only, from a folder named by one
    // plain segment, and only paths that stay inside it.
    assert!(!RICH_FOLDER_NAME.is_empty());
    assert!(!RICH_FOLDER_NAME.contains('/'));
    assert!(!RICH_FOLDER_NAME.contains('\\'));
    assert!(!RICH_FOLDER_NAME.contains('.'));
    assert!(!is_contained("../Cargo.toml"));
}

#[test]
fn split_uri_strips_query_and_fragment() {
    assert_eq!(split_uri("md-asset://preview-1/index.html#intro"), Some(("preview-1", "index.html")));
    assert_eq!(split_uri("md-asset://p/rich/fonts/K.woff2?x=1"), Some(("p", "rich/fonts/K.woff2")));
    assert_eq!(split_uri("md-asset://p/"), Some(("p", "")));
    assert_eq!(split_uri("https://p/index.html"), None);
    assert_eq!(split_uri("md-asset://hostonly"), None);
    assert_eq!(split_uri("md-asset:index.html"), None);
}

#[test]
fn containment_rejects_escapes() {
    assert!(is_contained("katex.min.js"));
    assert!(is_contained("fonts/KaTeX_Main-Regular.woff2"));
    assert!(!is_contained("../Cargo.toml"));
    assert!(!is_contained("fonts/../../x"));
    assert!(!is_contained("fonts/./x"));
    assert!(!is_contained("fonts//x"));
    assert!(!is_contained("/etc/passwd"));
    assert!(!is_contained("%2e%2e/x"));
    assert!(!is_contained("fonts\\x"));
    assert!(!is_contained("a\0b"));
    assert!(!is_contained(""));
}

#[test]
fn the_content_security_policy_refuses_inline_script() {
    // docs/linux-facts.md §3: the header that refused `javascript:` URLs and inline handlers while
    // every engine rendered — no 'unsafe-inline' in script-src.
    let script_src = CONTENT_SECURITY_POLICY
        .split(';')
        .map(str::trim)
        .find(|d| d.starts_with("script-src"))
        .expect("a script-src directive");
    assert_eq!(script_src, "script-src 'self' 'unsafe-eval' 'wasm-unsafe-eval'");
    assert!(CONTENT_SECURITY_POLICY.contains("img-src 'self' data: blob: https: http:"));
    assert!(CONTENT_SECURITY_POLICY.contains("font-src 'self' data:"));
}
