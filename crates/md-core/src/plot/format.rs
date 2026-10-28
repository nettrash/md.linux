//! Number formatting, by hand — the foot of md.win's `Plot.cs`.
//!
//! Every formatter here is hand-written from the exact binary expansion of the double. They
//! reproduce what the site's Rust prints (`{:.1e}`, `{:.2}`), which is also what md, md.macOS,
//! md.Android, md.vscode and md.win print, through one algorithm the five ports share: the first
//! 25 significant digits of the exact value, then ties to even on those digits. Never `{:.2}`
//! itself either: past 25 significant digits it prints the double's whole expansion where every
//! sibling stops, and a sibling's choice is the contract.

/// Significant digits taken from the exact binary value. Twenty-five decides every rounding this
/// module performs: a double is m/2^k and a decimal tie at the second digit is k/10^m, and two
/// such numbers that differ do so by at least ~10⁻¹⁸ relatively — far above the 10⁻²⁵ the last
/// digit resolves. So a digit string reading "…5000…0" here *is* an exact tie.
const SIGNIFICANT_DIGIT_COUNT: usize = 25;

/// One limb of [`Natural`]: nine decimal digits.
const LIMB: u64 = 1_000_000_000;

/// A natural number in base 10⁹, least significant limb first — exactly the arithmetic
/// `SignificantDigits` needs (m · 2^k, m · 5^n, then its decimal digits), standing in for .NET's
/// `BigInteger`.
struct Natural {
    limbs: Vec<u32>,
}

impl Natural {
    fn from_u64(mut value: u64) -> Self {
        let mut limbs = Vec::new();
        while value > 0 {
            limbs.push((value % LIMB) as u32);
            value /= LIMB;
        }
        if limbs.is_empty() {
            limbs.push(0);
        }
        Natural { limbs }
    }

    /// `self *= factor`. A limb is below 10⁹ and the factor below 2³², so the product and its
    /// carry fit a `u64` with room to spare.
    fn multiply(&mut self, factor: u32) {
        let mut carry = 0u64;
        for limb in &mut self.limbs {
            let product = u64::from(*limb) * u64::from(factor) + carry;
            *limb = (product % LIMB) as u32;
            carry = product / LIMB;
        }
        while carry > 0 {
            self.limbs.push((carry % LIMB) as u32);
            carry /= LIMB;
        }
    }

    /// `self *= 2^n`, thirty-one bits at a time.
    fn shift_left(&mut self, mut n: u32) {
        while n >= 31 {
            self.multiply(1 << 31);
            n -= 31;
        }
        if n > 0 {
            self.multiply(1 << n);
        }
    }

    /// `self *= 5^n`, thirteen fives at a time (5¹³ = 1 220 703 125 < 2³²).
    fn multiply_by_power_of_five(&mut self, mut n: u32) {
        const FIVE_TO_THE_13: u32 = 1_220_703_125;
        while n >= 13 {
            self.multiply(FIVE_TO_THE_13);
            n -= 13;
        }
        if n > 0 {
            self.multiply(5u32.pow(n));
        }
    }

    /// The decimal digits, most significant first, no leading zeros (`"0"` for zero).
    fn digits(&self) -> Vec<u8> {
        let mut top = self.limbs.len();
        while top > 1 && self.limbs[top - 1] == 0 {
            top -= 1;
        }
        let mut out = self.limbs[top - 1].to_string().into_bytes();
        for limb in self.limbs[..top - 1].iter().rev() {
            out.extend_from_slice(format!("{limb:09}").as_bytes());
        }
        out
    }
}

/// The first 25 significant decimal digits of `value` (> 0, finite) and the decimal exponent of
/// the first one.
///
/// Exact by construction, the way Kotlin's `BigDecimal(v)` is: the double is m × 2^k, so for
/// k < 0 its digits are those of m × 5^−k with −k decimal places. A tie at the 25th digit rounds
/// up, as Kotlin's HALF_UP does (Swift's printf rounds it to even); the argument on
/// [`SIGNIFICANT_DIGIT_COUNT`] is why no decision this module makes can see the difference.
fn significant_digits(value: f64) -> (Vec<u8>, i32) {
    let bits = value.to_bits();
    let biased = ((bits >> 52) & 0x7FF) as i32;
    let mut mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
    let mut k;
    if biased == 0 {
        k = -1074; // subnormal: no implicit bit
    } else {
        mantissa |= 1u64 << 52;
        k = biased - 1075;
    }
    // Shed trailing zero bits: it keeps 5^n small for ordinary values.
    while k < 0 && (mantissa & 1) == 0 {
        mantissa >>= 1;
        k += 1;
    }
    let mut number = Natural::from_u64(mantissa);
    let (digits, exponent) = if k >= 0 {
        number.shift_left(k as u32);
        let digits = number.digits();
        let exponent = digits.len() as i32 - 1;
        (digits, exponent)
    } else {
        let scale = -k;
        number.multiply_by_power_of_five(scale as u32);
        let digits = number.digits();
        let exponent = digits.len() as i32 - 1 - scale;
        (digits, exponent)
    };
    if digits.len() < SIGNIFICANT_DIGIT_COUNT {
        let mut padded = digits;
        padded.resize(SIGNIFICANT_DIGIT_COUNT, b'0');
        return (padded, exponent);
    }
    if digits.len() == SIGNIFICANT_DIGIT_COUNT {
        return (digits, exponent);
    }
    let kept = digits[..SIGNIFICANT_DIGIT_COUNT].to_vec();
    if digits[SIGNIFICANT_DIGIT_COUNT] < b'5' {
        return (kept, exponent);
    }
    let (carried, overflow) = increment(&kept);
    if !overflow {
        return (carried, exponent);
    }
    let mut one = vec![b'0'; SIGNIFICANT_DIGIT_COUNT];
    one[0] = b'1';
    (one, exponent + 1)
}

/// Rust's `{:.<n>e}` — `1.0e3`, `-5.0e-3`, `4.9e-324`: no `+` on the exponent, no zero padding,
/// ties to even on the exact value (1250 is `1.2e3`, where .NET's `"E1"` writes `1.2E+003`).
pub fn format_exponential(v: f64, fraction_digits: usize) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v == f64::INFINITY {
        return "inf".to_string();
    }
    if v == f64::NEG_INFINITY {
        return "-inf".to_string();
    }
    let sign = if is_negative(v) { "-" } else { "" };
    let magnitude = v.abs();
    if magnitude == 0.0 {
        let fraction = if fraction_digits > 0 { format!(".{}", zeros(fraction_digits)) } else { String::new() };
        return format!("{sign}0{fraction}e0");
    }
    let (scanned, scanned_exponent) = significant_digits(magnitude);
    let (digits, overflow) = round_digits(&scanned, fraction_digits + 1);
    let exponent = scanned_exponent + i32::from(overflow);
    let mut out = String::with_capacity(fraction_digits + 8);
    out.push_str(sign);
    out.push(char::from(digits[0]));
    if fraction_digits > 0 {
        out.push('.');
        out.push_str(ascii(&digits[1..]));
    }
    out.push('e');
    out.push_str(&exponent.to_string());
    out
}

/// Rust's `{:.<n>}` — `2.50`, `-0.00`, `1000.00`: ties to even on the exact value (0.125 is
/// `0.12`), and the sign survives a rounded-away zero.
pub fn format_fixed(v: f64, fraction_digits: usize) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v == f64::INFINITY {
        return "inf".to_string();
    }
    if v == f64::NEG_INFINITY {
        return "-inf".to_string();
    }
    let sign = if is_negative(v) { "-" } else { "" };
    let magnitude = v.abs();

    let mut whole: Vec<u8> = vec![b'0'];
    let mut fraction: Vec<u8> = Vec::new();
    if magnitude != 0.0 {
        let (digits, exponent) = significant_digits(magnitude);
        let whole_length = exponent + 1;
        if whole_length <= 0 {
            fraction = vec![b'0'; (-whole_length) as usize];
            fraction.extend_from_slice(&digits);
        } else if whole_length as usize >= digits.len() {
            whole = digits.clone();
            whole.resize(whole_length as usize, b'0');
        } else {
            whole = digits[..whole_length as usize].to_vec();
            fraction = digits[whole_length as usize..].to_vec();
        }
    }

    if fraction.len() <= fraction_digits {
        fraction.resize(fraction_digits, b'0');
    } else {
        let kept = fraction[..fraction_digits].to_vec();
        let next = fraction[fraction_digits] - b'0';
        let mut up = next > 5;
        if next == 5 {
            let more = fraction[fraction_digits + 1..].iter().any(|&digit| digit != b'0');
            if more {
                up = true;
            } else {
                let previous =
                    if fraction_digits > 0 { fraction[fraction_digits - 1] } else { whole[whole.len() - 1] } - b'0';
                up = previous % 2 == 1;
            }
        }
        if !up {
            fraction = kept;
        } else {
            let (carried, overflow) = increment(&kept);
            if overflow {
                // `.99` + 1 is `1.00`: the fraction goes back to zeros and the carry lands on the
                // integer part, the one place a digit string may grow (999 -> 1000).
                fraction = vec![b'0'; fraction_digits];
                whole = increment(&whole).0;
            } else {
                fraction = carried;
            }
        }
    }

    let mut out = String::with_capacity(whole.len() + fraction_digits + 2);
    out.push_str(sign);
    out.push_str(ascii(&whole));
    if fraction_digits > 0 {
        out.push('.');
        out.push_str(ascii(&fraction));
    }
    out
}

/// Round a digit string to `keep` digits, ties to even, reporting whether the carry ran off the
/// front — then the digits are `1` followed by zeros and the caller owes the exponent a 1.
fn round_digits(digits: &[u8], keep: usize) -> (Vec<u8>, bool) {
    if keep >= digits.len() {
        let mut padded = digits.to_vec();
        padded.resize(keep, b'0');
        return (padded, false);
    }
    let kept = digits[..keep].to_vec();
    let next = digits[keep] - b'0';
    let mut up = next > 5;
    if next == 5 {
        let more = digits[keep + 1..].iter().any(|&digit| digit != b'0');
        // `keep` is at least 1 (one digit before the point), so the digit before exists.
        up = more || (digits[keep - 1] - b'0') % 2 == 1;
    }
    if !up {
        return (kept, false);
    }
    let (carried, overflow) = increment(&kept);
    if !overflow {
        return (carried, false);
    }
    let mut one = vec![b'0'; keep];
    one[0] = b'1';
    (one, true)
}

/// `digits` + 1, keeping the length; the flag says the carry ran off the front (then the result
/// is one digit longer: `"1"` followed by the zeros).
fn increment(digits: &[u8]) -> (Vec<u8>, bool) {
    let mut out = digits.to_vec();
    let mut index = out.len();
    while index > 0 {
        index -= 1;
        if out[index] == b'9' {
            out[index] = b'0';
        } else {
            out[index] += 1;
            return (out, false);
        }
    }
    out.insert(0, b'1');
    (out, true)
}

/// Rust's `f64::round`: halfway cases go away from zero. Not `floor(v + 0.5)` (sends
/// 0.49999999999999994 to 1), not .NET's `Math.Round`, whose default is banker's.
pub(crate) fn round_ties_away(v: f64) -> f64 {
    let whole = v.trunc();
    let fraction = v - whole;
    if fraction >= 0.5 {
        return whole + 1.0;
    }
    if fraction <= -0.5 {
        return whole - 1.0;
    }
    whole
}

/// Negative, −0 included (NaN is neither: the formatters test for it first).
fn is_negative(v: f64) -> bool {
    v < 0.0 || (v == 0.0 && 1.0 / v < 0.0)
}

fn zeros(count: usize) -> String {
    "0".repeat(count)
}

/// The digit buffers hold ASCII only, so this never falls back.
fn ascii(digits: &[u8]) -> &str {
    std::str::from_utf8(digits).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_numbers_print_their_exact_digits() {
        let mut n = Natural::from_u64(0);
        assert_eq!(n.digits(), b"0");
        n = Natural::from_u64(u64::MAX);
        assert_eq!(n.digits(), u64::MAX.to_string().into_bytes());
        n.shift_left(64);
        assert_eq!(ascii(&n.digits()), "340282366920938463444927863358058659840");
        let mut five = Natural::from_u64(1);
        five.multiply_by_power_of_five(27);
        assert_eq!(ascii(&five.digits()), "7450580596923828125");
    }

    #[test]
    fn significant_digits_are_the_exact_expansion() {
        // 0.1 is 0.1000000000000000055511151231257827021181583404541015625.
        let (digits, exponent) = significant_digits(0.1);
        assert_eq!(ascii(&digits), "1000000000000000055511151");
        assert_eq!(exponent, -1);
        // The smallest subnormal, 4.940656458412465441765687928682213723651e-324: the 26th
        // digit is a 6, so the 25th rounds up.
        let (digits, exponent) = significant_digits(5e-324);
        assert_eq!(ascii(&digits), "4940656458412465441765688");
        assert_eq!(exponent, -324);
        let (digits, exponent) = significant_digits(f64::MAX);
        assert_eq!(ascii(&digits), "1797693134862315708145274");
        assert_eq!(exponent, 308);
        let (digits, exponent) = significant_digits(1250.0);
        assert_eq!(ascii(&digits), "1250000000000000000000000");
        assert_eq!(exponent, 3);
    }

    #[test]
    fn round_ties_away_is_rusts_round() {
        for v in [0.5, -0.5, 1.5, 2.5, -2.5, 0.49999999999999994, -0.3, 4503599627370497.0, 1e300, -0.0] {
            assert_eq!(round_ties_away(v).to_bits(), v.round().to_bits(), "{v}");
        }
        assert!(round_ties_away(f64::NAN).is_nan());
        assert_eq!(round_ties_away(f64::INFINITY), f64::INFINITY);
    }

    #[test]
    fn increment_carries_and_grows() {
        assert_eq!(increment(b"129"), (b"130".to_vec(), false));
        assert_eq!(increment(b"999"), (b"1000".to_vec(), true));
        assert_eq!(increment(b""), (b"1".to_vec(), true));
    }
}
