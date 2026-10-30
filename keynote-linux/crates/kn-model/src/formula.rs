//! Typed table formulas (Keynote 15.4, Mac round 30): parsed to Keynote's postfix AST node
//! array, evaluated, and written back as text for the cell editor.
//!
//! Storage, as Keynote writes it: cell references relative to the formula's cell (or absolute
//! with `$`), parentheses kept as a LIST node, `-5` as NUMBER 5 then NEGATION, IF's branches
//! wrapped in BEGIN_THUNK / END_THUNK, numbers with their Decimal128 form alongside.

use std::collections::HashMap;

use kn_format::Msg;

use crate::table::CellValue;

/// A number as plain text: "5", "0.67", "2.5".
fn plain(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// A function Keynote knows, by its AST_function_node_index.
struct FnDef {
    name: &'static str,
    index: u64,
}

/// Function indices measured on the Mac (rounds 18, 30 and 31).
const FUNCTIONS: &[FnDef] = &[
    FnDef { name: "ABS", index: 1 },
    FnDef { name: "AND", index: 7 },
    FnDef { name: "AVERAGE", index: 15 },
    FnDef { name: "CEILING", index: 17 },
    FnDef { name: "CONCATENATE", index: 25 },
    FnDef { name: "COUNT", index: 30 },
    FnDef { name: "COUNTA", index: 31 },
    FnDef { name: "COUNTIF", index: 33 },
    FnDef { name: "DATE", index: 39 },
    FnDef { name: "DAY", index: 41 },
    FnDef { name: "FLOOR", index: 55 },
    FnDef { name: "IF", index: 62 },
    FnDef { name: "INDEX", index: 63 },
    FnDef { name: "INT", index: 65 },
    FnDef { name: "ISBLANK", index: 69 },
    FnDef { name: "LEFT", index: 76 },
    FnDef { name: "LEN", index: 77 },
    FnDef { name: "LOWER", index: 82 },
    FnDef { name: "MATCH", index: 83 },
    FnDef { name: "MAX", index: 84 },
    FnDef { name: "MEDIAN", index: 86 },
    FnDef { name: "MID", index: 87 },
    FnDef { name: "MIN", index: 88 },
    FnDef { name: "MOD", index: 92 },
    FnDef { name: "MONTH", index: 94 },
    FnDef { name: "NOT", index: 96 },
    FnDef { name: "NOW", index: 97 },
    FnDef { name: "OR", index: 102 },
    FnDef { name: "PI", index: 104 },
    FnDef { name: "POWER", index: 107 },
    FnDef { name: "PRODUCT", index: 113 },
    FnDef { name: "RAND", index: 118 },
    FnDef { name: "RIGHT", index: 124 },
    FnDef { name: "ROUND", index: 126 },
    FnDef { name: "ROUNDDOWN", index: 127 },
    FnDef { name: "ROUNDUP", index: 128 },
    FnDef { name: "SQRT", index: 139 },
    FnDef { name: "STDEV", index: 140 },
    FnDef { name: "SUMIF", index: 145 },
    FnDef { name: "SUMPRODUCT", index: 146 },
    FnDef { name: "TODAY", index: 154 },
    FnDef { name: "TRIM", index: 155 },
    FnDef { name: "UPPER", index: 158 },
    FnDef { name: "VLOOKUP", index: 165 },
    FnDef { name: "YEAR", index: 167 },
    FnDef { name: "SUM", index: 168 },
    FnDef { name: "AVERAGEIF", index: 234 },
    FnDef { name: "IFERROR", index: 235 },
    FnDef { name: "TEXTJOIN", index: 328 },
];

/// The functions Keydeck computes, then every other Keynote / Numbers function (parsed and
/// stored with its right index; Keynote computes those when it opens the file).
fn all_defs() -> &'static [FnDef] {
    static ALL: std::sync::OnceLock<Vec<FnDef>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<FnDef> = FUNCTIONS.iter().map(|f| FnDef { name: f.name, index: f.index }).collect();
        for &(name, index) in crate::functions_all::ALL_FUNCTIONS {
            if !v.iter().any(|f| f.name == name) {
                v.push(FnDef { name, index });
            }
        }
        v
    })
}

fn fn_by_name(name: &str) -> Option<&'static FnDef> {
    let n = name.to_ascii_uppercase();
    let n = if n == "AVG" { "AVERAGE".to_string() } else { n };
    all_defs().iter().find(|f| f.name == n)
}

fn fn_by_index(i: u64) -> Option<&'static FnDef> {
    all_defs().iter().find(|f| f.index == i)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

impl Op {
    fn node(self) -> &'static str {
        match self {
            Op::Add => "ADDITION_NODE",
            Op::Sub => "SUBTRACTION_NODE",
            Op::Mul => "MULTIPLICATION_NODE",
            Op::Div => "DIVISION_NODE",
            Op::Pow => "POWER_NODE",
            Op::Concat => "CONCATENATION_NODE",
            Op::Eq => "EQUAL_TO_NODE",
            Op::Ne => "NOT_EQUAL_TO_NODE",
            Op::Lt => "LESS_THAN_NODE",
            Op::Gt => "GREATER_THAN_NODE",
            Op::Le => "LESS_THAN_OR_EQUAL_TO_NODE",
            Op::Ge => "GREATER_THAN_OR_EQUAL_TO_NODE",
        }
    }

    fn from_node(s: &str) -> Option<Op> {
        [Op::Add, Op::Sub, Op::Mul, Op::Div, Op::Pow, Op::Concat, Op::Eq, Op::Ne, Op::Lt, Op::Gt, Op::Le, Op::Ge].into_iter().find(|o| o.node() == s)
    }

    /// How Keynote shows it (×, ÷, −).
    fn text(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "−",
            Op::Mul => "×",
            Op::Div => "÷",
            Op::Pow => "^",
            Op::Concat => "&",
            Op::Eq => "=",
            Op::Ne => "≠",
            Op::Lt => "<",
            Op::Gt => ">",
            Op::Le => "≤",
            Op::Ge => "≥",
        }
    }

    fn prec(self) -> u8 {
        match self {
            Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => 1,
            Op::Concat => 2,
            Op::Add | Op::Sub => 3,
            Op::Mul | Op::Div => 4,
            Op::Pow => 5,
        }
    }
}

/// A cell coordinate in a formula: relative to the formula's cell, or absolute (`$`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coord {
    pub v: i64,
    pub abs: bool,
}

impl Coord {
    fn at(self, base: u32) -> Option<u32> {
        let x = if self.abs { self.v } else { i64::from(base) + self.v };
        u32::try_from(x).ok()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Num(f64),
    Str(String),
    Bool(bool),
    Ref { row: Coord, col: Coord },
    Range { r0: Coord, c0: Coord, r1: Coord, c1: Coord },
    Bin(Op),
    Neg,
    Percent,
    List(u32),
    Func { index: u64, args: u32 },
    BeginThunk,
    EndThunk,
}

// ------------------------------------------------------------------------------- parsing

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Cell(Coord, Coord),
    Op(char),
    Open,
    Close,
    Comma,
    Colon,
}

/// "B2", "$B$2" -> (row, col) as written (absolute or not).
fn cell_tok(s: &str) -> Option<(u32, bool, u32, bool)> {
    let b = s.as_bytes();
    let mut i = 0;
    let col_abs = b.first() == Some(&b'$');
    if col_abs {
        i += 1;
    }
    let l0 = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == l0 || i - l0 > 3 {
        return None;
    }
    let letters = &s[l0..i];
    let row_abs = b.get(i) == Some(&b'$');
    if row_abs {
        i += 1;
    }
    let digits = &s[i..];
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col = letters.to_ascii_uppercase().bytes().fold(0u32, |a, c| a * 26 + u32::from(c - b'A' + 1)) - 1;
    let row = digits.parse::<u32>().ok()?.checked_sub(1)?;
    Some((row, row_abs, col, col_abs))
}

fn tokens(text: &str, row: u32, col: u32) -> Result<Vec<Tok>, String> {
    let s: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && s.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let st = i;
            while i < s.len() && (s[i].is_ascii_digit() || s[i] == '.') {
                i += 1;
            }
            if i < s.len() && (s[i] == 'e' || s[i] == 'E') && s.get(i + 1).is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+') {
                i += 2;
                while i < s.len() && s[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let t: String = s[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| format!("“{t}” isn't a number"))?));
            continue;
        }
        if c == '"' {
            let mut t = String::new();
            i += 1;
            loop {
                match s.get(i) {
                    None => return Err("a text in quotes isn't closed".into()),
                    Some('"') if s.get(i + 1) == Some(&'"') => {
                        t.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(ch) => {
                        t.push(*ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(t));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '$' || c == '_' {
            let st = i;
            while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == '$' || s[i] == '_' || s[i] == '.') {
                i += 1;
            }
            let w: String = s[st..i].iter().collect();
            match cell_tok(&w) {
                Some((r, ra, c2, ca)) => {
                    let rc = Coord { v: if ra { i64::from(r) } else { i64::from(r) - i64::from(row) }, abs: ra };
                    let cc = Coord { v: if ca { i64::from(c2) } else { i64::from(c2) - i64::from(col) }, abs: ca };
                    out.push(Tok::Cell(rc, cc));
                }
                None => out.push(Tok::Name(w)),
            }
            continue;
        }
        let two: String = s[i..(i + 2).min(s.len())].iter().collect();
        let op = match two.as_str() {
            "<=" => Some('≤'),
            ">=" => Some('≥'),
            "<>" => Some('≠'),
            _ => None,
        };
        if let Some(o) = op {
            out.push(Tok::Op(o));
            i += 2;
            continue;
        }
        out.push(match c {
            '(' => Tok::Open,
            ')' => Tok::Close,
            ',' | ';' => Tok::Comma,
            ':' => Tok::Colon,
            '+' | '-' | '−' | '*' | '×' | '/' | '÷' | '^' | '&' | '=' | '<' | '>' | '%' | '≠' | '≤' | '≥' => Tok::Op(c),
            _ => return Err(format!("“{c}” can't be used in a formula")),
        });
        i += 1;
    }
    Ok(out)
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
    out: Vec<Node>,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn binop(&self) -> Option<Op> {
        match self.peek()? {
            Tok::Op('+') => Some(Op::Add),
            Tok::Op('-' | '−') => Some(Op::Sub),
            Tok::Op('*' | '×') => Some(Op::Mul),
            Tok::Op('/' | '÷') => Some(Op::Div),
            Tok::Op('^') => Some(Op::Pow),
            Tok::Op('&') => Some(Op::Concat),
            Tok::Op('=') => Some(Op::Eq),
            Tok::Op('≠') => Some(Op::Ne),
            Tok::Op('<') => Some(Op::Lt),
            Tok::Op('>') => Some(Op::Gt),
            Tok::Op('≤') => Some(Op::Le),
            Tok::Op('≥') => Some(Op::Ge),
            _ => None,
        }
    }

    /// Precedence climbing; operators of equal precedence group left to right.
    fn expr(&mut self, min: u8) -> Result<(), String> {
        self.unary()?;
        while let Some(op) = self.binop() {
            if op.prec() < min {
                break;
            }
            self.i += 1;
            self.expr(op.prec() + 1)?;
            self.out.push(Node::Bin(op));
        }
        Ok(())
    }

    fn unary(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Tok::Op('-' | '−')) => {
                self.i += 1;
                self.unary()?;
                self.out.push(Node::Neg);
            }
            Some(Tok::Op('+')) => {
                self.i += 1;
                self.unary()?;
            }
            _ => self.postfix()?,
        }
        Ok(())
    }

    fn postfix(&mut self) -> Result<(), String> {
        self.primary()?;
        while self.peek() == Some(&Tok::Op('%')) {
            self.i += 1;
            self.out.push(Node::Percent);
        }
        Ok(())
    }

    fn primary(&mut self) -> Result<(), String> {
        let t = self.peek().cloned().ok_or("the formula ends too early")?;
        self.i += 1;
        match t {
            Tok::Num(v) => self.out.push(Node::Num(v)),
            Tok::Str(s) => self.out.push(Node::Str(s)),
            Tok::Cell(r, c) => {
                if self.peek() == Some(&Tok::Colon) {
                    self.i += 1;
                    let Some(Tok::Cell(r1, c1)) = self.peek().cloned() else { return Err("a range needs a cell after “:”".into()) };
                    self.i += 1;
                    self.out.push(Node::Range { r0: r, c0: c, r1, c1 });
                } else {
                    self.out.push(Node::Ref { row: r, col: c });
                }
            }
            Tok::Open => {
                self.expr(0)?;
                if self.peek() != Some(&Tok::Close) {
                    return Err("a “(” isn't closed".into());
                }
                self.i += 1;
                self.out.push(Node::List(1));
            }
            Tok::Name(n) if n.eq_ignore_ascii_case("TRUE") || n.eq_ignore_ascii_case("FALSE") => {
                if self.peek() == Some(&Tok::Open) && self.t.get(self.i + 1) == Some(&Tok::Close) {
                    self.i += 2;
                }
                self.out.push(Node::Bool(n.eq_ignore_ascii_case("TRUE")));
            }
            Tok::Name(n) => {
                let f = fn_by_name(&n).ok_or(format!("{} isn't a function Keydeck knows yet", n.to_ascii_uppercase()))?;
                if self.peek() != Some(&Tok::Open) {
                    return Err(format!("{} needs its arguments in parentheses", f.name));
                }
                self.i += 1;
                let mut args = 0;
                if self.peek() == Some(&Tok::Close) {
                    self.i += 1;
                } else {
                    loop {
                        // IF's branches are evaluated lazily: Keynote wraps them in thunks
                        let thunk = f.name == "IF" && args >= 1;
                        if thunk {
                            self.out.push(Node::BeginThunk);
                        }
                        self.expr(0)?;
                        if thunk {
                            self.out.push(Node::EndThunk);
                        }
                        args += 1;
                        match self.peek() {
                            Some(Tok::Comma) => self.i += 1,
                            Some(Tok::Close) => {
                                self.i += 1;
                                break;
                            }
                            _ => return Err(format!("{}( isn't closed", f.name)),
                        }
                    }
                }
                self.out.push(Node::Func { index: f.index, args });
            }
            other => return Err(format!("unexpected {other:?}")),
        }
        Ok(())
    }
}

/// "=B2+B3" typed into (row, col) -> Keynote's node array.
pub fn parse(text: &str, row: u32, col: u32) -> Result<Vec<Node>, String> {
    let body = text.trim().strip_prefix('=').ok_or("a formula starts with “=”")?;
    let mut p = Parser { t: tokens(body, row, col)?, i: 0, out: Vec::new() };
    if p.t.is_empty() {
        return Err("the formula is empty".into());
    }
    p.expr(0)?;
    if p.i < p.t.len() {
        return Err("there is something extra at the end of the formula".into());
    }
    Ok(p.out)
}

// ---------------------------------------------------------------------- storage (AST)

/// Keynote's Decimal128 fields of a number node: (low, high).
fn decimal_parts(v: f64) -> (u64, u64) {
    // shortest decimal that round-trips, as coefficient × 10^exp
    let s = format!("{}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let digits = format!("{int}{frac}");
    let coeff: u128 = digits.trim_start_matches('0').parse().unwrap_or(0);
    let exp = -(frac.len() as i64);
    let biased = (6176 + exp) as u64;
    let low = coeff as u64;
    let high = (biased << 49) | ((coeff >> 64) as u64) | if v < 0.0 { 1 << 63 } else { 0 };
    (low, high)
}

pub fn write_nodes(nodes: &[Node], out: &mut Msg) {
    let arr = out.msg_mut_or_create("AST_node_array");
    for n in nodes {
        let m = arr.push_new("AST_node");
        match n {
            Node::Num(v) => {
                m.set_enum("AST_node_type", "NUMBER_NODE");
                m.set_f64("AST_number_node_number", *v);
                let (lo, hi) = decimal_parts(*v);
                m.set_u64("AST_number_node_decimal_low", lo);
                m.set_u64("AST_number_node_decimal_high", hi);
            }
            Node::Str(s) => {
                m.set_enum("AST_node_type", "STRING_NODE");
                m.set_str("AST_string_node_string", s);
            }
            Node::Bool(b) => {
                m.set_enum("AST_node_type", "BOOLEAN_NODE");
                m.set_bool("AST_boolean_node_boolean", *b);
            }
            Node::Ref { row, col } => {
                m.set_enum("AST_node_type", "CELL_REFERENCE_NODE");
                let c = m.msg_mut_or_create("AST_column");
                c.set_i64("column", col.v);
                c.set_bool("absolute", col.abs);
                let r = m.msg_mut_or_create("AST_row");
                r.set_i64("row", row.v);
                r.set_bool("absolute", row.abs);
            }
            Node::Range { r0, c0, r1, c1 } => {
                m.set_enum("AST_node_type", "COLON_TRACT_NODE");
                let sticky = m.msg_mut_or_create("AST_sticky_bits");
                sticky.set_bool("begin_row_is_absolute", r0.abs);
                sticky.set_bool("begin_column_is_absolute", c0.abs);
                sticky.set_bool("end_row_is_absolute", r1.abs);
                sticky.set_bool("end_column_is_absolute", c1.abs);
                let ct = m.msg_mut_or_create("AST_colon_tract");
                let span = |m: &mut Msg, a: Coord, b: Coord| {
                    let (lo, hi) = if a.v <= b.v { (a.v, b.v) } else { (b.v, a.v) };
                    m.set_i64("range_begin", lo);
                    if hi != lo {
                        m.set_i64("range_end", hi);
                    }
                };
                span(ct.push_new(if c0.abs { "absolute_column" } else { "relative_column" }), *c0, *c1);
                span(ct.push_new(if r0.abs { "absolute_row" } else { "relative_row" }), *r0, *r1);
                ct.set_bool("preserve_rectangular", true);
            }
            Node::Bin(op) => m.set_enum("AST_node_type", op.node()),
            Node::Neg => m.set_enum("AST_node_type", "NEGATION_NODE"),
            Node::Percent => m.set_enum("AST_node_type", "PERCENT_NODE"),
            Node::List(n) => {
                m.set_enum("AST_node_type", "LIST_NODE");
                m.set_u64("AST_list_node_numArgs", u64::from(*n));
            }
            Node::Func { index, args } => {
                m.set_enum("AST_node_type", "FUNCTION_NODE");
                m.set_u64("AST_function_node_index", *index);
                m.set_u64("AST_function_node_numArgs", u64::from(*args));
            }
            Node::BeginThunk => m.set_enum("AST_node_type", "BEGIN_THUNK_NODE"),
            Node::EndThunk => m.set_enum("AST_node_type", "END_THUNK_NODE"),
        }
    }
}

/// A stored formula's nodes (None when it uses a node this engine doesn't know).
pub fn read_nodes(f: &Msg) -> Option<Vec<Node>> {
    let mut out = Vec::new();
    for n in f.msg("AST_node_array")?.msgs("AST_node") {
        // a reference into another table or sheet ("Table 2::B2"): not read yet. Without this it
        // read as this table's own cell, and recalculation overwrote the right value; unread, the
        // formula keeps the value Keynote / Numbers stored
        if n.has("AST_cross_table_reference_extra_info") {
            return None;
        }
        let t = n.enum_name("AST_node_type")?;
        out.push(match t {
            "NUMBER_NODE" => Node::Num(n.f64("AST_number_node_number")?),
            "STRING_NODE" => Node::Str(n.str("AST_string_node_string").unwrap_or("").to_string()),
            "BOOLEAN_NODE" => Node::Bool(n.bool("AST_boolean_node_boolean").unwrap_or(false)),
            "CELL_REFERENCE_NODE" => {
                let c = n.msg("AST_column")?;
                let r = n.msg("AST_row")?;
                Node::Ref { row: Coord { v: r.i64("row")?, abs: r.bool("absolute").unwrap_or(false) }, col: Coord { v: c.i64("column")?, abs: c.bool("absolute").unwrap_or(false) } }
            }
            "COLON_TRACT_NODE" => {
                let ct = n.msg("AST_colon_tract")?;
                let span = |abs: &str, rel: &str| -> Option<(Coord, Coord)> {
                    let (m, a) = match ct.msg(abs) {
                        Some(m) => (m, true),
                        None => (ct.msg(rel)?, false),
                    };
                    let b = m.i64("range_begin")?;
                    let e = m.i64("range_end").unwrap_or(b);
                    Some((Coord { v: b, abs: a }, Coord { v: e, abs: a }))
                };
                let (r0, r1) = span("absolute_row", "relative_row")?;
                let (c0, c1) = span("absolute_column", "relative_column")?;
                Node::Range { r0, c0, r1, c1 }
            }
            "NEGATION_NODE" => Node::Neg,
            "PERCENT_NODE" => Node::Percent,
            "LIST_NODE" => Node::List(n.u32("AST_list_node_numArgs").unwrap_or(1)),
            "FUNCTION_NODE" => Node::Func { index: n.u64("AST_function_node_index")?, args: n.u32("AST_function_node_numArgs").unwrap_or(0) },
            "BEGIN_THUNK_NODE" => Node::BeginThunk,
            "END_THUNK_NODE" => Node::EndThunk,
            other => Node::Bin(Op::from_node(other)?),
        });
    }
    Some(out)
}

// ------------------------------------------------------------------------- evaluation

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Num(f64),
    Text(String),
    Bool(bool),
    /// A cell or range (for functions that read ranges), as absolute cells.
    Cells(Vec<(u32, u32)>),
    Err(String),
}

fn cell_value(values: &HashMap<(u32, u32), CellValue>, rc: (u32, u32)) -> Value {
    match values.get(&rc) {
        Some(CellValue::Number(x) | CellValue::Duration(x)) => Value::Num(*x),
        Some(CellValue::Text(t)) => Value::Text(t.clone()),
        Some(CellValue::Bool(b)) => Value::Bool(*b),
        _ => Value::Num(0.0),
    }
}

fn num(v: &Value, values: &HashMap<(u32, u32), CellValue>) -> Result<f64, String> {
    match v {
        Value::Num(x) => Ok(*x),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Text(t) => t.trim().parse().map_err(|_| format!("“{t}” isn't a number")),
        Value::Cells(c) if c.len() == 1 => num(&cell_value(values, c[0]), values),
        Value::Cells(_) => Err("a range can't be used here".into()),
        Value::Err(e) => Err(e.clone()),
    }
}

fn text(v: &Value, values: &HashMap<(u32, u32), CellValue>) -> String {
    match v {
        Value::Num(x) => plain(*x),
        Value::Text(t) => t.clone(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Cells(c) if c.len() == 1 => text(&cell_value(values, c[0]), values),
        _ => String::new(),
    }
}

/// The numbers a function argument stands for (ranges: the numbers in them).
fn numbers(v: &Value, values: &HashMap<(u32, u32), CellValue>, out: &mut Vec<f64>) {
    match v {
        Value::Cells(cells) => {
            for rc in cells {
                if let Some(CellValue::Number(x) | CellValue::Duration(x)) = values.get(rc) {
                    out.push(*x);
                }
            }
        }
        Value::Num(x) => out.push(*x),
        Value::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
        _ => {}
    }
}

/// A criteria string of SUMIF / COUNTIF / AVERAGEIF (">2", "<=3", "<>a", "abc", 5) applied to a value.
fn meets(criteria: &Value, v: &CellValue, values: &HashMap<(u32, u32), CellValue>) -> bool {
    let c = text(criteria, values);
    let (op, rest) = ["<=", ">=", "<>", "=", "<", ">"].iter().find_map(|o| c.strip_prefix(o).map(|r| (*o, r))).unwrap_or(("=", c.as_str()));
    let vn = match v {
        CellValue::Number(x) | CellValue::Duration(x) => Some(*x),
        _ => None,
    };
    match (rest.trim().parse::<f64>(), vn) {
        (Ok(n), Some(x)) => match op {
            "<=" => x <= n,
            ">=" => x >= n,
            "<>" => x != n,
            "<" => x < n,
            ">" => x > n,
            _ => x == n,
        },
        _ => {
            let vt = match v {
                CellValue::Text(t) => t.to_lowercase(),
                CellValue::Number(x) => plain(*x),
                _ => String::new(),
            };
            let eq = vt == rest.to_lowercase();
            if op == "<>" { !eq } else { op == "=" && eq }
        }
    }
}

/// Days since 1970 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A date as Keynote shows it by default ("1/2/26").
fn date_text(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{m}/{d}/{:02}", y.rem_euclid(100))
}

fn call(f: &FnDef, args: Vec<Value>, values: &HashMap<(u32, u32), CellValue>) -> Value {
    let all = |args: &[Value]| {
        let mut v = Vec::new();
        for a in args {
            numbers(a, values, &mut v);
        }
        v
    };
    // a single value (a one-cell reference stands for its value)
    let one = |i: usize| -> Value {
        match args.get(i) {
            Some(Value::Cells(c)) if c.len() == 1 => cell_value(values, c[0]),
            Some(v) => v.clone(),
            None => Value::Err(format!("{} needs more arguments", f.name)),
        }
    };
    let n = |i: usize| num(&one(i), values);
    let s = |i: usize| text(&one(i), values);
    let truth = |v: &Value| -> Result<bool, String> {
        match v {
            Value::Bool(b) => Ok(*b),
            Value::Cells(c) => Ok(c.iter().all(|rc| num(&cell_value(values, *rc), values).map(|x| x != 0.0).unwrap_or(false))),
            v => num(v, values).map(|x| x != 0.0),
        }
    };
    let cells_of = |i: usize| -> Vec<(u32, u32)> {
        match args.get(i) {
            Some(Value::Cells(c)) => c.clone(),
            _ => Vec::new(),
        }
    };
    let r = (|| -> Result<Value, String> {
        Ok(match f.name {
            "SUM" => Value::Num(all(&args).iter().sum()),
            "AVERAGE" => {
                let v = all(&args);
                if v.is_empty() {
                    return Err("AVERAGE needs at least one number".into());
                }
                Value::Num(v.iter().sum::<f64>() / v.len() as f64)
            }
            "MIN" => Value::Num(all(&args).into_iter().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))).unwrap_or(0.0)),
            "MAX" => Value::Num(all(&args).into_iter().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))).unwrap_or(0.0)),
            "COUNT" => Value::Num(all(&args).len() as f64),
            "COUNTA" => Value::Num(
                args.iter()
                    .map(|a| match a {
                        Value::Cells(c) => c.iter().filter(|rc| values.get(rc).is_some_and(|v| !matches!(v, CellValue::Empty))).count(),
                        _ => 1,
                    })
                    .sum::<usize>() as f64,
            ),
            "PRODUCT" => {
                let v = all(&args);
                Value::Num(if v.is_empty() { 0.0 } else { v.iter().product() })
            }
            "MEDIAN" => {
                let mut v = all(&args);
                if v.is_empty() {
                    return Err("MEDIAN needs at least one number".into());
                }
                v.sort_by(|a, b| a.total_cmp(b));
                let m = v.len() / 2;
                Value::Num(if v.len() % 2 == 1 { v[m] } else { (v[m - 1] + v[m]) / 2.0 })
            }
            "STDEV" => {
                let v = all(&args);
                if v.len() < 2 {
                    return Err("STDEV needs at least two numbers".into());
                }
                let mean = v.iter().sum::<f64>() / v.len() as f64;
                Value::Num((v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (v.len() - 1) as f64).sqrt())
            }
            "SUMPRODUCT" => {
                let lists: Vec<Vec<(u32, u32)>> = (0..args.len()).map(cells_of).collect();
                let len = lists.first().map(|l| l.len()).unwrap_or(0);
                if lists.iter().any(|l| l.len() != len) {
                    return Err("SUMPRODUCT needs ranges of the same size".into());
                }
                Value::Num((0..len).map(|k| lists.iter().map(|l| num(&cell_value(values, l[k]), values).unwrap_or(0.0)).product::<f64>()).sum())
            }
            "ABS" => Value::Num(n(0)?.abs()),
            "INT" => Value::Num(n(0)?.floor()),
            "SQRT" => {
                let x = n(0)?;
                if x < 0.0 {
                    return Err("SQRT of a negative number".into());
                }
                Value::Num(x.sqrt())
            }
            "POWER" => Value::Num(n(0)?.powf(n(1)?)),
            "MOD" => {
                let (a, b) = (n(0)?, n(1)?);
                if b == 0.0 {
                    return Err("division by zero".into());
                }
                Value::Num(a - b * (a / b).floor())
            }
            "ROUND" | "ROUNDUP" | "ROUNDDOWN" => {
                let x = n(0)?;
                let d = if args.len() > 1 { n(1)?.round() as i32 } else { 0 };
                let k = 10f64.powi(d);
                let y = x * k;
                let r = match f.name {
                    "ROUNDUP" => y.abs().ceil() * y.signum(),
                    "ROUNDDOWN" => y.abs().floor() * y.signum(),
                    _ => y.round(),
                };
                Value::Num(r / k)
            }
            "CEILING" | "FLOOR" => {
                let x = n(0)?;
                let m = if args.len() > 1 { n(1)? } else { 1.0 };
                if m == 0.0 {
                    return Ok(Value::Num(0.0));
                }
                Value::Num(if f.name == "CEILING" { (x / m).ceil() * m } else { (x / m).floor() * m })
            }
            "PI" => Value::Num(std::f64::consts::PI),
            "RAND" => {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(1);
                Value::Num(f64::from(t.wrapping_mul(2654435761) % 1_000_000) / 1_000_000.0)
            }
            "AND" => Value::Bool(args.iter().map(&truth).collect::<Result<Vec<_>, _>>()?.into_iter().all(|b| b)),
            "OR" => Value::Bool(args.iter().map(&truth).collect::<Result<Vec<_>, _>>()?.into_iter().any(|b| b)),
            "NOT" => Value::Bool(!truth(&one(0))?),
            "ISBLANK" => Value::Bool(cells_of(0).iter().all(|rc| values.get(rc).is_none_or(|v| matches!(v, CellValue::Empty)))),
            "IFERROR" => match one(0) {
                Value::Err(_) => one(1),
                v => v,
            },
            "CONCATENATE" => Value::Text((0..args.len()).map(s).collect()),
            "TEXTJOIN" => {
                let delim = s(0);
                let skip = truth(&one(1))?;
                let mut parts = Vec::new();
                for a in args.iter().skip(2) {
                    match a {
                        Value::Cells(c) => {
                            for rc in c {
                                let t = text(&cell_value(values, *rc), values);
                                let empty = values.get(rc).is_none_or(|v| matches!(v, CellValue::Empty));
                                if !(skip && empty) {
                                    parts.push(t);
                                }
                            }
                        }
                        v => parts.push(text(v, values)),
                    }
                }
                Value::Text(parts.join(&delim))
            }
            "LEN" => Value::Num(s(0).chars().count() as f64),
            "LEFT" | "RIGHT" => {
                let t: Vec<char> = s(0).chars().collect();
                let k = if args.len() > 1 { n(1)?.max(0.0) as usize } else { 1 }.min(t.len());
                Value::Text(if f.name == "LEFT" { t[..k].iter().collect() } else { t[t.len() - k..].iter().collect() })
            }
            "MID" => {
                let t: Vec<char> = s(0).chars().collect();
                let start = (n(1)?.max(1.0) as usize - 1).min(t.len());
                let k = (n(2)?.max(0.0) as usize).min(t.len() - start);
                Value::Text(t[start..start + k].iter().collect())
            }
            "UPPER" => Value::Text(s(0).to_uppercase()),
            "LOWER" => Value::Text(s(0).to_lowercase()),
            "TRIM" => Value::Text(s(0).split_whitespace().collect::<Vec<_>>().join(" ")),
            "SUMIF" | "COUNTIF" | "AVERAGEIF" => {
                let crit = one(1);
                let test = cells_of(0);
                let sum_cells = if args.len() > 2 { cells_of(2) } else { test.clone() };
                let mut hits = Vec::new();
                for (k, rc) in test.iter().enumerate() {
                    if meets(&crit, values.get(rc).unwrap_or(&CellValue::Empty), values) {
                        hits.push(sum_cells.get(k).copied().unwrap_or(*rc));
                    }
                }
                let nums: Vec<f64> = hits.iter().filter_map(|rc| num(&cell_value(values, *rc), values).ok()).collect();
                match f.name {
                    "COUNTIF" => Value::Num(hits.len() as f64),
                    "SUMIF" => Value::Num(nums.iter().sum()),
                    _ => {
                        if nums.is_empty() {
                            return Err("AVERAGEIF found nothing to average".into());
                        }
                        Value::Num(nums.iter().sum::<f64>() / nums.len() as f64)
                    }
                }
            }
            "INDEX" => {
                let c = cells_of(0);
                let (r0, c0) = c.first().copied().ok_or("INDEX needs a range")?;
                let (r1, c1) = c.last().copied().unwrap_or((r0, c0));
                let width = c1 - c0 + 1;
                let (row, col) = (n(1)? as i64, if args.len() > 2 { n(2)? as i64 } else { 1 });
                // a single column (or row) range takes the one index along it
                let (row, col) = if width == 1 || r1 != r0 { (row, col) } else { (1, row) };
                if row < 1 || col < 1 || row as u32 > r1 - r0 + 1 || col as u32 > width {
                    return Err("INDEX is outside the range".into());
                }
                cell_value(values, (r0 + row as u32 - 1, c0 + col as u32 - 1))
            }
            "MATCH" => {
                let want = one(0);
                let c = cells_of(1);
                let exact = args.len() < 3 || n(2)? == 0.0;
                let wn = num(&want, values).ok();
                let pos = c.iter().position(|rc| {
                    let v = cell_value(values, *rc);
                    match (wn, num(&v, values).ok()) {
                        (Some(a), Some(b)) if exact => a == b,
                        (Some(a), Some(b)) => b <= a,
                        _ => text(&v, values).eq_ignore_ascii_case(&text(&want, values)),
                    }
                });
                Value::Num((pos.ok_or("MATCH found no match")? + 1) as f64)
            }
            "VLOOKUP" => {
                let want = one(0);
                let c = cells_of(1);
                let (r0, c0) = c.first().copied().ok_or("VLOOKUP needs a range")?;
                let (r1, _) = c.last().copied().unwrap_or((r0, c0));
                let col = n(2)? as u32;
                let exact = args.len() > 3 && !truth(&one(3))?;
                let wn = num(&want, values).ok();
                let mut found = None;
                for r in r0..=r1 {
                    let v = cell_value(values, (r, c0));
                    let hit = match (wn, num(&v, values).ok()) {
                        (Some(a), Some(b)) if exact => a == b,
                        (Some(a), Some(b)) => b <= a,
                        _ => text(&v, values).eq_ignore_ascii_case(&text(&want, values)),
                    };
                    if hit {
                        found = Some(r);
                        if exact {
                            break;
                        }
                    }
                }
                let r = found.ok_or("VLOOKUP found no match")?;
                cell_value(values, (r, c0 + col.max(1) - 1))
            }
            "DATE" => {
                let d = days_from_civil(n(0)? as i64, n(1)? as i64, n(2)? as i64);
                Value::Text(date_text(d))
            }
            "TODAY" | "NOW" => {
                let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
                Value::Text(date_text(secs.div_euclid(86400)))
            }
            "YEAR" | "MONTH" | "DAY" => match one(0) {
                Value::Cells(_) | Value::Num(_) | Value::Bool(_) => return Err("missing value".into()),
                v => {
                    // a date typed as text, "1/2/26" or "2026-01-02"
                    let t = text(&v, values);
                    let parts: Vec<i64> = t.split(['/', '-']).filter_map(|p| p.trim().parse().ok()).collect();
                    let (y, m, d) = match parts.as_slice() {
                        [y, m, d] if *y > 31 => (*y, *m, *d),
                        [m, d, y] => (if *y < 100 { 2000 + *y } else { *y }, *m, *d),
                        _ => return Err("missing value".into()),
                    };
                    Value::Num(match f.name {
                        "YEAR" => y,
                        "MONTH" => m,
                        _ => d,
                    } as f64)
                }
            },
            _ => return Err(format!("{} can't be calculated yet", f.name)),
        })
    })();
    r.unwrap_or_else(Value::Err)
}

/// Quotes a document's remote data store holds (STOCK / STOCKH): (function index 298 / 299,
/// symbol in capitals, attribute, date as seconds since 2001 for STOCKH) -> value.
pub type RemoteValues = HashMap<(u32, String, u32, Option<i64>), Value>;

/// STOCK / STOCKH attribute names and Numbers' numbers for them (Mac N23: matched against
/// Numbers' own STOCK() results and its stored quotes).
pub const STOCK_ATTRIBUTES: &[(&str, u32)] = &[
    ("price", 24),
    ("change", 7),
    ("percent change", 22),
    ("open", 20),
    ("high", 9),
    ("low", 10),
    ("volume", 25),
    ("market cap", 17),
    ("52-week high", 15),
    ("52-week low", 16),
    ("eps", 13),
    ("yield", 12),
    ("beta", 4),
    ("name", 18),
    ("exchange", 14),
    ("currency", 8),
    ("symbol", 26),
    ("close", 23),
    // the popover's other labels, accepted by name too (N24)
    ("1-year target", 19),
    ("avg. 3-month volume", 3),
    ("annual dividend", 11),
    ("p/e ratio", 21),
    ("previous close", 23),
];

/// STOCK's attribute codes (=STOCK("AAPL",0) is the price), the label Numbers' stock popover
/// gives each (its 22 items, in this order) and the stored attribute the document keeps the
/// value under. Codes 13, 14, 17 and 18 aren't used (Numbers gives an error). Every code and
/// stored id is confirmed by Mac rounds N23 / N24 (a STOCK() per code, matched by value with the
/// file's stored quotes; price 24 vs previous close 23 by N23's STOCK(…,"price")).
pub const STOCK_ARGS: &[(u32, &str, Option<u32>)] = &[
    (0, "Price", Some(24)),
    (1, "Name", Some(18)),
    (2, "Change", Some(7)),
    (3, "Percent Change", Some(22)),
    (4, "Open", Some(20)),
    (5, "High", Some(9)),
    (6, "Low", Some(10)),
    (7, "Market Cap", Some(17)),
    (8, "Volume", Some(25)),
    (9, "Yield", Some(12)),
    (10, "1-Year Target", Some(19)),
    (11, "52-Week High", Some(15)),
    (12, "52-Week Low", Some(16)),
    (15, "Avg. 3-Month Volume", Some(3)),
    (16, "Beta", Some(4)),
    (19, "Currency", Some(8)),
    (20, "Annual Dividend", Some(11)),
    (21, "EPS", Some(13)),
    (22, "Exchange", Some(14)),
    (23, "P/E Ratio", Some(21)),
    (24, "Previous Close", Some(23)),
    (25, "Symbol", Some(26)),
];

/// STOCKH's attribute codes (0 is the close; 5 and up give an error), N24.
pub const STOCKH_ARGS: &[(u32, &str, Option<u32>)] = &[(0, "Close", Some(23)), (1, "Open", Some(20)), (2, "High", Some(9)), (3, "Low", Some(10)), (4, "Volume", Some(25))];

/// An attribute's number from its name (any case).
pub fn stock_attribute(name: &str) -> Option<u32> {
    let n = name.trim().to_lowercase();
    STOCK_ATTRIBUTES.iter().find(|(a, _)| *a == n).map(|(_, k)| *k)
}

/// A date's day (days since 1970) as seconds since 2001-01-01, the remote data key's date.
pub fn date_key(days: i64) -> i64 {
    (days - 11_323) * 86_400
}

/// STOCK(symbol, attribute) / STOCKH(symbol, attribute, date): the document's stored quote.
fn stock(f: &FnDef, a: &[Value], values: &HashMap<(u32, u32), CellValue>, remote: &RemoteValues) -> Value {
    let one = |i: usize| -> Value {
        match a.get(i) {
            Some(Value::Cells(c)) if c.len() == 1 => cell_value(values, c[0]),
            Some(v) => v.clone(),
            None => Value::Err(format!("{} needs more arguments", f.name)),
        }
    };
    let symbol = text(&one(0), values).trim().to_uppercase();
    let attr = match one(1) {
        // a number is the attribute's place in Numbers' list
        Value::Num(x) if x >= 0.0 && x.fract() == 0.0 => {
            let args = if f.name == "STOCKH" { STOCKH_ARGS } else { STOCK_ARGS };
            args.iter().find(|(code, _, _)| *code as f64 == x).and_then(|(_, _, id)| *id)
        }
        Value::Num(_) => None,
        v => stock_attribute(&text(&v, values)),
    };
    let Some(attr) = attr else { return Value::Err(format!("{} doesn't know that attribute", f.name)) };
    let (func, date) = if f.name == "STOCKH" {
        // the date as DATE() gives it (text), or typed "2026-10-16" / "10/16/26"
        let t = text(&one(2), values);
        let parts: Vec<i64> = t.split(['/', '-']).filter_map(|p| p.trim().parse().ok()).collect();
        let (y, m, d) = match parts.as_slice() {
            [y, m, d] if *y > 31 => (*y, *m, *d),
            [m, d, y] => (if *y < 100 { 2000 + *y } else { *y }, *m, *d),
            _ => return Value::Err("STOCKH needs a date".into()),
        };
        (299, Some(date_key(days_from_civil(y, m, d))))
    } else {
        (298, None)
    };
    remote.get(&(func, symbol, attr, date)).cloned().unwrap_or_else(|| Value::Err("the quote hasn't been fetched yet".into()))
}

/// Evaluate a formula in (row, col).
pub fn eval(nodes: &[Node], row: u32, col: u32, values: &HashMap<(u32, u32), CellValue>) -> Value {
    eval_with(nodes, row, col, values, &RemoteValues::new())
}

/// Evaluate a formula in (row, col), with the document's stored quotes for STOCK / STOCKH.
pub fn eval_with(nodes: &[Node], row: u32, col: u32, values: &HashMap<(u32, u32), CellValue>, remote: &RemoteValues) -> Value {
    let mut st: Vec<Value> = Vec::new();
    // thunks: the nodes of each are evaluated only when IF picks them
    let mut i = 0;
    let mut thunks: Vec<Vec<Node>> = Vec::new();
    while i < nodes.len() {
        let n = &nodes[i];
        i += 1;
        let v = match n {
            Node::BeginThunk => {
                let mut depth = 1;
                let st0 = i;
                while i < nodes.len() && depth > 0 {
                    match nodes[i] {
                        Node::BeginThunk => depth += 1,
                        Node::EndThunk => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                thunks.push(nodes[st0..i.saturating_sub(1)].to_vec());
                Value::Num(f64::from(thunks.len() as u32 - 1)) // placeholder: index of the thunk
            }
            Node::EndThunk => continue,
            Node::Num(x) => Value::Num(*x),
            Node::Str(s) => Value::Text(s.clone()),
            Node::Bool(b) => Value::Bool(*b),
            Node::Ref { row: r, col: c } => match (r.at(row), c.at(col)) {
                (Some(r), Some(c)) => Value::Cells(vec![(r, c)]),
                _ => Value::Err("the reference is outside the table".into()),
            },
            Node::Range { r0, c0, r1, c1 } => match (r0.at(row), r1.at(row), c0.at(col), c1.at(col)) {
                (Some(a), Some(b), Some(c), Some(d)) => {
                    let mut cells = Vec::new();
                    for r in a.min(b)..=a.max(b) {
                        for cc in c.min(d)..=c.max(d) {
                            cells.push((r, cc));
                        }
                    }
                    Value::Cells(cells)
                }
                _ => Value::Err("the range is outside the table".into()),
            },
            Node::List(_) => continue,
            Node::Neg => {
                let a = st.pop().unwrap_or(Value::Num(0.0));
                num(&a, values).map(|x| Value::Num(-x)).unwrap_or_else(Value::Err)
            }
            Node::Percent => {
                let a = st.pop().unwrap_or(Value::Num(0.0));
                num(&a, values).map(|x| Value::Num(x / 100.0)).unwrap_or_else(Value::Err)
            }
            Node::Bin(op) => {
                let b = st.pop().unwrap_or(Value::Num(0.0));
                let a = st.pop().unwrap_or(Value::Num(0.0));
                let r = (|| -> Result<Value, String> {
                    Ok(match op {
                        Op::Concat => Value::Text(text(&a, values) + &text(&b, values)),
                        Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                            let ord = match (num(&a, values), num(&b, values)) {
                                (Ok(x), Ok(y)) => x.partial_cmp(&y),
                                _ => Some(text(&a, values).to_lowercase().cmp(&text(&b, values).to_lowercase())),
                            };
                            let o = ord.ok_or("the values can't be compared")?;
                            Value::Bool(match op {
                                Op::Eq => o.is_eq(),
                                Op::Ne => o.is_ne(),
                                Op::Lt => o.is_lt(),
                                Op::Gt => o.is_gt(),
                                Op::Le => o.is_le(),
                                _ => o.is_ge(),
                            })
                        }
                        _ => {
                            let (x, y) = (num(&a, values)?, num(&b, values)?);
                            Value::Num(match op {
                                Op::Add => x + y,
                                Op::Sub => x - y,
                                Op::Mul => x * y,
                                Op::Div => {
                                    if y == 0.0 {
                                        return Err("division by zero".into());
                                    }
                                    x / y
                                }
                                _ => x.powf(y),
                            })
                        }
                    })
                })();
                r.unwrap_or_else(Value::Err)
            }
            Node::Func { index, args } => {
                let n = *args as usize;
                let at = st.len().saturating_sub(n);
                let a: Vec<Value> = st.split_off(at);
                let Some(f) = fn_by_index(*index) else { return Value::Err("an unknown function".into()) };
                if f.name == "IF" {
                    let cond = a.first().map(|c| num(c, values).map(|x| x != 0.0)).unwrap_or(Ok(false));
                    let pick = match cond {
                        Ok(true) => a.get(1),
                        Ok(false) => a.get(2),
                        Err(e) => return Value::Err(e),
                    };
                    match pick {
                        Some(Value::Num(k)) => {
                            let t = thunks.get(*k as usize).cloned().unwrap_or_default();
                            eval_with(&t, row, col, values, remote)
                        }
                        _ => Value::Bool(false),
                    }
                } else if f.name == "STOCK" || f.name == "STOCKH" {
                    stock(f, &a, values, remote)
                } else {
                    call(f, a, values)
                }
            }
        };
        st.push(v);
    }
    match st.pop() {
        Some(Value::Cells(c)) if c.len() == 1 => cell_value(values, c[0]),
        Some(Value::Cells(_)) => Value::Err("a formula can't show a whole range".into()),
        Some(v) => v,
        None => Value::Err("empty formula".into()),
    }
}

/// The cells a formula reads: single cells (Keynote's expanded edges) and ranges.
pub fn precedents(nodes: &[Node], row: u32, col: u32) -> (Vec<(u32, u32)>, Vec<(u32, u32, u32, u32)>) {
    let (mut cells, mut ranges) = (Vec::new(), Vec::new());
    for n in nodes {
        match n {
            Node::Ref { row: r, col: c } => {
                if let (Some(r), Some(c)) = (r.at(row), c.at(col)) {
                    cells.push((r, c));
                }
            }
            Node::Range { r0, c0, r1, c1 } => {
                if let (Some(a), Some(b), Some(c), Some(d)) = (r0.at(row), r1.at(row), c0.at(col), c1.at(col)) {
                    ranges.push((a.min(b), c.min(d), a.max(b), c.max(d)));
                }
            }
            _ => {}
        }
    }
    cells.sort();
    cells.dedup();
    (cells, ranges)
}

// ------------------------------------------------------------------------------ as text

fn cell_name(r: u32, ra: bool, c: u32, ca: bool) -> String {
    let mut letters = String::new();
    let mut n = c + 1;
    while n > 0 {
        letters.insert(0, char::from(b'A' + ((n - 1) % 26) as u8));
        n = (n - 1) / 26;
    }
    format!("{}{letters}{}{}", if ca { "$" } else { "" }, if ra { "$" } else { "" }, r + 1)
}

/// The formula as Keynote shows it in the cell editor ("=B2×B3−B4").
pub fn to_text(nodes: &[Node], row: u32, col: u32) -> Option<String> {
    // (text, precedence of its outermost operator; 9 = atom)
    let mut st: Vec<(String, u8)> = Vec::new();
    let mut thunk_depth = 0;
    for n in nodes {
        match n {
            Node::BeginThunk => {
                thunk_depth += 1;
                continue;
            }
            Node::EndThunk => {
                thunk_depth -= 1;
                continue;
            }
            _ => {}
        }
        let _ = thunk_depth;
        let item = match n {
            Node::Num(v) => (plain(*v), 9),
            Node::Str(s) => (format!("\"{}\"", s.replace('"', "\"\"")), 9),
            Node::Bool(b) => (if *b { "TRUE" } else { "FALSE" }.to_string(), 9),
            Node::Ref { row: r, col: c } => (cell_name(r.at(row)?, r.abs, c.at(col)?, c.abs), 9),
            Node::Range { r0, c0, r1, c1 } => {
                let a = cell_name(r0.at(row)?, r0.abs, c0.at(col)?, c0.abs);
                let b = cell_name(r1.at(row)?, r1.abs, c1.at(col)?, c1.abs);
                (if a == b { a } else { format!("{a}:{b}") }, 9)
            }
            Node::List(_) => {
                let (t, _) = st.pop()?;
                (format!("({t})"), 9)
            }
            Node::Neg => {
                let (t, p) = st.pop()?;
                (if p < 9 { format!("−({t})") } else { format!("−{t}") }, 8)
            }
            Node::Percent => {
                let (t, _) = st.pop()?;
                (format!("{t}%"), 9)
            }
            Node::Bin(op) => {
                let (b, _) = st.pop()?;
                let (a, _) = st.pop()?;
                (format!("{a}{}{b}", op.text()), op.prec())
            }
            Node::Func { index, args } => {
                let f = fn_by_index(*index)?;
                let at = st.len().checked_sub(*args as usize)?;
                let a: Vec<String> = st.split_off(at).into_iter().map(|x| x.0).collect();
                (format!("{}({})", f.name, a.join(", ")), 9)
            }
            Node::BeginThunk | Node::EndThunk => unreachable!(),
        };
        st.push(item);
    }
    let (t, _) = st.pop()?;
    st.is_empty().then(|| format!("={t}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals() -> HashMap<(u32, u32), CellValue> {
        // B2:B4 = 2, 3, 5 (the Mac's R30 table)
        [((1, 1), 2.0), ((2, 1), 3.0), ((3, 1), 5.0)].into_iter().map(|(k, v)| (k, CellValue::Number(v))).collect()
    }

    #[test]
    fn r30_formulas_parse_and_evaluate_like_keynote() {
        let v = vals();
        let cases: &[(&str, u32, Value)] = &[
            ("=B2+B3", 1, Value::Num(5.0)),
            ("=B2*B3-B4", 2, Value::Num(1.0)),
            ("=(B2+B3)/B4", 3, Value::Num(1.0)),
            ("=B2^2", 4, Value::Num(4.0)),
            ("=SUM(B2:B4)*2", 5, Value::Num(20.0)),
            ("=ROUND(B2/B3,2)", 6, Value::Num(0.67)),
            ("=IF(B2>B3,\"yes\",\"no\")", 7, Value::Text("no".into())),
            ("=B2&\" items\"", 8, Value::Text("2 items".into())),
            ("=ABS(-5)", 9, Value::Num(5.0)),
            ("=AVERAGE(B2,B4)", 10, Value::Num(3.5)),
            ("=B2%", 11, Value::Num(0.02)),
            ("=COUNTA(B2:B4)", 12, Value::Num(3.0)),
        ];
        for (t, row, want) in cases {
            let n = parse(t, *row, 2).unwrap_or_else(|e| panic!("{t}: {e}"));
            assert_eq!(&eval(&n, *row, 2, &v), want, "{t}");
        }
        // node shapes from the Mac's dump
        assert_eq!(parse("=(B2+B3)/B4", 3, 2).unwrap().iter().filter(|n| matches!(n, Node::List(1))).count(), 1);
        assert_eq!(parse("=ABS(-5)", 9, 2).unwrap(), vec![Node::Num(5.0), Node::Neg, Node::Func { index: 1, args: 1 }]);
        assert_eq!(
            parse("=IF(B2>B3,\"yes\",\"no\")", 7, 2).unwrap(),
            vec![
                Node::Ref { row: Coord { v: -6, abs: false }, col: Coord { v: -1, abs: false } },
                Node::Ref { row: Coord { v: -5, abs: false }, col: Coord { v: -1, abs: false } },
                Node::Bin(Op::Gt),
                Node::BeginThunk,
                Node::Str("yes".into()),
                Node::EndThunk,
                Node::BeginThunk,
                Node::Str("no".into()),
                Node::EndThunk,
                Node::Func { index: 62, args: 3 },
            ]
        );
        assert_eq!(decimal_parts(2.0), (2, 3476778912330022912));
    }

    #[test]
    fn written_back_as_keynote_shows_it() {
        for (t, shown) in [("=B2*B3-B4", "=B2×B3−B4"), ("=(B2+B3)/B4", "=(B2+B3)÷B4"), ("=SUM(B2:B4)*2", "=SUM(B2:B4)×2"), ("=IF(B2>B3,\"yes\",\"no\")", "=IF(B2>B3, \"yes\", \"no\")")] {
            let n = parse(t, 4, 2).unwrap();
            assert_eq!(to_text(&n, 4, 2).as_deref(), Some(shown), "{t}");
            assert_eq!(parse(shown, 4, 2).unwrap(), n, "{shown} parses back the same");
        }
    }

    /// Every Keynote / Numbers function parses and is stored with its TSCE index (N1), also
    /// the ones Keydeck doesn't compute; ids checked against Keynote's file take precedence.
    #[test]
    fn all_functions_known() {
        assert_eq!(fn_by_name("XLOOKUP").map(|f| f.index), crate::functions_all::ALL_FUNCTIONS.iter().find(|f| f.0 == "XLOOKUP").map(|f| f.1));
        // (the same as Keynote's own file, R31)
        assert_eq!(fn_by_name("today").map(|f| f.index), Some(154));
        assert_eq!(fn_by_name("UPPER").map(|f| f.index), Some(158));
        assert_eq!(fn_by_name("CEILING").map(|f| f.index), Some(17));
        assert_eq!(fn_by_name("SUM").map(|f| f.index), Some(168));
        assert_eq!(fn_by_name("XLOOKUP").map(|f| f.index), Some(314));
        assert_eq!(fn_by_name("NETWORKDAYS").map(|f| f.index), Some(260));
        let n = parse("=NETWORKDAYS(B2,B3)", 4, 2).unwrap();
        assert_eq!(to_text(&n, 4, 2).as_deref(), Some("=NETWORKDAYS(B2, B3)"));
        assert!(crate::functions_all::ALL_FUNCTIONS.len() >= 350);
    }

    /// A reference into another table ("Table 2::B2", Numbers sample 10) is not read as one
    /// of this table's cells: the formula is left unread (its stored value stays).
    #[test]
    fn cross_table_reference_left_alone() {
        let nodes = parse("=B2*2", 1, 1).unwrap();
        let mut f = Msg::new("TSCE.FormulaArchive");
        write_nodes(&nodes, &mut f);
        assert!(read_nodes(&f).is_some());
        let n = f.msg_mut("AST_node_array").unwrap().msgs_mut("AST_node").find(|n| n.enum_name("AST_node_type") == Some("CELL_REFERENCE_NODE")).unwrap();
        n.msg_mut_or_create("AST_cross_table_reference_extra_info");
        assert!(read_nodes(&f).is_none());
    }
}
