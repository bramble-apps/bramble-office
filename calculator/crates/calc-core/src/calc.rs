//! The keypad: what each key does to the expression being typed, and what the display shows.
//! iOS 18+ behaviour: the whole expression is shown while typing and moves to the small line
//! when "=" is pressed; operators follow precedence; "=" again repeats the last operation.

use crate::expr::{self, Bin, Ctx, Fun, Post, Tok};
use crate::format::{format_value, group_typed, plain, NumberStyle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seg {
    pub t: String,
    /// 0 normal, 1 superscript
    pub sup: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Shown {
    pub expr: Vec<Tok>,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Calc {
    pub toks: Vec<Tok>,
    pub result: Option<Shown>,
    pub memory: f64,
    pub second: bool,
    pub rad: bool,
    #[serde(skip)]
    pub max_digits: usize,
    #[serde(skip)]
    last_op: Option<(Bin, f64)>,
    #[serde(skip)]
    rng: u64,
}

/// One finished calculation, for the history.
#[derive(Clone, Debug)]
pub struct Done {
    pub expr: Vec<Tok>,
    pub value: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Display {
    pub main: Vec<Seg>,
    pub expr: Vec<Seg>,
    pub is_result: bool,
    pub error: bool,
    /// "C" while a number is being typed, otherwise "AC"
    pub clear_label: &'static str,
    /// iOS 18's first key: "back" while there is input, otherwise "ac"
    pub first_key: &'static str,
    pub rad: bool,
    pub second: bool,
    pub memory: bool,
    /// the operator key waiting for its second operand (classic themes light it)
    pub pending_op: Option<&'static str>,
    pub value: f64,
    pub empty: bool,
}

impl Default for Calc {
    fn default() -> Self {
        Calc { toks: Vec::new(), result: None, memory: 0.0, second: false, rad: false, max_digits: 9, last_op: None, rng: 0 }
    }
}

fn op_tok(b: Bin) -> Tok {
    Tok::Op(b)
}

impl Calc {
    fn ctx(&self) -> Ctx<'static> {
        Ctx { rad: self.rad, vars: None }
    }

    fn rand(&mut self) -> f64 {
        if self.rng == 0 {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
            self.rng = t | 1;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    fn last_is_operand(&self) -> bool {
        self.toks.last().is_some_and(|t| t.is_operand_end())
    }

    fn open_parens(&self) -> i32 {
        self.toks.iter().fold(0, |n, t| match t {
            Tok::LParen => n + 1,
            Tok::RParen => n - 1,
            _ => n,
        })
    }

    /// Turns a shown result into the start of a new expression (operators continue from it).
    fn continue_from_result(&mut self) {
        if let Some(r) = self.result.take() {
            self.toks.clear();
            if let Some(v) = r.value {
                self.toks.push(Tok::Val(v));
            }
        }
    }

    fn start_fresh_if_result(&mut self) {
        if self.result.take().is_some() {
            self.toks.clear();
            self.last_op = None;
        }
    }

    /// Where the operand that ends the expression starts (for sin, x², +/- and the like).
    pub fn operand_start(toks: &[Tok]) -> Option<usize> {
        let mut i = toks.len();
        while i > 0 && matches!(toks[i - 1], Tok::Post(_)) {
            i -= 1;
        }
        if i == 0 {
            return None;
        }
        match &toks[i - 1] {
            Tok::Num { .. } | Tok::Val(_) | Tok::Pi | Tok::E | Tok::Var(_) => Some(i - 1),
            Tok::RParen => {
                let mut depth = 0;
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    match toks[j] {
                        Tok::RParen => depth += 1,
                        Tok::LParen => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if j > 0 && matches!(toks[j - 1], Tok::Fn(_)) {
                    j -= 1;
                }
                Some(j)
            }
            _ => None,
        }
    }

    fn push_operand(&mut self, t: Tok) {
        self.start_fresh_if_result();
        if self.last_is_operand() {
            self.toks.push(op_tok(Bin::Mul));
        }
        self.toks.push(t);
    }

    fn digit(&mut self, d: char) {
        self.start_fresh_if_result();
        if let Some(Tok::Num { s, .. }) = self.toks.last_mut() {
            let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
            if d == '.' {
                if !s.contains('.') {
                    if s.is_empty() {
                        s.push('0');
                    }
                    s.push('.');
                }
            } else if s == "0" {
                *s = d.to_string();
            } else if digits < self.max_digits {
                s.push(d);
            }
            return;
        }
        if self.last_is_operand() {
            self.toks.push(op_tok(Bin::Mul));
        }
        let s = if d == '.' { "0.".to_string() } else { d.to_string() };
        self.toks.push(Tok::Num { s, neg: false });
    }

    fn binary(&mut self, b: Bin) {
        self.continue_from_result();
        match self.toks.last() {
            None => {
                if b == Bin::Sub {
                    self.toks.push(Tok::Num { s: "0".into(), neg: false });
                } else {
                    self.toks.push(Tok::Num { s: "0".into(), neg: false });
                }
                self.toks.push(op_tok(b));
            }
            Some(Tok::Op(_)) => {
                self.toks.pop();
                if self.toks.is_empty() {
                    self.toks.push(Tok::Num { s: "0".into(), neg: false });
                }
                self.toks.push(op_tok(b));
            }
            Some(Tok::LParen) | Some(Tok::Fn(_)) => {
                if b == Bin::Sub {
                    self.toks.push(op_tok(b));
                }
            }
            Some(_) => {
                if b == Bin::Ee && !matches!(self.toks.last(), Some(Tok::Num { .. })) {
                    return;
                }
                self.toks.push(op_tok(b));
            }
        }
    }

    fn postfix(&mut self, p: Post) {
        self.continue_from_result();
        if self.toks.is_empty() {
            self.toks.push(Tok::Num { s: "0".into(), neg: false });
        }
        if self.last_is_operand() {
            self.toks.push(Tok::Post(p));
        }
    }

    fn func(&mut self, f: Fun) {
        if self.result.is_some() {
            self.continue_from_result();
        }
        if let Some(st) = Calc::operand_start(&self.toks).filter(|_| self.last_is_operand()) {
            self.toks.insert(st, Tok::LParen);
            self.toks.insert(st, Tok::Fn(f));
            self.toks.push(Tok::RParen);
        } else {
            if self.last_is_operand() {
                self.toks.push(op_tok(Bin::Mul));
            }
            self.toks.push(Tok::Fn(f));
            self.toks.push(Tok::LParen);
        }
    }

    fn negate(&mut self) {
        if let Some(r) = self.result.take() {
            self.toks.clear();
            if let Some(v) = r.value {
                self.toks.push(Tok::Num { s: plain(v.abs()), neg: v > 0.0 });
            }
            return;
        }
        match self.toks.last_mut() {
            Some(Tok::Num { neg, .. }) => *neg = !*neg,
            Some(Tok::Val(v)) => *v = -*v,
            Some(t) if t.is_operand_end() => {
                if let Some(st) = Calc::operand_start(&self.toks) {
                    // -(…) twice gives the operand back
                    if matches!(self.toks.get(st), Some(Tok::Fn(Fun::Neg))) {
                        self.toks.remove(st);
                        self.toks.remove(st);
                        self.toks.pop();
                    } else {
                        self.toks.insert(st, Tok::LParen);
                        self.toks.insert(st, Tok::Fn(Fun::Neg));
                        self.toks.push(Tok::RParen);
                    }
                }
            }
            _ => {
                if self.last_is_operand() {
                    self.toks.push(op_tok(Bin::Mul));
                }
                self.toks.push(Tok::Num { s: String::new(), neg: true });
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(r) = self.result.take() {
            self.toks.clear();
            self.last_op = None;
            if let Some(v) = r.value {
                let s = plain(v.abs());
                if s.contains('e') {
                    return;
                }
                self.toks.push(Tok::Num { s, neg: v < 0.0 });
            } else {
                return;
            }
        }
        match self.toks.last_mut() {
            Some(Tok::Num { s, .. }) => {
                s.pop();
                if s.is_empty() {
                    self.toks.pop();
                }
            }
            Some(Tok::LParen) => {
                self.toks.pop();
                if matches!(self.toks.last(), Some(Tok::Fn(_))) {
                    self.toks.pop();
                }
            }
            Some(_) => {
                self.toks.pop();
            }
            None => {}
        }
    }

    pub fn clear_entry(&mut self) {
        if self.result.is_some() {
            return self.all_clear();
        }
        if matches!(self.toks.last(), Some(Tok::Num { .. })) {
            self.toks.pop();
        } else {
            self.all_clear();
        }
    }

    pub fn all_clear(&mut self) {
        self.toks.clear();
        self.result = None;
        self.last_op = None;
    }

    /// The current value: the result, or the expression so far.
    pub fn value(&self) -> f64 {
        if let Some(r) = &self.result {
            return r.value.unwrap_or(0.0);
        }
        expr::eval(&self.toks, &self.ctx()).unwrap_or(0.0)
    }

    fn equals(&mut self) -> Option<Done> {
        if let Some(r) = &self.result {
            let (b, c) = self.last_op?;
            let a = r.value?;
            let expr = vec![Tok::Val(a), Tok::Op(b), Tok::Val(c)];
            let value = expr::apply_bin(b, a, c).ok().filter(|v| v.is_finite());
            self.result = Some(Shown { expr: expr.clone(), value });
            return Some(Done { expr, value });
        }
        if self.toks.is_empty() {
            return None;
        }
        let mut t = self.toks.clone();
        while matches!(t.last(), Some(Tok::Op(_)) | Some(Tok::Fn(_)) | Some(Tok::LParen)) {
            if matches!(t.pop(), Some(Tok::LParen)) && matches!(t.last(), Some(Tok::Fn(_))) {
                t.pop();
            }
        }
        if t.is_empty() {
            return None;
        }
        let open = t.iter().fold(0i32, |n, x| match x {
            Tok::LParen => n + 1,
            Tok::RParen => n - 1,
            _ => n,
        });
        for _ in 0..open {
            t.push(Tok::RParen);
        }
        let value = expr::eval(&t, &self.ctx()).ok();
        self.last_op = expr::last_operation(&t, &self.ctx());
        self.result = Some(Shown { expr: t.clone(), value });
        self.toks.clear();
        Some(Done { expr: t, value })
    }

    /// Loads a calculation from the history (its expression on the small line, result big).
    pub fn load(&mut self, expr: Vec<Tok>, value: Option<f64>) {
        self.toks.clear();
        self.last_op = expr::last_operation(&expr, &self.ctx());
        self.result = Some(Shown { expr, value });
    }

    /// Puts a number in as if it had been typed (paste, history value, conversions).
    pub fn enter_value(&mut self, v: f64) {
        self.start_fresh_if_result();
        if matches!(self.toks.last(), Some(Tok::Num { .. })) {
            self.toks.pop();
        }
        if self.last_is_operand() {
            self.toks.push(op_tok(Bin::Mul));
        }
        let s = plain(v.abs());
        if s.contains('e') {
            self.toks.push(Tok::Val(v));
        } else {
            self.toks.push(Tok::Num { s, neg: v < 0.0 });
        }
    }

    /// Presses a key by id. Returns a finished calculation when "=" completes one.
    pub fn press(&mut self, key: &str) -> Option<Done> {
        let second = self.second;
        match key {
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => self.digit(key.chars().next().unwrap()),
            "dot" => self.digit('.'),
            "add" => self.binary(Bin::Add),
            "sub" => self.binary(Bin::Sub),
            "mul" => self.binary(Bin::Mul),
            "div" => self.binary(Bin::Div),
            "eq" => return self.equals(),
            "pct" => self.postfix(Post::Pct),
            "neg" => self.negate(),
            "ac" => {
                if self.clear_label() == "C" {
                    self.clear_entry()
                } else {
                    self.all_clear()
                }
            }
            "allclear" => self.all_clear(),
            "back" => self.backspace(),
            "lp" => {
                self.start_fresh_if_result();
                if self.last_is_operand() {
                    self.toks.push(op_tok(Bin::Mul));
                }
                self.toks.push(Tok::LParen);
            }
            "rp" => {
                if self.result.is_none() && self.open_parens() > 0 && self.last_is_operand() {
                    self.toks.push(Tok::RParen);
                }
            }
            "mc" => self.memory = 0.0,
            "mp" => self.memory += self.value(),
            "mm" => self.memory -= self.value(),
            "mr" => {
                let m = self.memory;
                self.push_operand(Tok::Val(m));
            }
            "2nd" => self.second = !self.second,
            "sq" => self.postfix(Post::Sq),
            "cube" => self.postfix(Post::Cube),
            "pow" => self.binary(Bin::Pow),
            "exp" => {
                if second {
                    self.binary(Bin::PowRev)
                } else {
                    self.func(Fun::Exp)
                }
            }
            "p10" => self.func(if second { Fun::Exp2 } else { Fun::Exp10 }),
            "recip" => self.postfix(Post::Recip),
            "sqrt" => self.func(Fun::Sqrt),
            "cbrt" => self.func(Fun::Cbrt),
            "root" => self.binary(Bin::Root),
            "ln" => {
                if second {
                    self.binary(Bin::LogY)
                } else {
                    self.func(Fun::Ln)
                }
            }
            "log10" => self.func(if second { Fun::Log2 } else { Fun::Log10 }),
            "fact" => self.postfix(Post::Fact),
            "sin" => self.func(if second { Fun::Asin } else { Fun::Sin }),
            "cos" => self.func(if second { Fun::Acos } else { Fun::Cos }),
            "tan" => self.func(if second { Fun::Atan } else { Fun::Tan }),
            "sinh" => self.func(if second { Fun::Asinh } else { Fun::Sinh }),
            "cosh" => self.func(if second { Fun::Acosh } else { Fun::Cosh }),
            "tanh" => self.func(if second { Fun::Atanh } else { Fun::Tanh }),
            "e" => self.push_operand(Tok::E),
            "pi" => self.push_operand(Tok::Pi),
            "ee" => self.binary(Bin::Ee),
            "rand" => {
                let r = self.rand();
                self.push_operand(Tok::Val(r));
            }
            "rad" => self.rad = !self.rad,
            _ => {}
        }
        None
    }

    pub fn clear_label(&self) -> &'static str {
        if self.result.is_none() && matches!(self.toks.last(), Some(Tok::Num { .. })) {
            "C"
        } else {
            "AC"
        }
    }

    pub fn display(&self, style: NumberStyle, classic: bool) -> Display {
        let md = self.max_digits;
        let (main, expr, is_result, error, value) = if let Some(r) = &self.result {
            let main = match r.value {
                Some(v) => format_value(v, md, style),
                None => "Error".into(),
            };
            (vec![Seg { t: main, sup: false }], segments(&r.expr, style, md), true, r.value.is_none(), r.value.unwrap_or(0.0))
        } else if classic {
            let v = self.classic_value();
            let main = match self.toks.last() {
                Some(Tok::Num { s, neg }) => format!("{}{}", if *neg { "-" } else { "" }, group_typed(s, style)),
                _ => v.map(|v| format_value(v, md, style)).unwrap_or_else(|| "Error".into()),
            };
            (vec![Seg { t: main, sup: false }], vec![], false, v.is_none(), v.unwrap_or(0.0))
        } else if self.toks.is_empty() {
            (vec![Seg { t: "0".into(), sup: false }], vec![], false, false, 0.0)
        } else {
            (segments(&self.toks, style, md), vec![], false, false, self.value())
        };
        let pending_op = match self.toks.last() {
            Some(Tok::Op(Bin::Add)) => Some("add"),
            Some(Tok::Op(Bin::Sub)) => Some("sub"),
            Some(Tok::Op(Bin::Mul)) => Some("mul"),
            Some(Tok::Op(Bin::Div)) => Some("div"),
            _ => None,
        };
        Display {
            main,
            expr,
            is_result,
            error,
            clear_label: self.clear_label(),
            first_key: if self.result.is_none() && !self.toks.is_empty() { "back" } else { "ac" },
            rad: self.rad,
            second: self.second,
            memory: self.memory != 0.0,
            pending_op,
            value,
            empty: self.toks.is_empty() && self.result.is_none(),
        }
    }

    /// What a one-line calculator shows: the number being typed, or the running value.
    fn classic_value(&self) -> Option<f64> {
        let mut t = self.toks.clone();
        match t.last() {
            None => return Some(0.0),
            Some(Tok::Op(Bin::Mul | Bin::Div)) => {
                t.pop();
                let mut depth = 0;
                let mut k = 0;
                for (i, x) in t.iter().enumerate() {
                    match x {
                        Tok::LParen => depth += 1,
                        Tok::RParen => depth -= 1,
                        Tok::Op(Bin::Add | Bin::Sub) if depth == 0 && i > 0 => k = i + 1,
                        _ => {}
                    }
                }
                return expr::eval(&t[k..], &self.ctx()).ok();
            }
            Some(Tok::Op(_)) => {
                t.pop();
            }
            _ => {}
        }
        expr::eval(&t, &self.ctx()).ok()
    }
}

/// Renders tokens for the display, with superscripts for powers and eˣ-style functions.
pub fn segments(toks: &[Tok], style: NumberStyle, md: usize) -> Vec<Seg> {
    let n = toks.len();
    let mut sup = vec![false; n];
    let mut hide = vec![false; n];
    for i in 0..n {
        let span_start = i + 1;
        let mark = match &toks[i] {
            Tok::Op(Bin::Pow) => true,
            Tok::Fn(f) if f.sup_arg() => true,
            _ => false,
        };
        if !mark || span_start >= n {
            continue;
        }
        // the operand right after: a number/constant, or a parenthesised group (+ postfixes)
        let mut j = span_start;
        while j < n && matches!(toks[j], Tok::Op(Bin::Sub)) {
            j += 1;
        }
        if j < n && matches!(toks[j], Tok::Fn(_)) {
            j += 1;
        }
        let end = if j < n && matches!(toks[j], Tok::LParen) {
            let mut depth = 0;
            let mut k = j;
            let mut end = n;
            while k < n {
                match toks[k] {
                    Tok::LParen => depth += 1,
                    Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            end = k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            if matches!(toks[i], Tok::Fn(_)) && j == span_start {
                hide[j] = true;
                if end <= n && end > j + 1 && matches!(toks[end - 1], Tok::RParen) {
                    hide[end - 1] = true;
                }
            }
            end
        } else {
            (j + 1).min(n)
        };
        let mut end = end;
        while end < n && matches!(toks[end], Tok::Post(_)) {
            end += 1;
        }
        for s in sup.iter_mut().take(end).skip(span_start) {
            *s = true;
        }
    }
    let only = n == 1;
    let mut out: Vec<Seg> = Vec::new();
    let mut push = |t: String, s: bool| {
        if t.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(l) if l.sup == s => l.t.push_str(&t),
            _ => out.push(Seg { t, sup: s }),
        }
    };
    for (i, t) in toks.iter().enumerate() {
        if hide[i] {
            continue;
        }
        let s = sup[i];
        let text = match t {
            Tok::Num { s: d, neg } => {
                let body = if d.is_empty() { "0".to_string() } else { group_typed(d, style) };
                if *neg {
                    if only || i == n - 1 && d.is_empty() {
                        format!("-{body}")
                    } else {
                        format!("(-{body})")
                    }
                } else {
                    body
                }
            }
            Tok::Val(v) => {
                let f = format_value(*v, md.max(9), style);
                if *v < 0.0 && !only {
                    format!("({f})")
                } else {
                    f
                }
            }
            Tok::Pi => "π".into(),
            Tok::E => "e".into(),
            Tok::Op(b) => match b {
                Bin::Add => "+".into(),
                Bin::Sub => "−".into(),
                Bin::Mul => "×".into(),
                Bin::Div => "÷".into(),
                Bin::Pow => String::new(),
                Bin::PowRev => " y^x ".into(),
                Bin::Root => " yroot ".into(),
                Bin::LogY => " logy ".into(),
                Bin::Ee => "E".into(),
            },
            Tok::Fn(f) => f.name().to_string(),
            Tok::LParen => "(".into(),
            Tok::RParen => ")".into(),
            Tok::Post(p) => match p {
                Post::Sq => "²".into(),
                Post::Cube => "³".into(),
                Post::Fact => "!".into(),
                Post::Pct => "%".into(),
                Post::Recip => "⁻¹".into(),
            },
            Tok::Var(v) => v.clone(),
        };
        // ² and ³ are already raised; inside a superscript they stay as written
        push(text, s);
    }
    if out.is_empty() {
        out.push(Seg { t: "0".into(), sup: false });
    }
    out
}

pub fn plain_text(segs: &[Seg]) -> String {
    segs.iter().map(|s| if s.sup { format!("^{}", s.t) } else { s.t.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(keys: &[&str]) -> Calc {
        let mut c = Calc::default();
        for k in keys {
            c.press(k);
        }
        c
    }

    fn main_text(c: &Calc) -> String {
        plain_text(&c.display(NumberStyle::Comma, false).main)
    }

    #[test]
    fn typing_and_equals() {
        let c = run(&["1", "0", "2", "4", "mul", "4"]);
        assert_eq!(main_text(&c), "1,024×4");
        let mut c = c;
        c.press("eq");
        let d = c.display(NumberStyle::Comma, false);
        assert_eq!(plain_text(&d.main), "4,096");
        assert_eq!(plain_text(&d.expr), "1,024×4");
        c.press("eq");
        assert_eq!(main_text(&c), "16,384");
    }

    #[test]
    fn repeat_add() {
        let mut c = run(&["2", "add", "3", "eq", "eq"]);
        assert_eq!(main_text(&c), "8");
        c.press("add");
        c.press("2");
        assert_eq!(main_text(&c), "8+2");
    }

    #[test]
    fn functions_wrap_operand() {
        let c = run(&["8", "neg", "mul", "9", "add", "8", "2", "cos"]);
        assert_eq!(main_text(&c), "(-8)×9+cos(82)");
        let c = run(&["2", "exp"]);
        assert_eq!(main_text(&c), "e^2");
        let c = run(&["2", "pow", "1", "0"]);
        assert_eq!(main_text(&c), "2^10");
    }

    #[test]
    fn clear_and_back() {
        let mut c = run(&["1", "2", "add", "3", "4"]);
        assert_eq!(c.clear_label(), "C");
        c.press("ac");
        assert_eq!(main_text(&c), "12+");
        assert_eq!(c.clear_label(), "AC");
        c.press("back");
        assert_eq!(main_text(&c), "12");
        c.press("back");
        assert_eq!(main_text(&c), "1");
        c.press("ac");
        c.press("ac");
        assert_eq!(main_text(&c), "0");
    }

    #[test]
    fn digit_limit() {
        let c = run(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "1"]);
        assert_eq!(main_text(&c), "123,456,789");
    }

    #[test]
    fn percent_of_sum() {
        let mut c = run(&["5", "0", "add", "1", "0", "pct"]);
        c.press("eq");
        assert_eq!(main_text(&c), "55");
    }

    #[test]
    fn classic_shows_running_value() {
        let c = run(&["2", "add", "3", "mul"]);
        assert_eq!(plain_text(&c.display(NumberStyle::Comma, true).main), "3");
        let c = run(&["2", "add", "3", "add"]);
        assert_eq!(plain_text(&c.display(NumberStyle::Comma, true).main), "5");
    }
}
