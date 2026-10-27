//! The name a save dialog is opened with. Port of md.win `ExportFileNamesTests.cs`.
//!
//! Two layers, tested apart. `portable_stem` is the family rule (macOS
//! DocumentExport.sanitized, Android Exporter.sanitized, md.vscode sanitized, md.win
//! PortableStem) and must stay byte-identical with those. `sanitized` adds what Linux refuses —
//! NUL, and the names "." and ".." — and nothing else: md.win's Windows layer (control
//! characters, trailing dots and spaces, device names) is not Linux's, and the Mac, the family's
//! source of truth, has none of it. md.win's three Windows-layer tests are ported to that
//! decision under new names (noted on each), asserting what this port produces instead.
//!
//! The macOS suite has no test of its own for `sanitized` (it is a private helper of
//! DocumentExport), so these vectors are read straight off the implementations and off the
//! shapes the export paths compose: "<title>.<ext>" everywhere and "<title>-<ordinal+1>.svg" for a
//! diagram.

use md_core::export::export_file_names::{FALLBACK, portable_stem, sanitized};

const FORBIDDEN: [char; 10] = ['/', '\\', ':', '?', '%', '*', '|', '"', '<', '>'];

// MARK: - The family rule

#[test]
fn portable_stem_replaces_every_character_of_the_family_set() {
    for c in FORBIDDEN {
        assert_eq!(portable_stem(&format!("a{c}b")), "a-b");
    }
    assert_eq!(portable_stem("Report/2026"), "Report-2026");
    // It is a split-join, so a run of two offending characters becomes TWO dashes, not one -
    // pinned in the md.vscode port and true of all of them.
    assert_eq!(portable_stem("a<>b"), "a--b");
    assert_eq!(portable_stem("///"), "---");
    assert_eq!(portable_stem("/"), "-");
    // Characters no port touches, including a line break: the family rule leaves them, because
    // no other platform minds.
    assert_eq!(portable_stem("a\nb"), "a\nb");
    assert_eq!(portable_stem("Q1 (final) #2 & co."), "Q1 (final) #2 & co.");
}

#[test]
fn portable_stem_trims_and_falls_back_to_document() {
    assert_eq!(portable_stem("  Draft notes\n"), "Draft notes");
    assert_eq!(portable_stem(""), "Document");
    assert_eq!(portable_stem("   "), "Document");
    assert_eq!(portable_stem("\r\n\t "), "Document");
    assert_eq!(FALLBACK, "Document");
}

#[test]
fn the_trim_is_foundations_whitespace_not_dotnets() {
    // Foundation's frozen tables still call U+200B a space separator, so macOS trims it and
    // `str::trim` would not. A title pasted with zero-width padding must not name the file with
    // an invisible character.
    assert_eq!(portable_stem("\u{200B}Draft\u{200B}"), "Draft");
    assert_eq!(portable_stem("\u{200B}"), "Document");
    // U+180E is NOT in the set (Cf since Unicode 6.3, and Foundation agrees).
    assert_eq!(portable_stem("\u{180E}Draft"), "\u{180E}Draft");
    // The newline half of the set is trimmed too (U+0085 NEL, U+2028 LS).
    assert_eq!(portable_stem("\u{0085}Draft\u{2028}"), "Draft");
}

// MARK: - Composing a file name

#[test]
fn sanitized_composes_the_stem_and_the_extension() {
    assert_eq!(sanitized("Draft", "svg"), "Draft.svg");
    // Given with or without its leading dot, and any number of them.
    assert_eq!(sanitized("Draft", ".svg"), "Draft.svg");
    assert_eq!(sanitized("Draft", "..svg"), "Draft.svg");
    // No extension at all: no trailing dot.
    assert_eq!(sanitized("Draft", ""), "Draft");
    assert_eq!(sanitized("Draft", "."), "Draft");
    assert_eq!(sanitized("Draft", "   "), "Draft");
    // The extension gets the same character rule, and the same trim.
    assert_eq!(sanitized("Draft", "sv/g"), "Draft.sv-g");
    assert_eq!(sanitized("Draft", "svg "), "Draft.svg");
    assert_eq!(sanitized("", "pdf"), "Document.pdf");
    // Every extension the export paths use, on a title that needs cleaning.
    assert_eq!(sanitized("Report: Q1/2026", "pdf"), "Report- Q1-2026.pdf");
    for extension in ["pdf", "html", "epub", "tex", "svg", "textbundle"] {
        assert_eq!(sanitized("Draft", extension), format!("Draft.{extension}"));
    }
}

#[test]
fn sanitized_names_a_diagram_export_one_based() {
    // The shape the SVG export composes: "<title>-<ordinal + 1>.svg".
    let ordinal = 2;
    assert_eq!(sanitized(&format!("Notes| 2026-{}", ordinal + 1), "svg"), "Notes- 2026-3.svg");
}

// MARK: - The Linux layer

#[test]
fn sanitized_replaces_nul_and_keeps_other_control_characters() {
    // md.win: SanitizedDropsControlCharactersTheFamilyRuleKeeps. Windows refuses U+0000–U+001F
    // in a file name; Linux refuses only NUL, which no C string (GTK's included) can carry. It
    // becomes the same dash the family set becomes. Every other control character is a legal
    // Linux name, and the Mac keeps them, so this port does too.
    assert_eq!(portable_stem("a\nb"), "a\nb");
    assert_eq!(sanitized("a\nb", "svg"), "a\nb.svg");
    assert_eq!(sanitized("a\u{0001}b", "svg"), "a\u{0001}b.svg");
    assert_eq!(sanitized("a\r\nb", "svg"), "a\r\nb.svg");
    assert_eq!(sanitized("a\u{007F}b", "svg"), "a\u{007F}b.svg");
    assert_eq!(sanitized("a\0b", "svg"), "a-b.svg");
    assert_eq!(sanitized("a\0\0b", "svg"), "a--b.svg");
    assert_eq!(sanitized("\0", "svg"), "-.svg");
    assert_eq!(sanitized("Draft", "s\0g"), "Draft.s-g");
    // The family rule is the family's: it knows nothing about NUL.
    assert_eq!(portable_stem("a\0b"), "a\0b");
}

#[test]
fn sanitized_keeps_dots_like_the_mac() {
    // md.win: SanitizedDropsTrailingDots. A trailing dot is a Windows restriction; Linux and the
    // Mac take "Notes....svg" as it is. Only the names "." and ".." — reachable with no
    // extension — are refused, and fall back to Document.
    assert_eq!(sanitized("Notes...", "svg"), "Notes....svg");
    assert_eq!(sanitized("...", "svg"), "....svg");
    assert_eq!(sanitized(". . .", "svg"), ". . ..svg");
    // A leading dot is kept: `.keep` is an ordinary name.
    assert_eq!(sanitized(".keep", "svg"), ".keep.svg");
    // Interior dots are untouched.
    assert_eq!(sanitized("v1.2.3", "svg"), "v1.2.3.svg");
    assert_eq!(sanitized(".", ""), "Document");
    assert_eq!(sanitized("..", ""), "Document");
    assert_eq!(sanitized(" .. ", "."), "Document");
    assert_eq!(sanitized("...", ""), "...");
    assert_eq!(sanitized(".", "svg"), "..svg");
}

#[test]
fn sanitized_has_no_reserved_device_names() {
    // md.win: SanitizedEscapesTheReservedDeviceNames. CON, PRN, AUX, NUL, COM1–9 and LPT1–9 are
    // devices only on Windows; on Linux (and the Mac) they are ordinary names and keep the
    // user's casing untouched.
    assert_eq!(sanitized("CON", "svg"), "CON.svg");
    assert_eq!(sanitized("con", "svg"), "con.svg");
    assert_eq!(sanitized("CoN", "svg"), "CoN.svg");
    assert_eq!(sanitized("CON.old", "txt"), "CON.old.txt");
    assert_eq!(sanitized("CON ", "svg"), "CON.svg");
    for device in ["CON", "PRN", "AUX", "NUL"] {
        assert_eq!(sanitized(device, "svg"), format!("{device}.svg"));
    }
    for n in 1..=9 {
        assert_eq!(sanitized(&format!("COM{n}"), "svg"), format!("COM{n}.svg"));
        assert_eq!(sanitized(&format!("LPT{n}"), "svg"), format!("LPT{n}.svg"));
    }
    for ordinary in ["COM10", "COM0", "LPT0", "CONS", "CONSOLE", "NULL", "AU", "COM"] {
        assert_eq!(sanitized(ordinary, "svg"), format!("{ordinary}.svg"));
    }
    assert_eq!(portable_stem("CON"), "CON");
}

// MARK: - Culture

#[test]
fn sanitized_is_independent_of_the_current_culture() {
    // md.win runs this on a thread under tr-TR, where an invariant upper-case would turn "i" into
    // a dotted capital. Rust has no ambient culture and this port folds nothing, so the outputs
    // are simply pinned.
    assert_eq!(sanitized("con", "svg"), "con.svg");
    assert_eq!(sanitized("Istanbul I i", "svg"), "Istanbul I i.svg");
}

#[test]
fn the_title_itself_is_never_case_folded() {
    // The name the user sees is their own text.
    assert_eq!(sanitized("\u{0130}stanbul", "svg"), "\u{0130}stanbul.svg");
    assert_eq!(sanitized("Grosse Strasse", "epub"), "Grosse Strasse.epub");
}
