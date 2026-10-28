//! A minimal zip reader: enough to pull the entries out of a `.textpack` (macOS `ZipReader`,
//! md.win `Md.Core.Export.ZipReader`). It walks the central directory, which is authoritative for
//! sizes and local-header offsets even when an entry was written with a streaming data
//! descriptor (sizes zeroed in the local header) — the reason a front-to-back local-header walk
//! is unreliable. STORED (0) and DEFLATE (8) only; anything else, or a truncated archive, yields
//! `None` rather than a guess. No I/O: bytes in, entries out. CRCs are not checked, on any port.

use super::inflate;
use super::zip_writer::ZipEntry;

/// The most a single entry may claim to inflate to. The central directory's uncompressed size
/// is an attacker-controlled u32, and md.win and macOS allocate the buffer before decoding — a
/// hundred-byte crafted pack could otherwise force a multi-gigabyte allocation just by being
/// opened. (This port grows the buffer as it decodes, but keeps the same cap so a pack opens
/// here exactly when it opens there.)
pub const MAX_ENTRY_SIZE: usize = 128 * 1024 * 1024;

const LOCAL_HEADER_SIGNATURE: u32 = 0x0403_4B50;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4B50;

/// Every file entry (directory entries — names ending in "/" — dropped), or `None` if the bytes
/// are not a zip this reader can parse. An entry `should_inflate` rejects is still listed (by
/// name, with empty data) but never read or inflated: import reads only `text.md`, so a hostile
/// pack full of huge assets costs nothing.
pub fn entries(archive: &[u8], should_inflate: Option<&dyn Fn(&str) -> bool>) -> Option<Vec<ZipEntry>> {
    let (count, central_offset) = locate_end_record(archive)?;

    // Offsets and sizes are u32 on the wire; they are added in u64 so a hostile value cannot
    // wrap before the bounds checks compare it against the buffer, and every read is a
    // checked slice besides.
    let length = archive.len() as u64;
    let mut offset = u64::from(central_offset);
    let mut results = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        if offset + 46 > length || u32_at(archive, offset)? != CENTRAL_HEADER_SIGNATURE {
            return None;
        }
        let method = u16_at(archive, offset + 10)?;
        let compressed_size = u64::from(u32_at(archive, offset + 20)?);
        let uncompressed_size = u64::from(u32_at(archive, offset + 24)?);
        let name_length = u64::from(u16_at(archive, offset + 28)?);
        let extra_length = u64::from(u16_at(archive, offset + 30)?);
        let comment_length = u64::from(u16_at(archive, offset + 32)?);
        let local_offset = u64::from(u32_at(archive, offset + 42)?);
        let name_start = offset + 46;
        // Lossy UTF-8 like Swift's String(decoding:as:UTF8.self); flag bit 11 is ignored and
        // CP437 legacy names are not supported on any port.
        let name = String::from_utf8_lossy(slice(archive, name_start, name_length)?).into_owned();
        offset = name_start + name_length + extra_length + comment_length;

        // A directory carries no payload — and has no local header worth chasing.
        if name.ends_with('/') {
            continue;
        }

        if let Some(should_inflate) = should_inflate
            && !should_inflate(&name)
        {
            results.push(ZipEntry { name, data: Vec::new() });
            continue;
        }

        // The local header repeats its own name/extra lengths, which may differ from the central
        // copy, so the data offset is computed from the local ones.
        if local_offset + 30 > length || u32_at(archive, local_offset)? != LOCAL_HEADER_SIGNATURE {
            return None;
        }
        let local_name_length = u64::from(u16_at(archive, local_offset + 26)?);
        let local_extra_length = u64::from(u16_at(archive, local_offset + 28)?);
        let data_start = local_offset + 30 + local_name_length + local_extra_length;
        let payload = slice(archive, data_start, compressed_size)?;

        let data = match method {
            // macOS caps a stored entry as well (md.win does not): the Mac is the reference.
            0 if payload.len() <= MAX_ENTRY_SIZE => payload.to_vec(),
            8 => inflate(payload, uncompressed_size)?,
            _ => return None,
        };
        results.push(ZipEntry { name, data });
    }
    Some(results)
}

/// The End Of Central Directory record: 22 bytes near the tail, possibly trailed by a comment of
/// up to 65 535 bytes, so it is found by scanning backward for its signature. Returns the entry
/// count and the central directory's offset.
fn locate_end_record(bytes: &[u8]) -> Option<(u16, u32)> {
    if bytes.len() < 22 {
        return None;
    }
    let lower_bound = bytes.len().saturating_sub(22 + 0xFFFF);
    (lower_bound..=bytes.len() - 22).rev().find(|&i| bytes[i..i + 4] == [0x50, 0x4B, 0x05, 0x06]).and_then(|i| {
        let i = i as u64;
        Some((u16_at(bytes, i + 10)?, u32_at(bytes, i + 16)?))
    })
}

/// Inflate an entry to exactly its declared size, or `None`. Refuses, without decoding, a claim
/// above [`MAX_ENTRY_SIZE`]. Like the platform decoders: a stream that yields fewer bytes than
/// declared is truncated or corrupt; one that would yield more, or that is corrupt anywhere up
/// to its end, is refused (md.win reads one byte past the declared size and requires nothing
/// there); one that runs out of input right after the declared bytes — before its final
/// end-of-block code — is accepted, as zlib and `DeflateStream` accept it.
fn inflate(compressed: &[u8], expected_size: u64) -> Option<Vec<u8>> {
    // Swift's `guard expectedSize > 0 else { return Data() }`: a zero claim skips the decoder.
    if expected_size == 0 {
        return Some(Vec::new());
    }
    if expected_size > MAX_ENTRY_SIZE as u64 || compressed.is_empty() {
        return None;
    }
    let expected = expected_size as usize;
    let inflate::Inflated { output, status } = inflate::inflate_partial(compressed, expected);
    match status {
        Ok(()) | Err(inflate::InflateError::Truncated) if output.len() == expected => Some(output),
        _ => None,
    }
}

fn slice(bytes: &[u8], start: u64, length: u64) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    let end = start.checked_add(usize::try_from(length).ok()?)?;
    bytes.get(start..end)
}

fn u16_at(bytes: &[u8], index: u64) -> Option<u16> {
    let raw = slice(bytes, index, 2)?;
    Some(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], index: u64) -> Option<u32> {
    let raw = slice(bytes, index, 4)?;
    Some(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}
