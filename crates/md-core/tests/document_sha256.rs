//! The hand-written SHA-256 behind the view-mode memory's file identity, against the published
//! vectors (FIPS 180-2 Appendix B and the NIST CAVP short-message set), including every padding
//! edge: 55, 56, 63, 64 and 65 bytes, where the length field does or does not fit the last block.

use md_core::document::sha256;

fn hex(data: &[u8]) -> String {
    sha256::hex_lower(&sha256::digest(data))
}

#[test]
fn fips_180_2_appendix_b_vectors() {
    // B.1: one block.
    assert_eq!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", hex(b"abc"));
    // B.2: two blocks (56 bytes — the length no longer fits the first).
    assert_eq!(
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
    );
    // B.3: one million 'a'.
    assert_eq!("cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0", hex(&vec![b'a'; 1_000_000]));
}

#[test]
fn the_empty_message_and_the_896_bit_vector() {
    assert_eq!("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", hex(b""));
    assert_eq!(
        "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
        hex(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu")
    );
    assert_eq!(
        "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592",
        hex(b"The quick brown fox jumps over the lazy dog")
    );
}

#[test]
fn nist_cavp_short_messages() {
    // SHA256ShortMsg.rsp, Len = 8 and Len = 16.
    assert_eq!("28969cdfa74a12c82f3bad960b0b000aca2ac329deea5c2328ebc6f2ba9802c1", hex(&[0xd3]));
    assert_eq!("5ca7133fa735326081558ac312c620eeca9970d1e70a4b95533d956f072d1f98", hex(&[0x11, 0xaf]));
}

#[test]
fn padding_boundaries() {
    // The message of n bytes 0x00, 0x01, … (n - 1) around the padding edges: 55 leaves room for the
    // 0x80 and the length in one block, 56–63 push the length into a second, 64 is a whole block
    // plus a padding block, 119/120 repeat the edge one block later. Digests from Python's hashlib.
    let expected = [
        (55, "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59"),
        (56, "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562"),
        (63, "29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488"),
        (64, "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108"),
        (65, "4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781"),
        (119, "da18797ed7c3a777f0847f429724a2d8cd5138e6ed2895c3fa1a6d39d18f7ec6"),
        (120, "f52b23db1fbb6ded89ef42a23ce0c8922c45f25c50b568a93bf1c075420bbb7c"),
        (128, "471fb943aa23c511f6f72f8d1652d9c880cfa392ad80503120547703e56a2be5"),
    ];
    for (length, digest) in expected {
        let message: Vec<u8> = (0..length).map(|i: usize| i as u8).collect();
        assert_eq!(digest, hex(&message), "{length} bytes");
    }
}

#[test]
fn hex_is_lower_case() {
    assert_eq!("00ff10ab", sha256::hex_lower(&[0x00, 0xFF, 0x10, 0xAB]));
    assert_eq!("", sha256::hex_lower(&[]));
}
