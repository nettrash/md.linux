//! Port of md.win `Preview/JsonScript.cs`: decoding what a script evaluation hands back.
//!
//! An evaluation answers in JSON, not as a value: `1` comes back as the one character `1`, the
//! string `"1"` as the three characters `"1"` (the render-complete flag is a string attribute,
//! which is why Android compares against `"\"1\""`), and a failed or void script as the four
//! characters `null`. On WebKitGTK the shell produces that text with
//! `javascriptcore::Value::to_json(0)` on the result of `evaluate_javascript_future` (and on a
//! script message's value), exactly the shape WebView2's `ExecuteScriptAsync` gives md.win.
//! Decoding it by hand is how a port ends up writing the literal word `null` into a file, so
//! everything goes through the one parser here.
//!
//! Nothing panics: a script that failed, a web view that is gone and a page that returned something
//! unexpected are all "no answer", and every caller has a defined behaviour for that.
//!
//! LINUX DECISION: md.win parses with `System.Text.Json.JsonDocument`; md-logic takes no JSON crate,
//! so [`parse`] is a strict RFC 8259 parser written for this: one value, optional surrounding
//! whitespace, no comments or trailing commas, at most 64 levels deep (JsonDocument's default; a
//! deeper document is malformed, never a stack overflow), the last of repeated object members wins
//! (JsonElement's `TryGetProperty`). A `\uD800`-style lone surrogate escape cannot live in a Rust
//! string and decodes to U+FFFD. md.win's `KeyMessage` is not ported: the GTK shell needs no
//! key-forwarding channel, because every accelerator fires once from the `WebKitWebView` too
//! (docs/linux-facts.md §1).

/// The JSON number, or `None` for `null`, a string, a non-finite value or malformed JSON.
pub fn number(json: &str) -> Option<f64> {
    match parse(json)? {
        Json::Number(value) if value.is_finite() => Some(value),
        _ => None,
    }
}

/// The JSON string, or `None` for `null`, a number, an object or malformed JSON.
pub fn string(json: &str) -> Option<String> {
    match parse(json)? {
        Json::String(value) => Some(value),
        _ => None,
    }
}

/// The scroll-sync message (§4.6): `{"fraction":0.42,"echo":false}`, the value the page posts
/// through `window.webkit.messageHandlers.mdScroll`. `None` when it is not one — the page can post
/// nothing else today, but the channel is open and the host must not crash on a surprise. A
/// missing `echo` is not an echo.
pub fn scroll_message(json: &str) -> Option<ScrollMessage> {
    let value = parse(json)?;
    let fraction = match value.member("fraction")? {
        Json::Number(f) if f.is_finite() => *f,
        _ => return None,
    };
    let echo = matches!(value.member("echo"), Some(Json::Bool(true)));
    Some(ScrollMessage { fraction, echo })
}

/// What the page's scroll listener posts: how far down it is, as a fraction of its scrollable
/// height, and whether the scroll was the host's own (inside the 300 ms programmatic window).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMessage {
    pub fraction: f64,
    pub echo: bool,
}

/// One parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Members in document order, duplicates kept.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An object's member by name — the last one when the name repeats; `None` for a non-object.
    pub fn member(&self, name: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().rev().find(|(key, _)| key == name).map(|(_, value)| value),
            _ => None,
        }
    }
}

/// The deepest nesting accepted (JsonDocument's default `MaxDepth`).
const MAX_DEPTH: usize = 64;

/// One JSON value, or `None` for anything that is not exactly one (empty input, `undefined`,
/// trailing text, a syntax error).
pub fn parse(json: &str) -> Option<Json> {
    let mut parser = Parser { bytes: json.as_bytes(), text: json, at: 0 };
    parser.whitespace();
    let value = parser.value(0)?;
    parser.whitespace();
    (parser.at == parser.bytes.len()).then_some(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Option<Json> {
        if self.text[self.at..].starts_with(word) {
            self.at += word.len();
            Some(value)
        } else {
            None
        }
    }

    fn value(&mut self, depth: usize) -> Option<Json> {
        match self.peek()? {
            b'n' => self.literal("null", Json::Null),
            b't' => self.literal("true", Json::Bool(true)),
            b'f' => self.literal("false", Json::Bool(false)),
            b'"' => self.string().map(Json::String),
            b'[' => self.array(depth + 1),
            b'{' => self.object(depth + 1),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn array(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.at += 1; // [
        let mut items = Vec::new();
        self.whitespace();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Some(Json::Array(items));
        }
        loop {
            self.whitespace();
            items.push(self.value(depth)?);
            self.whitespace();
            match self.peek()? {
                b',' => self.at += 1,
                b']' => {
                    self.at += 1;
                    return Some(Json::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn object(&mut self, depth: usize) -> Option<Json> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.at += 1; // {
        let mut members = Vec::new();
        self.whitespace();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Some(Json::Object(members));
        }
        loop {
            self.whitespace();
            if self.peek()? != b'"' {
                return None;
            }
            let key = self.string()?;
            self.whitespace();
            if self.peek()? != b':' {
                return None;
            }
            self.at += 1;
            self.whitespace();
            let value = self.value(depth)?;
            members.push((key, value));
            self.whitespace();
            match self.peek()? {
                b',' => self.at += 1,
                b'}' => {
                    self.at += 1;
                    return Some(Json::Object(members));
                }
                _ => return None,
            }
        }
    }

    /// RFC 8259 number grammar, then the shortest-correct `f64` parse — which overflows a huge
    /// exponent to infinity exactly as `JsonElement.TryGetDouble` does (callers refuse it).
    fn number(&mut self) -> Option<Json> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        match self.peek()? {
            b'0' => self.at += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        // The slice is ASCII digits and signs, so it is on char boundaries.
        self.text[start..self.at].parse::<f64>().ok().map(Json::Number)
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
    }

    /// A string at `"`: escapes decoded, a surrogate pair joined, a lone surrogate U+FFFD, an
    /// unescaped control character refused.
    fn string(&mut self) -> Option<String> {
        self.at += 1; // "
        let mut out = String::new();
        loop {
            let rest = &self.text[self.at..];
            // Copy the plain run up to the next quote, backslash or control character in one go.
            let run = rest.find(|c: char| c == '"' || c == '\\' || (c as u32) < 0x20).unwrap_or(rest.len());
            out.push_str(&rest[..run]);
            self.at += run;
            match self.peek()? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = self.peek()?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        _ => return None,
                    }
                }
                _ => return None, // a raw control character
            }
        }
    }

    /// The scalar a `\uXXXX` (already past the `u`) spells, joining a following low surrogate.
    fn unicode_escape(&mut self) -> Option<char> {
        let unit = self.hex4()?;
        if (0xD800..0xDC00).contains(&unit) && self.text[self.at..].starts_with("\\u") {
            let save = self.at;
            self.at += 2;
            let low = self.hex4()?;
            if (0xDC00..0xE000).contains(&low) {
                return char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00));
            }
            // Not a pair: the high half alone, and the next escape read on its own.
            self.at = save;
        }
        Some(char::from_u32(unit).unwrap_or('\u{FFFD}'))
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.at..self.at + 4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at += 4;
        u32::from_str_radix(digits, 16).ok()
    }
}
