//! Port of md.win `tests/Md.App.Logic.Tests/Export/EpubCoverTests.cs`.
//!
//! The EPUB's `cover:` (1.6, #2) read for real: beside the document with the TextBundle export's
//! containment, fetched over https through the pipeline's seam, or decoded from a `data:` image —
//! and, whatever goes wrong, a book without a cover rather than a failed export.
//!
//! LINUX: md.win's fetch policy lives in `HttpClient` plumbing tested only for its redirect helper
//! and its bounded read. Here the policy is md-logic's own over the one-request `CoverFetcher` seam,
//! so the rest of it — the redirect count, the https check of each hop, the success range, the
//! declared and the streamed cap, the 15 s deadline and cancellation — is pinned below as well.

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use common::export_harness::{DOCS, ExportHarness};
use common::task::Task;
use md_core::export::epub_metadata::MAX_COVER_BYTES;
use md_core::export::{ZipEntry, zip_reader};
use md_logic::export::cover_reader::{self, FETCH_TIMEOUT, MAX_REDIRECTS};
use md_logic::export::{CoverBody, CoverFetcher};
use md_logic::testing::*;

const PNG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16];

fn source(cover: &str) -> String {
    format!("---\ntitle: Book\nauthor: A\ncover: {cover}\n---\n\nText.\n")
}

fn written(h: &ExportHarness, path: &str) -> Vec<ZipEntry> {
    zip_reader::entries(&h.written_bytes(path).expect("written"), None).expect("a zip")
}

fn opf(h: &ExportHarness, path: &str) -> String {
    let entries = written(h, path);
    let entry = entries.iter().find(|entry| entry.name == "OEBPS/content.opf").expect("the package document");
    String::from_utf8(entry.data.clone()).unwrap()
}

fn has_cover_entry(h: &ExportHarness, path: &str) -> bool {
    written(h, path).iter().any(|entry| entry.name.contains("cover"))
}

fn notes() -> String {
    format!("{DOCS}/notes.md")
}

/// `Convert.ToBase64String`.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let bits = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((bits >> (18 - 6 * i)) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn a_cover_beside_the_document_goes_on_the_book() {
    let h = ExportHarness::new();
    h.files.add_file(&format!("{DOCS}/cover.png"), PNG);
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("cover.png"), "notes.md", Some(&notes())));

    let entries = written(&h, &path);
    let cover = entries.last().expect("entries");
    assert_eq!(cover.name, "OEBPS/images/cover.png");
    assert_eq!(cover.data, PNG);
    assert!(opf(&h, &path).contains("<dc:creator>A</dc:creator>"));
    assert!(opf(&h, &path).contains("properties=\"cover-image\""));
    assert!(h.fetched().is_empty());
}

#[test]
fn a_cover_file_past_the_cap_is_never_read() {
    let h = ExportHarness::new();
    let mut big = vec![0u8; MAX_COVER_BYTES + 1];
    big[..PNG.len()].copy_from_slice(PNG);
    h.files.add_file(&format!("{DOCS}/big.png"), &big);
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("big.png"), "notes.md", Some(&notes())));

    assert!(!opf(&h, &path).contains("properties=\"cover-image\""));
    assert!(!h.files.reads().iter().any(|read| read.ends_with("big.png")));
}

#[test]
fn a_cover_that_cannot_be_read_is_left_out_silently() {
    for reference in [
        "../outside.png", // climbs out of the document's folder
        "missing.png",    // not there
        "notes.txt",      // there, but not an image
    ] {
        let h = ExportHarness::new();
        h.files.add_file("/home/writer/outside.png", PNG);
        h.files.add_file(&format!("{DOCS}/notes.txt"), "not a picture");
        let path = h.will_save(".epub");

        h.drain(h.pipeline.export_epub(&source(reference), "notes.md", Some(&notes())));

        assert!(!has_cover_entry(&h, &path), "{reference}");
        assert!(!opf(&h, &path).contains("cover-image"), "{reference}");
        assert!(h.alerts.warnings().is_empty(), "{reference}");
    }
}

#[test]
fn an_unsaved_document_finds_no_local_cover() {
    let h = ExportHarness::new();
    h.files.add_file(&format!("{DOCS}/cover.png"), PNG);
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("cover.png"), "Untitled", None));

    assert!(!has_cover_entry(&h, &path));
}

#[test]
fn an_https_cover_is_fetched_once() {
    let h = ExportHarness::new();
    h.set_fetch(|_| Some(JPEG.to_vec()));
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("https://example.com/c.jpg"), "notes.md", None));

    assert_eq!(h.fetched(), ["https://example.com/c.jpg"]);
    assert_eq!(written(&h, &path).last().expect("entries").name, "OEBPS/images/cover.jpg");
}

#[test]
fn a_failed_fetch_is_a_book_without_a_cover() {
    let h = ExportHarness::new();
    h.set_fetch(|_| None); // the request itself fails ("offline")
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("https://example.com/c.jpg"), "notes.md", None));

    assert!(!has_cover_entry(&h, &path));
    assert!(h.alerts.warnings().is_empty());
}

#[test]
fn nothing_is_fetched_for_a_reference_that_can_never_be_read() {
    for reference in [
        "http://example.com/c.jpg",
        "C:\\pictures\\c.jpg",
        "/home/writer/pictures/c.jpg", // LINUX: an absolute path is md-core's refusal here
    ] {
        let h = ExportHarness::new();
        h.set_fetch(|_| Some(JPEG.to_vec()));
        h.will_save(".epub");

        h.drain(h.pipeline.export_epub(&source(reference), "notes.md", Some(&notes())));

        assert!(h.fetched().is_empty(), "{reference}");
    }
}

#[test]
fn a_data_cover_needs_no_file_and_no_fetch() {
    let h = ExportHarness::new();
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source(&format!("data:image/png;base64,{}", base64(PNG))), "Untitled", None));

    assert_eq!(written(&h, &path).last().expect("entries").data, PNG);
    assert!(h.fetched().is_empty());
}

#[test]
fn a_redirect_is_followed_only_to_https() {
    for (from, location, expected) in [
        ("https://a.example/c.png", "https://b.example/d.png", Some("https://b.example/d.png")),
        ("https://a.example/x/c.png", "/d.png", Some("https://a.example/d.png")),
        ("https://a.example/x/c.png", "d.png", Some("https://a.example/x/d.png")),
        ("https://a.example/c.png", "//b.example/d.png", Some("https://b.example/d.png")),
        ("https://a.example/c.png", "http://b.example/d.png", None),
        ("https://a.example/c.png", "ftp://b.example/d.png", None),
    ] {
        assert_eq!(cover_reader::followed_redirect(from, Some(location)).as_deref(), expected, "{from} → {location}");
        assert_eq!(cover_reader::followed_redirect(from, None), None);
    }
}

// ---- the fetch policy, over the seam ----

fn fetch(fetcher: &FakeCoverFetcher, scheduler: &FakeScheduler, url: &str) -> Option<Vec<u8>> {
    let cancel = CancelToken::new();
    let task = Task::start(cover_reader::fetch(fetcher, scheduler, url, &cancel));
    assert!(task.is_completed(), "every answer was scripted");
    task.output().expect("not cancelled")
}

#[test]
fn a_non_https_url_is_never_requested() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.set_handler(|_| Ok(FakeCoverAnswer::ok(PNG)));

    assert_eq!(fetch(&fetcher, &scheduler, "http://example.com/c.png"), None);
    assert_eq!(fetch(&fetcher, &scheduler, "https:///c.png"), None); // no host
    assert!(fetcher.requests().is_empty());
}

struct Chunks(RefCell<VecDeque<Vec<u8>>>);

impl CoverBody for Chunks {
    fn next_chunk(&self) -> LocalBoxFuture<'static, Result<Option<Vec<u8>>, String>> {
        ready(Ok(self.0.borrow_mut().pop_front()))
    }
}

#[test]
fn a_bounded_read_stops_past_the_limit() {
    let small = Chunks(RefCell::new(VecDeque::from([vec![0u8; 10]])));
    assert_eq!(block_on_ref(cover_reader::read_bounded(&small, 10)).map(|data| data.map(|d| d.len())), Ok(Some(10)));

    let big = Chunks(RefCell::new(VecDeque::from([vec![0u8; 11]])));
    assert_eq!(block_on_ref(cover_reader::read_bounded(&big, 10)), Ok(None));
}

fn block_on_ref<T>(future: impl std::future::Future<Output = T>) -> T {
    let task = Task::start(future);
    assert!(task.is_completed());
    task.output()
}

#[test]
fn redirects_are_followed_hop_by_hop_and_relative_locations_resolve_against_the_current_url() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.queue(Ok(FakeCoverAnswer::redirect(301, Some("https://cdn.example/a/b.png"))));
    fetcher.queue(Ok(FakeCoverAnswer::redirect(302, Some("../c.png"))));
    fetcher.queue(Ok(FakeCoverAnswer::ok(PNG)));

    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/cover.png"), Some(PNG.to_vec()));
    assert_eq!(
        fetcher.requests(),
        ["https://example.com/cover.png", "https://cdn.example/a/b.png", "https://cdn.example/c.png"]
    );
}

#[test]
fn five_redirects_are_followed_and_a_sixth_is_refused() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    for hop in 0..MAX_REDIRECTS {
        fetcher.queue(Ok(FakeCoverAnswer::redirect(307, Some(&format!("/hop{hop}.png")))));
    }
    fetcher.queue(Ok(FakeCoverAnswer::ok(PNG)));
    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), Some(PNG.to_vec()));
    assert_eq!(fetcher.requests().len(), MAX_REDIRECTS + 1);

    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.set_handler(|_| Ok(FakeCoverAnswer::redirect(302, Some("/again.png"))));
    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), None);
    assert_eq!(fetcher.requests().len(), MAX_REDIRECTS + 1); // the sixth Location is never requested
}

#[test]
fn a_redirect_off_https_or_without_a_location_is_never_requested() {
    for answer in
        [FakeCoverAnswer::redirect(302, Some("http://example.com/c.png")), FakeCoverAnswer::redirect(301, None)]
    {
        let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
        fetcher.queue(Ok(answer));
        fetcher.set_handler(|_| Ok(FakeCoverAnswer::ok(PNG)));

        assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), None);
        assert_eq!(fetcher.requests(), ["https://example.com/c.png"]);
    }
}

#[test]
fn only_a_2xx_answer_is_a_cover() {
    for status in [404, 500, 204] {
        let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
        let mut answer = FakeCoverAnswer::ok(PNG);
        answer.status = status;
        fetcher.queue(Ok(answer));

        let fetched = fetch(&fetcher, &scheduler, "https://example.com/c.png");
        assert_eq!(fetched.is_some(), status == 204, "{status}"); // 204 is a success with this body
    }
}

#[test]
fn a_declared_length_past_the_cap_is_refused_before_the_body_is_read() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.queue(Ok(FakeCoverAnswer::ok(PNG).with_content_length(Some(MAX_COVER_BYTES as u64 + 1))));

    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), None);
    assert_eq!(fetcher.chunks_read(), 0);
}

#[test]
fn a_body_that_passes_the_cap_stops_being_read_at_the_chunk_that_passes_it() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    let eight_mib = vec![0u8; 8 * 1024 * 1024];
    fetcher.queue(Ok(FakeCoverAnswer::chunked(&[&eight_mib, &eight_mib, &eight_mib, &eight_mib])));

    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), None);
    assert_eq!(fetcher.chunks_read(), 3); // 24 MiB > 20 MiB; the fourth chunk is never asked for

    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.queue(Ok(FakeCoverAnswer::chunked(&[PNG, PNG])));
    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), Some([PNG, PNG].concat()));
}

#[test]
fn a_transfer_that_fails_mid_body_is_no_cover() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    let mut answer = FakeCoverAnswer::chunked(&[PNG]);
    answer.chunks.push(Err("connection reset".to_string()));
    fetcher.queue(Ok(answer));

    assert_eq!(fetch(&fetcher, &scheduler, "https://example.com/c.png"), None);
}

#[test]
fn fifteen_seconds_in_all_is_the_deadline_for_the_answer_and_for_the_body() {
    assert_eq!(FETCH_TIMEOUT, Duration::from_secs(15));
    assert_eq!(MAX_REDIRECTS, 5);

    // A server that never answers.
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    let _never = fetcher.hold();
    let cancel = CancelToken::new();
    let mut task = Task::start(cover_reader::fetch(&fetcher, &scheduler, "https://example.com/c.png", &cancel));
    scheduler.advance(FETCH_TIMEOUT - Duration::from_millis(1));
    assert!(!task.poll());
    scheduler.advance(Duration::from_millis(1));
    assert!(task.poll());
    assert_eq!(task.output(), Ok(None));
    assert_eq!(scheduler.pending_timers(), 0);

    // A body that stalls after its first chunk.
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    fetcher.queue(Ok(FakeCoverAnswer::chunked(&[PNG]).stalling()));
    let mut task = Task::start(cover_reader::fetch(&fetcher, &scheduler, "https://example.com/c.png", &cancel));
    assert!(!task.is_completed());
    scheduler.advance(FETCH_TIMEOUT);
    assert!(task.poll());
    assert_eq!(task.output(), Ok(None));
}

#[test]
fn a_cancelled_export_is_cancelled_not_coverless() {
    let (fetcher, scheduler) = (FakeCoverFetcher::new(), FakeScheduler::new());
    let _never = fetcher.hold();
    let cancel = CancelToken::new();
    let mut task = Task::start(cover_reader::fetch(&fetcher, &scheduler, "https://example.com/c.png", &cancel));
    assert!(!task.is_completed());

    cancel.cancel();
    assert!(task.poll());
    assert_eq!(task.output(), Err(Cancelled));

    let cancelled = CancelToken::new();
    cancelled.cancel();
    let task = Task::start(cover_reader::fetch(&fetcher, &scheduler, "https://example.com/c.png", &cancelled));
    assert_eq!(task.output(), Err(Cancelled));
    assert_eq!(fetcher.requests().len(), 1); // the second never asked
}

#[test]
fn a_cover_fetch_that_times_out_still_writes_the_book() {
    let h = ExportHarness::new();
    let _never = h.fetcher.hold();
    let path = h.will_save(".epub");

    h.drain(h.pipeline.export_epub(&source("https://example.com/c.jpg"), "notes.md", None));

    assert!(!has_cover_entry(&h, &path));
    assert!(h.alerts.warnings().is_empty());
}

#[test]
fn the_fetcher_seam_is_one_request_per_call() {
    // What the app implements: one GET, a redirect answered as itself. The fake shows the shape.
    let fetcher = FakeCoverFetcher::new();
    fetcher.queue(Ok(FakeCoverAnswer::redirect(302, Some("/elsewhere.png"))));
    let task = Task::start(fetcher.get("https://example.com/c.png".to_string()));
    let response = task.output().expect("answered");
    assert_eq!(response.status, 302);
    assert_eq!(response.location.as_deref(), Some("/elsewhere.png"));
}
