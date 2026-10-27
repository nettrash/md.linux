//! The ```plot fence: source text in, an SVG string out. Port of md.win
//! `Md.Core.Markdown.Plot`, `PlotMemo` and `PlotInternals`.
//!
//! This module is pure and synchronous — no engine, no asset, no script, no platform API. That is
//! what lets the live preview, the self-contained HTML export, print, PDF, EPUB, LaTeX and
//! "export diagram as SVG" all receive a finished `<svg>` in the bytes the renderer already
//! returns. The one piece of state is [`PlotMemo`], a memo of a pure function.
//!
//! It is a transliteration of md.win's `Plot.cs`, itself one of md.vscode's
//! `src/render/plot.ts`, which md, md.macOS and md.Android are also transliterations of: same
//! function names, same order, hand-written scanning instead of regular expressions, plain
//! records instead of generics. Byte parity with the other ports is the contract — the golden
//! figure and the Rust-generated oracle pin it.
//!
//! WHERE IT DELIBERATELY DIVERGES FROM THE SITE'S PLOTTER
//! Four behaviours of nettrash.me's math.rs are bugs, fixed here: floor/ceil/round draw;
//! comparisons yield 1.0/0.0; `^` is right-associative; everything is a double (5/2 = 2.5). And
//! two in the axes: tick labels are rounded rather than truncated, and tick positions are computed
//! by index rather than accumulated.
//!
//! NUMBER FORMATTING IS THE CROSS-PLATFORM HAZARD
//! Rust's `{:.1e}` writes `1.0e3`; .NET's "E1" writes `1.2E+003`; ties differ too (1250 is `1.2e3`
//! in Rust, ties to even on the exact value). Every formatter in `format.rs` is hand-written from
//! the exact binary expansion, as md.win's are — not delegated to Rust's own, which agrees below
//! 25 significant digits and prints the whole expansion past them where every sibling stops.
//!
//! AND SO IS pow(10, e)
//! pow and log10 are not correctly rounded and differ by *engine version* (Node 20 is wrong on 68
//! of the 632 integer exponents, OpenJDK 21 on 64). Decades are parsed from decimal literals,
//! which every language specifies to be correctly rounded. Do not reintroduce `powi`/`powf(10, e)`.
//!
//! UTF-8, WHERE md.win IS UTF-16
//! Every grammar character is ASCII, so scanning bytes instead of md.win's code units changes
//! nothing — except the legend gutter, which counts code points, and the `unexpected character`
//! message, which names a whole code point. Both are spelt out where they happen.
//!
//! PlotInternals.cs only opens md.win's internals to its test assembly (a memo of the tests' own,
//! `NiceStep` with an injectable `log10`). Rust's integration tests see only the public surface,
//! so those hooks — [`render_plot_with_memo`], [`nice_step_with`], [`decade_of`] — are public.

// `!(a > b)` is deliberate wherever it appears: unlike `a <= b` it is true for NaN, which is how
// md.win refuses a NaN range or step.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

mod draw;
mod expression;
mod format;
mod memo;
mod strings;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::LazyLock;

pub use draw::{decade, decade_of, format_label, nice_step, nice_step_with, ticks};
pub use expression::{Node, NodeKind, evaluate, parse_expression};
pub use format::{format_exponential, format_fixed};
pub use memo::{PlotMemo, Statistics};

use draw::escape_html;
use format::round_ties_away;
use strings::{
    contains, index_of, index_of_pair, is_finite_number, is_identifier_part, is_identifier_start, is_lower_letter,
    is_space, lowercased, lowercased_char, matching_parenthesis, split_lines, split_whitespace, starts_with_word,
    top_level_comma, trim,
};

/// Everything that makes a block unrenderable, with the message the reader sees (md.win's
/// `PlotException`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlotError {
    message: String,
}

impl PlotError {
    pub fn new(message: impl Into<String>) -> Self {
        PlotError { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for PlotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PlotError {}

// MARK: - The entry point

/// One ```plot fence, as the block of markup the renderer embeds.
///
/// The container is emitted unconditionally — for a good plot, an empty block and a broken one
/// alike — because every export path counts `div.plot` containers to pair a figure with its
/// source block. A block that cannot be parsed keeps its source visible under one `plot: …` line.
/// Memoised on `source`; failures are memoised too, on purpose — a half-typed fence is a parse
/// error on most keystrokes.
pub fn render_plot(source: &str) -> String {
    render_plot_with_memo(source, &MEMO)
}

/// [`render_plot`] through a memo of the caller's own — the tests' hook (internal in md.win), so
/// the statistics they pin cannot be polluted by another test rendering a plot in parallel.
pub fn render_plot_with_memo(source: &str, memo: &PlotMemo) -> String {
    if let Some(memoised) = memo.value(source) {
        return memoised;
    }
    // Nothing may escape into the Markdown renderer, which has no other failure mode: md.win
    // catches every exception, and the Rust spelling of "every" is a caught panic. The port has
    // no known panic; this is the guarantee that a future one shows its source instead of
    // taking the preview down.
    let container = match catch_unwind(AssertUnwindSafe(|| plot_svg(source))) {
        Ok(Ok(svg)) => format!("<div class=\"plot\">{svg}</div>"),
        Ok(Err(error)) => fallback(error.message(), source),
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "internal error".to_string());
            fallback(&message, source)
        }
    };
    memo.store(&container, source);
    container
}

fn fallback(message: &str, source: &str) -> String {
    format!("<div class=\"plot\"><pre>{}</pre></div>", escape_html(&format!("plot: {message}\n{source}")))
}

static MEMO: LazyLock<PlotMemo> = LazyLock::new(|| PlotMemo::new(32));

/// Entries, hits and misses of the shared memo. Nothing in the app branches on these.
pub fn memo_statistics() -> Statistics {
    MEMO.snapshot()
}

/// Drop every memoised container. Only the tests need this.
pub fn clear_memo() {
    MEMO.remove_all();
}

/// The `<svg>` element for a fence, `""` for an empty block, or a [`PlotError`].
pub fn plot_svg(source: &str) -> Result<String, PlotError> {
    let spec = parse_plot(source)?;
    // A block with nothing in it draws nothing, like an empty Mermaid block. A block with
    // directives but no series is not empty: draw the empty axes those directives describe, so
    // the author's text does not vanish.
    if spec.series.is_empty() && !spec.has_directives {
        return Ok(String::new());
    }
    Ok(draw::draw(&spec))
}

// MARK: - The fence

/// A parsed ```plot block: the directives, resolved, and the series in order.
#[derive(Debug)]
pub struct Spec {
    pub x_min: f64,
    pub x_max: f64,
    /// `None` when `y: auto` (the default) — the range is fitted to the samples.
    pub y_min: Option<f64>,
    pub y_max: Option<f64>,
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub legend: Legend,
    pub grid: bool,
    pub axes: bool,
    pub width: i32,
    pub height: i32,
    pub samples: i32,
    pub series: Vec<Series>,
    /// Whether the fence carried at least one directive — what separates a genuinely empty block
    /// from one the author wrote into but gave no series.
    pub has_directives: bool,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            x_min: -10.0,
            x_max: 10.0,
            y_min: None,
            y_max: None,
            title: String::new(),
            x_label: String::new(),
            y_label: String::new(),
            legend: Legend::Auto,
            grid: true,
            axes: true,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            samples: DEFAULT_SAMPLES,
            series: Vec::new(),
            has_directives: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Legend {
    On,
    Off,
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeriesKind {
    Function,
    Parametric,
    Points,
}

/// One curve — three shapes in one flat record, which every port reads without a downcast.
#[derive(Debug)]
pub struct Series {
    pub kind: SeriesKind,
    /// The author's own label, or `None` — then the legend shows `source`.
    pub label: Option<String>,
    /// The source text of the series, verbatim, for the legend and for messages.
    pub source: String,
    /// `Function`: y = f(x). `Parametric`: the x half. `None` for `Points`.
    pub expression: Option<Node>,
    /// `Parametric` only: the y half.
    pub y_expression: Option<Node>,
    /// `Parametric` only: the parameter's name (`"x"` for a function, `""` for points).
    pub parameter: String,
    pub t_min: f64,
    pub t_max: f64,
    pub points: Vec<Point>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// The directive keys. Anything else before a colon is a series, not an error.
const DIRECTIVES: [&str; 11] =
    ["x", "y", "title", "xlabel", "ylabel", "legend", "grid", "axes", "width", "height", "samples"];

const DEFAULT_WIDTH: i32 = 600;
const DEFAULT_HEIGHT: i32 = 400;
const DEFAULT_SAMPLES: i32 = 1000;
const MIN_WIDTH: i32 = 160;
const MAX_WIDTH: i32 = 2000;
const MIN_HEIGHT: i32 = 120;
const MAX_HEIGHT: i32 = 2000;
const MIN_SAMPLES: i32 = 50;
const MAX_SAMPLES: i32 = 5000;
/// The most series one fence may draw — the one input an author sets just by adding lines, and
/// the preview re-renders the document on every keystroke.
const MAX_SERIES: usize = 24;

/// Read a fence into a [`Spec`].
///
/// Blank lines are ignored, a line whose first non-space character is `#` is a comment, and order
/// is free. A line is a directive when it reads `key: value` and the key is known, so `f: x`
/// still plots.
pub fn parse_plot(source: &str) -> Result<Spec, PlotError> {
    let mut spec = Spec::default();

    for raw in split_lines(source) {
        let line = trim(raw);
        if line.is_empty() {
            continue;
        }
        if line.as_bytes()[0] == b'#' {
            continue;
        }
        if let Some((key, value)) = match_directive(line) {
            apply_directive(&mut spec, key, value)?;
            spec.has_directives = true;
            continue;
        }
        if spec.series.len() == MAX_SERIES {
            return Err(PlotError::new(format!("too many series (limit {MAX_SERIES})")));
        }
        spec.series.push(parse_series(line)?);
    }

    if !(spec.x_max > spec.x_min) {
        return Err(PlotError::new("x range must be increasing"));
    }
    if let (Some(low), Some(high)) = (spec.y_min, spec.y_max)
        && !(high > low)
    {
        return Err(PlotError::new("y range must be increasing"));
    }
    Ok(spec)
}

/// `key: value`, whatever the key. Hand-scanned (the equivalent of `^\s*([a-z][a-z-]*)\s*:\s*(.*)$`
/// with the grammar's own whitespace), because the same scan has to exist in every port.
fn match_keyed(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() && is_space(bytes[index]) {
        index += 1;
    }
    let start = index;
    if index >= bytes.len() || !is_lower_letter(bytes[index]) {
        return None;
    }
    while index < bytes.len() && (is_lower_letter(bytes[index]) || bytes[index] == b'-') {
        index += 1;
    }
    let key = &line[start..index];
    while index < bytes.len() && is_space(bytes[index]) {
        index += 1;
    }
    if index >= bytes.len() || bytes[index] != b':' {
        return None;
    }
    Some((key, trim(&line[index + 1..])))
}

/// The same line, when the key is one this renderer knows — what keeps `f: x` from failing the
/// block.
fn match_directive(line: &str) -> Option<(&str, &str)> {
    let keyed = match_keyed(line)?;
    if !contains(&DIRECTIVES, keyed.0) {
        return None;
    }
    Some(keyed)
}

fn apply_directive(spec: &mut Spec, key: &str, value: &str) -> Result<(), PlotError> {
    if key == "x" {
        let (min, max) = parse_range(value, "x")?;
        spec.x_min = min;
        spec.x_max = max;
        return Ok(());
    }
    if key == "y" {
        if lowercased(value) == "auto" {
            spec.y_min = None;
            spec.y_max = None;
            return Ok(());
        }
        let (min, max) = parse_range(value, "y")?;
        spec.y_min = Some(min);
        spec.y_max = Some(max);
        return Ok(());
    }
    if key == "title" {
        spec.title = value.to_string();
        return Ok(());
    }
    if key == "xlabel" {
        spec.x_label = value.to_string();
        return Ok(());
    }
    if key == "ylabel" {
        spec.y_label = value.to_string();
        return Ok(());
    }
    if key == "legend" {
        spec.legend = match lowercased(value).as_str() {
            "on" => Legend::On,
            "off" => Legend::Off,
            "auto" => Legend::Auto,
            _ => return Err(PlotError::new("legend must be 'on', 'off' or 'auto'")),
        };
        return Ok(());
    }
    if key == "grid" || key == "axes" {
        let word = lowercased(value);
        if word != "on" && word != "off" {
            return Err(PlotError::new(format!("{key} must be 'on' or 'off'")));
        }
        if key == "grid" {
            spec.grid = word == "on";
        } else {
            spec.axes = word == "on";
        }
        return Ok(());
    }
    if key == "width" {
        spec.width = clamp_integer(number(value, "width")?, MIN_WIDTH, MAX_WIDTH);
        return Ok(());
    }
    if key == "height" {
        spec.height = clamp_integer(number(value, "height")?, MIN_HEIGHT, MAX_HEIGHT);
        return Ok(());
    }
    // `samples`, the only key left.
    spec.samples = clamp_integer(number(value, "samples")?, MIN_SAMPLES, MAX_SAMPLES);
    Ok(())
}

/// `A..B`. Both ends are constant expressions, so `x: -pi..pi` and `x: 0..2*pi` work; an end that
/// mentions a variable is an error rather than a silent zero. The first `..` wins.
fn parse_range(value: &str, key: &str) -> Result<(f64, f64), PlotError> {
    let Some(at) = index_of_pair(value, b'.', b'.') else {
        return Err(PlotError::new(format!("{key} range must be written min..max")));
    };
    let min = constant(&value[..at], key)?;
    let max = constant(&value[at + 2..], key)?;
    if !(max > min) {
        return Err(PlotError::new(format!("{key} range must be increasing")));
    }
    Ok((min, max))
}

/// A constant expression: no variable, finite.
fn constant(text: &str, key: &str) -> Result<f64, PlotError> {
    let trimmed = trim(text);
    if trimmed.is_empty() {
        return Err(PlotError::new(format!("{key} range must be written min..max")));
    }
    let value = evaluate(&parse_expression(trimmed, "")?, "", 0.0);
    if !is_finite_number(value) {
        return Err(PlotError::new(format!("{key} range must be finite")));
    }
    Ok(value)
}

fn number(value: &str, key: &str) -> Result<f64, PlotError> {
    let parsed = evaluate(&parse_expression(trim(value), "")?, "", 0.0);
    if !is_finite_number(parsed) {
        return Err(PlotError::new(format!("{key} must be a number")));
    }
    Ok(parsed)
}

/// A pixel count or a sample count: rounded to a whole number, then clamped.
fn clamp_integer(value: f64, low: i32, high: i32) -> i32 {
    let whole = round_ties_away(value);
    if whole < f64::from(low) {
        return low;
    }
    if whole > f64::from(high) {
        return high;
    }
    // `number` already refused anything non-finite, so the cast is exact here.
    whole as i32
}

/// One series line. The label is everything left of the first top-level `=` that is not part of
/// `==`, `<=`, `>=` or `!=`; the language has no assignment, so a bare `=` is always a label
/// separator.
fn parse_series(line: &str) -> Result<Series, PlotError> {
    let mut label: Option<String> = None;
    let mut body = line;
    if let Some(at) = label_separator(line) {
        let left = trim(&line[..at]);
        body = trim(&line[at + 1..]);
        if !left.is_empty() {
            label = Some(left.to_string());
        }
    } else if points_prefix(line).is_none() {
        // `f: x` — a `key:` line whose key is no directive of ours: the key is the label and the
        // rest is the series. `points:` is the one colon that means something else, and it is
        // claimed above.
        if let Some((key, value)) = match_keyed(line)
            && !value.is_empty()
        {
            label = Some(key.to_string());
            body = value;
        }
    }
    if body.is_empty() {
        return Err(PlotError::new("a series needs an expression"));
    }

    if let Some(points) = parse_points_series(label.as_deref(), body)? {
        return Ok(points);
    }
    if let Some(parametric) = parse_parametric_series(label.as_deref(), body)? {
        return Ok(parametric);
    }

    Ok(Series {
        kind: SeriesKind::Function,
        label,
        source: body.to_string(),
        expression: Some(parse_expression(body, "x")?),
        y_expression: None,
        parameter: "x".to_string(),
        t_min: 0.0,
        t_max: 0.0,
        points: Vec::new(),
    })
}

/// The index of the label `=`, or `None`.
fn label_separator(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 0i64;
    for (index, &c) in bytes.iter().enumerate() {
        if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
        } else if c == b'=' && depth == 0 {
            // `==` is a comparison, and `<=`, `>=`, `!=` end with one. The scan stops at the
            // first top-level `=` either way — it does not keep looking.
            if index + 1 < bytes.len() && bytes[index + 1] == b'=' {
                return None;
            }
            if index > 0 {
                let before = bytes[index - 1];
                if before == b'=' || before == b'<' || before == b'>' || before == b'!' {
                    return None;
                }
            }
            return Some(index);
        }
    }
    None
}

const POINTS_PREFIX: &str = "points:";

/// Does this line open a points series? Case-insensitive prefix. The byte length of the prefix
/// as written, or `None`.
///
/// md.win lowercases the first seven UTF-16 units and compares them with `points:`. A unit that
/// is half of a surrogate pair lowercases to itself, and no other character lowercases into this
/// ASCII word under .NET's invariant mapping (`İ` stays `İ`; the Kelvin sign's `k` is not in
/// it), so the question is exactly whether the first seven characters each lowercase to the
/// prefix's — and when they do, all seven are BMP and one unit each, and their bytes are where
/// the pairs begin.
fn points_prefix(line: &str) -> Option<usize> {
    let mut chars = line.char_indices();
    for wanted in POINTS_PREFIX.chars() {
        let (_, c) = chars.next()?;
        if lowercased_char(c) != wanted {
            return None;
        }
    }
    Some(chars.next().map_or(line.len(), |(at, _)| at))
}

/// `points: x,y x,y …`, or `None` when this is not a points series.
fn parse_points_series(label: Option<&str>, body: &str) -> Result<Option<Series>, PlotError> {
    let Some(prefix) = points_prefix(body) else {
        return Ok(None);
    };

    let mut points = Vec::new();
    for token in split_whitespace(&body[prefix..]) {
        let Some(comma) = index_of(token, b',') else {
            // U+2014 EM DASH, as in every port.
            return Err(PlotError::new(format!("points must be x,y pairs \u{2014} '{token}' is not one")));
        };
        points.push(Point { x: constant(&token[..comma], "point")?, y: constant(&token[comma + 1..], "point")? });
    }
    if points.is_empty() {
        return Err(PlotError::new("points needs at least one x,y pair"));
    }
    Ok(Some(Series {
        kind: SeriesKind::Points,
        label: label.map(str::to_string),
        source: body.to_string(),
        expression: None,
        y_expression: None,
        parameter: String::new(),
        t_min: 0.0,
        t_max: 0.0,
        points,
    }))
}

/// `(fx(t), fy(t)) for t in A..B`, or `None` when this is not a parametric series. Deliberately
/// narrow — an opening parenthesis whose match is followed by the word `for` — so `(x+1)*2` stays
/// a function of x.
fn parse_parametric_series(label: Option<&str>, body: &str) -> Result<Option<Series>, PlotError> {
    if body.as_bytes()[0] != b'(' {
        return Ok(None);
    }
    let Some(close) = matching_parenthesis(body, 0) else {
        return Ok(None);
    };
    let tail = trim(&body[close + 1..]);
    if !starts_with_word(tail, "for") {
        return Ok(None);
    }

    let inside = &body[1..close];
    let Some(comma) = top_level_comma(inside) else {
        return Err(PlotError::new("a parametric series needs (x(t), y(t))"));
    };

    // `for t in A..B`
    let rest = trim(&tail[3..]);
    let rest_bytes = rest.as_bytes();
    let mut index = 0;
    while index < rest_bytes.len() && is_identifier_part(rest_bytes[index]) {
        index += 1;
    }
    let parameter = &rest[..index];
    if index == 0 || !is_identifier_start(rest_bytes[0]) {
        return Err(PlotError::new("expected a parameter name after 'for'"));
    }
    let after_name = trim(&rest[index..]);
    if !starts_with_word(after_name, "in") {
        return Err(PlotError::new(format!("expected 'in' after '{parameter}'")));
    }
    let (t_min, t_max) = parse_range(trim(&after_name[2..]), parameter)?;

    Ok(Some(Series {
        kind: SeriesKind::Parametric,
        label: label.map(str::to_string),
        source: body.to_string(),
        expression: Some(parse_expression(trim(&inside[..comma]), parameter)?),
        y_expression: Some(parse_expression(trim(&inside[comma + 1..]), parameter)?),
        parameter: parameter.to_string(),
        t_min,
        t_max,
        points: Vec::new(),
    }))
}
