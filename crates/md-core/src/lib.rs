//! md's parity core — the Rust port of md.win's `Md.Core`, itself pinned byte for byte to
//! md.macOS through the fixtures every md port shares (`tests/fixtures`). No GTK, and no I/O
//! beyond what a module names; the GTK app and md-logic code against this crate.

pub mod book;
pub mod document;
pub mod editing;
pub mod export;
pub mod markdown;
pub mod plot;
pub mod text;
