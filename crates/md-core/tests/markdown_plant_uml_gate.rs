//! Port of md.win `tests/Md.Core.Tests/PlantUmlGateTests.cs` (md.vscode's
//! `test/plantuml-limit.test.ts`): the one deliberate patch inside a vendored engine, pinned.
//!
//! The browser build of PlantUML (1.2026.4beta4) compares a laid-out diagram's width and height
//! against `4096.0` and throws the drawing away past either — "Diagram too large for browser
//! rendering: 7655x1445 (max 4096)" — so a 48-participant sequence diagram never rendered. md's
//! consumers take the SVG itself (preview, HTML, SVG, PDF) or the source (LaTeX); the EPUB snapshot
//! draws the element's laid-out rect, which the stylesheet caps with
//! `.plantuml svg { max-width: 100% }`, so the gate defended a raster budget nothing here can
//! overspend. Both literals are raised to 1.0E9, exactly as md.vscode did — the patched
//! `rich/plantuml.js` is byte-identical to the extension's and to md.win's.
//!
//! A re-vendored engine would bring the gate back silently. This test fails the day that happens,
//! by design: the pinned fragment is minifier output a fresh build will not reproduce. Re-apply
//! with `perl -pi -e 's/o<=4096\.0\)\{p=m\.bDm;if\(p<=4096\.0\)/o<=1.0E9){p=m.bDm;if(p<=1.0E9)/'
//! rich/plantuml.js` (adjusting the identifiers to the new build's), re-verify in a browser,
//! update the pin.

const PATCHED_GATE: &[u8] = b"o<=1.0E9){p=m.bDm;if(p<=1.0E9)";

/// `rich/plantuml.js` at the repository root, as bytes (md.win reads it as Latin-1; every needle
/// here is ASCII, so bytes are the same search).
fn engine() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rich/plantuml.js");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

#[test]
fn holds_the_raised_gate_once() {
    assert_eq!(occurrences(&engine(), PATCHED_GATE), 1);
}

#[test]
fn holds_no_4096_pixel_comparison() {
    // `4096` alone appears legitimately (palette names, Unicode tables); the gate compared doubles,
    // and no other `4096.0` exists in the build.
    assert_eq!(occurrences(&engine(), b"4096.0"), 0);
}
