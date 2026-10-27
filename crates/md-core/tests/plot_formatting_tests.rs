//! The hand-written number formatters against an independent oracle, and the plot suite's own
//! helpers against what they stand in for. Not a port of an md.win test: md.win's formatters are
//! pinned by the 100 recorded labels and a handful of literals; these sweep a few hundred
//! thousand doubles.

mod common;
#[path = "common/plot.rs"]
mod plot_common;

use md_core::plot;
use plot_common::*;

/// A deterministic xorshift64* stream, so a failure names a reproducible value.
struct Stream(u64);

impl Stream {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A finite double with a uniformly drawn bit pattern whose decimal exponent is in range.
    fn double_between(&mut self, low_exponent: i32, high_exponent: i32) -> f64 {
        loop {
            let v = f64::from_bits(self.next());
            if v.is_finite() && v != 0.0 {
                let e = v.abs().log10().floor() as i32;
                if (low_exponent..=high_exponent).contains(&e) {
                    return v;
                }
            }
            // Mostly rejected at the extremes: pull the exponent into range directly.
            let mantissa = self.next() & 0x000F_FFFF_FFFF_FFFF;
            let span = (high_exponent - low_exponent + 1) as u64;
            let decimal = low_exponent + (self.next() % span) as i32;
            let binary = ((f64::from(decimal) * std::f64::consts::LOG2_10) as i64 + 1023).clamp(1, 2046) as u64;
            let sign = self.next() & (1 << 63);
            let candidate = f64::from_bits(sign | (binary << 52) | mantissa);
            let e = candidate.abs().log10().floor() as i32;
            if (low_exponent..=high_exponent).contains(&e) {
                return candidate;
            }
        }
    }
}

/// Doubles a formatter is most likely to get wrong: exact decimal ties at one to three places,
/// their neighbours one ulp either side, and carries (…995, …9995).
fn tie_neighbourhood() -> Vec<f64> {
    let mut out = Vec::new();
    for whole in [0i64, 1, 2, 7, 9, 99, 307, 999, 12345] {
        for eighths in 0..8 {
            let tie = whole as f64 + eighths as f64 / 8.0;
            for v in [tie, tie.next_up(), tie.next_down(), -tie] {
                out.push(v);
            }
        }
        for thousandths in [5, 15, 25, 125, 995, 9995] {
            let near = whole as f64 + thousandths as f64 / if thousandths >= 1000 { 10000.0 } else { 1000.0 };
            for v in [near, near.next_up(), near.next_down(), -near] {
                out.push(v);
            }
        }
    }
    out
}

/// Below 10²⁵ and at a few fraction digits, md.win's 25-significant-digit algorithm decides every
/// rounding exactly as an exact formatter does — the claim `format.rs` rests on. Rust's own
/// `{:.N}` is exact and ties to even, so it is the oracle.
#[test]
fn format_fixed_agrees_with_rusts_exact_formatter_below_ten_to_the_twenty_two() {
    let mut stream = Stream(0x9E37_79B9_7F4A_7C15);
    let mut values = tie_neighbourhood();
    for _ in 0..60_000 {
        values.push(stream.double_between(-12, 21));
    }
    for v in values {
        for digits in 0..=3 {
            assert_eq!(format!("{v:.digits$}"), plot::format_fixed(v, digits), "{v:e} at {digits}");
        }
    }
}

/// `{:.1e}` needs two significant digits, which 25 decide at every magnitude, subnormals
/// included.
#[test]
fn format_exponential_agrees_with_rusts_exact_formatter_everywhere() {
    let mut stream = Stream(0xD1B5_4A32_D192_ED03);
    let mut values = tie_neighbourhood();
    values.extend([5e-324, 1e-323, f64::MIN_POSITIVE, f64::MAX, 1250.0, 1350.0, 0.0125, 9.95, 9.95e-300, 99.5]);
    for _ in 0..60_000 {
        values.push(f64::from_bits(stream.next()));
    }
    for v in values {
        if !v.is_finite() {
            continue;
        }
        for digits in 0..=3 {
            assert_eq!(format!("{v:.digits$e}"), plot::format_exponential(v, digits), "{v:e} at {digits}");
        }
    }
}

/// Past 25 significant digits the formatter stops where every sibling stops, and Rust's own does
/// not — the reason the port does not simply call `format!`.
#[test]
fn format_fixed_stops_at_twenty_five_significant_digits_as_every_port_does() {
    assert_eq!("1000000000000000019884625000000.00", plot::format_fixed(1e30, 2));
    assert_eq!("1000000000000000019884624838656.00", format!("{:.2}", 1e30));
    assert_eq!("0.100000000000000005551115100000", plot::format_fixed(0.1, 30));
}

/// The integer branch of the label rounds ties away from zero, like `f64::round`.
#[test]
fn format_label_rounds_its_integer_branch_ties_away() {
    assert_eq!("3", plot::format_label(2.9999999999));
    assert_eq!("-3", plot::format_label(-2.9999999999));
    assert_eq!("0.50", plot::format_label(0.5));
    assert_eq!("NaN", plot::format_label(f64::NAN));
    assert_eq!("inf", plot::format_label(f64::INFINITY));
    assert_eq!("1.0e-2", plot::format_label(0.00999));
}

// MARK: - The suite's helpers

/// `double.ToString("R")` on .NET Core 3.0+, for the spellings the suite meets and the layout
/// boundaries.
#[test]
fn dotnet_r_writes_what_dotnet_writes() {
    for (v, want) in [
        (0.0, "0"),
        (-0.0, "-0"),
        (1.0, "1"),
        (-60.0, "-60"),
        (0.1, "0.1"),
        (1.0 / 3.0, "0.3333333333333333"),
        (2.5, "2.5"),
        (123.456, "123.456"),
        (0.0001, "0.0001"),
        (0.00001, "1E-05"),
        (1.5e-7, "1.5E-07"),
        (1e14, "100000000000000"),
        (1e15, "1E+15"),
        (1e16, "1E+16"),
        (1234567890123456.0, "1234567890123456"),
        (9.223372036854776e18, "9.223372036854776E+18"),
        (0.7999999999999998, "0.7999999999999998"),
        (9.999999999999999e-05, "9.999999999999999E-05"),
        (f64::MAX, "1.7976931348623157E+308"),
        (5e-324, "5E-324"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        assert_eq!(want, dotnet_r(v), "{v:e}");
    }
}

#[test]
fn dotnet_r_round_trips() {
    let mut stream = Stream(42);
    for _ in 0..20_000 {
        let v = f64::from_bits(stream.next());
        if v.is_finite() {
            assert_eq!(v.to_bits(), literal(&dotnet_r(v)).to_bits(), "{v:e}");
        }
    }
}

#[test]
fn dotnet_try_parse_takes_the_float_style_and_nothing_else() {
    assert_eq!(Some(40.0), dotnet_try_parse("40.0"));
    assert_eq!(Some(-0.5), dotnet_try_parse(" -.5\t"));
    assert_eq!(Some(1e-5), dotnet_try_parse("1E-05"));
    assert!(dotnet_try_parse("NaN").is_some_and(f64::is_nan));
    assert_eq!(Some(f64::INFINITY), dotnet_try_parse("Infinity"));
    assert_eq!(None, dotnet_try_parse("inf"));
    assert_eq!(None, dotnet_try_parse("nan"));
    assert_eq!(None, dotnet_try_parse("."));
    assert_eq!(None, dotnet_try_parse("0x10"));
}

/// The JSON reader is exact where serde_json's default is not: these are the two literals of
/// `plot-vectors.json` serde_json reads one ulp off (the first as 1000).
#[test]
fn the_json_reader_keeps_numbers_exact() {
    let parsed = json::parse(r#"{"a": 999.9999999999999, "b": [1.5430806348152437, "xé😀", null, true]}"#);
    assert_eq!("408f3fffffffffff", bits(parsed.get("a").f64()));
    let items = parsed.get("b").array();
    assert_eq!("3ff8b07551d9f550", bits(items[0].f64()));
    assert_eq!("x\u{e9}\u{1F600}", items[1].str());
    assert!(items[2].is_null());
    assert_eq!(json::Json::Bool(true), items[3]);
}

#[test]
fn the_xml_reader_refuses_what_xdocument_refuses() {
    assert!(xml::parse("<a>&amp;&lt;&#65;&#x42;</a>").is_ok_and(|root| root.value() == "&<AB"));
    assert!(xml::parse("<a>&</a>").is_err());
    assert!(xml::parse("<a>&nbsp;</a>").is_err());
    assert!(xml::parse("<a><b></a>").is_err());
    assert!(xml::parse("<a x=\"<\"/>").is_err());
    assert!(xml::parse("<a x=\"1\" x=\"2\"/>").is_err());
    assert!(xml::parse("<a/><b/>").is_err());
    assert!(xml::parse("<a>\u{0001}</a>").is_err());
    let root = xml::parse("<div><svg xmlns=\"n\"><title>t</title></svg><pre>p</pre></div>").expect("well-formed");
    assert_eq!(1, root.descendants("n", "title").len());
    assert_eq!(0, root.descendants("", "title").len());
    assert_eq!("p", root.element("", "pre").expect("pre").value());
}

#[test]
fn the_hand_written_patterns_match_as_the_regexes_do() {
    assert_eq!(vec!["1,2 3,4", "5,6"], runs("<polyline points=\"1,2 3,4\"/><polyline points=\"5,6\"/>"));
    assert_eq!(vec!["a"], x_labels("text-anchor=\"middle\">a<b>c</text> text-anchor=\"middle\">a</text>"));
    assert_eq!(vec!["&amp;", "&lt;"], entities("&amp; & &; &A; &lt;"));
    assert_eq!(
        vec!["1", "2", "3"],
        numeric_attributes("<line x1=\"1\" y=\"2\" xx=\"9\" cy=\"3\" stroke-width=\"4\"/>")
    );
    assert_eq!(
        Some((
            "<rect x=\"1.5\" y=\"2\" width=\"3\" height=\"4.0\"".to_string(),
            ["1.5".to_string(), "2".to_string(), "3".to_string(), "4.0".to_string()]
        )),
        rect("<rect x=\"a\"/><rect x=\"1.5\" y=\"2\" width=\"3\" height=\"4.0\" fill=\"none\"/>")
    );
    assert_eq!("", rect_value("<rect x=\"1\" y=\"\" width=\"3\" height=\"4\""));
    assert_eq!(vec!["sin", "x", "e3", "_a1"], identifiers("sin(x)+1e3*_a1"));
}
