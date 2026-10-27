//! Port of md.win `tests/Md.Core.Tests/HtmlAdversarialTests.cs`: adversarial pins for the HTML
//! writer, written during md.win's review from the ports themselves — never from an
//! implementation's output. Each expectation names where it came from: the Swift oracle
//! (`md.macOS/md/*.swift` compiled with `swiftc` and driven over the case), the TypeScript oracle
//! (`md.vscode/src/render/{html,inline}.ts` bundled with esbuild and driven from node), or the
//! Kotlin source (`md.Android/.../markdown/MarkdownHtml.kt`), all measured on 2026-09-06.
//!
//! Two of these record a place where the four ports do **not** agree, and say which side this one
//! takes. The goldens were captured from md.vscode, which agrees with the Swift on everything the
//! fixtures contain, and the disagreements below sit outside the fixtures in both directions.
//! Neither is a bug to be quietly "fixed" on one platform.

mod common;

use md_core::markdown::html::{self, EngineNeeds};

fn inline(text: &str) -> String {
    html::inline(text, false)
}

fn body(source: &str) -> String {
    html::body(source, "t", false).html
}

const ACUTE: &str = "\u{0301}"; // COMBINING ACUTE ACCENT (Mn, GCB=Extend)
const ZWNJ: &str = "\u{200C}"; // ZERO WIDTH NON-JOINER (Cf, GCB=Extend)
const DOTTED_I: &str = "\u{0130}"; // LATIN CAPITAL LETTER I WITH DOT ABOVE
const COMBINING_DOT: &str = "\u{0307}"; // COMBINING DOT ABOVE
const SIGMA: &str = "\u{03A3}"; // GREEK CAPITAL LETTER SIGMA
const SMALL_SIGMA: &str = "\u{03C3}"; // GREEK SMALL LETTER SIGMA
const FINAL_SIGMA: &str = "\u{03C2}"; // GREEK SMALL LETTER FINAL SIGMA
const TOKEN_OPEN: &str = "\u{E000}";
const TOKEN_CLOSE: &str = "\u{E001}";

// MARK: - Escaping is ordinal, never grapheme-aware

/// `escape` and the soft-break conversion replace CODE UNITS (here: scalars). Swift does not:
/// Foundation's `String.replacingOccurrences(of:with:)` matches whole grapheme clusters under
/// canonical equivalence, so an `&`, `<`, `>`, `"` or `\n` immediately followed by a combining mark
/// or a joiner is one cluster, is not equal to the bare character, and is left ALONE — measured on
/// the Swift oracle, `[a](x"{acute}y)` renders as `<a href="x"{acute}y">a</a>`, a raw quote that
/// closes the `href` early. TypeScript and Kotlin both work on code units and escape all of these,
/// and so does this port: deliberately NOT bug-compatible with the Swift, which would mean shipping
/// a raw `"` into an attribute value on purpose. No fixture contains any of these sequences, so the
/// goldens are silent — which is exactly why this test exists.
#[test]
fn escape_is_ordinal_never_grapheme_aware() {
    // TS oracle: "a&lt;́b" / "a&gt;́b" / "a&amp;́b" / "a&quot;́b".
    assert_eq!(inline(&format!("a<{ACUTE}b")), format!("a&lt;{ACUTE}b"));
    assert_eq!(inline(&format!("a>{ACUTE}b")), format!("a&gt;{ACUTE}b"));
    assert_eq!(inline(&format!("a&{ACUTE}b")), format!("a&amp;{ACUTE}b"));
    assert_eq!(inline(&format!("a\"{ACUTE}b")), format!("a&quot;{ACUTE}b"));
    // A joiner is GCB=Extend too, so it forms a cluster the same way.
    assert_eq!(inline(&format!("a<{ZWNJ}b")), format!("a&lt;{ZWNJ}b"));
    // Inside a code span, whose content is escaped by the same routine.
    assert_eq!(inline(&format!("`a<{ACUTE}b`")), format!("<code>a&lt;{ACUTE}b</code>"));
    // The attribute case, where it actually matters.
    assert_eq!(inline(&format!("[a](x\"{ACUTE}y)")), format!("<a href=\"x&quot;{ACUTE}y\">a</a>"));
    // And the soft-break pass, which is the same kind of literal replace.
    assert_eq!(html::inline(&format!("a\n{ACUTE}b"), true), format!("a<br>\n{ACUTE}b"));
    // A block-level render carries it too.
    assert_eq!(body(&format!("para a<{ACUTE}b end")), format!("<p>para a&lt;{ACUTE}b end</p>"));
}

/// Restore replaces the protection token by ordinal comparison, so a format character following
/// the span cannot swallow it. Swift oracle: `$$x`y`z$$` followed by a ZWNJ collapses to
/// `{U+E000}1{U+E001}{ZWNJ}` — the whole formula is gone, not merely leaked. TS oracle and this
/// port both give the span with the documented inner leak. Removing the ZWNJ makes all three agree,
/// which is what the second assertion pins.
#[test]
fn token_restore_is_ordinal_so_a_format_character_cannot_swallow_it() {
    let leaked = format!("<span class=\"md-mathd\">x{TOKEN_OPEN}0{TOKEN_CLOSE}z</span>");
    assert_eq!(inline(&format!("$$x`y`z$${ZWNJ}")), format!("{leaked}{ZWNJ}"));
    // Swift, TypeScript and this port all agree once the joiner is gone.
    assert_eq!(inline("$$x`y`z$$ w"), format!("{leaked} w"));
}

// MARK: - Folding the fence's info word

/// The info word is folded with the FULL Unicode lowercase mapping, not the simple 1:1 one. U+0130
/// lowercases to two code points, `i` + U+0307, in `SpecialCasing.txt`; all three other ports do
/// that and all three oracles agree on `class="language-i{U+0307}stanbul"`. The simple mapping
/// leaves U+0130 alone, which is why `full_lowercase` — the routine the slug already uses — has to
/// be asked here. No locale is consulted either way.
#[test]
fn fence_language_is_folded_with_swifts_full_unicode_mapping() {
    assert_eq!(
        body(&format!("```{DOTTED_I}stanbul\nbody\n```")),
        format!("<pre><code class=\"language-i{COMBINING_DOT}stanbul\">body</code></pre>")
    );
    assert_eq!(
        body(&format!("```T\u{00DC}RK{DOTTED_I}YE\ncode\n```")),
        format!("<pre><code class=\"language-t\u{00FC}rki{COMBINING_DOT}ye\">code</code></pre>")
    );
    // A dotted I cannot change which branch is taken, only the class it lands in.
    assert_eq!(
        html::body(&format!("```{DOTTED_I}stanbul\nbody\n```"), "t", false).needs,
        EngineNeeds::new(false, false, false, false, true)
    );
    // U+212A KELVIN SIGN folds to a plain "k" in every port.
    assert_eq!(body("```\u{212A}\nbody\n```"), "<pre><code class=\"language-k\">body</code></pre>");
}

/// The fold is scalar by scalar: the Greek Final_Sigma context rule is NOT applied. Swift maps
/// each scalar on its own, so a trailing U+03A3 lowercases to U+03C3 and never to U+03C2.
/// JavaScript's `toLowerCase` and Java's `toLowerCase(Locale.ROOT)` both DO apply the rule, so
/// md.vscode emits `{sigma}{final sigma}` here — a genuine three-way split settled in favour of
/// md.macOS, the source of truth. No fixture reaches it.
#[test]
fn fence_language_folding_never_applies_the_final_sigma_rule() {
    assert_eq!(
        body(&format!("```{SIGMA}{SIGMA}\nbody\n```")),
        format!("<pre><code class=\"language-{SMALL_SIGMA}{SMALL_SIGMA}\">body</code></pre>")
    );
    assert_eq!(
        body(&format!("```{SIGMA}a{SIGMA}\nbody\n```")),
        format!("<pre><code class=\"language-{SMALL_SIGMA}a{SMALL_SIGMA}\">body</code></pre>")
    );
    assert!(!body(&format!("```{SIGMA}{SIGMA}\nbody\n```")).contains(FINAL_SIGMA));
}

// MARK: - The engine probes, asked directly

/// The five probe strings, spelled exactly as `MarkdownHTML.document` spells them, asked of the
/// public `needs` rather than through a rendered body. Every other engine test drives `body`, so
/// the probe LITERALS are never pinned there: dropping the closing `>` from the Mermaid probe
/// leaves the rest green. The HTML export hands `needs` markup that came back out of a web view,
/// where a processed container reads `<pre class="mermaid" data-processed="true">` — which the
/// Swift probe rejects and a truncated one would accept.
#[test]
fn engine_probes_are_the_exact_container_strings_swift_spells() {
    let off = EngineNeeds::NONE;

    // Mermaid and PlantUML carry their closing angle bracket; an attribute defeats them.
    assert_eq!(html::needs("<pre class=\"mermaid\">x</pre>"), EngineNeeds { mermaid: true, ..off });
    assert_eq!(html::needs("<pre class=\"mermaid\" data-processed=\"true\">x</pre>"), off);
    assert_eq!(html::needs("<div class=\"plantuml\">x</div>"), EngineNeeds { plantuml: true, ..off });
    assert_eq!(html::needs("<div class=\"plantuml\" data-md-src=\"eA==\">x</div>"), off);

    // Graphviz is the one probe that stops before the angle, because `data-engine` varies.
    assert_eq!(
        html::needs("<div class=\"graphviz\" data-engine=\"circo\">x</div>"),
        EngineNeeds { graphviz: true, ..off }
    );
    assert_eq!(html::needs("<div class=\"graphviz\">x</div>"), EngineNeeds { graphviz: true, ..off });

    // Highlight keys off the tag pair as well as the class prefix.
    assert_eq!(
        html::needs("<pre><code class=\"language-swift\">x</code></pre>"),
        EngineNeeds { highlight: true, ..off }
    );
    assert_eq!(html::needs("<pre> <code class=\"language-swift\">x</code></pre>"), off);
    assert_eq!(html::needs("<code class=\"language-swift\">x</code>"), off);

    // Math is the bare class name, which is why prose naming it pulls KaTeX in.
    assert_eq!(html::needs("md-mathi"), EngineNeeds { math: true, ..off });
    assert_eq!(html::needs("md-mathd"), EngineNeeds { math: true, ..off });

    // The probes are ordinal and case-sensitive.
    assert_eq!(html::needs("<PRE CLASS=\"MERMAID\">x</PRE>"), off);
    assert_eq!(html::needs("MD-MATHI"), off);
    assert_eq!(html::needs(""), off);
}

// MARK: - Culture

/// md.win runs this under four `CurrentCulture`s (tr-TR, de-DE, ar-SA, fa-IR) because .NET
/// formatting and casing consult it: the list indent is a `double` formatted with "F2", which a
/// German culture would write `1,60` (and the browser would drop the rule), and the fence's info
/// word is folded, which a Turkish culture would spell differently. Rust has no ambient culture —
/// nothing in md-core reads a locale — so the four runs are the same run; they are kept, named, so
/// the case list matches md.win's. The expectation is the golden corpus itself, anchored to
/// md.vscode's recorded bytes rather than to a second call of the same code.
#[test]
fn every_fixture_renders_to_its_golden_under_any_current_culture() {
    for _culture in ["tr-TR", "de-DE", "ar-SA", "fa-IR"] {
        for (directory, name) in [
            ("testdata", "lists"),
            ("testdata", "test"),
            ("testdata", "tables"),
            ("testdata", "code"),
            ("testdata", "math"),
            ("examples", "09-Writer Tools"),
        ] {
            let source = common::fixture_text(&format!("{directory}/{name}.md"));
            let expected = common::fixture_text(&format!("golden/{directory}/{name}.html"));
            assert!(
                html::body(&source, name, false).html + "\n" == expected,
                "{directory}/{name} differs from its golden"
            );
        }

        // The indent is the case a culture would break first, and a deep one is not in any fixture.
        assert!(body("- a\n  - b").contains("padding-left:1.60em"));
        assert!(body("- a\n      - d").contains("padding-left:4.80em"));

        // The stylesheets are byte-identical too.
        assert_eq!(common::fixture_text("golden/stylesheet-light.css"), html::css(false, false) + "\n");
        assert_eq!(common::fixture_text("golden/stylesheet-export.css"), html::css(false, true) + "\n");

        // A Turkish locale must not fold the info word with its own I rules: `LATEX` still reaches
        // the math branch (Swift oracle: `<div class="md-mathd">x</div>`) and `DOT` still reaches
        // the dot layout engine.
        assert_eq!(body("```LATEX\nx\n```"), "<div class=\"md-mathd\">x</div>");
        assert_eq!(body("```DOT\ndigraph {}\n```"), "<div class=\"graphviz\" data-engine=\"dot\">digraph {}</div>");
    }
}
