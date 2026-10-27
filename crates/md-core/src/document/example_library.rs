//! The File ▸ Examples model (md.win `Document/ExampleLibrary.cs`, md.macOS/md/mdApp.swift
//! `ExampleLibrary`) as a pure function over a folder listing.
//!
//! The caller lists the TOP LEVEL of the bundled `examples/` folder only — the `Example Book/`
//! subfolder is deliberately not a menu item; it unpacks whole via Example Book…. Opening an
//! example is the app's job: read as UTF-8, create an untitled document with the text, mark it
//! dirty so closing offers to save.

use std::cmp::Ordering;

use crate::book::book_order;
use crate::text::graphemes;

/// One bundled example: the file name as shipped and the menu label derived from it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Example {
    pub file_name: String,
    pub name: String,
}

/// The examples the menu lists, in menu order, from the file names of one folder. Keeps only
/// `.md` files (ordinal, as Bundle's resource lookup and Kotlin's `extension == "md"`; a bare
/// `.md` has no extension in Foundation and is not listed) and sorts them with
/// [`compare_natural`].
pub fn from_listing<I, S>(file_names: I) -> Vec<Example>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut names: Vec<String> =
        file_names.into_iter().map(|name| name.as_ref().to_string()).filter(|name| is_example_file(name)).collect();
    names.sort_by(|a, b| compare_natural(a, b));
    names
        .into_iter()
        .map(|file_name| {
            let name = display_name(&file_name).to_string();
            Example { file_name, name }
        })
        .collect()
}

/// A `.md` file with a non-empty stem — case-sensitive, so `x.MD` is not listed.
pub fn is_example_file(file_name: &str) -> bool {
    file_name.len() > 3 && file_name.ends_with(".md")
}

/// Menu label: the stem without its ordering prefix — `"01-Welcome.md"` → `"Welcome"`. The prefix
/// is ASCII digits then `-` (Swift `isASCII && isNumber`; never a Unicode digit test), and it is
/// stripped ONLY if something remains: `"01-"` stays `"01-"`. Android's regex has no such guard and
/// yields `""` there; the Mac is the source of truth. Defined for the names [`from_listing`] feeds
/// it — only a `.md` suffix is dropped.
pub fn display_name(file_name: &str) -> &str {
    // deletingPathExtension: ".md" alone has no extension in Foundation, so only a non-empty stem loses it.
    let stem = if is_example_file(file_name) { &file_name[..file_name.len() - 3] } else { file_name };
    let bytes = stem.as_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || digits >= stem.len() || bytes[digits] != b'-' {
        return stem;
    }
    // Swift compares Characters: a dash wearing a combining mark, a ZWJ or a variation selector is
    // one cluster that is not "-", and the prefix stays. The same UAX #29 clusters here.
    if graphemes::next_len(&stem[digits..]) != 1 {
        return stem;
    }
    let rest = &stem[digits + 1..];
    if rest.is_empty() { stem } else { rest }
}

/// Finder order (Swift `localizedStandardCompare`) for the ASCII names the bundle ships,
/// hand-written so no culture, ICU build or locale table is consulted and every machine lists the
/// menu the same way:
///
/// 1. Letters compare without regard to case, runs of ASCII digits by numeric value, and
///    everything else by the root-collation order of the ASCII punctuation — TAB, space, then
///    ``_ - , ; : ! ? . ' " ( ) [ ] { } @ * / \ & # % ` ^ + < = > | ~ $`` — with all punctuation
///    before every digit and every digit before every letter (so `Tables (copy).md` lists before
///    `Tables.md` and `a_b` before `a.b`). A name that runs out first lists first.
/// 2. When that leaves two names equal, one more walk from the left, where a digit run is told
///    apart by its leading zeros (fewer first) and a letter by its case (lower first); whichever
///    difference comes first decides — `1-A` < `01-a`, but `a01` < `A1`.
///
/// Equal only for identical strings, so the order is total. Non-ASCII is outside the contract: it
/// compares by UTF-16 code unit after every ASCII character, as md.win does. md.win carries two
/// identical copies of this algorithm (here and `BookOrder.NaturalCompare`); this port keeps one,
/// [`book_order::natural_compare`], so the Examples menu and the book sidebar can never drift.
pub fn compare_natural(a: &str, b: &str) -> Ordering {
    book_order::natural_compare(a, b)
}
