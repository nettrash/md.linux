//! The little JSON `session.json` needs, std only (md-logic takes no serde): a strict RFC 8259
//! reader into a value tree, and the escaping a writer needs. md.win uses `System.Text.Json`.
//!
//! The reader is as strict as `JsonDocument.Parse` with its default options — no comments, no
//! trailing commas, no single quotes, no leading zeros, nesting at most 64 deep — because a file
//! it cannot read whole is read as nothing at all (`session_store::decode`).

/// One parsed value. A number keeps its literal, so a caller decides what it may be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// An object's member. A name given twice answers with its last value.
    pub(crate) fn get(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Object(members) => members.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `JsonElement.TryGetInt32`: an integer literal (no fraction, no exponent) that fits `i32`.
    pub(crate) fn as_i32(&self) -> Option<i32> {
        match self {
            Value::Number(literal) if !literal.contains(['.', 'e', 'E']) => literal.parse().ok(),
            _ => None,
        }
    }
}

/// The value `text` holds, or `None` for anything that is not exactly one JSON value (whitespace
/// around it allowed).
pub(crate) fn parse(text: &str) -> Option<Value> {
    let mut parser = Parser { bytes: text.as_bytes(), pos: 0, depth: 0 };
    parser.skip_whitespace();
    let value = parser.value()?;
    parser.skip_whitespace();
    (parser.pos == parser.bytes.len()).then_some(value)
}

/// `"…"` with the escapes JSON requires — `"`, `\` and the C0 controls — and every other
/// character as itself, UTF-8.
pub(crate) fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

const MAX_DEPTH: usize = 64;

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Option<Value> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Some(value)
        } else {
            None
        }
    }

    fn value(&mut self) -> Option<Value> {
        match self.peek()? {
            b'{' => self.nested(Self::object),
            b'[' => self.nested(Self::array),
            b'"' => self.string().map(Value::String),
            b't' => self.literal("true", Value::Bool(true)),
            b'f' => self.literal("false", Value::Bool(false)),
            b'n' => self.literal("null", Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn nested(&mut self, read: fn(&mut Self) -> Option<Value>) -> Option<Value> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let value = read(self);
        self.depth -= 1;
        value
    }

    fn object(&mut self) -> Option<Value> {
        self.pos += 1; // '{'
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.eat(b'}') {
            return Some(Value::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return None;
            }
            let name = self.string()?;
            self.skip_whitespace();
            if !self.eat(b':') {
                return None;
            }
            self.skip_whitespace();
            let value = self.value()?;
            members.push((name, value));
            self.skip_whitespace();
            if self.eat(b',') {
                continue;
            }
            return self.eat(b'}').then_some(Value::Object(members));
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.eat(b']') {
            return Some(Value::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value()?);
            self.skip_whitespace();
            if self.eat(b',') {
                continue;
            }
            return self.eat(b']').then_some(Value::Array(items));
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        self.eat(b'-');
        match self.peek()? {
            b'0' => self.pos += 1,
            b'1'..=b'9' => self.digits(),
            _ => return None,
        }
        if self.eat(b'.') {
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            self.digits();
        }
        // The literal is ASCII, so the slice is on character boundaries.
        let literal = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        Some(Value::Number(literal.to_string()))
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
    }

    /// A string at `"`. Raw control characters are refused, as is an escaped surrogate that is not
    /// half of a pair (a Rust string cannot hold one).
    fn string(&mut self) -> Option<String> {
        self.pos += 1; // '"'
        let mut out = Vec::new();
        loop {
            let byte = self.peek()?;
            self.pos += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escape = self.peek()?;
                    self.pos += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => self.unicode_escape()?,
                        _ => return None,
                    };
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                0x00..=0x1F => return None,
                other => out.push(other),
            }
        }
    }

    /// After `\u`: four hex digits, and for a high surrogate the `\uDC00`–`\uDFFF` that must
    /// follow.
    fn unicode_escape(&mut self) -> Option<char> {
        let first = self.hex4()?;
        if (0xDC00..=0xDFFF).contains(&first) {
            return None;
        }
        if !(0xD800..=0xDBFF).contains(&first) {
            return char::from_u32(u32::from(first));
        }
        if !(self.eat(b'\\') && self.eat(b'u')) {
            return None;
        }
        let second = self.hex4()?;
        if !(0xDC00..=0xDFFF).contains(&second) {
            return None;
        }
        char::from_u32(0x10000 + ((u32::from(first) - 0xD800) << 10) + (u32::from(second) - 0xDC00))
    }

    fn hex4(&mut self) -> Option<u16> {
        let digits = self.bytes.get(self.pos..self.pos + 4)?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.pos += 4;
        u16::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_strictly() {
        assert_eq!(parse(" {\"a\": [1, -2.5e3, true, null, \"x\\u00e9\\ud83d\\ude00\"]} "), {
            Some(Value::Object(vec![(
                "a".to_string(),
                Value::Array(vec![
                    Value::Number("1".into()),
                    Value::Number("-2.5e3".into()),
                    Value::Bool(true),
                    Value::Null,
                    Value::String("x\u{e9}\u{1F600}".into()),
                ]),
            )]))
        });
        for bad in ["", "{", "[1,]", "{\"a\":1,}", "01", "1.", "'a'", "\"\\ud800\"", "\"a\tb\"", "{} x", "nul"] {
            assert_eq!(parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nesting_stops_at_sixty_four() {
        let ok = format!("{}{}", "[".repeat(64), "]".repeat(64));
        let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
        assert!(parse(&ok).is_some());
        assert!(parse(&deep).is_none());
    }

    #[test]
    fn a_repeated_name_answers_with_its_last_value_and_integers_are_integers() {
        let value = parse("{\"v\":1,\"v\":2,\"f\":1.0,\"e\":1e2,\"big\":4294967296}").unwrap();
        assert_eq!(value.get("v").and_then(Value::as_i32), Some(2));
        assert_eq!(value.get("f").and_then(Value::as_i32), None);
        assert_eq!(value.get("e").and_then(Value::as_i32), None);
        assert_eq!(value.get("big").and_then(Value::as_i32), None);
    }

    #[test]
    fn written_strings_read_back() {
        let original = "/home/n/\"quoted\" \\ back\tslash\u{1}/\u{41F}\u{440}\u{438}.md";
        let mut out = String::new();
        write_string(&mut out, original);
        assert_eq!(parse(&out), Some(Value::String(original.to_string())));
    }
}
