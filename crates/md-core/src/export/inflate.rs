//! A raw DEFLATE (RFC 1951) decoder for the zip reader — a zip entry holds the bare stream, no
//! zlib header or trailer. md.win hands this to `System.IO.Compression.DeflateStream` (zlib-ng)
//! and macOS to the `Compression` framework; md-core has std only, so it is written out here.
//!
//! Built for hostile input. Every read is bounds-checked (garbage yields an error, never a
//! panic), the output never grows past the caller's `limit`, and the work is linear in the input
//! plus the output: each Huffman symbol either consumes input bits or produces bytes. The only
//! allocation that scales is the output itself; its initial capacity is the smaller of the limit
//! and the most the input could expand to (DEFLATE peaks at 258 bytes per two bits), so a few
//! bytes that claim a large size cannot reserve it.
//!
//! The validity rules are zlib's (`inflate.c` / `inftrees.c`), which is what the other ports'
//! decoders answer: an over-subscribed code is refused everywhere; an incomplete code only when it
//! is not a single one-bit code (a stream with one distance, or only the end-of-block code, is
//! legal); a dynamic block may declare at most 286 literal/length and 30 distance codes and must
//! give the end-of-block code a length; a distance may not reach before the start of the output.
//! Errors are reported at the bit zlib reports them, so "the input ran out" and "the data is
//! invalid" are told apart exactly as zlib tells them apart — which is what the zip reader's
//! acceptance of a stream that stops short of its last end-of-block code depends on.

use std::fmt;
use std::sync::OnceLock;

/// Why a stream did not inflate. The messages are zlib's own (`strm->msg`), so a vector from
/// zlib's test suite names the error it expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InflateError {
    /// The input ended before the final block did.
    Truncated,
    /// Block type 3.
    InvalidBlockType,
    /// A stored block whose LEN is not the complement of its NLEN.
    InvalidStoredBlockLengths,
    /// HLIT above 286 or HDIST above 30.
    TooManyLengthOrDistanceSymbols,
    /// The code-length code is over-subscribed or incomplete.
    InvalidCodeLengthsSet,
    /// A repeat code (16) with nothing to repeat, or a repeat past the declared lengths.
    InvalidBitLengthRepeat,
    /// A dynamic block whose end-of-block code has no length.
    MissingEndOfBlock,
    /// The literal/length code is over-subscribed, or incomplete beyond one one-bit code.
    InvalidLiteralLengthsSet,
    /// The distance code is over-subscribed, or incomplete beyond one one-bit code.
    InvalidDistancesSet,
    /// A bit pattern no literal/length code has, or the reserved symbols 286 and 287.
    InvalidLiteralLengthCode,
    /// A bit pattern no distance code has, or the reserved symbols 30 and 31.
    InvalidDistanceCode,
    /// A distance reaching before the first byte of the output.
    InvalidDistanceTooFarBack,
    /// The stream holds more than the caller's limit.
    OutputLimitExceeded,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InflateError::Truncated => "unexpected end of stream",
            InflateError::InvalidBlockType => "invalid block type",
            InflateError::InvalidStoredBlockLengths => "invalid stored block lengths",
            InflateError::TooManyLengthOrDistanceSymbols => "too many length or distance symbols",
            InflateError::InvalidCodeLengthsSet => "invalid code lengths set",
            InflateError::InvalidBitLengthRepeat => "invalid bit length repeat",
            InflateError::MissingEndOfBlock => "invalid code -- missing end-of-block",
            InflateError::InvalidLiteralLengthsSet => "invalid literal/lengths set",
            InflateError::InvalidDistancesSet => "invalid distances set",
            InflateError::InvalidLiteralLengthCode => "invalid literal/length code",
            InflateError::InvalidDistanceCode => "invalid distance code",
            InflateError::InvalidDistanceTooFarBack => "invalid distance too far back",
            InflateError::OutputLimitExceeded => "output limit exceeded",
        })
    }
}

impl std::error::Error for InflateError {}

/// What a decode produced before it stopped, and why it stopped. `output` holds every byte
/// decoded up to the stop — on [`InflateError::Truncated`] that is the whole readable prefix.
#[derive(Clone, PartialEq, Eq)]
pub struct Inflated {
    pub output: Vec<u8>,
    pub status: Result<(), InflateError>,
}

impl fmt::Debug for Inflated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Inflated({} bytes, {:?})", self.output.len(), self.status)
    }
}

/// Inflate a complete raw DEFLATE stream of at most `limit` bytes. Anything after the final
/// block is ignored, as zlib ignores it.
pub fn inflate(compressed: &[u8], limit: usize) -> Result<Vec<u8>, InflateError> {
    let Inflated { output, status } = inflate_partial(compressed, limit);
    status.map(|()| output)
}

/// Inflate as far as the stream allows, keeping what was decoded when it stops — the shape the
/// zip reader needs to judge a stream by the bytes it yields, as the platform decoders do.
pub fn inflate_partial(compressed: &[u8], limit: usize) -> Inflated {
    // At most 258 bytes per two bits of input: a capacity hint, never a bound on the output.
    let hint = compressed.len().saturating_mul(1032).saturating_add(258).min(limit);
    let mut decoder =
        Decoder { input: compressed, position: 0, bits: 0, bit_count: 0, output: Vec::with_capacity(hint), limit };
    let status = decoder.run();
    Inflated { output: decoder.output, status }
}

const MAX_BITS: u32 = 15;
/// Codes up to this long decode with one table lookup; longer ones walk the canonical code.
const FAST_BITS: u32 = 9;
const FAST_MASK: u64 = (1 << FAST_BITS) - 1;

const LENGTH_BASE: [u16; 29] =
    [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LENGTH_EXTRA: [u32; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u32; 30] =
    [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
/// The order the code-length code's lengths are transmitted in.
const CODE_LENGTH_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// A canonical Huffman code (RFC 1951 §3.2.2).
struct Huffman {
    /// How many codes have each length (index 0 unused).
    count: [u16; 16],
    /// The first code of each length.
    first: [u32; 16],
    /// Where each length's symbols start in `symbol`.
    offset: [u16; 16],
    /// The coded symbols, ordered by code.
    symbol: [u16; 288],
    /// Left-aligned to 15 bits, the codes tile `0..total` in canonical order; a prefix at or past
    /// `total` can begin no code. 1 << 15 for a complete code.
    total: u32,
    /// One lookup for every code up to `FAST_BITS` long, indexed by the next input bits:
    /// `symbol | length << 12`, or 0 where the walk has to decide.
    fast: [u16; 1 << FAST_BITS],
}

/// A code built from its lengths, with what zlib's checks need to know about its shape.
struct Built {
    code: Huffman,
    /// Unused code space at 15 bits; above 0 means incomplete.
    left: i32,
    /// The longest length present; 0 when every length is 0.
    max_length: usize,
}

/// Build the canonical code for `lengths` (each 0–15), or `None` if it is over-subscribed.
fn build(lengths: &[u8]) -> Option<Built> {
    let mut code = Huffman {
        count: [0; 16],
        first: [0; 16],
        offset: [0; 16],
        symbol: [0; 288],
        total: 0,
        fast: [0; 1 << FAST_BITS],
    };
    for &length in lengths {
        code.count[usize::from(length & 15)] += 1;
    }
    let max_length = (1..16).rev().find(|&length| code.count[length] != 0).unwrap_or(0);
    let mut left: i32 = 1;
    for length in 1..16 {
        left <<= 1;
        left -= i32::from(code.count[length]);
        if left < 0 {
            return None;
        }
    }
    for length in 1..15 {
        code.offset[length + 1] = code.offset[length] + code.count[length];
        code.first[length + 1] = (code.first[length] + u32::from(code.count[length])) << 1;
    }
    let mut next = code.offset;
    for (symbol, &length) in lengths.iter().enumerate() {
        let length = usize::from(length & 15);
        if length != 0 {
            code.symbol[usize::from(next[length])] = symbol as u16;
            next[length] += 1;
        }
    }
    for length in 1..16 {
        code.total += u32::from(code.count[length]) << (MAX_BITS as usize - length);
    }
    for length in 1..=FAST_BITS as usize {
        for index in 0..usize::from(code.count[length]) {
            let value = code.first[length] + index as u32;
            let symbol = code.symbol[usize::from(code.offset[length]) + index];
            // Codes are sent most-significant bit first into an LSB-first bit stream, so the
            // table is indexed by the code reversed, with every possible tail.
            let mut slot = reverse(value, length as u32) as usize;
            while slot < code.fast.len() {
                code.fast[slot] = symbol | (length as u16) << 12;
                slot += 1 << length;
            }
        }
    }
    Some(Built { code, left, max_length })
}

fn reverse(value: u32, length: u32) -> u32 {
    value.reverse_bits() >> (32 - length)
}

/// The fixed codes of block type 1: literal/lengths 0–143 at 8 bits, 144–255 at 9, 256–279 at 7,
/// 280–287 at 8; all 32 distance codes at 5 bits (30 and 31 are refused when used).
fn fixed() -> &'static (Huffman, Huffman) {
    static FIXED: OnceLock<(Huffman, Huffman)> = OnceLock::new();
    FIXED.get_or_init(|| {
        let mut lengths = [0u8; 288];
        lengths[..144].fill(8);
        lengths[144..256].fill(9);
        lengths[256..280].fill(7);
        lengths[280..].fill(8);
        let literal = build(&lengths).map(|built| built.code);
        let distance = build(&[5u8; 32]).map(|built| built.code);
        match (literal, distance) {
            (Some(literal), Some(distance)) => (literal, distance),
            _ => unreachable!("the fixed codes are complete"),
        }
    })
}

struct Decoder<'a> {
    input: &'a [u8],
    position: usize,
    /// Pending input bits, least significant first.
    bits: u64,
    bit_count: u32,
    output: Vec<u8>,
    limit: usize,
}

impl Decoder<'_> {
    fn run(&mut self) -> Result<(), InflateError> {
        loop {
            let last = self.take(1)?;
            match self.take(2)? {
                0 => self.stored()?,
                1 => {
                    let (literal, distance) = fixed();
                    self.codes(literal, distance)?;
                }
                2 => {
                    let (literal, distance) = self.dynamic()?;
                    self.codes(&literal, &distance)?;
                }
                _ => return Err(InflateError::InvalidBlockType),
            }
            if last == 1 {
                return Ok(());
            }
        }
    }

    /// Top the bit buffer up to at least 57 bits, or with whatever input is left.
    fn refill(&mut self) {
        while self.bit_count <= 56 {
            let Some(&byte) = self.input.get(self.position) else { break };
            self.bits |= u64::from(byte) << self.bit_count;
            self.bit_count += 8;
            self.position += 1;
        }
    }

    fn consume(&mut self, count: u32) {
        self.bits >>= count;
        self.bit_count -= count;
    }

    /// The next `count` (at most 32) bits as a number, first bit least significant.
    fn take(&mut self, count: u32) -> Result<u32, InflateError> {
        if self.bit_count < count {
            self.refill();
            if self.bit_count < count {
                return Err(InflateError::Truncated);
            }
        }
        let value = (self.bits & ((1u64 << count) - 1)) as u32;
        self.consume(count);
        Ok(value)
    }

    fn push(&mut self, byte: u8) -> Result<(), InflateError> {
        if self.output.len() >= self.limit {
            return Err(InflateError::OutputLimitExceeded);
        }
        self.output.push(byte);
        Ok(())
    }

    /// Decode one symbol. `invalid` names a bit pattern the code does not have.
    fn decode(&mut self, code: &Huffman, invalid: InflateError) -> Result<u16, InflateError> {
        if self.bit_count < MAX_BITS {
            self.refill();
        }
        if self.bit_count >= FAST_BITS {
            let entry = code.fast[(self.bits & FAST_MASK) as usize];
            if entry != 0 {
                self.consume(u32::from(entry >> 12));
                return Ok(entry & 0x0FFF);
            }
        }
        // Walk the canonical code one bit at a time (puff's decode).
        let mut prefix: u32 = 0;
        for length in 1..=MAX_BITS {
            if length > self.bit_count {
                // Out of input in mid-code. Like zlib, call the pattern invalid only once the
                // bits in hand already rule out every code; until then, more input might help.
                let aligned = prefix << (MAX_BITS + 1 - length);
                return Err(if length > 1 && aligned >= code.total { invalid } else { InflateError::Truncated });
            }
            prefix = (prefix << 1) | ((self.bits >> (length - 1)) & 1) as u32;
            let index = length as usize;
            let within = prefix.wrapping_sub(code.first[index]);
            if within < u32::from(code.count[index]) {
                self.consume(length);
                return Ok(code.symbol[usize::from(code.offset[index]) + within as usize]);
            }
        }
        Err(invalid)
    }

    /// Block type 0: byte-aligned LEN, NLEN, then LEN bytes copied through.
    fn stored(&mut self) -> Result<(), InflateError> {
        self.consume(self.bit_count % 8);
        let length = self.take(16)?;
        let complement = self.take(16)?;
        if length != !complement & 0xFFFF {
            return Err(InflateError::InvalidStoredBlockLengths);
        }
        let mut remaining = length as usize;
        // Whole bytes already pulled into the bit buffer come first…
        while remaining > 0 && self.bit_count >= 8 {
            let byte = self.take(8)? as u8;
            self.push(byte)?;
            remaining -= 1;
        }
        if remaining == 0 {
            return Ok(());
        }
        // …then the rest straight from the input. Room runs out before input does only when the
        // block really holds more than the limit; input running out first is a truncation.
        let available = self.input.len() - self.position;
        let room = self.limit - self.output.len();
        let count = remaining.min(available).min(room);
        self.output.extend_from_slice(&self.input[self.position..self.position + count]);
        self.position += count;
        remaining -= count;
        if remaining == 0 {
            Ok(())
        } else if count < available {
            Err(InflateError::OutputLimitExceeded)
        } else {
            Err(InflateError::Truncated)
        }
    }

    /// Block type 2's header: the code-length code, then the literal/length and distance codes.
    fn dynamic(&mut self) -> Result<(Huffman, Huffman), InflateError> {
        let literal_count = self.take(5)? as usize + 257;
        let distance_count = self.take(5)? as usize + 1;
        let code_length_count = self.take(4)? as usize + 4;
        if literal_count > 286 || distance_count > 30 {
            return Err(InflateError::TooManyLengthOrDistanceSymbols);
        }

        let mut lengths = [0u8; 286 + 30];
        for &index in &CODE_LENGTH_ORDER[..code_length_count] {
            lengths[index] = self.take(3)? as u8;
        }
        let Some(code_lengths) = build(&lengths[..19]) else {
            return Err(InflateError::InvalidCodeLengthsSet);
        };
        // zlib accepts an all-zero code-length code and then reads every length as a 0 costing
        // one bit; the block fails later, at the missing end-of-block code. Kept, so a stream cut
        // off inside such a header is a truncation here exactly as there.
        let empty = code_lengths.max_length == 0;
        if code_lengths.left > 0 && !empty {
            return Err(InflateError::InvalidCodeLengthsSet);
        }

        lengths = [0u8; 286 + 30];
        let total = literal_count + distance_count;
        let mut have = 0;
        while have < total {
            let symbol = if empty {
                self.take(1)?;
                0
            } else {
                self.decode(&code_lengths.code, InflateError::InvalidCodeLengthsSet)?
            };
            if symbol < 16 {
                lengths[have] = symbol as u8;
                have += 1;
                continue;
            }
            let (value, repeat) = match symbol {
                16 => {
                    let extra = self.take(2)? as usize;
                    if have == 0 {
                        return Err(InflateError::InvalidBitLengthRepeat);
                    }
                    (lengths[have - 1], 3 + extra)
                }
                17 => (0, 3 + self.take(3)? as usize),
                _ => (0, 11 + self.take(7)? as usize),
            };
            if have + repeat > total {
                return Err(InflateError::InvalidBitLengthRepeat);
            }
            lengths[have..have + repeat].fill(value);
            have += repeat;
        }

        if lengths[256] == 0 {
            return Err(InflateError::MissingEndOfBlock);
        }
        let literal = match build(&lengths[..literal_count]) {
            Some(built) if built.left == 0 || built.max_length == 1 => built.code,
            _ => return Err(InflateError::InvalidLiteralLengthsSet),
        };
        let distance = match build(&lengths[literal_count..total]) {
            Some(built) if built.left == 0 || built.max_length <= 1 => built.code,
            _ => return Err(InflateError::InvalidDistancesSet),
        };
        Ok((literal, distance))
    }

    /// The symbols of a Huffman-coded block, up to and including its end-of-block code.
    fn codes(&mut self, literal: &Huffman, distance: &Huffman) -> Result<(), InflateError> {
        loop {
            let symbol = self.decode(literal, InflateError::InvalidLiteralLengthCode)?;
            if symbol < 256 {
                self.push(symbol as u8)?;
                continue;
            }
            if symbol == 256 {
                return Ok(());
            }
            let index = usize::from(symbol - 257);
            if index >= LENGTH_BASE.len() {
                return Err(InflateError::InvalidLiteralLengthCode);
            }
            let length = usize::from(LENGTH_BASE[index]) + self.take(LENGTH_EXTRA[index])? as usize;
            let index = usize::from(self.decode(distance, InflateError::InvalidDistanceCode)?);
            if index >= DISTANCE_BASE.len() {
                return Err(InflateError::InvalidDistanceCode);
            }
            let distance = usize::from(DISTANCE_BASE[index]) + self.take(DISTANCE_EXTRA[index])? as usize;
            if distance > self.output.len() {
                return Err(InflateError::InvalidDistanceTooFarBack);
            }
            if length > self.limit - self.output.len() {
                return Err(InflateError::OutputLimitExceeded);
            }
            self.copy(distance, length);
        }
    }

    /// Append `length` bytes starting `distance` back, overlapping allowed. The source window
    /// doubles on every pass (the bytes just written are a copy of the period), so a long run
    /// costs a handful of slice copies rather than one per byte.
    fn copy(&mut self, distance: usize, length: usize) {
        let start = self.output.len() - distance;
        let mut remaining = length;
        while remaining > 0 {
            let count = remaining.min(self.output.len() - start);
            self.output.extend_from_within(start..start + count);
            remaining -= count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverses_bits_within_a_length() {
        assert_eq!(reverse(0b1, 1), 0b1);
        assert_eq!(reverse(0b110, 3), 0b011);
        assert_eq!(reverse(0b0000_0001, 8), 0b1000_0000);
    }

    #[test]
    fn the_fixed_codes_are_complete() {
        let (literal, distance) = fixed();
        assert_eq!(literal.total, 1 << 15);
        assert_eq!(distance.total, 1 << 15);
        assert_eq!(literal.count[7], 24);
        assert_eq!(literal.count[8], 152);
        assert_eq!(literal.count[9], 112);
    }

    #[test]
    fn an_over_subscribed_code_is_refused_and_an_incomplete_one_measured() {
        assert!(build(&[1, 1, 1]).is_none());
        let single = build(&[0, 1]).expect("one one-bit code is not over-subscribed");
        assert_eq!((single.left, single.max_length, single.code.total), (1 << 14, 1, 1 << 14));
        let empty = build(&[0, 0, 0]).expect("no codes at all is not over-subscribed");
        assert_eq!((empty.max_length, empty.code.total), (0, 0));
    }
}
