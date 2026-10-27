//! Port of md.win `tests/Md.App.Logic.Tests/CommandsAboutTests.cs`: what Help ▸ About md and
//! Help ▸ md Help / Privacy Policy put in front of a user (§2.8). The dialog itself is the GTK
//! app's (`adw::AboutDialog`); everything it says comes from `strings`, so the wording is pinned
//! where it can be tested.

use md_logic::commands::{CommandId, command_table};
use md_logic::strings::{self, help};

#[test]
fn about_lists_the_five_bundled_engines_with_their_versions() {
    assert_eq!(
        help::ENGINES,
        [
            "KaTeX 0.17.0 + mhchem",
            "Mermaid 11.16.0",
            "Graphviz 14.1.1 via Viz.js 3.24.0",
            "PlantUML 1.2026.4beta4",
            "highlight.js 11.11.1",
        ]
    );
}

#[test]
fn about_never_claims_there_are_no_third_party_dependencies() {
    // The family's standing rule: md bundles five open source engines and says so; claiming
    // otherwise in a store-facing surface is a factual error. (ASCII lower-casing is enough: every
    // string here is ASCII but the copyright sign.)
    let mut lines = vec![strings::APP_NAME, help::ENGINES_HEADING, help::COPYRIGHT];
    lines.extend(help::ENGINES);
    let about = lines.join("\n").to_ascii_lowercase();
    assert!(!about.contains("third-party"));
    assert!(!about.contains("no dependencies"));
    assert!(help::ENGINES_HEADING.to_ascii_lowercase().contains("engines"));
}

#[test]
fn help_points_at_the_two_store_linked_pages() {
    // LINUX: the Linux edition's pages beside the other channels' (md.win: msstore/), and the two
    // rows that open them.
    assert_eq!(help::SUPPORT_URL, "https://nettrash.me/linux/md/support.html");
    assert_eq!(help::PRIVACY_URL, "https://nettrash.me/linux/md/privacy.html");
    assert_eq!(command_table::spec(CommandId::Help).title, "md Help");
    assert_eq!(command_table::spec(CommandId::PrivacyPolicy).title, "Privacy Policy");
}

#[test]
fn the_copyright_is_the_familys_line() {
    assert_eq!(help::COPYRIGHT, "\u{00A9} 2026 nettrash. MIT licensed.");
}
