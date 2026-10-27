//! Port of md.win `tests/Md.Core.Tests/MarkdownHtmlTests.cs`: the HTML writer, case for case from
//! mdTests.swift (the testHTML…, testFootnote…, testRaw…, testCSV…, testGraphvizInkRules…
//! families and the HTML halves of the straddling front-matter tests), plus the pins the UTF-16
//! ports added — md.vscode's html.test.ts / inline.test.ts, which spell ICU's regex classes out,
//! and the Kotlin suite's positive halves — and md.win's own divergence pins (a word guard asked
//! of the whole code point, the measured `\s` set, ICU's `.`).
//!
//! Every string assertion is ordinal. Non-ASCII characters are built from their code points so
//! that no invisible byte can hide in this file.

use std::collections::HashSet;

use md_core::markdown::html::{self, EngineNeeds, GRAPHVIZ_ENGINES};
use md_core::markdown::{ColumnAlignment, MarkdownBlock, MetadataField, parser};
use md_core::text::whitespace;

fn doc(source: &str) -> String {
    html::document(source, "t", false, false)
}

fn doc_with(source: &str, dark: bool, export: bool) -> String {
    html::document(source, "t", dark, export)
}

fn body(source: &str) -> String {
    html::body(source, "t", false).html
}

fn needs(source: &str) -> EngineNeeds {
    html::body(source, "t", false).needs
}

fn inline(text: &str) -> String {
    html::inline(text, false)
}

fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

#[track_caller]
fn has(html: &str, needle: &str) {
    assert!(html.contains(needle), "expected {needle:?} in:\n{html}");
}

#[track_caller]
fn lacks(html: &str, needle: &str) {
    assert!(!html.contains(needle), "did not expect {needle:?} in:\n{html}");
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

const ACUTE: &str = "\u{0301}"; // COMBINING ACUTE ACCENT
const ZWJ: &str = "\u{200D}"; // ZERO WIDTH JOINER
const ZWNJ: &str = "\u{200C}"; // ZERO WIDTH NON-JOINER
const BOM: &str = "\u{FEFF}"; // ZERO WIDTH NO-BREAK SPACE
const NBSP: &str = "\u{00A0}"; // NO-BREAK SPACE
const ZWSP: &str = "\u{200B}"; // ZERO WIDTH SPACE
const NEL: &str = "\u{0085}"; // NEXT LINE
const LINE_SEPARATOR: &str = "\u{2028}";
const SUPER_TWO: &str = "\u{00B2}"; // SUPERSCRIPT TWO (No)
const HALF: &str = "\u{00BD}"; // VULGAR FRACTION ONE HALF (No)
const ROMAN_TEN: &str = "\u{2169}"; // ROMAN NUMERAL TEN (Nl)
const ARABIC_THREE: &str = "\u{0663}"; // ARABIC-INDIC DIGIT THREE (Nd)
const EF: &str = "\u{0444}"; // CYRILLIC SMALL LETTER EF
const E_ACUTE: &str = "\u{00E9}";
const MATH_BOLD_A: &str = "\u{1D400}"; // MATHEMATICAL BOLD CAPITAL A (astral, L)
const CIRCLED_A: &str = "\u{24B6}"; // CIRCLED LATIN CAPITAL LETTER A (So, Other_Alphabetic)
const GRINNING: &str = "\u{1F600}";
const TOKEN_OPEN: &str = "\u{E000}";
const TOKEN_CLOSE: &str = "\u{E001}";

// MARK: - The document skeleton

#[test]
fn html_wraps_document() {
    let html = html::document("# Title", "Doc", false, false);
    assert!(html.starts_with("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n"));
    has(&html, "<title>Doc</title>");
    has(&html, "<h1 id=\"title\">Title</h1>");
    has(&html, "<body data-md-dark=\"0\">");
    has(&html, "<script type=\"module\" src=\"rich/md-init.js\"></script>\n</body>\n</html>");
    assert!(html.ends_with("</html>"));
}

#[test]
fn html_escapes_the_title_and_never_sniffs_it() {
    // Always the caller's string: nothing from an H1 or from front matter.
    has(&html::document("# Real Heading", "a & <b>", false, false), "<title>a &amp; &lt;b&gt;</title>");
    lacks(&html::document("---\ntitle: Front\n---\n\n# Real", "given", false, false), "<title>Front");
}

#[test]
fn html_glues_the_head_includes_onto_style_and_ends_at_html() {
    // An empty head yields `</style>\n</head>`; a math document reads `…</style><link …>` on one
    // line; there is no trailing newline.
    let plain = doc("hi");
    has(&plain, "</style>\n</head>");
    let maths = doc("$a+b$");
    has(
        &maths,
        "</style><link rel=\"stylesheet\" href=\"rich/katex.min.css\">\n<script defer src=\"rich/katex.min.js\"></script>\n<script defer src=\"rich/mhchem.min.js\"></script>\n</head>",
    );
    assert!(maths.ends_with("</html>"));
}

#[test]
fn html_leaves_a_blank_line_where_an_empty_sources_body_would_be() {
    has(&doc(""), "<body data-md-dark=\"0\">\n\n<script type=\"module\"");
    assert_eq!(body(""), "");
}

#[test]
fn html_escapes_special_characters() {
    has(&doc("a < b & c > d"), "a &lt; b &amp; c &gt; d");
    // No raw-HTML passthrough and no autolinks anywhere: both render as their own text.
    assert_eq!(body("<b>hi</b>"), "<p>&lt;b&gt;hi&lt;/b&gt;</p>");
    assert_eq!(body("An autolink: <https://nettrash.me>"), "<p>An autolink: &lt;https://nettrash.me&gt;</p>");
}

#[test]
fn escape_is_exactly_four_characters_ampersand_first() {
    assert_eq!(html::escape("&<>\""), "&amp;&lt;&gt;&quot;");
    assert_eq!(html::escape("a & b"), "a &amp; b");
    assert_eq!(html::escape("it's"), "it's");
    // Astral characters pass through whole.
    assert_eq!(
        html::escape(&format!("{MATH_BOLD_A} & {GRINNING}")).replace("&amp;", "&"),
        format!("{MATH_BOLD_A} & {GRINNING}")
    );
    // The whole XSS story: the URL class stops at the first `)`, and `"` is escaped, so a URL
    // cannot break out of its href. The stray paren is left as text.
    assert_eq!(inline("[a](\"onerror=alert(1))"), "<a href=\"&quot;onerror=alert(1\">a</a>)");
}

#[test]
fn html_omits_author_notes() {
    let html = doc("visible\n\n<!-- note: secret draft thought -->");
    has(&html, "visible");
    lacks(&html, "secret draft thought");
}

#[test]
fn html_plain_document_stays_light() {
    let html = doc("# Just text\n\nA paragraph.");
    lacks(&html, "katex.min.js");
    lacks(&html, "mermaid.min.js");
    lacks(&html, "viz-global.js");
    lacks(&html, "highlight.min.js");
    has(&html, "rich/md-init.js");
}

#[test]
fn html_theme_variants_differ() {
    let light = doc("hi");
    let dark = doc_with("hi", true, false);
    assert_ne!(light, dark);
    has(&dark, "color-scheme: dark");
    // Backgrounds must be forced to print so the theme survives to PDF.
    has(&dark, "print-color-adjust: exact");
    has(&dark, "background: #241E18");
    has(&dark, "data-md-dark=\"1\"");
}

#[test]
fn export_page_is_plain_white_and_always_light() {
    let export = doc_with("hello", true, true);
    has(&export, "background: #FFFFFF");
    has(&export, "color-scheme: light");
    lacks(&export, "#241E18");
    // The rich renderers (Mermaid's theme, PlantUML's flag) see the light mode too.
    has(&export, "data-md-dark=\"0\"");
    let preview = doc_with("hello", true, false);
    has(&preview, "background: #241E18");
    has(&preview, "data-md-dark=\"1\"");
}

#[test]
fn html_page_break_marker_and_export_css() {
    let preview = doc("a\n\n\\newpage\n\nb");
    has(&preview, "md-pagebreak");
    lacks(&preview, "break-after: page");
    has(&preview, ".md-pagebreak { border-top: 2px dashed rgba(43,38,32,0.16); margin: 1.6em 0; }");
    let export = doc_with("a\n\n\\newpage\n\nb", false, true);
    has(&export, ".md-pagebreak { height: 0; margin: 0; break-after: page; }");
}

#[test]
fn html_export_keeps_page_breaks_visible() {
    // The rule collapses; the marker stays, so the HTML file export can swap the rule back.
    let export = doc_with("a\n\n\\newpage\n\nb", false, true);
    has(&export, "break-after: page");
    has(&export, "<div class=\"md-pagebreak\"></div>");
}

#[test]
fn yaml_front_matter_is_hidden_from_the_html() {
    // The HTML half of testYAMLFrontMatterIsParsedAndHidden: the opening `---` must not survive as
    // a rule and the metadata must not survive as prose.
    let html = doc(&lines(&[
        "---",
        "title: My Book",
        "author: nettrash",
        "date: 2026-07-24",
        "---",
        "",
        "# Chapter One",
        "",
        "Text.",
    ]));
    lacks(&html, "My Book");
    lacks(&html, "<hr>");
    has(&html, "Chapter One");
}

#[test]
fn front_matter_does_not_steal_the_first_headings_anchor() {
    // The HTML half of testFrontMatterDoesNotLeakIntoTheOutlineOrNotes: the closing `---`
    // underlines the last metadata line, and a scanner reading it as a setext heading would push
    // every real anchor out of step with the outline.
    has(&doc("---\ntitle: My Post\n---\n\n# Hello\n"), "id=\"hello\"");
    has(&doc("---\nHello: x\n---\n\n# Hello: x\n"), "id=\"hello-x\"");
}

#[test]
fn html_headings_carry_anchor_ids() {
    let html = doc("# My Title\n\n# My Title");
    has(&html, "<h1 id=\"my-title\">");
    has(&html, "<h1 id=\"my-title-1\">");
    // Heading text goes through the inline pass; the slug is taken from the raw text.
    assert_eq!(body("# 50% **off**"), "<h1 id=\"50-off\">50% <strong>off</strong></h1>");
    // A quoted heading gets no id and consumes no counter.
    assert_eq!(
        body("> ## Same\n\n## Same"),
        lines(&["<blockquote>", "<h2>Same</h2>", "</blockquote>", "<h2 id=\"same\">Same</h2>"])
    );
}

// MARK: - Inline spans

#[test]
fn html_inline_emphasis() {
    let html = doc("**bold** and *italic* and ~~gone~~");
    has(&html, "<strong>bold</strong>");
    has(&html, "<em>italic</em>");
    has(&html, "<del>gone</del>");
    assert_eq!(inline("__bold__ and _italic_"), "<strong>bold</strong> and <em>italic</em>");
    // Bold before italic, so `***` nests; "no delimiter inside" content classes.
    assert_eq!(inline("***x***"), "<em><strong>x</strong></em>");
    lacks(&inline("**a*b**"), "<strong>");
    // Every pattern is a replace-all.
    assert_eq!(inline("*a* and *b* and *c*"), "<em>a</em> and <em>b</em> and <em>c</em>");
    assert_eq!(inline("`x` and `y`"), "<code>x</code> and <code>y</code>");
}

#[test]
fn html_code_span_is_escaped_and_not_reinterpreted() {
    let html = doc("`a < *b* > c`");
    has(&html, "<code>a &lt; *b* &gt; c</code>");
    lacks(&html, "<em>b</em>");
}

#[test]
fn html_code_span_dollar_is_not_math() {
    has(&doc("use `$x$` here"), "<code>$x$</code>");
    assert_eq!(inline("use `$x$` here"), "use <code>$x$</code> here");
}

#[test]
fn html_code_span_is_restored_literally_dollars_and_all() {
    // Restore is a literal replace; a regex replace would read `$$` as a template.
    assert_eq!(inline("`$$`"), "<code>$$</code>");
    assert_eq!(inline("`a$&b`"), "<code>a$&amp;b</code>");
    assert_eq!(inline("`$1 $2`"), "<code>$1 $2</code>");
}

#[test]
fn html_link() {
    has(&doc("[site](https://nettrash.me)"), "<a href=\"https://nettrash.me\">site</a>");
}

#[test]
fn html_link_with_title() {
    has(
        &doc("[site](https://nettrash.me \"Hover title\")"),
        "<a href=\"https://nettrash.me\" title=\"Hover title\">site</a>",
    );
}

#[test]
fn html_image() {
    // A void tag, not self-closed.
    has(
        &doc("![Alt text](https://nettrash.me/favicon.ico)"),
        "<img src=\"https://nettrash.me/favicon.ico\" alt=\"Alt text\">",
    );
}

#[test]
fn html_image_with_title() {
    // Attribute order is src, alt, title.
    has(
        &doc("![Alt](https://nettrash.me/favicon.ico \"The favicon\")"),
        "<img src=\"https://nettrash.me/favicon.ico\" alt=\"Alt\" title=\"The favicon\">",
    );
}

#[test]
fn html_linked_image() {
    // The image pass runs before the link pass, so the <img> nests inside the <a>.
    has(
        &doc("[![badge](https://nettrash.me/favicon.ico)](https://nettrash.me)"),
        "<a href=\"https://nettrash.me\"><img src=\"https://nettrash.me/favicon.ico\" alt=\"badge\"></a>",
    );
}

#[test]
fn html_titled_forms_run_before_untitled_and_alt_may_be_empty() {
    // Otherwise the untitled pattern stops at the first `)` and the title leaks as text.
    lacks(&inline("[a](u \"t\")"), "&quot;t&quot;)");
    assert_eq!(inline("![](x.png)"), "<img src=\"x.png\" alt=\"\">");
    // A link label may not be empty.
    assert_eq!(inline("[](x)"), "[](x)");
}

#[test]
fn html_underscore_in_word_is_not_italic() {
    lacks(&doc("call some_long_name now"), "<em>");
    lacks(&inline("a_b_c is one word"), "<em>");
}

#[test]
fn html_inline_math_is_not_mangled_by_emphasis() {
    let html = doc("total $a*b*c$ units");
    has(&html, "class=\"md-mathi\"");
    has(&html, "a*b*c");
    lacks(&html, "<em>");
    has(&html, "katex.min.js");
    assert_eq!(inline("total $a*b*c$ units"), "total <span class=\"md-mathi\">a*b*c</span> units");
}

#[test]
fn html_display_math_span_preserved() {
    let html = doc("$$x^2 + y^2$$");
    has(&html, "class=\"md-mathd\"");
    has(&html, "x^2 + y^2");
    assert_eq!(inline("$$x^2 + y^2$$"), "<span class=\"md-mathd\">x^2 + y^2</span>");
    assert_eq!(inline("\\[x^2\\]"), "<span class=\"md-mathd\">x^2</span>");
    assert_eq!(inline("A \\(a_i\\) here."), "A <span class=\"md-mathi\">a_i</span> here.");
    // The LaTeX is escaped for HTML; KaTeX reads the decoded textContent.
    assert_eq!(inline("$a < b$"), "<span class=\"md-mathi\">a &lt; b</span>");
    // The bracket form keeps its inner spaces (golden math.html).
    assert_eq!(inline("\\[ \\sum_{k=1}^{n} k \\]"), "<span class=\"md-mathd\"> \\sum_{k=1}^{n} k </span>");
}

#[test]
fn html_currency_dollars_are_not_math() {
    let html = doc("it costs $5 and $10 today");
    has(&html, "$5 and $10");
    lacks(&html, "class=\"md-mathi\"");
    lacks(&html, "katex.min.js");
    assert_eq!(inline("it costs $5 and $10 today"), "it costs $5 and $10 today");
    // A dollar glued to a word character on either side, or a newline inside, is prose.
    for prose in [
        "a$x$b".to_string(),
        "5$x$".to_string(),
        "$x$b".to_string(),
        format!("{ACUTE}$x$"),
        "$x\ny$".to_string(),
        "a $$ b".to_string(),
    ] {
        lacks(&inline(&prose), "md-math");
    }
    assert_eq!(inline("total $a+b$ units"), "total <span class=\"md-mathi\">a+b</span> units");
    // A failed close is retried as an opener, exactly as the regex would.
    assert_eq!(inline("$a$b $c$"), "$a$b <span class=\"md-mathi\">c</span>");
}

#[test]
fn unicode_spacing_and_word_boundaries_agree_across_platforms() {
    // `\w` means Unicode here, through ICU: a Cyrillic letter is a word character, so an
    // underscore between letters is not emphasis and a dollar between letters is not a formula.
    let cyrillic = doc(&format!("{EF}_em_{EF} and {EF}$x${EF}"));
    lacks(&cyrillic, "<em>");
    lacks(&cyrillic, "<span class=\"md-mathi\">");
    // …and the other half (Kotlin's `underscoreItalicGuardIsUnicodeAware`): the patterns must
    // still MATCH, or a silently broken guard passes every negative assertion above.
    let working = doc("an _em_ word and $x$ alone");
    has(&working, "<em>em</em>");
    has(&working, "<span class=\"md-mathi\">x</span>");
}

#[test]
fn icu_word_class_is_spelled_out() {
    // ICU `\w` = Alphabetic + M + Nd + Pc + ZWNJ/ZWJ. A non-ASCII digit, a number-letter, a
    // combining mark and the joiners are word characters… Every row below was measured against
    // NSRegularExpression's own `(?<![\w$])\$([^$\n]+?)\$(?![\w$])` (2026-09-06): Nd, Nl, Mn, ZWJ,
    // ZWNJ, an enclosed Latin letter and an astral letter block the math; No (`²`, `½`) and an
    // emoji do not.
    lacks(&inline(&format!("{ARABIC_THREE}$x$")), "md-mathi");
    lacks(&inline(&format!("{ROMAN_TEN}$x$")), "md-mathi");
    for joiner in [ACUTE, ZWJ, ZWNJ] {
        lacks(&inline(&format!("e{joiner}$x$")), "md-mathi");
        lacks(&inline(&format!("e{joiner}_x_ y")), "<em>");
    }
    // …while `²` and `½` (No) are not — so `²$x$` IS a formula here, where both LaTeX writers
    // (`[\p{L}\p{N}_]`) and Kotlin's HTML (`[\p{L}\p{N}_$]`) read it as prose. Recorded, not
    // resolved (port spec OQ-3); this port follows the Swift HTML writer, whose bytes the golden
    // corpus was captured from.
    has(&inline(&format!("{SUPER_TWO}$x$")), "md-mathi");
    has(&inline(&format!("{HALF}$x$")), "md-mathi");
    // NFD `é`: the mark before the `$` is a word character, so this is prose on Apple and a formula
    // through `[\p{L}\p{N}_]` — the other half of OQ-3.
    lacks(&inline(&format!("e{ACUTE}$x$")), "md-mathi");
    lacks(&inline(&format!("{E_ACUTE}$x$")), "md-mathi");
}

#[test]
fn word_guard_is_asked_of_the_whole_code_point() {
    // A .NET regex lookbehind would see a low surrogate before the `$` — not a word character —
    // and typeset `𝐀$x$`; ICU and TypeScript (`u` flag) see an astral letter and leave it as prose.
    lacks(&inline(&format!("{MATH_BOLD_A}$x$")), "md-mathi");
    lacks(&inline(&format!("$x${MATH_BOLD_A}")), "md-mathi");
    lacks(&inline(&format!("{MATH_BOLD_A}_x_{MATH_BOLD_A}")), "<em>");
    // `Other_Alphabetic` outside L/M/Nl: the circled Latin letters (So) are Alphabetic to ICU.
    lacks(&inline(&format!("{CIRCLED_A}$x$")), "md-mathi");
    // An emoji is neither: `😀$x$` is a formula on every platform.
    has(&inline(&format!("{GRINNING}$x$")), "md-mathi");
}

#[test]
fn icu_space_class_is_the_measured_set() {
    // ICU `\s` = [\t\n\v\f\r U+0085 \p{Z}]: U+FEFF and U+200B are allowed inside a URL, NBSP, NEL
    // and VT end it. Measured against NSRegularExpression itself, `^[^)\s]+$` over `x<c>y`
    // (2026-09-06): TAB, NEL, VT, FF and NBSP end the URL; ZWSP and BOM do not.
    assert_eq!(inline(&format!("[a](x{BOM}y)")), format!("<a href=\"x{BOM}y\">a</a>"));
    assert_eq!(inline(&format!("[a](x{ZWSP}y)")), format!("<a href=\"x{ZWSP}y\">a</a>"));
    lacks(&inline(&format!("[a](x{NBSP}y)")), "<a href");
    lacks(&inline(&format!("[a](x{NEL}y)")), "<a href");
    lacks(&inline("[a](x\u{000B}y)"), "<a href");
    // The title separator is the same class: NEL separates a URL from its title.
    assert_eq!(inline(&format!("[a](u{NEL}\"t\")")), "<a href=\"u\" title=\"t\">a</a>");
}

#[test]
fn icu_dot_excludes_every_line_terminator_in_a_title() {
    // The title's `(.*?)` compiles without dotall; ICU's `.` stops at all seven terminators, not
    // only `\n` as .NET's would. A title holding NEL, LS or VT is therefore no title — and with the
    // space in it the untitled pattern cannot match either, so nothing links.
    for terminator in [NEL, LINE_SEPARATOR, "\u{000B}", "\u{000C}"] {
        lacks(&inline(&format!("[a](u \"x{terminator}y\")")), "<a href");
        lacks(&inline(&format!("![a](u \"x{terminator}y\")")), "<img");
    }
    assert_eq!(inline("[a](u \"x y\")"), "<a href=\"u\" title=\"x y\">a</a>");
}

#[test]
fn delimiter_runs_resolve_the_way_the_regexes_would() {
    // The hand-written scanners must agree with the patterns they stand in for, including where a
    // delimiter closes one span and opens the next, and where a guard fails and the scan resumes
    // one unit on.
    assert_eq!(inline("`a``b`"), "<code>a</code><code>b</code>");
    assert_eq!(inline("``a``"), "`<code>a</code>`");
    assert_eq!(inline("$a$$b$"), "$a$$b$");
    assert_eq!(inline("$$a$$b$$c$$"), "<span class=\"md-mathd\">a</span>b<span class=\"md-mathd\">c</span>");
    assert_eq!(inline("\\(a\\)\\(b\\)"), "<span class=\"md-mathi\">a</span><span class=\"md-mathi\">b</span>");
    assert_eq!(inline("a\\[b\\]c"), "a<span class=\"md-mathd\">b</span>c");
    assert_eq!(
        inline("*a* _b_ **c** __d__ ~~e~~ `f` $g$"),
        "<em>a</em> <em>b</em> <strong>c</strong> <strong>d</strong> <del>e</del> <code>f</code> <span class=\"md-mathi\">g</span>"
    );
    // A footnote id may be a single `_` or `-`; an empty alt with a blank URL is not an image.
    assert_eq!(inline("[^_]"), "<sup class=\"md-fnref\" data-fn=\"_\"></sup>");
    assert_eq!(inline("![](  )"), "![](  )");
}

#[test]
fn an_entity_spelled_out_in_the_source_cannot_forge_an_attribute() {
    // Phase 2 escapes the author's own `&`, so the `&quot;` the title patterns look for can only
    // come from a real `"` — a hand-written entity is inert text and the link is not one.
    assert_eq!(inline("&amp;"), "&amp;amp;");
    assert_eq!(inline("[a](u &quot;t&quot;)"), "[a](u &amp;quot;t&amp;quot;)");
    // Angle brackets in a URL are escaped where they land, inside the quoted href.
    assert_eq!(inline("[a](<u>)"), "<a href=\"&lt;u&gt;\">a</a>");
}

#[test]
fn restore_leaks_the_token_for_a_code_span_inside_display_math() {
    // KNOWN LATENT BUG, REPRODUCED DELIBERATELY: the enclosed code token is restored before its
    // enclosing math is back in the string, so U+E000 0 U+E001 survive (OQ-6; every port).
    assert_eq!(inline("$$ `x` $$"), format!("<span class=\"md-mathd\"> {TOKEN_OPEN}0{TOKEN_CLOSE} </span>"));
    // And the indices are assigned on a BACK-TO-FRONT walk, so the last match of a pass takes the
    // lowest index: Kotlin walks forward and would leak `0` then `1` here.
    assert_eq!(
        inline("$$ `a` `b` $$"),
        format!("<span class=\"md-mathd\"> {TOKEN_OPEN}1{TOKEN_CLOSE} {TOKEN_OPEN}0{TOKEN_CLOSE} </span>")
    );
}

#[test]
fn inline_converts_soft_breaks_only_when_the_paragraph_asks() {
    assert_eq!(inline("a\nb"), "a\nb");
    assert_eq!(html::inline("a\nb", true), "a<br>\nb");
    assert_eq!(body("one\ntwo"), "<p>one<br>\ntwo</p>");
    // Before restore, so a multi-line display formula keeps its own newlines.
    assert_eq!(body("$$a\nb$$"), "<p><span class=\"md-mathd\">a\nb</span></p>");
    // A hard break (two trailing spaces) keeps its spaces before the `<br>`.
    assert_eq!(body("x  \ny"), "<p>x  <br>\ny</p>");
}

// MARK: - Blocks

#[test]
fn renders_a_flat_list_never_a_ul_or_an_ol() {
    let html = body("- a\n  - b");
    assert_eq!(
        html,
        "<div class=\"md-list\"><div class=\"md-item\" style=\"padding-left:0.00em\"><span class=\"md-marker\">&bull;</span><span>a</span></div><div class=\"md-item\" style=\"padding-left:1.60em\"><span class=\"md-marker\">&bull;</span><span>b</span></div></div>"
    );
    lacks(&html, "<ul>");
    lacks(&html, "<ol>");
    // Two decimals, `.` separator, whatever the machine's locale.
    has(&body("- a\n    - b\n      - c"), "padding-left:3.20em");
}

#[test]
fn list_markers_task_beats_ordinal_and_ordinal_beats_bullet() {
    let html = body("1. [x] done\n2. plain");
    has(&html, "<div class=\"md-item done\" style=\"padding-left:0.00em\"><span class=\"md-marker\">&#9745;</span>");
    has(&html, "<span class=\"md-marker\">2.</span>");
    has(
        &body("- [ ] open"),
        "<div class=\"md-item\" style=\"padding-left:0.00em\"><span class=\"md-marker\">&#9744;</span>",
    );
    // An unordered item inside an ordered run falls back to a bullet; source numbers are kept.
    has(&body("1. one\n- two"), "<span class=\"md-marker\">&bull;</span><span>two</span>");
    has(&body("7. seven\n9. nine"), "<span class=\"md-marker\">9.</span>");
}

#[test]
fn html_table_alignments_and_cells() {
    let html = doc("| A | B |\n|:-:|--:|\n| 1 | 2 |");
    has(&html, "text-align:center");
    has(&html, "text-align:right");
    has(&html, "<td");
    assert_eq!(
        body("| A | B |\n|:-:|--:|\n| 1 | 2 |"),
        "<table><thead><tr><th style=\"text-align:center\">A</th><th style=\"text-align:right\">B</th></tr></thead><tbody><tr><td style=\"text-align:center\">1</td><td style=\"text-align:right\">2</td></tr></tbody></table>"
    );
}

#[test]
fn table_emits_tbody_with_no_rows_and_defaults_a_stray_column_to_left() {
    has(&body("| A |\n| --- |"), "<tbody></tbody>");
    let table = MarkdownBlock::table(vec!["A".into()], vec![], vec![vec!["1".into()]]);
    assert_eq!(
        html::render_block(&table),
        "<table><thead><tr><th style=\"text-align:left\">A</th></tr></thead><tbody><tr><td style=\"text-align:left\">1</td></tr></tbody></table>"
    );
    // A ragged row renders only its own cells — nothing is padded and nothing is dropped.
    let ragged = MarkdownBlock::table(
        vec!["A".into(), "B".into()],
        vec![ColumnAlignment::Trailing, ColumnAlignment::Center],
        vec![vec!["1".into()], vec!["2".into(), "3".into(), "4".into()]],
    );
    assert_eq!(
        html::render_block(&ragged),
        "<table><thead><tr><th style=\"text-align:right\">A</th><th style=\"text-align:center\">B</th></tr></thead><tbody><tr><td style=\"text-align:right\">1</td></tr><tr><td style=\"text-align:right\">2</td><td style=\"text-align:center\">3</td><td style=\"text-align:left\">4</td></tr></tbody></table>"
    );
    // Every cell goes through the inline pass.
    has(&body("| `a` |\n| --- |\n| *b* |"), "<th style=\"text-align:left\"><code>a</code></th>");
    has(&body("| `a` |\n| --- |\n| *b* |"), "<td style=\"text-align:left\"><em>b</em></td>");
}

#[test]
fn renders_the_simple_kinds() {
    assert_eq!(body("---"), "<hr>");
    assert_eq!(body("\\newpage"), "<div class=\"md-pagebreak\"></div>");
    assert_eq!(body("> quoted"), "<blockquote>\n<p>quoted</p>\n</blockquote>");
    assert_eq!(
        body("> a\n>> b"),
        lines(&["<blockquote>", "<p>a</p>", "<blockquote>", "<p>b</p>", "</blockquote>", "</blockquote>"])
    );
    assert_eq!(html::render_block(&MarkdownBlock::heading(3, "Inside")), "<h3>Inside</h3>");
}

#[test]
fn notes_front_matter_and_footnote_definitions_render_as_nothing() {
    for block in parser::parse("<!-- note: n -->") {
        assert_eq!(html::render_block(&block), "");
    }
    assert_eq!(html::render_block(&MarkdownBlock::footnote_definition("a", "text")), "");
    assert_eq!(html::render_block(&MarkdownBlock::front_matter(vec![MetadataField::new("k", "v")])), "");
    // Empty blocks still take part in the join and leave blank lines. Observable bytes.
    assert_eq!(body(&lines(&["---", "title: T", "---", "", "Body."])), "\n<p>Body.</p>");
    assert_eq!(
        body(&lines(&["before", "", "<!-- note: one -->", "", "<!-- note: two -->", "", "after"])),
        "<p>before</p>\n\n\n<p>after</p>"
    );
    has(&body(&lines(&["Text[^a].", "", "[^a]: The note."])), "</p>\n\n<section class=\"md-footnotes\">");
}

#[test]
fn render_blocks_joins_with_a_single_newline() {
    let blocks = parser::parse("a\n\n---\n\nb");
    assert_eq!(html::render_blocks(&blocks), "<p>a</p>\n<hr>\n<p>b</p>");
    assert_eq!(html::render_blocks(&[]), "");
}

// MARK: - Rich containers

#[test]
fn html_mermaid_block_emits_container() {
    let html = doc("```mermaid\ngraph TD\nA-->B\n```");
    has(&html, "<pre class=\"mermaid\">");
    has(&html, "graph TD");
    lacks(&html, "<pre><code>graph TD");
    has(&html, "mermaid.min.js");
    lacks(&html, "katex.min.js");
    lacks(&html, "viz-global.js");
    assert_eq!(body("```mermaid\ngraph TD\nA-->B\n```"), "<pre class=\"mermaid\">graph TD\nA--&gt;B</pre>");
}

#[test]
fn html_plantuml_block_emits_container() {
    let html = doc("```plantuml\n@startuml\nA->B\n@enduml\n```");
    has(&html, "<div class=\"plantuml\">");
    has(&html, "@startuml");
    has(&html, "viz-global.js");
    for language in ["plantuml", "puml", "plant-uml"] {
        assert_eq!(
            body(&format!("```{language}\n@startuml\n@enduml\n```")),
            "<div class=\"plantuml\">@startuml\n@enduml</div>"
        );
    }
}

#[test]
fn html_graphviz_block_emits_container() {
    let html = doc("```dot\ndigraph { a -> b }\n```");
    has(&html, "<div class=\"graphviz\" data-engine=\"dot\">");
    has(&html, "digraph { a -&gt; b }");
    lacks(&html, "<pre><code>digraph");
    has(&html, "viz-global.js");
    lacks(&html, "mermaid.min.js");
    lacks(&html, "katex.min.js");
}

#[test]
fn html_graphviz_aliases_and_layout_engines() {
    for alias in ["graphviz", "gv"] {
        has(&doc(&format!("```{alias}\ngraph {{ a -- b }}\n```")), "data-engine=\"dot\"");
    }
    for engine in ["neato", "circo", "fdp", "sfdp", "twopi", "osage", "patchwork"] {
        has(&doc(&format!("```{engine}\ngraph {{ a -- b }}\n```")), &format!("data-engine=\"{engine}\""));
    }
    // Ten keys, eight engines, and the mapped *value* reaches the attribute.
    assert_eq!(GRAPHVIZ_ENGINES.len(), 10);
    assert_eq!(GRAPHVIZ_ENGINES.iter().map(|(_, engine)| *engine).collect::<HashSet<_>>().len(), 8);
    for (word, engine) in GRAPHVIZ_ENGINES {
        assert_eq!(
            body(&format!("```{word}\ndigraph {{ a -> b }}\n```")),
            format!("<div class=\"graphviz\" data-engine=\"{engine}\">digraph {{ a -&gt; b }}</div>")
        );
    }
    // An unrelated language is a code block, never a diagram.
    let swift = doc("```swift\nlet x = 1\n```");
    has(&swift, "<pre><code class=\"language-swift\">let x = 1");
    lacks(&swift, "class=\"graphviz\"");
    lacks(&swift, "viz-global.js");
}

#[test]
fn html_graphviz_escapes_angle_brackets() {
    let html = doc("```dot\ndigraph { n [label=<<b>hi</b>>] }\n```");
    has(&html, "&lt;&lt;b&gt;hi&lt;/b&gt;&gt;");
    lacks(&html, "<b>hi</b>");
}

#[test]
fn html_math_fence_emits_display_math() {
    let html = doc("```math\n\\int_0^1 x\\,dx\n```");
    has(&html, "class=\"md-mathd\"");
    has(&html, "\\int_0^1");
    has(&html, "katex.min.js");
    // A fence is a `<div>`; `$$…$$` in a paragraph is a `<span>`.
    for language in ["math", "latex", "tex"] {
        assert_eq!(
            body(&format!("```{language}\n\\int_0^1 x\\,dx\n```")),
            "<div class=\"md-mathd\">\\int_0^1 x\\,dx</div>"
        );
    }
}

#[test]
fn html_code_language_emits_highlight_class_and_loads_engine() {
    let html = doc("```Swift\nlet x = 1\n```");
    has(&html, "<pre><code class=\"language-swift\">");
    has(&html, "<script defer src=\"rich/highlight.min.js\"></script>");
    assert_eq!(body("```Swift\nlet x = 1\n```"), "<pre><code class=\"language-swift\">let x = 1</code></pre>");
    // The class is the escaped info word; the content is escaped too, with no trailing newline.
    assert_eq!(
        body("~~~json\n{ \"tilde\": \"fence\" }\n~~~"),
        "<pre><code class=\"language-json\">{ &quot;tilde&quot;: &quot;fence&quot; }</code></pre>"
    );
}

#[test]
fn html_bare_fence_and_special_fences_are_not_highlighted() {
    let bare = doc("```\nplain text\n```");
    has(&bare, "<pre><code>plain text");
    lacks(&bare, "language-");
    lacks(&bare, "highlight.min.js");
    assert_eq!(body("```\nplain text\n```"), "<pre><code>plain text</code></pre>");
    let mermaid = doc("```mermaid\ngraph TD\nA-->B\n```");
    lacks(&mermaid, "highlight.min.js");
    lacks(&mermaid, "language-mermaid");
    lacks(&doc("```dot\ndigraph{a->b}\n```"), "highlight.min.js");
    lacks(&doc("```math\n\\int_0^1 x\\,dx\n```"), "highlight.min.js");
    lacks(&doc("```csv\na,b\n1,2\n```"), "highlight.min.js");
    let plot = doc("```plot\nx: -10..10\nsin(x)\n```");
    lacks(&plot, "highlight.min.js");
    lacks(&plot, "language-plot");
    has(&plot, "<div class=\"plot\"><svg");
}

#[test]
fn html_diagram_math_and_data_fences_are_not_highlighted() {
    // Kotlin's enumeration: the languages with their own handling are never tagged for hljs.
    for fence in [
        "```mermaid\ngraph TD\nA-->B\n```",
        "```dot\ndigraph { a -> b }\n```",
        "```plantuml\n@startuml\nA->B\n@enduml\n```",
        "```math\n\\int_0^1 x\\,dx\n```",
        "```csv\nName,Role\nAnn,Editor\n```",
        "```plot\nx: -10..10\nsin(x)\n```",
    ] {
        let html = doc(fence);
        lacks(&html, "class=\"language-");
        lacks(&html, "highlight.min.js");
    }
}

#[test]
fn html_plain_document_does_not_load_highlight_engine() {
    let html = doc("# Title\n\nJust prose, `inline code` aside.");
    lacks(&html, "highlight.min.js");
    lacks(&html, "language-");
}

#[test]
fn info_word_is_folded_with_the_invariant_culture() {
    has(&body("```MERMAID\ngraph TD\n```"), "<pre class=\"mermaid\">");
    has(&body("```DOT\ndigraph {}\n```"), "data-engine=\"dot\"");
    has(&body("```PLOT\nsin(x)\n```"), "<div class=\"plot\"><svg ");
    has(&body("```LATEX\nx\n```"), "<div class=\"md-mathd\">x</div>");
    has(&body("```TSV\na\tb\n1\t2\n```"), "<td style=\"text-align:right\">2</td>");
}

#[test]
fn plot_fence_holds_a_finished_svg_or_its_own_source_never_a_hole() {
    let html = body("```plot\nx: -1..1\nsin(x)\n```");
    assert!(html.starts_with("<div class=\"plot\"><svg "), "{html}");
    assert!(html.ends_with("</svg></div>"), "{html}");
    has(&html, "viewBox=\"0 0 600 400\"");
    lacks(&html, "<pre><code>");
    // A broken plot stays readable as its source inside the container.
    assert_eq!(
        body("```plot\nsinc(x)\n```"),
        "<div class=\"plot\"><pre>plot: unknown function 'sinc'\nsinc(x)</pre></div>"
    );
    assert_eq!(body("```plot\n```"), "<div class=\"plot\"></div>");
}

// MARK: - Engine gating

#[test]
fn needs_nothing_for_plain_prose_a_code_span_or_a_bare_fence() {
    assert_eq!(needs("# Title\n\nJust prose."), EngineNeeds::NONE);
    assert_eq!(needs("a `code span` here"), EngineNeeds::NONE);
    assert_eq!(needs("```\nplain\n```"), EngineNeeds::NONE);
    assert_eq!(needs("it costs $5 and $10 today"), EngineNeeds::NONE);
    assert_eq!(EngineNeeds::NONE.to_string(), "");
}

#[test]
fn needs_key_each_engine_off_its_own_container() {
    let off = EngineNeeds::NONE;
    assert_eq!(needs("$a+b$"), EngineNeeds { math: true, ..off });
    assert_eq!(needs("$$a+b$$"), EngineNeeds { math: true, ..off });
    assert_eq!(needs("```math\nx\n```"), EngineNeeds { math: true, ..off });
    assert_eq!(needs("```mermaid\ngraph TD\n```"), EngineNeeds { mermaid: true, ..off });
    assert_eq!(needs("```plantuml\n@startuml\n```"), EngineNeeds { plantuml: true, ..off });
    assert_eq!(needs("```dot\ndigraph {}\n```"), EngineNeeds { graphviz: true, ..off });
    assert_eq!(needs("```twopi\ndigraph {}\n```"), EngineNeeds { graphviz: true, ..off });
    assert_eq!(needs("```swift\nlet x = 1\n```"), EngineNeeds { highlight: true, ..off });
    assert_eq!(needs("```csv\na,b\n1,2\n```"), off);
    // The class is exactly `plot`: a plot-only document loads nothing, good or broken.
    assert_eq!(needs("```plot\nsin(x)\n```"), off);
    assert_eq!(needs("```plot\nsinc(x)\n```"), off);
    // The display form is the space-joined list in probe order.
    assert_eq!(EngineNeeds::new(true, true, true, true, true).to_string(), "math mermaid plantuml graphviz highlight");
    assert_eq!(EngineNeeds::new(false, true, false, false, true).to_string(), "mermaid highlight");
}

#[test]
fn needs_probe_the_emitted_markup_so_a_quoted_diagram_loads_its_engine() {
    // THE reason the gate reads markup and not the block list: `render_block` recurses.
    assert_eq!(needs("> ```dot\n> digraph { a }\n> ```"), EngineNeeds { graphviz: true, ..EngineNeeds::NONE });
    let quoted = doc("> ```dot\n> digraph { a }\n> ```");
    has(&quoted, "class=\"graphviz\"");
    has(&quoted, "rich/viz-global.js");
    let mermaid = doc("> ```mermaid\n> graph TD\n> ```");
    has(&mermaid, "<pre class=\"mermaid\">");
    has(&mermaid, "rich/mermaid.min.js");
    let plantuml = doc("> ```plantuml\n> @startuml\n> ```");
    has(&plantuml, "<div class=\"plantuml\">");
    has(&plantuml, "rich/viz-global.js");
    // A code block that merely quotes a container string cannot trigger it: it is escaped.
    assert_eq!(
        needs("```html\n<pre class=\"mermaid\">x</pre>\n```"),
        EngineNeeds { highlight: true, ..EngineNeeds::NONE }
    );
    assert_eq!(needs("```\n<div class=\"plantuml\">\n```"), EngineNeeds::NONE);
}

#[test]
fn needs_pull_katex_in_for_prose_that_merely_names_the_class() {
    // The escaping argument is FALSE for math: no `<` or `"` in the probe. Verified, harmless at
    // runtime, a real byte difference — reproduced on purpose.
    assert!(needs("The class md-mathd is mentioned in prose.").math);
    has(&doc("md-mathi"), "katex.min.js");
    assert!(html::needs("md-mathi").math);
    assert_eq!(html::needs("<pre class=\"mermaid \">"), EngineNeeds::NONE);
}

#[test]
fn raw_diagram_documents_hard_code_their_needs() {
    // No Markdown, so no math is possible even when the diagram's own text says so.
    let puml = html::body("@startuml\nnote: md-mathi\n@enduml\n", "t", false);
    assert_eq!(puml.needs, EngineNeeds::new(false, false, true, false, false));
    assert_eq!(puml.html, "<div class=\"plantuml\">@startuml\nnote: md-mathi\n@enduml\n</div>");
    let dot = html::body("digraph G {\n  a -> b; // md-mathd\n}\n", "t", false);
    assert_eq!(dot.needs, EngineNeeds::new(false, false, false, true, false));
    assert_eq!(
        dot.html,
        "<div class=\"graphviz\" data-engine=\"dot\">digraph G {\n  a -&gt; b; // md-mathd\n}\n</div>"
    );
}

#[test]
fn html_mhchem_loads_with_and_after_katex() {
    let math = doc("Reaction $\\ce{H2O}$ here");
    has(&math, "katex.min.js");
    has(&math, "mhchem.min.js");
    assert!(math.find("katex.min.js").unwrap() < math.find("mhchem.min.js").unwrap());
    // mhchem shares KaTeX's `defer`, or it would run before it and find no global to extend.
    has(&math, "<script defer src=\"rich/katex.min.js\"></script>");
    has(&math, "<script defer src=\"rich/mhchem.min.js\"></script>");
}

#[test]
fn html_mhchem_absent_without_math() {
    lacks(&doc("# Just text\n\nA paragraph."), "mhchem.min.js");
    lacks(&doc("```mermaid\ngraph TD\nA-->B\n```"), "mhchem.min.js");
    lacks(&doc("it costs $5 and $10 today"), "mhchem.min.js");
}

#[test]
fn head_includes_come_in_order_and_viz_is_shared_by_plantuml_and_dot() {
    for source in ["```plantuml\n@startuml\n```", "```dot\ndigraph {}\n```"] {
        let html = doc(source);
        has(&html, "</style>\n<script src=\"rich/viz-global.js\"></script>\n</head>");
        lacks(&html, "plantuml.js");
    }
    // Mermaid and Viz are not deferred; the full head for a document that needs everything.
    let all = doc(&lines(&[
        "$a$",
        "",
        "```mermaid",
        "graph TD",
        "```",
        "",
        "```dot",
        "digraph {}",
        "```",
        "",
        "```js",
        "x",
        "```",
    ]));
    has(
        &all,
        &lines(&[
            "</style><link rel=\"stylesheet\" href=\"rich/katex.min.css\">",
            "<script defer src=\"rich/katex.min.js\"></script>",
            "<script defer src=\"rich/mhchem.min.js\"></script>",
            "<script src=\"rich/mermaid.min.js\"></script>",
            "<script src=\"rich/viz-global.js\"></script>",
            "<script defer src=\"rich/highlight.min.js\"></script>",
            "</head>",
        ]),
    );
}

#[test]
fn a_document_of_nothing_but_plots_is_light() {
    let html = doc("```plot\nsin(x)\n```");
    for engine in ["katex.min.js", "mermaid.min.js", "viz-global.js", "highlight.min.js", "mhchem.min.js"] {
        lacks(&html, engine);
    }
    has(&html, "<svg ");
}

// MARK: - Footnotes

#[test]
fn footnote_definition_is_parsed_and_not_drawn_in_place() {
    let html = doc("Text[^a].\n\n[^a]: The note.");
    lacks(&html, "<p>[^a]: The note.</p>");
    has(&html, "<li id=\"fn-1\">The note.");
    assert_eq!(
        body("Text[^a].\n\n[^a]: The note."),
        "<p>Text<sup class=\"md-fnref\" id=\"fnref-1\"><a href=\"#fn-1\">1</a></sup>.</p>\n\n<section class=\"md-footnotes\"><hr><ol><li id=\"fn-1\">The note. <a class=\"md-fnback\" href=\"#fnref-1\">&#8617;</a></li></ol></section>"
    );
}

#[test]
fn footnotes_are_numbered_by_first_reference_not_definition_order() {
    let html = doc("See[^b] then[^a].\n\n[^a]: Alpha.\n[^b]: Bravo.");
    has(&html, "<li id=\"fn-1\">Bravo.");
    has(&html, "<li id=\"fn-2\">Alpha.");
    assert!(html.find("#fn-1").unwrap() < html.find("#fn-2").unwrap());
}

#[test]
fn repeated_footnote_reference_gets_its_own_anchor() {
    let html = doc("One[^a] two[^a].\n\n[^a]: Note.");
    assert_eq!(count(&html, "href=\"#fn-1\""), 2);
    has(&html, "id=\"fnref-1\"");
    has(&html, "id=\"fnref-1-2\"");
    // The back-link goes to the first citation, never `fnref-1-2`.
    has(&html, "<a class=\"md-fnback\" href=\"#fnref-1\">&#8617;</a>");
    lacks(&html, "href=\"#fnref-1-2\"");
}

#[test]
fn footnote_reference_without_definition_stays_literal_text() {
    let html = doc("A claim[^nope].");
    has(&html, "[^nope]");
    // Assert on the emitted markup, not the class name: the stylesheet names every class.
    lacks(&html, "<sup class=\"md-fnref\"");
    lacks(&html, "<section class=\"md-footnotes\">");
    assert_eq!(body("A claim[^nope]."), "<p>A claim[^nope].</p>");
}

#[test]
fn unreferenced_footnote_is_still_printed() {
    let html = doc("Body.\n\n[^lone]: Never cited.");
    has(&html, "<li id=\"fn-1\">Never cited.");
    lacks(&html, "<a class=\"md-fnback\"");
    // Cited notes first, then the uncited ones in definition order.
    let mixed = body("X[^b].\n\n[^a]: A.\n[^b]: B.\n[^c]: C.");
    has(
        &mixed,
        "<li id=\"fn-1\">B. <a class=\"md-fnback\" href=\"#fnref-1\">&#8617;</a></li><li id=\"fn-2\">A.</li><li id=\"fn-3\">C.</li>",
    );
}

#[test]
fn first_footnote_definition_wins_on_a_duplicate_id() {
    let html = body("X[^a].\n\n[^a]: One.\n[^a]: Two.");
    has(&html, "<li id=\"fn-1\">One.");
    lacks(&html, "Two.");
    assert_eq!(count(&html, "<li id="), 1);
}

#[test]
fn footnote_definitions_are_gathered_from_the_top_level_only() {
    // `body` walks the top-level blocks; a definition nested in a quote renders as nothing and is
    // never gathered, so the reference naming it has no definition and stays literal text.
    assert_eq!(body("X[^a].\n\n> [^a]: quoted definition"), "<p>X[^a].</p>\n<blockquote>\n\n</blockquote>");
    // A document that is nothing but a definition still prints it — under the leading newline the
    // section carries, on top of the blank the empty definition block left in the join.
    assert_eq!(
        body("[^a]: only a definition"),
        "\n<section class=\"md-footnotes\"><hr><ol><li id=\"fn-1\">only a definition</li></ol></section>"
    );
}

#[test]
fn footnote_reference_in_a_heading_is_numbered_and_leaves_the_slug() {
    // The slug is taken from the raw heading text, where `[`, `^` and `]` are dropped and the id
    // keeps only the letters; the reference itself is resolved like any other.
    assert_eq!(
        body("# Heading[^a]\n\n[^a]: note"),
        "<h1 id=\"headinga\">Heading<sup class=\"md-fnref\" id=\"fnref-1\"><a href=\"#fn-1\">1</a></sup></h1>\n\n<section class=\"md-footnotes\"><hr><ol><li id=\"fn-1\">note <a class=\"md-fnback\" href=\"#fnref-1\">&#8617;</a></li></ol></section>"
    );
}

#[test]
fn every_further_citation_takes_the_next_anchor_and_the_id_keeps_its_shape() {
    // Occurrence 1 is `fnref-1`; the rest are `fnref-1-<occurrence>` — an id, not a number, so a
    // note cited three times still resolves to one entry.
    let html = body("Text[^a] and[^a] and[^a].\n\n[^a]: thrice");
    has(&html, "id=\"fnref-1\"");
    has(&html, "id=\"fnref-1-2\"");
    has(&html, "id=\"fnref-1-3\"");
    assert_eq!(count(&html, "href=\"#fn-1\""), 3);
    assert_eq!(count(&html, "<li id="), 1);
    // Ids may hold letters, digits, `_` and `-` — the parser's set and the reference's set.
    has(&body("a[^A_b-1] z\n\n[^A_b-1]: mixed id"), "<sup class=\"md-fnref\" id=\"fnref-1\">");
}

#[test]
fn footnote_text_is_inline_markdown_and_is_escaped() {
    let html = doc("X[^a].\n\n[^a]: *Emphasis* and <b>literal</b> & co.");
    has(&html, "<em>Emphasis</em>");
    has(&html, "&lt;b&gt;literal&lt;/b&gt;");
    has(&html, "&amp; co.");
}

#[test]
fn footnote_inside_code_is_not_a_reference() {
    let html = doc("Use `arr[^1]` here.\n\n[^1]: Note.");
    has(&html, "<code>arr[^1]</code>");
    lacks(&html, "<code>arr<sup");
}

#[test]
fn footnote_reference_cannot_break_out_into_markup() {
    let image = doc("![alt [^a] here](i.png)\n\n[^a]: N.");
    lacks(&image, "alt=\"alt <sup");
    lacks(&image, "<img");
    // A label holding a reference is not a link — the harmless failure.
    let link = doc("[see [^a] here](u)\n\n[^a]: N.");
    lacks(&link, "<a href=\"u\">");
    lacks(&link, "</a></a>");
    let plain = doc("[text](u) and ![a](i.png)");
    has(&plain, "<a href=\"u\">text</a>");
    has(&plain, "<img src=\"i.png\" alt=\"a\">");
    // The placeholder the span pass leaves for the document-wide numbering.
    assert_eq!(inline("a[^b]"), "a<sup class=\"md-fnref\" data-fn=\"b\"></sup>");
    assert_eq!(inline(&format!("[^caf{E_ACUTE}]")), format!("[^caf{E_ACUTE}]"));
    assert_eq!(inline("[^two words]"), "[^two words]");
}

#[test]
fn footnote_reference_nested_inside_a_note_becomes_literal_text() {
    let html = doc("X[^a].\n\n[^a]: see [^b] too.\n[^b]: Other.");
    lacks(&html, "data-fn=");
    has(&html, "[^b]");
    // Kotlin's exact pin: the uncited `b` is still printed, and the nested reference reads as the
    // author typed it.
    let kotlin = doc("X[^a].\n\n[^a]: See [^b] as well.\n[^b]: Bee.");
    has(&kotlin, "<li id=\"fn-1\">See [^b] as well.");
    has(&kotlin, "<li id=\"fn-2\">Bee.");
    lacks(&kotlin, "data-fn=");
}

// MARK: - Raw diagram documents

#[test]
fn raw_plant_uml_document_renders_as_diagram() {
    let html = html::document("@startuml\nAlice -> Bob: hi\nBob --> Alice: hi\n@enduml\n", "d", false, false);
    has(&html, "<div class=\"plantuml\">");
    has(&html, "rich/viz-global.js");
    lacks(&html, "<p>@startuml");
    // The whole file, trailing newline included, is escaped: the close tag lands on its own line.
    assert_eq!(body("@startuml\nA -> B\n@enduml\n"), "<div class=\"plantuml\">@startuml\nA -&gt; B\n@enduml\n</div>");

    assert!(parser::is_raw_plant_uml("' header comment\n\n@startmindmap\n* root\n@endmindmap"));
    assert!(parser::is_raw_plant_uml("   \n@startuml\n@enduml"));
    assert!(parser::is_raw_plant_uml("\r\n@startuml\r\nA -> B\r\n@enduml\r\n"));
    assert!(parser::is_raw_plant_uml("' note\r\n\r\n@startmindmap\r\n* r\r\n@endmindmap"));
    // Kotlin adds the lone-CR (classic Mac) case.
    assert!(parser::is_raw_plant_uml("' note\r\r@startuml\r@enduml"));
    assert!(!parser::is_raw_plant_uml("# Title\n\nSome prose about @startuml in passing."));
    assert!(!parser::is_raw_plant_uml(""));
    let md = html::document("# Title\n\nHello.", "d", false, false);
    lacks(&md, "<div class=\"plantuml\">");
    has(&md, "<h1");
}

#[test]
fn raw_graphviz_document_renders_as_diagram() {
    let html = html::document("digraph G {\n  a -> b;\n}\n", "d", false, false);
    has(&html, "<div class=\"graphviz\" data-engine=\"dot\">");
    has(&html, "rich/viz-global.js");
    lacks(&html, "<p>digraph");

    for yes in [
        "digraph { a -> b }".to_string(),
        "graph {}".to_string(),
        "strict digraph G {\n}".to_string(),
        "DiGraph Foo {\n}".to_string(),
        "digraph{a}".to_string(),
        "digraph\n{\n  a\n}".to_string(),
        "// generated\n\n/* by hand */\ndigraph { a }".to_string(),
        "digraph \"my graph\" {\n}".to_string(),
        format!("digraph cafe{ACUTE} {{\n}}"),
        format!("digraph caf{E_ACUTE} {{\n}}"),
        format!("digraph {ROMAN_TEN} {{\n}}"),
        format!("// generated{NEL}digraph G {{{NEL}}}"),
        "\r\ndigraph G {\r\n  a -> b;\r\n}\r\n".to_string(),
        "// generated\r\ndigraph G {\r\n}\r\n".to_string(),
        "// c\rdigraph G {\r}".to_string(),
    ] {
        assert!(parser::is_raw_graphviz(&yes), "{yes:?}");
    }
    for no in [
        "# Notes\n\ngraph { the mental model }",
        "graph theory is a branch of maths.\n\nSee $\\frac{a}{b}$.",
        "digraph models are useful { in theory }",
        "graphviz is a fine tool { see }",
        "digraphs are a topic { here }",
        "digraph without a brace",
        "# Title\n\nSome prose about digraph { } in passing.",
        "",
    ] {
        assert!(!parser::is_raw_graphviz(no), "{no:?}");
    }

    // A diagram nested in a quote still pulls its engine in; a Markdown document that merely
    // contains a dot fence is still Markdown.
    let quoted = html::document("> ```dot\n> digraph { a }\n> ```", "d", false, false);
    has(&quoted, "class=\"graphviz\"");
    has(&quoted, "rich/viz-global.js");
    let mixed = html::document("# Title\n\n```dot\ndigraph { a }\n```\n", "d", false, false);
    has(&mixed, "<h1");
    has(&mixed, "class=\"graphviz\"");
}

// MARK: - CSV / TSV fences

#[test]
fn csv_fence_renders_as_a_table() {
    let html = doc("```csv\nName,Role\nAnn,Editor\n```");
    has(&html, "<th style=\"text-align:left\">Name</th>");
    has(&html, "<td style=\"text-align:left\">Ann</td>");
    lacks(&html, "<pre><code>Name,Role");
}

#[test]
fn csv_numeric_columns_are_right_aligned() {
    let html = doc("```csv\nCity,People\nOslo,709037\nBergen,289330\n```");
    has(&html, "<th style=\"text-align:left\">City</th>");
    has(&html, "<th style=\"text-align:right\">People</th>");
    has(&html, "<td style=\"text-align:right\">709037</td>");
    has(&doc("```csv\nA\n1\nn/a\n```"), "<th style=\"text-align:left\">A</th>");
}

#[test]
fn decimal_number_grammar_decides_the_alignment() {
    // The HTML half of testDecimalNumberGrammarIsExplicitNotDoubleInit: a hex column stays left,
    // and an invisible U+200B is not padding because the cell trim is ASCII space/tab.
    has(&doc("```csv\nItem,Value\na,0x10\n```"), "<th style=\"text-align:left\">Value</th>");
    has(&doc(&format!("```csv\nItem,Value\na,{ZWSP}1{ZWSP}\n```")), "<th style=\"text-align:left\">Value</th>");
    has(&doc("```csv\nItem,Value\na, 1 \n```"), "<th style=\"text-align:right\">Value</th>");
}

#[test]
fn tsv_fence_uses_tabs() {
    let html = doc("```tsv\nCity\tPeople\nOslo\t709037\n```");
    has(&html, "<th style=\"text-align:left\">City</th>");
    has(&html, "<td style=\"text-align:right\">709037</td>");
}

#[test]
fn empty_csv_fence_stays_a_code_block() {
    let html = doc("```csv\n\n```");
    lacks(&html, "<table>");
    // The fallback is the *bare* form, which trips no highlight gate. A fence holding one blank
    // line and an empty fence are the same code: the parser joins its collected lines with "\n",
    // so a single empty line is an empty string, not a newline.
    assert_eq!(body("```csv\n\n```"), "<pre><code></code></pre>");
    assert_eq!(body("```csv\n```"), "<pre><code></code></pre>");
    assert!(!needs("```csv\n```").highlight);
    lacks(&html, "highlight.min.js");
}

// MARK: - The stylesheet

/// Everything between `<style>` and `</style>`.
fn style_block(html: &str) -> &str {
    let open = html.find("<style>").unwrap() + "<style>".len();
    let close = open + html[open..].find("</style>").unwrap();
    &html[open..close]
}

/// Strip comments the way a CSS parser does, not the way a grep does; an unterminated comment
/// swallows the rest.
fn strip_css_comments(sheet: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    loop {
        let Some(open) = sheet[cursor..].find("/*").map(|i| cursor + i) else {
            output.push_str(&sheet[cursor..]);
            return output;
        };
        output.push_str(&sheet[cursor..open]);
        let Some(close) = sheet[open + 2..].find("*/").map(|i| open + 2 + i) else {
            return output;
        };
        cursor = close + 2;
    }
}

#[test]
fn graphviz_ink_rules_survive_css_comment_stripping() {
    // If a comment ever closes early, the prose after it becomes the prelude of the next rule and
    // the parser swallows that rule whole — the diagram then draws black on carbon paper, while a
    // test that merely greps for the rule text still passes.
    for dark in [false, true] {
        let document = doc_with("```dot\ndigraph { a }\n```", dark, false);
        let css = style_block(&document);
        assert_eq!(css.split("/*").count(), css.split("*/").count());
        let stripped = strip_css_comments(css);
        let ink = if dark { "#E7DBC2" } else { "#2B2620" };
        for rule in ["text:not([fill])", "text[fill=\"black\"]", "[stroke=\"black\"]", "[fill=\"black\"]:not(text)"] {
            has(&stripped, &format!(".graphviz svg {rule}"));
        }
        // Graphviz writes no fill at all unless asked, so the uncoloured-label rule is the one that
        // matters — and the first casualty of a broken comment.
        has(&stripped, &format!(".graphviz svg text:not([fill]) {{ fill: {ink}; }}"));
        // The rules must not have been swallowed into a comment: none of them is inside one.
        lacks(&stripped, "/*");
    }
}

#[test]
fn graphviz_ink_recolor_is_scoped_to_the_engines_own_black() {
    // Kotlin's pin: the recolor must not overwrite an author's `fontcolor` / `color`; each rule is
    // scoped to what the engine emits when nothing was asked for.
    let html = doc_with("```dot\ndigraph { a }\n```", true, false);
    has(&html, ".graphviz svg text:not([fill]) { fill: #E7DBC2; }");
    has(&html, ".graphviz svg text[fill=\"black\"] { fill: #E7DBC2; }");
    has(&html, ".graphviz svg [stroke=\"black\"] { stroke: #E7DBC2; }");
    has(&html, ".graphviz svg [fill=\"black\"]:not(text) { fill: #E7DBC2; }");
    lacks(&html, ".graphviz svg text { fill:");
    lacks(&html, ".graphviz svg [fill=\"black\"] { fill:");
}

#[test]
fn highlight_theme_is_the_apps_own_palette() {
    // Hand-written in the paper palette, not a stock hljs theme, and it rides in every document's
    // <style>; only the engine <script> is gated. These are the Swift rules — Kotlin ships a
    // shorter theme with different string tones (#6A5433 / #B79A67), which must never appear here.
    let dark = doc_with("```swift\nlet x = 1\n```", true, false);
    has(&dark, ".hljs-type, .hljs-title, .hljs-section, .hljs-name, .hljs-doctag { color: #C99A55; }");
    has(&dark, ".hljs-comment, .hljs-quote, .hljs-meta { color: #B3A98E; font-style: italic; }");
    has(&dark, ".hljs-attr, .hljs-attribute, .hljs-addition { color: #CDBF9E; }");
    has(&dark, ".hljs-deletion { color: #B3A98E; text-decoration: line-through; }");
    let light = doc("```swift\nlet x = 1\n```");
    has(&light, ".hljs-type, .hljs-title, .hljs-section, .hljs-name, .hljs-doctag { color: #9C6B2E; }");
    has(&light, ".hljs-comment, .hljs-quote, .hljs-meta { color: #6B635A; font-style: italic; }");
    has(&light, ".hljs-attr, .hljs-attribute, .hljs-addition { color: #4A4034; }");
    // "Georgia" is the family's screen stand-in and "Lucida Sans Typewriter" is md.win's; both are
    // grafted on by an app, never by Core, and a golden would die the day either leaked in.
    for kotlin in ["#6A5433", "#B79A67", "Georgia", "Lucida Sans Typewriter"] {
        lacks(&light, kotlin);
        lacks(&dark, kotlin);
    }
    has(&light, "font-family: \"American Typewriter\", \"Courier New\", serif;");
}

#[test]
fn stylesheet_has_three_variants_of_101_lines() {
    for sheet in [html::css(false, false), html::css(true, false), html::css(false, true)] {
        assert_eq!(sheet.split('\n').count(), 101);
        assert!(!sheet.ends_with('\n'));
        assert!(sheet.starts_with("/* Force"));
        // The EPUB builder decides the `svg` manifest property by searching a document for the
        // literal tag, so the sheet may never contain one.
        lacks(&sheet, "<svg");
        lacks(&sheet, "\r");
    }
    // The dark kill is applied inside `css` too, so there is no fourth, never-shipped variant.
    assert_eq!(html::css(true, true), html::css(false, true));
    // Line 32 is genuinely blank in the preview sheets and the code-wrapping rule in export.
    assert_eq!(html::css(false, false).split('\n').nth(31), Some(""));
    assert_eq!(
        html::css(false, true).split('\n').nth(31),
        Some("pre { white-space: pre-wrap; overflow-wrap: anywhere; }")
    );
    lacks(&html::css(false, false), "pre-wrap");
    assert_eq!(html::css(false, true).split('\n').nth(11).map(whitespace::trim_ws), Some("font-size: 11pt;"));
    assert_eq!(html::css(true, false).split('\n').nth(11).map(whitespace::trim_ws), Some("font-size: 13pt;"));
    // `\newpage` in the comment is ONE backslash.
    has(&html::css(false, false), "/* The author's `\\newpage`: a dashed rule on screen;");
}

// MARK: - Plot integration (PlotTests.swift section 8, the renderer's share)

const PLOT_FENCE: &str = "```plot\nx: -10..10\ntitle: T\nsin(x)\n```";

#[test]
fn integration_dispatches_the_fence_to_a_plot_container_and_loads_no_engine() {
    let html = doc(PLOT_FENCE);
    has(&html, "<div class=\"plot\"><svg");
    // The class is exactly `plot`, which keeps all five probes false: no engine, no `needs` flag.
    for engine in ["mermaid.min.js", "viz-global.js", "katex.min.js", "mhchem.min.js", "highlight.min.js"] {
        lacks(&html, engine);
    }
    lacks(&html, "language-plot");
    assert_eq!(needs(PLOT_FENCE), EngineNeeds::NONE);
}

#[test]
fn integration_is_styled_by_the_screen_and_the_export_stylesheet() {
    // The rules are *joined*, never added: `.plot` rides the container rule the other rich blocks
    // share, and `.plot svg` the cap rule. Both variants carry them.
    for export in [false, true] {
        let html = doc_with(PLOT_FENCE, false, export);
        has(&html, ".mermaid, .plantuml, .graphviz, .plot, .md-mathd {");
        has(&html, ".mermaid svg, .plantuml svg, .graphviz svg, .plot svg { max-width: 100%; height: auto; }");
    }
}
