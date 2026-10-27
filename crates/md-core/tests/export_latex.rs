//! The .tex writer. Pure string work with no rendering behind it, so it is covered densely: every
//! block kind, the escaping table, and the handful of places where LaTeX's own syntax bites back
//! (an optional argument eaten off the front of an item, a code block that closes its own verbatim
//! environment, a footnote in a longtable's head).
//!
//! Port of md.win `LaTeXExportTests.cs` — itself the port of the 100 `testLaTeX*` cases in
//! md.macOS/mdTests/mdTests.swift plus the Kotlin-only ones, the `plot` fence case from
//! PlotTests.swift §8, and the guards that are .NET's own — case for case and name for name.
//! md.win runs three of them under a Turkish culture; Rust has no culture, so those run as they
//! stand and still pin that nothing folds case by locale.

mod common;
#[path = "common/latex.rs"]
mod latex_common;

use std::borrow::Cow;

use latex_common::*;
use md_core::book::structured_book::{BookSection, BookUnit, StructuredBook};
use md_core::export::latex_export;
use md_core::markdown::{ColumnAlignment, DelimitedTable};
use md_core::text::scalar_text;

// MARK: - Helpers

fn unit(title: &str, source: &str) -> BookUnit {
    BookUnit { title: title.to_string(), source: source.to_string() }
}

fn section(title: &str, units: Vec<BookUnit>) -> BookSection {
    BookSection { title: title.to_string(), units }
}

/// A two-chapter book in the reading order the PDF compile and the EPUB already use.
fn sample_book() -> StructuredBook {
    StructuredBook {
        title: "The Book".to_string(),
        front_units: vec![unit("Preface", "Opening words.")],
        sections: vec![
            section("One", vec![unit("First", "# Inner\n\nA[^a].\n\n[^a]: n1"), unit("Second", "B[^b].\n\n[^b]: n2")]),
            section("Two", vec![unit("Third", "C[^c] C[^c].\n\n[^c]: n3")]),
        ],
    }
}

fn one_chapter(chapter: &str, units: Vec<BookUnit>) -> StructuredBook {
    StructuredBook { title: "B".to_string(), front_units: vec![], sections: vec![section(chapter, units)] }
}

// MARK: - Escaping

#[test]
fn latex_escapes_every_special_character() {
    // The ten characters TeX reserves. Three have no backslash form and need a command instead — a
    // leading backslash on `~` or `^` would be an accent waiting for its letter.
    assert_eq!("\\#", latex_export::escape("#"));
    assert_eq!("\\$", latex_export::escape("$"));
    assert_eq!("\\%", latex_export::escape("%"));
    assert_eq!("\\&", latex_export::escape("&"));
    assert_eq!("\\_", latex_export::escape("_"));
    assert_eq!("\\{", latex_export::escape("{"));
    assert_eq!("\\}", latex_export::escape("}"));
    assert_eq!("\\textasciitilde{}", latex_export::escape("~"));
    assert_eq!("\\textasciicircum{}", latex_export::escape("^"));
    assert_eq!("\\textbackslash{}", latex_export::escape("\\"));
}

#[test]
fn latex_escape_does_not_escape_its_own_output() {
    // The regression a chain of replace calls would produce: `\` becomes `\textbackslash{}`, whose
    // own braces and backslash are then escaped again.
    assert_eq!("a\\textbackslash{}b", latex_export::escape("a\\b"));
    assert_eq!("\\textasciitilde{}\\textasciicircum{}", latex_export::escape("~^"));
    // One pass, in order, whatever the mix.
    assert_eq!(
        "100\\% of \\{a\\_b\\} \\& \\textbackslash{}c \\textasciitilde{} \\textasciicircum{} \\$\\#",
        latex_export::escape("100% of {a_b} & \\c ~ ^ $#")
    );
}

#[test]
fn latex_escape_leaves_unreserved_punctuation_alone() {
    // `<`, `>` and `|` are not TeX specials, and the ports agree on this table character for character.
    assert_eq!("a < b > c | d", latex_export::escape("a < b > c | d"));
}

#[test]
fn latex_escape_walks_scalars_not_grapheme_clusters() {
    // A special followed by a combining mark is one grapheme in Swift and not `"#"`; a live `%`
    // comments away the rest of the author's sentence. A scalar walk sees the special.
    assert_eq!(format!("\\%{ACUTE}"), latex_export::escape(&format!("%{ACUTE}")));
    assert_eq!(format!("\\#{VS16}\u{20E3}"), latex_export::escape(&format!("#{VS16}\u{20E3}")));
    assert_eq!(format!("a\\%{ACUTE}b"), latex_export::escape_url(&format!("a%{ACUTE}b")));
    // Whole-document form: the sentence after the special survives.
    assert_eq!(
        format!("Fifty\\%{ACUTE} percent, and the rest survives."),
        tex_body(&format!("Fifty%{ACUTE} percent, and the rest survives."))
    );
}

#[test]
fn latex_percent_decodes_an_image_path() {
    // `my%20dir` is a directory on no disk anywhere: the author meant `my dir`.
    assert_eq!("my dir/a.png", latex_export::percent_decoded("my%20dir/a.png"));
    assert_eq!("a/b c.png", latex_export::percent_decoded("a%2Fb%20c.png"));
    // Multi-byte sequences come back as the character they encode.
    assert_eq!(format!("{}.png", chr(0x041F)), latex_export::percent_decoded("%D0%9F.png"));
    // What is not percent-encoding is a file name with a `%` in it, handed back exactly as it came.
    assert_eq!("100%.png", latex_export::percent_decoded("100%.png"));
    assert_eq!("a%zzb", latex_export::percent_decoded("a%zzb"));
    assert_eq!("trailing%2", latex_export::percent_decoded("trailing%2"));
    // Bytes that are not UTF-8 are not repaired into U+FFFD either.
    assert_eq!("a%C0%80b", latex_export::percent_decoded("a%C0%80b"));
    assert_eq!("plain.png", latex_export::percent_decoded("plain.png"));
}

#[test]
fn percent_decode_encodes_surrogate_pairs_whole_and_refuses_lone_ones() {
    // .NET-only. The bytes are produced a *scalar* at a time: encoding UTF-16 unit by unit turns
    // one emoji into two U+FFFD, and the strict decode then hands back a file name nobody has.
    let grinning = chr(0x1F600);
    assert_eq!(format!("{grinning} x.png"), latex_export::percent_decoded(&format!("{grinning}%20x.png")));
    assert_eq!(format!("{grinning}.png"), latex_export::percent_decoded("%F0%9F%98%80.png"));
    // md.win's third assertion hands in an unpaired surrogate and gets the path back unchanged. A
    // Rust string cannot hold one (the case cannot arise), so there is nothing to port.
}

#[test]
fn latex_url_escape_guards_only_what_breaks_the_argument() {
    // hyperref reads its URL argument with its own catcodes, so `_` and `&` arrive intact.
    assert_eq!("https://x.com/a_b?c=1&d=2", latex_export::escape_url("https://x.com/a_b?c=1&d=2"));
    // A comment character would swallow the rest of the line and a parameter character is illegal there.
    assert_eq!("https://x.com/100\\%25\\#top", latex_export::escape_url("https://x.com/100%25#top"));
    // An unbalanced brace would end the argument early.
    assert_eq!("a\\{b\\}c", latex_export::escape_url("a{b}c"));
}

// MARK: - Preamble

#[test]
fn latex_plain_document_has_short_preamble() {
    // Nothing but the class and the input encoding: a document with no image, no link, no table
    // and no strikethrough owes no packages.
    assert_eq!("\\documentclass{article}\n\\usepackage[utf8]{inputenc}\n", tex_preamble("# Title\n\nJust prose."));
}

#[test]
fn latex_packages_follow_the_features_used() {
    assert_contains(&tex_preamble("![a](x.png)"), "\\usepackage{graphicx}");
    assert_contains(&tex_preamble("[a](https://x.com)"), "\\usepackage{hyperref}");
    assert_contains(&tex_preamble("| a |\n|---|\n| b |"), "\\usepackage{longtable}");
    // `normalem` is not decoration: plain ulem redefines `\emph` to underline.
    assert_contains(&tex_preamble("~~gone~~"), "\\usepackage[normalem]{ulem}");
    assert_not_contains(&tex_preamble("~~gone~~"), "\\usepackage{ulem}");
}

#[test]
fn latex_packages_are_not_triggered_by_quoted_code() {
    // The flags are set by the renderer as it emits each command, not by scanning the file — a
    // code block *about* LaTeX is not a link, an image or a table.
    let preamble = tex_preamble("```\n\\href{x}{y} \\includegraphics{z} \\begin{tabular}{l}\n```");
    assert_not_contains(&preamble, "hyperref");
    assert_not_contains(&preamble, "graphicx");
    assert_not_contains(&preamble, "longtable");
}

#[test]
fn latex_hyperref_is_loaded_last() {
    // The one package with a documented loading order.
    let preamble = tex_preamble("[a](https://x.com) ![b](c.png) ~~d~~\n\n| a |\n|---|\n| b |");
    let hyperref = preamble.find("\\usepackage{hyperref}").expect("hyperref should be loaded");
    for other in ["graphicx", "ulem", "longtable"] {
        let index = preamble.find(other).unwrap_or_else(|| panic!("{other} should be loaded"));
        assert!(index < hyperref, "{other} must come before hyperref");
    }
}

#[test]
fn latex_cyrillic_pulls_in_fontenc_with_t2a_last() {
    // Without a Cyrillic encoding pdfTeX does not stop — the Russian is simply not in the PDF.
    let preamble = tex_preamble("Привет, мир!");
    assert_contains(&preamble, "\\usepackage[T1,T2A]{fontenc}");
    // fontenc makes the *last* encoding the default, so `[T2A,T1]` would drop exactly the letters
    // it was added for.
    assert_not_contains(&preamble, "[T2A,T1]");
}

#[test]
fn latex_plain_english_gets_no_fontenc() {
    assert_not_contains(&tex_preamble("Plain English prose."), "fontenc");
}

#[test]
fn latex_cyrillic_inside_code_still_pulls_in_fontenc() {
    // Verbatim content is typeset too.
    assert_contains(&tex_preamble("```\n// комментарий\n```"), "fontenc");
}

#[test]
fn latex_cyrillic_only_in_a_private_note_does_not_pull_in_fontenc() {
    // A private note never reaches the document, so it cannot make the document need anything.
    assert_not_contains(&tex_preamble("English.\n\n<!-- note: Привет -->"), "fontenc");
}

#[test]
fn cyrillic_scan_covers_every_block_it_claims() {
    // Kotlin-only. Each of the four blocks at its first character — the one that gets forgotten
    // is the low end of the main block: U+0401 Ё is an everyday Russian letter.
    assert!(latex_export::contains_cyrillic(&chr(0x0400)), "U+0400");
    assert!(latex_export::contains_cyrillic(&chr(0x0401)), "U+0401");
    assert!(latex_export::contains_cyrillic(&chr(0x0501)), "U+0501");
    assert!(latex_export::contains_cyrillic(&chr(0x2DE0)), "U+2DE0");
    assert!(latex_export::contains_cyrillic(&chr(0xA640)), "U+A640");
    assert!(!latex_export::contains_cyrillic(&chr(0x03FF)), "just below the block");
    assert!(!latex_export::contains_cyrillic("Plain Latin, ancient Greek: α β γ"));
}

#[test]
fn cyrillic_scan_is_not_fooled_by_surrogate_pairs() {
    // Kotlin-only. Both halves of a supplementary character sit in D800–DFFF, clear of every
    // Cyrillic block; here the scan is over scalars, which is exact by construction.
    assert!(!latex_export::contains_cyrillic(&format!("emoji {} and {} and 中文", chr(0x1F600), chr(0x1D504))));
    assert!(latex_export::contains_cyrillic("mostly English but ё"));
    // …and the pair itself survives the escaper intact.
    assert_eq!(format!("a {} b", chr(0x1F600)), latex_export::escape(&format!("a {} b", chr(0x1F600))));
}

// MARK: - Front matter and the title block

#[test]
fn latex_front_matter_becomes_title_block() {
    let tex = tex(&lines(&[
        "---",
        "title: On Escaping",
        "author: nettrash",
        "date: 24 July 2026",
        "slug: on-escaping",
        "tags: latex, md",
        "---",
        "",
        "Body.",
    ]));
    assert_contains(&tex, "\\title{On Escaping}");
    assert_contains(&tex, "\\author{nettrash}");
    assert_contains(&tex, "\\date{24 July 2026}");
    assert_contains(&tex, "\\maketitle");
    // Everything else is metadata *about* the document and has nowhere to go in a typeset one.
    assert_not_contains(&tex, "on-escaping");
    assert_not_contains(&tex, "tags");
}

#[test]
fn latex_title_block_without_a_date_does_not_invent_one() {
    // `\maketitle` stamps today when no date is given — a date nobody wrote.
    assert_contains(&tex("---\ntitle: Untimed\n---\n\nBody."), "\\date{}");
}

#[test]
fn latex_without_front_matter_has_no_title_block() {
    let tex = tex("# Heading\n\nBody.");
    assert_not_contains(&tex, "\\title");
    assert_not_contains(&tex, "\\maketitle");
}

#[test]
fn latex_title_fields_are_escaped() {
    assert_contains(&tex("---\ntitle: 100% & more_stuff\n---\n\nBody."), "\\title{100\\% \\& more\\_stuff}");
}

#[test]
fn repeated_front_matter_key_keeps_the_first() {
    // Kotlin-only. The parser records every line the author wrote; the rest of the app reads the
    // first, and so does the title block.
    let tex = tex("---\ntitle: First\ntitle: Second\n---\n\nBody.");
    assert_contains(&tex, "\\title{First}");
    assert_not_contains(&tex, "Second");
}

#[test]
fn front_matter_keys_fold_against_the_root_locale() {
    // Kotlin-only. `ToLower()` under a Turkish culture turns "TITLE" into "tıtle" with a dotless ı,
    // and the title block would vanish on that machine alone. (md.win runs this under tr-TR; the
    // fold here has no locale to consult.)
    assert_contains(&tex("---\nTITLE: Kitap\n---\n\nBody."), "\\title{Kitap}");
}

#[test]
fn cyrillic_only_in_the_title_block_still_pulls_in_fontenc() {
    // Kotlin-only. The title is typeset like any other text and is not part of the body the scan
    // walks, so it is scanned in its own right.
    assert_contains(&tex_preamble("---\ntitle: Привет\n---\n\nEnglish body."), "fontenc");
}

// MARK: - Headings

#[test]
fn latex_heading_levels_map_onto_sectioning() {
    let expected = [
        "\\section{H}",
        "\\subsection{H}",
        "\\subsubsection{H}",
        "\\paragraph{H}",
        "\\subparagraph{H}",
        "\\subparagraph{H}",
    ];
    for level in 1..=6 {
        assert_eq!(expected[level - 1], tex_body(&format!("{} H", "#".repeat(level))));
    }
}

#[test]
fn latex_heading_text_is_escaped_and_formatted() {
    assert_eq!("\\section{50\\% \\textbf{off}}", tex_body("# 50% **off**"));
}

// MARK: - Mathematics

#[test]
fn latex_inline_math_passes_through_unescaped() {
    // The reason the format exists: the formula comes back as the source the author typed.
    assert_eq!("Then $a_1^{2} \\frac{x}{y}$ follows.", tex_body("Then $a_1^{2} \\frac{x}{y}$ follows."));
}

#[test]
fn latex_display_math_uses_the_bracket_form() {
    assert_eq!("\\[x^2\\]", tex_body("$$x^2$$"));
    assert_eq!("\\[x^2\\]", tex_body("\\[x^2\\]"));
    assert_eq!("\\[\nx^2\n\\]", tex_body("```math\nx^2\n```"));
    // ```latex and ```tex name the same thing.
    assert_eq!("\\[\nx^2\n\\]", tex_body("```latex\nx^2\n```"));
    assert_eq!("\\[\nx^2\n\\]", tex_body("```tex\nx^2\n```"));
}

#[test]
fn latex_paren_math_becomes_dollar_math() {
    assert_eq!("A $a_i$ here.", tex_body("A \\(a_i\\) here."));
}

#[test]
fn latex_currency_is_not_mistaken_for_mathematics() {
    // The same guard the preview uses, so the two agree about what a formula is.
    assert_eq!("It costs \\$5 and \\$10.", tex_body("It costs $5 and $10."));
}

#[test]
fn latex_word_guard_is_spelled_out_rather_than_backslash_w() {
    // `\w` is three different classes on three engines; the guard is `[\p{L}\p{N}_]` on every
    // port. A superscript two is `\p{N}` and *not* in ICU's `\w`, so it stops what follows being a
    // formula only under the spelled-out guard.
    assert_eq!(format!("{SUP2}\\$x\\$"), tex_body(&format!("{SUP2}$x$")));
    assert_eq!(format!("{SUP2}\\_x\\_"), tex_body(&format!("{SUP2}_x_")));
    // A combining mark is in ICU's `\w` and is not a letter, a number or an underscore.
    assert_eq!(format!("e{ACUTE}$x$"), tex_body(&format!("e{ACUTE}$x$")));
    assert_eq!(format!("e{ACUTE}\\emph{{x}}"), tex_body(&format!("e{ACUTE}_x_")));
}

#[test]
fn latex_math_inside_backticks_stays_code() {
    // Code wins over math, exactly as in the preview.
    assert_eq!("Use \\texttt{\\$x\\$} literally.", tex_body("Use `$x$` literally."));
}

#[test]
fn underscore_italic_guard_is_unicode_aware() {
    // Kotlin-only. A Cyrillic word with an underscore in it is prose on every platform.
    assert_eq!("ф\\_em\\_ф", tex_body("ф_em_ф"));
}

#[test]
fn astral_letters_and_digits_guard_the_delimiters_like_bmp_ones() {
    // .NET-only. Its regex is code-unit based: a lookbehind before `$` sees the low surrogate of
    // U+1D504 (category Cs) and lets the formula through where ICU, the JVM and JS (`u`) see a
    // letter and refuse it. Mathematical alphanumerics next to a formula are realistic.
    let fraktur_a = chr(0x1D504); // Lu
    let double_struck_zero = chr(0x1D7D8); // Nd
    let grinning = chr(0x1F600); // So — neither letter nor number, on every port
    assert_eq!(format!("{fraktur_a}\\$x\\$"), tex_body(&format!("{fraktur_a}$x$")));
    assert_eq!(format!("\\$x\\${fraktur_a}"), tex_body(&format!("$x${fraktur_a}")));
    assert_eq!(format!("{double_struck_zero}\\_x\\_"), tex_body(&format!("{double_struck_zero}_x_")));
    assert_eq!(format!("\\_x\\_{double_struck_zero}"), tex_body(&format!("_x_{double_struck_zero}")));
    assert_eq!(format!("{grinning}$x$"), tex_body(&format!("{grinning}$x$")));
    assert_eq!(format!("{grinning}\\emph{{x}}"), tex_body(&format!("{grinning}_x_")));
    // A rejected opener resumes the scan one position later, as ICU's failed lookbehind does, so a
    // legitimate formula further along the line is still found.
    assert_eq!(format!("{fraktur_a}\\$a\\$ and $b$"), tex_body(&format!("{fraktur_a}$a$ and $b$")));
    assert_eq!("a\\_b\\_c and \\emph{d}", tex_body("a_b_c and _d_"));

    // `\p{N}` is the three number categories, not the digits: a Roman numeral (Nl), a vulgar
    // fraction (No) and an Arabic-Indic digit (Nd) all stop a delimiter, and each is one that at
    // least one engine's `\w` would have let through.
    assert_eq!(format!("{}\\$x\\$", chr(0x2160)), tex_body(&format!("{}$x$", chr(0x2160))));
    assert_eq!(format!("{}\\$x\\$", chr(0x00BD)), tex_body(&format!("{}$x$", chr(0x00BD))));
    assert_eq!(format!("{}\\_x\\_", chr(0x06F1)), tex_body(&format!("{}_x_", chr(0x06F1))));
    // A zero-width space is neither, so it stops nothing. (Mid-line: the parser trims one at the
    // head of a paragraph, U+200B being white space to Foundation and so to Whitespace.)
    assert_eq!(format!("a{}$x$", chr(0x200B)), tex_body(&format!("a{}$x$", chr(0x200B))));
}

// MARK: - Inline formatting

#[test]
fn latex_emphasis_becomes_commands() {
    assert_eq!("\\textbf{b} \\textbf{b} \\emph{i} \\emph{i} \\sout{s}", tex_body("**b** __b__ *i* _i_ ~~s~~"));
}

#[test]
fn latex_snake_case_survives_underscore_italic() {
    assert_eq!("a\\_b\\_c is one word", tex_body("a_b_c is one word"));
}

#[test]
fn latex_inline_code_is_texttt() {
    // `\texttt` rather than `\verb`: this text has to survive inside a section title, a caption
    // and a table cell. Its content is escaped like any other text.
    assert_eq!("Type \\texttt{a\\_b \\& c\\%}.", tex_body("Type `a_b & c%`."));
}

#[test]
fn latex_link_becomes_href() {
    assert_eq!("See \\href{https://x.com/a_b}{the docs}.", tex_body("See [the docs](https://x.com/a_b)."));
}

#[test]
fn latex_link_label_keeps_its_own_markup() {
    // The label stays ordinary text through the link pass, so the emphasis pass still finds it.
    assert_eq!("\\href{https://x.com}{a \\textbf{bold} label}", tex_body("[a **bold** label](https://x.com)"));
}

#[test]
fn latex_link_title_is_consumed_not_printed() {
    // A title is a browser tooltip; a typeset page has nowhere to put one — but it must not be left
    // behind as stray prose either.
    assert_eq!("\\href{https://x.com}{a}", tex_body("[a](https://x.com \"hover text\")"));
}

#[test]
fn link_title_separator_is_icu_whitespace_and_title_is_icu_dot() {
    // .NET-only. The Swift patterns use `\s` and `.`, which NSRegularExpression evaluates with
    // ICU's sets — measured on macOS: `\s` is [\t\n\v\f\r\u0085\p{Z}] (U+200B, U+FEFF and U+180E
    // excluded) and `.` refuses every line terminator (\n \v \f \r U+0085 U+2028 U+2029) but
    // nothing else.
    let nbsp = chr(0x00A0);
    let zwsp = chr(0x200B);
    let nel = chr(0x0085);
    let lsep = chr(0x2028);
    // NBSP separates URL from title, so the title is consumed.
    assert_eq!("\\href{https://x.com}{a}", tex_body(&format!("[a](https://x.com{nbsp}\"hover\")")));
    // ZWSP does not, so the whole run is the URL — quote and all.
    assert_eq!(format!("\\href{{u{zwsp}\"t\"}}{{a}}"), tex_body(&format!("[a](u{zwsp}\"t\")")));
    // A line terminator inside the title stops `.`; neither link form matches and the text stays
    // the author's, escaped.
    assert_eq!(format!("[a](https://x.com \"ho{nel}ver\")"), tex_body(&format!("[a](https://x.com \"ho{nel}ver\")")));
    assert_eq!(format!("[a](https://x.com \"ho{lsep}ver\")"), tex_body(&format!("[a](https://x.com \"ho{lsep}ver\")")));
    // A zero-width space inside the title is an ordinary character to `.`.
    assert_eq!("\\href{https://x.com}{a}", tex_body(&format!("[a](https://x.com \"ho{zwsp}ver\")")));
    // A byte-order mark is not in ICU's set either (JavaScript's `\s` does hold it), so it belongs
    // to the URL like the zero-width space.
    assert_eq!(format!("\\href{{u{}\"t\"}}{{a}}", chr(0xFEFF)), tex_body(&format!("[a](u{}\"t\")", chr(0xFEFF))));
    // An Ogham space mark is Zs, and every Z is in the set.
    assert_eq!("\\href{https://x.com}{a}", tex_body(&format!("[a](https://x.com{}\"hover\")", chr(0x1680))));
    // The image forms share the classes.
    assert_eq!("\\noindent\\includegraphics[width=\\linewidth]{p.png}", tex_body(&format!("![](p.png{nbsp}\"t\")")));
}

#[test]
fn latex_image_with_alt_text_becomes_a_captioned_figure() {
    assert_eq!(
        lines(&[
            "\\begin{figure}[ht]",
            "\\centering",
            "\\includegraphics[width=\\linewidth]{img/a_1.png}",
            "\\caption{A wide shot}",
            "\\end{figure}",
        ]),
        tex_body("![A wide shot](img/a_1.png)")
    );
}

#[test]
fn latex_image_without_alt_text_is_just_the_graphic() {
    // No caption (an empty one prints a bare "Figure 1") and a `\noindent`, because a `\linewidth`
    // graphic starting a paragraph is pushed over the margin by the indent.
    assert_eq!("\\noindent\\includegraphics[width=\\linewidth]{img/a.png}", tex_body("![](img/a.png)"));
}

#[test]
fn latex_alt_text_is_rendered_from_the_markup_the_author_wrote() {
    // The alt text is captured from a string the literal-span pass has already tokenised;
    // rendering it where it stands would leave that pass's tokens inside the caption. So the
    // caption is a fresh, complete pass over the Markdown they actually wrote.
    assert_contains(&tex_body("![Alt with $x$ set](f.png)"), "\\caption{Alt with $x$ set}");
    assert_contains(&tex_body("![Alt `c` and **b**](f.png)"), "\\caption{Alt \\texttt{c} and \\textbf{b}}");
    // Nothing of either pass is left in the file.
    assert_not_contains(&tex_body("![Alt with $x$ and `c`](f.png)"), E000);
    assert_not_contains(&tex_body("![Alt with $x$ and `c`](f.png)"), E001);
}

#[test]
fn caption_keeps_the_alt_texts_own_markup() {
    // Kotlin-only. The caption is the alt text rendered as inline Markdown, as a moving argument.
    assert_contains(&tex_body("![a **bold** shot](x.png)"), "\\caption{a \\textbf{bold} shot}");
}

#[test]
fn latex_image_path_is_percent_decoded_before_it_is_written() {
    assert_eq!("\\noindent\\includegraphics[width=\\linewidth]{my dir/a.png}", tex_body("![](my%20dir/a.png)"));
}

#[test]
fn latex_image_path_latex_cannot_read_is_skipped_and_named() {
    // graphicx reads its argument as a file name, and there is no spelling of `%` or `#` that works
    // in one — so the file is skipped and named, and the alt text set in its place.
    let body = tex_body("Before ![alt](a#b.png) after.");
    assert_not_contains(&body, "\\includegraphics");
    assert_contains_because(&body, "\\emph{alt}", "the author's words survive");
    assert_contains(&body, "% md: image skipped");
    assert_contains_because(&body, "a#b.png", "the author is told which file");
    assert_contains(&body, "Before");
    assert_contains_because(&body, "after.", "nothing after the image is lost");
    // The comment comes last and ends its own line: `%` runs to the end of the physical line.
    assert_contains(&body, "\\emph{alt}\n% md: image skipped");
    assert_contains(&body, "LaTeX can read.\n");
    // A path that is not percent-encoding keeps its `%` and is refused for it.
    assert_contains(&tex_body("![](100%.png)"), "% md: image skipped");
    // A package is not loaded for a graphic that was never drawn.
    assert_not_contains(&tex_preamble("![](a#b.png)"), "graphicx");
}

// MARK: - Lists

#[test]
fn latex_lists_use_itemize_and_enumerate() {
    assert_eq!(lines(&["\\begin{itemize}", "\\item a", "\\item b", "\\end{itemize}"]), tex_body("- a\n- b"));
    assert_eq!(lines(&["\\begin{enumerate}", "\\item a", "\\item b", "\\end{enumerate}"]), tex_body("1. a\n2. b"));
}

#[test]
fn latex_nested_lists_nest_environments() {
    assert_eq!(
        lines(&[
            "\\begin{itemize}",
            "\\item a",
            "\\begin{itemize}",
            "\\item b",
            "\\begin{itemize}",
            "\\item c",
            "\\end{itemize}",
            "\\end{itemize}",
            "\\item d",
            "\\end{itemize}",
        ]),
        tex_body("- a\n  - b\n    - c\n- d")
    );
}

#[test]
fn latex_list_starting_indented_opens_only_one_environment() {
    // Two `\begin{itemize}` in a row is "perhaps a missing \item".
    let body = tex_body("  - already indented\n    - deeper");
    assert_not_contains_because(
        &body,
        "\\begin{itemize}\n\\begin{itemize}",
        "no environment may open without an item before it",
    );
    assert_eq!(2, occurrences(&body, "\\begin{itemize}"));
    assert_eq!(2, occurrences(&body, "\\end{itemize}"));
}

#[test]
fn latex_deep_list_stops_at_latexs_nesting_limit() {
    // LaTeX refuses to nest lists more than four deep; every item still has to appear.
    let source = (0..6).map(|i| format!("{}- l{i}", " ".repeat(i * 2))).collect::<Vec<_>>().join("\n");
    let body = tex_body(&source);
    assert_eq!(4, occurrences(&body, "\\begin{itemize}"));
    for level in 0..6 {
        assert_contains_because(&body, &format!("\\item l{level}"), &format!("l{level} must survive"));
    }
}

#[test]
fn latex_task_list_renders_literal_checkboxes() {
    // `\item [x]` reads the bracket as the item's optional *label*; the empty group stops it.
    assert_eq!(
        lines(&["\\begin{itemize}", "\\item {}[\\,] open", "\\item {}[x] done", "\\end{itemize}"]),
        tex_body("- [ ] open\n- [x] done")
    );
}

#[test]
fn latex_item_beginning_with_a_bracket_is_guarded() {
    assert_contains(&tex_body("- [draft] not a task"), "\\item {}[draft]");
    assert_contains(&tex_body("- plain"), "\\item plain");
}

// MARK: - Code and diagrams

#[test]
fn latex_code_block_is_verbatim_and_unescaped() {
    assert_eq!(
        lines(&["\\begin{verbatim}", "if (a & b) { x_1 = 100%; }", "\\end{verbatim}"]),
        tex_body("```\nif (a & b) { x_1 = 100%; }\n```")
    );
}

#[test]
fn latex_code_block_quoting_verbatim_end_is_split() {
    // LaTeX's verbatim terminator is matched as *characters*, so a block that quotes
    // `\end{verbatim}` would close the environment early.
    let body = tex_body("```\nbefore\n\\end{verbatim}\nafter\n```");
    assert_contains_because(&body, "\\verb|\\end{verbatim}|", "the terminator itself must be set with \\verb");
    assert_contains(&body, "before");
    assert_contains_because(&body, "after", "nothing after the hazard may be lost");
    let opens = occurrences(&body, "\\begin{verbatim}");
    let closes = occurrences(&body, "\\end{verbatim}");
    assert_eq!(2, opens);
    assert_eq!(opens + 1, closes);
}

#[test]
fn latex_diagram_source_survives_as_verbatim() {
    // LaTeX has no Mermaid, PlantUML or Graphviz; dropping the diagram would lose a figure.
    for language in ["mermaid", "plantuml", "dot", "neato"] {
        let body = tex_body(&format!("```{language}\nA -> B\n```"));
        assert_starts_with(&body, &format!("% {language} diagram source"));
        assert_contains_because(
            &body,
            "\\begin{verbatim}\nA -> B\n\\end{verbatim}",
            &format!("{language} source must survive verbatim"),
        );
    }
    // The comment in full, to the byte — the dash is U+2014 in all four ports, and the language is
    // the author's own spelling, not the lowercased one the dispatch matched on.
    assert_eq!(
        format!(
            "% Mermaid diagram source {EM_DASH} LaTeX has no renderer for it, so it is kept as written.\n\\begin{{verbatim}}\nA -> B\n\\end{{verbatim}}"
        ),
        tex_body("```Mermaid\nA -> B\n```")
    );
}

#[test]
fn integration_keeps_the_source_in_the_latex_export_under_a_comment() {
    // PlotTests.swift §8: the one rich fence the app can draw itself still travels as source.
    let tex = tex(&lines(&["```plot", "x: -10..10", "sin(x)", "```"]));
    assert_contains(&tex, "% plot diagram source");
    assert_contains(&tex, "\\begin{verbatim}");
    assert_contains(&tex, "sin(x)");
}

#[test]
fn fence_language_folds_against_no_culture() {
    // .NET-only. The dispatch lowercases the info string; a culture-sensitive lowercase under
    // Turkish turns `MERMAID` into `mermaıd` and the diagram becomes plain code. (md.win runs this
    // under tr-TR; the fold here has no locale to consult.)
    let body = tex_body("```MERMAID\nA -> B\n```");
    assert_starts_with(&body, "% MERMAID diagram source");
    assert_eq!("\\[\nx^2\n\\]", tex_body("```MATH\nx^2\n```"));
    // The `data:` scheme test folds the same way.
    assert_contains(&tex_body("![](DATA:image/png;base64,iVBORw0KGgo=)"), "% md: image skipped");
}

// MARK: - Tables

#[test]
fn latex_table_column_spec_follows_the_alignments() {
    assert_eq!(
        lines(&[
            "\\begin{longtable}{lcr}",
            "\\hline",
            "\\textbf{L} & \\textbf{C} & \\textbf{R} \\\\",
            "\\hline",
            "\\endhead",
            "a & b & c \\\\",
            "\\hline",
            "\\end{longtable}",
        ]),
        tex_body("| L | C | R |\n|:--|:-:|--:|\n| a | b | c |")
    );
}

#[test]
fn latex_table_cells_are_escaped_and_keep_inline_markup() {
    assert_contains(&tex_body("| a | b |\n|---|---|\n| 50% | $x^2$ **b** |"), "50\\% & $x^2$ \\textbf{b} \\\\");
}

#[test]
fn latex_table_row_beginning_with_a_bracket_is_guarded() {
    // `\\` followed by `[` is a row with a vertical skip.
    assert_contains(&tex_body("| a |\n|---|\n| [draft] |"), "{}[draft] \\\\");
}

#[test]
fn latex_csv_block_becomes_the_same_table() {
    // The parse and the "a column of figures is right-aligned" rule are shared with the HTML renderer.
    let body = tex_body("```csv\nName,Qty\nBolt,12\nNut,3.5\n```");
    assert_contains_because(&body, "\\begin{longtable}{lr}", "the numeric column is right-aligned");
    assert_contains(&body, "\\textbf{Name} & \\textbf{Qty} \\\\");
    assert_contains(&body, "Bolt & 12 \\\\");
    // A tab-separated block is the same table.
    assert_contains(&tex_body("```tsv\nName\tQty\nBolt\t12\n```"), "\\begin{longtable}{lr}");
}

#[test]
fn a_row_wider_than_its_header_keeps_every_cell() {
    // Kotlin-only. The column count is the widest row, not the header's; the header's empty third
    // cell is not bold.
    let body = tex_body("```csv\na,b\nx,y,z\n```");
    assert_contains(&body, "\\begin{longtable}{lll}");
    assert_contains(&body, "\\textbf{a} & \\textbf{b} &  \\\\");
    assert_contains_because(&body, "x & y & z \\\\", "the third cell must survive");
}

#[test]
fn delimited_table_is_the_one_the_html_renderer_uses() {
    // Kotlin-only. The shared parse is what keeps the two exports agreeing.
    let table = DelimitedTable::from("Name,Qty\nBolt,12\nNut,3.5", ',').expect("a table");
    assert_eq!(vec!["Name", "Qty"], table.header);
    assert_eq!(vec![ColumnAlignment::Leading, ColumnAlignment::Trailing], table.alignments);
    assert_eq!(vec![vec!["Bolt", "12"], vec!["Nut", "3.5"]], table.rows);
}

#[test]
fn latex_unparseable_delimited_block_stays_code() {
    // Nothing the author wrote disappears: a ```csv block that parses to no table is still their text.
    assert_eq!("\\begin{verbatim}\n\n\\end{verbatim}", tex_body("```csv\n\n```"));
}

// MARK: - Quotes, rules and breaks

#[test]
fn latex_quotes_recurse() {
    assert_eq!(
        lines(&["\\begin{quote}", "outer", "", "\\begin{quote}", "inner", "\\end{quote}", "\\end{quote}"]),
        tex_body("> outer\n>\n> > inner")
    );
}

#[test]
fn latex_thematic_break_and_page_break() {
    assert_eq!("\\par\\noindent\\hrulefill\\par", tex_body("***"));
    assert_eq!("a\n\n\\newpage\n\nb", tex_body("a\n\n\\newpage\n\nb"));
}

#[test]
fn latex_private_notes_are_dropped() {
    assert_eq!("before\n\nafter", tex_body("before\n\n<!-- note: private -->\n\nafter"));
}

#[test]
fn latex_soft_breaks_become_line_breaks() {
    // md shows a soft break as a break in the preview, the HTML and the PDF; the .tex agrees.
    assert_eq!("one\\\\\ntwo\\\\\nthree", tex_body("one\ntwo\nthree"));
    // Never a trailing `\\` — "there's no line here to end" is an error.
    assert!(!tex_body("one\ntwo").ends_with("\\\\"));
}

#[test]
fn latex_soft_break_before_a_bracket_is_guarded() {
    // `\\` followed by `[` is read as `\\[length]`.
    assert_eq!("line\\\\\n{}[bracketed]", tex_body("line\n[bracketed]"));
}

#[test]
fn latex_soft_break_before_an_undefined_footnote_is_guarded() {
    // While the lines are being split the reference is still a token; the guard has to run on the
    // restored line, like the other two.
    assert_eq!("line one\\\\\n{}[\\textasciicircum{}missing] line two", tex_body("line one\n[^missing] line two"));
}

// MARK: - Footnotes

#[test]
fn latex_footnote_is_inlined_at_its_first_reference() {
    // What a LaTeX footnote *is* — which is why this export has no collected list at the foot.
    assert_eq!(
        "Text\\footnote{The note with \\emph{emphasis}.} here.",
        tex_body("Text[^a] here.\n\n[^a]: The note with *emphasis*.")
    );
}

#[test]
fn latex_repeated_footnote_references_cite_the_number() {
    assert_eq!(
        "A\\footnote{one} B\\footnote{two} C\\footnotemark[1].",
        tex_body("A[^a] B[^b] C[^a].\n\n[^a]: one\n\n[^b]: two")
    );
}

#[test]
fn footnote_numbers_follow_reading_order_not_splice_order() {
    // Kotlin-only. Replacements are built in reading order and only then spliced; build them
    // backwards and the texts land under the wrong marks.
    assert_eq!(
        "A\\footnote{n1} B\\footnotemark[1] C\\footnote{n2}.",
        tex_body("A[^a] B[^a] C[^b].\n\n[^a]: n1\n\n[^b]: n2")
    );
}

#[test]
fn latex_undefined_footnote_reference_stays_literal() {
    assert_eq!("Text[\\textasciicircum{}missing] here.", tex_body("Text[^missing] here."));
}

#[test]
fn latex_uncited_footnote_is_still_printed() {
    let body = tex_body("Prose.\n\n[^unused]: Nobody points at this.");
    assert_contains(&body, "% Footnotes defined but never referenced");
    assert_contains(&body, &format!("% Footnotes defined but never referenced {EM_DASH} kept so nothing is lost."));
    assert_contains(&body, "\\footnote{Nobody points at this.}");
}

#[test]
fn latex_footnote_inside_a_footnote_degrades_to_text() {
    // LaTeX cannot nest footnotes; the inner reference becomes text, and its definition — now
    // uncited — is printed at the end rather than lost.
    let body = tex_body("A[^a].\n\n[^a]: See[^b].\n\n[^b]: The target.");
    assert_contains(&body, "\\footnote{See[\\textasciicircum{}b].}");
    assert_contains(&body, "\\footnote{The target.}");
}

#[test]
fn latex_footnote_in_a_heading_is_protected() {
    // A section title is a moving argument — a bare `\footnote` there is a compile error.
    assert_eq!("\\section{Head\\protect\\footnote{note}}", tex_body("# Head[^a]\n\n[^a]: note"));
}

#[test]
fn latex_footnote_definition_renders_nothing_where_it_was_written() {
    assert_eq!("A\\footnote{note}.\n\nB.", tex_body("A[^a].\n\n[^a]: note\n\nB."));
}

// MARK: - Restricted places

#[test]
fn latex_display_math_in_a_table_cell_is_set_inline() {
    // `\[` inside a table stops the *file*, not the cell.
    let body = tex_body("| formula |\n|---------|\n| $$x^2$$ |");
    assert_contains(&body, "$x^2$ \\\\");
    assert_not_contains(&body, "\\[");
    assert_contains(&tex_body("| a |\n|---|\n| \\[x^2\\] |"), "$x^2$ \\\\");
}

#[test]
fn latex_display_math_in_a_footnote_is_set_inline() {
    assert_eq!("A\\footnote{$x^2$ ends it.}.", tex_body("A[^a].\n\n[^a]: $$x^2$$ ends it."));
}

#[test]
fn latex_image_in_a_table_cell_is_not_a_float() {
    // "LaTeX Error: Not in outer par mode." — and the whole document stops there.
    let body = tex_body("| picture |\n|---------|\n| ![A caption](a.png) |");
    assert_contains(&body, "\\includegraphics[width=\\linewidth]{a.png} \\emph{A caption} \\\\");
    assert_not_contains(&body, "\\begin{figure}");
    assert_not_contains(&body, "\\caption");
}

#[test]
fn latex_image_in_a_footnote_is_not_a_float() {
    let body = tex_body("Text[^a] here.\n\n[^a]: See ![A caption](a.png) for detail.");
    assert_contains(&body, "\\footnote{See \\includegraphics[width=\\linewidth]{a.png} \\emph{A caption} for detail.}");
    assert_not_contains(&body, "\\begin{figure}");
}

#[test]
fn latex_footnote_cited_from_a_table_cell_keeps_its_words() {
    // A `longtable` is not a float, so an ordinary `\footnote` sets its own words in the cell.
    let body = tex_body("| cell |\n|------|\n| A[^a] |\n\n[^a]: Zarquon lives here.");
    assert_contains(&body, "A\\footnote{Zarquon lives here.} \\\\");
    assert_not_contains(&body, "\\footnotemark");
    assert_not_contains(&body, "\\footnotetext");
}

#[test]
fn latex_footnote_numbers_keep_agreeing_across_a_table() {
    let body = tex_body(&lines(&[
        "| cell |",
        "|------|",
        "| A[^a] B[^a] |",
        "",
        "Then C[^b].",
        "",
        "[^a]: first",
        "",
        "[^b]: second",
    ]));
    assert_contains(&body, "A\\footnote{first} B\\footnotemark[1]");
    assert_contains(&body, "Then C\\footnote{second}.");
}

#[test]
fn latex_restriction_is_inherited_by_a_footnote_raised_from_a_table() {
    let body = tex_body("| a |\n|---|\n| A[^a] |\n\n[^a]: $$x^2$$ and ![cap](a.png)");
    assert_contains(&body, "\\footnote{$x^2$ and \\includegraphics[width=\\linewidth]{a.png} \\emph{cap}}");
    assert_not_contains(&body, "\\begin{figure}");
    assert_not_contains(&body, "\\[");
}

#[test]
fn latex_table_ends_at_its_own_end() {
    assert_ends_with(&tex_body("| a |\n|---|\n| b |"), "\\end{longtable}");
    assert_not_contains(&tex_body("| a |\n|---|\n| b |"), "\\footnotetext");
}

// MARK: - Books

#[test]
fn latex_book_uses_the_book_class_and_the_reading_order() {
    let tex = latex_export::book(&sample_book());
    assert_starts_with(&tex, "\\documentclass{book}\n");
    assert_contains(&tex, "\\title{The Book}");
    assert_contains(&tex, "\\maketitle");
    // Root articles first, then each chapter with its articles.
    let order = [
        "\\section{Preface}",
        "\\chapter{One}",
        "\\section{First}",
        "\\section{Second}",
        "\\chapter{Two}",
        "\\section{Third}",
    ];
    let mut cursor = 0;
    for piece in order {
        let found = scalar_text::first_index(&tex, piece, cursor)
            .unwrap_or_else(|| panic!("{piece} is missing or out of order"));
        cursor = found + piece.len();
    }
}

#[test]
fn latex_book_pushes_article_headings_below_their_section() {
    // An article is already a `\section`, so its own `#` has to become a `\subsection`.
    assert_contains(&latex_export::book(&sample_book()), "\\subsection{Inner}");
}

#[test]
fn latex_book_restarts_footnote_numbers_at_each_chapter() {
    // `book` resets the footnote counter at every `\chapter`: the third chapter's repeated note is
    // number 1 again, not number 3.
    assert_contains(&latex_export::book(&sample_book()), "C\\footnote{n3} C\\footnotemark[1].");
}

#[test]
fn book_footnote_numbers_run_on_across_articles() {
    // Kotlin-only. A chapter is one footnote counter, however many articles it holds — and an
    // uncited definition is printed as a real `\footnote`, so it takes a number too.
    let book = one_chapter(
        "C",
        vec![unit("Orphan", "Prose.\n\n[^x]: nobody cites this"), unit("Cites", "D[^d] D[^d].\n\n[^d]: n")],
    );
    assert_contains(&latex_export::book(&book), "D\\footnote{n} D\\footnotemark[2].");
}

#[test]
fn book_articles_do_not_borrow_each_others_footnotes() {
    // Kotlin-only. A definition belongs to the article it was written in.
    let book = one_chapter("C", vec![unit("Defines", "P[^a].\n\n[^a]: only here"), unit("Borrows", "Q[^a].")]);
    let tex = latex_export::book(&book);
    assert_contains(&tex, "P\\footnote{only here}.");
    assert_contains(&tex, "Q[\\textasciicircum{}a].");
}

#[test]
fn book_chapter_forgets_the_numbers_it_handed_out() {
    // Kotlin-only. Resetting the counter at a `\chapter` is only half of it: the id -> number map
    // has to go with it, or the second chapter cites the first chapter's number.
    let book = StructuredBook {
        title: "B".to_string(),
        front_units: vec![],
        sections: vec![
            section("One", vec![unit("A", "X[^a] Y[^a].\n\n[^a]: first")]),
            section("Two", vec![unit("B", "Z[^a] W[^a].\n\n[^a]: second")]),
        ],
    };
    let tex = latex_export::book(&book);
    assert_contains(&tex, "X\\footnote{first} Y\\footnotemark[1].");
    assert_contains(&tex, "Z\\footnote{second} W\\footnotemark[1].");
}

#[test]
fn latex_book_collects_packages_across_every_article() {
    // The preamble belongs to the whole file.
    let book = one_chapter("C", vec![unit("Plain", "Nothing special."), unit("Rich", "![a](x.png) ~~b~~")]);
    let tex = latex_export::book(&book);
    assert_contains(&tex, "\\usepackage{graphicx}");
    assert_contains(&tex, "\\usepackage[normalem]{ulem}");
}

#[test]
fn latex_book_titles_are_escaped() {
    let book =
        StructuredBook { title: "R&D 100%".to_string(), front_units: vec![], sections: vec![section("A_B", vec![])] };
    let tex = latex_export::book(&book);
    assert_contains(&tex, "\\title{R\\&D 100\\%}");
    assert_contains(&tex, "\\chapter{A\\_B}");
}

#[test]
fn latex_book_articles_each_number_their_own_notes() {
    // Two articles of one chapter, each numbering its own notes from `[^1]` — carrying the numbers
    // across made the second's `[^1]` a `\footnotemark[1]` citing the first's note.
    let book = one_chapter(
        "C",
        vec![unit("One", "First[^1].\n\n[^1]: The first note."), unit("Two", "Second[^1].\n\n[^1]: The second note.")],
    );
    let tex = latex_export::book(&book);
    assert_contains(&tex, "First\\footnote{The first note.}");
    assert_contains_because(
        &tex,
        "Second\\footnote{The second note.}",
        "the second article's own note must be printed",
    );
    assert_not_contains(&tex, "\\footnotemark");
}

// MARK: - Whole documents

#[test]
fn latex_document_is_well_formed() {
    let tex = tex("# H\n\nBody with $x$ and a [link](https://x.com).");
    assert_starts_with(&tex, "\\documentclass{article}\n");
    assert_ends_with(&tex, "\\end{document}\n");
    assert_eq!(1, occurrences(&tex, "\\begin{document}"));
    assert_eq!(1, occurrences(&tex, "\\end{document}"));
}

#[test]
fn latex_empty_document_still_compiles_as_one() {
    let tex = tex("");
    assert_contains(&tex, "\\begin{document}");
    assert_contains(&tex, "\\end{document}");
    // Three blank lines: the empty body between the two empty spacer lines.
    assert_eq!(
        "\\documentclass{article}\n\\usepackage[utf8]{inputenc}\n\\begin{document}\n\n\n\n\\end{document}\n",
        tex
    );
}

#[test]
fn title_overload_writes_no_title() {
    // The app-layer shape (core-api.md Part B) carries the file's name for the save step; a file
    // name is not part of a document, so nothing of it reaches the .tex.
    let tex = latex_export::document("Body.", "My File");
    assert_eq!(latex_export::document("Body.", ""), tex);
    assert_eq!(
        "\\documentclass{article}\n\\usepackage[utf8]{inputenc}\n\\begin{document}\n\nBody.\n\n\\end{document}\n",
        tex
    );
    assert_not_contains(&tex, "\\title");
    assert_not_contains(&tex, "My File");
}

#[test]
fn latex_no_command_it_emits_is_ever_escaped() {
    // After a document full of specials and markup, no command this file produced has been
    // through the escaper (`\textbackslash{}textbf` would be the symptom).
    let tex = tex(&lines(&[
        "**bold** with 100% & _under_ and `a_b`, a [link](https://x.com/a_b),",
        "![alt](p.png), $x_1^2$ and ~~gone~~.",
        "",
        "| a & b |",
        "|-------|",
        "| 50%   |",
    ]));
    assert_not_contains(&tex, "\\textbackslash{}text");
    assert_not_contains(&tex, "\\textbackslash{}begin");
    assert_not_contains_because(&tex, "\\{}", "no command's braces were escaped");
    assert_contains(&tex, "\\textbf{bold}");
    assert_contains(&tex, "$x_1^2$");
}

// MARK: - Scalar-exact string work (regression — third review)
//
// Swift's stdlib matches grapheme clusters, so every delimiter guard was blind to an ASCII
// character with a mark after it. Rust's `str` operations are on bytes and scalars; these pin that
// the operations chosen here keep firing.

#[test]
fn latex_every_escape_fires_with_a_combining_mark_after_it() {
    let table = [
        ("#", "\\#"),
        ("$", "\\$"),
        ("%", "\\%"),
        ("&", "\\&"),
        ("_", "\\_"),
        ("{", "\\{"),
        ("}", "\\}"),
        ("~", "\\textasciitilde{}"),
        ("^", "\\textasciicircum{}"),
        ("\\", "\\textbackslash{}"),
    ];
    for (special, escaped) in table {
        for mark in MARKS {
            assert_eq!(format!("a{escaped}{mark}b"), latex_export::escape(&format!("a{special}{mark}b")));
        }
    }
    // …and the URL table, which is a different set of characters.
    for (special, escaped) in [("%", "\\%"), ("#", "\\#"), ("{", "\\{"), ("}", "\\}"), ("\\", "\\textbackslash{}")] {
        for mark in MARKS {
            assert_eq!(format!("a{escaped}{mark}b"), latex_export::escape_url(&format!("a{special}{mark}b")));
        }
    }
}

#[test]
fn latex_every_guard_fires_with_a_combining_mark_after_it() {
    for mark in MARKS {
        // The bracket guard, at all three of its callers.
        assert_contains_because(&tex_body(&format!("- [{mark}draft] item")), "\\item {}[", "list item guard");
        assert_contains_because(&tex_body(&format!("line\n[{mark}draft]")), "\\\\\n{}[", "soft-break guard");
        assert_contains_because(&tex_body(&format!("| a |\n|---|\n| [{mark}draft] |")), "{}[", "table row guard");

        // The percent-decoder's own early exit.
        assert_eq!(format!("a%{mark}b"), latex_export::percent_decoded(&format!("a%{mark}b")));
        assert_contains_because(
            &tex_body(&format!("![](a%{mark}b.png)")),
            "% md: image skipped",
            "an unreadable path must still be refused",
        );
    }
}

#[test]
fn latex_verbatim_is_split_around_a_terminator_carrying_a_mark() {
    for mark in MARKS {
        let body = tex_body(&format!("```\nbefore\n\\end{{verbatim}}{mark}\nafter\n```"));
        assert_contains_because(&body, "\\verb|\\end{verbatim}|", "the terminator must be set with \\verb");
        assert_contains_because(&body, "after", "nothing after it may be lost");
        assert_eq!(2, occurrences(&body, "\\begin{verbatim}"));
    }
}

#[test]
fn latex_token_restore_survives_a_mark_after_the_token() {
    for mark in MARKS {
        let body = tex_body(&format!("A `code` span{mark} and $x^2${mark} after."));
        assert_not_contains_because(&body, E000, "no token may reach the file");
        assert_not_contains(&body, E001);
        assert!(scalar_text::contains(&body, "\\texttt{code}"));
        assert!(scalar_text::contains(&body, "$x^2$"));
    }
}

#[test]
fn latex_soft_breaks_split_on_every_newline_including_crlf() {
    // `\r\n` is one grapheme cluster in Swift; the split here is on bytes.
    assert_eq!(vec!["a\r", "b"], scalar_text::split("a\r\nb", "\n"));
    assert_eq!(vec!["a", "b", "c"], scalar_text::split("a\nb\nc", "\n"));
    assert_eq!(vec![""], scalar_text::split("", "\n"));
    assert_eq!(vec!["a", "b", "c"], scalar_text::split("aXbXc", "X"));
    assert_eq!(vec!["", "a", ""], scalar_text::split("XaX", "X"));
    assert_eq!(Some(1), "\r\n".find('\n'));
    // A CRLF document is normalised by the parser before it reaches the writer.
    assert_eq!("a\\\\\nb", tex_body("a\r\nb"));
}

#[test]
fn latex_scalar_helpers_are_exact_where_the_stdlib_is_not() {
    // Stated as the property the writer depends on. (The Swift-stdlib negative halves have no
    // counterpart: Rust's `str` agrees with ScalarText here.)
    let marked = format!("a%{ACUTE}b");
    assert!(scalar_text::contains(&marked, "%"));
    assert!(marked.contains('%'));

    assert!(scalar_text::has_prefix(&format!("[{VS16}x"), "["));
    assert!(format!("[{VS16}x").starts_with('['));

    assert!(scalar_text::has_suffix("x\n", "\n"));
    assert!(!scalar_text::has_suffix("x", "\n"));

    let tokened = format!("a{E000}0{E001}{ACUTE}b");
    let token = format!("{E000}0{E001}");
    assert_eq!(format!("a!{ACUTE}b"), scalar_text::replacing(&tokened, &token, "!"));
    assert_eq!(format!("a!{ACUTE}b"), tokened.replace(&token, "!"));
}

// MARK: - Tables break across pages (regression — third review)

#[test]
fn latex_table_is_a_longtable_and_not_a_float() {
    // A float cannot break across a page, so a table taller than one is *truncated* at exit 0.
    let rows = (1..=70).map(|i| format!("| r{i} | c{i} |")).collect::<Vec<_>>().join("\n");
    let body = tex_body(&format!("| A | B |\n|---|---|\n{rows}"));
    assert_starts_with(&body, "\\begin{longtable}{ll}");
    assert_ends_with(&body, "\\end{longtable}");
    assert_not_contains(&body, "\\begin{table}");
    assert_not_contains(&body, "\\begin{tabular}");
    for index in 1..=70 {
        assert_contains_because(&body, &format!("r{index} & c{index} \\\\"), &format!("row {index}"));
    }
}

#[test]
fn latex_longtable_repeats_its_header_on_every_page() {
    let body = tex_body("| A | B |\n|---|---|\n| a | b |");
    let head = body.find("\\endhead").expect("the header must be marked as one");
    assert_contains_because(&body[..head], "\\textbf{A} & \\textbf{B}", "the header row belongs above \\endhead");
    assert_contains_because(&body[head + "\\endhead".len()..], "a & b \\\\", "the body rows below it");
}

// MARK: - Images LaTeX cannot include (regression — third review)

#[test]
fn latex_image_path_with_braces_or_a_backslash_is_skipped() {
    // `escape_url` turns each of these into a control sequence, which is fatal for graphicx.
    for path in ["img/{a}.png", "img/a}.png", "img\\b.png"] {
        let body = tex_body(&format!("Before ![alt]({path}) after."));
        assert_not_contains_because(&body, "\\includegraphics", path);
        assert_contains_because(&body, "% md: image skipped", path);
        assert_contains_because(&body, "\\emph{alt}", &format!("the alt text survives {path}"));
        assert_contains_because(&body, "after.", "nothing after it is lost");
    }
    // A path with none of them is still an image.
    assert_contains(&tex_body("![](img/a-b_1.png)"), "\\includegraphics");
}

#[test]
fn latex_remote_and_data_images_are_skipped_and_named() {
    // TeX fetches nothing, and a `data:` URI is a picture with no file name at all.
    for path in ["https://nettrash.me/favicon.ico", "http://x.com/a.png", "data:image/png;base64,iVBORw0KGgo="] {
        let body = tex_body(&format!("![A picture]({path})"));
        assert_not_contains_because(&body, "\\includegraphics", path);
        assert_contains_because(&body, "% md: image skipped", path);
        assert_contains_because(&body, path, "the author is told which one");
        assert_contains_because(&body, "\\emph{A picture}", "the alt text survives");
    }
    // A relative path with a colon in it is a file somebody can really have.
    assert_contains(&tex_body("![](notes:draft.png)"), "\\includegraphics");
}

#[test]
fn latex_alt_text_survives_where_there_is_no_caption() {
    // Three places the author's own description of a picture used to be dropped without a word.
    assert_contains(&tex_body("| p |\n|---|\n| ![In a cell](a.png) |"), "\\emph{In a cell}");
    assert_contains(&tex_body("A[^a].\n\n[^a]: ![In a note](a.png)"), "\\emph{In a note}");
    assert_contains(&tex_body("![On a skipped one](a#b.png)"), "\\emph{On a skipped one}");
    // It is the Markdown the author wrote, rendered — not the raw text.
    assert_contains(&tex_body("| p |\n|---|\n| ![Alt with `c`](a.png) |"), "\\emph{Alt with \\texttt{c}}");
}

#[test]
fn latex_soft_break_after_an_image_alone_on_its_line_is_not_written() {
    // `\end{figure}\\` is "There's no line here to end" — a float begins no line.
    let figure = tex_body("![A caption](a.png)\nafter");
    assert_contains(&figure, "\\end{figure}\nafter");
    assert_not_contains(&figure, "\\end{figure}\\\\");

    // A skipped image with alt text does set a line, so the break after it is written.
    let skipped = tex_body("![Alt](https://x.com/a.png)\nafter");
    assert_contains(&skipped, "\\emph{Alt}");
    assert_contains(&skipped, "after");
    assert_not_contains_because(&skipped, "\\\\\n\n", "never a blank line after a \\\\");

    // Without alt text there is nothing but the comment, so no break before it and none after.
    let bare = tex_body("![](https://x.com/a.png)\nafter");
    assert_contains(&bare, "% md: image skipped");
    assert_contains(&bare, "after");
    assert_not_contains_because(&bare, "\\\\", "a comment begins no line to end");
    assert_not_contains_because(&bare, "\n\n", "and no paragraph break either");

    // An ordinary pair of lines still breaks.
    assert_eq!("one\\\\\ntwo", tex_body("one\ntwo"));
}

// MARK: - Mathematics that carries its own separators

#[test]
fn latex_display_math_with_its_own_separators_gets_an_aligned() {
    // `\[a &= b\]` is "Misplaced alignment tab character" in body text; in a table cell a
    // top-level `\\` ends the *row* from inside math mode.
    assert_eq!("\\[\\begin{aligned}a &= b \\\\ c &= d\\end{aligned}\\]", tex_body("$$a &= b \\\\ c &= d$$"));
    assert_contains(&tex_body("| f |\n|---|\n| $$a \\\\ b$$ |"), "$\\begin{aligned}a \\\\ b\\end{aligned}$ \\\\");
    // A formula that opens an environment of its own already owns its separators.
    assert_eq!("\\[\\begin{aligned} p &= q \\end{aligned}\\]", tex_body("$$\\begin{aligned} p &= q \\end{aligned}$$"));
    // And a formula with neither is left exactly as written.
    assert_eq!("\\[x^2\\]", tex_body("$$x^2$$"));
    // The helper itself.
    assert_eq!("x^2", latex_export::aligned("x^2"));
    assert_eq!("\\begin{aligned}a &= b\\end{aligned}", latex_export::aligned("a &= b"));
    assert_eq!("\\begin{cases} a \\\\ b \\end{cases}", latex_export::aligned("\\begin{cases} a \\\\ b \\end{cases}"));
}

#[test]
fn latex_amsmath_is_loaded_for_any_mathematics() {
    // `\begin{aligned}` in a ```math fence is "Environment aligned undefined" without it.
    for source in [
        "$x^2$",
        "$$x^2$$",
        "\\[x^2\\]",
        "\\(x^2\\)",
        "```math\n\\begin{aligned}a &= b\\end{aligned}\n```",
        "| f |\n|---|\n| $x$ |",
    ] {
        assert_contains_because(&tex_preamble(source), "\\usepackage{amsmath}", source);
    }
    assert_not_contains(&tex_preamble("Just prose."), "amsmath");
    assert_not_contains_because(
        &tex_preamble("```\n$x^2$\n```"),
        "amsmath",
        "a code block quoting a formula is not one",
    );
}

// MARK: - The last silent losses (regression — fourth review)

#[test]
fn latex_footnote_cited_from_a_table_header_keeps_its_words() {
    // A `longtable` typesets its header row *once*, into the box `\endhead` reinserts — and LaTeX
    // throws a footnote insertion made inside a box away. So the head carries the mark and the
    // note's text is written after the table.
    let body = tex_body("| h1[^1] | h2 |\n|---|---|\n| c1 | c2 |\n\n[^1]: Zarquon in the head.");
    assert_contains(&body, "\\textbf{h1\\stepcounter{footnote}\\footnotemark[1]}");
    assert_contains(&body, "\\end{longtable}\n\\footnotetext[1]{Zarquon in the head.}");
    assert_not_contains_because(&body, "\\footnote{Zarquon", "never an insertion inside the saved head box");

    // `\footnotemark[n]` does not step LaTeX's counter, so the head steps it by hand.
    let mixed = tex_body(&lines(&[
        "| ha[^a] | hb |",
        "|--------|----|",
        "| b[^b]  | c  |",
        "",
        "Tail[^c].",
        "",
        "[^a]: note a",
        "",
        "[^b]: note b",
        "",
        "[^c]: note c",
    ]));
    assert_contains(&mixed, "\\textbf{ha\\stepcounter{footnote}\\footnotemark[1]}");
    assert_contains(&mixed, "b\\footnote{note b}");
    assert_contains(&mixed, "Tail\\footnote{note c}.");
    assert_contains(&mixed, "\\end{longtable}\n\\footnotetext[1]{note a}");

    // A note cited again from a body cell is the number, as ever.
    assert_contains(&tex_body("| h[^a] |\n|---|\n| A[^a] |\n\n[^a]: n"), "A\\footnotemark[1] \\\\");

    // A note first cited from a body cell keeps the plain `\footnote` it always had.
    let plain = tex_body("| h |\n|---|\n| A[^a] |\n\n[^a]: n");
    assert_contains(&plain, "A\\footnote{n} \\\\");
    assert_not_contains(&plain, "\\footnotetext");
    assert_ends_with(&plain, "\\end{longtable}");
}

#[test]
fn latex_header_cell_carrying_an_alignment_tab_is_grouped() {
    // `\textbf` reads its argument with a delimited macro that a top-level `&` ends the row out
    // from under; an extra group is the whole fix.
    assert_contains(
        &tex_body("| $a &= b$ | h |\n|---|---|\n| c | d |"),
        "\\textbf{{$\\begin{aligned}a &= b\\end{aligned}$}}",
    );
    assert_contains(
        &tex_body("| [x](http://e.com/?a=1&b=2) | h |\n|---|---|\n| c | d |"),
        "\\textbf{{\\href{http://e.com/?a=1&b=2}{x}}}",
    );

    // Written only where there is a tab to guard.
    assert_contains(&tex_body("| A | B |\n|---|---|\n| a | b |"), "\\textbf{A} & \\textbf{B}");
    assert_contains(&tex_body("| a & b | B |\n|---|---|\n| c | d |"), "\\textbf{a \\& b} & \\textbf{B}");

    // A body cell needs none of it.
    assert_contains(&tex_body("| h |\n|---|\n| $a &= b$ |"), "$\\begin{aligned}a &= b\\end{aligned}$ \\\\");
}

#[test]
fn latex_image_path_with_a_double_quote_is_skipped() {
    // graphicx quotes a file name with spaces in a pair of `"`, so one the author wrote breaks
    // graphicx's own parser.
    assert!(latex_export::unreadable_image("a\"b.png").is_some());
    let body = tex_body("Before ![alt](a\"b.png) after.");
    assert_not_contains(&body, "\\includegraphics");
    assert_contains(&body, &format!("% md: image skipped {EM_DASH} a\"b.png"));
    assert_contains_because(&body, "\\emph{alt}", "the alt text survives");
    assert_contains_because(&body, "after.", "and so does the rest of the sentence");

    // A `"` in a *link* is refused nothing.
    assert_eq!(None, latex_export::unreadable_image("a-b_1.png"));
    assert_contains(&tex_body("[x](http://e.com/a\"b)"), "\\href{http://e.com/a\"b}{x}");
    // The two reasons, verbatim.
    assert_eq!(Some("is not a file name LaTeX can read"), latex_export::unreadable_image("a#b.png"));
    assert_eq!(
        Some("is a URL, and LaTeX has nothing to fetch it with"),
        latex_export::unreadable_image("https://x.com/a.png")
    );
    // A Windows drive letter is not a scheme (no `//`, and not `data:`), so it is refused for its
    // backslash and nothing else — and the same path with a forward slash is a file.
    assert_eq!(
        Some("is not a file name LaTeX can read"),
        latex_export::unreadable_image(&format!("C:{}x.png", chr(0x5C)))
    );
    assert_eq!(None, latex_export::unreadable_image("C:/x.png"));
}

#[test]
fn latex_authors_own_token_sentinels_cannot_be_read_as_tokens() {
    // U+E000 and U+E001 are what an inline pass wraps a span index in; a document carrying them of
    // its own had its *own* characters read back as a token index.
    assert_eq!("text \\textbf{b 0 x} more", tex_body(&format!("text **b {E000}0{E001} x** more")));
    // Every other place the author's text is read: a formula, a code span, a table cell, and the
    // front matter, which never reaches an inline pass at all.
    assert_eq!("A $x_1$ and \\texttt{cd} here.", tex_body(&format!("A $x{E000}_1{E001}$ and `c{E000}d` here.")));
    assert_contains(&tex_body(&format!("| h{E001}1 | b |\n|---|---|\n| c | d |")), "\\textbf{h1} & \\textbf{b}");
    assert_contains(&tex_preamble(&format!("---\ntitle: Ti{E000}tle\n---\n\nBody.")), "\\title{Title}");

    // The strip itself, and that it is the only thing it touches.
    assert_eq!("abc", latex_export::without_sentinels(&format!("a{E000}b{E001}c")));
    assert_eq!("plain", latex_export::without_sentinels("plain"));
    assert_eq!(
        format!("{}{}", chr(0xE002), chr(0xF8FF)),
        latex_export::without_sentinels(&format!("{}{}", chr(0xE002), chr(0xF8FF)))
    );
    // A string with nothing to strip comes back as the same string, not a copy.
    let untouched = "nothing here";
    assert!(matches!(latex_export::without_sentinels(untouched), Cow::Borrowed(same) if std::ptr::eq(same, untouched)));
}

#[test]
fn latex_skipped_image_names_its_file_below_the_alt_text() {
    // The alt text comes *first* and the comment after it: `%` runs to the end of its physical
    // line, so a comment in front of the words would swallow the words it is there to explain.
    assert_contains(
        &tex("![Alt words](https://x.com/a.png)"),
        &format!(
            "\\emph{{Alt words}}\n% md: image skipped {EM_DASH} https://x.com/a.png is a URL, and LaTeX has nothing to fetch it with.\n\n"
        ),
    );

    // And an image is a captioned float wherever it stands in body text.
    let sentence = tex_body("See ![the chart](c.png) for details.");
    assert_starts_with(&sentence, "See \\begin{figure}[ht]");
    assert_contains(&sentence, "\\caption{the chart}");
    assert_ends_with(&sentence, "\\end{figure} for details.");
}

// MARK: - The shipped corpus

/// Every environment this writer opens, and the one place it writes a bare terminator on purpose.
const ENVIRONMENTS: [&str; 8] =
    ["document", "figure", "longtable", "itemize", "enumerate", "quote", "verbatim", "aligned"];

#[track_caller]
fn assert_well_formed(tex: &str, document_class: &str, what: &str) {
    assert_starts_with(tex, &format!("\\documentclass{{{document_class}}}\n"));
    assert_ends_with(tex, "\\end{document}\n");
    assert_not_contains_because(tex, E000, &format!("{what} must carry no token"));
    assert_not_contains_because(tex, E001, what);
    for environment in ENVIRONMENTS {
        let opens = occurrences(tex, &format!("\\begin{{{environment}}}"));
        let mut closes = occurrences(tex, &format!("\\end{{{environment}}}"));
        // The one deliberate extra: a code block quoting the verbatim terminator has it set with
        // `\verb`, which is a terminator that closes nothing.
        if environment == "verbatim" {
            closes -= occurrences(tex, "\\verb|\\end{verbatim}|");
        }
        assert!(opens == closes, "{what}: {environment} opened {opens} times, closed {closes}");
    }
}

#[test]
fn every_shipped_example_exports_a_well_formed_document_and_book() {
    // The nine documents the app ships, run through the whole writer — the closest thing to the
    // compile check a machine with tectonic on it would do. They between them exercise every block
    // kind: tables, math, all four diagram fences, plots, images, task lists.
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(common::fixture_path("examples"))
        .expect("the examples fixture")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    files.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    assert_eq!(9, files.len());
    let mut units = Vec::new();
    for file in &files {
        let name = file.file_stem().expect("a file name").to_string_lossy().into_owned();
        let source = std::fs::read_to_string(file).expect("UTF-8 text");
        assert_well_formed(&tex(&source), "article", &name);
        units.push(unit(&name, &source));
    }

    // …and the same nine as one book, which is the other entry point and the only place the
    // chapter reset and the cross-article package collection run.
    let book = StructuredBook {
        title: "Examples".to_string(),
        front_units: vec![],
        sections: vec![section("Examples", units)],
    };
    let tex = latex_export::book(&book);
    assert_well_formed(&tex, "book", "the example book");
    assert_contains(&tex, "\\chapter{Examples}");
    assert_eq!(9, occurrences(&tex, "\\section{"));
}

// MARK: - sets_something, the soft-break oracle

#[test]
fn sets_something_sees_through_comments_and_floats_but_not_escapes() {
    assert!(!latex_export::sets_something(""));
    assert!(!latex_export::sets_something("  \t\r\n"));
    assert!(!latex_export::sets_something("% a comment\n  % another"));
    assert!(!latex_export::sets_something("\\begin{figure}[ht]\n\\centering\n\\end{figure}"));
    assert!(!latex_export::sets_something("\\begin{figure} never closed"));
    assert!(latex_export::sets_something("\\end{figure} words"));
    assert!(latex_export::sets_something("% comment\nthen words"));
    // An escaped percent is a character, not a comment — deliberately no backslash case.
    assert!(latex_export::sets_something("\\% of it"));
    assert!(latex_export::sets_something("\\emph{Alt}\n% md: image skipped"));
}
