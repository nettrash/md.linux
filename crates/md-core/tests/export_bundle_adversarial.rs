//! Adversarial inputs for the TextBundle / zip / codec module. Every expected value was derived
//! from the macOS behaviour (NSRegularExpression ranges, Foundation's String(data:encoding:) and
//! data(using:), EPUBZipWriter / ZipReader), measured with a Swift probe for md.win — never from
//! an implementation's output — so a passing test is parity, not self-agreement. Port of md.win
//! `BundleAdversarialTests.cs`. md.win's UTF-16 ranges are byte ranges here; the splices land in
//! the same places.

mod export_zip_fixtures;

use std::collections::HashMap;

use export_zip_fixtures::*;
use md_core::document::plain_text_codec::{self, DecodedText, EncodedText, TextEncoding};
use md_core::export::ZipWriter;
use md_core::export::text_bundle;
use md_core::export::zip_reader::{self, MAX_ENTRY_SIZE};

const MARK: &str = "\u{0301}"; // COMBINING ACUTE ACCENT
const FRAK: &str = "\u{1D504}"; // 𝔄, two UTF-16 units
const FRAK_B: &str = "\u{1D505}"; // 𝔅
const GRIN: &str = "\u{1F600}"; // 😀

fn one(_: &str) -> Option<Vec<u8>> {
    Some(vec![1])
}

fn decoded(text: &str, encoding: TextEncoding) -> Option<DecodedText> {
    Some(DecodedText { text: text.to_string(), encoding })
}

fn encoded(data: &[u8], encoding: TextEncoding) -> EncodedText {
    EncodedText { data: data.to_vec(), encoding }
}

#[test]
fn combining_marks_on_delimiters_are_their_own_units() {
    // ICU (measured): `![a](pic.png)` + U+0301 matches with the URL at range {5, 7}; the mark
    // after ')' is outside the match and survives the splice untouched.
    let after = text_bundle::export_rewriting(&format!("![a](pic.png){MARK}"), one);
    assert_eq!(after.text, format!("![a](assets/pic.png){MARK}"));
    assert_eq!(after.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["pic.png"]);

    // A mark right after '[' is alt text ({6, 5} for the URL); the ref still rewrites.
    assert_eq!(
        text_bundle::export_rewriting(&format!("![{MARK}a](x.png)"), one).text,
        format!("![{MARK}a](assets/x.png)")
    );

    // A mark right after '(' is the FIRST unit of the destination (ICU range {5, 8}): the loader
    // is asked for the marked path and the asset is named by it verbatim.
    let mut asked = None;
    let lead = text_bundle::export_rewriting(&format!("![a]({MARK}pic.png)"), |path| {
        asked = Some(path.to_string());
        Some(vec![1])
    });
    assert_eq!(asked, Some(format!("{MARK}pic.png")));
    assert_eq!(lead.text, format!("![a](assets/{MARK}pic.png)"));
    assert_eq!(lead.assets[0].name, format!("{MARK}pic.png"));

    // Classification is scalar-exact around the delimiters (Swift ScalarText): ':' + mark + "//"
    // is not a scheme; "://" anywhere is; a bare ':' is local.
    assert!(text_bundle::is_local_relative_reference(&format!("weird:{MARK}//name.png")));
    assert!(!text_bundle::is_local_relative_reference("a/b://c.png"));
    assert!(!text_bundle::is_local_relative_reference("://x.png"));
    assert!(text_bundle::is_local_relative_reference("x:y.png"));
    assert!(text_bundle::is_local_relative_reference("./photo.png"));
    assert!(text_bundle::is_local_relative_reference("../photo.png"));
    assert!(text_bundle::is_local_relative_reference("mailto"));
    assert!(!text_bundle::is_local_relative_reference("data:"));
    // Swift's lowercased() maps the Kelvin sign to 'k' but no letter of these schemes is a k;
    // nothing non-ASCII can ever spell them, so U+212A in a scheme stays local.
    assert!(text_bundle::is_local_relative_reference("dat\u{212A}:x.png"));

    // Zip names are bytes: a decomposed name is not normalised on the way through.
    let name = format!("Docu{MARK}ment.textbundle/text.md");
    let entries = zip_reader::entries(&stored(&[(&name, b"x")]), None);
    assert_eq!(entries.map(|e| e[0].name.clone()), Some(name.clone()));
    assert_eq!(text_bundle::text_from_pack(&stored(&[(&name, b"x")])).map(|d| d.text), Some("x".to_string()));
}

#[test]
fn supplementary_plane_characters_keep_utf16_offsets_and_scalar_splits() {
    // ICU (measured): in "𝔄𝔄 ![😀](𝔄/𝔅.png) 𝔅" the URL is NSRange {11, 9} — UTF-16 units — and
    // the splice must land on them so the astral text on both sides is untouched.
    let source = format!("{FRAK}{FRAK} ![{GRIN}]({FRAK}/{FRAK_B}.png) {FRAK_B}");
    let result = text_bundle::export_rewriting(&source, one);
    assert_eq!(result.text, format!("{FRAK}{FRAK} ![{GRIN}](assets/{FRAK_B}.png) {FRAK_B}"));
    assert_eq!(result.assets[0].name, format!("{FRAK_B}.png"));

    // disambiguated() splits at the last '.' with a SCALAR index > 0: 𝔅.png has the dot at
    // scalar 1 (UTF-16 unit 2, byte 4), so the stem is 𝔅 and the twin is 𝔅-2.png.
    let twins = text_bundle::export_rewriting(&format!("![a](x/{FRAK_B}.png) ![b](y/{FRAK_B}.png)"), one);
    assert_eq!(
        twins.assets.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
        [format!("{FRAK_B}.png"), format!("{FRAK_B}-2.png")]
    );
    // An extensionless astral name (no dot at all) gets the counter appended whole.
    let extless = text_bundle::export_rewriting(&format!("![a](x/{GRIN}) ![b](y/{GRIN})"), one);
    assert_eq!(
        extless.assets.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
        [GRIN.to_string(), format!("{GRIN}-2")]
    );

    // Codec: a surrogate pair behind a UTF-16 BOM decodes to one scalar and re-encodes to the same
    // six bytes (Foundation: BOM + host LE order); an astral scalar cannot live in CP1251 or
    // Latin-1, so both upgrade to UTF-8 and say so.
    let laugh = "\u{1F602}";
    let utf16 = [0xFF, 0xFE, 0x3D, 0xD8, 0x02, 0xDE];
    assert_eq!(plain_text_codec::decode(&utf16), decoded(laugh, TextEncoding::Utf16));
    assert_eq!(plain_text_codec::encode(laugh, TextEncoding::Utf16).data, utf16);
    assert_eq!(plain_text_codec::decode(&[0xF0, 0x9F, 0x98, 0x82]), decoded(laugh, TextEncoding::Utf8));
    assert_eq!(
        plain_text_codec::encode(laugh, TextEncoding::WindowsCp1251),
        encoded(&[0xF0, 0x9F, 0x98, 0x82], TextEncoding::Utf8)
    );
    assert_eq!(plain_text_codec::encode(laugh, TextEncoding::IsoLatin1).encoding, TextEncoding::Utf8);

    // Zip names are UTF-8 on the wire, so an astral folder name round-trips.
    let pack = text_bundle::bundle_wrapper(&format!("# {GRIN}\n"), vec![]).to_text_pack(FRAK).unwrap();
    let entries = zip_reader::entries(&pack, None).expect("a zip");
    assert_eq!(entries[1].name, format!("{FRAK}.textbundle/text.md"));
    assert_eq!(text_bundle::text_from_pack(&pack).map(|d| d.text), Some(format!("# {GRIN}\n")));
}

#[test]
fn crlf_is_preserved_everywhere_and_separates_a_title() {
    // CR and LF are both ICU whitespace: a destination stops at either, and the rewrite keeps
    // every CRLF byte-exact between the edits.
    let source = "![a](p.png)\r\n![b](q.png \"t\")\r\n";
    assert_eq!(text_bundle::export_rewriting(source, one).text, "![a](assets/p.png)\r\n![b](assets/q.png \"t\")\r\n");
    // A CRLF (and, measured, a U+2028) between destination and title is a `\s+` run.
    assert_eq!(text_bundle::export_rewriting("![a](p.png\r\n\"t\")", one).text, "![a](assets/p.png\r\n\"t\")");
    assert_eq!(text_bundle::export_rewriting("![a](p.png\u{2028}\"t\")", one).text, "![a](assets/p.png\u{2028}\"t\")");
    // A destination cannot span a line: the ref stays exactly as written, no asset.
    let broken = text_bundle::export_rewriting("![a](p\r\n.png)", one);
    assert_eq!(broken.text, "![a](p\r\n.png)");
    assert!(broken.assets.is_empty());

    // Foundation never normalises line endings (measured "U+0041 U+000D U+000A U+0042"), in any of
    // the four encodings, on either side of the round trip.
    assert_eq!(plain_text_codec::decode(&[0x41, 0x0D, 0x0A, 0x42]).map(|d| d.text), Some("A\r\nB".to_string()));
    assert_eq!(plain_text_codec::encode("A\r\nB", TextEncoding::Utf8).data, [0x41, 0x0D, 0x0A, 0x42]);
    let cyr = cp1251("Привет\r\nмир");
    let cp = plain_text_codec::decode(&cyr);
    assert_eq!(cp, decoded("Привет\r\nмир", TextEncoding::WindowsCp1251));
    let cp = cp.unwrap();
    assert_eq!(plain_text_codec::encode(&cp.text, cp.encoding).data, cyr);
    let utf16 = [0xFF, 0xFE, 0x41, 0x00, 0x0D, 0x00, 0x0A, 0x00, 0x42, 0x00];
    assert_eq!(plain_text_codec::decode(&utf16), decoded("A\r\nB", TextEncoding::Utf16));
    assert_eq!(plain_text_codec::encode("A\r\nB", TextEncoding::Utf16).data, utf16);
    assert_eq!(plain_text_codec::encode("A\r\nB", TextEncoding::IsoLatin1).data, [0x41, 0x0D, 0x0A, 0x42]);
    // text.md carries the author's line endings verbatim.
    assert_eq!(text_bundle::bundle_wrapper("A\r\nB", vec![]).text_bytes(), [0x41, 0x0D, 0x0A, 0x42]);
}

#[test]
fn empty_inputs_have_the_swift_shape() {
    // Nothing to rewrite: the source comes back as is and the loader is never asked.
    let mut calls = 0;
    let nothing = text_bundle::export_rewriting("", |_| {
        calls += 1;
        Some(vec![1])
    });
    assert_eq!(nothing.text, "");
    assert!(nothing.assets.is_empty());
    assert_eq!(calls, 0);

    // EPUBZipWriter().finish() with no entries is exactly the 22-byte end record.
    let empty = ZipWriter::new().finish().unwrap();
    assert_eq!(empty.len(), 22);
    assert_eq!(empty[..4], [0x50, 0x4B, 0x05, 0x06]);
    assert!(empty[4..].iter().all(|&b| b == 0));
    // ZipReader.entries finds that record at index 0 and returns [] — not None.
    assert_eq!(zip_reader::entries(&empty, None), Some(vec![]));
    // …while text_from_pack of it is None (no text file), and a 21-byte tail is not a zip.
    assert_eq!(text_bundle::text_from_pack(&empty), None);
    assert_eq!(zip_reader::entries(&empty[..21], None), None);
    assert!(!text_bundle::looks_like_pack("", &[]));

    // An empty document exports as an empty text.md — and, faithfully to Swift's `!data.isEmpty`
    // guard, that pack does NOT import (None), while the same bundle as a folder imports as ""
    // (FileWrapper hands back empty Data, the codec decodes it).
    let wrapper = text_bundle::bundle_wrapper("", vec![]);
    assert!(wrapper.text_bytes().is_empty());
    assert_eq!(wrapper.pack_entries("Doc").len(), 3);
    assert_eq!(text_bundle::text_from_pack(&wrapper.to_text_pack("Doc").unwrap()), None);
    let folder = HashMap::from([("text.md".to_string(), Vec::new())]);
    assert_eq!(text_bundle::text_from_bundle(&folder), decoded("", TextEncoding::Utf8));

    // Codec edges measured on Foundation: FF FE alone is "" as UTF-16; EF BB BF alone is "" as
    // UTF-8; the empty string encodes to nothing in every single-byte encoding and to a bare BOM
    // in UTF-16, each reporting the preferred encoding unchanged.
    assert_eq!(plain_text_codec::decode(&[0xFF, 0xFE]), decoded("", TextEncoding::Utf16));
    assert_eq!(plain_text_codec::decode(&[0xFE, 0xFF]), decoded("", TextEncoding::Utf16));
    assert_eq!(plain_text_codec::decode(&[0xEF, 0xBB, 0xBF]), decoded("", TextEncoding::Utf8));
    assert_eq!(plain_text_codec::encode("", TextEncoding::Utf8), encoded(&[], TextEncoding::Utf8));
    assert_eq!(plain_text_codec::encode("", TextEncoding::WindowsCp1251), encoded(&[], TextEncoding::WindowsCp1251));
    assert_eq!(plain_text_codec::encode("", TextEncoding::IsoLatin1), encoded(&[], TextEncoding::IsoLatin1));
    assert_eq!(plain_text_codec::encode("", TextEncoding::Utf16), encoded(&[0xFF, 0xFE], TextEncoding::Utf16));
    // (Those comparisons mean something only because EncodedText compares its bytes
    // structurally, like ZipEntry and Asset.)
    assert_eq!(encoded(&[1], TextEncoding::Utf8), encoded(&[1], TextEncoding::Utf8));
    assert_ne!(encoded(&[1], TextEncoding::Utf8), encoded(&[1], TextEncoding::IsoLatin1));
    assert_ne!(encoded(&[1], TextEncoding::Utf8), encoded(&[2], TextEncoding::Utf8));
    // Foundation strips exactly ONE UTF-8 BOM (measured: EF BB BF EF BB BF 41 → U+FEFF A).
    assert_eq!(
        plain_text_codec::decode(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, 0x41]).map(|d| d.text),
        Some("\u{FEFF}A".to_string())
    );
    // …and keeps a second UTF-16 BOM in the body (measured: A U+FEFF B).
    assert_eq!(
        plain_text_codec::decode(&[0xFF, 0xFE, 0x41, 0x00, 0xFF, 0xFE, 0x42, 0x00]).map(|d| d.text),
        Some("A\u{FEFF}B".to_string())
    );
}

#[test]
fn pathological_archives_are_read_or_refused_like_the_swift() {
    let payload = utf8("# comment\n");

    // 1. A zip comment after the end record — up to 65535 bytes, the Swift scans back exactly
    //    that far — must not hide the record; one byte further and it does.
    for comment_length in [1, 0xFFFF] {
        let commented = with_comment(&stored(&[("Doc.textbundle/text.md", &payload)]), comment_length);
        assert_eq!(text_bundle::text_from_pack(&commented).map(|d| d.text), Some("# comment\n".to_string()));
    }
    assert_eq!(
        zip_reader::entries(&with_comment(&stored(&[("Doc.textbundle/text.md", &payload)]), 0x10000), None),
        None
    );

    // 2. The LOCAL header carries an extra field the central record lacks: the data offset must
    //    come from the local lengths (Swift reads u16 at local+26/+28).
    let with_extra = with_local_extra_field(&stored(&[("text.md", &payload)]), &[0x55, 0x54, 0x00, 0x00]);
    let entries = zip_reader::entries(&with_extra, None).expect("a zip");
    assert_eq!(entries[0].data, payload);
    // A foreign reader agrees the archive is well formed (md.win asks the BCL).
    let foreign = assert_foreign_readers_accept(&with_extra, &["text.md"]);
    assert_eq!(utf8_text(&foreign[0].data), "# comment\n");

    // 3. The central directory's count is authoritative: fewer records than entries reads that
    //    many; more than exist runs off the directory and is refused.
    let two = stored(&[("a.txt", b"a"), ("b.txt", b"b")]);
    let mut one = two.clone();
    let n = one.len();
    one[n - 12] = 1;
    one[n - 14] = 1; // both u16 counts → 1
    assert_eq!(
        zip_reader::entries(&one, None).map(|e| e.into_iter().map(|e| e.name).collect::<Vec<_>>()),
        Some(vec!["a.txt".to_string()])
    );
    let mut three = two.clone();
    three[n - 12] = 3;
    three[n - 14] = 3;
    assert_eq!(zip_reader::entries(&three, None), None);

    // 4. A hostile size claim: the full u32 (4 GiB) is refused without an allocation attempt or a
    //    panic, whatever the stream holds.
    assert_eq!(zip_reader::entries(&deflated("text.md", &payload, Some(0xFFFF_FFFF), 8), None), None);
    assert_eq!(text_bundle::text_from_pack(&deflated("Doc.textbundle/text.md", &payload, Some(0xFFFF_FFFF), 8)), None);

    // 5. The cap is a boundary, not a fig leaf: a stream that GENUINELY inflates to 128 MiB + 1
    //    zero bytes, declared honestly, is refused (only the cap can refuse it — the stream itself
    //    is consistent), while exactly 128 MiB is accepted.
    let over_compressed = deflate_zeros(MAX_ENTRY_SIZE as u64 + 1);
    let over = deflated_precompressed("text.md", &over_compressed, MAX_ENTRY_SIZE as u32 + 1, 0, 8);
    assert_eq!(zip_reader::entries(&over, None), None);
    let at_cap_compressed = deflate_zeros(MAX_ENTRY_SIZE as u64);
    let at_cap =
        zip_reader::entries(&deflated_precompressed("text.md", &at_cap_compressed, MAX_ENTRY_SIZE as u32, 0, 8), None);
    assert_eq!(at_cap.as_ref().map(|e| e[0].data.len()), Some(MAX_ENTRY_SIZE));
    assert!(at_cap.unwrap()[0].data.iter().all(|&b| b == 0));

    // 6. A name that runs past the end of the buffer, and a local header whose signature is wrong,
    //    are each refused rather than guessed.
    let mut long_name = stored(&[("text.md", &payload)]);
    let at = long_name.len() - 22 - 7 - 46 + 28;
    long_name[at] = 0xFF; // central nameLength 7 → 255, past the buffer
    assert_eq!(zip_reader::entries(&long_name, None), None);
    let mut bad_local = stored(&[("text.md", &payload)]);
    bad_local[0] = 0x51;
    assert_eq!(zip_reader::entries(&bad_local, None), None);
}

// MARK: helpers

/// Append a zip comment of `length` bytes and record its length in the end record.
fn with_comment(archive: &[u8], length: usize) -> Vec<u8> {
    let mut output = archive.to_vec();
    let n = output.len();
    output[n - 2] = (length & 0xFF) as u8;
    output[n - 1] = ((length >> 8) & 0xFF) as u8;
    output.resize(n + length, b'#');
    output
}

/// Insert an extra field into the LOCAL header of a one-entry stored archive (leaving the central
/// record's extra length 0) and shift the end record's directory offset.
fn with_local_extra_field(archive: &[u8], extra: &[u8]) -> Vec<u8> {
    let name_length = usize::from(u16::from_le_bytes([archive[26], archive[27]]));
    let insert_at = 30 + name_length;
    let mut output = archive[..insert_at].to_vec();
    output.extend_from_slice(extra);
    output.extend_from_slice(&archive[insert_at..]);
    output[28..30].copy_from_slice(&(extra.len() as u16).to_le_bytes());
    let end = output.len() - 22;
    let central_offset = u32::from_le_bytes(output[end + 16..end + 20].try_into().unwrap()) + extra.len() as u32;
    output[end + 16..end + 20].copy_from_slice(&central_offset.to_le_bytes());
    output
}
