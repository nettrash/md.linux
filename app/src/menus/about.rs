//! Help ▸ About md — an `adw::AboutDialog` (shell-design §2.8): name, icon `me.nettrash.md`, the
//! crate version, developer, `strings::help` copyright, MIT, the support site, and the engines.
//! Never "no third-party dependencies", and no legal sections for the engines until nettrash
//! decides the notices question (docs/packaging.md §6).

use adw::prelude::*;
use md_logic::strings::{self, help};

use crate::app::APP_ID;

/// md-logic gap: the developer's name has no `strings::help` constant yet (the copyright line
/// carries it); the About dialog's developer field until one lands.
const DEVELOPER: &str = "nettrash";

/// The comments field: the engines heading, then one engine per line.
pub fn comments() -> String {
    let mut text = String::from(help::ENGINES_HEADING);
    for engine in help::ENGINES {
        text.push('\n');
        text.push_str(engine);
    }
    text
}

/// Presents About over `parent`.
pub fn show_about(parent: &gtk::Window) {
    let about = adw::AboutDialog::builder()
        .application_name(strings::APP_NAME)
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .developer_name(DEVELOPER)
        .copyright(help::COPYRIGHT)
        .license_type(gtk::License::MitX11)
        .website(help::SUPPORT_URL)
        .comments(comments())
        .build();
    about.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_comments_list_the_five_engines_under_their_heading() {
        let comments = comments();
        let lines: Vec<&str> = comments.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], help::ENGINES_HEADING);
        assert_eq!(&lines[1..], help::ENGINES);
        assert!(!comments.to_lowercase().contains("no third-party"));
    }
}
