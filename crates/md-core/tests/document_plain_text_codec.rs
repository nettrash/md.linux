//! Port of md.win `PlainTextCodecTests`: the plain-text codec shared by documents, the book editor
//! and TextBundle import (macOS "Plain-text codec" tests plus the Kotlin TextCodecTest cases and the
//! measured Foundation behaviours the port pins).

mod book_support;

use book_support::cp1251;
use md_core::document::plain_text_codec::{self as codec, DecodedText, TextEncoding};

fn utf8(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn utf8_text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("strict UTF-8")
}

#[test]
fn decodes_utf8() {
    let decoded = codec::decode(&utf8("# Привет\n")).unwrap();
    assert_eq!("# Привет\n", decoded.text);
    assert_eq!(TextEncoding::Utf8, decoded.encoding);
    // The empty file is UTF-8 too.
    assert_eq!(Some(DecodedText { text: String::new(), encoding: TextEncoding::Utf8 }), codec::decode(&[]));
}

#[test]
fn does_not_mistake_bomless_cp1251_for_utf16() {
    // Cyrillic prose in Windows-1251 — even-length and BOM-less, the shape a naive UTF-16 trial
    // happily (and wrongly) accepts as CJK mojibake. It must decode as CP1251 and round-trip
    // byte-exactly.
    let original = "Привет, мир!";
    let data = cp1251(original);
    assert_eq!(12, data.len());
    assert_eq!(vec![0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2, 0x2C, 0x20, 0xEC, 0xE8, 0xF0, 0x21], data);
    let decoded = codec::decode(&data).unwrap();
    assert_eq!(original, decoded.text);
    assert_eq!(TextEncoding::WindowsCp1251, decoded.encoding);
    assert_eq!(data, codec::encode(&decoded.text, decoded.encoding).data);
}

#[test]
fn round_trips_bomed_utf16() {
    let original = "# Chapter\n";
    // Foundation's data(using: .utf16) writes a BOM and host (little-endian) order.
    let mut data = vec![0xFF, 0xFE];
    data.extend(original.encode_utf16().flat_map(u16::to_le_bytes));
    let decoded = codec::decode(&data).unwrap();
    assert_eq!(original, decoded.text);
    assert_eq!(TextEncoding::Utf16, decoded.encoding);
    // Re-encoding restores a BOM'd UTF-16 file, not a silently rewritten one.
    let encoded = codec::encode(original, TextEncoding::Utf16);
    assert_eq!(TextEncoding::Utf16, encoded.encoding);
    assert_eq!(data, encoded.data);
    assert_eq!(original, codec::decode(&encoded.data).unwrap().text);
    // Measured: Foundation writes FF FE even for the empty string.
    assert_eq!(vec![0xFF, 0xFE], codec::encode("", TextEncoding::Utf16).data);
}

#[test]
fn honours_a_big_endian_bom_and_strips_it() {
    let decoded = codec::decode(&[0xFE, 0xFF, 0x00, 0x41, 0x04, 0x1F]).unwrap();
    assert_eq!("AП", decoded.text);
    assert_eq!(TextEncoding::Utf16, decoded.encoding);
    // Written back little-endian: the byte order is the codec's, the BOM says which.
    assert_eq!(vec![0xFF, 0xFE, 0x41, 0x00, 0x1F, 0x04], codec::encode("AП", TextEncoding::Utf16).data);
}

#[test]
fn upgrades_to_utf8_and_says_so() {
    // An emoji cannot live in CP1251: the encode must fall back to UTF-8 *and report it*, so an
    // autosaving caller updates its remembered encoding instead of failing the same way every save.
    let encoded = codec::encode("Привет 🙂", TextEncoding::WindowsCp1251);
    assert_eq!(TextEncoding::Utf8, encoded.encoding);
    assert_eq!("Привет 🙂", utf8_text(&encoded.data));
    // Latin-1 upgrades the same way, and never best-fits a Cyrillic letter.
    let latin = codec::encode("Привет", TextEncoding::IsoLatin1);
    assert_eq!(TextEncoding::Utf8, latin.encoding);
    assert_eq!(utf8("Привет"), latin.data);
}

#[test]
fn legacy_encoding_round_trips_through_an_edit() {
    // The book editor's session test at codec level: a Windows-1251 article is edited and flushed,
    // and the file keeps its own encoding.
    let original = "Привет, мир!";
    let decoded = codec::decode(&cp1251(original)).unwrap();
    assert_eq!(original, decoded.text);
    let edited = format!("{} Ещё.", decoded.text);
    let encoded = codec::encode(&edited, decoded.encoding);
    assert_eq!(TextEncoding::WindowsCp1251, encoded.encoding);
    assert_eq!(cp1251(&edited), encoded.data);
}

#[test]
fn falls_back_losslessly_for_arbitrary_bytes() {
    // Every byte is defined in Windows-1251 except 0x98, so binary-ish input decodes as CP1251 one
    // char per byte…
    let cp = codec::decode(&[0x41, 0xFF, 0x42]).unwrap();
    assert_eq!("AяB", cp.text);
    assert_eq!(TextEncoding::WindowsCp1251, cp.encoding);
    // …and 0x98 — the one byte Foundation's CP1251 rejects — falls through to ISO-8859-1, which maps
    // everything and round-trips.
    let latin = codec::decode(&[0x41, 0x98, 0x42]).unwrap();
    assert_eq!("A\u{0098}B", latin.text);
    assert_eq!(TextEncoding::IsoLatin1, latin.encoding);
    assert_eq!(vec![0x41, 0x98, 0x42], codec::encode(&latin.text, latin.encoding).data);
    // The mirror image: U+0098 does not fit CP1251 on the Mac, so it upgrades here too.
    let upgraded = codec::encode("A\u{0098}B", TextEncoding::WindowsCp1251);
    assert_eq!(TextEncoding::Utf8, upgraded.encoding);
    assert_eq!(vec![0x41, 0xC2, 0x98, 0x42], upgraded.data);
    // Every other C1 byte is a defined CP1251 character.
    assert_eq!(TextEncoding::WindowsCp1251, codec::decode(&[0x80, 0x99, 0x9F]).unwrap().encoding);
}

#[test]
fn strips_a_utf8_bom_on_decode_and_writes_none_back() {
    // Measured: Foundation's String(data:encoding:.utf8) drops a leading EF BB BF and Data(text.utf8)
    // never writes one, so a BOM'd UTF-8 file loses its BOM on save.
    let decoded = codec::decode(&[0xEF, 0xBB, 0xBF, 0x41]).unwrap();
    assert_eq!("A", decoded.text);
    assert_eq!(TextEncoding::Utf8, decoded.encoding);
    assert_eq!(vec![0x41], codec::encode("A", TextEncoding::Utf8).data);
}

#[test]
fn rejects_invalid_utf8_strictly_instead_of_substituting() {
    // A lenient decode would "succeed" with U+FFFD and the next autosave would bake it in; the trial
    // must fail and hand the bytes to the single-byte decoders.
    let decoded = codec::decode(&[0xD0, 0x9F, 0xE0]).unwrap(); // "П" then a truncated sequence
    assert_eq!(TextEncoding::WindowsCp1251, decoded.encoding);
    assert!(!decoded.text.contains('\u{FFFD}'));
    assert_eq!(vec![0xD0, 0x9F, 0xE0], codec::encode(&decoded.text, decoded.encoding).data);
}

#[test]
fn an_odd_tail_after_a_utf16_bom_is_not_swallowed() {
    // Divergence, pinned: Foundation turns FF FE 41 into "" (and would then write FF FE over the
    // file); here the UTF-16 trial fails and the bytes survive as CP1251.
    let decoded = codec::decode(&[0xFF, 0xFE, 0x41]).unwrap();
    assert_eq!(TextEncoding::WindowsCp1251, decoded.encoding);
    assert_eq!("яюA", decoded.text);
    assert_eq!(vec![0xFF, 0xFE, 0x41], codec::encode(&decoded.text, decoded.encoding).data);
}

/// md.win feeds a lone surrogate (`"a" + (char)0xD800 + "b"`) to prove the UTF-8 path never throws.
/// A Rust `&str` cannot hold a lone surrogate — the case cannot arise — so the port pins what it
/// becomes the moment it reaches a Rust string (U+FFFD, as .NET's encoder writes it) and that the
/// UTF-8 path, and the CP1251 path's upgrade to it, take that text without failing.
#[test]
fn encoding_utf8_never_fails() {
    let lossy = String::from_utf16_lossy(&[0x61, 0xD800, 0x62]);
    let encoded = codec::encode(&lossy, TextEncoding::Utf8);
    assert_eq!(TextEncoding::Utf8, encoded.encoding);
    assert_eq!(vec![0x61, 0xEF, 0xBF, 0xBD, 0x62], encoded.data);
    assert_eq!(TextEncoding::Utf8, codec::encode(&lossy, TextEncoding::WindowsCp1251).encoding);
}
