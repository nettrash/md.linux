//! md for Linux — the GTK 4 / libadwaita / WebKitGTK 6.0 shell over md-core and md-logic
//! (docs/shell-design.md). Each module is owned by one work package (docs/shell-work-packages.md
//! §2); they meet only at the interfaces the skeleton froze (§3.2).
//!
//! | Module | What | Owner |
//! | --- | --- | --- |
//! | [`app`] | `Shell`, the `adw::Application` and its signals | S0 → P1 |
//! | [`services`] | GLib seams, settings, paths (S0); files, pickers, alerts, recents, examples (P2) | S0, P2 |
//! | [`theme`] | the paper palette as CSS, dark fan-out | S0 |
//! | [`menus`] | menu bar, actions, accelerators, About | P1 |
//! | [`windows`] | `WindowHost` (frozen), window manager, document window, session, drafts | S0, P1 |
//! | [`editor`] | editor pane, typing, find bar, panes, Zen, footer, bars | P3 |
//! | [`web`] | `WebServices`, the `md-asset:` origin, the preview host | P4 |
//! | [`export`] | the overlay renderer, PDF, snapshots, cover fetch, per-window exports | P4 |
//! | [`book`] | the book library host, the Book window, its sidebar | P5 |
//! | `selftest` | `md --selftest OUT` (cargo feature `selftest`) | S0 runner, groups per package |

mod app;
mod book;
mod editor;
mod export;
mod menus;
#[cfg(feature = "selftest")]
mod selftest;
mod services;
mod theme;
mod web;
mod windows;

fn main() -> glib::ExitCode {
    // OsString, not String: a file name on the command line need not be UTF-8, and GApplication
    // hands the rest of the arguments to `open` as `GFile`s whatever their bytes are.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    #[cfg(feature = "selftest")]
    let mut args = args;

    #[cfg(feature = "selftest")]
    match selftest::Options::take(&mut args) {
        Err(message) => {
            eprintln!("md: {message}");
            return glib::ExitCode::FAILURE;
        }
        Ok(Some(options)) => {
            if let Err(message) = selftest_home::check(&options.out) {
                eprintln!("md: --selftest refuses to start: {message}");
                return glib::ExitCode::FAILURE;
            }
            return app::run(&args, app::Launch::SelfTest(options));
        }
        Ok(None) => {}
    }

    app::run(&args, app::Launch::Normal)
}

/// The self-test's profile redirection (shell-design §13.2). A self-test writes settings, a
/// session, drafts and the recent list, and must never write them into a real profile. GLib reads
/// the XDG variables once, and `std::env::set_var` is `unsafe` in edition 2024 (forbidden in this
/// workspace), so the process cannot redirect itself: `scripts/dev.sh selftest` exports them under
/// `OUT/home/`, and a run whose variables point anywhere else refuses to start. A relaunch check
/// passes them on to the second `md` with `std::process::Command::envs`
/// (`selftest::Harness::md_command`).
#[cfg(feature = "selftest")]
mod selftest_home {
    use std::path::{Component, Path};

    pub fn check(out: &Path) -> Result<(), String> {
        std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        let given = std::path::absolute(out).map_err(|e| format!("{}: {e}", out.display()))?;
        let real = std::fs::canonicalize(out).map_err(|e| format!("{}: {e}", out.display()))?;
        for variable in crate::selftest::XDG_VARIABLES {
            let Some(value) = std::env::var_os(variable).filter(|v| !v.is_empty()) else {
                return Err(format!("{variable} is not set; it must point into {}", given.display()));
            };
            let path = Path::new(&value);
            if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
                return Err(format!("{variable}={} must be an absolute path without `..`", path.display()));
            }
            if !path.starts_with(&given) && !path.starts_with(&real) {
                return Err(format!("{variable}={} is not inside {}", path.display(), given.display()));
            }
            // Lexically inside; a symlink on the way must not lead out again.
            std::fs::create_dir_all(path).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
            let resolved = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
            if !resolved.starts_with(&real) {
                return Err(format!("{variable}={} resolves to {} outside it", path.display(), resolved.display()));
            }
        }
        Ok(())
    }
}
