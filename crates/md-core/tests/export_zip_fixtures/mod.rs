//! The test-private archive builders the macOS suite keeps beside its TextBundle tests (md.win
//! `ZipFixtures.cs`): a STORED archive through the app's own writer, and a hand-built one-entry
//! DEFLATE archive so the reader's inflate path meets a genuine compressed stream (the writer only
//! stores). Shared by the zip, bundle and adversarial suites.
//!
//! md.win compresses its fixtures with `DeflateStream` and checks archives with
//! `System.IO.Compression.ZipArchive`; std has neither, so this module carries its own small
//! DEFLATE *encoder* (LZ77 + the fixed Huffman codes — independent of the decoder under test and
//! cross-checked against zlib in `export_zip_inflate.rs`) and an independent "foreign" zip walker
//! that reads front to back and verifies every CRC, plus CPython's `zipfile` when `python3` is on
//! the PATH.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use md_core::export::{ZipEntry, ZipWriter};

pub fn utf8(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// Strict UTF-8 decode (md.win's `throwOnInvalidBytes: true`).
pub fn utf8_text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("valid UTF-8")
}

/// Windows-1251 for ASCII and the Russian alphabet — written out here rather than taken from the
/// codec under test.
pub fn cp1251(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '\0'..='\u{7F}' => c as u8,
            'А'..='я' => (c as u32 - 'А' as u32 + 0xC0) as u8,
            'Ё' => 0xA8,
            'ё' => 0xB8,
            _ => panic!("{c:?} is outside this fixture's Windows-1251 subset"),
        })
        .collect()
}

/// A stored (method 0) archive through the very writer the EPUB export uses.
pub fn stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let entries: Vec<ZipEntry> = entries.iter().map(|&(name, data)| ZipEntry::new(name, data)).collect();
    ZipWriter::archive(&entries).expect("a small archive")
}

pub fn named<'a>(entries: Option<&'a [ZipEntry]>, name: &str) -> Option<&'a ZipEntry> {
    entries?.iter().find(|entry| entry.name == name)
}

/// A one-entry zip using DEFLATE (method 8), laid out exactly like the Swift test's
/// `deflatedArchive`: local header, payload, central record, end record, all little-endian, DOS
/// date 0x21. `claimed_size` overrides the uncompressed size both headers declare (a lying
/// archive); `method` overrides the method code (an unsupported one).
pub fn deflated(name: &str, payload: &[u8], claimed_size: Option<u32>, method: u16) -> Vec<u8> {
    deflated_precompressed(
        name,
        &deflate_raw(payload),
        claimed_size.unwrap_or(payload.len() as u32),
        ZipWriter::crc32(payload),
        method,
    )
}

/// The same one-entry method-8 archive around an already-compressed stream, for a payload the
/// test must never hold in memory (the 128 MiB cap boundary). The reader does not verify CRCs, so
/// `crc` may be left at 0.
pub fn deflated_precompressed(name: &str, compressed: &[u8], claimed: u32, crc: u32, method: u16) -> Vec<u8> {
    let name = name.as_bytes();
    let mut out = Vec::new();
    let u16 = |out: &mut Vec<u8>, value: u16| out.extend_from_slice(&value.to_le_bytes());
    let u32 = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());

    // Local file header + compressed payload.
    u32(&mut out, 0x0403_4B50);
    u16(&mut out, 20);
    u16(&mut out, 0);
    u16(&mut out, method); // version / flags / method
    u16(&mut out, 0);
    u16(&mut out, 0x21); // dos time / date
    u32(&mut out, crc);
    u32(&mut out, compressed.len() as u32);
    u32(&mut out, claimed);
    u16(&mut out, name.len() as u16);
    u16(&mut out, 0); // name / extra length
    out.extend_from_slice(name);
    out.extend_from_slice(compressed);

    // Central-directory file header.
    let central_start = out.len() as u32;
    u32(&mut out, 0x0201_4B50);
    u16(&mut out, 20);
    u16(&mut out, 20);
    u16(&mut out, 0);
    u16(&mut out, method); // made-by / needed / flags / method
    u16(&mut out, 0);
    u16(&mut out, 0x21);
    u32(&mut out, crc);
    u32(&mut out, compressed.len() as u32);
    u32(&mut out, claimed);
    u16(&mut out, name.len() as u16);
    u16(&mut out, 0);
    u16(&mut out, 0); // name / extra / comment
    u16(&mut out, 0);
    u16(&mut out, 0);
    u32(&mut out, 0); // disk / internal / external attrs
    u32(&mut out, 0); // local header offset
    out.extend_from_slice(name);
    let central_size = out.len() as u32 - central_start;

    // End of central directory.
    u32(&mut out, 0x0605_4B50);
    u16(&mut out, 0);
    u16(&mut out, 0);
    u16(&mut out, 1);
    u16(&mut out, 1); // disks / entries
    u32(&mut out, central_size);
    u32(&mut out, central_start);
    u16(&mut out, 0); // comment length
    out
}

// MARK: - A small DEFLATE encoder (RFC 1951, one final fixed-Huffman block)

const LENGTH_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u32; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct BitWriter {
    out: Vec<u8>,
    bits: u64,
    count: u32,
}

impl BitWriter {
    fn new() -> Self {
        Self { out: Vec::new(), bits: 0, count: 0 }
    }

    /// `count` bits of `value`, least significant first (header fields and extra bits).
    fn put(&mut self, value: u32, count: u32) {
        self.bits |= u64::from(value) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.out.push(self.bits as u8);
            self.bits >>= 8;
            self.count -= 8;
        }
    }

    /// A Huffman code, most significant bit first.
    fn code(&mut self, code: u32, length: u32) {
        self.put(code.reverse_bits() >> (32 - length), length);
    }

    fn literal_or_length(&mut self, symbol: u32) {
        match symbol {
            0..=143 => self.code(0x30 + symbol, 8),
            144..=255 => self.code(0x190 + symbol - 144, 9),
            256..=279 => self.code(symbol - 256, 7),
            _ => self.code(0xC0 + symbol - 280, 8),
        }
    }

    fn matched(&mut self, length: usize, distance: usize) {
        let index = LENGTH_BASE.iter().rposition(|&base| usize::from(base) <= length).unwrap();
        self.literal_or_length(257 + index as u32);
        self.put((length - usize::from(LENGTH_BASE[index])) as u32, LENGTH_EXTRA[index]);
        let index = DISTANCE_BASE.iter().rposition(|&base| usize::from(base) <= distance).unwrap();
        self.code(index as u32, 5);
        self.put((distance - usize::from(DISTANCE_BASE[index])) as u32, DISTANCE_EXTRA[index]);
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(self.bits as u8);
        }
        self.out
    }
}

/// Raw DEFLATE (no zlib header): greedy LZ77 over a 32 KiB window with hash chains, one final
/// block with the fixed codes — a genuine compressed stream, like md.win's `DeflateRaw`.
pub fn deflate_raw(data: &[u8]) -> Vec<u8> {
    const WINDOW: usize = 32 * 1024;
    const MAX_CHAIN: usize = 128;
    let hash = |i: usize| -> usize {
        ((usize::from(data[i]) << 10) ^ (usize::from(data[i + 1]) << 5) ^ usize::from(data[i + 2])) & 0x7FFF
    };
    let mut head = vec![usize::MAX; 1 << 15];
    let mut previous = vec![usize::MAX; data.len()];
    let insert = |i: usize, head: &mut Vec<usize>, previous: &mut Vec<usize>| {
        if i + 3 <= data.len() {
            let h = hash(i);
            previous[i] = head[h];
            head[h] = i;
        }
    };

    let mut writer = BitWriter::new();
    writer.put(1, 1); // BFINAL
    writer.put(1, 2); // BTYPE = fixed
    let mut i = 0;
    while i < data.len() {
        let mut best = (0, 0);
        if i + 3 <= data.len() {
            let mut candidate = head[hash(i)];
            let mut chain = 0;
            while candidate != usize::MAX && i - candidate <= WINDOW && chain < MAX_CHAIN {
                let limit = (data.len() - i).min(258);
                let length = (0..limit).take_while(|&k| data[candidate + k] == data[i + k]).count();
                if length > best.0 {
                    best = (length, i - candidate);
                }
                candidate = previous[candidate];
                chain += 1;
            }
        }
        if best.0 >= 3 {
            writer.matched(best.0, best.1);
            for k in i..i + best.0 {
                insert(k, &mut head, &mut previous);
            }
            i += best.0;
        } else {
            writer.literal_or_length(u32::from(data[i]));
            insert(i, &mut head, &mut previous);
            i += 1;
        }
    }
    writer.literal_or_length(256);
    writer.finish()
}

/// Raw DEFLATE of `count` zero bytes, produced without ever holding them: a literal 0, then
/// maximal (258, distance 1) matches.
pub fn deflate_zeros(count: u64) -> Vec<u8> {
    let mut writer = BitWriter::new();
    writer.put(1, 1);
    writer.put(1, 2);
    let mut remaining = count;
    if remaining > 0 {
        writer.literal_or_length(0);
        remaining -= 1;
    }
    while remaining > 0 {
        if remaining < 3 {
            writer.literal_or_length(0);
            remaining -= 1;
        } else {
            let length = remaining.min(258);
            writer.matched(length as usize, 1);
            remaining -= length;
        }
    }
    writer.literal_or_length(256);
    writer.finish()
}

// MARK: - A foreign reader

/// One entry as an independent reader sees it.
#[derive(Debug, PartialEq, Eq)]
pub struct ForeignEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// CRC-32 bit by bit — deliberately not the table the writer uses.
pub fn bitwise_crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The `unzip -t` of this suite: walk a STORED archive front to back by its local headers,
/// verify each payload's CRC and sizes, then check that the central directory and the end record
/// describe exactly what was walked and that nothing trails the archive. Written independently of
/// `zip_reader`, which trusts the central directory and checks no CRC.
pub fn foreign_entries(archive: &[u8]) -> Result<Vec<ForeignEntry>, String> {
    let u16_at = |i: usize| -> Result<usize, String> {
        archive.get(i..i + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]]))).ok_or(format!("u16 at {i}"))
    };
    let u32_at = |i: usize| -> Result<u32, String> {
        archive.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(format!("u32 at {i}"))
    };
    let mut entries = Vec::new();
    let mut locals = Vec::new();
    let mut at = 0;
    while u32_at(at)? == 0x0403_4B50 {
        let flags = u16_at(at + 6)?;
        let method = u16_at(at + 8)?;
        let crc = u32_at(at + 14)?;
        let compressed = u32_at(at + 18)? as usize;
        let size = u32_at(at + 22)? as usize;
        let name_length = u16_at(at + 26)?;
        let extra_length = u16_at(at + 28)?;
        if flags & 0x0008 != 0 || method != 0 || compressed != size {
            return Err(format!("entry at {at}: not a plain stored entry"));
        }
        let name = archive.get(at + 30..at + 30 + name_length).ok_or("name past the end")?;
        let start = at + 30 + name_length + extra_length;
        let data = archive.get(start..start + size).ok_or("payload past the end")?;
        if bitwise_crc32(data) != crc {
            return Err(format!("entry at {at}: CRC mismatch"));
        }
        let name = String::from_utf8(name.to_vec()).map_err(|_| "name is not UTF-8")?;
        locals.push((at, name.clone(), crc, size));
        entries.push(ForeignEntry { name, data: data.to_vec() });
        at = start + size;
    }
    let directory_start = at;
    for (offset, name, crc, size) in &locals {
        if u32_at(at)? != 0x0201_4B50 {
            return Err(format!("central record for {name} missing"));
        }
        let name_length = u16_at(at + 28)?;
        let central_name = archive.get(at + 46..at + 46 + name_length).ok_or("central name past the end")?;
        if central_name != name.as_bytes()
            || u32_at(at + 16)? != *crc
            || u32_at(at + 20)? as usize != *size
            || u32_at(at + 24)? as usize != *size
            || u32_at(at + 42)? as usize != *offset
        {
            return Err(format!("central record for {name} disagrees with its local header"));
        }
        at += 46 + name_length + u16_at(at + 30)? + u16_at(at + 32)?;
    }
    if u32_at(at)? != 0x0605_4B50 {
        return Err("end record missing".into());
    }
    let count = locals.len();
    if u16_at(at + 8)? != count
        || u16_at(at + 10)? != count
        || u32_at(at + 12)? as usize != at - directory_start
        || u32_at(at + 16)? as usize != directory_start
        || at + 22 + u16_at(at + 20)? != archive.len()
    {
        return Err("end record disagrees with the archive".into());
    }
    Ok(entries)
}

/// CPython's `zipfile` as a second foreign reader: the names it lists after `testzip()` has
/// checked every CRC, or `None` when there is no `python3` to ask. Names must be ASCII here
/// (with flag bit 11 clear, `zipfile` reads names as CP437).
pub fn python_zip_names(archive: &[u8]) -> Option<Result<Vec<String>, String>> {
    const SCRIPT: &str = "import io, sys, zipfile\n\
        z = zipfile.ZipFile(io.BytesIO(sys.stdin.buffer.read()))\n\
        bad = z.testzip()\n\
        if bad is not None:\n    sys.exit('bad entry: ' + bad)\n\
        sys.stdout.write('\\n'.join(i.filename for i in z.infolist()))\n";
    let mut child = Command::new("python3")
        .args(["-I", "-c", SCRIPT])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take()?;
        stdin.write_all(archive).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return Some(Err(String::from_utf8_lossy(&output.stderr).into_owned()));
    }
    let listing = String::from_utf8_lossy(&output.stdout).into_owned();
    Some(Ok(if listing.is_empty() { Vec::new() } else { listing.split('\n').map(str::to_string).collect() }))
}

/// Asserts the foreign readers accept `archive` and list `names`, in order (directory entries
/// included). Returns the payloads the independent walker read.
pub fn assert_foreign_readers_accept(archive: &[u8], names: &[&str]) -> Vec<ForeignEntry> {
    let entries = foreign_entries(archive).unwrap_or_else(|error| panic!("foreign walker refused: {error}"));
    assert_eq!(entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(), names);
    match python_zip_names(archive) {
        Some(Ok(listed)) => assert_eq!(listed, names, "CPython zipfile"),
        Some(Err(error)) => panic!("CPython zipfile refused the archive: {error}"),
        None => eprintln!("python3 not found: checked with the in-suite walker only"),
    }
    entries
}

// MARK: - Temporary folders

/// A fresh folder under the system temp directory, unique per process and call (the suites run
/// in parallel), removed with everything in it when dropped — even when the test panics.
pub struct TempFolder(PathBuf);

impl TempFolder {
    pub fn new(suite: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
        let unique = format!("{}-{nanos:x}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        let path = std::env::temp_dir().join(suite).join(unique);
        std::fs::create_dir_all(&path).expect("create a temporary folder");
        TempFolder(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
