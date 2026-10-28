//! Port of md.win `tests/Md.App.Logic.Tests/SmartTypingAdapterTests.cs`: the pure half of the
//! editor's typing hooks (smart-typing.md §3.3, the retype rule of §3.4, §3.5) — the diff, the
//! reduction of an insertion to one scalar, the retype rule, the plan the pane applies and the Enter
//! edit. The stateful half is `text_typing_hooks.rs`.
//!
//! LINUX: the editor's buffer holds LF (GTK), not WinUI's CR, so the rows md.win spells with a CR
//! line end are spelled with LF here, and the Enter edit is md-core's own (no LF → CR swap).

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use md_core::editing::smart_typing::{self, EnterEdit};
use md_logic::settings::settings_keys::{self, ALL};
use md_logic::settings::{InMemorySettingsStore, SettingsStore, TypingSettings};
use md_logic::text::CapitalizationPlan;
use md_logic::text::smart_typing_adapter::{
    applies, enter, first_scalar, insertion_over_selection, is_word_insertion, plan, retypes_own_capital, upper,
};

fn plan_of(position: usize, inserted: &str, capital: &str, replacement: &str) -> CapitalizationPlan {
    CapitalizationPlan::new(position, inserted, capital, replacement)
}

// ── The diff ───────────────────────────────────────────────────────────────────────────────

#[test]
fn the_insertion_over_the_selection_is_found() {
    let rows: [(&str, isize, isize, &str, &str); 8] = [
        ("", 0, 0, "h", "h"),                     // the first letter of a document
        ("ab", 1, 0, "axb", "x"),                 // a letter between two
        ("ab", 2, 0, "ab ", " "),                 // a space at the end
        ("ab", 0, 2, "hello", "hello"),           // a word over a whole selection
        ("abc", 1, 1, "aXYc", "XY"),              // two units over one
        ("aa", 1, 0, "aaa", "a"),                 // ambiguous by prefix/suffix, unambiguous by the selection
        ("- x\n", 4, 0, "- x\nh", "h"),           // LINUX: after an LF line end, as the buffer spells it
        ("ab", 1, 0, "a\u{1F600}b", "\u{1F600}"), // a surrogate pair is one insertion of two units
    ];
    for (old, start, length, new, expected) in rows {
        assert_eq!(insertion_over_selection(old, start, length, new).as_deref(), Some(expected), "{old:?} → {new:?}");
    }
}

#[test]
fn anything_else_is_not_an_insertion() {
    let rows: [(&str, isize, isize, &str); 9] = [
        ("ab", 1, 0, "ab"),       // unchanged
        ("ab", 1, 0, "a"),        // a deletion
        ("ab", 0, 1, "b"),        // a selection deleted
        ("Hello", 1, 0, "hello"), // an undo of a capital: same length, changed before the caret
        ("ab", 1, 0, "xab"),      // an insertion, but not at the selection
        ("ab", 1, 0, "abx"),      // an insertion after the selection, not at it
        ("ab", 3, 0, "abc"),      // a selection the text cannot hold
        ("ab", -1, 0, "xab"),
        ("ab", 1, -1, "axb"),
    ];
    for (old, start, length, new) in rows {
        assert_eq!(insertion_over_selection(old, start, length, new), None, "{old:?} {start}+{length} → {new:?}");
    }
}

#[test]
fn a_letter_over_a_selection_is_an_insertion() {
    assert_eq!(insertion_over_selection("ab", 0, 1, "xb").as_deref(), Some("x"));
}

// ── §3.3: word insertions ──────────────────────────────────────────────────────────────────

#[test]
fn a_word_insertion_starts_with_a_lowercase_letter_and_holds_no_space() {
    for insertion in [
        "h",
        "hello",
        "hello ", // one trailing SP — a predicted word with its space
        "écrire",
        "привет",
        "\u{10428}", // DESERET SMALL LETTER LONG I — Ll, off the BMP
        "don't",
        "re-enter",
        "\u{01C6}", // ǆ, Ll
    ] {
        assert!(is_word_insertion(insertion), "{insertion:?}");
    }
}

#[test]
fn anything_else_is_not_a_word_insertion() {
    for insertion in [
        "",
        "H",           // uppercase
        "1",           // a digit
        " ",           // SP alone: its first scalar is not a letter
        " h",          // a leading space
        "hello  ",     // two trailing spaces
        "hello world", // a phrase (dictation, paste)
        "hello\tx",
        "hello\u{00A0}x", // NBSP is WS19
        "hello\u{3000}",  // ideographic space, even trailing: only SP may trail
        "hello\u{200B}",  // zero-width space is WS19
        "hello\n",        // a line terminator
        "hello\r",
        "h\r\n",
        "https://a.b", // the four §2.1f markers, on the insertion itself
        "www.nettrash.me",
        "nettrash@nettrash.me",
        "~/Documents/x",
        "a/b",
        "\u{0301}x", // a mark first
        "\u{00AA}",  // ª is Lo since Unicode 6.1, not Ll
        "\u{2028}h", // LINE SEPARATOR is neither a terminator nor WS19 — but its first scalar is not a letter
    ] {
        assert!(!is_word_insertion(insertion), "{insertion:?}");
    }
}

#[test]
fn every_ws19_unit_inside_a_word_disqualifies_it() {
    let ws19: [u32; 19] = [
        0x0009, 0x0020, 0x00A0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009,
        0x200A, 0x200B, 0x202F, 0x205F, 0x3000,
    ];
    assert_eq!(ws19.len(), 19);
    for u in ws19 {
        let c = char::from_u32(u).unwrap();
        assert!(!is_word_insertion(&format!("he{c}llo")), "U+{u:04X} inside a word");
        assert!(!is_word_insertion(&format!("hello{c}{c}")), "two trailing U+{u:04X}");
    }
}

#[test]
fn units_that_are_not_ws19_are_ordinary_content() {
    // §0.1 / §0.3: U+2028, U+2029, U+0085, U+000B and U+000C are neither terminators nor whitespace.
    for u in [0x2028u32, 0x2029, 0x0085, 0x000B, 0x000C, 0xFEFF] {
        let c = char::from_u32(u).unwrap();
        assert!(is_word_insertion(&format!("he{c}llo")), "U+{u:04X} inside a word");
    }
}

#[test]
fn the_first_scalar_is_one_or_two_units() {
    assert_eq!(first_scalar("h"), "h");
    assert_eq!(first_scalar("hello"), "h");
    assert_eq!(first_scalar("\u{10428}x"), "\u{10428}");
}

/// md.win pins that a lone surrogate is its own scalar of category Cs, never a word, with no
/// `upper`. A Rust string cannot hold a lone surrogate, so the case cannot arise; what remains is
/// that the replacement character an invalid UTF-16 sequence decodes to is no letter either.
#[test]
fn lone_surrogates_are_their_own_scalar_and_belong_to_no_class() {
    let decoded = String::from_utf16_lossy(&[0xD83D, u16::from(b'x')]);
    assert_eq!(first_scalar(&decoded), "\u{FFFD}");
    assert!(!is_word_insertion(&decoded));
    assert_eq!(upper("\u{FFFD}"), None);
}

#[test]
#[should_panic(expected = "empty")]
fn the_first_scalar_of_nothing_is_an_error() {
    first_scalar("");
}

// ── §0.7 through the pure function ─────────────────────────────────────────────────────────

#[test]
fn upper_is_the_pure_functions_mapping() {
    for (scalar, expected) in [
        ("a", "A"),
        ("é", "É"),
        ("я", "Я"),
        ("\u{01C6}", "\u{01C4}"), // ǆ → Ǆ: uppercase, not titlecase ǅ
        ("\u{0131}", "I"),        // dotless i: the vector-decided mapping, not .NET's identity
        ("\u{10428}", "\u{10400}"),
    ] {
        assert_eq!(upper(scalar).as_deref(), Some(expected), "{scalar:?}");
    }
}

#[test]
fn upper_is_undefined_where_the_specification_says_so() {
    for scalar in [
        "ß",        // full mapping is SS, simple is itself
        "\u{00B5}", // MICRO SIGN, excluded
        "\u{10D0}", // Georgian, excluded
        "A",        // not lowercase
        "1",
    ] {
        assert_eq!(upper(scalar), None, "{scalar:?}");
    }
}

// ── §3.4: the retype rule ──────────────────────────────────────────────────────────────────

#[test]
fn a_lowercase_letter_over_its_own_capital_goes_in_as_typed() {
    assert!(retypes_own_capital("Md is", 0, 1, "m"));
    assert!(retypes_own_capital("x É", 2, 1, "é"));
    assert!(retypes_own_capital("\u{10400}", 0, 2, "\u{10428}")); // a two-unit selection, one scalar
}

#[test]
fn the_retype_rule_needs_exactly_one_scalar_that_is_the_upper() {
    assert!(!retypes_own_capital("Md is", 0, 0, "m")); // no selection
    assert!(!retypes_own_capital("Md is", 0, 2, "m")); // two scalars selected
    assert!(!retypes_own_capital("Md is", 1, 1, "m")); // the selected scalar is not M
    assert!(!retypes_own_capital("Nd is", 0, 1, "m")); // another capital
    assert!(!retypes_own_capital("Md is", 0, 1, "M")); // not a lowercase letter
    assert!(!retypes_own_capital("\u{10400}", 1, 1, "\u{10428}")); // half a pair
    assert!(!retypes_own_capital("Md", 1, 5, "m")); // out of range
}

// ── The plan: the letter hook, whole ───────────────────────────────────────────────────────

#[test]
fn the_first_letter_of_a_document_is_capitalized() {
    assert_eq!(plan("", 0, 0, "h"), Some(plan_of(0, "h", "H", "H")));
}

#[test]
fn a_predicted_word_is_capitalized_on_its_first_scalar_only() {
    assert_eq!(plan("Done. ", 6, 0, "Done. then "), Some(plan_of(6, "then ", "T", "Then ")));
}

#[test]
fn the_plan_remembers_what_the_insertion_replaced() {
    assert_eq!(plan("Xy", 0, 2, "h"), Some(plan_of(0, "h", "H", "H").replacing(2)));
}

#[test]
fn a_two_unit_first_scalar_is_replaced_whole() {
    assert_eq!(plan("", 0, 0, "\u{10428}x"), Some(plan_of(0, "\u{10428}x", "\u{10400}", "\u{10400}x")));
}

/// LINUX: md.win's "TheBoxOwnCRLineEndsAreReadAsLineEnds" — the buffer's own line ends are read as
/// line ends, nothing converted on the way in. GTK's are LF.
#[test]
fn the_box_own_cr_line_ends_are_read_as_line_ends() {
    assert_eq!(plan("Notes\n", 6, 0, "Notes\nh"), Some(plan_of(6, "h", "H", "H")));
    assert_eq!(plan("Notes\n- ", 8, 0, "Notes\n- h"), Some(plan_of(8, "h", "H", "H")));
    assert_eq!(plan("```\nx\n", 6, 0, "```\nx\nh"), None); // inside a fence
    // md.win's own CR spelling still reads as a line end (§0.1 accepts a lone CR).
    assert_eq!(plan("Notes\r", 6, 0, "Notes\rh"), Some(plan_of(6, "h", "H", "H")));
}

#[test]
fn mid_word_and_mid_sentence_letters_are_left_alone() {
    assert_eq!(plan("H", 1, 0, "He"), None);
    assert_eq!(plan("Hello ", 6, 0, "Hello w"), None);
}

#[test]
fn everything_that_is_not_a_word_insertion_is_left_alone() {
    assert_eq!(plan("", 0, 0, "hello world"), None); // a phrase
    assert_eq!(plan("", 0, 0, "https://a.b"), None); // a link
    assert_eq!(plan("", 0, 0, "H"), None); // already a capital
    assert_eq!(plan("", 0, 0, " "), None); // a space
    assert_eq!(plan("ab", 1, 0, "a"), None); // a deletion
    assert_eq!(plan("Hello", 1, 0, "hello"), None); // an undo
    assert_eq!(plan("", 0, 0, ""), None); // nothing
}

#[test]
fn select_and_retype_is_as_typed_by_the_plan_alone() {
    // §3.4's last rule needs no state: M selected, m typed → None, whoever wrote the M.
    assert_eq!(plan("Md is", 0, 1, "md is"), None);
    // The same keystroke over two scalars is judged: text′ is "" and rule A applies.
    assert_eq!(plan("Md is", 0, 2, "m is").unwrap().capital, "M");
}

#[test]
fn the_plan_knows_nothing_of_paste_or_the_override() {
    // Both are the hooks' decisions, made before plan is asked: a word at a line start is a plan
    // here whatever brought it, and text_typing_hooks pins that a paste and an armed override never
    // reach this.
    assert!(plan("", 0, 0, "hello").is_some());
    assert!(plan("", 0, 0, "m").is_some());
}

// ── applies: the check before the replacement ──────────────────────────────────────────────

#[test]
fn the_plan_applies_when_the_insertion_landed_and_the_caret_follows_it() {
    let p = plan_of(6, "then ", "T", "Then ");
    assert!(applies(&p, "Done. then ", 11, 0));
    assert!(!applies(&p, "Done. then ", 10, 0)); // caret elsewhere
    assert!(!applies(&p, "Done. then ", 6, 5)); // a selection that is not the replaced one
    // The shell reporting the selection a beat late: still the one the insertion replaced.
    assert!(applies(&p, "Done. then ", 6, 0));
    let over_selection = plan_of(0, "m", "M", "M").replacing(2);
    assert!(applies(&over_selection, "m is", 1, 0));
    assert!(applies(&over_selection, "m is", 0, 2));
    assert!(!applies(&over_selection, "m is", 0, 1));
    assert!(!applies(&over_selection, "m is", 0, 0));
    assert!(!applies(&p, "Done. them ", 11, 0)); // the input was changed
    assert!(!applies(&p, "Done. ", 6, 0)); // cancelled
    // md.win's last row, a plan at -1, cannot be built: a position is unsigned. A plan past the end
    // of the text is the nearest thing, and it does not apply either.
    assert!(!applies(&plan_of(1, "x", "X", "X"), "x", 1, 0));
}

// ── Enter ──────────────────────────────────────────────────────────────────────────────────

/// LINUX: "EnterContinuesAListWithTheControlsOwnLineBreak" — the buffer's own line break is LF.
#[test]
fn enter_continues_a_list_with_the_controls_own_line_break() {
    let edit = enter("- item", 6, 0).expect("a list item continues");
    assert_eq!(edit, EnterEdit { location: 6, length: 0, replacement: "\n- ".to_string(), caret: 9 });
    assert!(!edit.replacement.contains('\r'));
}

/// LINUX: "EnterEditsAreThePureFunctionsWithLFSwappedForCR" — with no swap, the hook's edit is the
/// pure function's exactly; the texts are md.win's with LF line ends.
#[test]
fn enter_edits_are_the_pure_functions_with_lf_swapped_for_cr() {
    let rows: [(&str, isize, isize); 6] = [
        ("- item", 6, 0),
        ("1. one\n2. two", 13, 0),
        ("> quote", 7, 0),
        ("| a | b |\n| - | - |\n| 1 | 2 |", 29, 0),
        ("- ", 2, 0),
        ("- a\n- b", 3, 0),
    ];
    for (text, start, length) in rows {
        let pure = smart_typing::enter(text, start, start + length);
        let boxed = enter(text, start, length);
        assert_eq!(pure, boxed, "{text:?}");
    }
    // At least the list rows continue, so the comparison above compared something.
    assert!(enter("1. one\n2. two", 13, 0).is_some());
}

#[test]
fn enter_answers_null_where_the_pure_function_does_and_for_a_bad_selection() {
    assert_eq!(enter("plain prose", 11, 0), None);
    assert_eq!(enter("", 0, 0), None);
    assert_eq!(enter("- item", 7, 0), None);
    assert_eq!(enter("- item", -1, 0), None);
    assert_eq!(enter("- item", 2, 10), None);
}

#[test]
fn enter_over_a_selection_deletes_it_in_the_same_edit() {
    // §0.9 item 4: "te" selected inside "- item" — the continue rule inserts at the caret and the
    // edit's range is the selection, so one undoable step both deletes and continues.
    assert_eq!(enter("- item", 3, 2), Some(EnterEdit { location: 3, length: 2, replacement: "\n- ".into(), caret: 6 }));
    // The whole content selected: text′ is an empty item, so the exit rule clears the line (§1.3).
    assert_eq!(enter("- item", 2, 4), Some(EnterEdit { location: 0, length: 6, replacement: String::new(), caret: 0 }));
}

// ── The settings ───────────────────────────────────────────────────────────────────────────

#[test]
fn both_typing_settings_default_to_on() {
    let store = InMemorySettingsStore::new();
    assert_eq!(TypingSettings::read(&store), TypingSettings::DEFAULTS);
    let defaults = TypingSettings::default();
    assert!(defaults.continue_lists);
    assert!(defaults.capitalize_sentences);
}

#[test]
fn the_settings_read_their_keys_and_nothing_else() {
    let store = InMemorySettingsStore::new();
    store.set_bool(settings_keys::CONTINUE_LISTS, false);
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(false, true));
    store.set_bool(settings_keys::CAPITALIZE_SENTENCES, false);
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(false, false));
    store.set_string(settings_keys::CONTINUE_LISTS, "true"); // the wrong type reads as absent
    assert_eq!(TypingSettings::read(&store), TypingSettings::new(true, false));
}

#[test]
fn toggle_flips_the_stored_value_from_its_default_and_raises_changed() {
    let store = InMemorySettingsStore::new();
    let changes = Rc::new(RefCell::new(Vec::<String>::new()));
    {
        let changes = changes.clone();
        store.changed().connect(move |key| changes.borrow_mut().push(key.to_string()));
    }

    TypingSettings::toggle(&store, settings_keys::CAPITALIZE_SENTENCES); // absent ⇒ true ⇒ off
    assert!(!store.get_bool(settings_keys::CAPITALIZE_SENTENCES, true));
    TypingSettings::toggle(&store, settings_keys::CAPITALIZE_SENTENCES);
    assert!(store.get_bool(settings_keys::CAPITALIZE_SENTENCES, false));
    TypingSettings::toggle(&store, settings_keys::CONTINUE_LISTS);
    assert!(!store.get_bool(settings_keys::CONTINUE_LISTS, true));

    assert_eq!(
        *changes.borrow(),
        [settings_keys::CAPITALIZE_SENTENCES, settings_keys::CAPITALIZE_SENTENCES, settings_keys::CONTINUE_LISTS]
    );
    // md.win throws ArgumentOutOfRangeException; the port panics.
    let wrong_key = catch_unwind(AssertUnwindSafe(|| TypingSettings::toggle(&store, settings_keys::PDF_PAGE_SIZE)));
    assert!(wrong_key.is_err());
}

#[test]
fn only_the_two_keys_are_typing_keys() {
    assert!(TypingSettings::is_key(settings_keys::CONTINUE_LISTS));
    assert!(TypingSettings::is_key(settings_keys::CAPITALIZE_SENTENCES));
    for key in ALL.iter().filter(|&&k| k != settings_keys::CONTINUE_LISTS && k != settings_keys::CAPITALIZE_SENTENCES) {
        assert!(!TypingSettings::is_key(key), "{key}");
    }
}
