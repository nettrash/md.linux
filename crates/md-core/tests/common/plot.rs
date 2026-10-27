//! What the plot tests share, in place of what md.win's PlotTests reach for in .NET: an exact
//! JSON reader for `plot-vectors.json`, `double.ToString("R")`, the handful of regular
//! expressions the suite matches with (md-core has no regex crate, so each is hand-written for
//! the one pattern it stands for), a small XML reader standing in for `XDocument.Parse`, and the
//! assertion helpers. Included by the plot test files with `#[path]`, so `common/mod.rs` stays
//! as every other module's tests expect it.
#![allow(dead_code)]

use std::sync::OnceLock;

use md_core::plot::{self, PlotError};

// MARK: - An exact JSON reader

/// JSON with every number kept as its literal text. serde_json (without `float_roundtrip`) is not
/// correctly rounded — on this very file it reads `999.9999999999999` as 1000 — while
/// System.Text.Json, which md.win reads it with, is; Rust's `str::parse::<f64>` is too, so the
/// literal is converted there.
pub mod json {
    #[derive(Clone, Debug, PartialEq)]
    pub enum Json {
        Null,
        Bool(bool),
        Number(String),
        String(String),
        Array(Vec<Json>),
        Object(Vec<(String, Json)>),
    }

    static NULL: Json = Json::Null;

    impl Json {
        /// `JsonElement.GetProperty`: the member, which must exist.
        pub fn get(&self, key: &str) -> &Json {
            match self {
                Json::Object(members) => members
                    .iter()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value)
                    .unwrap_or_else(|| panic!("no property {key}")),
                _ => panic!("not an object looking up {key}"),
            }
        }

        /// The member, or `null` when it is absent.
        pub fn get_or_null(&self, key: &str) -> &Json {
            match self {
                Json::Object(members) => members.iter().find(|(name, _)| name == key).map_or(&NULL, |(_, value)| value),
                _ => &NULL,
            }
        }

        pub fn array(&self) -> &[Json] {
            match self {
                Json::Array(items) => items,
                other => panic!("not an array: {other:?}"),
            }
        }

        pub fn str(&self) -> &str {
            match self {
                Json::String(text) => text,
                other => panic!("not a string: {other:?}"),
            }
        }

        pub fn opt_str(&self) -> Option<&str> {
            match self {
                Json::String(text) => Some(text),
                _ => None,
            }
        }

        /// `JsonElement.GetDouble`: correctly rounded.
        pub fn f64(&self) -> f64 {
            match self {
                Json::Number(text) => text.parse().unwrap_or_else(|e| panic!("number {text}: {e}")),
                other => panic!("not a number: {other:?}"),
            }
        }

        pub fn i32(&self) -> i32 {
            match self {
                Json::Number(text) => text.parse().unwrap_or_else(|e| panic!("integer {text}: {e}")),
                other => panic!("not a number: {other:?}"),
            }
        }

        pub fn is_null(&self) -> bool {
            matches!(self, Json::Null)
        }

        pub fn is_number(&self) -> bool {
            matches!(self, Json::Number(_))
        }
    }

    pub fn parse(text: &str) -> Json {
        let mut reader = Reader { chars: text.chars().collect(), at: 0 };
        let value = reader.value();
        reader.skip_space();
        assert_eq!(reader.at, reader.chars.len(), "trailing text after the JSON value");
        value
    }

    struct Reader {
        chars: Vec<char>,
        at: usize,
    }

    impl Reader {
        fn peek(&self) -> char {
            *self.chars.get(self.at).unwrap_or_else(|| panic!("JSON ends early"))
        }

        fn next(&mut self) -> char {
            let c = self.peek();
            self.at += 1;
            c
        }

        fn expect(&mut self, wanted: char) {
            let got = self.next();
            assert_eq!(got, wanted, "JSON: expected {wanted:?} at {}", self.at - 1);
        }

        fn skip_space(&mut self) {
            while self.at < self.chars.len() && matches!(self.chars[self.at], ' ' | '\t' | '\n' | '\r') {
                self.at += 1;
            }
        }

        fn literal(&mut self, word: &str, value: Json) -> Json {
            for wanted in word.chars() {
                self.expect(wanted);
            }
            value
        }

        fn value(&mut self) -> Json {
            self.skip_space();
            match self.peek() {
                '{' => self.object(),
                '[' => self.array(),
                '"' => Json::String(self.string()),
                't' => self.literal("true", Json::Bool(true)),
                'f' => self.literal("false", Json::Bool(false)),
                'n' => self.literal("null", Json::Null),
                _ => self.number(),
            }
        }

        fn object(&mut self) -> Json {
            self.expect('{');
            let mut members = Vec::new();
            self.skip_space();
            if self.peek() == '}' {
                self.at += 1;
                return Json::Object(members);
            }
            loop {
                self.skip_space();
                let name = self.string();
                self.skip_space();
                self.expect(':');
                let value = self.value();
                members.push((name, value));
                self.skip_space();
                match self.next() {
                    ',' => continue,
                    '}' => return Json::Object(members),
                    other => panic!("JSON: unexpected {other:?} in an object"),
                }
            }
        }

        fn array(&mut self) -> Json {
            self.expect('[');
            let mut items = Vec::new();
            self.skip_space();
            if self.peek() == ']' {
                self.at += 1;
                return Json::Array(items);
            }
            loop {
                items.push(self.value());
                self.skip_space();
                match self.next() {
                    ',' => continue,
                    ']' => return Json::Array(items),
                    other => panic!("JSON: unexpected {other:?} in an array"),
                }
            }
        }

        fn hex4(&mut self) -> u32 {
            let mut value = 0;
            for _ in 0..4 {
                value = value * 16 + self.next().to_digit(16).expect("JSON: bad \\u escape");
            }
            value
        }

        fn string(&mut self) -> String {
            self.expect('"');
            let mut out = String::new();
            loop {
                match self.next() {
                    '"' => return out,
                    '\\' => match self.next() {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000C}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let first = self.hex4();
                            let scalar = if (0xD800..0xDC00).contains(&first) {
                                self.expect('\\');
                                self.expect('u');
                                let second = self.hex4();
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(scalar).expect("JSON: lone surrogate"));
                        }
                        other => panic!("JSON: bad escape \\{other}"),
                    },
                    c => out.push(c),
                }
            }
        }

        fn number(&mut self) -> Json {
            let start = self.at;
            while self.at < self.chars.len() && matches!(self.chars[self.at], '-' | '+' | '.' | 'e' | 'E' | '0'..='9') {
                self.at += 1;
            }
            assert!(self.at > start, "JSON: unexpected {:?}", self.peek());
            Json::Number(self.chars[start..self.at].iter().collect())
        }
    }
}

// MARK: - The corpus

pub struct NiceStepRow {
    pub input: f64,
    pub input_label: String,
    pub input_bits: String,
    pub output_bits: String,
}

pub struct FormatLabelRow {
    pub input: f64,
    pub input_label: String,
    pub input_bits: String,
    pub output: String,
    pub fmt_1e: String,
    pub fmt_2f: String,
}

/// `result`/`result_bits` are `None` for the rows the site could not evaluate at all (the JSON
/// string `"none"`) — every one a corrected behaviour or a name this language does not have, and
/// the loop fails if any is neither.
pub struct EvalRow {
    pub expr: String,
    pub x: f64,
    pub x_label: String,
    pub x_bits: String,
    pub result: Option<f64>,
    pub result_bits: Option<String>,
}

pub struct SvgRow {
    pub name: String,
    pub expr: String,
    pub x_min: f64,
    pub x_max: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub first_point: Option<String>,
    pub last_point: Option<String>,
    pub polyline_count: i32,
    pub line_count: i32,
    pub text_count: i32,
    pub x_step: f64,
    pub y_step: f64,
    pub x_tick_labels: Vec<String>,
    pub y_tick_labels: Vec<String>,
}

pub struct Vectors {
    pub nice_step: Vec<NiceStepRow>,
    pub format_label: Vec<FormatLabelRow>,
    pub eval: Vec<EvalRow>,
    pub parse_errors: Vec<String>,
    pub svg: Vec<SvgRow>,
}

/// Read once. The non-finite values are the JSON strings `nan`, `inf`, `-inf`.
pub fn vectors() -> &'static Vectors {
    static CORPUS: OnceLock<Vectors> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let root = json::parse(&crate::common::fixture_text("plot-vectors.json"));
        let nice_step = root
            .get("niceStep")
            .array()
            .iter()
            .map(|row| NiceStepRow {
                input: real(row.get("input")),
                input_label: label(row.get("input")),
                input_bits: row.get("inputBits").str().to_string(),
                output_bits: row.get("outputBits").str().to_string(),
            })
            .collect();
        let format_label = root
            .get("formatLabel")
            .array()
            .iter()
            .map(|row| {
                let primitives = row.get("rustPrimitives");
                FormatLabelRow {
                    input: real(row.get("input")),
                    input_label: label(row.get("input")),
                    input_bits: row.get("inputBits").str().to_string(),
                    output: row.get("output").str().to_string(),
                    fmt_1e: primitives.get("fmt_1e").str().to_string(),
                    fmt_2f: primitives.get("fmt_2f").str().to_string(),
                }
            })
            .collect();
        let eval = root
            .get("eval")
            .array()
            .iter()
            .map(|row| {
                let result = row.get("result");
                let bits = row.get("resultBits");
                let none = result.is_null() || result.opt_str() == Some("none") || bits.is_null();
                EvalRow {
                    expr: row.get("expr").str().to_string(),
                    x: real(row.get("x")),
                    x_label: label(row.get("x")),
                    x_bits: row.get("xBits").str().to_string(),
                    result: if none { None } else { Some(real(result)) },
                    result_bits: if none { None } else { Some(bits.str().to_string()) },
                }
            })
            .collect();
        let parse_errors =
            root.get("parseErrors").array().iter().map(|row| row.get("expr").str().to_string()).collect();
        let svg = root
            .get("svg")
            .array()
            .iter()
            .map(|row| SvgRow {
                name: row.get("name").str().to_string(),
                expr: row.get("expr").str().to_string(),
                x_min: row.get("xMin").f64(),
                x_max: row.get("xMax").f64(),
                y_min: row.get("yMin").f64(),
                y_max: row.get("yMax").f64(),
                first_point: row.get("firstPoint").opt_str().map(str::to_string),
                last_point: row.get("lastPoint").opt_str().map(str::to_string),
                polyline_count: row.get("polylineCount").i32(),
                line_count: row.get("lineCount").i32(),
                text_count: row.get("textCount").i32(),
                x_step: row.get("xStep").f64(),
                y_step: row.get("yStep").f64(),
                x_tick_labels: strings(row.get("xTickLabels")),
                y_tick_labels: strings(row.get("yTickLabels")),
            })
            .collect();
        Vectors { nice_step, format_label, eval, parse_errors, svg }
    })
}

fn real(element: &json::Json) -> f64 {
    if element.is_number() {
        return element.f64();
    }
    match element.str() {
        "nan" => f64::NAN,
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        text => dotnet_try_parse(text).unwrap_or(f64::NAN),
    }
}

fn label(element: &json::Json) -> String {
    if element.is_number() { dotnet_r(element.f64()) } else { element.str().to_string() }
}

fn strings(array: &json::Json) -> Vec<String> {
    array.array().iter().map(|item| item.str().to_string()).collect()
}

// MARK: - .NET's number formatting and parsing, where the C# suite leans on it

/// `double.ToString("R", CultureInfo.InvariantCulture)` on .NET Core 3.0+: the shortest
/// round-trip digits (Rust's `{:e}` finds the same ones), written in .NET's layout — plain
/// notation while the decimal point falls within `max(digits, 15)` places of the first digit and
/// no more than three zeros follow the point, otherwise `d.dddE±XX` with at least two exponent
/// digits. `1E+15`, `100000000000000`, `0.0001`, `1E-05`, `-0`, `Infinity`.
pub fn dotnet_r(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v == 0.0 {
        return format!("{sign}0");
    }
    let scientific = format!("{:e}", v.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("{:e} has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i32 = exponent.parse().expect("{:e} exponent");
    let count = digits.len() as i32;
    let scale = exponent + 1;
    if scale > count.max(15) || scale < -3 {
        let rest = if count > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{}{rest}E{exponent_sign}{:02}", &digits[..1], exponent.abs());
    }
    if scale <= 0 {
        return format!("{sign}0.{}{digits}", "0".repeat((-scale) as usize));
    }
    if scale >= count {
        return format!("{sign}{digits}{}", "0".repeat((scale - count) as usize));
    }
    format!("{sign}{}.{}", &digits[..scale as usize], &digits[scale as usize..])
}

/// `double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture)`: leading and
/// trailing white space, a sign, a point, an exponent; `Infinity`, `-Infinity`, `NaN` and `∞`
/// spelt the invariant culture's way.
pub fn dotnet_try_parse(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\u{000B}' | '\u{000C}' | '\r'));
    match trimmed {
        "NaN" | "-NaN" | "+NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" | "\u{221E}" | "+\u{221E}" => return Some(f64::INFINITY),
        "-Infinity" | "-\u{221E}" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    // Rust's grammar also takes `inf`, `infinity` and `nan` in any case, which .NET does not.
    let body = trimmed.trim_start_matches(['+', '-']);
    if !body.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    trimmed.parse().ok()
}

/// `double.Parse(text, NumberStyles.Float, CultureInfo.InvariantCulture)` on a decimal literal.
pub fn literal(text: &str) -> f64 {
    dotnet_try_parse(text).unwrap_or_else(|| panic!("not a number: {text}"))
}

// MARK: - Bits

/// The big-endian IEEE-754 bit pattern, the way the vectors record a double.
pub fn bits(value: f64) -> String {
    format!("{:016x}", value.to_bits())
}

pub fn from_bits(bits: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(bits, 16).unwrap_or_else(|e| panic!("bits {bits}: {e}")))
}

/// Bit equality, with NaN compared as NaN and never by payload or sign: a runtime's NaN may be
/// `fff8000000000000` where Rust's oracle recorded `7ff8000000000000`. No program can tell them
/// apart.
pub fn assert_bits(want: &str, got: f64, label: &str) {
    if got.is_nan() {
        assert!(is_nan_bits(want), "{label} = NaN, want {want}");
        return;
    }
    assert!(want == bits(got), "{label} = {}, want {want}", bits(got));
}

pub fn is_nan_bits(bits: &str) -> bool {
    from_bits(bits).is_nan()
}

/// Distance in representable doubles. `i64::MAX` when the two are not even on the same side of
/// zero, so a sign flip (or +0 against −0, which the corpus records apart) can never pass as
/// "close enough".
pub fn ulp_distance(a: f64, b: f64) -> i64 {
    let ia = a.to_bits() as i64;
    let ib = b.to_bits() as i64;
    if (ia < 0) != (ib < 0) {
        return if ia == ib { 0 } else { i64::MAX };
    }
    (ia - ib).abs()
}

// MARK: - Plot shortcuts

/// `f(x)` through the public surface.
pub fn value(expr: &str, x: f64) -> Result<f64, PlotError> {
    plot::parse_expression(expr, "x").map(|node| plot::evaluate(&node, "x", x))
}

/// [`value`] for an expression that must parse.
pub fn v(expr: &str, x: f64) -> f64 {
    value(expr, x).unwrap_or_else(|e| panic!("{expr}: {e}"))
}

pub fn lines(parts: &[&str]) -> String {
    parts.join("\n")
}

pub fn svg(source: &str) -> String {
    plot::plot_svg(source).unwrap_or_else(|e| panic!("{source:?}: {e}"))
}

// MARK: - Assertions — ordinal, as every string assertion in the C# suite is

pub fn clip(s: &str) -> String {
    if s.chars().count() <= 200 { s.to_string() } else { format!("{}…", s.chars().take(200).collect::<String>()) }
}

pub fn has(haystack: &str, needle: &str, note: &str) {
    assert!(haystack.contains(needle), "{note} expected to contain '{needle}' in: {}", clip(haystack));
}

pub fn lacks(haystack: &str, needle: &str, note: &str) {
    assert!(!haystack.contains(needle), "{note} expected NOT to contain '{needle}' in: {}", clip(haystack));
}

pub fn starts_with(text: &str, prefix: &str) {
    assert!(text.starts_with(prefix), "expected prefix '{prefix}' in: {}", clip(text));
}

pub fn ends_with(text: &str, suffix: &str) {
    assert!(text.ends_with(suffix), "expected suffix '{suffix}' in: {}", clip(text));
}

/// Assert that `result` is a [`PlotError`] whose message contains `message`.
pub fn assert_plot_error<T: std::fmt::Debug>(result: Result<T, PlotError>, message: &str, note: &str) -> PlotError {
    match result {
        Ok(value) => panic!("expected a PlotError containing '{message}' {note}, got {value:?}"),
        Err(error) => {
            if !message.is_empty() {
                assert!(error.message().contains(message), "'{}' should contain '{message}' {note}", error.message());
            }
            error
        }
    }
}

// MARK: - The suite's regular expressions, by hand

/// Every match of `prefix([^stop]*)suffix`, as `Regex.Matches` finds them (leftmost, never
/// overlapping): the captures.
pub fn captures(text: &str, prefix: &str, stop: char, suffix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(prefix) {
        let start = from + found;
        let body_start = start + prefix.len();
        let body_end = text[body_start..].find(stop).map_or(text.len(), |at| body_start + at);
        if text[body_end..].starts_with(suffix) {
            out.push(text[body_start..body_end].to_string());
            from = body_end + suffix.len();
        } else {
            // Every prefix here begins with an ASCII byte, so one past it is a char boundary.
            from = start + 1;
        }
    }
    out
}

/// `new Regex(literal).Matches(text).Count`.
pub fn count(text: &str, literal: &str) -> usize {
    text.matches(literal).count()
}

/// `<polyline points="([^"]*)"` — every run.
pub fn runs(svg: &str) -> Vec<String> {
    captures(svg, "<polyline points=\"", '"', "\"")
}

/// `text-anchor="middle">([^<]*)</text>` — the x tick labels (and a centred title or x label).
pub fn x_labels(svg: &str) -> Vec<String> {
    captures(svg, "text-anchor=\"middle\">", '<', "</text>")
}

/// `dominant-baseline="middle">([^<]*)</text>` — the y tick labels.
pub fn y_labels(svg: &str) -> Vec<String> {
    captures(svg, "dominant-baseline=\"middle\">", '<', "</text>")
}

/// `points="([^"]*)"`.
pub fn points_attributes(svg: &str) -> Vec<String> {
    captures(svg, "points=\"", '"', "\"")
}

/// `&[a-z]+;` — every entity-shaped run.
pub fn entities(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'&' {
            let mut end = index + 1;
            while end < bytes.len() && bytes[end].is_ascii_lowercase() {
                end += 1;
            }
            if end > index + 1 && end < bytes.len() && bytes[end] == b';' {
                out.push(text[index..=end].to_string());
                index = end + 1;
                continue;
            }
        }
        index += 1;
    }
    out
}

/// ` (?:x|y|x1|y1|x2|y2|width|height|cx|cy)="([^"]*)"` — every numeric attribute's value.
pub fn numeric_attributes(text: &str) -> Vec<String> {
    const NAMES: [&str; 10] = ["x", "y", "x1", "y1", "x2", "y2", "width", "height", "cx", "cy"];
    let mut out = Vec::new();
    let mut index = 0;
    'scan: while let Some(found) = text[index..].find(' ') {
        let start = index + found;
        for name in NAMES {
            let opener = format!(" {name}=\"");
            if text[start..].starts_with(&opener) {
                let body_start = start + opener.len();
                if let Some(close) = text[body_start..].find('"') {
                    out.push(text[body_start..body_start + close].to_string());
                    index = body_start + close + 1;
                    continue 'scan;
                }
            }
        }
        index = start + 1;
    }
    out
}

/// `<rect x="([\d.]+)" y="([\d.]+)" width="([\d.]+)" height="([\d.]+)"` — the first match's whole
/// text (`Match.Value`, `""` when there is none) and its four groups.
pub fn rect(svg: &str) -> Option<(String, [String; 4])> {
    const NAMES: [&str; 4] = ["<rect x=\"", "\" y=\"", "\" width=\"", "\" height=\""];
    let mut from = 0;
    'scan: while let Some(found) = svg[from..].find(NAMES[0]) {
        let start = from + found;
        let mut at = start;
        let mut groups: [String; 4] = Default::default();
        for (index, opener) in NAMES.iter().enumerate() {
            if !svg[at..].starts_with(opener) {
                from = start + 1;
                continue 'scan;
            }
            at += opener.len();
            let length = svg[at..].bytes().take_while(|c| c.is_ascii_digit() || *c == b'.').count();
            if length == 0 {
                from = start + 1;
                continue 'scan;
            }
            groups[index] = svg[at..at + length].to_string();
            at += length;
        }
        if !svg[at..].starts_with('"') {
            from = start + 1;
            continue;
        }
        return Some((svg[start..=at].to_string(), groups));
    }
    None
}

/// `rect(svg)`'s whole match, or `""` — `RectPattern.Match(svg).Value`.
pub fn rect_value(svg: &str) -> String {
    rect(svg).map(|(whole, _)| whole).unwrap_or_default()
}

/// `[A-Za-z_][A-Za-z0-9_]*` — every identifier-shaped run.
pub fn identifiers(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_alphabetic() || bytes[index] == b'_' {
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_') {
                index += 1;
            }
            out.push(&text[start..index]);
        } else {
            index += 1;
        }
    }
    out
}

// MARK: - A small XML reader, for the well-formedness checks `XDocument.Parse` makes

pub mod xml {
    #[derive(Debug)]
    pub enum Node {
        Element(Element),
        Text(String),
    }

    #[derive(Debug)]
    pub struct Element {
        pub name: String,
        pub namespace: String,
        pub attributes: Vec<(String, String)>,
        pub children: Vec<Node>,
    }

    impl Element {
        /// `XElement.Value`: the text of every descendant, in order.
        pub fn value(&self) -> String {
            let mut out = String::new();
            for child in &self.children {
                match child {
                    Node::Text(text) => out.push_str(text),
                    Node::Element(element) => out.push_str(&element.value()),
                }
            }
            out
        }

        /// `XDocument.Descendants(name)` from the root: this element and every element below it,
        /// in document order, that has this namespace and local name.
        pub fn descendants<'a>(&'a self, namespace: &str, name: &str) -> Vec<&'a Element> {
            let mut out = Vec::new();
            self.collect(namespace, name, &mut out);
            out
        }

        fn collect<'a>(&'a self, namespace: &str, name: &str, out: &mut Vec<&'a Element>) {
            if self.namespace == namespace && self.name == name {
                out.push(self);
            }
            for child in &self.children {
                if let Node::Element(element) = child {
                    element.collect(namespace, name, out);
                }
            }
        }

        /// `XElement.Element(name)`: the first child element with this namespace and name.
        pub fn element(&self, namespace: &str, name: &str) -> Option<&Element> {
            self.children.iter().find_map(|child| match child {
                Node::Element(element) if element.namespace == namespace && element.name == name => Some(element),
                _ => None,
            })
        }
    }

    /// Parse a document with one root element: start and end tags that match, attributes quoted
    /// once each, no raw `<` in a value, every `&` a defined entity or a character reference, and
    /// only XML 1.0 characters. Comments, processing instructions and CDATA sections are read and
    /// dropped. Line breaks are normalised to `\n` first, as an XML processor does.
    pub fn parse(text: &str) -> Result<Element, String> {
        let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(bad) = normalised.chars().find(|&c| !is_xml_char(c)) {
            return Err(format!("character U+{:04X} is not allowed in XML", bad as u32));
        }
        let mut reader = Reader { chars: normalised.chars().collect(), at: 0 };
        reader.misc()?;
        let root = reader.element("")?;
        reader.misc()?;
        if reader.at != reader.chars.len() {
            return Err(format!("content after the root element at {}", reader.at));
        }
        Ok(root)
    }

    fn is_xml_char(c: char) -> bool {
        matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
    }

    fn is_name_start(c: char) -> bool {
        c.is_alphabetic() || c == '_' || c == ':'
    }

    fn is_name_char(c: char) -> bool {
        is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.'
    }

    struct Reader {
        chars: Vec<char>,
        at: usize,
    }

    impl Reader {
        fn starts_with(&self, text: &str) -> bool {
            (self.at..).zip(text.chars()).all(|(at, c)| self.chars.get(at) == Some(&c))
        }

        fn peek(&self) -> Option<char> {
            self.chars.get(self.at).copied()
        }

        fn skip_space(&mut self) -> bool {
            let start = self.at;
            while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
                self.at += 1;
            }
            self.at > start
        }

        fn skip_past(&mut self, end: &str) -> Result<(), String> {
            while self.at < self.chars.len() {
                if self.starts_with(end) {
                    self.at += end.chars().count();
                    return Ok(());
                }
                self.at += 1;
            }
            Err(format!("unterminated construct, expected {end}"))
        }

        /// White space, comments and processing instructions around the root.
        fn misc(&mut self) -> Result<(), String> {
            loop {
                self.skip_space();
                if self.starts_with("<?") {
                    self.skip_past("?>")?;
                } else if self.starts_with("<!--") {
                    self.skip_past("-->")?;
                } else {
                    return Ok(());
                }
            }
        }

        fn name(&mut self) -> Result<String, String> {
            let start = self.at;
            match self.peek() {
                Some(c) if is_name_start(c) => self.at += 1,
                other => return Err(format!("expected a name at {start}, found {other:?}")),
            }
            while matches!(self.peek(), Some(c) if is_name_char(c)) {
                self.at += 1;
            }
            Ok(self.chars[start..self.at].iter().collect())
        }

        fn reference(&mut self) -> Result<char, String> {
            // At the `&`.
            self.at += 1;
            let start = self.at;
            while matches!(self.peek(), Some(c) if c != ';' && !c.is_whitespace() && c != '&' && c != '<') {
                self.at += 1;
            }
            if self.peek() != Some(';') {
                return Err(format!("a raw '&' at {}", start - 1));
            }
            let body: String = self.chars[start..self.at].iter().collect();
            self.at += 1;
            let scalar = if let Some(hex) = body.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(decimal) = body.strip_prefix('#') {
                decimal.parse::<u32>().ok()
            } else {
                return match body.as_str() {
                    "amp" => Ok('&'),
                    "lt" => Ok('<'),
                    "gt" => Ok('>'),
                    "quot" => Ok('"'),
                    "apos" => Ok('\''),
                    _ => Err(format!("undefined entity &{body};")),
                };
            };
            scalar
                .and_then(char::from_u32)
                .filter(|&c| is_xml_char(c))
                .ok_or_else(|| format!("bad character reference &{body};"))
        }

        fn element(&mut self, inherited: &str) -> Result<Element, String> {
            if self.peek() != Some('<') {
                return Err(format!("expected '<' at {}", self.at));
            }
            self.at += 1;
            let qualified = self.name()?;
            let mut attributes: Vec<(String, String)> = Vec::new();
            let empty;
            loop {
                let spaced = self.skip_space();
                if self.starts_with("/>") {
                    self.at += 2;
                    empty = true;
                    break;
                }
                if self.peek() == Some('>') {
                    self.at += 1;
                    empty = false;
                    break;
                }
                if !spaced {
                    return Err(format!("expected white space before an attribute at {}", self.at));
                }
                let name = self.name()?;
                self.skip_space();
                if self.peek() != Some('=') {
                    return Err(format!("attribute {name} has no value"));
                }
                self.at += 1;
                self.skip_space();
                let quote = match self.peek() {
                    Some(q @ ('"' | '\'')) => q,
                    _ => return Err(format!("attribute {name} is not quoted")),
                };
                self.at += 1;
                let mut value = String::new();
                loop {
                    match self.peek() {
                        None => return Err(format!("attribute {name} is not closed")),
                        Some(c) if c == quote => {
                            self.at += 1;
                            break;
                        }
                        Some('<') => return Err(format!("a raw '<' in attribute {name}")),
                        Some('&') => value.push(self.reference()?),
                        Some(c) => {
                            value.push(c);
                            self.at += 1;
                        }
                    }
                }
                if attributes.iter().any(|(existing, _)| *existing == name) {
                    return Err(format!("attribute {name} appears twice"));
                }
                attributes.push((name, value));
            }
            let namespace = attributes
                .iter()
                .find(|(name, _)| name == "xmlns")
                .map_or_else(|| inherited.to_string(), |(_, value)| value.clone());
            if qualified.contains(':') {
                return Err(format!("prefixed element names are not supported: {qualified}"));
            }
            let mut element = Element { name: qualified.clone(), namespace, attributes, children: Vec::new() };
            if empty {
                return Ok(element);
            }
            let mut text = String::new();
            loop {
                if self.at >= self.chars.len() {
                    return Err(format!("element {qualified} is not closed"));
                }
                if self.starts_with("</") {
                    if !text.is_empty() {
                        element.children.push(Node::Text(std::mem::take(&mut text)));
                    }
                    self.at += 2;
                    let closing = self.name()?;
                    if closing != qualified {
                        return Err(format!("</{closing}> closes <{qualified}>"));
                    }
                    self.skip_space();
                    if self.peek() != Some('>') {
                        return Err(format!("end tag {closing} is not closed"));
                    }
                    self.at += 1;
                    return Ok(element);
                }
                if self.starts_with("<!--") {
                    self.skip_past("-->")?;
                    continue;
                }
                if self.starts_with("<![CDATA[") {
                    self.at += "<![CDATA[".len();
                    let start = self.at;
                    self.skip_past("]]>")?;
                    text.extend(&self.chars[start..self.at - 3]);
                    continue;
                }
                if self.starts_with("<?") {
                    self.skip_past("?>")?;
                    continue;
                }
                if self.peek() == Some('<') {
                    if !text.is_empty() {
                        element.children.push(Node::Text(std::mem::take(&mut text)));
                    }
                    let namespace = element.namespace.clone();
                    let child = self.element(&namespace)?;
                    element.children.push(Node::Element(child));
                    continue;
                }
                if self.starts_with("]]>") {
                    return Err("']]>' in text".to_string());
                }
                if self.peek() == Some('&') {
                    text.push(self.reference()?);
                    continue;
                }
                if let Some(c) = self.peek() {
                    text.push(c);
                }
                self.at += 1;
            }
        }
    }
}
