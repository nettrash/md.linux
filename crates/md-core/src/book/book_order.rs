//! Reading order for sibling names (md.win `Book/BookOrder.cs`, Swift `BookLibrary.ordered /
//! leadingNumber`).
//!
//! A leading integer sorts first, numerically ("2. setup" before "10-ending"); numbered before
//! unnumbered; ties and the rest in Finder-style natural order.
//!
//! Swift's fallback is `localizedStandardCompare`, which neither .NET nor Rust has.
//! [`natural_compare`] reproduces it for ASCII names by hand — the algorithm a 33,600-pair random
//! differential against Foundation found exact for the Examples menu, and a 36,000-pair
//! differential over this comparator (random ASCII, mixed-script, and realistic book names)
//! agreed with on every ASCII pair. Non-ASCII is outside the contract and compares by UTF-16 code
//! unit after every ASCII character, as md.win does (Foundation weighs "é" as "e" plus an accent,
//! "ß" as "ss"): the two Foundation-faithful ports and Android's `naturalCompare` all differ there.
//! The earlier Kotlin-style stand-in (per-unit lowercase, ordinal punctuation) disagreed with
//! Foundation on 8.6% of random ASCII pairs and 4.2% of realistic names — "1_x" before "1-x", "1)"
//! before "1." — and would have listed the book sidebar and the Examples menu in two different
//! orders inside one app. Never a locale collation (glibc's `strcoll`, GLib's
//! `g_utf8_collate_key_for_filename`): that is a third order again, and differs per machine.

use std::cmp::Ordering;

use super::book_naming;

/// The integer prefix of a name ("01-intro" → 1), or `None` when there is none. A run too long for
/// i64 ("99999999999999999999-y") is unnumbered rather than an overflow — Swift's `Int(digits)`
/// returns nil there too. No sign handling: "-3 degrees" is unnumbered.
pub fn leading_number(name: &str) -> Option<i64> {
    let count = book_naming::leading_ascii_digits(name);
    if count == 0 {
        return None;
    }
    name[..count].parse::<i64>().ok()
}

/// Finder order (Swift `localizedStandardCompare`) for ASCII names, hand-written so no locale,
/// ICU build or collation table is consulted and every machine lists the book the same way:
///
/// 1. Letters compare without regard to case, runs of ASCII digits by numeric value (no integer
///    parse, so a 40-digit run cannot overflow), and everything else by the root-collation order
///    of the ASCII punctuation — TAB, space, then ``_ - , ; : ! ? . ' " ( ) [ ] { } @ * / \ & # %
///    ` ^ + < = > | ~ $`` — with all punctuation before every digit and every digit before every
///    letter (so `Tables (copy)` lists before `Tables` and `a_b` before `a.b`). A name that runs
///    out first lists first.
/// 2. When that leaves two names equal, one more walk from the left, where a digit run is told
///    apart by its leading zeros (fewer first) and a letter by its case (lower first); whichever
///    difference comes first decides — `1-A` < `01-a`, but `a01` < `A1`.
///
/// Equal only for identical strings, so the order is total and a stable sort needs no tie-break.
/// The walk is over UTF-16 code units, exactly as md.win's, so the non-ASCII tail orders the same
/// way too (a supplementary-plane character before U+E000…U+FFFF, as UTF-16 has it).
pub fn natural_compare(a: &str, b: &str) -> Ordering {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if is_ascii_digit(a[i]) && is_ascii_digit(b[j]) {
            let run_a = digit_run_length(&a, i);
            let run_b = digit_run_length(&b, j);
            let by_value = compare_value(&a[i..i + run_a], &b[j..j + run_b]);
            if by_value != Ordering::Equal {
                return by_value;
            }
            i += run_a;
            j += run_b;
            continue;
        }
        let by_weight = primary_weight(a[i]).cmp(&primary_weight(b[j]));
        if by_weight != Ordering::Equal {
            return by_weight;
        }
        i += 1;
        j += 1;
    }
    if i < a.len() {
        return Ordering::Greater;
    }
    if j < b.len() {
        return Ordering::Less;
    }

    // Same letters, same numbers: the second walk. Equal primary weights mean the two elements are
    // either digit runs (compared here by their zeros) or the same letter (compared by its case).
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if is_ascii_digit(a[i]) {
            let run_a = digit_run_length(&a, i);
            let run_b = digit_run_length(&b, j);
            let by_zeros = leading_zeros(&a[i..i + run_a]).cmp(&leading_zeros(&b[j..j + run_b]));
            if by_zeros != Ordering::Equal {
                return by_zeros;
            }
            i += run_a;
            j += run_b;
            continue;
        }
        if a[i] != b[j] {
            return if is_ascii_lower(a[i]) { Ordering::Less } else { Ordering::Greater };
        }
        i += 1;
        j += 1;
    }
    Ordering::Equal
}

/// The full book ordering: numbered first by number, natural order for ties and the rest.
pub fn compare(a: &str, b: &str) -> Ordering {
    match (leading_number(a), leading_number(b)) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| natural_compare(a, b)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => natural_compare(a, b),
    }
}

/// Swift's strict `ordered(a, b)`: a sorts before b.
pub fn ordered(a: &str, b: &str) -> bool {
    compare(a, b) == Ordering::Less
}

// TAB and space first, then the punctuation in root-collation order — the sequence Foundation
// returned for the 33 printable non-alphanumeric ASCII characters plus TAB, alone and between
// letters.
const PUNCTUATION_ORDER: &[u8] = b"\t _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

const ASCII_WEIGHT: [u32; 128] = build_ascii_weights();

const fn build_ascii_weights() -> [u32; 128] {
    let mut weights = [0u32; 128];
    // Other controls never appear in a file name; ordered by code unit ahead of everything so the
    // order stays total.
    let mut c = 0;
    while c < 128 {
        weights[c] = c as u32;
        c += 1;
    }
    let mut k = 0;
    while k < PUNCTUATION_ORDER.len() {
        weights[PUNCTUATION_ORDER[k] as usize] = 100 + k as u32;
        k += 1;
    }
    let mut d = b'0';
    while d <= b'9' {
        weights[d as usize] = 200;
        d += 1;
    }
    let mut l = b'a';
    while l <= b'z' {
        weights[l as usize] = 300 + (l - b'a') as u32;
        weights[(l - 32) as usize] = 300 + (l - b'a') as u32;
        l += 1;
    }
    weights
}

fn primary_weight(unit: u16) -> u32 {
    if unit < 128 { ASCII_WEIGHT[unit as usize] } else { 1000 + u32::from(unit) }
}

fn digit_run_length(s: &[u16], start: usize) -> usize {
    s[start..].iter().take_while(|&&unit| is_ascii_digit(unit)).count()
}

/// Zeros ahead of the value — a run of nothing but zeros keeps its last digit as the value.
fn leading_zeros(run: &[u16]) -> usize {
    let mut zeros = 0;
    while zeros + 1 < run.len() && run[zeros] == u16::from(b'0') {
        zeros += 1;
    }
    zeros
}

/// Numeric order for runs of any length — no integer parse, so a 40-digit run cannot overflow.
fn compare_value(a: &[u16], b: &[u16]) -> Ordering {
    let sa = &a[leading_zeros(a)..];
    let sb = &b[leading_zeros(b)..];
    sa.len().cmp(&sb.len()).then_with(|| sa.cmp(sb))
}

fn is_ascii_digit(unit: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&unit)
}

fn is_ascii_lower(unit: u16) -> bool {
    (u16::from(b'a')..=u16::from(b'z')).contains(&unit)
}
