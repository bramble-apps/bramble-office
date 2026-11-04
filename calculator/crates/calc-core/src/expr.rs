//! Expression tokens and their evaluation. The keypad builds a token list (calc.rs) and Math
//! Notes turns typed lines into the same tokens (`tokenize`), so both share one evaluator.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    /// xʸ
    Pow,
    /// yˣ: the second operand is the base
    PowRev,
    /// ʸ√x: the second operand is the index
    Root,
    /// log_y x: the second operand is the base
    LogY,
    /// EE: a × 10^b
    Ee,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Fun {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sinh,
    Cosh,
    Tanh,
    Asinh,
    Acosh,
    Atanh,
    Ln,
    Log10,
    Log2,
    Sqrt,
    Cbrt,
    /// eˣ
    Exp,
    /// 10ˣ
    Exp10,
    /// 2ˣ
    Exp2,
    /// unary minus around a group: -(…)
    Neg,
    Abs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Post {
    Sq,
    Cube,
    Fact,
    Pct,
    Recip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Tok {
    /// A typed number: digits and at most one '.', `neg` from +/-.
    Num { s: String, neg: bool },
    /// A value that was not typed: a result (Ans), memory recall, Rand.
    Val(f64),
    Pi,
    E,
    Op(Bin),
    Fn(Fun),
    LParen,
    RParen,
    Post(Post),
    /// A Math Notes variable (x for graphs).
    Var(String),
}

impl Tok {
    pub fn is_operand_end(&self) -> bool {
        matches!(self, Tok::Num { .. } | Tok::Val(_) | Tok::Pi | Tok::E | Tok::RParen | Tok::Post(_) | Tok::Var(_))
    }
}

impl Fun {
    pub fn name(self) -> &'static str {
        match self {
            Fun::Sin => "sin",
            Fun::Cos => "cos",
            Fun::Tan => "tan",
            Fun::Asin => "sin⁻¹",
            Fun::Acos => "cos⁻¹",
            Fun::Atan => "tan⁻¹",
            Fun::Sinh => "sinh",
            Fun::Cosh => "cosh",
            Fun::Tanh => "tanh",
            Fun::Asinh => "sinh⁻¹",
            Fun::Acosh => "cosh⁻¹",
            Fun::Atanh => "tanh⁻¹",
            Fun::Ln => "ln",
            Fun::Log10 => "log₁₀",
            Fun::Log2 => "log₂",
            Fun::Sqrt => "√",
            Fun::Cbrt => "∛",
            Fun::Exp => "e",
            Fun::Exp10 => "10",
            Fun::Exp2 => "2",
            Fun::Neg => "-",
            Fun::Abs => "abs",
        }
    }
    /// eˣ, 10ˣ, 2ˣ: the argument is drawn as a superscript.
    pub fn sup_arg(self) -> bool {
        matches!(self, Fun::Exp | Fun::Exp10 | Fun::Exp2)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    Syntax,
    Domain,
    Unknown(String),
}

pub struct Ctx<'a> {
    pub rad: bool,
    pub vars: Option<&'a HashMap<String, f64>>,
}

/// Evaluates a token list. Unclosed parentheses are closed at the end, a trailing operator is
/// ignored, and missing multiplications ("2π", "3(4)", ")(") are implied.
pub fn eval(toks: &[Tok], ctx: &Ctx) -> Result<f64, EvalError> {
    let mut toks: Vec<Tok> = toks.to_vec();
    while matches!(toks.last(), Some(Tok::Op(_)) | Some(Tok::Fn(_))) {
        toks.pop();
    }
    while matches!(toks.last(), Some(Tok::LParen)) {
        toks.pop();
        while matches!(toks.last(), Some(Tok::Fn(_))) {
            toks.pop();
        }
    }
    if toks.is_empty() {
        return Ok(0.0);
    }
    let mut p = Parser { t: &toks, i: 0, ctx };
    let (v, _) = p.sum()?;
    if p.i < p.t.len() {
        return Err(EvalError::Syntax);
    }
    if v.is_finite() {
        Ok(if v == 0.0 { 0.0 } else { v })
    } else {
        Err(EvalError::Domain)
    }
}

/// The last top-level operation of an expression, for repeated "=" (2+3 = = gives 8).
pub fn last_operation(toks: &[Tok], ctx: &Ctx) -> Option<(Bin, f64)> {
    let mut depth = 0i32;
    let mut split = None;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::LParen => depth += 1,
            Tok::RParen => depth -= 1,
            Tok::Op(b @ (Bin::Add | Bin::Sub | Bin::Mul | Bin::Div)) if depth == 0 && i > 0 && toks[i - 1].is_operand_end() => {
                split = Some((i, *b))
            }
            _ => {}
        }
    }
    let (i, b) = split?;
    let rhs = &toks[i + 1..];
    if rhs.iter().any(|t| matches!(t, Tok::Post(Post::Pct))) {
        return None;
    }
    let v = eval(rhs, ctx).ok()?;
    Some((b, v))
}

pub fn apply_bin(b: Bin, a: f64, c: f64) -> Result<f64, EvalError> {
    let r = match b {
        Bin::Add => a + c,
        Bin::Sub => a - c,
        Bin::Mul => a * c,
        Bin::Div => {
            if c == 0.0 {
                return Err(EvalError::Domain);
            }
            a / c
        }
        Bin::Pow => pow(a, c),
        Bin::PowRev => pow(c, a),
        Bin::Root => root(a, c),
        Bin::LogY => a.ln() / c.ln(),
        Bin::Ee => a * 10f64.powf(c),
    };
    if r.is_nan() {
        Err(EvalError::Domain)
    } else {
        Ok(r)
    }
}

fn pow(a: f64, b: f64) -> f64 {
    // Odd integer roots of negative numbers through fractional exponents are left as NaN, as on iOS.
    a.powf(b)
}

fn root(x: f64, n: f64) -> f64 {
    if x < 0.0 && n.fract() == 0.0 && (n as i64) % 2 != 0 {
        -(-x).powf(1.0 / n)
    } else {
        x.powf(1.0 / n)
    }
}

pub fn gamma(x: f64) -> f64 {
    if x < 0.5 {
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma(1.0 - x))
    } else {
        // Lanczos approximation, g = 7
        const G: [f64; 9] = [
            0.999_999_999_999_809_9,
            676.520_368_121_885_1,
            -1_259.139_216_722_402_8,
            771.323_428_777_653_1,
            -176.615_029_162_140_6,
            12.507_343_278_686_905,
            -0.138_571_095_265_720_12,
            9.984_369_578_019_572e-6,
            1.505_632_735_149_311_6e-7,
        ];
        let x = x - 1.0;
        let mut a = G[0];
        let t = x + 7.5;
        for (i, g) in G.iter().enumerate().skip(1) {
            a += g / (x + i as f64);
        }
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
    }
}

pub fn factorial(x: f64) -> f64 {
    if x < 0.0 && x.fract() == 0.0 {
        return f64::NAN;
    }
    if x.fract() == 0.0 && x <= 170.0 {
        let mut r = 1.0;
        let mut i = 2.0;
        while i <= x {
            r *= i;
            i += 1.0;
        }
        return r;
    }
    gamma(x + 1.0)
}

fn apply_fun(f: Fun, x: f64, rad: bool) -> f64 {
    let to_r = |v: f64| if rad { v } else { v.to_radians() };
    let from_r = |v: f64| if rad { v } else { v.to_degrees() };
    // Exact values at the multiples of 90° that floating point misses (sin 180° = 0, tan 90° undefined).
    let exact = |v: f64| if v.abs() < 1e-15 { 0.0 } else { v };
    match f {
        Fun::Sin => {
            if !rad && (x % 180.0) == 0.0 {
                0.0
            } else {
                exact(to_r(x).sin())
            }
        }
        Fun::Cos => {
            if !rad && ((x - 90.0) % 180.0) == 0.0 {
                0.0
            } else {
                exact(to_r(x).cos())
            }
        }
        Fun::Tan => {
            if !rad && ((x - 90.0) % 180.0) == 0.0 {
                f64::NAN
            } else if !rad && (x % 180.0) == 0.0 {
                0.0
            } else {
                exact(to_r(x).tan())
            }
        }
        Fun::Asin => from_r(x.asin()),
        Fun::Acos => from_r(x.acos()),
        Fun::Atan => from_r(x.atan()),
        Fun::Sinh => x.sinh(),
        Fun::Cosh => x.cosh(),
        Fun::Tanh => x.tanh(),
        Fun::Asinh => x.asinh(),
        Fun::Acosh => x.acosh(),
        Fun::Atanh => x.atanh(),
        Fun::Ln => x.ln(),
        Fun::Log10 => x.log10(),
        Fun::Log2 => x.log2(),
        Fun::Sqrt => x.sqrt(),
        Fun::Cbrt => x.cbrt(),
        Fun::Exp => x.exp(),
        Fun::Exp10 => 10f64.powf(x),
        Fun::Exp2 => 2f64.powf(x),
        Fun::Neg => -x,
        Fun::Abs => x.abs(),
    }
}

struct Parser<'a> {
    t: &'a [Tok],
    i: usize,
    ctx: &'a Ctx<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    /// Sums; a right operand ending in % is a percentage of the left (50 + 10% = 55).
    fn sum(&mut self) -> Result<(f64, bool), EvalError> {
        let (mut a, mut pct) = self.product()?;
        loop {
            let b = match self.peek() {
                Some(Tok::Op(b @ (Bin::Add | Bin::Sub))) => *b,
                _ => break,
            };
            self.i += 1;
            if self.peek().is_none() {
                break;
            }
            let (mut c, cp) = self.product()?;
            if cp {
                c *= a;
            }
            a = apply_bin(b, a, c)?;
            pct = false;
        }
        Ok((a, pct))
    }

    fn product(&mut self) -> Result<(f64, bool), EvalError> {
        let (mut a, mut pct) = self.unary()?;
        loop {
            match self.peek() {
                Some(Tok::Op(b @ (Bin::Mul | Bin::Div))) => {
                    let b = *b;
                    self.i += 1;
                    if self.peek().is_none() {
                        break;
                    }
                    let (c, _) = self.unary()?;
                    a = apply_bin(b, a, c)?;
                }
                // implied multiplication
                Some(Tok::Num { .. } | Tok::Val(_) | Tok::Pi | Tok::E | Tok::Fn(_) | Tok::LParen | Tok::Var(_)) => {
                    let (c, _) = self.unary()?;
                    a *= c;
                }
                _ => break,
            }
            pct = false;
        }
        Ok((a, pct))
    }

    fn unary(&mut self) -> Result<(f64, bool), EvalError> {
        match self.peek() {
            Some(Tok::Op(Bin::Sub)) => {
                self.i += 1;
                let (v, p) = self.unary()?;
                Ok((-v, p))
            }
            Some(Tok::Op(Bin::Add)) => {
                self.i += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<(f64, bool), EvalError> {
        let (a, pct) = self.postfix()?;
        match self.peek() {
            Some(Tok::Op(b @ (Bin::Pow | Bin::PowRev | Bin::Root | Bin::LogY | Bin::Ee))) => {
                let b = *b;
                self.i += 1;
                if self.peek().is_none() {
                    return Ok((a, pct));
                }
                let (c, _) = self.unary()?;
                Ok((apply_bin(b, a, c)?, false))
            }
            _ => Ok((a, pct)),
        }
    }

    fn postfix(&mut self) -> Result<(f64, bool), EvalError> {
        let mut v = self.primary()?;
        let mut pct = false;
        while let Some(Tok::Post(p)) = self.peek() {
            let p = *p;
            self.i += 1;
            v = match p {
                Post::Sq => v * v,
                Post::Cube => v * v * v,
                Post::Fact => factorial(v),
                Post::Recip => {
                    if v == 0.0 {
                        return Err(EvalError::Domain);
                    }
                    1.0 / v
                }
                Post::Pct => {
                    pct = true;
                    v / 100.0
                }
            };
        }
        Ok((v, pct))
    }

    fn primary(&mut self) -> Result<f64, EvalError> {
        let t = self.peek().cloned().ok_or(EvalError::Syntax)?;
        self.i += 1;
        match t {
            Tok::Num { s, neg } => {
                let v: f64 = if s.is_empty() || s == "." { 0.0 } else { s.parse().map_err(|_| EvalError::Syntax)? };
                Ok(if neg { -v } else { v })
            }
            Tok::Val(v) => Ok(v),
            Tok::Pi => Ok(std::f64::consts::PI),
            Tok::E => Ok(std::f64::consts::E),
            Tok::Var(n) => self.ctx.vars.and_then(|m| m.get(&n).copied()).ok_or(EvalError::Unknown(n)),
            Tok::LParen => {
                let (v, _) = self.group()?;
                Ok(v)
            }
            Tok::Fn(f) => {
                let x = if matches!(self.peek(), Some(Tok::LParen)) {
                    self.i += 1;
                    self.group()?.0
                } else {
                    self.unary()?.0
                };
                let r = apply_fun(f, x, self.ctx.rad);
                if r.is_nan() {
                    Err(EvalError::Domain)
                } else {
                    Ok(r)
                }
            }
            _ => Err(EvalError::Syntax),
        }
    }

    fn group(&mut self) -> Result<(f64, bool), EvalError> {
        if matches!(self.peek(), Some(Tok::RParen)) {
            self.i += 1;
            return Ok((0.0, false));
        }
        let v = self.sum()?;
        match self.peek() {
            Some(Tok::RParen) => self.i += 1,
            None => {}
            _ => return Err(EvalError::Syntax),
        }
        Ok(v)
    }
}

/// Typed text (Math Notes) to tokens. Accepts the keypad's symbols and ASCII (* / ^ sqrt pi).
/// A word right after a number that is not a known name ("100mg") is a unit label and skipped.
pub fn tokenize(src: &str, known: &dyn Fn(&str) -> bool) -> Result<Vec<Tok>, EvalError> {
    let c: Vec<char> = src.chars().collect();
    let mut out: Vec<Tok> = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() || ch == ',' && i > 0 && i + 1 < c.len() && c[i - 1].is_ascii_digit() && c[i + 1].is_ascii_digit() {
            i += 1;
            continue;
        }
        if ch.is_ascii_digit() || ch == '.' {
            let st = i;
            while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.' || c[i] == ',' && i + 1 < c.len() && c[i + 1].is_ascii_digit()) {
                i += 1;
            }
            // exponent: 1E5, 2e-3
            if i + 1 < c.len() && (c[i] == 'e' || c[i] == 'E') {
                let j = if (c[i + 1] == '-' || c[i + 1] == '+') && i + 2 < c.len() { i + 2 } else { i + 1 };
                if c[j].is_ascii_digit() {
                    i = j;
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let s: String = c[st..i].iter().filter(|ch| **ch != ',').collect();
            out.push(Tok::Num { s, neg: false });
            // unit label right after the number
            if i < c.len() && c[i].is_ascii_alphabetic() {
                let ws = i;
                let mut j = i;
                while j < c.len() && (c[j].is_alphanumeric() || c[j] == '_') {
                    j += 1;
                }
                let w: String = c[ws..j].iter().collect();
                if !known(&w) && !is_word(&w) && w != "E" && w != "e" {
                    i = j;
                }
            }
            continue;
        }
        if (ch.is_alphabetic() && ch != 'π') || ch == '_' {
            let st = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let w: String = c[st..i].iter().collect();
            if known(&w) {
                out.push(Tok::Var(w));
                continue;
            }
            match word(&w) {
                Some(t) => out.push(t),
                None => {
                    if w == "E" && matches!(out.last(), Some(Tok::Num { .. })) {
                        out.push(Tok::Op(Bin::Ee));
                    } else {
                        out.push(Tok::Var(w))
                    }
                }
            }
            continue;
        }
        let t = match ch {
            '+' => Tok::Op(Bin::Add),
            '-' | '−' | '–' => Tok::Op(Bin::Sub),
            '*' | '×' | '·' => Tok::Op(Bin::Mul),
            '/' | '÷' => Tok::Op(Bin::Div),
            '^' => Tok::Op(Bin::Pow),
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '!' => Tok::Post(Post::Fact),
            '%' => Tok::Post(Post::Pct),
            '²' => Tok::Post(Post::Sq),
            '³' => Tok::Post(Post::Cube),
            'π' => Tok::Pi,
            '√' => Tok::Fn(Fun::Sqrt),
            '∛' => Tok::Fn(Fun::Cbrt),
            _ => return Err(EvalError::Syntax),
        };
        out.push(t);
        i += 1;
    }
    Ok(out)
}

fn is_word(w: &str) -> bool {
    word(w).is_some()
}

fn word(w: &str) -> Option<Tok> {
    Some(match w {
        "pi" | "PI" | "Pi" => Tok::Pi,
        "e" => Tok::E,
        "sin" => Tok::Fn(Fun::Sin),
        "cos" => Tok::Fn(Fun::Cos),
        "tan" => Tok::Fn(Fun::Tan),
        "asin" | "arcsin" => Tok::Fn(Fun::Asin),
        "acos" | "arccos" => Tok::Fn(Fun::Acos),
        "atan" | "arctan" => Tok::Fn(Fun::Atan),
        "sinh" => Tok::Fn(Fun::Sinh),
        "cosh" => Tok::Fn(Fun::Cosh),
        "tanh" => Tok::Fn(Fun::Tanh),
        "asinh" => Tok::Fn(Fun::Asinh),
        "acosh" => Tok::Fn(Fun::Acosh),
        "atanh" => Tok::Fn(Fun::Atanh),
        "ln" => Tok::Fn(Fun::Ln),
        "log" | "log10" => Tok::Fn(Fun::Log10),
        "log2" => Tok::Fn(Fun::Log2),
        "sqrt" => Tok::Fn(Fun::Sqrt),
        "cbrt" => Tok::Fn(Fun::Cbrt),
        "exp" => Tok::Fn(Fun::Exp),
        "abs" => Tok::Fn(Fun::Abs),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        let t = tokenize(s, &|_| false).unwrap();
        eval(&t, &Ctx { rad: false, vars: None }).unwrap()
    }

    #[test]
    fn precedence_and_percent() {
        assert_eq!(ev("2+3×4"), 14.0);
        assert_eq!(ev("50+10%"), 55.0);
        assert_eq!(ev("50-10%"), 45.0);
        assert_eq!(ev("50×10%"), 5.0);
        assert_eq!(ev("10%"), 0.1);
        assert_eq!(ev("2^3^2"), 512.0);
        assert_eq!(ev("-2^2"), -4.0);
        assert_eq!(ev("2π") , 2.0 * std::f64::consts::PI);
        assert_eq!(ev("(2+3"), 5.0);
        assert_eq!(ev("7!÷2-(2²×136)"), 1976.0);
        assert_eq!(ev("9!×3-((4)^8+3)"), 1_023_101.0);
    }

    #[test]
    fn trig_in_degrees() {
        assert_eq!(ev("sin(180)"), 0.0);
        assert_eq!(ev("cos(90)"), 0.0);
        assert!((ev("sin(30)") - 0.5).abs() < 1e-12);
        assert!(eval(&tokenize("tan(90)", &|_| false).unwrap(), &Ctx { rad: false, vars: None }).is_err());
    }

    #[test]
    fn factorial_gamma() {
        assert_eq!(factorial(5.0), 120.0);
        assert!((factorial(0.5) - 0.886_226_925_452_758).abs() < 1e-9);
    }

    #[test]
    fn unit_labels_are_skipped() {
        assert_eq!(ev("100mg × 2"), 200.0);
        assert_eq!(ev("1E5"), 100000.0);
        assert_eq!(ev("2e-3"), 0.002);
    }
}
