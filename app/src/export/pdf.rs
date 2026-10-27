//! PDF and print (shell-design §7.2, §7.3): `webkit::PrintOperation` with `gtk::PrintSettings` /
//! `gtk::PageSetup` aimed at GTK's built-in "Print to File" printer (no dialog), and
//! `run_dialog` for Print….
//!
//! S0 moved the spike's PDF code (`export.rs`, facts §5) here as the starting point.
#![allow(dead_code, unused_variables)] // S0 stub: removed by P4

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use md_logic::exec::Completion;
use webkit::prelude::*;

#[derive(Clone, Copy, Debug)]
pub enum Paper {
    A4,
    /// Width × height in inches, e.g. the 6 × 9 in trade paperback.
    Inches(f64, f64),
}

/// The name of GTK's file printer in the current locale. GTK names it `_("Print to File")` in its
/// `gtk40` gettext domain and WebKit finds the printer by that name, so a hard-coded English
/// string misses it under a translated desktop (facts §5).
pub fn file_printer_name() -> String {
    glib::dgettext(Some("gtk40"), "Print to File").to_string()
}

/// Prints the page in `view` to `path` as PDF — no dialog — with `margin_in` on all four sides.
pub async fn print_pdf(view: &webkit::WebView, path: &Path, paper: Paper, margin_in: f64) -> Result<Duration, String> {
    let start = Instant::now();
    let uri = gio::File::for_path(path).uri();
    let paper_size = match paper {
        Paper::A4 => gtk::PaperSize::new(Some(gtk::PAPER_NAME_A4)),
        Paper::Inches(w, h) => {
            let name = format!("custom_{w}x{h}in");
            gtk::PaperSize::new_custom(&name, &format!("{w} × {h} in"), w, h, gtk::Unit::Inch)
        }
    };

    let settings = gtk::PrintSettings::new();
    settings.set_printer(&file_printer_name());
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(uri.as_str()));
    settings.set_paper_size(&paper_size);
    settings.set_orientation(gtk::PageOrientation::Portrait);

    let setup = gtk::PageSetup::new();
    setup.set_paper_size(&paper_size);
    setup.set_orientation(gtk::PageOrientation::Portrait);
    setup.set_top_margin(margin_in, gtk::Unit::Inch);
    setup.set_bottom_margin(margin_in, gtk::Unit::Inch);
    setup.set_left_margin(margin_in, gtk::Unit::Inch);
    setup.set_right_margin(margin_in, gtk::Unit::Inch);

    let op = webkit::PrintOperation::new(view);
    op.set_print_settings(&settings);
    op.set_page_setup(&setup);
    // `finished` follows `failed`: only the first answer counts.
    let done: Completion<Result<(), String>> = Completion::new();
    let answered = Rc::new(Cell::new(false));
    {
        let (done, answered) = (done.clone(), Rc::clone(&answered));
        op.connect_finished(move |_| {
            if !answered.replace(true) {
                done.complete(Ok(()));
            }
        });
    }
    {
        let (done, answered) = (done.clone(), Rc::clone(&answered));
        op.connect_failed(move |_, err| {
            if !answered.replace(true) {
                done.complete(Err(err.message().to_owned()));
            }
        });
    }
    op.print();
    match glib::future_with_timeout(Duration::from_secs(90), done.wait()).await {
        Ok(Ok(())) => Ok(start.elapsed()),
        Ok(Err(message)) => Err(message),
        Err(_) => Err("timed out after 90 s".to_owned()),
    }
}
