//! PDF and print (shell-design §7.2, §7.3): `webkit::PrintOperation` with `gtk::PrintSettings` /
//! `gtk::PageSetup` aimed at GTK's built-in "Print to File" printer — no dialog — for Export ▸ PDF…,
//! and `run_dialog` — GTK's own print dialog — for Print….
//!
//! The spike's PDF code (docs/linux-facts.md §5), kept: GTK 4.22 has the file printer built in, and
//! its name is translated, so it is looked up in GTK's `gtk40` gettext domain; leaving the printer
//! unset would send the page to the default (CUPS) printer instead. A4 comes out 595 × 842 pt, the
//! body's 56 px padding plus the 0.5 in margins put the first glyph at 36 + 42 pt, backgrounds print.
// P4 → P1 / P5: until the windows construct exports, only the self-test reaches this; removed at the
// integration gate.
#![cfg_attr(not(feature = "selftest"), allow(dead_code))]

use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use md_logic::exec::Completion;
use md_logic::seams::PrintGeometry;
use webkit::prelude::*;

/// How long a print to file may take before it counts as failed (the golden 11-page document takes
/// ≈ 0.2 s; a book of diagrams a few seconds).
const PRINT_TIMEOUT: Duration = Duration::from_secs(120);

/// The name of GTK's file printer in the current locale: GTK names it `_("Print to File")` in its
/// `gtk40` domain and WebKit finds the printer by that name, so the English string misses it under
/// a translated desktop (facts §5: "Печать в файл" under `ru_RU`).
pub fn file_printer_name() -> String {
    glib::dgettext(Some("gtk40"), "Print to File").to_string()
}

/// The paper of `geometry`, portrait.
fn paper_size(geometry: &PrintGeometry) -> gtk::PaperSize {
    let (w, h) = (geometry.page_width_in, geometry.page_height_in);
    let name = format!("md_{}x{}in", round_to(w, 4), round_to(h, 4));
    let label = format!("{} \u{00D7} {} in", round_to(w, 2), round_to(h, 2));
    gtk::PaperSize::new_custom(&name, &label, w, h, gtk::Unit::Inch)
}

fn round_to(value: f64, places: i32) -> f64 {
    let factor = 10f64.powi(places);
    (value * factor).round() / factor
}

/// The page setup every md print starts from: `paper`, portrait, `margin_in` on all four sides.
fn page_setup(paper: &gtk::PaperSize, margin_in: f64) -> gtk::PageSetup {
    let setup = gtk::PageSetup::new();
    setup.set_paper_size(paper);
    setup.set_orientation(gtk::PageOrientation::Portrait);
    setup.set_top_margin(margin_in, gtk::Unit::Inch);
    setup.set_bottom_margin(margin_in, gtk::Unit::Inch);
    setup.set_left_margin(margin_in, gtk::Unit::Inch);
    setup.set_right_margin(margin_in, gtk::Unit::Inch);
    setup
}

/// Waits for the first of `finished` / `failed` (`finished` follows `failed`; only the first
/// answer counts).
fn outcome(operation: &webkit::PrintOperation) -> Completion<Result<(), String>> {
    let done: Completion<Result<(), String>> = Completion::new();
    let answered = Rc::new(Cell::new(false));
    {
        let (done, answered) = (done.clone(), Rc::clone(&answered));
        operation.connect_failed(move |_, err| {
            if !answered.replace(true) {
                done.complete(Err(err.message().to_owned()));
            }
        });
    }
    {
        let (done, answered) = (done.clone(), answered);
        operation.connect_finished(move |_| {
            if !answered.replace(true) {
                done.complete(Ok(()));
            }
        });
    }
    done
}

/// Prints the page in `view` to `path` as PDF on `geometry`'s paper with its margins — no dialog.
pub async fn print_to_file(view: &webkit::WebView, path: &Path, geometry: &PrintGeometry) -> Result<(), String> {
    let paper = paper_size(geometry);
    let settings = gtk::PrintSettings::new();
    settings.set_printer(&file_printer_name());
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_FILE_FORMAT, Some("pdf"));
    settings.set(gtk::PRINT_SETTINGS_OUTPUT_URI, Some(gio::File::for_path(path).uri().as_str()));
    settings.set_paper_size(&paper);
    settings.set_orientation(gtk::PageOrientation::Portrait);

    let operation = webkit::PrintOperation::new(view);
    operation.set_print_settings(&settings);
    operation.set_page_setup(&page_setup(&paper, geometry.margin_in));
    let done = outcome(&operation);
    operation.print();
    match glib::future_with_timeout(PRINT_TIMEOUT, done.wait()).await {
        Ok(result) => result,
        Err(_) => Err(format!("no answer from the print operation in {} s", PRINT_TIMEOUT.as_secs())),
    }
}

/// How Print… ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrintOutcome {
    /// The reader cancelled the dialog.
    Cancelled,
    /// The pages went to the printer.
    Printed,
    /// The printer said no; its words (swallowed by the pipeline, as on the Mac).
    Failed(String),
}

/// GTK's print dialog over the page in `view`, preset to A4 portrait with 0.5 in margins (the PDF's
/// geometry), modal to `parent`. Resolves when the dialog is gone and — after Print — the job has
/// finished or failed.
///
/// `run_dialog` runs a nested main loop, so it is called from an idle and completes a
/// [`Completion`]: no future is being polled while the nested loop runs.
pub async fn run_print_dialog(view: &webkit::WebView, parent: &gtk::Window) -> PrintOutcome {
    let geometry = PrintGeometry::paper();
    let paper = gtk::PaperSize::new(Some(gtk::PAPER_NAME_A4));
    let settings = gtk::PrintSettings::new();
    settings.set_paper_size(&paper);
    settings.set_orientation(gtk::PageOrientation::Portrait);

    let operation = webkit::PrintOperation::new(view);
    operation.set_print_settings(&settings);
    operation.set_page_setup(&page_setup(&paper, geometry.margin_in));
    let job = outcome(&operation);

    let answered: Completion<webkit::PrintOperationResponse> = Completion::new();
    {
        let (operation, parent, answered) = (operation.clone(), parent.clone(), answered.clone());
        glib::idle_add_local_once(move || {
            answered.complete(operation.run_dialog(Some(&parent)));
        });
    }
    match answered.wait().await {
        webkit::PrintOperationResponse::Print => match glib::future_with_timeout(PRINT_TIMEOUT, job.wait()).await {
            Ok(Ok(())) => PrintOutcome::Printed,
            Ok(Err(message)) => PrintOutcome::Failed(message),
            Err(_) => PrintOutcome::Failed(format!("no answer from the printer in {} s", PRINT_TIMEOUT.as_secs())),
        },
        _ => PrintOutcome::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use md_core::document::page_size::PageSize;

    #[test]
    fn geometry_rounds_for_names_only() {
        assert_eq!(round_to(595.2 / 72.0, 4), 8.2667);
        assert_eq!(round_to(6.0, 2), 6.0);
        let six_by_nine = PrintGeometry::for_page(&PageSize::SIX_BY_NINE);
        assert_eq!((six_by_nine.page_width_in, six_by_nine.page_height_in, six_by_nine.margin_in), (6.0, 9.0, 0.5));
    }
}
