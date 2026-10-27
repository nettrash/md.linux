//! Adversarial parity tests added by review of the `.tex` writer. Port of md.win
//! `LatexAdversarialTests.cs`.
//!
//! **Every expected value below came from the real Swift.** `md.macOS/md/LaTeXExport.swift` (with
//! `MarkdownParser`, `MarkdownHTML`, `ScalarText` and `Plot`) was compiled with `swiftc` into a
//! stdin/stdout oracle and run on these exact inputs; the strings here are its output, never a
//! port's read back. The oracle decodes its input with `String(decoding:as:UTF8.self)` —
//! `String(data:encoding:)` silently eats a leading U+FEFF and would have hidden every BOM case.
//!
//! Each test names the surviving mutant that motivated it. Mutation testing of md.win's 121-case
//! suite killed 23 of 26 mutants; these are the three that lived, each proved non-equivalent by
//! running the mutant over a 32,023-document corpus and diffing against that same Swift oracle.

#[path = "common/latex.rs"]
mod latex_common;

use latex_common::{chr, lines as join, tex};
use md_core::export::latex_export;

/// The body between `\begin{document}` and `\end{document}`, with its newlines trimmed.
#[track_caller]
fn body(source: &str) -> String {
    let tex = tex(source);
    let opening = "\\begin{document}\n";
    let closing = "\n\\end{document}";
    let (Some(start), Some(end)) = (tex.find(opening), tex.find(closing)) else {
        panic!("the document is not wrapped in a document environment: <{tex}>");
    };
    tex[start + opening.len()..end].trim_matches('\n').to_string()
}

/// Surviving mutant M15: `guarded` resuming at the end of a match instead of one position after
/// its start when a word guard rejects it. Nothing in the 121 cases moved.
///
/// It is not equivalent, and the reason is that the closing delimiter of a rejected match can be
/// the *opening* delimiter of a good one: in `a_b _c_` the leftmost match is `_b _`, rejected
/// because `a` precedes it, and the `_` that ended it opens the real emphasis. ICU restarts its
/// scan one code unit on from the failed attempt, so the Swift finds `_c_`; skipping past the whole
/// rejected match loses it and prints `\_c\_`.
#[test]
fn a_rejected_word_guard_resumes_one_code_unit_on_not_past_the_whole_match() {
    // Swift oracle, verbatim.
    assert_eq!(r"a\_b \emph{c}", body("a_b _c_"));
    assert_eq!(r"9\_b \emph{c}", body("9_b _c_"));
    assert_eq!(r"a\$b $c$", body("a$b $c$"));
    // The same with an astral letter doing the rejecting — U+1D504 MATHEMATICAL FRAKTUR CAPITAL A
    // is a letter here as it is to ICU.
    assert_eq!(format!(r"{}\_b \emph{{c}}", chr(0x1D504)), body(&format!("{}_b _c_", chr(0x1D504))));
}

/// Surviving mutant M26: the dollar guard built without its `$`, dropping it from the formula
/// pattern's `[\p{L}\p{N}_$]` guard class. All 121 cases passed — the currency case
/// (`"$5 and $10"`) is decided by the digit in the class, not the dollar.
///
/// The guard earns its `$` on a run of three or more: `$x$$` is not a formula in any port,
/// because the character after the closing delimiter is another dollar. Without it the writer
/// opens math mode on half a display delimiter, and `$a&b$$` additionally invents an `aligned`
/// around text the author never meant as mathematics.
#[test]
fn a_dollar_is_a_word_character_to_the_formula_guard_on_both_sides() {
    // Swift oracle, verbatim: none of these is a formula.
    assert_eq!(r"\$x\$\$", body("$x$$"));
    assert_eq!(r"\$a\&b\$\$", body("$a&b$$"));
    assert_eq!(r"\$\$x\$", body("$$x$"));
    assert_eq!(r"\$a\$\$b\$", body("$a$$b$"));
    // …while the same spans with an ordinary neighbour still are, so the guard is not simply off.
    assert_eq!("$x$", body("$x$"));
    assert_eq!("$x$ .", body("$x$ ."));
}

/// Surviving mutant M19: hoisting the footnote-reference pass above the image and link passes.
/// All 121 cases passed, because a footnote reference cannot appear in a link *label* or an
/// image's *alt* — `[^\]]` stops at the `]` that `[^a]` must contain.
///
/// It reaches the title form, whose `(.*?)` crosses a `]` happily. Run in the Swift's order the
/// title is consumed with the raw `[^a]` inside it, the note is never cited and the orphan trailer
/// prints it; run early, the reference becomes a whole `\footnote`, is swallowed by the title, and
/// the author's note is on no page while the counter still moved. The bracketed label is the
/// second half: `[x [^a]](u)` is not a link at all in the Swift.
#[test]
fn footnote_references_convert_after_images_and_links_not_before() {
    // Swift oracle, verbatim: the title is consumed whole, so the note stays uncited and the
    // orphan trailer is what prints it.
    assert_eq!(
        join(&[
            r"\begin{figure}[ht]",
            r"\centering",
            r"\includegraphics[width=\linewidth]{p.png}",
            r"\caption{x}",
            r"\end{figure}",
            r"",
            &format!(r"% Footnotes defined but never referenced {} kept so nothing is lost.", chr(0x2014)),
            r"\footnote{the note}",
        ]),
        body("![x](p.png \"see [^a]\")\n\n[^a]: the note\n")
    );

    // And a label carrying a reference is not a link: the label pattern cannot cross the `]`.
    assert_eq!(r"[x \footnote{the note}](u)", body("[x [^a]](u)\n\n[^a]: the note\n"));
    assert_eq!(r#"[x \footnote{the note}](u "t")"#, body("[x [^a]](u \"t\")\n\n[^a]: the note\n"));
}

/// The two spelled-out ICU classes in the link and image patterns, measured against the real
/// engine rather than argued: an `NSRegularExpression` probe on a Mac reports `\s` =
/// `[\t\n\v\f\r\u0085\p{Z}]` (U+180E, U+200B and U+FEFF are **out**) and `.` as excluding exactly
/// `\n \v \f \r U+0085 U+2028 U+2029`. Every value below is the Swift oracle's output for that
/// input.
///
/// This is where an engine's own metacharacters would part company: .NET's `.` excludes only
/// `\n`, so a title carrying U+2028 or NEL would keep matching and the link would silently survive
/// a line terminator the Swift refuses.
#[test]
fn link_url_and_title_use_icu_whitespace_and_icu_dot_not_dot_nets_own() {
    // URL class `[^)\s]+`: a separator ends the URL (no link, since no title follows), a format
    // character does not (it is part of the URL).
    for separator in [0x1680, 0x00A0, 0x0085, 0x000B] {
        let c = chr(separator);
        assert_eq!(format!("[a](x{c}y)"), body(&format!("[a](x{c}y)")), "U+{separator:04X}");
    }
    for format_character in [0x200B, 0xFEFF, 0x180E] {
        let c = chr(format_character);
        assert_eq!(format!(r"\href{{x{c}y}}{{a}}"), body(&format!("[a](x{c}y)")), "U+{format_character:04X}");
    }

    // Title class `.`: a line terminator kills the match, a zero-width character does not.
    for terminator in [0x0085, 0x2028, 0x000B] {
        let c = chr(terminator);
        assert_eq!(format!("[a](u \"one{c}two\")"), body(&format!("[a](u \"one{c}two\")")), "U+{terminator:04X}");
    }
    assert_eq!(r"\href{u}{a}", body(&format!("[a](u \"one{}two\")", chr(0x200B))));
    assert_eq!(r"\href{u}{a}", body(&format!("[a](u \"one{}two\")", chr(0xFEFF))));
}

/// `percent_decoded` on the bytes Swift cannot express and .NET can be talked into mangling: a
/// four-byte sequence must decode as one scalar (not two U+FFFD), a surrogate encoded as UTF-8 and
/// an overlong NUL must both be refused whole, and a decoded U+0000 is a file name character like
/// any other rather than a terminator. Swift oracle, verbatim.
#[test]
fn percent_decoding_is_whole_scalars_and_never_repairs_what_it_cannot_read() {
    assert_eq!(
        join(&[
            r"\begin{figure}[ht]",
            r"\centering",
            &format!(r"\includegraphics[width=\linewidth]{{{}.png}}", chr(0x1F600)),
            r"\caption{a}",
            r"\end{figure}",
        ]),
        body("![a](%F0%9F%98%80.png)")
    );

    // A UTF-8-encoded lone surrogate and an overlong NUL are not valid UTF-8, so the path is
    // handed back with its `%` intact — and a `%` is a file name `\includegraphics` refuses.
    assert_eq!(
        join(&[
            r"\emph{a}",
            &format!(r"% md: image skipped {} %ED%A0%80.png is not a file name LaTeX can read.", chr(0x2014)),
        ]),
        body("![a](%ED%A0%80.png)")
    );
    assert_eq!(
        join(&[
            r"\emph{a}",
            &format!(r"% md: image skipped {} a%C0%80b.png is not a file name LaTeX can read.", chr(0x2014))
        ]),
        body("![a](a%C0%80b.png)")
    );
    assert_eq!(
        join(&[
            r"\emph{a}",
            &format!(r"% md: image skipped {} trailing%2 is not a file name LaTeX can read.", chr(0x2014))
        ]),
        body("![a](trailing%2)")
    );

    // `%00` is valid UTF-8 for U+0000 and decodes; the scalar goes into the argument as itself.
    assert_eq!(
        join(&[
            r"\begin{figure}[ht]",
            r"\centering",
            &format!(r"\includegraphics[width=\linewidth]{{x{}y.png}}", chr(0x0000)),
            r"\caption{a}",
            r"\end{figure}",
        ]),
        body("![a](x%00y.png)")
    );

    assert_eq!(format!("{}.png", chr(0x1F600)), latex_export::percent_decoded("%F0%9F%98%80.png"));
    assert_eq!("%ED%A0%80.png", latex_export::percent_decoded("%ED%A0%80.png"));
}

/// One document exercising most of the writer at once, pinned to the byte against the Swift:
/// front matter (escaped title, a case-folded `AUTHOR` key, an invented `\date{}`), the Cyrillic
/// `fontenc` trigger hidden in a Latin-looking author name, package order, a footnote first cited
/// from a moving argument and then from a table head and a block quote, an `aligned` supplied in
/// both a heading and a header cell, the bracket guards on a row and on a soft-broken line, a
/// percent-decoded figure, a diagram fence, a code block that closes its own `verbatim`, nested
/// quotes, and the orphan trailer.
///
/// A whole-file assertion is the point: the per-feature tests all pass under a preamble whose
/// packages are reordered or a body whose blank-line spacing has drifted.
#[test]
fn a_whole_document_is_byte_for_byte_what_the_swift_writes() {
    let author = format!("AUTHOR: n{}ttrash", chr(0x0435));
    let source = join(&[
        "---",
        "title: 100% & _more_",
        // A Cyrillic small letter IE (U+0435) inside an otherwise Latin word: the T2A trigger has
        // to see the title block, not just the body.
        &author,
        "---",
        "",
        r"# H[^1] with $a &= b$",
        "",
        r"| h[^1] | $p \\ q$ |",
        "|:--|--:|",
        r"| [x] | c & d |",
        "| solo |",
        "",
        "text",
        "![cap **b**](my%20dir/a.png)",
        "more [l](u \"t\")",
        "[^2] tail",
        "",
        "```dot",
        "digraph{a->b}",
        "```",
        "",
        "```",
        r"\end{verbatim}",
        "```",
        "",
        "> quote [^1]",
        "> > deep",
        "",
        "[^1]: note **with** `code`",
        "[^3]: orphan",
        "",
    ]);

    let expected_author = format!(r"\author{{n{}ttrash}}", chr(0x0435));
    let dot_comment =
        format!(r"% dot diagram source {} LaTeX has no renderer for it, so it is kept as written.", chr(0x2014));
    let trailer = format!(r"% Footnotes defined but never referenced {} kept so nothing is lost.", chr(0x2014));
    let expected = join(&[
        r"\documentclass{article}",
        r"\usepackage[utf8]{inputenc}",
        r"\usepackage[T1,T2A]{fontenc}",
        r"\usepackage{amsmath}",
        r"\usepackage{graphicx}",
        r"\usepackage{longtable}",
        r"\usepackage{hyperref}",
        r"\title{100\% \& \_more\_}",
        &expected_author,
        r"\date{}",
        r"\begin{document}",
        r"\maketitle",
        r"",
        r"\section{H\protect\footnote{note \textbf{with} \texttt{code}} with $\begin{aligned}a &= b\end{aligned}$}",
        r"",
        r"\begin{longtable}{lr}",
        r"\hline",
        r"\textbf{h\footnotemark[1]} & \textbf{$\begin{aligned}p \\ q\end{aligned}$} \\",
        r"\hline",
        r"\endhead",
        r"{}[x] & c \& d \\",
        r"solo &  \\",
        r"\hline",
        r"\end{longtable}",
        r"",
        r"text",
        r"\begin{figure}[ht]",
        r"\centering",
        r"\includegraphics[width=\linewidth]{my dir/a.png}",
        r"\caption{cap \textbf{b}}",
        r"\end{figure}\\",
        r"more \href{u}{l}\\",
        r"{}[\textasciicircum{}2] tail",
        r"",
        &dot_comment,
        r"\begin{verbatim}",
        r"digraph{a->b}",
        r"\end{verbatim}",
        r"",
        r"\noindent\verb|\end{verbatim}|\par",
        r"",
        r"\begin{quote}",
        r"quote \footnotemark[1]",
        r"",
        r"\begin{quote}",
        r"deep",
        r"\end{quote}",
        r"\end{quote}",
        r"",
        &trailer,
        r"\footnote{orphan}",
        r"",
        r"\end{document}",
        r"",
    ]);

    assert_eq!(expected, tex(&source));
}
