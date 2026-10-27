//! Shared helpers for the integration tests: the fixtures every md port shares, copied byte for
//! byte from md.win's `tests/Md.Core.Tests/Fixtures`. Never edit a fixture to make a test pass.
#![allow(dead_code)]

use std::path::PathBuf;

/// Absolute path of `tests/fixtures/<relative>`.
pub fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join(relative)
}

/// The fixture's bytes.
pub fn fixture_bytes(relative: &str) -> Vec<u8> {
    std::fs::read(fixture_path(relative)).unwrap_or_else(|e| panic!("fixture {relative}: {e}"))
}

/// The fixture as UTF-8 text.
pub fn fixture_text(relative: &str) -> String {
    String::from_utf8(fixture_bytes(relative)).unwrap_or_else(|e| panic!("fixture {relative} is not UTF-8: {e}"))
}

/// Absolute path of the repository's `examples/` (the bundled examples, identical to md.macOS's).
pub fn examples_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("examples")
}
