//! Port of md.win `Documents/TextFileDressing.cs`.

use md_core::document::plain_text_codec::{self, TextEncoding};

use super::line_endings::{self, NewLine};

const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Everything about a text file except its text: the encoding it was read in, whether it carried a
/// UTF-8 BOM, and its line-break convention (§6.3). The session keeps one and re-applies it at
/// every save, so a file that arrived as CRLF Windows-1251 with a BOM leaves the same way —
/// nothing about a document silently changes because it was opened in md.
///
/// The BOM is a decision this design records: Foundation's `.utf8` decode drops a leading EF BB BF
/// and its encode writes none back, so on the Mac a BOM'd file loses its BOM the first time it is
/// saved. Here it is remembered and re-emitted — the file the writer gets back is the file they
/// had. A UTF-16 BOM needs no flag: `plain_text_codec::encode` always writes one for that encoding.
///
/// - `encoding`: what `plain_text_codec::decode` matched; a save that outgrows it upgrades to
///   UTF-8 and the session remembers the upgrade.
/// - `had_utf8_bom`: only meaningful for [`TextEncoding::Utf8`]; a CP1251 file upgraded to UTF-8
///   gains no BOM.
/// - `new_line`: the first line break in the file; none, and a lone CR, mean [`NewLine::Lf`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextFileDressing {
    pub encoding: TextEncoding,
    pub had_utf8_bom: bool,
    pub new_line: NewLine,
}

impl Default for TextFileDressing {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl TextFileDressing {
    /// What a new, untitled document writes: UTF-8, no BOM, LF (the Mac writes `\n`).
    pub const DEFAULT: TextFileDressing = TextFileDressing::new(TextEncoding::Utf8, false, NewLine::Lf);

    /// md.win's positional record constructor.
    pub const fn new(encoding: TextEncoding, had_utf8_bom: bool, new_line: NewLine) -> Self {
        TextFileDressing { encoding, had_utf8_bom, new_line }
    }

    /// Read a file's bytes: LF text plus the dressing to write it back with. `None` when no
    /// encoding matched — unreachable in practice (ISO-8859-1 maps every byte) and kept only
    /// because `plain_text_codec::decode` has the shape.
    pub fn undress(bytes: &[u8]) -> Option<(String, TextFileDressing)> {
        let decoded = plain_text_codec::decode(bytes)?;
        let had_bom = decoded.encoding == TextEncoding::Utf8 && bytes.starts_with(&UTF8_BOM);
        let dressing = TextFileDressing::new(decoded.encoding, had_bom, line_endings::detect(&decoded.text));
        Some((line_endings::normalize(&decoded.text), dressing))
    }

    /// The bytes to write for LF `text`: line breaks restored, encoded in [`Self::encoding`], BOM
    /// re-attached — and the encoding actually written: a CP1251 file that has grown an emoji is
    /// upgraded to UTF-8, and the caller must remember that ([`Self::with_encoding`]) or every
    /// later autosave retries the encoding that cannot hold the text.
    pub fn dress(&self, text: &str) -> (Vec<u8>, TextEncoding) {
        let encoded = plain_text_codec::encode(&line_endings::apply(text, self.new_line), self.encoding);
        let used = encoded.encoding;
        // Both halves matter: the flag means "this UTF-8 file had a BOM", so a file read in
        // another encoding never gains one just because the save upgraded it to UTF-8.
        if !self.had_utf8_bom || self.encoding != TextEncoding::Utf8 || used != TextEncoding::Utf8 {
            return (encoded.data, used);
        }
        let mut with_bom = Vec::with_capacity(encoded.data.len() + UTF8_BOM.len());
        with_bom.extend_from_slice(&UTF8_BOM);
        with_bom.extend_from_slice(&encoded.data);
        (with_bom, used)
    }

    /// The dressing after a save that upgraded the encoding (§6.3: the upgrade is remembered). The
    /// BOM flag does not survive: the only encoding change there is is an upgrade to UTF-8 from
    /// something that had no UTF-8 BOM to begin with.
    pub fn with_encoding(&self, encoding: TextEncoding) -> TextFileDressing {
        if encoding == self.encoding { *self } else { TextFileDressing::new(encoding, false, self.new_line) }
    }
}
