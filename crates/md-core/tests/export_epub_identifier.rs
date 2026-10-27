//! The hand-written SHA-1 behind the EPUB's UUIDv5 identifier, against the published vectors
//! (FIPS 180-2 Appendix A and the NIST short/long messages), including every padding edge — 55,
//! 56, 63, 64 and 65 bytes, where the length field does or does not fit the last block — then the
//! identifier and the `dcterms:modified` stamp against oracles outside this family (Python's
//! `hashlib`, `uuid.uuid5(uuid.NAMESPACE_URL, name)` and `datetime`). Not a port of an md.win test:
//! md.win asks .NET for SHA-1 and for the calendar, and this port has to earn both.

#[path = "common/epub.rs"]
mod epub;

use std::time::{Duration, UNIX_EPOCH};

use epub::instant;
use md_core::export::epub_export;

fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha1(data: &[u8]) -> String {
    hex(&epub_export::sha1(data))
}

#[test]
fn fips_180_2_appendix_a_vectors() {
    // A.1: one block.
    assert_eq!("a9993e364706816aba3e25717850c26c9cd0d89d", sha1(b"abc"));
    // A.2: two blocks (56 bytes — the length no longer fits the first).
    assert_eq!(
        "84983e441c3bd26ebaae4aa1f95129e5e54670f1",
        sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
    );
    // A.3: one million 'a'.
    assert_eq!("34aa973cd4c4daa4f61eeb2bdbad27316534016f", sha1(&vec![b'a'; 1_000_000]));
}

#[test]
fn the_empty_message_the_896_bit_vector_and_the_pangram() {
    assert_eq!("da39a3ee5e6b4b0d3255bfef95601890afd80709", sha1(b""));
    assert_eq!(
        "a49b2446a02c645bf419f995b67091253a04a259",
        sha1(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")
    );
    assert_eq!("2fd4e1c67a2d28fced849ee1bb76e7391b93eb12", sha1(b"The quick brown fox jumps over the lazy dog"));
}

#[test]
fn every_padding_edge() {
    // hashlib.sha1(b"a" * n): the 0x80 byte and the 64-bit length either fit after the message
    // in its last block or spill into one more.
    for (length, expected) in [
        (55, "c1c8bbdc22796e28c0e15163d20899b65621d65a"),
        (56, "c2db330f6083854c99d4b5bfb6e8f29f201be699"),
        (63, "03f09f5b158a7a8cdad920bddc29b81c18a551f5"),
        (64, "0098ba824b5c16427bd7a1122a5a442a25ec644d"),
        (65, "11655326c708d70319be2610e8a57d9a5b959d3b"),
        (119, "ee971065aaa017e0632a8ca6c77bb3bf8b1dfc56"),
        (120, "f34c1488385346a55709ba056ddd08280dd4c6d6"),
        (128, "ad5b3fdbcb526778c2839d2f151ea753995e26a0"),
    ] {
        assert_eq!(expected, sha1(&vec![b'a'; length]), "{length} bytes");
    }
}

#[test]
fn stable_identifier_matches_python_for_long_and_non_ascii_titles() {
    // uuid.uuid5(uuid.NAMESPACE_URL, name): a name that pushes the hash input past one block, the
    // German sharp s, an astral emoji (four UTF-8 bytes, a surrogate pair in md.win), CJK, and a
    // four-block name.
    for (title, expected) in [
        (
            "A Considerably Longer Book Title That Spans Two SHA-1 Blocks Of Input",
            "urn:uuid:14e02348-11f1-56ab-87cd-1d808f953258",
        ),
        ("Stra\u{DF}e", "urn:uuid:43911992-cd1e-5e78-95b0-fa78838024d1"),
        ("\u{1F642} Emoji Book", "urn:uuid:91758bfd-9d7b-5feb-81fa-c238990b79cd"),
        ("\u{65E5}\u{672C}\u{8A9E}\u{306E}\u{672C}", "urn:uuid:706ab9a4-6f0e-58c1-9957-4939a371b207"),
        (&"x".repeat(200), "urn:uuid:cd3580b9-d244-586a-8077-74317319b48d"),
    ] {
        assert_eq!(expected, epub_export::stable_identifier(title), "{title:?}");
    }
}

#[test]
fn modified_walks_the_calendar_as_python_does() {
    // datetime.fromtimestamp(ts, timezone.utc): the epoch, one second before it, both sides of a
    // 400-year leap day, the 2100 non-leap day, and the two ends of .NET's DateTimeOffset range.
    for (seconds, expected) in [
        (0i64, "1970-01-01T00:00:00Z"),
        (-1, "1969-12-31T23:59:59Z"),
        (951_868_799, "2000-02-29T23:59:59Z"),
        (951_868_800, "2000-03-01T00:00:00Z"),
        (4_107_542_400, "2100-03-01T00:00:00Z"),
        (-62_135_596_800, "0001-01-01T00:00:00Z"),
        (253_402_300_799, "9999-12-31T23:59:59Z"),
        (1_000_000_000, "2001-09-09T01:46:40Z"),
    ] {
        let when = if seconds >= 0 {
            UNIX_EPOCH + Duration::from_secs(seconds.unsigned_abs())
        } else {
            UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
        };
        assert_eq!(expected, epub_export::modified(when), "{seconds}");
    }
}

#[test]
fn a_fraction_of_a_second_is_dropped_towards_the_past() {
    // `ToString("ss")` truncates the tick count, and before the epoch that is still towards the
    // past: half a second before midnight is 23:59:59, never 00:00:00.
    assert_eq!("1969-12-31T23:59:59Z", epub_export::modified(UNIX_EPOCH - Duration::from_millis(500)));
    assert_eq!("1970-01-01T00:00:00Z", epub_export::modified(UNIX_EPOCH + Duration::from_millis(999)));
    assert_eq!("2026-07-24T00:00:00Z", epub_export::modified(instant(2026, 7, 24, 0, 0, 0, 999, 0)));
}
