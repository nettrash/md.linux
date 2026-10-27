//! Drawing and the axes: samples in, one `<svg>` string out.

use super::expression::{Scratch, evaluate_with};
use super::format::{format_exponential, format_fixed, round_ties_away};
use super::strings::{count_characters, is_finite_number};
use super::{Legend, Series, SeriesKind, Spec};

// MARK: - Drawing

/// The palette, purple first so a one-series plot matches the site's colour.
const PALETTE: [&str; 8] = ["#673AB7", "#E5390F", "#0F9D58", "#F4B400", "#00838F", "#C2185B", "#5D4037", "#455A64"];

/// The site's margin, and the room each extra asks for beyond it.
const MARGIN: f64 = 40.0;
const TITLE_ROOM: f64 = 24.0;
const AXIS_LABEL_ROOM: f64 = 18.0;
const LEGEND_MINIMUM: f64 = 72.0;
const LEGEND_PADDING: f64 = 28.0;
const LEGEND_FONT: f64 = 11.0;
/// Never let the extras eat the figure: the plot area keeps at least this.
const MIN_PLOT: f64 = 40.0;

#[derive(Clone, Copy)]
struct Sample {
    x: f64,
    y: f64,
}

pub(super) fn draw(spec: &Spec) -> String {
    // Sample first: `y: auto` fits the range to what the series produce, so the samples must
    // exist before the geometry does. They are reused for the drawing pass — sampling twice would
    // be one more chance to disagree.
    let sampled: Vec<Vec<Sample>> = spec.series.iter().map(|series| sample_series(series, spec)).collect();

    let (y_min, y_max) = resolve_y(spec, &sampled);

    let labels: Vec<&str> =
        spec.series.iter().map(|series| series.label.as_deref().unwrap_or(&series.source)).collect();
    let show_legend = spec.legend == Legend::On
        || (spec.legend == Legend::Auto && (spec.series.len() >= 2 || has_explicit_label(&spec.series)));

    let width = spec.width;
    let height = spec.height;

    let mut legend_width = 0.0;
    if show_legend {
        let mut longest = 0.0;
        for label in &labels {
            longest = math_max(longest, text_width(label, LEGEND_FONT));
        }
        legend_width = math_max(LEGEND_MINIMUM, longest.ceil() + LEGEND_PADDING);
        legend_width = math_min(legend_width, math_max(0.0, f64::from(width) - 2.0 * MARGIN - MIN_PLOT));
        if legend_width < LEGEND_MINIMUM / 2.0 {
            legend_width = 0.0;
        }
    }

    let horizontal = fit_margins(
        f64::from(width),
        MARGIN + if spec.y_label.is_empty() { 0.0 } else { AXIS_LABEL_ROOM },
        MARGIN + legend_width,
    );
    let vertical = fit_margins(
        f64::from(height),
        MARGIN + if spec.title.is_empty() { 0.0 } else { TITLE_ROOM },
        MARGIN + if spec.x_label.is_empty() { 0.0 } else { AXIS_LABEL_ROOM },
    );
    let left = horizontal.0;
    let top = vertical.0;
    let plot_w = f64::from(width) - left - horizontal.1;
    let plot_h = f64::from(height) - top - vertical.1;

    let x_min = spec.x_min;
    let x_max = spec.x_max;
    // Exactly this association — divide first, then multiply by the plot size. It is what makes
    // the two-decimal coordinates match the other ports.
    let sx = |x: f64| left + ((x - x_min) / (x_max - x_min)) * plot_w;
    let sy = |y: f64| top + ((y_max - y) / (y_max - y_min)) * plot_h;

    let x_step = nice_step(x_max - x_min);
    let y_step = nice_step(y_max - y_min);
    let x_ticks = ticks(x_min, x_max, x_step);
    let y_ticks = ticks(y_min, y_max, y_step);

    let w = width.to_string();
    let h = height.to_string();
    let mut out = String::with_capacity(20000);
    out.push_str("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 ");
    out.push_str(&w);
    out.push(' ');
    out.push_str(&h);
    out.push('"');
    out.push_str(" width=\"");
    out.push_str(&w);
    out.push_str("\" height=\"");
    out.push_str(&h);
    out.push_str("\" role=\"img\">");
    // The accessible name. No `id` anywhere in this SVG: a document may hold two plots and a
    // duplicated id is how one figure wears another's clip path. An <svg role="img"> is named by
    // its own <title>.
    out.push_str("<title>");
    out.push_str(&escape_html(&accessible_name(spec, &labels)));
    out.push_str("</title>");

    // Ink is `currentColor` with opacity throughout — never the site's greys. The renderer is
    // theme-blind in every port, so one SVG has to be right in light, dark, print, an exported
    // page and a saved file. No <style> either: SVG <style> inside an HTML document is
    // document-scoped and leaks.
    if spec.grid {
        out.push_str("<g stroke=\"currentColor\" stroke-width=\"0.5\" opacity=\"0.15\">");
        for &tick in &x_ticks {
            let at = fixed(sx(tick), 1);
            vertical_line(&mut out, &at, &fixed(top, 1), &fixed(top + plot_h, 1));
        }
        for &tick in &y_ticks {
            let at = fixed(sy(tick), 1);
            horizontal_line(&mut out, &fixed(left, 1), &at, &fixed(left + plot_w, 1));
        }
        out.push_str("</g>");
    }

    if spec.axes {
        out.push_str("<g stroke=\"currentColor\" stroke-width=\"1\" opacity=\"0.4\">");
        if y_min <= 0.0 && y_max >= 0.0 {
            let at = fixed(sy(0.0), 1);
            horizontal_line(&mut out, &fixed(left, 1), &at, &fixed(left + plot_w, 1));
        }
        if x_min <= 0.0 && x_max >= 0.0 {
            let at = fixed(sx(0.0), 1);
            vertical_line(&mut out, &at, &fixed(top, 1), &fixed(top + plot_h, 1));
        }
        out.push_str("</g>");

        out.push_str("<g font-size=\"10\" fill=\"currentColor\" opacity=\"0.65\" font-family=\"sans-serif\">");
        for &tick in &x_ticks {
            out.push_str("<text x=\"");
            out.push_str(&fixed(sx(tick), 1));
            out.push_str("\" y=\"");
            out.push_str(&fixed(top + plot_h + 15.0, 1));
            out.push_str("\" text-anchor=\"middle\">");
            out.push_str(&escape_html(&format_label(tick)));
            out.push_str("</text>");
        }
        for &tick in &y_ticks {
            out.push_str("<text x=\"");
            out.push_str(&fixed(left - 5.0, 1));
            out.push_str("\" y=\"");
            out.push_str(&fixed(sy(tick), 1));
            out.push_str("\" text-anchor=\"end\" dominant-baseline=\"middle\">");
            out.push_str(&escape_html(&format_label(tick)));
            out.push_str("</text>");
        }
        out.push_str("</g>");

        out.push_str("<rect x=\"");
        out.push_str(&fixed(left, 1));
        out.push_str("\" y=\"");
        out.push_str(&fixed(top, 1));
        out.push_str("\" width=\"");
        out.push_str(&fixed(plot_w, 1));
        out.push_str("\" height=\"");
        out.push_str(&fixed(plot_h, 1));
        out.push_str("\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\" opacity=\"0.25\"/>");
    }

    if !spec.title.is_empty() {
        // Centred on the plot area, not the canvas: the xlabel below is, and a legend gutter
        // would otherwise push the two out of line.
        out.push_str("<text x=\"");
        out.push_str(&fixed(left + plot_w / 2.0, 1));
        out.push_str("\" y=\"");
        out.push_str(&fixed(top - 14.0, 1));
        out.push_str(
            "\" text-anchor=\"middle\" font-size=\"13\" font-weight=\"600\" fill=\"currentColor\" opacity=\"0.85\" ",
        );
        out.push_str("font-family=\"sans-serif\">");
        out.push_str(&escape_html(&spec.title));
        out.push_str("</text>");
    }
    if !spec.x_label.is_empty() {
        out.push_str("<text x=\"");
        out.push_str(&fixed(left + plot_w / 2.0, 1));
        out.push_str("\" y=\"");
        out.push_str(&fixed(f64::from(height - 8), 1));
        out.push_str(
            "\" text-anchor=\"middle\" font-size=\"11\" fill=\"currentColor\" opacity=\"0.85\" font-family=\"sans-serif\">",
        );
        out.push_str(&escape_html(&spec.x_label));
        out.push_str("</text>");
    }
    if !spec.y_label.is_empty() {
        let x = fixed(14.0, 1);
        let y = fixed(top + plot_h / 2.0, 1);
        out.push_str("<text x=\"");
        out.push_str(&x);
        out.push_str("\" y=\"");
        out.push_str(&y);
        out.push_str("\" text-anchor=\"middle\" transform=\"rotate(-90 ");
        out.push_str(&x);
        out.push(' ');
        out.push_str(&y);
        out.push_str(")\" ");
        out.push_str("font-size=\"11\" fill=\"currentColor\" opacity=\"0.85\" font-family=\"sans-serif\">");
        out.push_str(&escape_html(&spec.y_label));
        out.push_str("</text>");
    }

    for (index, series) in spec.series.iter().enumerate() {
        let window = Window { x_min, x_max, y_min, y_max };
        polylines(&mut out, &sampled[index], series, colour(index), &sx, &sy, window);
    }

    if legend_width > 0.0 {
        let x = f64::from(width) - horizontal.1 + 8.0;
        out.push_str("<g font-size=\"11\" font-family=\"sans-serif\">");
        for (index, label) in labels.iter().enumerate() {
            let y = top + 12.0 + index as f64 * 16.0;
            out.push_str("<line x1=\"");
            out.push_str(&fixed(x, 1));
            out.push_str("\" y1=\"");
            out.push_str(&fixed(y - 4.0, 1));
            out.push_str("\" x2=\"");
            out.push_str(&fixed(x + 14.0, 1));
            out.push_str("\" y2=\"");
            out.push_str(&fixed(y - 4.0, 1));
            out.push_str("\" stroke=\"");
            out.push_str(colour(index));
            out.push_str("\" stroke-width=\"2\"/>");
            out.push_str("<text x=\"");
            out.push_str(&fixed(x + 20.0, 1));
            out.push_str("\" y=\"");
            out.push_str(&fixed(y, 1));
            out.push_str("\" fill=\"currentColor\" opacity=\"0.85\">");
            out.push_str(&escape_html(label));
            out.push_str("</text>");
        }
        out.push_str("</g>");
    }

    out.push_str("</svg>");
    out
}

/// `<line x1=at y1=low x2=at y2=high/>` — a grid line or an axis at one x.
fn vertical_line(out: &mut String, at: &str, low: &str, high: &str) {
    out.push_str("<line x1=\"");
    out.push_str(at);
    out.push_str("\" y1=\"");
    out.push_str(low);
    out.push_str("\" x2=\"");
    out.push_str(at);
    out.push_str("\" y2=\"");
    out.push_str(high);
    out.push_str("\"/>");
}

/// `<line x1=low y1=at x2=high y2=at/>` — a grid line or an axis at one y.
fn horizontal_line(out: &mut String, low: &str, at: &str, high: &str) {
    out.push_str("<line x1=\"");
    out.push_str(low);
    out.push_str("\" y1=\"");
    out.push_str(at);
    out.push_str("\" x2=\"");
    out.push_str(high);
    out.push_str("\" y2=\"");
    out.push_str(at);
    out.push_str("\"/>");
}

/// The data window a point must fall in to be drawn.
#[derive(Clone, Copy)]
struct Window {
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
}

/// The polylines of one series. A point is drawable when it is finite and inside the window
/// (tested on the raw values, inclusive); anything else ends the run and the next drawable point
/// starts a new one — which is what makes `tan(x)` seven branches. A run of one point is still
/// emitted.
fn polylines(
    out: &mut String,
    samples: &[Sample],
    series: &Series,
    stroke: &str,
    sx: &dyn Fn(f64) -> f64,
    sy: &dyn Fn(f64) -> f64,
    window: Window,
) {
    let mut run = String::new();
    let mut started = false;
    let mut marks: Vec<String> = Vec::new();
    for point in samples {
        let drawable = is_finite_number(point.x)
            && is_finite_number(point.y)
            && point.x >= window.x_min
            && point.x <= window.x_max
            && point.y >= window.y_min
            && point.y <= window.y_max;
        if drawable {
            let px = fixed(sx(point.x), 2);
            let py = fixed(sy(point.y), 2);
            if started {
                run.push(' ');
            }
            run.push_str(&px);
            run.push(',');
            run.push_str(&py);
            started = true;
            if series.kind == SeriesKind::Points {
                marks.push(format!("<circle cx=\"{px}\" cy=\"{py}\" r=\"2.5\" fill=\"{stroke}\"/>"));
            }
        } else if started {
            emit_polyline(out, &run, stroke);
            run.clear();
            started = false;
        }
    }
    if !run.is_empty() {
        emit_polyline(out, &run, stroke);
    }
    for mark in &marks {
        out.push_str(mark);
    }
}

fn emit_polyline(out: &mut String, run: &str, stroke: &str) {
    out.push_str("<polyline points=\"");
    out.push_str(run);
    out.push_str("\" fill=\"none\" stroke=\"");
    out.push_str(stroke);
    out.push_str("\" stroke-width=\"2\"/>");
}

/// The samples of one series: `x_i = xMin + (i / samples) * (xMax - xMin)` for i in 0…samples
/// inclusive — that expression and not the cheaper `xMin + i * dx`, which differs in the last
/// bits for 221 of the 1001 default samples and does change the output.
fn sample_series(series: &Series, spec: &Spec) -> Vec<Sample> {
    let samples = spec.samples;
    let mut out = Vec::with_capacity(usize::try_from(samples).unwrap_or(0) + 1);
    if series.kind == SeriesKind::Points {
        out.extend(series.points.iter().map(|point| Sample { x: point.x, y: point.y }));
        return out;
    }
    let mut scratch = Scratch::new();
    if series.kind == SeriesKind::Parametric {
        let t_span = series.t_max - series.t_min;
        for i in 0..=samples {
            let t = series.t_min + (f64::from(i) / f64::from(samples)) * t_span;
            let x =
                series.expression.as_ref().map_or(f64::NAN, |e| evaluate_with(e, &series.parameter, t, &mut scratch));
            let y =
                series.y_expression.as_ref().map_or(f64::NAN, |e| evaluate_with(e, &series.parameter, t, &mut scratch));
            out.push(Sample { x, y });
        }
        return out;
    }
    let span = spec.x_max - spec.x_min;
    for i in 0..=samples {
        let x = spec.x_min + (f64::from(i) / f64::from(samples)) * span;
        let y = series.expression.as_ref().map_or(f64::NAN, |e| evaluate_with(e, "x", x, &mut scratch));
        out.push(Sample { x, y });
    }
    out
}

/// `y: auto` — fit the finite samples, then pad 5 % on each side. Nothing finite at all falls
/// back to −1…1; a flat series is padded by 5 % of its own value, or by 1 when that is zero too.
fn resolve_y(spec: &Spec, sampled: &[Vec<Sample>]) -> (f64, f64) {
    if let (Some(fixed_low), Some(fixed_high)) = (spec.y_min, spec.y_max) {
        return (fixed_low, fixed_high);
    }
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for samples in sampled {
        for point in samples {
            if !is_finite_number(point.y) {
                continue;
            }
            if point.y < low {
                low = point.y;
            }
            if point.y > high {
                high = point.y;
            }
        }
    }
    if !is_finite_number(low) || !is_finite_number(high) {
        return (-1.0, 1.0);
    }
    let span = high - low;
    if span > 0.0 {
        return padded(low - span * 0.05, high + span * 0.05, low, high);
    }
    let padding = high.abs() * 0.05;
    let pad = if padding > 0.0 { padding } else { 1.0 };
    padded(low - pad, high + pad, low, high)
}

/// The padded range, or the unpadded one when padding overflowed to infinity (`high - low`
/// overflows for a series straddling ~1e308) — otherwise every `sy()` is NaN and the curve
/// silently vanishes.
fn padded(min: f64, max: f64, low: f64, high: f64) -> (f64, f64) {
    if is_finite_number(min) && is_finite_number(max) && max > min {
        return bounded(min, max);
    }
    if high > low {
        return bounded(low, high);
    }
    bounded(low - 1.0, high + 1.0)
}

/// A range whose *width* is finite, not merely whose ends are. Only an overflowing width needs
/// help; [1.69e308, 1.71e308] is left exactly as asked.
fn bounded(min: f64, max: f64) -> (f64, f64) {
    if !(max > min) {
        return (-1.0, 1.0);
    }
    if is_finite_number(max - min) {
        return (min, max);
    }
    (min / 2.0, max / 2.0)
}

fn accessible_name(spec: &Spec, labels: &[&str]) -> String {
    if !spec.title.is_empty() {
        return spec.title.clone();
    }
    if labels.is_empty() {
        return "Plot".to_string();
    }
    format!("Plot of {}", labels.join(", "))
}

fn has_explicit_label(series: &[Series]) -> bool {
    series.iter().any(|one| one.label.is_some())
}

fn colour(index: usize) -> &'static str {
    PALETTE[index % PALETTE.len()]
}

/// An estimate of a string's width at `size` pixels, for the legend gutter only: 0.6 em per code
/// point, no font metrics.
fn text_width(text: &str, size: f64) -> f64 {
    count_characters(text) as f64 * size * 0.6
}

/// Two margins that leave at least [`MIN_PLOT`] between them; without this a 160 × 120 figure
/// with a title, both labels and a legend draws inside out.
fn fit_margins(total: f64, low: f64, high: f64) -> (f64, f64) {
    if total - low - high >= MIN_PLOT {
        return (low, high);
    }
    let room = math_max(0.0, total - MIN_PLOT);
    let sum = low + high;
    if sum <= 0.0 {
        return (0.0, 0.0);
    }
    let scaled = ((room * low) / sum).floor();
    (scaled, room - scaled)
}

/// .NET's `Math.Max(double, double)`, which md.win calls: NaN if the first argument is NaN, the
/// second when only it is (`val2 < val1` is false), and +0 over −0. Not `f64::max`, which returns
/// the operand that is not NaN. No input here is NaN; the port keeps md.win's answer anyway.
fn math_max(val1: f64, val2: f64) -> f64 {
    if val1 != val2 {
        if !val1.is_nan() {
            return if val2 < val1 { val1 } else { val2 };
        }
        return val1;
    }
    if val2.is_sign_negative() { val1 } else { val2 }
}

/// .NET's `Math.Min(double, double)`: the mirror of [`math_max`], −0 under +0.
fn math_min(val1: f64, val2: f64) -> f64 {
    if val1 != val2 {
        if !val1.is_nan() {
            return if val1 < val2 { val1 } else { val2 };
        }
        return val1;
    }
    if val1.is_sign_negative() { val1 } else { val2 }
}

// MARK: - Axes

/// A decade — 10 raised to `exponent` — read from a decimal literal, never from `pow`.
///
/// `pow(10, e)` is not correctly rounded and differs by runtime version: for rough =
/// 9.999999999999999e-05 it is one ULP low under Node 20 and OpenJDK 21, which moved a tick step
/// and turned md.vscode's CI red. Parsing a decimal literal is specified to be correctly rounded
/// in every port's language (Rust's `str::parse` included). The three non-finite answers are what
/// `pow` gave (the oracle records them), and the guards are also what keeps the `as i32` cast
/// meaningful.
pub fn decade(exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent >= 309.0 {
        return f64::INFINITY; // 1e309 overflows a double
    }
    if exponent <= -324.0 {
        return 0.0; // 1e-324 underflows to zero
    }
    // `(int)exponent` in md.win truncates toward zero, as `as i32` does.
    format!("1e{}", exponent as i32).parse::<f64>().unwrap_or(f64::NAN)
}

/// The site's tick spacing, ported exactly — except that the decade comes from [`decade`] and
/// the exponent is pinned by exact comparison rather than trusted from `log10`, which is not
/// correctly rounded either (`floor(log10(9.999999999999999e-05))` is −4, one decade high).
pub fn nice_step(range: f64) -> f64 {
    nice_step_with(range, f64::log10)
}

/// [`nice_step`] with an injectable `log10` — the tests perturb it by ±1 decade and demand every
/// oracle row unchanged, which is the family's guard that the pin, not the platform, decides the
/// step. (Internal in md.win; public here because Rust's integration tests see only the public
/// surface.)
pub fn nice_step_with(range: f64, log10: impl Fn(f64) -> f64) -> f64 {
    let rough = range / 8.0;
    let mut exponent = log10(rough).floor();
    if decade(exponent) > rough {
        exponent -= 1.0;
    } else if decade(exponent + 1.0) <= rough {
        exponent += 1.0;
    }
    let magnitude = decade(exponent);
    let normalised = rough / magnitude;
    let mut step = 10.0;
    if normalised <= 1.5 {
        step = 1.0;
    } else if normalised <= 3.0 {
        step = 2.0;
    } else if normalised <= 7.0 {
        step = 5.0;
    }
    step * magnitude
}

/// The pinned decade of `rough` alone — Kotlin's `decadeOf`, for the tests' witness.
pub fn decade_of(rough: f64) -> f64 {
    let mut exponent = rough.log10().floor();
    if decade(exponent) > rough {
        exponent -= 1.0;
    } else if decade(exponent + 1.0) <= rough {
        exponent += 1.0;
    }
    decade(exponent)
}

/// How far past `max` a tick may land and still be drawn: one part in 10⁹ of a step.
const TICK_EPSILON: f64 = 1e-9;
/// A safety valve. [`nice_step`] gives eight to eleven ticks; a thousand is a bug.
const MAX_TICKS: usize = 1000;

/// The ticks of one axis, computed by index, never accumulated: `gx += step` reaches −5.55e−17 at
/// the origin and prints "-5.6e-17". The epsilon keeps a last tick that lands one ULP over the
/// maximum.
pub fn ticks(min: f64, max: f64, step: f64) -> Vec<f64> {
    let mut out = Vec::new();
    if !(step > 0.0) || !is_finite_number(step) {
        return out;
    }
    let first = (min / step).ceil() * step;
    let limit = max + step * TICK_EPSILON;
    for i in 0..MAX_TICKS {
        let tick = first + i as f64 * step;
        if tick > limit {
            break;
        }
        out.push(tick);
    }
    out
}

/// A tick's label — the site's `format_label` with its integer branch ROUNDED rather than
/// truncated (a tick at −4 arriving as −3.9999999999999996 printed "-3" there).
pub fn format_label(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    if v.is_nan() {
        return format_fixed(v, 2);
    }
    let magnitude = v.abs();
    if !(0.01..1000.0).contains(&magnitude) {
        return format_exponential(v, 1);
    }
    let rounded = round_ties_away(v);
    if (v - rounded).abs() < 1e-9 {
        return format_fixed(rounded, 0);
    }
    format_fixed(v, 2)
}

/// [`format_fixed`], for the coordinates the emitter writes.
fn fixed(v: f64, fraction_digits: usize) -> String {
    format_fixed(v, fraction_digits)
}

// MARK: - Markup

/// Four of the five predefined XML entities — `&amp;` `&lt;` `&gt;` `&quot;` — and never anything
/// else. Not `&apos;`: a bare `'` is well-formed in text and in the double-quoted attributes this
/// renderer writes, and the reference escapes exactly these four, which the byte-parity golden
/// depends on. Never a named entity like `&nbsp;`, undefined in XML — the EPUB body is XHTML.
pub(super) fn escape_html(s: &str) -> String {
    // The four are ASCII, so a per-char pass is exact — and a combining mark after `&` is a
    // character of its own, never part of the entity.
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_max_and_min_answer_as_dotnet_does() {
        assert_eq!(math_max(1.0, 2.0), 2.0);
        assert_eq!(math_max(2.0, 1.0), 2.0);
        assert!(math_max(f64::NAN, 1.0).is_nan());
        assert!(math_max(1.0, f64::NAN).is_nan());
        assert!(math_max(-0.0, 0.0).is_sign_positive());
        assert!(math_max(0.0, -0.0).is_sign_positive());
        assert_eq!(math_min(1.0, 2.0), 1.0);
        assert!(math_min(f64::NAN, 1.0).is_nan());
        assert!(math_min(1.0, f64::NAN).is_nan());
        assert!(math_min(0.0, -0.0).is_sign_negative());
        assert!(math_min(-0.0, 0.0).is_sign_negative());
    }

    #[test]
    fn fit_margins_scales_both_margins_into_the_room_left() {
        assert_eq!(fit_margins(600.0, 40.0, 40.0), (40.0, 40.0));
        // 120 high with a title and an x label: 64 + 58 leaves less than 40.
        assert_eq!(fit_margins(120.0, 64.0, 58.0), (41.0, 39.0));
        assert_eq!(fit_margins(30.0, 0.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn decade_parses_the_literal_and_truncates_a_fractional_exponent() {
        assert_eq!(decade(2.0), 100.0);
        assert_eq!(decade(2.7), 100.0);
        assert_eq!(decade(-2.7), 0.01);
        assert_eq!(decade(-323.0), 1e-323);
        assert_eq!(decade(308.0), 1e308);
    }

    #[test]
    fn escape_html_writes_four_entities_per_character() {
        assert_eq!(escape_html("a&\u{0301}<b>\"'"), "a&amp;\u{0301}&lt;b&gt;&quot;'");
    }
}
