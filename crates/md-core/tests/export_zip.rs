//! The STORED writer (macOS `EPUBZipWriter`) and the central-directory reader (`ZipReader`): the
//! byte layout the EPUB and TextPack exports depend on, and the import path's defences —
//! inflate-on-demand, the size cap, refusal of anything that is not a zip it can parse. Port of
//! md.win `ZipTests.cs`.

mod export_zip_fixtures;

use export_zip_fixtures::*;
use md_core::export::text_bundle;
use md_core::export::zip_reader::{self, MAX_ENTRY_SIZE};
use md_core::export::{ZipEntry, ZipError, ZipWriter};

fn u16_at(data: &[u8], index: usize) -> u16 {
    u16::from_le_bytes([data[index], data[index + 1]])
}

fn u32_at(data: &[u8], index: usize) -> u32 {
    u32::from_le_bytes([data[index], data[index + 1], data[index + 2], data[index + 3]])
}

// MARK: Writer

#[test]
fn writer_produces_valid_stored_archive() {
    let mut zip = ZipWriter::new();
    zip.add("mimetype", b"application/epub+zip").unwrap();
    zip.add("OEBPS/a.txt", b"hello").unwrap();
    let data = zip.finish().unwrap();
    // Local file header signature "PK\3\4" at byte 0.
    assert_eq!(data[..4], [0x50, 0x4B, 0x03, 0x04]);
    // Compression method (bytes 8–9) is 0: stored, as EPUB requires of the mimetype.
    assert_eq!(data[8], 0);
    assert_eq!(data[9], 0);
    // The FIRST entry is the mimetype, its bytes verbatim right after the 30-byte header and
    // 8-byte name.
    assert_eq!(utf8_text(&data[30..38]), "mimetype");
    assert_eq!(utf8_text(&data[38..58]), "application/epub+zip");
    // End-of-central-directory record closes the archive.
    assert_eq!(data[data.len() - 22..data.len() - 18], [0x50, 0x4B, 0x05, 0x06]);
}

#[test]
fn writer_crc32_matches_known_vector() {
    // The canonical CRC-32 check value.
    assert_eq!(ZipWriter::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(ZipWriter::crc32(&[]), 0);
}

#[test]
fn writer_crc32_matches_published_vectors() {
    // Beyond md.win: the CRC-32/ISO-HDLC vectors every implementation publishes, a megabyte that
    // walks the whole table, and agreement with an independent bit-at-a-time implementation.
    let vectors: [(&[u8], u32); 7] = [
        (b"a", 0xE8B7_BE43),
        (b"abc", 0x3524_41C2),
        (b"message digest", 0x2015_9D7F),
        (b"abcdefghijklmnopqrstuvwxyz", 0x4C27_50BD),
        (b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", 0x1FC2_E6D2),
        (b"The quick brown fox jumps over the lazy dog", 0x414F_A339),
        (&[0u8; 32], 0x190A_55AD),
    ];
    for (input, expected) in vectors {
        assert_eq!(ZipWriter::crc32(input), expected, "{:?}", String::from_utf8_lossy(input));
    }
    assert_eq!(ZipWriter::crc32(&[0xFF; 32]), 0xFF6C_AB0B);
    let mut state = 0x1234_5678u32;
    let noise: Vec<u8> = (0..1 << 20)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect();
    assert_eq!(ZipWriter::crc32(&noise), bitwise_crc32(&noise));
}

#[test]
fn writer_layout_is_exact_and_deterministic() {
    // Two entries: 30-byte local headers, 46-byte central records, 22-byte end record, no extra
    // fields, no comment, no data descriptors — so the size is arithmetic.
    let data = stored(&[("mimetype", b"application/epub+zip"), ("OEBPS/a.txt", b"hello")]);
    let body = (30 + 8 + 20) + (30 + 11 + 5);
    let directory = (46 + 8) + (46 + 11);
    assert_eq!(data.len(), body + directory + 22);

    // The fixed DOS time 0 and date 0x0021 (1980-01-01), bytes 10..14 of the local header.
    assert_eq!(data[10..14], [0x00, 0x00, 0x21, 0x00]);
    // Central directory: record count twice, its length, and its offset (= body length).
    let end = data.len() - 22;
    assert_eq!(u16_at(&data, end + 8), 2);
    assert_eq!(u16_at(&data, end + 10), 2);
    assert_eq!(u32_at(&data, end + 12), directory as u32);
    assert_eq!(u32_at(&data, end + 16), body as u32);
    // The second central record points at the second local header.
    assert_eq!(u32_at(&data, body + 46 + 8 + 42), (30 + 8 + 20) as u32);
    // Flag bit 11 (UTF-8 names) deliberately clear, as on every port — in the local headers and
    // the central records alike.
    assert_eq!(u16_at(&data, 6), 0);
    assert_eq!(u16_at(&data, body + 8), 0);

    // A pure function of its payloads: the same input is the same archive.
    assert_eq!(data, stored(&[("mimetype", b"application/epub+zip"), ("OEBPS/a.txt", b"hello")]));
}

#[test]
fn writer_output_opens_in_a_foreign_reader() {
    // md.win: WriterOutputOpensInTheBclReader. The unzip -t of this port: a foreign reader must
    // accept the archive and hand back every payload — the suite's independent walker (which
    // checks every CRC) and, where installed, CPython's zipfile.
    let data = stored(&[("mimetype", b"application/epub+zip"), ("sub/b.txt", b"hello")]);
    let entries = assert_foreign_readers_accept(&data, &["mimetype", "sub/b.txt"]);
    assert_eq!(utf8_text(&entries[0].data), "application/epub+zip");
    assert_eq!(entries[0].data.len(), 20);
}

#[test]
fn writer_refuses_what_the_classic_format_cannot_record() {
    // md.win's `checked` casts throw; here the export gets an error and the writer is untouched.
    let mut zip = ZipWriter::new();
    zip.add("a", b"1").unwrap();
    let before = zip.finish().unwrap();
    assert_eq!(zip.add(&"n".repeat(65_536), b""), Err(ZipError::NameTooLong));
    assert_eq!(zip.finish().unwrap(), before);
    zip.add(&"n".repeat(65_535), b"").unwrap();
    for _ in 1..65_534 {
        zip.add("x", b"").unwrap();
    }
    assert_eq!(zip.add("one too many", b""), Err(ZipError::TooManyEntries));
    let archive = zip.finish().unwrap();
    assert_eq!(u16_at(&archive, archive.len() - 12), 65_535);
}

// MARK: Reader

#[test]
fn reader_parses_stored_archive() {
    // A stored archive built by the app's own writer: the reader must walk its central directory
    // and return each payload, including a nested path.
    let a = [1u8, 2, 3, 4, 5];
    let b = utf8("hello");
    let entries = zip_reader::entries(&stored(&[("a.bin", &a), ("sub/b.txt", &b)]), None).expect("a zip");
    assert_eq!(entries.len(), 2);
    assert_eq!(named(Some(&entries), "a.bin").map(|e| e.data.as_slice()), Some(&a[..]));
    assert_eq!(named(Some(&entries), "sub/b.txt").map(|e| e.data.as_slice()), Some(&b[..]));
}

#[test]
fn reader_inflates_deflated_entry() {
    // Real TextPacks deflate their entries; prove the inflate path against a hand-built method-8
    // archive (a stored-only reader would miss it).
    let payload = utf8(&"abcABC123 the quick brown fox ".repeat(40));
    let archive = deflated("x/text.md", &payload, None, 8);
    assert!(archive.len() < payload.len(), "the payload really is compressed");
    let entries = zip_reader::entries(&archive, None).expect("a zip");
    assert_eq!(entries[0].name, "x/text.md");
    assert_eq!(entries[0].data, payload);
}

#[test]
fn reader_rejects_non_zip() {
    assert_eq!(zip_reader::entries(b"not a zip at all", None), None);
    assert_eq!(zip_reader::entries(&[], None), None);
}

#[test]
fn reader_inflates_only_the_entries_the_caller_asks_for() {
    // The memory-exhaustion defence: an entry the caller doesn't want is listed by name but never
    // inflated (nor allocated), so a hostile pack full of huge deflate-bomb assets costs nothing to
    // open — import reads only text.md.
    let archive = deflated("assets/big.bin", &[0u8; 50_000], None, 8);
    let inflated = zip_reader::entries(&archive, None);
    assert_eq!(inflated.as_ref().map(|e| e[0].data.len()), Some(50_000));

    let skipped = zip_reader::entries(&archive, Some(&|_: &str| false));
    assert_eq!(skipped.as_ref().map(|e| e[0].name.as_str()), Some("assets/big.bin"));
    assert_eq!(skipped.as_ref().map(|e| e[0].data.len()), Some(0));

    // And end to end: a pack whose only inflated entry is text.md imports, while its (skipped)
    // assets are irrelevant to the outcome.
    let big = [0xFFu8; 10_000];
    let pack = stored(&[("Doc.textbundle/text.md", b"# Real\n"), ("Doc.textbundle/assets/x.png", &big)]);
    assert_eq!(text_bundle::text_from_pack(&pack).map(|d| d.text), Some("# Real\n".to_string()));
}

#[test]
fn reader_drops_directory_entries() {
    // A name ending in "/" is a folder marker with no payload and no local header worth chasing —
    // exactly what an empty assets/ in a pack looks like.
    let entries =
        zip_reader::entries(&stored(&[("Doc.textbundle/assets/", b""), ("Doc.textbundle/text.md", b"x")]), None);
    assert_eq!(
        entries.map(|e| e.into_iter().map(|e| e.name).collect::<Vec<_>>()),
        Some(vec!["Doc.textbundle/text.md".to_string()])
    );
}

#[test]
fn reader_refuses_an_oversized_claim_before_allocating() {
    // The central directory's uncompressed size is attacker-controlled: one byte past the 128 MiB
    // cap is refused outright, whatever the stream would really inflate to.
    let too_big = deflated("text.md", b"tiny", Some(MAX_ENTRY_SIZE as u32 + 1), 8);
    assert_eq!(zip_reader::entries(&too_big, None), None);
    assert_eq!(MAX_ENTRY_SIZE, 128 * 1024 * 1024);
}

#[test]
fn reader_caps_a_stored_entry_like_the_mac() {
    // Beyond md.win, which reads a stored entry of any size: macOS refuses a stored payload past
    // the cap too (`guard payload.count <= limit`), and the Mac is the reference. A pack built
    // around 128 MiB + 1 real bytes is refused; exactly 128 MiB opens.
    assert_eq!(zip_reader::entries(&stored_zeros(MAX_ENTRY_SIZE + 1), None), None);
    let at_cap = zip_reader::entries(&stored_zeros(MAX_ENTRY_SIZE), None);
    assert_eq!(at_cap.map(|e| e[0].data.len()), Some(MAX_ENTRY_SIZE));
}

/// A one-entry stored archive of `size` zero bytes, built in place (the writer would hold the
/// payload three times over).
fn stored_zeros(size: usize) -> Vec<u8> {
    let mut archive = deflated_precompressed("text.md", &[], 0, 0, 0);
    let central = archive.split_off(30 + 7);
    archive.resize(archive.len() + size, 0);
    archive[18..22].copy_from_slice(&(size as u32).to_le_bytes());
    archive[22..26].copy_from_slice(&(size as u32).to_le_bytes());
    let central_offset = archive.len() as u32;
    archive.extend_from_slice(&central);
    let central_record = central_offset as usize;
    archive[central_record + 20..central_record + 28].copy_from_slice(&[(size as u32).to_le_bytes(); 2].concat());
    let end = archive.len() - 22;
    archive[end + 16..end + 20].copy_from_slice(&central_offset.to_le_bytes());
    archive
}

#[test]
fn reader_rejects_a_stream_that_disagrees_with_its_declared_size() {
    // The inflate is "exactly the declared size or nothing": a stream that stops short is
    // truncated; one that carries more than it declared is just as corrupt.
    let payload = utf8(&"0123456789".repeat(10));
    assert_eq!(zip_reader::entries(&deflated("text.md", &payload, Some(200), 8), None), None);
    assert_eq!(zip_reader::entries(&deflated("text.md", &payload, Some(50), 8), None), None);
    // A zero claim with a non-empty stream is the one declared size that skips the decoder
    // entirely (Swift's `guard expectedSize > 0 else { return Data() }`).
    let zero = zip_reader::entries(&deflated("text.md", &payload, Some(0), 8), None).expect("a zip");
    assert!(zero[0].data.is_empty());
}

#[test]
fn reader_rejects_unknown_methods_and_corrupt_directories() {
    let payload = utf8("# hi\n");
    // Method 12 (bzip2) is neither STORED nor DEFLATE.
    assert_eq!(zip_reader::entries(&deflated("text.md", &payload, None, 12), None), None);

    // An end record whose central-directory offset points past the buffer.
    let mut broken = stored(&[("text.md", &payload)]);
    let length = broken.len() as u32;
    let at = broken.len() - 22 + 16;
    broken[at..at + 4].copy_from_slice(&length.to_le_bytes());
    assert_eq!(zip_reader::entries(&broken, None), None);

    // A truncated archive has lost its end record.
    let whole = stored(&[("text.md", &payload)]);
    assert_eq!(zip_reader::entries(&whole[..whole.len() - 1], None), None);
}

/// What .NET 10's `ZipArchive` wrote for md.win's `ReaderReadsAnArchiveWrittenByTheBcl` (a
/// directory entry, then `CompressionLevel.Optimal`), captured byte for byte.
const BCL_ARCHIVE: &str = include_str!("export_zip_fixtures/bcl-archive.hex");
/// The same two entries written by Info-ZIP `zip -9` (local and central extra fields of different
/// lengths) and by CPython's `zipfile` into a pipe (data descriptors: zero sizes and CRC in the
/// local headers, the real ones only in the central directory).
const INFO_ZIP_ARCHIVE: &str = include_str!("export_zip_fixtures/info-zip-archive.hex");
const STREAMED_ARCHIVE: &str = include_str!("export_zip_fixtures/streamed-archive.hex");

fn from_hex(hex: &str) -> Vec<u8> {
    let digits: Vec<u8> = hex.bytes().filter(u8::is_ascii_hexdigit).collect();
    digits.chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

#[test]
fn reader_reads_an_archive_written_by_the_bcl() {
    // A pack another tool wrote: DEFLATE, a directory entry, CRC and sizes wherever that writer
    // chose to put them — the central directory is what is trusted.
    let body = "# Deflated\n\nBody text that compresses.\n".repeat(8);
    for (writer, hex) in [("BCL", BCL_ARCHIVE), ("Info-ZIP", INFO_ZIP_ARCHIVE), ("zipfile, streamed", STREAMED_ARCHIVE)]
    {
        let archive = from_hex(hex);
        let entries = zip_reader::entries(&archive, None).unwrap_or_else(|| panic!("{writer}: not read"));
        assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Doc.textbundle/text.md"], "{writer}");
        assert!(utf8_text(&entries[0].data).starts_with("# Deflated\n\nBody"), "{writer}");
        assert_eq!(text_bundle::text_from_pack(&archive).map(|d| d.text), Some(body.clone()), "{writer}");
    }
}

#[test]
fn reader_decodes_names_as_utf8() {
    // Names are UTF-8 on every port regardless of flag bit 11; a Cyrillic bundle folder
    // round-trips through the writer and back.
    let archive = stored(&[("Документ.textbundle/text.md", "# Привет\n".as_bytes())]);
    let entries = zip_reader::entries(&archive, None);
    assert_eq!(entries.as_ref().map(|e| e[0].name.as_str()), Some("Документ.textbundle/text.md"));
    assert_eq!(text_bundle::text_from_pack(&archive).map(|d| d.text), Some("# Привет\n".to_string()));
}

#[test]
fn reader_decodes_invalid_name_bytes_lossily() {
    // Beyond md.win: a name that is not UTF-8 decodes with U+FFFD per maximal invalid subpart,
    // like .NET's Encoding.UTF8 and Swift's String(decoding:as:) — never a panic or a refusal.
    let mut archive = stored(&[("ab.md", b"x")]);
    archive[30] = 0xFF; // the local name, which the reader does not use
    let central_name = archive.len() - 22 - 5;
    archive[central_name] = 0xC3; // a lead byte without its continuation
    let entries = zip_reader::entries(&archive, None).expect("a zip");
    assert_eq!(entries[0].name, "\u{FFFD}b.md");
}

#[test]
fn entry_equality_is_structural() {
    // A plain record would compare the byte[] by reference and every payload assertion above
    // would be meaningless.
    assert_eq!(ZipEntry::new("a", [1u8, 2]), ZipEntry::new("a", [1u8, 2]));
    assert_ne!(ZipEntry::new("a", [1u8, 2]), ZipEntry::new("a", [1u8, 3]));
    assert_ne!(ZipEntry::new("a", [1u8, 2]), ZipEntry::new("A", [1u8, 2]));
    // The debug form names the size, never the payload.
    assert_eq!(format!("{:?}", ZipEntry::new("a", [1u8, 2])), "ZipEntry(\"a\", 2 bytes)");
}
