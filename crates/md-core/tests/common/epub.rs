//! Helpers the EPUB suites share (`tests/export_epub_*.rs`): md.win's private helpers from
//! `EpubExportTests`, `EpubAdversarialTests` and `EpubMetadataTests`, plus what .NET gave those
//! tests for free — a base64 encoder (`Convert.ToBase64String`), an instant built from a civil date
//! and an offset (`DateTimeOffset`), and an independent central-directory walk standing in for
//! `System.IO.Compression.ZipArchive`. Each is written from its spec, not from the code under test.
#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use md_core::book::structured_book::{BookSection, BookUnit, StructuredBook};
use md_core::export::ZipEntry;
use md_core::export::zip_reader;

/// `new UTF8Encoding(false, throwOnInvalidBytes: true).GetString(bytes)`.
pub fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("the entry is UTF-8")
}

/// A one-pixel-ish PNG stand-in: the app's bytes are opaque to Core, only their identity matters.
pub fn png(marker: u8) -> Vec<u8> {
    vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, marker]
}

/// The named entry's bytes, or `None` (md.win's `Entry` / `ZipFixtures.Named`).
pub fn entry<'a>(entries: &'a [ZipEntry], name: &str) -> Option<&'a [u8]> {
    entries.iter().find(|entry| entry.name == name).map(|entry| entry.data.as_slice())
}

/// The named entry as text; the entry must exist.
pub fn entry_text(entries: &[ZipEntry], name: &str) -> String {
    text(entry(entries, name).unwrap_or_else(|| panic!("no entry {name}")))
}

/// `ZipReader.Entries(archive)`, asserted non-null.
pub fn read(archive: &[u8]) -> Vec<ZipEntry> {
    zip_reader::entries(archive, None).expect("the archive reads back")
}

pub fn unit(title: &str, source: &str) -> BookUnit {
    BookUnit { title: title.to_string(), source: source.to_string() }
}

pub fn section(title: &str, units: Vec<BookUnit>) -> BookSection {
    BookSection { title: title.to_string(), units }
}

pub fn book(title: &str, front_units: Vec<BookUnit>, sections: Vec<BookSection>) -> StructuredBook {
    StructuredBook { title: title.to_string(), front_units, sections }
}

/// Every `href="…"` value with at least one character — `Regex.Matches(text, "href=\"([^\"]+)\"")`.
pub fn hrefs(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("href=\"") {
        let value = from + at + "href=\"".len();
        match text[value..].find('"') {
            Some(0) => from = from + at + 1,
            Some(length) => {
                found.push(text[value..value + length].to_string());
                from = value + length + 1;
            }
            None => break,
        }
    }
    found
}

/// `Convert.ToBase64String`, RFC 4648 §4 with padding.
pub fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |acc, (i, &b)| acc | (u32::from(b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((bits >> (18 - 6 * i)) & 0x3F) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Days from 1970-01-01 to a proleptic Gregorian date — the textbook day count (a year's days, its
/// leap days, the month table), deliberately not the era arithmetic the module uses.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mut days = 0i64;
    if year >= 1970 {
        for y in 1970..year {
            days += if leap(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if leap(y) { 366 } else { 365 };
        }
    }
    let lengths = [31, if leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    days += lengths[..month as usize - 1].iter().sum::<i64>();
    days + i64::from(day) - 1
}

/// `new DateTimeOffset(year, month, day, hour, minute, second, millisecond, offset)` as the
/// instant it names.
pub fn instant(
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millisecond: u32,
    offset_minutes: i64,
) -> SystemTime {
    let seconds = days_from_civil(year, month, day) * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60
        - offset_minutes * 60
        + i64::from(second);
    let millis = seconds * 1000 + i64::from(millisecond);
    if millis >= 0 {
        UNIX_EPOCH + Duration::from_millis(millis.unsigned_abs())
    } else {
        UNIX_EPOCH - Duration::from_millis(millis.unsigned_abs())
    }
}

/// One central-directory record, read straight off the bytes.
#[derive(Debug)]
pub struct CentralRecord {
    pub name: String,
    pub method: u16,
    pub compressed: u32,
    pub uncompressed: u32,
    pub data: Vec<u8>,
}

pub fn u16_at(data: &[u8], index: usize) -> u16 {
    u16::from_le_bytes([data[index], data[index + 1]])
}

pub fn u32_at(data: &[u8], index: usize) -> u32 {
    u32::from_le_bytes([data[index], data[index + 1], data[index + 2], data[index + 3]])
}

/// The archive as `ZipArchive` lists it: the end record (no comment, so the last 22 bytes), then
/// each central record, then that member's stored payload through its local header. Independent of
/// md-core's reader, which the tests compare it with.
pub fn central_directory(archive: &[u8]) -> Vec<CentralRecord> {
    let end = archive.len() - 22;
    assert_eq!(archive[end..end + 4], [0x50, 0x4B, 0x05, 0x06], "end of central directory");
    let count = usize::from(u16_at(archive, end + 10));
    let mut cursor = u32_at(archive, end + 16) as usize;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        assert_eq!(archive[cursor..cursor + 4], [0x50, 0x4B, 0x01, 0x02], "central record");
        let method = u16_at(archive, cursor + 10);
        let compressed = u32_at(archive, cursor + 20);
        let uncompressed = u32_at(archive, cursor + 24);
        let name_length = usize::from(u16_at(archive, cursor + 28));
        let extra = usize::from(u16_at(archive, cursor + 30));
        let comment = usize::from(u16_at(archive, cursor + 32));
        let local = u32_at(archive, cursor + 42) as usize;
        let name = text(&archive[cursor + 46..cursor + 46 + name_length]);
        assert_eq!(archive[local..local + 4], [0x50, 0x4B, 0x03, 0x04], "local header");
        let payload = local + 30 + usize::from(u16_at(archive, local + 26)) + usize::from(u16_at(archive, local + 28));
        let data = archive[payload..payload + compressed as usize].to_vec();
        records.push(CentralRecord { name, method, compressed, uncompressed, data });
        cursor += 46 + name_length + extra + comment;
    }
    records
}
