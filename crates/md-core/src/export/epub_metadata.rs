//! What a document's front matter adds to its EPUB package document. Port of md.win
//! `Md.Core.Export.EpubMetadata` (`EpubMetadata.cs`: the records `EpubMetadata`, `EpubCover`,
//! `CoverReference` and the static class `EpubMetadataReader`, whose members are this module's
//! free functions).
//!
//! From 1.6 every md EPUB writer reads the same front-matter keys, by the same rules, into the same
//! lines in the same order (md.vscode's `src/export/epub-metadata.ts` is where the family wrote them
//! down; `tests/export_epub_metadata.rs` carries its vectors):
//!
//! | Key | Becomes |
//! | --- | --- |
//! | `author` | `<dc:creator>` |
//! | `language` | `<dc:language>` when it is a language tag, else `en` |
//! | `date` | `<dc:date>` when it is a real W3C date, else left out |
//! | `description` | `<dc:description>` |
//! | `series`, `series_index` | a `belongs-to-collection` of type series, with its position when the index is a number |
//! | `cover` | a `cover-image` item, when the image can be read |
//!
//! A key matches as `title:` does — case-insensitively, the first non-empty value winning, trimmed
//! of whitespace and newlines. Only a single document's EPUB reads them; a book keeps
//! [`EpubMetadata::default`] (book metadata is a later release's).
//!
//! Pure: no file system, no network — reading a cover is the app's. md.win's regexes are written
//! out by hand here (md-core has no regex crate); each one's .NET spelling is quoted above the
//! matcher that replaces it.

use std::fmt;

use super::epub_export::xml_escape;
use crate::markdown::MetadataField;
use crate::text::whitespace;

/// The package document's metadata from a document's front matter.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EpubMetadata {
    pub creator: Option<String>,
    pub language: String,
    pub date: Option<String>,
    pub description: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
}

/// md.win's `EpubMetadata.Default`: a document with no front matter — the package document
/// exactly as 1.5 wrote it.
impl Default for EpubMetadata {
    fn default() -> Self {
        EpubMetadata {
            creator: None,
            language: "en".to_string(),
            date: None,
            description: None,
            series: None,
            series_index: None,
        }
    }
}

/// A cover image recognised by its own bytes. `extension` is `png`, `jpg`, `gif` or `webp`.
/// Equality is structural, over the bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EpubCover {
    pub extension: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

impl EpubCover {
    pub fn new(extension: impl Into<String>, media_type: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        EpubCover { extension: extension.into(), media_type: media_type.into(), data: data.into() }
    }

    /// The cover's path inside `OEBPS/`.
    pub fn href(&self) -> String {
        format!("images/cover.{}", self.extension)
    }
}

/// md.win's `ToString`: the path, the type and the size — never the bytes, which can be 20 MiB.
impl fmt::Debug for EpubCover {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EpubCover({}, {}, {} bytes)", self.href(), self.media_type, self.data.len())
    }
}

/// Where a `cover:` value says the image is.
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum CoverReference {
    /// A `data:` image, already decoded.
    Inline(Vec<u8>),
    /// An `https:` image, to be fetched.
    Https(String),
    /// A path relative to the document's own folder.
    Local(String),
}

impl fmt::Debug for CoverReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoverReference::Inline(data) => write!(f, "Inline({} bytes)", data.len()),
            CoverReference::Https(url) => f.debug_tuple("Https").field(url).finish(),
            CoverReference::Local(path) => f.debug_tuple("Local").field(path).finish(),
        }
    }
}

/// A cover larger than this is skipped: a reader shows a thumbnail, not a poster.
pub const MAX_COVER_BYTES: usize = 20 * 1024 * 1024;

/// The first non-empty value of `key` — [`super::epub_export::document_title_from_front_matter`]'s
/// rule.
///
/// md.win compares the key with `OrdinalIgnoreCase`. Every key this module asks for is ASCII, and
/// folding ASCII only is exact against an ASCII key: .NET's ordinal casing table deliberately maps
/// U+0131 `ı` and U+017F `ſ` to themselves (the only two non-ASCII scalars whose simple upper case
/// is ASCII), so no other character can stand in for a letter of the key.
pub fn field_value(fields: &[MetadataField], key: &str) -> Option<String> {
    for field in fields {
        if !field.key.eq_ignore_ascii_case(key) {
            continue;
        }
        let value = whitespace::trim_wsnl(&field.value);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// The package document's metadata from a document's front matter.
pub fn from_front_matter(fields: &[MetadataField]) -> EpubMetadata {
    let language = field_value(fields, "language");
    let date = field_value(fields, "date");
    let series = field_value(fields, "series");
    let series_index = field_value(fields, "series_index");
    let series_index = match (&series, series_index) {
        (Some(_), Some(index)) if is_series_index(&index) => Some(index),
        _ => None,
    };
    EpubMetadata {
        creator: field_value(fields, "author"),
        language: language.filter(|value| is_language_tag(value)).unwrap_or_else(|| "en".to_string()),
        date: date.filter(|value| is_w3c_date(value)),
        description: field_value(fields, "description"),
        series,
        series_index,
    }
}

// The regexes, by hand. md.win spells every class out in ASCII, uses `\z` rather than `$` (which
// also matches before a final `\n` in .NET) and never `\d` (which matches every Unicode digit in
// .NET): the other ports' regexes are ASCII-only. A byte walk over ASCII classes is exact on UTF-8.

/// `^[A-Za-z]{2,3}(?:-[A-Za-z0-9]{1,8})*\z`
fn is_language_tag(value: &str) -> bool {
    let mut parts = value.split('-');
    let primary = parts.next().unwrap_or("");
    (2..=3).contains(&primary.len())
        && primary.bytes().all(|b| b.is_ascii_alphabetic())
        && parts.all(|part| (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// `^[0-9]+(?:\.[0-9]+)?\z`
fn is_series_index(value: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    match value.split_once('.') {
        Some((whole, fraction)) => digits(whole) && digits(fraction),
        None => digits(value),
    }
}

/// `^[A-Za-z0-9+/]*={0,2}\z`
fn is_base64_payload(value: &str) -> bool {
    let bytes = value.as_bytes();
    let body = bytes.iter().take_while(|&&b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/').count();
    let padding = &bytes[body..];
    padding.len() <= 2 && padding.iter().all(|&b| b == b'=')
}

/// `^[A-Za-z][A-Za-z0-9+.-]*:` (a prefix match: `IsMatch`, no `\z`).
fn has_scheme(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_alphabetic) {
        return false;
    }
    let run = bytes[1..].iter().take_while(|&&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-')).count();
    bytes.get(1 + run) == Some(&b':')
}

/// The capture groups of md.win's `W3CDate` regex:
/// `^([0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2})(?:T([0-9]{2}):([0-9]{2})(?::([0-9]{2}))?(?:Z|[+-]([0-9]{2}):([0-9]{2})))?)?)?\z`.
/// Every optional group is followed by `\z`, so the walk is deterministic: at each step either
/// the input has ended, or the next group must match in full.
#[derive(Default)]
struct DateParts {
    year: u32,
    month: Option<u32>,
    day: Option<u32>,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>,
    offset_hour: Option<u32>,
    offset_minute: Option<u32>,
}

/// Exactly `count` ASCII digits at `at`, as a number.
fn ascii_digits(bytes: &[u8], at: usize, count: usize) -> Option<u32> {
    let run = bytes.get(at..at + count)?;
    if !run.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(run.iter().fold(0, |n, &b| n * 10 + u32::from(b - b'0')))
}

fn w3c_date_parts(value: &str) -> Option<DateParts> {
    let b = value.as_bytes();
    let mut parts = DateParts { year: ascii_digits(b, 0, 4)?, ..DateParts::default() };
    if b.len() == 4 {
        return Some(parts);
    }
    // -MM
    if b.get(4) != Some(&b'-') {
        return None;
    }
    parts.month = Some(ascii_digits(b, 5, 2)?);
    if b.len() == 7 {
        return Some(parts);
    }
    // -DD
    if b.get(7) != Some(&b'-') {
        return None;
    }
    parts.day = Some(ascii_digits(b, 8, 2)?);
    if b.len() == 10 {
        return Some(parts);
    }
    // THH:MM
    if b.get(10) != Some(&b'T') || b.get(13) != Some(&b':') {
        return None;
    }
    parts.hour = Some(ascii_digits(b, 11, 2)?);
    parts.minute = Some(ascii_digits(b, 14, 2)?);
    let mut at = 16;
    // (?::SS)? — greedy; giving it back could not help, since the zone cannot start with ':'.
    if b.get(at) == Some(&b':')
        && let Some(second) = ascii_digits(b, at + 1, 2)
    {
        parts.second = Some(second);
        at += 3;
    }
    // (?:Z|[+-]HH:MM), then \z.
    match b.get(at) {
        Some(b'Z') => at += 1,
        Some(b'+' | b'-') => {
            if b.get(at + 3) != Some(&b':') {
                return None;
            }
            parts.offset_hour = Some(ascii_digits(b, at + 1, 2)?);
            parts.offset_minute = Some(ascii_digits(b, at + 4, 2)?);
            at += 6;
        }
        _ => return None,
    }
    (at == b.len()).then_some(parts)
}

/// Whether `value` is a W3CDTF date EPUB accepts: the shape, and then real ranges — month 01–12, a
/// day that month has (Gregorian leap years), hour 00–23, minute and second 00–59, a zone offset of
/// 00–23 hours and 00–59 minutes.
pub fn is_w3c_date(value: &str) -> bool {
    let Some(parts) = w3c_date_parts(value) else {
        return false;
    };
    let year = parts.year;
    if let Some(month) = parts.month {
        if !(1..=12).contains(&month) {
            return false;
        }
        if let Some(day) = parts.day {
            let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
            if !(1..=days[month as usize - 1]).contains(&day) {
                return false;
            }
        }
    }
    parts.hour.is_none_or(|n| n <= 23)
        && parts.minute.is_none_or(|n| n <= 59)
        && parts.second.is_none_or(|n| n <= 59)
        && parts.offset_hour.is_none_or(|n| n <= 23)
        && parts.offset_minute.is_none_or(|n| n <= 59)
}

/// md.win's `StartsWithAsciiIgnoringCase`: folds A–Z only; `prefix` is lower-case ASCII. A
/// non-ASCII scalar's UTF-8 bytes are all ≥ 0x80 and can never equal an ASCII byte.
fn starts_with_ascii_ignoring_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len() && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Classify a `cover:` value, or `None` when it can never be read: a `data:` URI that is not strict
/// base64, `http:` or any other scheme, an absolute path, a fragment.
pub fn reference(value: Option<&str>) -> Option<CoverReference> {
    let value = value.filter(|value| !value.is_empty())?;
    if starts_with_ascii_ignoring_case(value, "data:") {
        let comma = value.find(',')?;
        let header = &value.as_bytes()[5..comma];
        // `EndsWith(";base64", OrdinalIgnoreCase)`: exact as an ASCII fold (see `field_value`).
        const BASE64: &[u8] = b";base64";
        if header.len() < BASE64.len() || !header[header.len() - BASE64.len()..].eq_ignore_ascii_case(BASE64) {
            return None;
        }
        // md.win measures in UTF-16 units and this in bytes; they differ only when the payload is
        // not ASCII, which the payload match below refuses whichever check it reaches first.
        let length = value.len() - comma - 1;
        if length == 0 || length % 4 != 0 {
            return None;
        }
        // Past the cover cap once decoded: refused before the payload is matched or decoded.
        if length / 4 * 3 > MAX_COVER_BYTES + 2 {
            return None;
        }
        let payload = &value[comma + 1..];
        if !is_base64_payload(payload) {
            return None;
        }
        return Some(CoverReference::Inline(decode_base64(payload.as_bytes())));
    }
    if starts_with_ascii_ignoring_case(value, "https://") {
        return Some(CoverReference::Https(value.to_string()));
    }
    if value.contains("://") || has_scheme(value) {
        return None;
    }
    if matches!(value.as_bytes()[0], b'/' | b'\\' | b'#') {
        return None;
    }
    Some(CoverReference::Local(value.to_string()))
}

/// `Convert.FromBase64String` over a payload [`is_base64_payload`] has accepted and whose length is
/// a multiple of four. Like .NET, the unused low bits of a padded final quantum are ignored.
fn decode_base64(payload: &[u8]) -> Vec<u8> {
    fn sextet(b: u8) -> u32 {
        u32::from(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => 0,
        })
    }
    let mut out = Vec::with_capacity(payload.len() / 4 * 3);
    for quantum in payload.chunks(4) {
        let padding = quantum.iter().filter(|&&b| b == b'=').count();
        let bits = quantum.iter().fold(0u32, |acc, &b| (acc << 6) | sextet(b));
        let bytes = [(bits >> 16) as u8, (bits >> 8) as u8, bits as u8];
        out.extend_from_slice(&bytes[..3 - padding]);
    }
    out
}

/// A cover from its bytes: PNG, JPEG, GIF or WebP by magic number, no larger than
/// [`MAX_COVER_BYTES`].
pub fn recognise(data: Option<&[u8]>) -> Option<EpubCover> {
    let data = data?;
    if data.is_empty() || data.len() > MAX_COVER_BYTES {
        return None;
    }
    let at = |offset: usize, bytes: &[u8]| data.get(offset..offset + bytes.len()) == Some(bytes);
    let cover = |extension: &str, media_type: &str| Some(EpubCover::new(extension, media_type, data));
    if at(0, &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return cover("png", "image/png");
    }
    if at(0, &[0xFF, 0xD8, 0xFF]) {
        return cover("jpg", "image/jpeg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return cover("gif", "image/gif");
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return cover("webp", "image/webp");
    }
    None
}

/// The lines between `dc:title` and `dcterms:modified` — identical text in all five md EPUB
/// writers. With [`EpubMetadata::default`] and no cover this is exactly the one
/// `<dc:language>en</dc:language>` line every md EPUB has always carried.
pub fn lines(metadata: &EpubMetadata, cover: Option<&EpubCover>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(creator) = &metadata.creator {
        lines.push(format!("<dc:creator>{}</dc:creator>", xml_escape(creator)));
    }
    lines.push(format!("<dc:language>{}</dc:language>", xml_escape(&metadata.language)));
    if let Some(date) = &metadata.date {
        lines.push(format!("<dc:date>{}</dc:date>", xml_escape(date)));
    }
    if let Some(description) = &metadata.description {
        lines.push(format!("<dc:description>{}</dc:description>", xml_escape(description)));
    }
    if let Some(series) = &metadata.series {
        lines.push(format!("<meta property=\"belongs-to-collection\" id=\"series\">{}</meta>", xml_escape(series)));
        lines.push("<meta refines=\"#series\" property=\"collection-type\">series</meta>".to_string());
        if let Some(index) = &metadata.series_index {
            lines.push(format!("<meta refines=\"#series\" property=\"group-position\">{}</meta>", xml_escape(index)));
        }
    }
    // EPUB 3 finds the cover by the manifest property; this EPUB 2 meta is for the readers —
    // Kindle's converter among them — that still look for it.
    if cover.is_some() {
        lines.push("<meta name=\"cover\" content=\"cover-image\"/>".to_string());
    }
    lines
}

/// The cover's manifest item — the last item of every md EPUB that has one.
pub fn manifest_item(cover: &EpubCover) -> String {
    format!(
        "<item id=\"cover-image\" href=\"{}\" media-type=\"{}\" properties=\"cover-image\"/>",
        cover.href(),
        cover.media_type
    )
}
