//! The ordering-prefix grammar behind Rename… / Move Up / Move Down and every display title (md.win
//! `Book/BookNaming.cs`).
//!
//! A sibling name is `prefix + stem + "." + ext`, where the prefix is a leading ASCII number plus
//! its separators ("01-", "2. ") and the extension only counts when it is one of the article
//! extensions (a chapter folder "v1.2" keeps its dot in the stem). Port of
//! `BookLibrary.splitExtension / splitPrefix / displayName / renamedName` (md.macOS
//! BookNavigator.swift).
//!
//! The two existing ports disagree on the prefix grammar. Swift accepts the separators `- . )
//! space` and needs none ("3rd party" → prefix "3", stem "rd party"); Kotlin accepts `- . space _`
//! and demands at least one ("3rd party" is unnumbered, "3_draft" has prefix "3_"). macOS is the
//! source of truth, so this file follows Swift; the naming tests pin both disputed names so the
//! choice is visible.

use crate::text::unicode::{self, GeneralCategory};

/// File extensions treated as articles — the plain-text types the document side reads.
pub const ARTICLE_EXTENSIONS: [&str; 3] = ["md", "markdown", "txt"];

/// Swift `"-.) "` — the characters that may follow the digits of an ordering prefix.
const PREFIX_SEPARATORS: &[u8] = b"-.) ";

/// Whether a file with this name is an article: an article extension after the last dot, any
/// case. "draft.md.bak" is not, "SHOUTY.MD" is, ".md" (a dotfile) is not — the listing never shows
/// dotfiles anyway.
pub fn is_article_name(name: &str) -> bool {
    !split_extension(name).1.is_empty()
}

/// A sibling name split into the rest and its article extension (without the dot, original case
/// — [`renamed_name`] puts it back as written). Only an article extension counts; anything else,
/// or a dot at position 0, leaves the name whole.
pub fn split_extension(name: &str) -> (&str, &str) {
    let Some(dot) = name.rfind('.') else {
        return (name, "");
    };
    if dot == 0 {
        return (name, "");
    }
    let ext = &name[dot + 1..];
    // lower_invariant (md.win's ToLowerInvariant) mirrors Swift's lowercased(): the Kelvin sign
    // folds to "k", which an upper-casing compare would not reproduce. Irrelevant for real names,
    // exact for parity.
    let lowered = unicode::lower_invariant(ext);
    if !ARTICLE_EXTENSIONS.contains(&lowered.as_str()) {
        return (name, "");
    }
    (&name[..dot], ext)
}

/// A base name (extension already removed) split into its ordering prefix — the leading ASCII
/// digits and the separator run after them — and the display stem. No digits → no prefix. Nothing
/// left after digits and separators ("2026", "01-") → no prefix either: an all-number name keeps
/// the number as its stem, there would be nothing to display otherwise.
pub fn split_prefix(base_name: &str) -> (&str, &str) {
    let digits = leading_ascii_digits(base_name);
    let bytes = base_name.as_bytes();
    let mut index = digits;
    while index < bytes.len() && PREFIX_SEPARATORS.contains(&bytes[index]) {
        index += 1;
    }
    if digits == 0 || index >= bytes.len() {
        return ("", base_name);
    }
    (&base_name[..index], &base_name[index..])
}

/// The name as the writer reads it: prefix and article extension stripped. Pre-fills Rename….
pub fn display_name(name: &str) -> &str {
    split_prefix(split_extension(name).0).1
}

/// The on-disk name after a rename to the display name `stem`: the ordering prefix survives
/// exactly as written (a loose "2. " is not normalised — only a Move does that) and so does the
/// extension.
pub fn renamed_name(name: &str, stem: &str) -> String {
    let (base_name, ext) = split_extension(name);
    let prefix = split_prefix(base_name).0;
    if ext.is_empty() { format!("{prefix}{stem}") } else { format!("{prefix}{stem}.{ext}") }
}

/// Length (in bytes, which here are ASCII digits) of the leading run of ASCII '0'…'9'. Swift walks
/// `Character`s, so a digit carrying a combining mark ("1\u{301}") is one non-ASCII grapheme and
/// ends the run *before* that digit; a scalar scan would count it. Unicode digits (U+0663 "٣")
/// never count on either side.
pub(crate) fn leading_ascii_digits(s: &str) -> usize {
    let mut chars = s.chars().peekable();
    let mut n = 0;
    while let Some(c) = chars.next() {
        if !c.is_ascii_digit() {
            break;
        }
        if chars.peek().is_some_and(|&next| is_grapheme_extender(next)) {
            break;
        }
        n += 1;
    }
    n
}

/// GB9/GB9a: a mark or joiner that glues onto the preceding code point.
fn is_grapheme_extender(c: char) -> bool {
    matches!(c, '\u{200C}' | '\u{200D}')
        || matches!(
            unicode::category(c),
            GeneralCategory::NonSpacingMark | GeneralCategory::SpacingCombiningMark | GeneralCategory::EnclosingMark
        )
}
