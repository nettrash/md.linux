//! Port of md.win `tests/Md.App.Logic.Tests/Preview/JsonScriptTests.cs`: a script evaluation
//! answers in JSON, and every port that forgot has written the four characters `null` into a file.
//! Nothing here panics: no answer is an answer. (md.win's `null` inputs are a Rust `&str` that
//! cannot be one; `""` stands for them.)

use md_logic::preview::json_script::{self, Json, number, parse, scroll_message, string};

#[test]
fn the_render_complete_flag_is_a_string_with_its_quotes() {
    assert_eq!(string("\"1\"").as_deref(), Some("1"));
    assert_eq!(number("\"1\""), None); // a JSON string is not a number
}

#[test]
fn null_is_the_four_characters_and_decodes_to_nothing() {
    assert_eq!(number("null"), None);
    assert_eq!(string("null"), None);
    assert_eq!(scroll_message("null"), None);
}

#[test]
fn numbers_decode_invariantly() {
    for (json, expected) in [("0", 0.0), ("1234", 1234.0), ("1234.5", 1234.5), ("-0.25", -0.25), ("1e3", 1000.0)] {
        assert_eq!(number(json), Some(expected), "{json}");
    }
}

#[test]
fn anything_else_is_no_answer() {
    for json in ["", "undefined", "{", "true", "{\"a\":1}"] {
        assert_eq!(number(json), None, "{json}");
        assert_eq!(string(json), None, "{json}");
    }
}

#[test]
fn strings_come_back_unescaped() {
    assert_eq!(string("\"a\\\"b\"").as_deref(), Some("a\"b"));
    assert_eq!(string("\"\"").as_deref(), Some(""));
}

#[test]
fn the_scroll_message_is_fraction_and_echo() {
    let message = scroll_message("{\"fraction\":0.42,\"echo\":false}").expect("a scroll message");
    assert_eq!(message.fraction, 0.42);
    assert!(!message.echo);

    let echoed = scroll_message("{\"echo\":true,\"fraction\":1}").expect("a scroll message");
    assert!(echoed.echo);
    assert_eq!(echoed.fraction, 1.0);
}

#[test]
fn a_message_that_is_not_one_is_ignored() {
    for json in [
        "{\"echo\":false}",                      // no fraction
        "{\"fraction\":\"0.5\",\"echo\":false}", // fraction as a string
        "[0.5,false]",
        "0.5",
        // From md.win's KeyMessage test: a key message is not a scroll message either.
        "{\"key\":114,\"ctrl\":false,\"alt\":false,\"shift\":false}",
    ] {
        assert_eq!(scroll_message(json), None, "{json}");
    }
}

#[test]
fn a_missing_echo_is_not_an_echo() {
    let message = scroll_message("{\"fraction\":0.5}").expect("a scroll message");
    assert!(!message.echo);
}

#[test]
#[ignore = "LINUX: no key-forwarding channel — every GTK accelerator fires once from the WebKitWebView too (docs/linux-facts.md §1), so JsonScript.KeyMessage is not ported"]
fn the_key_message_is_a_chord_and_nothing_else_is_one() {
    // md.win: KeyMessage("{\"key\":114,…,\"shift\":true}") == Chord(F3, Shift), and a scroll
    // message, a string key, 0, 256 and null are none. The one assertion that still means something
    // here — a key message is not a scroll message — is in a_message_that_is_not_one_is_ignored.
}

// ── LINUX: the hand-written parser that replaces System.Text.Json ─────────────────────────────

#[test]
fn the_parser_reads_what_javascriptcore_writes() {
    // Value::to_json(0) output: compact, members in insertion order.
    assert_eq!(
        parse("{\"a\":[1,2.5,-3e2],\"b\":{\"c\":null},\"d\":true,\"e\":\"x\"}"),
        Some(Json::Object(vec![
            ("a".into(), Json::Array(vec![Json::Number(1.0), Json::Number(2.5), Json::Number(-300.0)])),
            ("b".into(), Json::Object(vec![("c".into(), Json::Null)])),
            ("d".into(), Json::Bool(true)),
            ("e".into(), Json::String("x".into())),
        ]))
    );
    assert_eq!(parse(" \t\r\n [ ] \n"), Some(Json::Array(vec![])));
    assert_eq!(parse("{ }"), Some(Json::Object(vec![])));
}

#[test]
fn escapes_and_surrogate_pairs_decode() {
    assert_eq!(string(r#""\/\\\b\f\n\r\t""#).as_deref(), Some("/\\\u{8}\u{C}\n\r\t"));
    assert_eq!(string(r#""\u0041\u00e9\u0436""#).as_deref(), Some("Aéж"));
    assert_eq!(string(r#""\ud83d\ude00""#).as_deref(), Some("😀"));
    assert_eq!(string("\"Привет 😀\"").as_deref(), Some("Привет 😀"));
    // A lone surrogate cannot live in a Rust string: U+FFFD, and the next escape on its own.
    assert_eq!(string(r#""\ud800x""#).as_deref(), Some("\u{FFFD}x"));
    assert_eq!(string(r#""\ud800\u0041""#).as_deref(), Some("\u{FFFD}A"));
    assert_eq!(string(r#""\udc00""#).as_deref(), Some("\u{FFFD}"));
}

#[test]
fn malformed_json_is_no_answer_and_never_a_panic() {
    for json in [
        "nul",
        "tru",
        "01",
        "+1",
        ".5",
        "1.",
        "1e",
        "1e+",
        "-",
        "--1",
        "0x10",
        "NaN",
        "Infinity",
        "\"abc",
        "\"a\nb\"",
        "\"\\x\"",
        "\"\\u12\"",
        "\"\\u12g4\"",
        "\"\\é\"",
        "[1,]",
        "[1 2]",
        "{\"a\" 1}",
        "{\"a\":1,}",
        "{a:1}",
        "{1:1}",
        "1 2",
        "[",
        "]",
        "\"\"\"",
        "// x\n1",
        "é",
        "\"\\",
    ] {
        assert_eq!(parse(json), None, "{json:?}");
        assert_eq!(number(json), None, "{json:?}");
    }
}

#[test]
fn nesting_deeper_than_sixty_four_is_malformed() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(parse(&deep(64)).is_some());
    assert_eq!(parse(&deep(65)), None);
    // And a pathological depth is refused, not a stack overflow.
    assert_eq!(parse(&deep(100_000)), None);
}

#[test]
fn the_last_of_repeated_members_wins() {
    // JsonElement.TryGetProperty's rule.
    let message = scroll_message("{\"fraction\":0.1,\"fraction\":0.9}").expect("a scroll message");
    assert_eq!(message.fraction, 0.9);
    assert_eq!(json_script::parse("{\"a\":1,\"a\":2}").and_then(|j| j.member("a").cloned()), Some(Json::Number(2.0)));
}
