//! Number formatting the way the iOS display does it: grouped digits, at most `max_digits`
//! digits on screen, and scientific notation ("1.2345e15") when a value does not fit.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NumberStyle {
    /// 1,234.5
    #[default]
    Comma,
    /// 1.234,5
    Dot,
    /// 1 234,5
    Space,
}

impl NumberStyle {
    pub fn decimal(self) -> char {
        match self {
            NumberStyle::Comma => '.',
            _ => ',',
        }
    }
    pub fn group(self) -> char {
        match self {
            NumberStyle::Comma => ',',
            NumberStyle::Dot => '.',
            NumberStyle::Space => '\u{202F}',
        }
    }
    pub fn all() -> [(NumberStyle, &'static str); 3] {
        [(NumberStyle::Comma, "1,234.56"), (NumberStyle::Dot, "1.234,56"), (NumberStyle::Space, "1 234,56")]
    }
}

/// Groups the integer part of a plain typed number ("12345.6" -> "12,345.6").
pub fn group_typed(s: &str, style: NumberStyle) -> String {
    let (int, frac) = match s.find('.') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let int = if int.is_empty() { "0" } else { int };
    let mut out = group_int(int, style.group());
    if let Some(f) = frac {
        out.push(style.decimal());
        out.push_str(f);
    }
    out
}

fn group_int(int: &str, sep: char) -> String {
    let n = int.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// A result for the display. `max_digits` counts digits only (9 on the basic keypad).
pub fn format_value(v: f64, max_digits: usize, style: NumberStyle) -> String {
    if v.is_nan() || v.is_infinite() {
        return "Error".into();
    }
    if v == 0.0 {
        return "0".into();
    }
    let neg = v < 0.0;
    let a = v.abs();
    let exp = a.log10().floor() as i32;
    let body = if exp >= max_digits as i32 || exp < -(max_digits as i32 - 1).min(8) {
        sci(a, max_digits, style)
    } else {
        // round to the digits that fit: integer digits + fraction digits <= max_digits
        let int_digits = if exp >= 0 { exp as usize + 1 } else { 0 };
        let frac = max_digits.saturating_sub(int_digits);
        let mut s = format!("{:.*}", frac, a);
        // rounding can add a digit (999999999.6 -> 1000000000)
        let int_len = s.find('.').unwrap_or(s.len());
        if int_len > max_digits {
            return format!("{}{}", if neg { "-" } else { "" }, sci(a, max_digits, style));
        }
        if s.contains('.') {
            while s.ends_with('0') {
                s.pop();
            }
            if s.ends_with('.') {
                s.pop();
            }
        }
        if s == "0" {
            sci(a, max_digits, style)
        } else {
            group_typed(&s, style)
        }
    };
    if neg {
        format!("-{body}")
    } else {
        body
    }
}

fn sci(a: f64, max_digits: usize, style: NumberStyle) -> String {
    let s = format!("{:.*e}", max_digits.saturating_sub(3).max(1), a);
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let mut m = m.to_string();
    if m.contains('.') {
        while m.ends_with('0') {
            m.pop();
        }
        if m.ends_with('.') {
            m.pop();
        }
    }
    let m = m.replace('.', &style.decimal().to_string());
    format!("{m}e{e}")
}

/// A plain machine-readable rendering (for copying and for history reloads).
pub fn plain(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.12}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let s = NumberStyle::Comma;
        assert_eq!(format_value(1976.0, 9, s), "1,976");
        assert_eq!(format_value(1_023_101.0, 9, s), "1,023,101");
        assert_eq!(format_value(1.0 / 3.0, 9, s), "0.333333333");
        assert_eq!(format_value(2.0 / 3.0, 9, s), "0.666666667");
        assert_eq!(format_value(1e9, 9, s), "1e9");
        assert_eq!(format_value(123_456_789_012.0, 9, s), "1.234568e11");
        assert_eq!(format_value(-0.5, 9, s), "-0.5");
        assert_eq!(format_value(685.46, 9, NumberStyle::Dot), "685,46");
        assert_eq!(format_value(0.000_000_001_5, 9, s), "1.5e-9");
        assert_eq!(group_typed("12345.60", s), "12,345.60");
    }
}
