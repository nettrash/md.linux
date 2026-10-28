//! A small, strict XML 1.0 + Namespaces reader for the EPUB tests — the stand-in for md.win's
//! `XmlReader` (the well-formedness gate, `DtdProcessing.Ignore`) and `XDocument.Parse` (a
//! namespace-aware tree). md-core takes no XML dependency, and neither may its tests, so this is
//! written from the XML 1.0 (Fifth Edition) productions it names. It is a *checker*: anything a
//! conforming parser rejects that an EPUB writer could plausibly emit is rejected here too — an
//! undeclared entity (`&bull;`), an unclosed `<br>`, a stray `<` or `&`, a mismatched end tag, a
//! duplicate or unbound-prefix attribute, a character XML forbids, a second root, `]]>` in text,
//! `--` in a comment. `tests/export_epub_xml_oracle.rs` pins each refusal, so a well-formedness
//! test built on it cannot pass vacuously.
//!
//! Not implemented, because nothing here produces them: an internal DTD subset is skipped rather
//! than read (no entity it declares is ever expanded), and the XML declaration is checked for its
//! shape only.
#![allow(dead_code)]

use std::collections::HashMap;

pub const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";
pub const XMLNS_NAMESPACE: &str = "http://www.w3.org/2000/xmlns/";

/// One attribute with its expanded name. Namespace declarations are kept too, in the xmlns
/// namespace, as `XElement.Attributes()` reports them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub namespace: Option<String>,
    pub local: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub namespace: Option<String>,
    pub local: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Node>,
}

impl Element {
    /// `element.Name == XName.Get(local, namespace)`.
    pub fn is(&self, namespace: &str, local: &str) -> bool {
        self.namespace.as_deref() == Some(namespace) && self.local == local
    }

    /// `element.Attribute(local)` — an attribute in no namespace.
    pub fn attribute(&self, local: &str) -> Option<&str> {
        self.attributes.iter().find(|a| a.namespace.is_none() && a.local == local).map(|a| a.value.as_str())
    }

    /// `element.Attribute(XName.Get(local, namespace))`.
    pub fn attribute_ns(&self, namespace: &str, local: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.namespace.as_deref() == Some(namespace) && a.local == local)
            .map(|a| a.value.as_str())
    }

    /// The child elements, in order (`Elements()`).
    pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    /// `Elements(XName.Get(local, namespace))`.
    pub fn elements<'a>(&'a self, namespace: &'a str, local: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.child_elements().filter(move |e| e.is(namespace, local))
    }

    /// `Element(XName.Get(local, namespace))` — the first such child.
    pub fn element(&self, namespace: &str, local: &str) -> Option<&Element> {
        self.child_elements().find(|e| e.is(namespace, local))
    }

    /// Every descendant element in document order, this one excluded (`Descendants()`).
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = Vec::new();
        fn walk<'a>(element: &'a Element, out: &mut Vec<&'a Element>) {
            for child in element.child_elements() {
                out.push(child);
                walk(child, out);
            }
        }
        walk(self, &mut out);
        out
    }

    /// `XElement.Value`: the concatenated text of every descendant text node.
    pub fn value(&self) -> String {
        let mut out = String::new();
        fn walk(element: &Element, out: &mut String) {
            for child in &element.children {
                match child {
                    Node::Text(text) => out.push_str(text),
                    Node::Element(inner) => walk(inner, out),
                }
            }
        }
        walk(self, &mut out);
        out
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub root: Element,
}

impl Document {
    /// `XDocument.Descendants()`: the root and everything under it.
    pub fn descendants(&self) -> Vec<&Element> {
        let mut out = vec![&self.root];
        out.extend(self.root.descendants());
        out
    }
}

/// Parse `text` as a namespace-well-formed XML document, or say why it is not one.
pub fn parse(text: &str) -> Result<Document, String> {
    // §2.11: line ends are normalised before anything else sees them.
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut parser = Parser { s: &normalised, at: 0 };
    parser.document()
}

/// Whether `text` parses — the `XmlReader` loop of `AssertArchiveXmlIsWellFormed`.
pub fn is_well_formed(text: &str) -> bool {
    parse(text).is_ok()
}

struct Parser<'a> {
    s: &'a str,
    at: usize,
}

/// A raw start tag, before its names are resolved against the namespace scope.
struct RawElement {
    qname: String,
    attributes: Vec<(String, String)>,
    children: Vec<RawNode>,
}

enum RawNode {
    Element(RawElement),
    Text(String),
}

fn is_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

fn is_name_start(c: char) -> bool {
    matches!(c,
        ':' | 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}')
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.s[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn starts(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    fn error<T>(&self, what: &str) -> Result<T, String> {
        let line = self.s[..self.at].matches('\n').count() + 1;
        let context: String = self.rest().chars().take(40).collect();
        Err(format!("{what} at line {line}, byte {}: {context:?}", self.at))
    }

    fn expect(&mut self, literal: &str) -> Result<(), String> {
        if self.starts(literal) {
            self.at += literal.len();
            Ok(())
        } else {
            self.error(&format!("expected {literal:?}"))
        }
    }

    fn skip_space(&mut self) -> bool {
        let before = self.at;
        while let Some(c) = self.peek().filter(|&c| is_space(c)) {
            self.at += c.len_utf8();
        }
        self.at > before
    }

    fn name(&mut self) -> Result<String, String> {
        let start = self.at;
        match self.peek() {
            Some(c) if is_name_start(c) => self.at += c.len_utf8(),
            _ => return self.error("expected a name"),
        }
        while let Some(c) = self.peek().filter(|&c| is_name_char(c)) {
            self.at += c.len_utf8();
        }
        Ok(self.s[start..self.at].to_string())
    }

    /// Everything up to `terminator` (consumed), every character checked.
    fn until(&mut self, terminator: &str, what: &str) -> Result<String, String> {
        let Some(found) = self.rest().find(terminator) else {
            return self.error(&format!("unterminated {what}"));
        };
        let body = self.rest()[..found].to_string();
        if let Some(bad) = body.chars().find(|&c| !is_char(c)) {
            return self.error(&format!("character U+{:04X} is not allowed in XML", bad as u32));
        }
        self.at += found + terminator.len();
        Ok(body)
    }

    fn document(&mut self) -> Result<Document, String> {
        if self.starts("<?xml") && self.rest()[5..].starts_with(|c: char| is_space(c) || c == '?') {
            let declaration = self.until("?>", "XML declaration")?;
            if !declaration.contains("version=\"1.0\"") && !declaration.contains("version='1.0'") {
                return self.error("the XML declaration names no version 1.0");
            }
        }
        let mut seen_doctype = false;
        let mut root: Option<RawElement> = None;
        loop {
            self.skip_space();
            if self.at == self.s.len() {
                break;
            }
            if self.starts("<!--") {
                self.comment()?;
            } else if self.starts("<?") {
                self.processing_instruction()?;
            } else if self.starts("<!DOCTYPE") {
                if seen_doctype || root.is_some() {
                    return self.error("a DOCTYPE out of place");
                }
                seen_doctype = true;
                self.doctype()?;
            } else if self.starts("<") {
                if root.is_some() {
                    return self.error("a second root element");
                }
                root = Some(self.element()?);
            } else {
                return self.error("text outside the root element");
            }
        }
        let Some(root) = root else {
            return self.error("no root element");
        };
        let mut scope = vec![HashMap::from([("xml".to_string(), XML_NAMESPACE.to_string())])];
        Ok(Document { root: resolve(root, &mut scope)? })
    }

    fn comment(&mut self) -> Result<(), String> {
        self.expect("<!--")?;
        let body = self.until("-->", "comment")?;
        if body.contains("--") || body.ends_with('-') {
            return self.error("'--' inside a comment");
        }
        Ok(())
    }

    fn processing_instruction(&mut self) -> Result<(), String> {
        self.expect("<?")?;
        let target = self.name()?;
        if target.eq_ignore_ascii_case("xml") {
            return self.error("an XML declaration that is not at the start");
        }
        if !self.starts("?>") && !self.skip_space() {
            return self.error("a processing instruction target runs into its data");
        }
        self.until("?>", "processing instruction")?;
        Ok(())
    }

    /// `<!DOCTYPE name (ExternalID)? S? ('[' intSubset ']' S?)? '>'` — read for its shape and
    /// skipped, as `DtdProcessing.Ignore` skips it.
    fn doctype(&mut self) -> Result<(), String> {
        self.expect("<!DOCTYPE")?;
        if !self.skip_space() {
            return self.error("expected space after DOCTYPE");
        }
        self.name()?;
        loop {
            match self.peek() {
                None => return self.error("unterminated DOCTYPE"),
                Some('>') => {
                    self.at += 1;
                    return Ok(());
                }
                Some(quote @ ('"' | '\'')) => {
                    self.at += 1;
                    self.until(&quote.to_string(), "DOCTYPE literal")?;
                }
                Some('[') => {
                    self.at += 1;
                    self.until("]", "internal subset")?;
                }
                Some(c) => self.at += c.len_utf8(),
            }
        }
    }

    fn element(&mut self) -> Result<RawElement, String> {
        self.expect("<")?;
        let qname = self.name()?;
        let mut attributes: Vec<(String, String)> = Vec::new();
        loop {
            let spaced = self.skip_space();
            if self.starts("/>") {
                self.at += 2;
                return Ok(RawElement { qname, attributes, children: Vec::new() });
            }
            if self.starts(">") {
                self.at += 1;
                break;
            }
            if !spaced {
                return self.error("attributes must be separated by white space");
            }
            let name = self.name()?;
            self.skip_space();
            self.expect("=")?;
            self.skip_space();
            let value = self.attribute_value()?;
            if attributes.iter().any(|(existing, _)| *existing == name) {
                return self.error(&format!("duplicate attribute {name}"));
            }
            attributes.push((name, value));
        }
        let children = self.content()?;
        self.expect("</")?;
        let closing = self.name()?;
        if closing != qname {
            return self.error(&format!("end tag </{closing}> does not match <{qname}>"));
        }
        self.skip_space();
        self.expect(">")?;
        Ok(RawElement { qname, attributes, children })
    }

    fn attribute_value(&mut self) -> Result<String, String> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return self.error("an attribute value must be quoted"),
        };
        self.at += 1;
        let mut value = String::new();
        loop {
            match self.peek() {
                None => return self.error("unterminated attribute value"),
                Some(c) if c == quote => {
                    self.at += 1;
                    return Ok(value);
                }
                Some('<') => return self.error("'<' in an attribute value"),
                Some('&') => value.push_str(&self.reference()?),
                // §3.3.3: literal white space in an attribute value is normalised to a space.
                Some('\t' | '\n' | '\r') => {
                    value.push(' ');
                    self.at += 1;
                }
                Some(c) if !is_char(c) => {
                    return self.error(&format!("character U+{:04X} is not allowed in XML", c as u32));
                }
                Some(c) => {
                    value.push(c);
                    self.at += c.len_utf8();
                }
            }
        }
    }

    /// `&name;` (the five predefined — no DTD declares any other), `&#N;` or `&#xH;`.
    fn reference(&mut self) -> Result<String, String> {
        self.expect("&")?;
        if self.starts("#") {
            self.at += 1;
            let hex = self.starts("x");
            if hex {
                self.at += 1;
            }
            let start = self.at;
            while self.peek().is_some_and(|c| if hex { c.is_ascii_hexdigit() } else { c.is_ascii_digit() }) {
                self.at += 1;
            }
            let digits = &self.s[start..self.at];
            let code = u32::from_str_radix(digits, if hex { 16 } else { 10 }).ok();
            self.expect(";")?;
            return match code.and_then(char::from_u32).filter(|&c| is_char(c)) {
                Some(c) => Ok(c.to_string()),
                None => self.error(&format!("&#{digits}; is not a legal character")),
            };
        }
        let name = self.name()?;
        self.expect(";")?;
        match name.as_str() {
            "amp" => Ok("&".to_string()),
            "lt" => Ok("<".to_string()),
            "gt" => Ok(">".to_string()),
            "quot" => Ok("\"".to_string()),
            "apos" => Ok("'".to_string()),
            _ => self.error(&format!("reference to undeclared entity '{name}'")),
        }
    }

    fn content(&mut self) -> Result<Vec<RawNode>, String> {
        let mut children = Vec::new();
        let mut text = String::new();
        loop {
            if self.at == self.s.len() {
                return self.error("unexpected end of document inside an element");
            }
            if self.starts("</") {
                break;
            }
            if self.starts("<!--") {
                self.comment()?;
            } else if self.starts("<![CDATA[") {
                self.at += "<![CDATA[".len();
                text.push_str(&self.until("]]>", "CDATA section")?);
            } else if self.starts("<?") {
                self.processing_instruction()?;
            } else if self.starts("<!") {
                return self.error("markup declaration inside an element");
            } else if self.starts("<") {
                if !text.is_empty() {
                    children.push(RawNode::Text(std::mem::take(&mut text)));
                }
                children.push(RawNode::Element(self.element()?));
            } else if self.starts("&") {
                text.push_str(&self.reference()?);
            } else if self.starts("]]>") {
                return self.error("']]>' in character data");
            } else {
                let c = self.peek().unwrap_or('\0');
                if !is_char(c) {
                    return self.error(&format!("character U+{:04X} is not allowed in XML", c as u32));
                }
                text.push(c);
                self.at += c.len_utf8();
            }
        }
        if !text.is_empty() {
            children.push(RawNode::Text(text));
        }
        Ok(children)
    }
}

/// Split a QName into (prefix, local), refusing an empty half or a second colon (Namespaces §3).
fn split_qname(qname: &str) -> Result<(Option<&str>, &str), String> {
    match qname.split_once(':') {
        None => Ok((None, qname)),
        Some((prefix, local)) if !prefix.is_empty() && !local.is_empty() && !local.contains(':') => {
            if local.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '.')) {
                return Err(format!("{qname} is not a QName"));
            }
            Ok((Some(prefix), local))
        }
        _ => Err(format!("{qname} is not a QName")),
    }
}

fn lookup(scope: &[HashMap<String, String>], prefix: &str) -> Option<String> {
    scope.iter().rev().find_map(|frame| frame.get(prefix)).cloned()
}

fn resolve(raw: RawElement, scope: &mut Vec<HashMap<String, String>>) -> Result<Element, String> {
    // Declarations first: they apply to the element that carries them.
    let mut frame = HashMap::new();
    for (name, value) in &raw.attributes {
        if name == "xmlns" {
            frame.insert(String::new(), value.clone());
        } else if let Some(prefix) = name.strip_prefix("xmlns:") {
            if value.is_empty() {
                return Err(format!("prefix {prefix} bound to the empty namespace"));
            }
            frame.insert(prefix.to_string(), value.clone());
        }
    }
    scope.push(frame);

    let (prefix, local) = split_qname(&raw.qname)?;
    let namespace = match prefix {
        Some(prefix) => Some(lookup(scope, prefix).ok_or_else(|| format!("undeclared prefix {prefix}"))?),
        None => lookup(scope, "").filter(|uri| !uri.is_empty()),
    };

    let mut attributes = Vec::with_capacity(raw.attributes.len());
    for (name, value) in &raw.attributes {
        let attribute = if name == "xmlns" {
            Attribute { namespace: Some(XMLNS_NAMESPACE.to_string()), local: "xmlns".to_string(), value: value.clone() }
        } else if let Some(declared) = name.strip_prefix("xmlns:") {
            Attribute {
                namespace: Some(XMLNS_NAMESPACE.to_string()),
                local: declared.to_string(),
                value: value.clone(),
            }
        } else {
            let (prefix, local) = split_qname(name)?;
            // An unprefixed attribute is in no namespace, whatever the default is.
            let namespace = match prefix {
                Some(prefix) => Some(lookup(scope, prefix).ok_or_else(|| format!("undeclared prefix {prefix}"))?),
                None => None,
            };
            Attribute { namespace, local: local.to_string(), value: value.clone() }
        };
        if attributes.iter().any(|a: &Attribute| a.namespace == attribute.namespace && a.local == attribute.local) {
            return Err(format!("attribute {name} appears twice by expanded name"));
        }
        attributes.push(attribute);
    }

    let mut children = Vec::with_capacity(raw.children.len());
    for child in raw.children {
        children.push(match child {
            RawNode::Text(text) => Node::Text(text),
            RawNode::Element(inner) => Node::Element(resolve(inner, scope)?),
        });
    }
    scope.pop();
    Ok(Element { namespace, local: local.to_string(), attributes, children })
}
