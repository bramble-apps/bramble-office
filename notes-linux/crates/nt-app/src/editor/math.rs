//! Math Results: "2+3*4=" suggests 14; "x=5" on a line above makes x a variable ("x*2=" → 10).
//! Operators + − * × / ÷ ^ %, parentheses, and the usual functions.

use std::collections::HashMap;

use super::Loc;
use super::doc::{Doc, OBJ};

/// The variables defined above `at` in its flow ("name = expression" lines).
pub fn variables(doc: &Doc, at: Loc) -> HashMap<String, f64> {
    let mut vars = HashMap::new();
    if at.cell.is_some() {
        return vars;
    }
    for p in &doc.paras[..at.para.min(doc.paras.len())] {
        let t = p.text.replace(OBJ, "");
        let Some((name, expr)) = t.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_alphabetic()) || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        if let Some(v) = eval(expr, &vars) {
            vars.insert(name.to_string(), v);
        }
    }
    vars
}

/// The value of the expression at the end of a line (from its start, or after "name =").
pub fn eval_line(line: &str, vars: &HashMap<String, f64>) -> Option<f64> {
    let e = line.trim();
    if e.is_empty() {
        return None;
    }
    // a bare number or variable isn't a calculation ("2x" and "2π" are: a product)
    let has_op = e.chars().any(|c| "+-*/×÷^%(\u{2212}".contains(c)) || e.chars().any(|c| c.is_ascii_digit()) && e.chars().any(|c| c.is_alphabetic() || c == 'π');
    if !has_op && !vars.contains_key(e) {
        return None;
    }
    eval(e, vars)
}

/// The value of an expression.
pub fn eval(e: &str, vars: &HashMap<String, f64>) -> Option<f64> {
    let mut p = Parser { s: e.chars().collect(), i: 0, vars };
    let v = p.expr()?;
    p.ws();
    (p.i == p.s.len() && v.is_finite()).then_some(v)
}

/// 14, 2.5, 0.3333333333 (ten significant digits at most, no trailing zeros).
pub fn format(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{:.10}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

struct Parser<'a> {
    s: Vec<char>,
    i: usize,
    vars: &'a HashMap<String, f64>,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(c) = self.peek() {
            match c {
                '+' => {
                    self.i += 1;
                    v += self.term()?;
                }
                '-' | '\u{2212}' => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(c) = self.peek() {
            match c {
                '*' | '×' | '\u{22c5}' => {
                    self.i += 1;
                    v *= self.power()?;
                }
                '/' | '÷' => {
                    self.i += 1;
                    v /= self.power()?;
                }
                '%' => {
                    self.i += 1;
                    v /= 100.0;
                }
                // side by side is a product: "(2)3", "2(3+4)", "(1+2)(3+4)", "2x", "3(x+1)", "2π"
                c if c == '(' || c.is_ascii_digit() || c == '.' || c.is_alphabetic() || c == 'π' => {
                    v *= self.power()?;
                }
                _ => break,
            }
        }
        Some(v)
    }

    fn power(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            let e = self.power()?;
            return Some(b.powf(e));
        }
        Some(b)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            '-' | '\u{2212}' => {
                self.i += 1;
                Some(-self.unary()?)
            }
            '+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Option<f64> {
        let c = self.peek()?;
        if c == '(' {
            self.i += 1;
            let v = self.expr()?;
            if self.peek() != Some(')') {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        if c.is_ascii_digit() || c == '.' {
            let st = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == '.' || self.s[self.i] == ',') {
                self.i += 1;
            }
            let n: String = self.s[st..self.i].iter().filter(|c| **c != ',').collect();
            return n.parse().ok();
        }
        if c.is_alphabetic() {
            let st = self.i;
            while self.i < self.s.len() && (self.s[self.i].is_alphanumeric() || self.s[self.i] == '_') {
                self.i += 1;
            }
            let name: String = self.s[st..self.i].iter().collect();
            // (a variable before a bracket multiplies it: "x(2+1)")
            if self.peek() == Some('(') && !self.vars.contains_key(&name) {
                self.i += 1;
                let a = self.expr()?;
                if self.peek() != Some(')') {
                    return None;
                }
                self.i += 1;
                return match name.as_str() {
                    "sqrt" => Some(a.sqrt()),
                    "sin" => Some(a.sin()),
                    "cos" => Some(a.cos()),
                    "tan" => Some(a.tan()),
                    "ln" => Some(a.ln()),
                    "log" => Some(a.log10()),
                    "abs" => Some(a.abs()),
                    "round" => Some(a.round()),
                    _ => None,
                };
            }
            return match name.as_str() {
                "pi" | "π" => Some(std::f64::consts::PI),
                "e" => Some(std::f64::consts::E),
                _ => self.vars.get(&name).copied(),
            };
        }
        if c == 'π' {
            self.i += 1;
            return Some(std::f64::consts::PI);
        }
        None
    }
}

/// A line's result as shown: a number, or a quantity with its unit ("3 m + 20 cm" → "3.2 m",
/// "10 km in miles" → "6.2137 miles", "100 °F in °C" → "37.7778 °C").
pub fn result(line: &str, vars: &HashMap<String, f64>) -> Option<String> {
    if let Some(r) = units(line, vars) {
        return Some(r);
    }
    eval_line(line, vars).map(format)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dim {
    Length,
    Mass,
    Volume,
    Time,
    Area,
    Speed,
    Data,
    Temp,
}

/// Units: what's written, its kind, its size in the kind's base unit (temperatures: °C steps,
/// with an offset after).
const UNITS: &[(&[&str], Dim, f64)] = &[
    (&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Dim::Length, 0.001),
    (&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Dim::Length, 0.01),
    (&["m", "meter", "meters", "metre", "metres"], Dim::Length, 1.0),
    (&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Dim::Length, 1000.0),
    (&["in", "inch", "inches", "\""], Dim::Length, 0.0254),
    (&["ft", "foot", "feet", "'"], Dim::Length, 0.3048),
    (&["yd", "yard", "yards"], Dim::Length, 0.9144),
    (&["mi", "mile", "miles"], Dim::Length, 1609.344),
    (&["nmi", "nautical mile", "nautical miles"], Dim::Length, 1852.0),
    (&["mg", "milligram", "milligrams"], Dim::Mass, 0.000001),
    (&["g", "gram", "grams"], Dim::Mass, 0.001),
    (&["kg", "kilogram", "kilograms", "kilo", "kilos"], Dim::Mass, 1.0),
    (&["t", "tonne", "tonnes"], Dim::Mass, 1000.0),
    (&["oz", "ounce", "ounces"], Dim::Mass, 0.028349523125),
    (&["lb", "lbs", "pound", "pounds"], Dim::Mass, 0.45359237),
    (&["st", "stone", "stones"], Dim::Mass, 6.35029318),
    (&["ml", "mL", "milliliter", "milliliters", "millilitre", "millilitres"], Dim::Volume, 0.001),
    (&["cl", "cL"], Dim::Volume, 0.01),
    (&["dl", "dL"], Dim::Volume, 0.1),
    (&["l", "L", "liter", "liters", "litre", "litres"], Dim::Volume, 1.0),
    (&["tsp", "teaspoon", "teaspoons"], Dim::Volume, 0.00492892159375),
    (&["tbsp", "tablespoon", "tablespoons"], Dim::Volume, 0.01478676478125),
    (&["fl oz", "fluid ounce", "fluid ounces"], Dim::Volume, 0.0295735295625),
    (&["cup", "cups"], Dim::Volume, 0.2365882365),
    (&["pt", "pint", "pints"], Dim::Volume, 0.473176473),
    (&["qt", "quart", "quarts"], Dim::Volume, 0.946352946),
    (&["gal", "gallon", "gallons"], Dim::Volume, 3.785411784),
    (&["ms", "millisecond", "milliseconds"], Dim::Time, 0.001),
    (&["s", "sec", "secs", "second", "seconds"], Dim::Time, 1.0),
    (&["min", "mins", "minute", "minutes"], Dim::Time, 60.0),
    (&["h", "hr", "hrs", "hour", "hours"], Dim::Time, 3600.0),
    (&["day", "days"], Dim::Time, 86400.0),
    (&["week", "weeks", "wk"], Dim::Time, 604800.0),
    (&["m²", "m2", "sq m", "square meters", "square metres"], Dim::Area, 1.0),
    (&["km²", "km2", "sq km", "square kilometers", "square kilometres"], Dim::Area, 1e6),
    (&["ft²", "ft2", "sq ft", "square feet"], Dim::Area, 0.09290304),
    (&["ha", "hectare", "hectares"], Dim::Area, 10000.0),
    (&["acre", "acres"], Dim::Area, 4046.8564224),
    (&["m/s"], Dim::Speed, 1.0),
    (&["km/h", "kph", "kmh"], Dim::Speed, 1.0 / 3.6),
    (&["mph"], Dim::Speed, 0.44704),
    (&["kn", "knot", "knots"], Dim::Speed, 0.514444),
    (&["B", "byte", "bytes"], Dim::Data, 1.0),
    (&["KB", "kB", "kilobyte", "kilobytes"], Dim::Data, 1e3),
    (&["MB", "megabyte", "megabytes"], Dim::Data, 1e6),
    (&["GB", "gigabyte", "gigabytes"], Dim::Data, 1e9),
    (&["TB", "terabyte", "terabytes"], Dim::Data, 1e12),
    (&["°C", "C", "celsius", "Celsius"], Dim::Temp, 0.0),
    (&["°F", "F", "fahrenheit", "Fahrenheit"], Dim::Temp, 1.0),
    (&["K", "kelvin", "Kelvin"], Dim::Temp, 2.0),
];

/// The unit a piece ends with (the longest name that fits): (its entry, the number part).
fn split_unit(s: &str) -> Option<(&'static (&'static [&'static str], Dim, f64), &'static str, &str)> {
    let s = s.trim();
    let mut best: Option<(&(&[&str], Dim, f64), &str, &str)> = None;
    for u in UNITS {
        for name in u.0 {
            if let Some(rest) = s.strip_suffix(name) {
                // (a word unit needs a space or a digit before it; "°C" and "\"" don't)
                let ok = rest.ends_with(|c: char| c.is_whitespace() || c.is_ascii_digit() || c == ')') || name.starts_with('°') || *name == "\"" || *name == "'";
                if ok && !rest.trim().is_empty() && best.is_none_or(|b| name.len() > b.1.len()) {
                    best = Some((u, name, rest.trim_end()));
                }
            }
        }
    }
    best
}

fn unit_named(s: &str) -> Option<(&'static (&'static [&'static str], Dim, f64), &'static str)> {
    let s = s.trim();
    UNITS.iter().find_map(|u| u.0.iter().find(|n| **n == s || n.eq_ignore_ascii_case(s) && n.len() > 2).map(|n| (u, *n)))
}

fn to_celsius(v: f64, k: f64) -> f64 {
    match k as i32 {
        1 => (v - 32.0) * 5.0 / 9.0,
        2 => v - 273.15,
        _ => v,
    }
}

fn from_celsius(c: f64, k: f64) -> f64 {
    match k as i32 {
        1 => c * 9.0 / 5.0 + 32.0,
        2 => c + 273.15,
        _ => c,
    }
}

/// Quantities: sums and differences of "<expression> <unit>" of one kind, optionally converted
/// ("… in / to / as <unit>"). None when the line has no units.
fn units(line: &str, vars: &HashMap<String, f64>) -> Option<String> {
    let line = line.trim();
    // the conversion's target
    let (body, target) = [" in ", " to ", " as "].iter().find_map(|k| line.rsplit_once(k).and_then(|(a, b)| unit_named(b).map(|u| (a, Some(u))))).unwrap_or((line, None));
    // terms at the top level, split on + and − (not inside brackets, not a leading sign)
    let mut terms: Vec<(f64, String)> = Vec::new();
    let (mut depth, mut start, mut sign) = (0i32, 0usize, 1.0f64);
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let mut pieces: Vec<(f64, &str)> = Vec::new();
    for (k, &(i, c)) in chars.iter().enumerate() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '+' | '-' | '\u{2212}' if depth == 0 && k > 0 && !body[start..i].trim().is_empty() => {
                pieces.push((sign, &body[start..i]));
                sign = if c == '+' { 1.0 } else { -1.0 };
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    pieces.push((sign, &body[start..]));
    let mut dim = None;
    let mut first_unit: Option<(&(&[&str], Dim, f64), &str)> = None;
    let mut total = 0.0;
    for (sign, p) in pieces {
        let (u, name, num) = split_unit(p)?;
        if dim.is_some_and(|d| d != u.1) {
            return None;
        }
        dim = Some(u.1);
        let v = eval(num, vars)?;
        if u.1 == Dim::Temp {
            // (temperatures don't add; one only)
            if first_unit.is_some() {
                return None;
            }
            total = to_celsius(sign * v, u.2);
        } else {
            total += sign * v * u.2;
        }
        first_unit.get_or_insert((u, name));
        terms.push((v, name.to_string()));
    }
    let (fu, fname) = first_unit?;
    // a lone "5 km" with nothing to do isn't a calculation
    if terms.len() == 1 && target.is_none() {
        return None;
    }
    let (tu, tname) = target.unwrap_or((fu, fname));
    if tu.1 != fu.1 {
        return None;
    }
    let v = if tu.1 == Dim::Temp { from_celsius(total, tu.2) } else { total / tu.2 };
    let n = if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{:.4}", v).trim_end_matches('0').trim_end_matches('.').to_string() };
    // (inches and feet as marks sit on the number)
    Some(if tname == "\"" || tname == "'" { format!("{n}{tname}") } else { format!("{n} {tname}") })
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn units_add_and_convert() {
        let v = HashMap::new();
        assert_eq!(result("3 m + 20 cm", &v).as_deref(), Some("3.2 m"));
        assert_eq!(result("10 km in miles", &v).as_deref(), Some("6.2137 miles"));
        assert_eq!(result("5 ft 0 in", &v), None, "not an expression");
        assert_eq!(result("2 lb to kg", &v).as_deref(), Some("0.9072 kg"));
        assert_eq!(result("1.5 h - 20 min", &v).as_deref(), Some("1.1667 h"));
        assert_eq!(result("2 * 3 kg + 500 g", &v).as_deref(), Some("6.5 kg"));
        assert_eq!(result("100 °F in °C", &v).as_deref(), Some("37.7778 °C"));
        assert_eq!(result("3 cups in ml", &v).as_deref(), Some("709.7647 ml"));
        assert_eq!(result("1 GB in MB", &v).as_deref(), Some("1000 MB"));
        assert_eq!(result("5 km + 2 kg", &v), None, "different kinds");
        assert_eq!(result("2+3", &v).as_deref(), Some("5"), "plain sums as before");
        assert_eq!(result("5 km", &v), None);
    }

    #[test]
    fn brackets_side_by_side_multiply() {
        let v = HashMap::new();
        for (e, want) in [("2(5)*4", "40"), ("2(5)", "10"), ("(2)(3)", "6"), ("(1+2)4", "12"), ("2(3+4)", "14"), ("(1+2)(3+4)", "21"), ("2 (5) * 4", "40"), ("((2))(3)", "6"), ("-2(5)", "-10"), ("2(5)^2", "50"), ("10/2(5)", "25")] {
            assert_eq!(result(e, &v).as_deref(), Some(want), "{e}");
        }
        let x = HashMap::from([("x".to_string(), 3.0)]);
        assert_eq!(result("x(2+1)", &x).as_deref(), Some("9"));
        assert_eq!(result("2x(4)", &x).as_deref(), Some("24"));
    }
}
