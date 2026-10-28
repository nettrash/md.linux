//! Decoding and encoding for the plain-text files the app edits — shared by the document model,
//! the book's in-place article editor and TextBundle import, so every path reads and round-trips
//! a file's bytes identically (macOS `PlainTextCodec`, md.win `Md.Core.Document.PlainTextCodec`).
//!
//! Every trial decoder refuses instead of substituting U+FFFD: a lenient decode would "succeed"
//! on the wrong encoding and the next autosave would bake the mojibake in.

use crate::text::unicode_tables::CP1251_HIGH;

/// The encodings the app reads and round-trips (Swift `String.Encoding` subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    Utf8,
    /// BOM'd UTF-16, either byte order on read; written little-endian with a BOM.
    Utf16,
    WindowsCp1251,
    IsoLatin1,
}

/// A decoded file: its text and the encoding that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
}

/// Bytes to save and the encoding actually used — not necessarily the one preferred.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedText {
    pub data: Vec<u8>,
    pub encoding: TextEncoding,
}

/// Windows-1251 leaves exactly one byte undefined, 0x98, and Foundation refuses it in both
/// directions; so does this codec, or a file carrying 0x98 would open as CP1251 here and as
/// Latin-1 on the Mac.
const UNDEFINED_CP1251_BYTE: u8 = 0x98;

/// Decode bytes as text, returning the matched encoding. UTF-16 is considered only behind an
/// explicit BOM (FF FE / FE FF): without one a strict UTF-16 decoder happily pairs the bytes of
/// BOM-less CP1251 prose into CJK mojibake. The single-byte trials run most- to least-specific —
/// strict UTF-8, strict Windows-1251 (only 0x98 is undefined), then ISO-8859-1, which maps every
/// byte and so never fails. `None` is therefore unreachable in practice and kept only for the
/// Swift shape.
pub fn decode(data: &[u8]) -> Option<DecodedText> {
    if data.len() >= 2 && ((data[0] == 0xFF && data[1] == 0xFE) || (data[0] == 0xFE && data[1] == 0xFF)) {
        // The byte order comes from the BOM, which is stripped. An odd tail or a lone surrogate
        // is refused (Foundation turns "BOM + one odd byte" into "", and the next autosave would
        // write a two-byte file over it); the single-byte decoders then take the file losslessly.
        if let Some(text) = strict_utf16(&data[2..], data[0] == 0xFF) {
            return Some(DecodedText { text, encoding: TextEncoding::Utf16 });
        }
    }

    if let Ok(text) = std::str::from_utf8(data) {
        // Foundation's .utf8 decode drops a leading EF BB BF, and its encode writes none back —
        // so a BOM'd UTF-8 file loses its BOM on save.
        let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
        return Some(DecodedText { text: text.to_string(), encoding: TextEncoding::Utf8 });
    }
    if !data.contains(&UNDEFINED_CP1251_BYTE) {
        let text = data.iter().map(|&b| cp1251_char(b)).collect::<Option<String>>();
        if let Some(text) = text {
            return Some(DecodedText { text, encoding: TextEncoding::WindowsCp1251 });
        }
    }
    let text = data.iter().map(|&b| char::from(b)).collect();
    Some(DecodedText { text, encoding: TextEncoding::IsoLatin1 })
}

/// Encode for saving in the encoding the file was read in, so a save round-trips instead of
/// silently rewriting the file as UTF-8. If the text no longer fits (an emoji typed into a
/// Windows-1251 file) it is upgraded to UTF-8 — and the encoding actually used is reported, so
/// the caller remembers the upgrade instead of failing the same way on every autosave.
pub fn encode(text: &str, preferred: TextEncoding) -> EncodedText {
    match preferred {
        TextEncoding::Utf8 => utf8(text),
        TextEncoding::Utf16 => {
            // Foundation writes BOM + host order, little-endian on every Mac (FF FE, present even
            // for an empty string).
            let mut data = vec![0xFF, 0xFE];
            for unit in text.encode_utf16() {
                data.extend_from_slice(&unit.to_le_bytes());
            }
            EncodedText { data, encoding: TextEncoding::Utf16 }
        }
        TextEncoding::WindowsCp1251 => {
            let bytes = text.chars().map(cp1251_byte).collect::<Option<Vec<u8>>>();
            match bytes {
                Some(data) => EncodedText { data, encoding: TextEncoding::WindowsCp1251 },
                None => utf8(text),
            }
        }
        TextEncoding::IsoLatin1 => {
            let bytes = text.chars().map(|c| u8::try_from(c as u32).ok()).collect::<Option<Vec<u8>>>();
            match bytes {
                Some(data) => EncodedText { data, encoding: TextEncoding::IsoLatin1 },
                None => utf8(text),
            }
        }
    }
}

fn utf8(text: &str) -> EncodedText {
    EncodedText { data: text.as_bytes().to_vec(), encoding: TextEncoding::Utf8 }
}

fn strict_utf16(body: &[u8], little_endian: bool) -> Option<String> {
    if !body.len().is_multiple_of(2) {
        return None;
    }
    let units = body
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| if little_endian { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) });
    char::decode_utf16(units).collect::<Result<String, _>>().ok()
}

fn cp1251_char(byte: u8) -> Option<char> {
    if byte < 0x80 {
        return Some(char::from(byte));
    }
    match CP1251_HIGH[(byte - 0x80) as usize] {
        0 => None,
        unit => char::from_u32(u32::from(unit)),
    }
}

fn cp1251_byte(c: char) -> Option<u8> {
    let cp = c as u32;
    if cp < 0x80 {
        return Some(cp as u8);
    }
    CP1251_HIGH.iter().position(|&unit| unit != 0 && u32::from(unit) == cp).map(|index| 0x80 + index as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(data: &[u8]) -> DecodedText {
        decode(data).expect("the codec always decodes")
    }

    #[test]
    fn decodes_utf8() {
        let d = decoded("Привет, md".as_bytes());
        assert_eq!(d.text, "Привет, md");
        assert_eq!(d.encoding, TextEncoding::Utf8);
    }

    #[test]
    fn does_not_mistake_bomless_cp1251_for_utf16() {
        let bytes = encode("Привет, мир", TextEncoding::WindowsCp1251).data;
        let d = decoded(&bytes);
        assert_eq!(d.encoding, TextEncoding::WindowsCp1251);
        assert_eq!(d.text, "Привет, мир");
    }

    #[test]
    fn round_trips_bomed_utf16() {
        let e = encode("a😀ж", TextEncoding::Utf16);
        assert_eq!(&e.data[..2], &[0xFF, 0xFE]);
        let d = decoded(&e.data);
        assert_eq!((d.text.as_str(), d.encoding), ("a😀ж", TextEncoding::Utf16));
        assert_eq!(encode("", TextEncoding::Utf16).data, vec![0xFF, 0xFE]);
    }

    #[test]
    fn honours_a_big_endian_bom_and_strips_it() {
        let d = decoded(&[0xFE, 0xFF, 0x04, 0x36, 0x00, 0x61]);
        assert_eq!((d.text.as_str(), d.encoding), ("жa", TextEncoding::Utf16));
    }

    #[test]
    fn upgrades_to_utf8_and_says_so() {
        let e = encode("Привет 😀", TextEncoding::WindowsCp1251);
        assert_eq!(e.encoding, TextEncoding::Utf8);
        assert_eq!(e.data, "Привет 😀".as_bytes());
        let e = encode("ж", TextEncoding::IsoLatin1);
        assert_eq!(e.encoding, TextEncoding::Utf8);
        let e = encode("\u{0098}", TextEncoding::WindowsCp1251);
        assert_eq!(e.encoding, TextEncoding::Utf8);
    }

    #[test]
    fn falls_back_losslessly_for_arbitrary_bytes() {
        let bytes: Vec<u8> = (0..=255u8).collect();
        let d = decoded(&bytes);
        assert_eq!(d.encoding, TextEncoding::IsoLatin1);
        assert_eq!(encode(&d.text, TextEncoding::IsoLatin1).data, bytes);
        let all_but_98: Vec<u8> = (0..=255u8).filter(|&b| b != 0x98).collect();
        let d = decoded(&all_but_98);
        assert_eq!(d.encoding, TextEncoding::WindowsCp1251);
        assert_eq!(encode(&d.text, TextEncoding::WindowsCp1251).data, all_but_98);
    }

    #[test]
    fn strips_a_utf8_bom_on_decode_and_writes_none_back() {
        let d = decoded(&[0xEF, 0xBB, 0xBF, b'h', b'i']);
        assert_eq!((d.text.as_str(), d.encoding), ("hi", TextEncoding::Utf8));
        assert_eq!(encode(&d.text, TextEncoding::Utf8).data, b"hi");
    }

    #[test]
    fn rejects_invalid_utf8_strictly_instead_of_substituting() {
        let d = decoded(&[b'a', 0xC3, b'b']);
        assert_ne!(d.encoding, TextEncoding::Utf8);
        assert!(!d.text.contains('\u{FFFD}'));
    }

    #[test]
    fn an_odd_tail_after_a_utf16_bom_is_not_swallowed() {
        let d = decoded(&[0xFF, 0xFE, 0x41]);
        assert_ne!(d.encoding, TextEncoding::Utf16);
        assert_eq!(d.text.chars().count(), 3);
        let lone_surrogate = decoded(&[0xFF, 0xFE, 0x00, 0xD8]);
        assert_ne!(lone_surrogate.encoding, TextEncoding::Utf16);
    }
}
