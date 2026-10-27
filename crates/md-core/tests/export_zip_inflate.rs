//! The DEFLATE decoder behind the zip reader — md.win leaves this to `DeflateStream`, so these
//! tests have no md.win counterpart. They pin it against zlib, the reference every platform
//! decoder descends from: zlib's own coverage vectors (`test/infcover.c`), streams zlib wrote
//! with every strategy and flush mode (`export_zip_fixtures/zlib-corpus.txt`), the limit and
//! truncation semantics the zip reader relies on, and a deterministic sweep of hostile input.
//!
//! When the port was written, the zip reader was also run differentially against md.win's own
//! `ZipReader.Inflate` on .NET 10 (`DeflateStream`, zlib-ng) over about 360 000 streams — the
//! corpus below mutated, truncated and given wrong declared sizes, plus noise — and accepted or
//! refused every one exactly as md.win did, with the same bytes.

mod export_zip_fixtures;

use export_zip_fixtures::*;
use md_core::export::inflate::{self, InflateError, Inflated};
use md_core::export::zip_reader;

fn spaced_hex(text: &str) -> Vec<u8> {
    text.split_whitespace().map(|byte| u8::from_str_radix(byte, 16).unwrap()).collect()
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

/// A tiny deterministic generator (the LCG from Numerical Recipes), shared with the script that
/// wrote the corpus.
struct Lcg(u32);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() >> 8) as usize % bound
    }
}

const WORDS: [&str; 24] = [
    "the",
    "quick",
    "brown",
    "fox",
    "jumps",
    "over",
    "lazy",
    "dog",
    "md",
    "markdown",
    "Привет",
    "мир",
    "😀",
    "\n",
    "\n\n",
    "# ",
    "- ",
    "**",
    "`code`",
    "![a](p.png)",
    "1234567890",
    "lorem",
    "ipsum",
    "\t",
];

/// The corpus payloads, regenerated exactly as the corpus script generated them.
fn generate(kind: &str, seed: u32, length: usize) -> Vec<u8> {
    let mut random = Lcg(seed);
    let mut out = Vec::new();
    let word = |random: &mut Lcg, out: &mut Vec<u8>| {
        out.extend_from_slice(WORDS[(random.next() >> 16) as usize % WORDS.len()].as_bytes());
        out.push(b' ');
    };
    match kind {
        "text" => {
            while out.len() < length {
                word(&mut random, &mut out);
            }
        }
        "noise" => {
            while out.len() < length {
                out.push((random.next() >> 24) as u8);
            }
        }
        "runs" => {
            while out.len() < length {
                let byte = (random.next() >> 24) as u8;
                let run = (random.next() >> 24) % 40 + 1;
                out.extend(std::iter::repeat_n(byte, run as usize));
            }
        }
        "zeros" => out = vec![0; length],
        "mixed" => {
            while out.len() < length {
                if random.next() >> 31 == 0 {
                    for _ in 0..500 {
                        word(&mut random, &mut out);
                    }
                } else {
                    for _ in 0..500 {
                        out.push((random.next() >> 24) as u8);
                    }
                }
            }
        }
        _ => panic!("unknown corpus kind {kind}"),
    }
    out.truncate(length);
    out
}

struct Sample {
    name: String,
    payload: Vec<u8>,
    compressed: Vec<u8>,
}

fn corpus() -> Vec<Sample> {
    include_str!("export_zip_fixtures/zlib-corpus.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            let [name, kind, seed, length, crc, _settings, compressed] = fields[..] else { panic!("bad line {line}") };
            let payload = generate(kind, seed.parse().unwrap(), length.parse().unwrap());
            assert_eq!(
                format!("{:08x}", bitwise_crc32(&payload)),
                crc,
                "{name}: the generator drifted from the script"
            );
            Sample { name: name.to_string(), payload, compressed: hex(compressed) }
        })
        .collect()
}

enum Expect {
    /// Decodes to the end of its final block: length and CRC-32 of the output.
    Done(usize, u32),
    /// Runs out of input after this much output.
    Cut(usize, u32),
    Fail(InflateError),
}

#[test]
fn zlibs_own_vectors_decode_or_fail_exactly_as_zlib_says() {
    // zlib test/infcover.c: every raw-inflate `try()` vector (the error ones name zlib's message,
    // which InflateError's Display repeats), then its `inf()` raw vectors evaluated with a 32 KiB
    // window. Outcomes measured with zlib 1.2.12 (CPython's zlib.decompressobj(-15)).
    use Expect::*;
    use InflateError::*;
    let vectors: &[(&str, &str, Expect)] = &[
        ("0 0 0 0 0", "invalid stored block lengths", Fail(InvalidStoredBlockLengths)),
        ("3 0", "fixed", Done(0, 0)),
        ("6", "invalid block type", Fail(InvalidBlockType)),
        ("1 1 0 fe ff 0", "stored", Done(1, 0xd202_ef8d)),
        ("fc 0 0", "too many length or distance symbols", Fail(TooManyLengthOrDistanceSymbols)),
        ("4 0 fe ff", "invalid code lengths set", Fail(InvalidCodeLengthsSet)),
        ("4 0 24 49 0", "invalid bit length repeat", Fail(InvalidBitLengthRepeat)),
        ("4 0 24 e9 ff ff", "invalid bit length repeat", Fail(InvalidBitLengthRepeat)),
        ("4 0 24 e9 ff 6d", "invalid code -- missing end-of-block", Fail(MissingEndOfBlock)),
        ("4 80 49 92 24 49 92 24 71 ff ff 93 11 0", "invalid literal/lengths set", Fail(InvalidLiteralLengthsSet)),
        ("4 80 49 92 24 49 92 24 f b4 ff ff c3 84", "invalid distances set", Fail(InvalidDistancesSet)),
        ("4 c0 81 8 0 0 0 0 20 7f eb b 0 0", "invalid literal/length code", Fail(InvalidLiteralLengthCode)),
        ("2 7e ff ff", "invalid distance code", Fail(InvalidDistanceCode)),
        ("c c0 81 0 0 0 0 0 90 ff 6b 4 0", "invalid distance too far back", Fail(InvalidDistanceTooFarBack)),
        ("5 c0 21 d 0 0 0 80 b0 fe 6d 2f 91 6c", "pull 17", Done(0, 0)),
        ("5 e0 81 91 24 cb b2 2c 49 e2 f 2e 8b 9a 47 56 9f fb fe ec d2 ff 1f", "long code", Done(0, 0)),
        ("ed c0 1 1 0 0 0 40 20 ff 57 1b 42 2c 4f", "length extra", Done(516, 0xac72_7b17)),
        (
            "ed cf c1 b1 2c 47 10 c4 30 fa 6f 35 1d 1 82 59 3d fb be 2e 2a fc f c",
            "long distance and extra",
            Done(518, 0x838d_92af),
        ),
        (
            "ed c0 81 0 0 0 0 80 a0 fd a9 17 a9 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 6",
            "window end",
            Done(33025, 0xf62a_fa2f),
        ),
        ("63 0", "force window allocation", Cut(1, 0xd202_ef8d)),
        ("63 18 5", "force window replacement", Cut(259, 0xc276_fc68)),
        ("63 18 68 30 d0 0 0", "force split window update", Done(257, 0xb7bf_a0ce)),
        ("2 8 20 80 0 3 0", "inflate_fast TYPE return", Done(0, 0)),
        ("63 18 5 40 c 0", "window wrap", Done(262, 0xb8ea_39f0)),
        (
            "e5 e0 81 ad 6d cb b2 2c c9 01 1e 59 63 ae 7d ee fb 4d fd b5 35 41 68 ff 7f 0f 0 0 0",
            "fast length extra bits",
            Fail(InvalidDistanceTooFarBack),
        ),
        (
            "25 fd 81 b5 6d 59 b6 6a 49 ea af 35 6 34 eb 8c b9 f6 b9 1e ef 67 49 50 fe ff ff 3f 0 0",
            "fast distance extra bits",
            Fail(InvalidDistanceTooFarBack),
        ),
        ("3 7e 0 0 0 0 0", "fast invalid distance code", Fail(InvalidDistanceCode)),
        ("1b 7 0 0 0 0 0", "fast invalid literal/length code", Fail(InvalidLiteralLengthCode)),
        (
            "d c7 1 ae eb 38 c 4 41 a0 87 72 de df fb 1f b8 36 b1 38 5d ff ff 0",
            "fast 2nd level codes and too far back",
            Fail(InvalidDistanceTooFarBack),
        ),
        ("63 18 5 8c 10 8 0 0 0 0", "very common case", Done(266, 0x0061_4fd9)),
        (
            "63 60 60 18 c9 0 8 18 18 18 26 c0 28 0 29 0 0 0",
            "contiguous and wrap around window",
            Done(270, 0x8b0e_98b7),
        ),
        ("63 0 3 0 0 0 0 0", "copy direct from output", Done(6, 0xb1c2_a1a3)),
    ];
    for (input, what, expect) in vectors {
        let Inflated { output, status } = inflate::inflate_partial(&spaced_hex(input), usize::MAX);
        match *expect {
            Done(length, crc) => {
                assert_eq!(status, Ok(()), "{what}");
                assert_eq!((output.len(), bitwise_crc32(&output)), (length, crc), "{what}");
            }
            Cut(length, crc) => {
                assert_eq!(status, Err(InflateError::Truncated), "{what}");
                assert_eq!((output.len(), bitwise_crc32(&output)), (length, crc), "{what}");
            }
            Fail(error) => {
                assert_eq!(status, Err(error), "{what}");
                if what.starts_with("invalid") || what.starts_with("too many") {
                    assert_eq!(error.to_string(), *what, "zlib's message");
                }
            }
        }
    }
}

#[test]
fn hand_assembled_blocks_follow_rfc_1951() {
    // An empty fixed block, an empty stored block, zlib's raw deflate of "hello".
    assert_eq!(inflate::inflate(&[0x03, 0x00], 10), Ok(vec![]));
    assert_eq!(inflate::inflate(&[0x01, 0x00, 0x00, 0xFF, 0xFF], 10), Ok(vec![]));
    assert_eq!(inflate::inflate(&spaced_hex("cb 48 cd c9 c9 07 00"), 10), Ok(b"hello".to_vec()));
    // A non-final stored block, then a final fixed block: blocks chain, and the stored block's
    // padding bits are skipped.
    let mut two = vec![0x00, 0x02, 0x00, 0xFD, 0xFF, b'a', b'b'];
    two.extend_from_slice(&deflate_raw(b"c"));
    assert_eq!(inflate::inflate(&two, 10), Ok(b"abc".to_vec()));
    // Bytes after the final block are ignored, as zlib ignores them.
    let mut trailing = deflate_raw(b"hello");
    trailing.extend_from_slice(b"\xFFgarbage");
    assert_eq!(inflate::inflate(&trailing, 10), Ok(b"hello".to_vec()));
    // An overlapping back-reference repeats its period: "ab" + (length 7, distance 2).
    assert_eq!(inflate::inflate(&deflate_raw(b"abababababa"), 20), Ok(b"abababababa".to_vec()));
}

#[test]
fn every_stream_zlib_wrote_inflates_byte_exactly() {
    let samples = corpus();
    assert_eq!(samples.len(), 23);
    for sample in &samples {
        let output = inflate::inflate(&sample.compressed, sample.payload.len());
        assert!(output.as_deref() == Ok(sample.payload.as_slice()), "{}: {:?}", sample.name, output.map(|o| o.len()));
    }
}

#[test]
fn the_output_never_passes_the_limit() {
    // One byte short of the stream is an error, never a silently short buffer, and what came out
    // is a true prefix.
    for sample in corpus().iter().filter(|sample| !sample.payload.is_empty()) {
        let length = sample.payload.len();
        for limit in [0, 1, length / 2, length - 1] {
            let Inflated { output, status } = inflate::inflate_partial(&sample.compressed, limit);
            assert_eq!(status, Err(InflateError::OutputLimitExceeded), "{} at {limit}", sample.name);
            assert!(output.len() <= limit && sample.payload.starts_with(&output), "{} at {limit}", sample.name);
        }
        assert_eq!(inflate::inflate(&sample.compressed, length + 1).map(|o| o.len()), Ok(length));
    }
}

#[test]
fn a_cut_stream_is_truncated_never_invalid() {
    // Every prefix of a valid stream is a prefix of valid bits, so the only honest answer is
    // "truncated", with a true prefix of the output — the distinction the zip reader's
    // acceptance rule rests on. Sampled cut points plus every one of the last 24 bytes.
    for sample in corpus() {
        let full = sample.compressed.len();
        let step = (full / 48).max(1);
        let cuts = (0..full).step_by(step).chain(full.saturating_sub(24)..full);
        for cut in cuts {
            let Inflated { output, status } = inflate::inflate_partial(&sample.compressed[..cut], usize::MAX);
            assert_eq!(status, Err(InflateError::Truncated), "{} cut at {cut}", sample.name);
            assert!(sample.payload.starts_with(&output), "{} cut at {cut}", sample.name);
        }
    }
}

#[test]
fn the_suites_own_encoder_writes_streams_zlib_reads() {
    // The fixtures' encoder (one fixed-Huffman block, greedy LZ77) round-trips through the
    // decoder; where python3 is installed, zlib itself confirms the streams are standard.
    let mut inputs: Vec<Vec<u8>> =
        vec![vec![], b"a".to_vec(), vec![0; 70_000], b"abcABC123 the quick brown fox ".repeat(40)];
    inputs.extend(corpus().into_iter().map(|sample| sample.payload).filter(|payload| payload.len() <= 30_000));
    for input in &inputs {
        let stream = deflate_raw(input);
        assert_eq!(inflate::inflate(&stream, input.len()).as_ref(), Ok(input));
        if let Some(decoded) = python_zlib_inflate(&stream) {
            assert_eq!(&decoded, input, "zlib disagrees with the test encoder");
        }
    }
    for count in [0u64, 1, 2, 3, 258, 259, 260, 261, 100_000] {
        assert_eq!(inflate::inflate(&deflate_zeros(count), usize::MAX), Ok(vec![0; count as usize]));
    }
}

fn python_zlib_inflate(stream: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("python3")
        .args(["-I", "-c", "import sys, zlib; sys.stdout.buffer.write(zlib.decompress(sys.stdin.buffer.read(), -15))"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(stream).ok()?;
    let output = child.wait_with_output().ok()?;
    assert!(output.status.success(), "zlib refused a stream the test encoder wrote");
    Some(output.stdout)
}

#[test]
fn the_zip_reader_takes_exactly_the_declared_bytes() {
    // What DeflateStream and Apple's decoder both do with a stream that runs out of input right
    // after its last byte — before its final end-of-block code: the declared bytes are there, so
    // the entry reads. Short of them, past them, or corrupt after them, it does not.
    let unfinished = [0x00, 0x05, 0x00, 0xFA, 0xFF, b'h', b'e', b'l', b'l', b'o']; // a non-final stored block
    let read = |stream: &[u8], claimed: u32| {
        zip_reader::entries(&deflated_precompressed("text.md", stream, claimed, 0, 8), None).map(|e| e[0].data.clone())
    };
    assert_eq!(read(&unfinished, 5), Some(b"hello".to_vec()));
    assert_eq!(read(&unfinished, 4), None, "the stream carries more than declared");
    assert_eq!(read(&unfinished, 6), None, "the stream stops short");
    let mut corrupt_after = unfinished.to_vec();
    corrupt_after.push(0x06); // block type 3
    assert_eq!(read(&corrupt_after, 5), None);
    let mut finished = unfinished.to_vec();
    finished.extend_from_slice(&[0x03, 0x00]); // an empty final fixed block
    assert_eq!(read(&finished, 5), Some(b"hello".to_vec()));
    finished.extend_from_slice(b"trailing bytes are not the stream's");
    assert_eq!(read(&finished, 5), Some(b"hello".to_vec()));
    // A stored block cut inside its payload is short of five declared bytes — but when exactly
    // the three that are there were declared, it reads, as it does through DeflateStream.
    assert_eq!(read(&unfinished[..8], 5), None);
    assert_eq!(read(&unfinished[..8], 3), Some(b"hel".to_vec()));
}

#[test]
fn a_tiny_archive_claiming_the_cap_is_refused_without_reserving_it() {
    // Eight bytes claiming 128 MiB: the decoder's buffer starts at what eight bytes could
    // possibly expand to, and the claim fails when the stream ends.
    let stream = deflate_zeros(1000);
    let claim = zip_reader::MAX_ENTRY_SIZE as u32;
    assert_eq!(zip_reader::entries(&deflated_precompressed("text.md", &stream[..8], claim, 0, 8), None), None);
    let Inflated { output, .. } = inflate::inflate_partial(&stream[..8], zip_reader::MAX_ENTRY_SIZE);
    assert!(output.capacity() <= 8 * 1032 + 258, "reserved {}", output.capacity());
}

#[test]
fn every_error_names_itself_as_zlib_does() {
    let names = [
        (InflateError::Truncated, "unexpected end of stream"),
        (InflateError::OutputLimitExceeded, "output limit exceeded"),
        (InflateError::MissingEndOfBlock, "invalid code -- missing end-of-block"),
    ];
    for (error, name) in names {
        assert_eq!(error.to_string(), name);
    }
}

/// Mutate `stream` in one of several ways chosen by `random`.
fn mutated(stream: &[u8], random: &mut Lcg) -> Vec<u8> {
    let mut out = stream.to_vec();
    match random.below(6) {
        0 if !out.is_empty() => {
            for _ in 0..=random.below(3) {
                let at = random.below(out.len());
                out[at] ^= 1 << random.below(8);
            }
        }
        1 if !out.is_empty() => {
            let at = random.below(out.len());
            out[at] = random.next() as u8;
        }
        2 => out.truncate(random.below(out.len() + 1)),
        3 => {
            let at = random.below(out.len() + 1);
            let noise: Vec<u8> = (0..=random.below(8)).map(|_| random.next() as u8).collect();
            out.splice(at..at, noise);
        }
        4 if !out.is_empty() => {
            let from = random.below(out.len());
            let to = (from + random.below(16)).min(out.len());
            out.drain(from..to);
        }
        _ => {
            for byte in out.iter_mut().skip(random.below(stream.len().max(1))).take(4) {
                *byte = !*byte;
            }
        }
    }
    out
}

#[test]
fn hostile_streams_never_panic_and_never_outgrow_the_limit() {
    // A deterministic sweep: thousands of mutated zlib streams and pure noise, through the
    // decoder and through the zip reader around it. No input may panic, hang, or produce more
    // than the limit; whatever does decode cleanly decodes the same way twice.
    let mut random = Lcg(0x5EED_1951);
    let samples = corpus();
    for sample in &samples {
        let limit = sample.payload.len() + 64;
        for _ in 0..120 {
            let stream = mutated(&sample.compressed, &mut random);
            let first = inflate::inflate_partial(&stream, limit);
            assert!(first.output.len() <= limit, "{}", sample.name);
            if first.status.is_ok() {
                assert_eq!(inflate::inflate_partial(&stream, limit), first);
            }
            let archive = deflated_precompressed("text.md", &stream, sample.payload.len() as u32, 0, 8);
            let _ = zip_reader::entries(&mutated(&archive, &mut random), None);
        }
    }
    for length in 0..400 {
        let noise: Vec<u8> = (0..length).map(|_| random.next() as u8).collect();
        let Inflated { output, .. } = inflate::inflate_partial(&noise, 4096);
        assert!(output.len() <= 4096);
        let _ = zip_reader::entries(&noise, None);
    }
}
