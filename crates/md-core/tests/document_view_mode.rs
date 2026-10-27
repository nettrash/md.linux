//! Port of md.win `ViewModeTests` — itself md.macOS/mdTests/ViewModeTests.swift (all eight cases)
//! plus the Kotlin-only width cases from md.Android's ViewModeTest.kt and the pins for the places
//! a port has to decide something the Swift could not tell it: the path identity (on Linux:
//! case-sensitive, see `canonical_path_collapses_the_spelling_and_folds_case_only_when_asked`).

mod book_support;

use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use book_support::{TempDir, combine, temp_path, unique};
use md_core::book::book_paths;
use md_core::document::book_article_opens::{self, BookArticleOpens};
use md_core::document::sha256;
use md_core::document::view_mode::ViewMode;
use md_core::document::view_mode_memory::{self as memory, Entry, InMemoryViewModeStore};
use md_core::document::view_mode_rule as rule;

// MARK: The rule

#[test]
fn open_rule_truth_table() {
    // A known file opens in exactly the mode it was left in, on any width, even when empty right now.
    for is_wide in [true, false] {
        for remembered in ViewMode::ALL {
            assert_eq!(remembered, rule::open_view_mode(Some(remembered), false, true, is_wide));
            assert_eq!(remembered, rule::open_view_mode(Some(remembered), true, true, is_wide));
        }
    }

    // Nothing to read yet: Edit, whatever the width, with or without a file.
    for is_wide in [true, false] {
        assert_eq!(ViewMode::Edit, rule::open_view_mode(None, true, false, is_wide));
        assert_eq!(ViewMode::Edit, rule::open_view_mode(None, true, true, is_wide));
    }

    // Unknown with content: Split where Split is on offer, Preview only where it is not.
    assert_eq!(ViewMode::Split, rule::open_view_mode(None, false, true, true));
    assert_eq!(ViewMode::Preview, rule::open_view_mode(None, false, true, false));
    // has_file_identity never changes the answer — an example opened from the menu (content, no file) opens like a document.
    assert_eq!(ViewMode::Split, rule::open_view_mode(None, false, false, true));
    assert_eq!(ViewMode::Preview, rule::open_view_mode(None, false, false, false));
}

#[test]
fn remembered_split_survives_a_narrow_open() {
    let store = InMemoryViewModeStore::new();
    let identity = memory::identity_for(&combine(&[&temp_path(), "md-viewmode-missing", "split.md"]));
    memory::remember(ViewMode::Split, &identity, &store);

    let remembered = memory::lookup(&identity, &store);
    assert!(remembered.is_some());
    let opened = rule::open_view_mode(remembered, false, true, false);
    assert_eq!(ViewMode::Split, opened);
    assert_eq!(ViewMode::Edit, rule::effective_mode(opened, false));

    // Storing what the narrow window opened with must not downgrade the memory.
    memory::remember(opened, &identity, &store);
    assert_eq!(Some(ViewMode::Split), memory::lookup(&identity, &store));
    assert_eq!(ViewMode::Split, rule::effective_mode(opened, true));
}

// Kotlin-only: narrowOffersEditAndPreviewOnly, wideOffersAllThree, storedSplitCollapsesToEditWhenNarrow,
// storedSplitStaysSplitWhenWide, editAndPreviewSurviveBothWidths, coercionRoundTripsWhenWindowWidensAgain.
#[test]
fn width_rule_offers_split_only_where_there_is_room_and_coerces_without_discarding() {
    assert_eq!(&[ViewMode::Edit, ViewMode::Preview], rule::available_modes(false));
    assert_eq!(&[ViewMode::Edit, ViewMode::Split, ViewMode::Preview], rule::available_modes(true));
    assert_eq!(&ViewMode::ALL, rule::available_modes(true));

    assert_eq!(ViewMode::Edit, rule::effective_mode(ViewMode::Split, false));
    assert_eq!(ViewMode::Split, rule::effective_mode(ViewMode::Split, true));
    for is_wide in [true, false] {
        assert_eq!(ViewMode::Edit, rule::effective_mode(ViewMode::Edit, is_wide));
        assert_eq!(ViewMode::Preview, rule::effective_mode(ViewMode::Preview, is_wide));
    }
    // The same stored value, narrow then wide: Split comes back because the preference was never rewritten.
    const STORED: ViewMode = ViewMode::Split;
    assert_eq!(ViewMode::Edit, rule::effective_mode(STORED, false));
    assert_eq!(ViewMode::Split, rule::effective_mode(STORED, true));
}

// MARK: Tokens

#[test]
fn mode_tokens_are_the_three_literals() {
    assert_eq!("edit", memory::token(ViewMode::Edit));
    assert_eq!("split", memory::token(ViewMode::Split));
    assert_eq!("preview", memory::token(ViewMode::Preview));

    for mode in ViewMode::ALL {
        let token = memory::token(mode);
        assert_eq!(Some(mode), memory::mode_for_token(token));
        assert_eq!(Some(mode), memory::mode_for_token(&token.to_ascii_uppercase()));
        // Swift `capitalized`: first letter up, the rest down.
        let capitalized = format!("{}{}", token[..1].to_ascii_uppercase(), &token[1..]);
        assert_eq!(Some(mode), memory::mode_for_token(&capitalized));
    }

    // Whitespace is forgiven, exactly as the Android port's modeFromToken forgives it.
    assert_eq!(Some(ViewMode::Split), memory::mode_for_token("  Split "));
    // A token that names nothing is None, never fatal.
    assert_eq!(None, memory::mode_for_token(""));
    assert_eq!(None, memory::mode_for_token("zen"));
    assert_eq!(None, memory::mode_for_token("reader"));
}

#[test]
fn mode_for_token_trims_foundation_whitespace_and_folds_ascii_case_only() {
    // Foundation `.whitespaces`: Zs, TAB and U+200B are trimmed; line terminators are not.
    assert_eq!(Some(ViewMode::Edit), memory::mode_for_token("\tedit\u{00A0}"));
    assert_eq!(Some(ViewMode::Edit), memory::mode_for_token("\u{200B}edit\u{3000}"));
    assert_eq!(Some(ViewMode::Edit), memory::mode_for_token("\u{2003}edit\u{202F}"));
    assert_eq!(None, memory::mode_for_token("edit\r"));
    assert_eq!(None, memory::mode_for_token("edit\n"));
    assert_eq!(None, memory::mode_for_token("\u{180E}edit"));
    // Swift's lowercased() is a full mapping, so no non-ASCII letter ever spells one of the three words:
    // İ (U+0130) lowercases to i̇ and ı (U+0131) stays ı. A Unicode-aware fold would accept both.
    assert_eq!(None, memory::mode_for_token("ED\u{0130}T"));
    assert_eq!(None, memory::mode_for_token("ed\u{0131}t"));
}

#[test]
fn raw_value_parses_the_three_spellings_exactly() {
    for mode in ViewMode::ALL {
        assert_eq!(Some(mode), ViewMode::from_raw_value(Some(mode.raw_value())));
    }
    // Mode(rawValue:) is exact: the per-window default takes over for anything else.
    assert_eq!(None, ViewMode::from_raw_value(Some("Split")));
    assert_eq!(None, ViewMode::from_raw_value(Some(" split")));
    assert_eq!(None, ViewMode::from_raw_value(None));
    assert_eq!(ViewMode::Split, ViewMode::from_raw_value(None).unwrap_or(ViewMode::WINDOW_DEFAULT));
    assert_eq!("md.viewMode", ViewMode::WINDOW_STATE_KEY);
    assert_eq!(vec!["Edit", "Split", "Preview"], ViewMode::ALL.map(ViewMode::label).to_vec());
    assert_eq!(vec!["1", "2", "3"], ViewMode::ALL.map(ViewMode::command_key).to_vec());
}

// MARK: Codec

#[test]
fn codec_round_trips_and_rejects_a_foreign_header() {
    let entries = vec![
        Entry::new("53ba23f60734adf1", ViewMode::Preview),
        Entry::new("427542472354b900", ViewMode::Split),
        Entry::new("0123456789abcdef", ViewMode::Edit),
    ];
    let encoded = memory::encode(&entries);
    assert_eq!("v1\n53ba23f60734adf1 preview\n427542472354b900 split\n0123456789abcdef edit", encoded);
    assert_eq!(entries, memory::decode(Some(&encoded)));
    // An empty memory is still a well-formed one.
    assert_eq!("v1", memory::encode(&[]));
    assert!(memory::decode(Some("v1")).is_empty());

    // Line 0 must read exactly `v1`, or the value is treated as absent.
    assert!(memory::decode(Some("")).is_empty());
    assert!(memory::decode(None).is_empty());
    assert!(memory::decode(Some("v2\n53ba23f60734adf1 edit")).is_empty());
    assert!(memory::decode(Some(" v1\n53ba23f60734adf1 edit")).is_empty());
    assert!(memory::decode(Some("53ba23f60734adf1 edit")).is_empty());
    assert!(memory::decode(Some("{\"53ba23f60734adf1\":\"edit\"}")).is_empty());

    // A malformed line is skipped, never fatal, and a repeated identity keeps only its newest (first) row.
    let survivor = Entry::new("53ba23f60734adf1", ViewMode::Edit);
    let malformed = [
        "v1",
        "not-hex-at-all edit",
        "53ba23f60734adf1extra edit",
        "53BA23F60734ADF1 edit",
        "427542472354b900 zen",
        "427542472354b900",
        "427542472354b900 edit split",
        "",
        "53ba23f60734adf1 edit",
        "53ba23f60734adf1 preview",
    ]
    .join("\n");
    assert_eq!(vec![survivor], memory::decode(Some(&malformed)));

    // The mode is parsed BEFORE the identity is marked seen — the exact value the Android suite pins.
    assert_eq!(
        vec![Entry::new("53ba23f60734adf1", ViewMode::Split)],
        memory::decode(Some("v1\n53ba23f60734adf1 zen\n53ba23f60734adf1 split"))
    );
}

#[test]
fn decode_splits_lines_on_lf_and_fields_on_space_exactly_like_swift() {
    // Swift's split(separator: " ") drops empty fields: a double space is still two fields …
    assert_eq!(
        vec![Entry::new("53ba23f60734adf1", ViewMode::Edit)],
        memory::decode(Some("v1\n53ba23f60734adf1  edit"))
    );
    assert_eq!(
        vec![Entry::new("53ba23f60734adf1", ViewMode::Edit)],
        memory::decode(Some("v1\n 53ba23f60734adf1 edit "))
    );
    // … a tab is not a separator on any port …
    assert!(memory::decode(Some("v1\n53ba23f60734adf1\tedit")).is_empty());
    // … a trailing tab on the token is trimmed by mode(forToken:), a tab on the identity is not …
    assert_eq!(1, memory::decode(Some("v1\n53ba23f60734adf1 edit\t")).len());
    assert!(memory::decode(Some("v1\n53ba23f60734adf1\t edit")).is_empty());
    // … and a CRLF-written value reads as absent everywhere (the header line is "v1\r").
    assert!(memory::decode(Some("v1\r\n53ba23f60734adf1 edit")).is_empty());
    // A CRLF past the header is where the ports part: Swift's "\r\n" is one Character, so the line never
    // splits and its three fields are skipped; Kotlin splits on the code unit and trims the "\r" away,
    // reading two entries. The Mac is the source of truth — nothing, not two, and never one.
    assert!(memory::decode(Some("v1\n53ba23f60734adf1 edit\r\n427542472354b900 split")).is_empty());
    // A lone CR is not a separator and not Foundation whitespace; a bare LF still splits.
    assert!(memory::decode(Some("v1\n53ba23f60734adf1 edit\r427542472354b900 split")).is_empty());
    assert_eq!(2, memory::decode(Some("v1\n53ba23f60734adf1 edit\n427542472354b900 split")).len());
    // A space fused with a combining mark is one Character that is not " ", so it does not split a line.
    assert!(memory::decode(Some("v1\n53ba23f60734adf1 \u{0301}edit")).is_empty());
    // No port writes uppercase, so the decoder does not take it.
    assert!(!memory::is_identity("53BA23F60734ADF1"));
    assert!(!memory::is_identity("53ba23f60734adf"));
    assert!(!memory::is_identity("\u{FF15}\u{FF13}ba23f60734adf1"));
    assert!(memory::is_identity("0123456789abcdef"));
}

// MARK: MRU

#[test]
fn touched_is_mru_and_truncates_at_two_hundred() {
    let (a, b, c) = ("aaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbb", "cccccccccccccccc");
    let mut list: Vec<Entry> = Vec::new();
    list = memory::touched(&list, a, ViewMode::Edit);
    list = memory::touched(&list, b, ViewMode::Split);
    list = memory::touched(&list, c, ViewMode::Preview);
    assert_eq!(vec![c, b, a], identities(&list));

    // Touching a file already in the list moves it to the front and updates its mode — never a duplicate.
    list = memory::touched(&list, a, ViewMode::Preview);
    assert_eq!(vec![a, c, b], identities(&list));
    assert_eq!(3, list.len());
    assert_eq!(ViewMode::Preview, list[0].mode);

    // The cap: exactly 200 survive, and it is the oldest that falls off.
    let mut big: Vec<Entry> = Vec::new();
    for index in 0..memory::MAX_ENTRIES {
        big = memory::touched(&big, &identity(index), ViewMode::Edit);
    }
    assert_eq!(memory::MAX_ENTRIES, big.len());
    assert_eq!(identity(0), big.last().unwrap().identity);

    big = memory::touched(&big, &identity(memory::MAX_ENTRIES), ViewMode::Split);
    assert_eq!(memory::MAX_ENTRIES, big.len());
    assert_eq!(identity(memory::MAX_ENTRIES), big[0].identity);
    assert_eq!(identity(1), big.last().unwrap().identity);
    assert!(!big.iter().any(|e| e.identity == identity(0)));

    // …and the same through the real store, round-tripped as text.
    let store = InMemoryViewModeStore::new();
    for index in 0..=memory::MAX_ENTRIES {
        memory::remember(ViewMode::Edit, &identity(index), &store);
    }
    assert_eq!(memory::MAX_ENTRIES, memory::entries(&store).len());
    assert_eq!(None, memory::lookup(&identity(0), &store));
    assert_eq!(Some(ViewMode::Edit), memory::lookup(&identity(memory::MAX_ENTRIES), &store));
    // An unknown file stays unknown — that is what makes the open rule's first branch reachable at all.
    assert_eq!(None, memory::lookup("ffffffffffffffff", &store));
    assert_eq!("md.viewModeMemory", memory::SETTINGS_KEY);
}

fn identities(list: &[Entry]) -> Vec<&str> {
    list.iter().map(|e| e.identity.as_str()).collect()
}

/// A distinct, well-formed identity per index: Swift `String(format: "%016x", index)`.
fn identity(index: usize) -> String {
    format!("{index:016x}")
}

// MARK: The book exemption

#[test]
fn book_article_mark_is_claimed_exactly_once() {
    let mut opens = BookArticleOpens::new();
    let article = combine(&[&temp_path(), &unique("md-book-"), "ch1.md"]);
    let plain = combine(&[&temp_path(), &unique("md-plain-"), "notes.md"]);

    assert!(!opens.claim_open(&article));

    opens.mark(&article);
    assert!(!opens.claim_open(&plain));
    assert!(opens.claim_open(&article));
    assert!(!opens.claim_open(&article));
}

#[test]
fn book_article_mark_expires_after_ten_seconds() {
    // md.win swaps a static clock and restores it in a finally; here every instance has its own.
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = Arc::clone(&now);
    let mut opens = BookArticleOpens::with_clock(move || *clock.lock().unwrap());
    let advance = |by: Duration| *now.lock().unwrap() += by;
    let article = combine(&[&temp_path(), &unique("md-book-"), "ch2.md"]);

    assert_eq!(Duration::from_secs(10), book_article_opens::MARK_LIFETIME);
    opens.mark(&article);
    // A mark exactly at the lifetime is gone: `pending.filter { $0.value > cutoff }`.
    advance(Duration::from_secs(10));
    assert!(!opens.claim_open(&article));

    opens.mark(&article);
    advance(Duration::from_millis(9999));
    assert!(opens.claim_open(&article));
}

// MARK: Navigation nudges

#[test]
fn navigation_nudge_is_transient() {
    assert_eq!(Some(ViewMode::Edit), rule::navigation_nudge(ViewMode::Preview, ViewMode::Edit));
    assert_eq!(Some(ViewMode::Preview), rule::navigation_nudge(ViewMode::Edit, ViewMode::Preview));
    assert_eq!(None, rule::navigation_nudge(ViewMode::Edit, ViewMode::Edit));
    assert_eq!(None, rule::navigation_nudge(ViewMode::Preview, ViewMode::Preview));
    assert_eq!(None, rule::navigation_nudge(ViewMode::Split, ViewMode::Edit));
    assert_eq!(None, rule::navigation_nudge(ViewMode::Split, ViewMode::Preview));

    let store = InMemoryViewModeStore::new();
    let file = memory::identity_for(&combine(&[&temp_path(), "md-viewmode-missing", "notes.md"]));

    // A file the reader keeps in Preview, opened in a desktop window — always wide, never coerced.
    memory::remember(ViewMode::Preview, &file, &store);
    let mut preferred = rule::open_view_mode(memory::lookup(&file, &store), false, true, true);
    let mut navigation: Option<ViewMode> = None;
    let displayed =
        |preferred: ViewMode, navigation: Option<ViewMode>| rule::displayed_mode(preferred, navigation, true);
    assert_eq!(ViewMode::Preview, displayed(preferred, navigation));

    // Go ▸ Notes: the note has to be visible to be read, and reading it is not a preference.
    if let Some(nudge) = rule::navigation_nudge(displayed(preferred, navigation), ViewMode::Edit) {
        navigation = Some(nudge);
    }
    assert_eq!(ViewMode::Edit, displayed(preferred, navigation));
    assert_eq!(Some(ViewMode::Preview), memory::lookup(&file, &store));

    // A second note: already nudged, nothing changes — the override is not cleared out from under them.
    if let Some(second) = rule::navigation_nudge(displayed(preferred, navigation), ViewMode::Edit) {
        navigation = Some(second);
    }
    assert_eq!(ViewMode::Edit, displayed(preferred, navigation));
    assert_eq!(Some(ViewMode::Preview), memory::lookup(&file, &store));

    // A deliberate pick (Ctrl+2): the nudge goes and the raw choice is what is persisted.
    navigation = None;
    preferred = ViewMode::Split;
    memory::remember(preferred, &file, &store);
    assert_eq!(None, navigation);
    assert_eq!(Some(ViewMode::Split), memory::lookup(&file, &store));
    assert_eq!(ViewMode::Split, displayed(preferred, navigation));

    // The narrow half: a raw Split displays as Edit, so a jump into the preview has a pane to bring on
    // screen — and Split survives in the store, where at that width it could never be re-picked.
    assert_eq!(ViewMode::Edit, rule::displayed_mode(ViewMode::Split, None, false));
    assert_eq!(Some(ViewMode::Preview), rule::navigation_nudge(ViewMode::Edit, ViewMode::Preview));
    assert_eq!(ViewMode::Preview, rule::displayed_mode(ViewMode::Split, Some(ViewMode::Preview), false));
    assert_eq!(Some(ViewMode::Split), memory::lookup(&file, &store));

    // Opening another document drops the override; the Save-As migration deliberately does not.
    navigation = Some(ViewMode::Edit);
    assert_eq!(ViewMode::Edit, displayed(preferred, navigation));
    navigation = None;
    assert_eq!(ViewMode::Split, displayed(preferred, navigation));
}

// MARK: Identity

#[test]
fn identity_matches_the_shared_sha_vectors() {
    // The two vectors from the spec — pure ASCII, so they catch a UTF-16-for-UTF-8 slip only together with the é case.
    assert_eq!("53ba23f60734adf1", memory::sha256_prefix("file:/tmp/a.md"));
    assert_eq!(
        "427542472354b900",
        memory::sha256_prefix("saf:com.android.externalstorage.documents:primary:Documents/a.md")
    );
    assert_eq!(16, memory::sha256_prefix("file:/tmp/\u{00E9}.md").len());
    assert_ne!(memory::sha256_prefix("file:/tmp/\u{00E9}.md"), memory::sha256_prefix("file:/tmp/e.md"));
    // Kotlin's self-check: hashed as UTF-8, not the string's UTF-16.
    let utf8 = sha256::digest("saf:com.example:\u{00E9}\u{4E2D}".as_bytes());
    assert_eq!(sha256::hex_lower(&utf8[..8]), memory::sha256_prefix("saf:com.example:\u{00E9}\u{4E2D}"));

    // identity_for is that hash over "file:" + the canonical path…
    let missing = combine(&[&temp_path(), &unique("md-viewmode-missing-")]);
    let a = combine(&[&missing, "a.md"]);
    let b = combine(&[&missing, "b.md"]);
    assert!(memory::is_identity(&memory::identity_for(&a)));
    assert_ne!(memory::identity_for(&a), memory::identity_for(&b));
    assert_eq!(memory::sha256_prefix(&format!("file:{}", memory::canonical_path(&a))), memory::identity_for(&a));

    // …and the symlink resolution is load-bearing: the same document under two paths is one entry.
    // The file must exist — resolution is a no-op for a path that names nothing, on Foundation and here.
    // (md.win skips the two-spellings half on a Windows runner without symlink rights; Linux always has them.)
    let real = TempDir::new("md-viewmode-");
    let link = combine(&[&temp_path(), &unique("md-viewmode-link-")]);
    fs::write(real.join(&["a.md"]), "# a\n").unwrap();
    std::os::unix::fs::symlink(&real.path, &link).unwrap();
    let produced = memory::identity_for(&real.join(&["a.md"]));
    let via_link = memory::identity_for(&combine(&[&link, "a.md"]));
    let canonical_real = memory::canonical_path(&real.join(&["a.md"]));
    let canonical_link = memory::canonical_path(&combine(&[&link, "a.md"]));
    fs::remove_file(&link).unwrap();
    assert_eq!(produced, via_link);
    assert_eq!(canonical_real, canonical_link);
    // A well-formed identity by the decoder's own standard, so anything identity_for produces round-trips.
    assert!(memory::is_identity(&produced));
    assert_eq!(
        vec![Entry::new(produced.clone(), ViewMode::Split)],
        memory::decode(Some(&memory::encode(&[Entry::new(produced, ViewMode::Split)])))
    );
}

/// LINUX DECISION (md.win folds case on Windows): the one-argument form never folds, so `MiXeD.md`
/// and `MIXED.MD` — two files on ext4 — are two identities. The two-argument form still folds when
/// asked, exactly as md.win pins it.
#[test]
fn canonical_path_collapses_the_spelling_and_folds_case_only_when_asked() {
    let tmp = temp_path();
    let stem = unique("md-viewmode-case-");
    let plain = combine(&[&tmp, &stem, "MiXeD.md"]);
    let dotted = combine(&[&tmp, &stem, "Sub", "..", "MiXeD.md"]);
    let trailing = format!("{}/", combine(&[&tmp, &stem, "MiXeD.md"]));

    // `..` and a trailing separator vanish (standardizedFileURL), nothing else changes for a path that names nothing.
    assert_eq!(memory::canonical_path_with(&plain, false), memory::canonical_path_with(&dotted, false));
    assert_eq!(memory::canonical_path_with(&plain, false), memory::canonical_path_with(&trailing, false));
    assert_eq!(book_paths::standardize(&plain), memory::canonical_path_with(&plain, false));
    assert!(memory::canonical_path_with(&plain, false).ends_with("MiXeD.md"));

    // The Windows rule, still available: fold case, so C:\Docs\A.md and c:\docs\a.md are one file with one memory.
    let upper = combine(&[&tmp, &stem, "MIXED.MD"]);
    assert_eq!(memory::canonical_path_with(&plain, true), memory::canonical_path_with(&upper, true));
    assert_eq!(
        md_core::text::unicode::upper_invariant(&memory::canonical_path_with(&plain, false)),
        memory::canonical_path_with(&plain, true)
    );
    assert_ne!(memory::canonical_path_with(&plain, false), memory::canonical_path_with(&upper, false));
    // The one-argument form picks the platform rule: Linux is case-sensitive.
    assert_eq!(memory::canonical_path_with(&plain, false), memory::canonical_path(&plain));
    assert_ne!(memory::identity_for(&plain), memory::identity_for(&upper));
}
