//! Port of md.win `ViewmodeAdversarialTests`: adversarial review pins for the view-mode / examples
//! / page-size module (2026-09-06). Every expected value was derived from a sibling port, never
//! from md.win's C#: the Swift codec and `Example.name` logic run verbatim on macOS, Foundation's
//! localizedStandardCompare and CharacterSet.whitespaces on the same machine (en_GB), Python
//! hashlib for SHA-256, and the Kotlin suite where it pins a literal. The last case pins three
//! guards a mutation campaign found no other test killing. Non-ASCII is spelled as escapes on
//! purpose, so the fused delimiters stay visible in a diff.

use std::cmp::Ordering::{self, Equal, Greater, Less};

use md_core::document::example_library as examples;
use md_core::document::page_size::PageSize;
use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_memory::{self as memory, Entry, InMemoryViewModeStore};

const ID1: &str = "53ba23f60734adf1";
const ID2: &str = "427542472354b900";

fn e(identity: &str, mode: ViewMode) -> Entry {
    Entry::new(identity, mode)
}

fn hex16(i: usize) -> String {
    format!("{i:016x}")
}

fn decode(text: &str) -> Vec<Entry> {
    memory::decode(Some(text))
}

#[test]
fn decode_treats_fused_delimiters_and_line_endings_like_swift_characters() {
    // A combining mark after LF: UAX #29 breaks after a control, so the LF still splits; the mark then rides
    // on the next identity (17 units, first one not hex) and only that line is lost.
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n{ID1} edit\n\u{0301}{ID2} split")));
    // A mark on the token, on the header, or on the identity's last digit; a ZWJ in the identity: nothing readable.
    assert!(decode(&format!("v1\n{ID1} edit\u{0301}")).is_empty());
    assert!(decode(&format!("v1\u{0301}\n{ID1} edit")).is_empty());
    assert!(decode("v1\n53ba23f60734adf\u{0301} edit").is_empty());
    assert!(decode("v1\n53ba23f60734adf\u{200D} edit").is_empty());
    // NBSP is not the field separator; around the token it is Foundation whitespace and is forgiven, as are ZWSP and TAB.
    assert!(decode(&format!("v1\n{ID1}\u{00A0}edit")).is_empty());
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n{ID1} \u{00A0}edit\u{00A0}")));
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n{ID1} \u{200B}edit\u{200B}")));
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n{ID1} \tedit\t")));

    // CRLF: "\r\n" is one Character in Swift, so a trailing CRLF rides on the token and the line is dropped
    // (Kotlin's trim() would read one entry here; the Mac is the source of truth). A bare trailing LF is fine.
    assert!(decode(&format!("v1\n{ID1} edit\r\n")).is_empty());
    assert!(decode("v1\r\n").is_empty());
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n{ID1} edit\n")));
    assert_eq!(vec![e(ID1, ViewMode::Edit)], decode(&format!("v1\n\n\n{ID1} edit\n\n")));
    assert_eq!(None, memory::mode_for_token("edit\r\n"));
    assert_eq!(None, memory::mode_for_token("\r\nedit"));
    assert_eq!(None, memory::mode_for_token("edit\u{0085}"));
    assert_eq!(None, memory::mode_for_token("edit\u{FEFF}"));
    // U+2028, NEL and a BOM are not line structure for this codec: line 0 is then not "v1".
    assert!(decode(&format!("v1\u{2028}{ID1} edit")).is_empty());
    assert!(decode(&format!("v1\u{0085}{ID1} edit")).is_empty());
    assert!(decode(&format!("\u{FEFF}v1\n{ID1} edit")).is_empty());
    // Full Unicode lowercasing never spells a token from a non-ASCII letter: fullwidth E, long s, Kelvin sign.
    assert!(decode(&format!("v1\n{ID1} \u{FF25}DIT")).is_empty());
    assert!(decode(&format!("v1\n{ID1} \u{017F}plit")).is_empty());
    assert_eq!(None, memory::mode_for_token("\u{212A}"));
}

#[test]
fn supplementary_plane_and_empty_inputs_match_the_siblings() {
    // An astral identity is 16 graphemes to Swift and 17 UTF-16 units to md.win; both reject it, and only its line is lost.
    assert!(!memory::is_identity("0123456789abcde\u{1F600}"));
    assert_eq!(vec![e(ID1, ViewMode::Split)], decode(&format!("v1\n0123456789abcde\u{1F600} edit\n{ID1} split")));
    // SHA-256 over UTF-8: four bytes for U+1F600, not two surrogates (Python hashlib).
    assert_eq!("d95eee229ad0f4a5", memory::sha256_prefix("file:/tmp/\u{1F600}.md"));
    assert_eq!("e3b0c44298fc1c14", memory::sha256_prefix(""));
    // NFC and NFD spellings are different bytes and different identities on every port: nothing normalises.
    assert_eq!("b3dc02924f7ab070", memory::sha256_prefix("file:/tmp/\u{00E9}.md"));
    assert_eq!("570313911bc0885e", memory::sha256_prefix("file:/tmp/e\u{0301}.md"));
    // Example names (Swift `Example.name` run verbatim): an astral stem is a stem; an emoji modifier fused onto the
    // dash makes it not the Character "-"; a lone dash or space after the prefix is a non-empty rest.
    assert_eq!("\u{1F600}", examples::display_name("01-\u{1F600}.md"));
    assert_eq!("01-\u{1F3FB}x", examples::display_name("01-\u{1F3FB}x.md"));
    assert_eq!("-", examples::display_name("5--.md"));
    assert_eq!(" ", examples::display_name("12- .md"));
    assert_eq!("9-\u{0308}", examples::display_name("9-\u{0308}.md"));
    assert_eq!("00-", examples::display_name("00-.md"));
    assert_eq!("foo.", examples::display_name("foo..md"));
    assert_eq!("foo ", examples::display_name("foo .md"));

    // Empty everything.
    assert!(decode("").is_empty());
    assert_eq!("v1", memory::encode(&[]));
    assert!(!memory::is_identity(""));
    assert_eq!(None, memory::mode_for_token("   \u{3000}"));
    assert_eq!(vec![e(ID1, ViewMode::Edit)], memory::touched(&[], ID1, ViewMode::Edit));
    let store = InMemoryViewModeStore::new();
    assert_eq!(None, memory::lookup(ID1, &store));
    assert_eq!(1, store.reads());
    assert_eq!(0, store.writes());
    memory::remember(ViewMode::Edit, ID1, &store);
    assert_eq!(Some(format!("v1\n{ID1} edit")), store.value());
    assert_eq!(1, store.writes());
    assert_eq!(Equal, examples::compare_natural("", ""));
    assert_eq!(Less, examples::compare_natural("", "a"));
    assert_eq!(Less, examples::compare_natural("", "\t"));
    assert_eq!("", examples::display_name(""));
    assert!(!examples::is_example_file(""));
    assert!(examples::from_listing(Vec::<String>::new()).is_empty());
    assert!(examples::from_listing(["", ".md", "md", "Example Book"]).is_empty());
}

#[test]
fn pathological_sizes_neither_cap_decode_nor_overflow_compare() {
    // Decode has no cap on any port, only touched does: 5,000 distinct identities all come back, in order.
    let ids: Vec<String> = (0..5000).map(hex16).collect();
    let big = format!("v1\n{}", ids.iter().map(|id| format!("{id} preview")).collect::<Vec<_>>().join("\n"));
    let decoded = decode(&big);
    assert_eq!(5000, decoded.len());
    assert_eq!(ids, decoded.iter().map(|e| e.identity.clone()).collect::<Vec<_>>());

    // One touch collapses it to the newest 200 with the new entry first; the encoding has exactly 200 line
    // breaks and the length the format dictates (2 + 200 x 18 + one "edit" + 199 x "preview").
    let touched = memory::touched(&decoded, "ffffffffffffffff", ViewMode::Edit);
    assert_eq!(200, touched.len());
    assert_eq!("ffffffffffffffff", touched[0].identity);
    assert_eq!(ids[198], touched.last().unwrap().identity);
    let encoded = memory::encode(&touched);
    assert_eq!(200, encoded.matches('\n').count());
    assert_eq!(2 + 200 * 18 + 4 + 199 * 7, encoded.len());
    assert_eq!(touched, decode(&encoded));
    // Through the store, one remember rewrites a 5,000-entry value as 200.
    let store = InMemoryViewModeStore::with_value(big);
    memory::remember(ViewMode::Split, ID1, &store);
    assert_eq!(200, memory::entries(&store).len());
    assert_eq!(Some(ViewMode::Split), memory::lookup(ID1, &store));

    // Tens of thousands of CRLF lines, or a value that is nothing but separators, read as absent without incident.
    assert!(decode(&format!("v1\r\n{}", format!("{ID1} edit\r\n").repeat(20000))).is_empty());
    assert!(decode(&format!("v1{}", "\n".repeat(100000))).is_empty());
    assert!(decode(&format!("v1\n{}", " ".repeat(100000))).is_empty());

    // 100,000-digit runs compare by value with no integer parse; a huge ordering prefix is still stripped.
    let zeros = "0".repeat(100000);
    let nines = "9".repeat(100000);
    assert_eq!(Less, examples::compare_natural(&format!("{nines}.md"), &format!("1{zeros}.md")));
    assert_eq!(Equal, examples::compare_natural(&format!("1{zeros}"), &format!("1{zeros}")));
    assert_eq!(Greater, examples::compare_natural(&format!("{zeros}1"), "1"));
    assert_eq!("x", examples::display_name(&format!("1{zeros}-x.md")));
    assert_eq!(format!("1{zeros}-"), examples::display_name(&format!("1{zeros}-.md")));
    assert_eq!(None, memory::mode_for_token(&format!("{}editx", " ".repeat(100000))));
    assert_eq!(Some(ViewMode::Edit), memory::mode_for_token(&format!("{}EDIT", "\u{3000}".repeat(100000))));
}

/// Rows Foundation's `localizedStandardCompare` returned on macOS for pairs the main theory does not
/// reach (TAB against space, zeros against zeros, punctuation soup), drawn from a 33,600-pair random
/// differential that produced no mismatch.
const FOUNDATION_ADVERSARIAL_PAIRS: &[(&str, Ordering, &str)] = &[
    ("a\t", Less, "a "),
    ("\t", Less, " "),
    ("0", Less, "00"),
    ("01", Less, "001"),
    ("a 1", Less, "a1"),
    ("x", Less, "X"),
    ("d", Less, "D"),
    (" -a", Less, " -A"),
    ("a1", Less, "A1"),
    ("00.", Greater, "."),
    ("\t\"\\4", Less, "+)?cd2"),
    (" 0a ", Less, "1..aA"),
    ("c7c", Greater, "<*e"),
    ("0 -0A", Less, "10.-10 "),
    ("A.A.- 1 ", Greater, "-a 0AA0"),
    ("A A", Less, "AA0Aaa-"),
    ("aa1 A", Greater, "0  11"),
    (">$@%a:", Greater, "\\#,3;2"),
    ("]._y", Less, "9}6"),
    ("Aaa", Greater, "-Aa.1A-"),
    ("- a0.1-0", Less, ".. 0a1 "),
    ("C9", Greater, "87@:=4"),
    (".-A0", Less, "A."),
    ("3*[*!C", Less, "Z\t9"),
    (".aAa ", Greater, "-110"),
    ("  ", Less, " 0A1A 0"),
    ("10a-", Less, "A.0A-"),
    ("-a-", Less, "A.a1-"),
    (")+ ", Less, "a2ezz"),
    (")>", Less, "ZD=AX\t"),
    ("0 A0 0a1", Less, "11.0"),
    ("a. ...- ", Greater, "0 0aa-a"),
    (" --A", Less, "11..a"),
    (" a", Less, "-.0."),
    ("--.aA-1", Less, "A "),
    ("a{7e", Greater, "';({(7"),
    ("A.aa0-.", Greater, " a"),
    ("4:&>,,]", Less, "a] ]^>9"),
];

#[test]
fn compare_natural_agrees_with_foundation_on_adversarial_ascii() {
    assert_eq!(38, FOUNDATION_ADVERSARIAL_PAIRS.len());
    for &(a, sign, b) in FOUNDATION_ADVERSARIAL_PAIRS {
        assert_eq!(sign, examples::compare_natural(a, b), "{a:?} vs {b:?}");
        assert_eq!(sign.reverse(), examples::compare_natural(b, a), "{b:?} vs {a:?}");
    }
}

#[test]
fn guards_no_other_test_killed() {
    // 200 is an on-disk contract shared by three apps, so it is pinned as a LITERAL (as the Kotlin suite does),
    // not through the constant the implementation reads: MAX_ENTRIES = 201 passed every other test.
    assert_eq!(200, memory::MAX_ENTRIES);
    let mut list: Vec<Entry> = Vec::new();
    for i in 0..250 {
        list = memory::touched(&list, &hex16(i), ViewMode::Edit);
    }
    assert_eq!(200, list.len());
    assert_eq!(hex16(249), list[0].identity);
    assert_eq!(hex16(50), list.last().unwrap().identity);

    // Half away from zero, as Swift's .rounded() and Kotlin's roundToInt: none of the seven shipped sizes lands
    // on .5, so banker's rounding passed every other test. 48 x 43.84375 / 841.8 is exactly 2.5 in IEEE doubles
    // and 56 x 15.942857142857145 / 595.2 exactly 1.5: "3px 2px", where ToEven would print "2px 2px".
    let half = PageSize { id: "half", label: "Half", width: 15.942857142857145, height: 43.84375 };
    assert_eq!(2.5, 48.0 * half.height / PageSize::A4.height);
    assert_eq!(1.5, 56.0 * half.width / PageSize::A4.width);
    assert_eq!("3px 2px", half.css_padding());

    // Named is exact and case-sensitive. named("A4") == A4 cannot tell a case fold from the fallback (the Kotlin
    // pin has the same blind spot); a wrong-case or padded id of a NON-default size must fall back to A4.
    assert_eq!(&PageSize::US_LETTER, PageSize::named(Some("letter")));
    assert_eq!(&PageSize::A4, PageSize::named(Some("LETTER")));
    assert_eq!(&PageSize::A4, PageSize::named(Some("Legal")));
    assert_eq!(&PageSize::A4, PageSize::named(Some("6X9")));
    assert_eq!(&PageSize::A4, PageSize::named(Some(" letter")));
    assert_eq!(&PageSize::A4, PageSize::named(Some("letter ")));
}
