//! The expression language: tokens, the tree, precedence climbing and evaluation.

use super::PlotError;
use super::format::round_ties_away;
use super::strings::{contains, is_digit, is_identifier_part, is_identifier_start, is_space};

// MARK: - Tokens

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Number,
    Name,
    Op,
    Open,
    Close,
    Comma,
    End,
}

/// One flat record: `text` is the spelling of a name or operator, `value` the value of a number.
#[derive(Clone, Copy, Debug)]
struct Token<'t> {
    kind: TokenKind,
    text: &'t str,
    value: f64,
}

/// The two-character operators, longest match first — `<=` before `<`.
const LONG_OPERATORS: [&str; 6] = ["||", "&&", "==", "!=", "<=", ">="];
const SHORT_OPERATORS: [&str; 9] = ["+", "-", "*", "/", "%", "^", "<", ">", "!"];

fn tokenize(text: &str) -> Result<Vec<Token<'_>>, PlotError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let c = bytes[index];
        // Space and tab only. Any other whitespace — NBSP, a newline — is an unexpected
        // character, as in every port.
        if is_space(c) {
            index += 1;
            continue;
        }
        if is_digit(c) || (c == b'.' && index + 1 < bytes.len() && is_digit(bytes[index + 1])) {
            let (end, value) = scan_number(text, index)?;
            tokens.push(Token { kind: TokenKind::Number, text: &text[index..end], value });
            index = end;
            continue;
        }
        if is_identifier_start(c) {
            let start = index;
            while index < bytes.len() && is_identifier_part(bytes[index]) {
                index += 1;
            }
            tokens.push(Token { kind: TokenKind::Name, text: &text[start..index], value: 0.0 });
            continue;
        }
        let single = match c {
            b'(' => Some(TokenKind::Open),
            b')' => Some(TokenKind::Close),
            b',' => Some(TokenKind::Comma),
            _ => None,
        };
        if let Some(kind) = single {
            tokens.push(Token { kind, text: &text[index..index + 1], value: 0.0 });
            index += 1;
            continue;
        }
        // Compared as bytes: a two-unit window may end inside a multi-byte character, which a
        // `&str` slice could not hold, and then it is no operator anyway.
        if index + 1 < bytes.len() && LONG_OPERATORS.iter().any(|op| op.as_bytes() == &bytes[index..index + 2]) {
            tokens.push(Token { kind: TokenKind::Op, text: &text[index..index + 2], value: 0.0 });
            index += 2;
            continue;
        }
        if c.is_ascii() && contains(&SHORT_OPERATORS, &text[index..index + 1]) {
            tokens.push(Token { kind: TokenKind::Op, text: &text[index..index + 1], value: 0.0 });
            index += 1;
            continue;
        }
        // Name the whole code point, as Swift does. `index` always sits on a char boundary (every
        // scan above consumes ASCII only), and a Rust string holds no lone surrogate half, so the
        // character is always whole here — md.win has to pair the halves by hand.
        let one = text[index..].chars().next().map(String::from).unwrap_or_default();
        return Err(PlotError::new(format!("unexpected character '{one}'")));
    }
    tokens.push(Token { kind: TokenKind::End, text: "", value: 0.0 });
    Ok(tokens)
}

/// A number literal: `123`, `1.5`, `.5`, `5.`, `1e-3`, `1.2E+4`. No hex, no digit separators,
/// no sign (that is unary minus). The exponent is taken only if at least one digit follows its
/// optional sign.
fn scan_number(text: &str, start: usize) -> Result<(usize, f64), PlotError> {
    let bytes = text.as_bytes();
    let mut index = start;
    while index < bytes.len() && is_digit(bytes[index]) {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && is_digit(bytes[index]) {
            index += 1;
        }
    }
    if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
        let mut lookahead = index + 1;
        if lookahead < bytes.len() && (bytes[lookahead] == b'+' || bytes[lookahead] == b'-') {
            lookahead += 1;
        }
        if lookahead < bytes.len() && is_digit(bytes[lookahead]) {
            index = lookahead;
            while index < bytes.len() && is_digit(bytes[index]) {
                index += 1;
            }
        }
    }
    let literal = &text[start..index];
    // The platform's decimal→binary conversion is the one place it is more trustworthy than
    // anything written by hand: correctly rounded, and saturating to ±∞ rather than failing on
    // `1e400`, which is what .NET (since Core 3.0), Swift, JavaScript and Kotlin all do with that
    // literal. Rust's grammar accepts every literal the scan above can produce (`5.`, `.5`,
    // `1.2E+4`).
    match literal.parse::<f64>() {
        Ok(value) if !value.is_nan() => Ok((index, value)),
        _ => Err(PlotError::new(format!("'{literal}' is not a number"))),
    }
}

// MARK: - The tree

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Number,
    Variable,
    Unary,
    Binary,
    Call,
}

/// An expression node — one record with the fields each kind uses, so the tree ports without
/// generics. Built only by [`parse_expression`], which is what lets [`evaluate`] trust its shape
/// (a call has its arity, a binary node both sides).
#[derive(Debug)]
pub struct Node {
    kind: NodeKind,
    value: f64,
    text: String,
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
    arguments: Vec<Node>,
}

impl Node {
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// `Number`.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// `Variable` (its name), `Unary`/`Binary` (the operator), `Call` (the function).
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `Unary` (the operand), `Binary` (the left side).
    pub fn left(&self) -> Option<&Node> {
        self.left.as_deref()
    }

    /// `Binary`.
    pub fn right(&self) -> Option<&Node> {
        self.right.as_deref()
    }

    /// `Call`.
    pub fn arguments(&self) -> &[Node] {
        &self.arguments
    }

    // The constructors box, and the parser passes boxes: a `Result<Node, _>` is ~80 bytes and
    // every recursive frame of the parser holds several, a boxed one is a pointer.

    fn number(value: f64) -> Box<Node> {
        Box::new(Node {
            kind: NodeKind::Number,
            value,
            text: String::new(),
            left: None,
            right: None,
            arguments: Vec::new(),
        })
    }

    fn variable(name: &str) -> Box<Node> {
        Box::new(Node {
            kind: NodeKind::Variable,
            value: 0.0,
            text: name.to_string(),
            left: None,
            right: None,
            arguments: Vec::new(),
        })
    }

    fn unary(op: &str, operand: Box<Node>) -> Box<Node> {
        Box::new(Node {
            kind: NodeKind::Unary,
            value: 0.0,
            text: op.to_string(),
            left: Some(operand),
            right: None,
            arguments: Vec::new(),
        })
    }

    fn binary(op: &str, left: Box<Node>, right: Box<Node>) -> Box<Node> {
        Box::new(Node {
            kind: NodeKind::Binary,
            value: 0.0,
            text: op.to_string(),
            left: Some(left),
            right: Some(right),
            arguments: Vec::new(),
        })
    }

    fn call(name: &str, arguments: Vec<Node>) -> Box<Node> {
        Box::new(Node { kind: NodeKind::Call, value: 0.0, text: name.to_string(), left: None, right: None, arguments })
    }

    /// Move this node's children onto `pending`, leaving it a leaf.
    fn release_children(&mut self, pending: &mut Vec<Node>) {
        if let Some(left) = self.left.take() {
            pending.push(*left);
        }
        if let Some(right) = self.right.take() {
            pending.push(*right);
        }
        pending.append(&mut self.arguments);
    }
}

/// Torn down with an explicit stack. The node budget allows a 4096-deep left chain
/// (`x+x+…`), and the compiler's recursive drop would spend a stack frame per level — the
/// same hazard as recursive evaluation, on the way out.
impl Drop for Node {
    fn drop(&mut self) {
        let mut pending = Vec::new();
        self.release_children(&mut pending);
        while let Some(mut node) = pending.pop() {
            node.release_children(&mut pending);
        }
    }
}

/// The function roster — exactly the site's names and arity, nothing else. `min`, `max`, `if`,
/// `log` are deliberately absent: a name that works in one port and not the others is worse
/// than one that works in none.
const FUNCTIONS: [&str; 26] = [
    "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "sqrt", "cbrt",
    "abs", "exp", "exp2", "ln", "log2", "log10", "floor", "ceil", "round", "atan2", "pow", "hypot",
];

/// The three two-argument functions; everything else takes one.
const BINARY_FUNCTIONS: [&str; 3] = ["atan2", "pow", "hypot"];

fn arity(name: &str) -> usize {
    if contains(&BINARY_FUNCTIONS, name) { 2 } else { 1 }
}

/// Precedence, loosest to tightest. `^` is the only right-associative level.
fn precedence(op: &str) -> i32 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | "<=" | ">" | ">=" => 3,
        "+" | "-" => 4,
        "*" | "/" | "%" => 5,
        "^" => 6,
        _ => 0,
    }
}

const POWER_PRECEDENCE: i32 = 6;

/// The deepest an expression may nest. Parser and evaluator both recurse in md.win, and a stack
/// overflow is not an error a process survives — in .NET or in Rust — so this guard is
/// load-bearing for the parser here (the evaluator and the tree's drop no longer recurse).
///
/// md.win also probes the stack (`TryEnsureSufficientExecutionStack`) and fails a fence on a
/// starved thread; safe Rust has no probe, so the bound itself is the guarantee. Measured, the
/// deepest legal fence of every shape (`(((…`, `---…`, `2^2^…`, `sin(sin(…`) parses in under
/// 64 KB of stack in a release build and under 384 KB unoptimised — far inside the 2 MiB every
/// Rust thread gets by default, and the main thread's 8 MiB.
const MAX_DEPTH: usize = 128;

/// The most binary/unary nodes one expression may hold. The depth guard cannot see a long flat
/// chain (`x+x+x+…` parses at constant depth); the budget keeps the output the siblings'.
const MAX_NODES: usize = 4096;

/// Parse `text` as an expression in which `variable` is the only free name (besides `pi` and
/// `e`). Pass `""` for a constant expression.
pub fn parse_expression(text: &str, variable: &str) -> Result<Node, PlotError> {
    let mut parser = Parser { tokens: tokenize(text)?, variable, index: 0, depth: 0, nodes: 0 };
    let node = parser.expression(1)?;
    parser.expect_end()?;
    Ok(*node)
}

/// Precedence climbing, hand-written — the same twenty lines in every port.
struct Parser<'t> {
    tokens: Vec<Token<'t>>,
    variable: &'t str,
    index: usize,
    depth: usize,
    nodes: usize,
}

impl<'t> Parser<'t> {
    /// Charge one node against the budget, so a flat chain cannot outrun the depth guard.
    fn count(&mut self) -> Result<(), PlotError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(PlotError::new("expression too large"));
        }
        Ok(())
    }

    fn expression(&mut self, minimum: i32) -> Result<Box<Node>, PlotError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(PlotError::new("expression nested too deeply"));
        }
        let result = self.expression_inner(minimum);
        self.depth -= 1;
        result
    }

    fn expression_inner(&mut self, minimum: i32) -> Result<Box<Node>, PlotError> {
        let mut left = self.unary()?;
        loop {
            let token = self.peek();
            if token.kind != TokenKind::Op {
                break;
            }
            let level = precedence(token.text);
            if level == 0 || level < minimum {
                break;
            }
            self.index += 1;
            // `^` is RIGHT-associative — `2^3^2` is 512. evalexpr's 64 is the site's bug, and
            // this is the deliberate divergence.
            let next = if token.text == "^" { level } else { level + 1 };
            let right = self.expression(next)?;
            self.count()?;
            left = Node::binary(token.text, left, right);
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Box<Node>, PlotError> {
        let token = self.peek();
        if token.kind == TokenKind::Op && (token.text == "-" || token.text == "+" || token.text == "!") {
            self.index += 1;
            // Parsed at the `^` level, which makes unary minus bind *looser* than
            // exponentiation: `-x^2` is −(x²) and `-2^2` is −4.
            let operand = self.expression(POWER_PRECEDENCE)?;
            self.count()?;
            return Ok(Node::unary(token.text, operand));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Box<Node>, PlotError> {
        let token = self.peek();
        match token.kind {
            TokenKind::Number => {
                self.index += 1;
                Ok(Node::number(token.value))
            }
            TokenKind::Open => {
                self.index += 1;
                let inner = self.expression(1)?;
                if self.peek().kind != TokenKind::Close {
                    return Err(PlotError::new("expected ')'"));
                }
                self.index += 1;
                Ok(inner)
            }
            TokenKind::Name => {
                self.index += 1;
                self.name(token.text)
            }
            TokenKind::End => Err(PlotError::new("the expression ends too early")),
            TokenKind::Close => Err(PlotError::new("unmatched ')'")),
            _ => Err(unexpected(token.text)),
        }
    }

    fn name(&mut self, spelling: &str) -> Result<Box<Node>, PlotError> {
        if self.peek().kind == TokenKind::Open {
            if !contains(&FUNCTIONS, spelling) {
                return Err(unknown_function(spelling));
            }
            self.index += 1;
            let mut args = Vec::new();
            if self.peek().kind != TokenKind::Close {
                args.push(*self.expression(1)?);
                while self.peek().kind == TokenKind::Comma {
                    self.index += 1;
                    args.push(*self.expression(1)?);
                }
            }
            if self.peek().kind != TokenKind::Close {
                return Err(PlotError::new("expected ')'"));
            }
            self.index += 1;
            let wanted = arity(spelling);
            if args.len() != wanted {
                return Err(wrong_arity(spelling, wanted, args.len()));
            }
            return Ok(Node::call(spelling, args));
        }
        if spelling == "pi" || spelling == "e" {
            return Ok(Node::number(if spelling == "pi" { std::f64::consts::PI } else { std::f64::consts::E }));
        }
        if !self.variable.is_empty() && spelling == self.variable {
            return Ok(Node::variable(spelling));
        }
        if contains(&FUNCTIONS, spelling) {
            return Err(is_a_function(spelling));
        }
        Err(unknown_name(spelling))
    }

    fn expect_end(&self) -> Result<(), PlotError> {
        let token = self.peek();
        match token.kind {
            TokenKind::End => Ok(()),
            TokenKind::Close => Err(PlotError::new("unmatched ')'")),
            _ => Err(unexpected(token.text)),
        }
    }

    /// The current token. Tokenize always ends the list with `End` and nothing consumes it, so
    /// the index never runs past; `End` stands in should it ever.
    fn peek(&self) -> Token<'t> {
        self.tokens.get(self.index).copied().unwrap_or(Token { kind: TokenKind::End, text: "", value: 0.0 })
    }
}

// The parser's formatted messages, out of line: `format!` expands into its caller, and in an
// unoptimised build its temporaries would ride in every frame of the parser's recursion.

#[cold]
#[inline(never)]
fn unexpected(text: &str) -> PlotError {
    PlotError::new(format!("unexpected '{text}'"))
}

#[cold]
#[inline(never)]
fn unknown_function(spelling: &str) -> PlotError {
    PlotError::new(format!("unknown function '{spelling}'"))
}

#[cold]
#[inline(never)]
fn wrong_arity(spelling: &str, wanted: usize, got: usize) -> PlotError {
    let plural = if wanted == 1 { "" } else { "s" };
    PlotError::new(format!("{spelling} takes {wanted} argument{plural}, not {got}"))
}

/// U+2014 EM DASH, U+2026 HORIZONTAL ELLIPSIS — the message every port pins.
#[cold]
#[inline(never)]
fn is_a_function(spelling: &str) -> PlotError {
    PlotError::new(format!("{spelling} is a function \u{2014} write {spelling}(\u{2026})"))
}

#[cold]
#[inline(never)]
fn unknown_name(spelling: &str) -> PlotError {
    PlotError::new(format!("unknown name '{spelling}'"))
}

// MARK: - Evaluation

/// One step of the explicit-stack walk: visit a node's operands, or apply it to their values.
#[derive(Clone, Copy)]
enum Step<'n> {
    Visit(&'n Node),
    Apply(&'n Node),
}

/// The two stacks of an evaluation, kept between the samples of one series so a thousand
/// evaluations allocate them once.
pub(super) struct Scratch<'n> {
    work: Vec<Step<'n>>,
    values: Vec<f64>,
}

impl<'n> Scratch<'n> {
    pub(super) fn new() -> Self {
        Scratch { work: Vec::with_capacity(16), values: Vec::with_capacity(16) }
    }
}

/// Evaluate `node` with `variable` bound to `value`.
///
/// Every value is a double and evaluation never fails: a domain error is NaN or ±∞, which breaks
/// the curve where it happens rather than failing the block. Comparisons and the Boolean
/// operators yield 1.0 and 0.0 — the site's eval closure discards Booleans, so
/// `(x > 0) * sqrt(x)` draws nothing there.
pub fn evaluate(node: &Node, variable: &str, value: f64) -> f64 {
    evaluate_with(node, variable, value, &mut Scratch::new())
}

/// [`evaluate`], walking the tree with an explicit stack.
///
/// md.win recurses, and a left-deep chain `x+x+…` within the 4096-node budget recurses 4096 deep —
/// more than a 1 MB Windows thread reliably holds, so md.win probes the stack and answers
/// `expression too large` where the process would otherwise die. Safe Rust has no stack probe,
/// and a stack overflow aborts the whole app, so this port does not recurse at all: every legal
/// expression evaluates on any thread, exactly as the Swift, Kotlin and TypeScript siblings
/// evaluate it. The operations, their operands and so every result are md.win's; only the order
/// of the walk differs, which a pure evaluation cannot observe.
pub(super) fn evaluate_with<'n>(node: &'n Node, variable: &str, value: f64, scratch: &mut Scratch<'n>) -> f64 {
    let work = &mut scratch.work;
    let values = &mut scratch.values;
    work.clear();
    values.clear();
    work.push(Step::Visit(node));
    while let Some(step) = work.pop() {
        match step {
            Step::Visit(node) => match node.kind {
                NodeKind::Number => values.push(node.value),
                NodeKind::Variable => values.push(if node.text == variable { value } else { f64::NAN }),
                NodeKind::Unary => {
                    work.push(Step::Apply(node));
                    if let Some(operand) = node.left.as_deref() {
                        work.push(Step::Visit(operand));
                    }
                }
                NodeKind::Binary => {
                    work.push(Step::Apply(node));
                    if let Some(right) = node.right.as_deref() {
                        work.push(Step::Visit(right));
                    }
                    if let Some(left) = node.left.as_deref() {
                        work.push(Step::Visit(left));
                    }
                }
                NodeKind::Call => {
                    work.push(Step::Apply(node));
                    for argument in node.arguments.iter().rev() {
                        work.push(Step::Visit(argument));
                    }
                }
            },
            Step::Apply(node) => match node.kind {
                NodeKind::Unary => {
                    let operand = values.pop().unwrap_or(f64::NAN);
                    values.push(match node.text.as_str() {
                        "-" => -operand,
                        "+" => operand,
                        _ => {
                            if operand == 0.0 {
                                1.0
                            } else {
                                0.0
                            }
                        }
                    });
                }
                NodeKind::Binary => {
                    let right = values.pop().unwrap_or(f64::NAN);
                    let left = values.pop().unwrap_or(f64::NAN);
                    values.push(binary(&node.text, left, right));
                }
                NodeKind::Call => {
                    if node.arguments.len() == 2 {
                        let second = values.pop().unwrap_or(f64::NAN);
                        let first = values.pop().unwrap_or(f64::NAN);
                        values.push(match node.text.as_str() {
                            "atan2" => first.atan2(second),
                            "pow" => pow_ieee(first, second),
                            _ => first.hypot(second),
                        });
                    } else {
                        let first = values.pop().unwrap_or(f64::NAN);
                        values.push(unary(&node.text, first));
                    }
                }
                NodeKind::Number | NodeKind::Variable => {}
            },
        }
    }
    values.pop().unwrap_or(f64::NAN)
}

fn binary(op: &str, left: f64, right: f64) -> f64 {
    let truth = |condition: bool| if condition { 1.0 } else { 0.0 };
    match op {
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        // Real division: `5/2` is 2.5, `1/0` is ∞, `0/0` is NaN.
        "/" => left / right,
        // The truncated remainder — C's fmod, JavaScript's, Rust's and C#'s `%` on doubles.
        // Never an IEEE remainder, never a floor-mod.
        "%" => left % right,
        "^" => pow_ieee(left, right),
        "==" => truth(left == right),
        "!=" => truth(left != right),
        "<" => truth(left < right),
        "<=" => truth(left <= right),
        ">" => truth(left > right),
        ">=" => truth(left >= right),
        "&&" => truth(left != 0.0 && right != 0.0),
        _ => truth(left != 0.0 || right != 0.0),
    }
}

/// `pow` as IEEE 754-2019 §9.2 and C99 define it: `pow(1, y)` is 1 for every y including NaN, and
/// `pow(±1, ±∞)` is 1.
///
/// The C `pow` under Rust's `powf` already answers so; JavaScript and Java return NaN for all
/// five, so their ports wrap. The wrapper is unconditional: the five results are the contract,
/// not a property of whichever libm is underneath.
fn pow_ieee(b: f64, exponent: f64) -> f64 {
    if b == 1.0 {
        return 1.0;
    }
    if b == -1.0 && exponent.is_infinite() {
        return 1.0;
    }
    b.powf(exponent)
}

/// The one-argument roster. The transcendental functions come from the platform's libm through
/// std, as md.win's `Math.*` come from the C runtime: IEEE 754 recommends them without requiring
/// correct rounding, so their last bit is the platform's (the tests hold them to one ulp of the
/// oracle; every coordinate is written to two decimals, which an ulp cannot move).
fn unary(name: &str, v: f64) -> f64 {
    match name {
        "sin" => v.sin(),
        "cos" => v.cos(),
        "tan" => v.tan(),
        "asin" => v.asin(),
        "acos" => v.acos(),
        "atan" => v.atan(),
        "sinh" => v.sinh(),
        "cosh" => v.cosh(),
        "tanh" => v.tanh(),
        "asinh" => v.asinh(),
        "acosh" => v.acosh(),
        "atanh" => v.atanh(),
        "sqrt" => v.sqrt(),
        "cbrt" => v.cbrt(),
        "abs" => v.abs(),
        "exp" => v.exp(),
        // pow(2, v), raw, in every port — not exp2; base 2 never reaches the five pow_ieee cases.
        "exp2" => 2f64.powf(v),
        "ln" => v.ln(),
        "log2" => v.log2(),
        "log10" => v.log10(),
        // floor / ceil / round DRAW. On the site they produce nothing (its preprocessor rewrites
        // every roster name to `math::…` while evalexpr binds exactly these three bare). `round`
        // is ties-away-from-zero, as Rust's is — never .NET's banker's default.
        "floor" => v.floor(),
        "ceil" => v.ceil(),
        _ => round_ties_away(v),
    }
}
