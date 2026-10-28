//! Port of md.win `Documents/LineEndings.cs`.

/// The line break a file uses on disk. The model in memory is always LF (§3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NewLine {
    #[default]
    Lf,
    CrLf,
}

pub const LF: &str = "\n";
pub const CRLF: &str = "\r\n";

/// The sequence a [`NewLine`] writes.
pub fn text(new_line: NewLine) -> &'static str {
    match new_line {
        NewLine::CrLf => CRLF,
        NewLine::Lf => LF,
    }
}

/// The convention of `text`, remembered at load and re-applied at save (§3.2): the first break
/// decides, and only CR+LF is CRLF — a lone CR, a lone LF and a file with no break at all are all
/// LF, as a new document is (the Mac writes `\n`). A lone CR is read as a break but never written
/// back — the model has no third form, and a classic-Mac file becomes an LF file the first time it
/// is saved (accepted, §3.2).
pub fn detect(text: &str) -> NewLine {
    // Byte scan: CR and LF are ASCII, so they can never be part of a multi-byte sequence.
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            return NewLine::Lf;
        }
        if b == b'\r' {
            return if bytes.get(i + 1) == Some(&b'\n') { NewLine::CrLf } else { NewLine::Lf };
        }
    }
    NewLine::Lf
}

/// Any mixture of CRLF / CR / LF to the model's LF. Order matters: CRLF first, or each pair
/// becomes two breaks.
pub fn normalize(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace(CRLF, LF).replace('\r', LF)
}

/// The file's convention applied to LF text on the way out. Input must already be LF
/// ([`normalize`]).
pub fn apply(lf_text: &str, new_line: NewLine) -> String {
    match new_line {
        NewLine::CrLf => lf_text.replace(LF, CRLF),
        NewLine::Lf => lf_text.to_string(),
    }
}
