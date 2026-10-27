//! md for Linux — the GTK 4 / libadwaita / WebKitGTK shell over md-core and md-logic.
//!
//! This is the W1 GUI spike: a minimal but real shell that proves the platform mechanics the
//! full app relies on (docs/linux-facts.md records what each part showed). `md --spike <dir>`
//! drives the running app through every check under Xvfb and writes screenshots, PDFs and
//! `spike-results.json` into `<dir>`; `md --probe <variant> <dir>` loads the golden pages with one
//! scheme registration in a fresh process.

mod assets;
mod export;
mod menu;
mod preview;
mod spike;
mod standin;
mod util;
mod window;

use adw::prelude::*;
use std::path::PathBuf;

/// The reverse-DNS id the whole family shares (`me.nettrash.md` on the Mac). GApplication uses it
/// as the D-Bus name that makes the app single-instance.
pub const APP_ID: &str = "me.nettrash.md";

fn main() -> glib::ExitCode {
    let mut args: Vec<String> = std::env::args().collect();
    let mut spike_out: Option<PathBuf> = None;
    let mut probe: Option<(String, PathBuf)> = None;
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        let (Some(variant), Some(dir)) = (args.get(i + 1).cloned(), args.get(i + 2).cloned()) else {
            eprintln!("usage: md --probe <plain|secure|cors|secure_cors|local|secure_cors_csp> <output directory>");
            return glib::ExitCode::FAILURE;
        };
        probe = Some((variant, PathBuf::from(dir)));
        args.drain(i..i + 3);
    }
    if let Some(i) = args.iter().position(|a| a == "--spike") {
        let Some(dir) = args.get(i + 1).cloned() else {
            eprintln!("usage: md --spike <output directory>");
            return glib::ExitCode::FAILURE;
        };
        spike_out = Some(PathBuf::from(dir));
        args.drain(i..i + 2);
    }

    if let Some((variant, dir)) = probe {
        // A probe is a throwaway process: never the primary instance, never handed files.
        let app = adw::Application::builder().application_id(APP_ID).flags(gio::ApplicationFlags::NON_UNIQUE).build();
        app.connect_activate(move |app| spike::probe(app, &variant, dir.clone()));
        return app.run_with_args(&args);
    }

    let app = adw::Application::builder()
        .application_id(APP_ID)
        // Files on the command line arrive in `open`; a second launch forwards them to the
        // primary instance over D-Bus and exits (the proof is in spike.rs, step "single instance").
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();

    app.connect_startup(|app| {
        menu::install(app);
    });

    let spike_dir = spike_out.clone();
    app.connect_activate(move |app| {
        let win = window::DocWindow::open(app);
        if let Some(dir) = &spike_dir {
            spike::start(app, &win, dir.clone());
        }
    });

    app.connect_open(|app, files, _hint| {
        for file in files {
            window::open_file(app, file);
        }
    });

    // GApplication parses argv itself: everything left after our own flags is a file to open.
    app.run_with_args(&args)
}
