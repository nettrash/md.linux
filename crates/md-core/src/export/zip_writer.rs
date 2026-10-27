//! The hand-rolled, STORED-only zip container the EPUB and TextPack exports pack into (macOS
//! `EPUBZipWriter`, md.win `Md.Core.Export.ZipWriter`). Not a general zip library: a general
//! writer records the real modification time and DEFLATEs, both byte differences from what the
//! shipping apps produce for no gain a reader can see. Every entry carries the fixed DOS date
//! 1980-01-01 (0x0021) and time 0, so an archive is a pure function of its payloads.
//!
//! No zip64, no data descriptors, no comment, and general-purpose flag bit 11 (UTF-8 names)
//! deliberately clear, as on every port: the names are written as UTF-8 bytes anyway, and every
//! md reader decodes them as UTF-8 whatever the flag says.

use std::fmt;

const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4B50;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4B50;
const END_RECORD_SIGNATURE: u32 = 0x0605_4B50;
/// (1980 - 1980) << 9 | 1 << 5 | 1
const DOS_DATE: u16 = 0x0021;
const DOS_TIME: u16 = 0;

/// One archive member, shared by [`ZipWriter`] and [`super::zip_reader`]. Equality is
/// structural over the bytes (md.win had to write that by hand; a derive gives it here).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
}

impl ZipEntry {
    pub fn new(name: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        Self { name: name.into(), data: data.into() }
    }
}

/// md.win's `ToString`: the name and the payload's size, never the payload — an entry can be
/// 128 MiB, and an assertion failure must not print it.
impl fmt::Debug for ZipEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ZipEntry({:?}, {} bytes)", self.name, self.data.len())
    }
}

/// What the classic (non-zip64) format cannot record. md.win throws `OverflowException` from a
/// `checked` cast at the same points; here the export reports it instead of taking the app down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZipError {
    /// A name longer than 65 535 UTF-8 bytes.
    NameTooLong,
    /// A payload of 4 GiB or more.
    EntryTooLarge,
    /// An entry starting, or a central directory starting or ending, past 4 GiB.
    ArchiveTooLarge,
    /// More than 65 535 entries.
    TooManyEntries,
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ZipError::NameTooLong => "a zip entry name is longer than 65535 bytes",
            ZipError::EntryTooLarge => "a zip entry is 4 GiB or larger",
            ZipError::ArchiveTooLarge => "the zip archive is 4 GiB or larger",
            ZipError::TooManyEntries => "the zip archive has more than 65535 entries",
        })
    }
}

impl std::error::Error for ZipError {}

/// The STORED-only writer: entries in the order they are added, then the central directory and
/// the end record.
#[derive(Clone, Debug, Default)]
pub struct ZipWriter {
    body: Vec<u8>,
    directory: Vec<u8>,
    count: u16,
}

impl ZipWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one file. Order is preserved — an EPUB adds "mimetype" first. On an error nothing
    /// is written, so the writer stays usable.
    pub fn add(&mut self, name: &str, contents: &[u8]) -> Result<(), ZipError> {
        let name_bytes = name.as_bytes();
        let name_length = u16::try_from(name_bytes.len()).map_err(|_| ZipError::NameTooLong)?;
        let size = u32::try_from(contents.len()).map_err(|_| ZipError::EntryTooLarge)?;
        let offset = u32::try_from(self.body.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        let count = self.count.checked_add(1).ok_or(ZipError::TooManyEntries)?;
        let crc = Self::crc32(contents);

        // Local file header (30 bytes), name, payload.
        let body = &mut self.body;
        body.reserve(30 + name_bytes.len() + contents.len());
        put_u32(body, LOCAL_HEADER_SIGNATURE);
        put_u16(body, 20); // version needed
        put_u16(body, 0); // flags
        put_u16(body, 0); // method: stored
        put_u16(body, DOS_TIME);
        put_u16(body, DOS_DATE);
        put_u32(body, crc);
        put_u32(body, size); // compressed == raw
        put_u32(body, size);
        put_u16(body, name_length);
        put_u16(body, 0); // extra length
        body.extend_from_slice(name_bytes);
        body.extend_from_slice(contents);

        // Matching central-directory record (46 bytes), name.
        let directory = &mut self.directory;
        put_u32(directory, CENTRAL_HEADER_SIGNATURE);
        put_u16(directory, 20); // version made by
        put_u16(directory, 20); // version needed
        put_u16(directory, 0); // flags
        put_u16(directory, 0); // method
        put_u16(directory, DOS_TIME);
        put_u16(directory, DOS_DATE);
        put_u32(directory, crc);
        put_u32(directory, size);
        put_u32(directory, size);
        put_u16(directory, name_length);
        put_u16(directory, 0); // extra
        put_u16(directory, 0); // comment
        put_u16(directory, 0); // disk number
        put_u16(directory, 0); // internal attributes
        put_u32(directory, 0); // external attributes
        put_u32(directory, offset);
        directory.extend_from_slice(name_bytes);
        self.count = count;
        Ok(())
    }

    /// The finished archive: entries, central directory, end record. The writer is left as it
    /// was, like md.win's `Finish`.
    pub fn finish(&self) -> Result<Vec<u8>, ZipError> {
        let directory_length = u32::try_from(self.directory.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        let directory_offset = u32::try_from(self.body.len()).map_err(|_| ZipError::ArchiveTooLarge)?;
        let mut output = Vec::with_capacity(self.body.len() + self.directory.len() + 22);
        output.extend_from_slice(&self.body);
        output.extend_from_slice(&self.directory);
        put_u32(&mut output, END_RECORD_SIGNATURE);
        put_u16(&mut output, 0); // this disk
        put_u16(&mut output, 0); // directory's disk
        put_u16(&mut output, self.count);
        put_u16(&mut output, self.count);
        put_u32(&mut output, directory_length);
        put_u32(&mut output, directory_offset);
        put_u16(&mut output, 0); // comment length
        Ok(output)
    }

    /// Pack ordered entries in one call; the order is the archive order.
    pub fn archive<'a>(entries: impl IntoIterator<Item = &'a ZipEntry>) -> Result<Vec<u8>, ZipError> {
        let mut writer = ZipWriter::new();
        for entry in entries {
            writer.add(&entry.name, &entry.data)?;
        }
        writer.finish()
    }

    /// Table-driven CRC-32 (the ZIP / PNG polynomial 0x04C11DB7, reflected as 0xEDB88320): init
    /// and final xor 0xFFFFFFFF. Pinned: "123456789" → 0xCBF43926, empty → 0.
    pub fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }
}

static CRC_TABLE: [u32; 256] = crc_table();

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 == 1 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
}

fn put_u16(buffer: &mut Vec<u8>, value: u16) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(buffer: &mut Vec<u8>, value: u32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}
