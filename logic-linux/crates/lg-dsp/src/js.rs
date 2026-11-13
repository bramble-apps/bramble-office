//! A small JavaScript interpreter for Scripter (midifx.rs, scripter.rs), written from scratch.
//!
//! It covers the language Scripter scripts use: var / let / const, functions (declarations,
//! expressions, arrows, closures), objects, arrays, strings, numbers, if / for / for-in / for-of /
//! while / do / switch / break / continue, try / catch / finally / throw, new, instanceof,
//! typeof, template strings, the usual operators, and the common built-ins (Math, String,
//! Array, Number, Object, JSON.stringify, parseInt …). No regular expressions, classes,
//! generators or async.
//!
//! - `parse` makes the tree (syntax errors carry the line); `Interp::run` runs a program in the
//!   global scope; `Interp::call` calls a function value. Every statement and call counts
//!   against a step budget so a runaway script can't hang the audio thread (`Abort::Halt`).
//! - Values are reference counted (`Rc`), so an `Interp` lives on one thread at a time; the
//!   Scripter unit owns its interpreter whole and nothing inside it escapes (see scripter.rs).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

// ---------------------------------------------------------------------------------------------
// values

pub type ObjRef = Rc<RefCell<Obj>>;
pub type NativeFn = Rc<dyn Fn(&mut Interp, &Value, &[Value]) -> Res>;
pub type Res = Result<Value, Abort>;

#[derive(Clone, Default)]
pub enum Value {
    #[default]
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Obj(ObjRef),
}

pub enum Kind {
    Plain,
    Array(Vec<Value>),
    Func(Rc<FuncDef>, Scope),
    Native(NativeFn),
}

pub struct Obj {
    pub props: Vec<(Rc<str>, Value)>,
    pub proto: Option<ObjRef>,
    pub kind: Kind,
}

/// A thrown value, or the script stopped (too many steps): not catchable.
#[derive(Clone)]
pub enum Abort {
    Throw(Value),
    Halt(String),
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Rc::from(s))
    }
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undef | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0 && !n.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Obj(_) => true,
        }
    }
    pub fn as_obj(&self) -> Option<&ObjRef> {
        if let Value::Obj(o) = self { Some(o) } else { None }
    }
    pub fn is_undef(&self) -> bool {
        matches!(self, Value::Undef)
    }
    pub fn is_callable(&self) -> bool {
        self.as_obj().is_some_and(|o| matches!(o.borrow().kind, Kind::Func(..) | Kind::Native(_)))
    }
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undef => "undefined",
            Value::Null => "object",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Obj(_) => {
                if self.is_callable() {
                    "function"
                } else {
                    "object"
                }
            }
        }
    }
}

impl Obj {
    pub fn get_own(&self, k: &str) -> Option<Value> {
        self.props.iter().find(|p| &*p.0 == k).map(|p| p.1.clone())
    }
    pub fn set_own(&mut self, k: &str, v: Value) {
        if let Some(p) = self.props.iter_mut().find(|p| &*p.0 == k) {
            p.1 = v;
        } else {
            self.props.push((Rc::from(k), v));
        }
    }
}

pub fn num_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n == 0.0 {
        "0".into()
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{n:.0}")
    } else if n.abs() < 1e-6 || n.abs() >= 1e21 {
        let s = format!("{n:e}");
        match s.split_once('e') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
            _ => s,
        }
    } else {
        format!("{n}")
    }
}

// ---------------------------------------------------------------------------------------------
// scopes

pub type Scope = Rc<RefCell<ScopeData>>;

pub struct ScopeData {
    vars: HashMap<Rc<str>, Value>,
    parent: Option<Scope>,
    /// A function's (or the program's) scope: `var` lands here.
    func: bool,
}

fn new_scope(parent: Option<Scope>, func: bool) -> Scope {
    Rc::new(RefCell::new(ScopeData { vars: HashMap::new(), parent, func }))
}

// ---------------------------------------------------------------------------------------------
// the lexer

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(Rc<str>),
    /// Template: the string parts and the source of each `${…}`.
    Tmpl(Vec<Rc<str>>, Vec<(String, u32)>),
    Id(Rc<str>),
    P(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
struct Token {
    t: Tok,
    line: u32,
    nl: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxError {
    pub line: u32,
    pub msg: String,
}

const PUNCT: [&str; 52] = [
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "=>", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**", "<<", ">>", "{", "}", "(", ")", "[", "]", ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "?", ":",
];

fn lex(src: &str, first_line: u32) -> Result<Vec<Token>, SyntaxError> {
    let c: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut line = first_line;
    let mut nl = false;
    let mut out = Vec::new();
    let err = |line: u32, m: &str| SyntaxError { line, msg: m.to_string() };
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            nl = true;
            i += 1;
            continue;
        }
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                if c[i] == '\n' {
                    line += 1;
                    nl = true;
                }
                i += 1;
            }
            if i >= c.len() {
                return Err(err(line, "Unterminated comment"));
            }
            i += 2;
            continue;
        }
        let tline = line;
        let t = if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let s = i;
            if ch == '0' && matches!(c.get(i + 1), Some('x') | Some('X')) {
                i += 2;
                while i < c.len() && c[i].is_ascii_hexdigit() {
                    i += 1;
                }
                let h: String = c[s + 2..i].iter().collect();
                Tok::Num(i64::from_str_radix(&h, 16).map_err(|_| err(line, "Bad hex number"))? as f64)
            } else {
                while i < c.len() && (c[i].is_ascii_digit() || c[i] == '.') {
                    i += 1;
                }
                if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                    i += 1;
                    if i < c.len() && (c[i] == '+' || c[i] == '-') {
                        i += 1;
                    }
                    while i < c.len() && c[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let n: String = c[s..i].iter().collect();
                Tok::Num(n.parse::<f64>().map_err(|_| err(line, &format!("Bad number {n}")))?)
            }
        } else if ch.is_alphabetic() || ch == '_' || ch == '$' {
            let s = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_' || c[i] == '$') {
                i += 1;
            }
            Tok::Id(Rc::from(c[s..i].iter().collect::<String>()))
        } else if ch == '"' || ch == '\'' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(&d) = c.get(i) else { return Err(err(line, "Unterminated string")) };
                if d == '\n' {
                    return Err(err(line, "Unterminated string"));
                }
                i += 1;
                if d == ch {
                    break;
                }
                if d == '\\' {
                    let (e, n) = escape(&c, i);
                    s.push_str(&e);
                    i = n;
                } else {
                    s.push(d);
                }
            }
            Tok::Str(Rc::from(s))
        } else if ch == '`' {
            i += 1;
            let mut parts = Vec::new();
            let mut exprs = Vec::new();
            let mut s = String::new();
            loop {
                let Some(&d) = c.get(i) else { return Err(err(line, "Unterminated template")) };
                i += 1;
                if d == '`' {
                    break;
                }
                if d == '\n' {
                    line += 1;
                }
                if d == '\\' {
                    let (e, n) = escape(&c, i);
                    s.push_str(&e);
                    i = n;
                } else if d == '$' && c.get(i) == Some(&'{') {
                    i += 1;
                    let st = i;
                    let mut depth = 1;
                    while i < c.len() && depth > 0 {
                        match c[i] {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                        i += 1;
                    }
                    if depth > 0 {
                        return Err(err(line, "Unterminated template"));
                    }
                    parts.push(Rc::from(std::mem::take(&mut s)));
                    exprs.push((c[st..i - 1].iter().collect::<String>(), line));
                } else {
                    s.push(d);
                }
            }
            parts.push(Rc::from(s));
            Tok::Tmpl(parts, exprs)
        } else {
            let rest: String = c[i..(i + 4).min(c.len())].iter().collect();
            // longest first; "=" and "." last
            let Some(p) = PUNCT.iter().copied().chain(["=", "."]).find(|p| rest.starts_with(p)) else {
                return Err(err(line, &format!("Unexpected character '{ch}'")));
            };
            // "?." before a digit is the conditional operator then a number
            let p = if p == "?." && c.get(i + 2).is_some_and(|d| d.is_ascii_digit()) { "?" } else { p };
            i += p.chars().count();
            Tok::P(p)
        };
        out.push(Token { t, line: tline, nl });
        nl = false;
    }
    out.push(Token { t: Tok::Eof, line, nl: true });
    Ok(out)
}

fn escape(c: &[char], i: usize) -> (String, usize) {
    let Some(&e) = c.get(i) else { return (String::new(), i) };
    let simple = |s: &str| (s.to_string(), i + 1);
    match e {
        'n' => simple("\n"),
        't' => simple("\t"),
        'r' => simple("\r"),
        'b' => simple("\u{8}"),
        '0' => simple("\0"),
        'x' | 'u' => {
            let (n, s) = if e == 'x' { (2, i + 1) } else if c.get(i + 1) == Some(&'{') { (0, i + 2) } else { (4, i + 1) };
            let end = if n == 0 { (s..c.len()).find(|&k| c[k] == '}').unwrap_or(c.len()) } else { (s + n).min(c.len()) };
            let h: String = c[s..end].iter().collect();
            let ch = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}');
            (ch.to_string(), if n == 0 { end + 1 } else { end })
        }
        '\n' => (String::new(), i + 1),
        other => (other.to_string(), i + 1),
    }
}

// ---------------------------------------------------------------------------------------------
// the tree

#[derive(Debug)]
pub struct FuncDef {
    pub name: Option<Rc<str>>,
    pub params: Vec<(Rc<str>, Option<Expr>)>,
    pub rest: Option<Rc<str>>,
    pub body: Vec<Stmt>,
    pub arrow: bool,
    pub line: u32,
}

#[derive(Debug)]
pub enum Expr {
    Num(f64),
    Str(Rc<str>),
    Tmpl(Vec<Rc<str>>, Vec<Expr>),
    Id(Rc<str>, u32),
    This,
    Array(Vec<Expr>),
    Spread(Box<Expr>),
    Object(Vec<(PropKey, Expr)>),
    Func(Rc<FuncDef>),
    Unary(&'static str, Box<Expr>),
    Update(&'static str, bool, Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Logic(&'static str, Box<Expr>, Box<Expr>),
    Assign(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>, bool),
    New(Box<Expr>, Vec<Expr>),
    Member(Box<Expr>, Rc<str>, bool),
    Index(Box<Expr>, Box<Expr>, bool),
    Seq(Vec<Expr>),
}

#[derive(Debug)]
pub enum PropKey {
    Name(Rc<str>),
    Computed(Expr),
}

#[derive(Debug)]
pub struct Stmt {
    pub line: u32,
    pub s: S,
}

#[derive(Debug)]
pub enum S {
    Expr(Expr),
    Var(bool, Vec<(Rc<str>, Option<Expr>)>),
    Func(Rc<FuncDef>),
    Return(Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    /// for (var k in / of obj): is-var, name, of, object, body
    ForIn(bool, Rc<str>, bool, Expr, Box<Stmt>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    Block(Vec<Stmt>),
    Break,
    Continue,
    Switch(Expr, Vec<(Option<Expr>, Vec<Stmt>)>),
    Throw(Expr),
    Try(Vec<Stmt>, Option<(Option<Rc<str>>, Vec<Stmt>)>, Option<Vec<Stmt>>),
    Empty,
}

// ---------------------------------------------------------------------------------------------
// the parser

struct Parser {
    t: Vec<Token>,
    i: usize,
}

type PRes<T> = Result<T, SyntaxError>;

const KEYWORDS: [&str; 26] = ["var", "let", "const", "function", "return", "if", "else", "for", "while", "do", "break", "continue", "switch", "case", "default", "throw", "try", "catch", "finally", "new", "typeof", "instanceof", "in", "of_never", "delete", "void"];

/// Parses a whole script.
pub fn parse(src: &str) -> Result<Vec<Stmt>, SyntaxError> {
    let mut p = Parser { t: lex(src, 1)?, i: 0 };
    let mut out = Vec::new();
    while !p.at_eof() {
        out.push(p.stmt()?);
    }
    Ok(out)
}

/// Just the first syntax error, if any (the editor's live check).
pub fn check(src: &str) -> Option<SyntaxError> {
    parse(src).err()
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.t[self.i].t
    }
    fn peek_at(&self, k: usize) -> &Tok {
        &self.t[(self.i + k).min(self.t.len() - 1)].t
    }
    fn line(&self) -> u32 {
        self.t[self.i].line
    }
    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }
    fn next(&mut self) -> Tok {
        let t = self.t[self.i].t.clone();
        if self.i + 1 < self.t.len() {
            self.i += 1;
        }
        t
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::P(q) if *q == p)
    }
    fn is_id(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Id(q) if &**q == k)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.next();
            true
        } else {
            false
        }
    }
    fn eat_id(&mut self, k: &str) -> bool {
        if self.is_id(k) {
            self.next();
            true
        } else {
            false
        }
    }
    fn err<T>(&self, m: &str) -> PRes<T> {
        Err(SyntaxError { line: self.line(), msg: m.to_string() })
    }
    fn unexpected<T>(&self) -> PRes<T> {
        let what = match self.peek() {
            Tok::Eof => "end of script".to_string(),
            Tok::Num(n) => num_to_string(*n),
            Tok::Str(_) | Tok::Tmpl(..) => "string".into(),
            Tok::Id(s) => format!("'{s}'"),
            Tok::P(p) => format!("'{p}'"),
        };
        self.err(&format!("Unexpected {what}"))
    }
    fn expect(&mut self, p: &str) -> PRes<()> {
        if self.eat(p) { Ok(()) } else { self.err(&format!("Expected '{p}'")) }
    }
    fn ident(&mut self) -> PRes<Rc<str>> {
        match self.peek().clone() {
            Tok::Id(s) if !KEYWORDS.contains(&&*s) => {
                self.next();
                Ok(s)
            }
            _ => self.err("Expected a name"),
        }
    }
    /// Automatic semicolon insertion: `;`, or a line break, `}` or the end.
    fn semi(&mut self) -> PRes<()> {
        if self.eat(";") || self.is("}") || self.at_eof() || self.t[self.i].nl {
            Ok(())
        } else {
            self.unexpected()
        }
    }

    fn block(&mut self) -> PRes<Vec<Stmt>> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.is("}") {
            if self.at_eof() {
                return self.err("Expected '}'");
            }
            out.push(self.stmt()?);
        }
        self.next();
        Ok(out)
    }

    fn stmt(&mut self) -> PRes<Stmt> {
        let line = self.line();
        let s = self.stmt_inner()?;
        Ok(Stmt { line, s })
    }

    fn var_decl(&mut self) -> PRes<S> {
        let is_var = self.is_id("var");
        self.next();
        let mut list = Vec::new();
        loop {
            let n = self.ident()?;
            let init = if self.eat("=") { Some(self.assign()?) } else { None };
            list.push((n, init));
            if !self.eat(",") {
                break;
            }
        }
        Ok(S::Var(is_var, list))
    }

    fn stmt_inner(&mut self) -> PRes<S> {
        if self.is("{") {
            return Ok(S::Block(self.block()?));
        }
        if self.eat(";") {
            return Ok(S::Empty);
        }
        let Tok::Id(k) = self.peek().clone() else {
            let e = self.expr()?;
            self.semi()?;
            return Ok(S::Expr(e));
        };
        match &*k {
            "var" | "let" | "const" => {
                let d = self.var_decl()?;
                self.semi()?;
                Ok(d)
            }
            "function" if matches!(self.peek_at(1), Tok::Id(_)) => {
                self.next();
                Ok(S::Func(self.func_rest(false)?))
            }
            "return" => {
                self.next();
                if self.eat(";") || self.is("}") || self.at_eof() || self.t[self.i].nl {
                    return Ok(S::Return(None));
                }
                let e = self.expr()?;
                self.semi()?;
                Ok(S::Return(Some(e)))
            }
            "if" => {
                self.next();
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                let a = self.stmt()?;
                let b = if self.eat_id("else") { Some(Box::new(self.stmt()?)) } else { None };
                Ok(S::If(c, Box::new(a), b))
            }
            "for" => {
                self.next();
                self.expect("(")?;
                // for (var k in obj) / for (k of arr)
                let decl = self.is_id("var") || self.is_id("let") || self.is_id("const");
                let off = if decl { 1 } else { 0 };
                if let (Tok::Id(n), Tok::Id(w)) = (self.peek_at(off).clone(), self.peek_at(off + 1).clone()) {
                    if &*w == "in" || &*w == "of" {
                        let is_var = self.is_id("var");
                        if decl {
                            self.next();
                        }
                        self.next();
                        self.next();
                        let o = self.expr()?;
                        self.expect(")")?;
                        let body = self.stmt()?;
                        return Ok(S::ForIn(is_var || !decl, n, &*w == "of", o, Box::new(body)));
                    }
                }
                let line = self.line();
                let init = if self.is(";") {
                    None
                } else if decl {
                    Some(Box::new(Stmt { line, s: self.var_decl()? }))
                } else {
                    Some(Box::new(Stmt { line, s: S::Expr(self.expr()?) }))
                };
                self.expect(";")?;
                let cond = if self.is(";") { None } else { Some(self.expr()?) };
                self.expect(";")?;
                let upd = if self.is(")") { None } else { Some(self.expr()?) };
                self.expect(")")?;
                let body = self.stmt()?;
                Ok(S::For(init, cond, upd, Box::new(body)))
            }
            "while" => {
                self.next();
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                Ok(S::While(c, Box::new(self.stmt()?)))
            }
            "do" => {
                self.next();
                let body = self.stmt()?;
                if !self.eat_id("while") {
                    return self.err("Expected 'while'");
                }
                self.expect("(")?;
                let c = self.expr()?;
                self.expect(")")?;
                self.eat(";");
                Ok(S::DoWhile(Box::new(body), c))
            }
            "break" | "continue" => {
                self.next();
                self.semi()?;
                Ok(if &*k == "break" { S::Break } else { S::Continue })
            }
            "throw" => {
                self.next();
                let e = self.expr()?;
                self.semi()?;
                Ok(S::Throw(e))
            }
            "switch" => {
                self.next();
                self.expect("(")?;
                let d = self.expr()?;
                self.expect(")")?;
                self.expect("{")?;
                let mut cases = Vec::new();
                while !self.eat("}") {
                    let test = if self.eat_id("case") {
                        let e = self.expr()?;
                        Some(e)
                    } else if self.eat_id("default") {
                        None
                    } else {
                        return self.unexpected();
                    };
                    self.expect(":")?;
                    let mut body = Vec::new();
                    while !self.is_id("case") && !self.is_id("default") && !self.is("}") {
                        if self.at_eof() {
                            return self.err("Expected '}'");
                        }
                        body.push(self.stmt()?);
                    }
                    cases.push((test, body));
                }
                Ok(S::Switch(d, cases))
            }
            "try" => {
                self.next();
                let body = self.block()?;
                let catch = if self.eat_id("catch") {
                    let name = if self.eat("(") {
                        let n = self.ident()?;
                        self.expect(")")?;
                        Some(n)
                    } else {
                        None
                    };
                    Some((name, self.block()?))
                } else {
                    None
                };
                let fin = if self.eat_id("finally") { Some(self.block()?) } else { None };
                if catch.is_none() && fin.is_none() {
                    return self.err("Missing catch or finally after try");
                }
                Ok(S::Try(body, catch, fin))
            }
            "class" | "import" | "export" | "async" | "yield" | "await" => self.err(&format!("'{k}' isn't supported in Scripter")),
            _ => {
                let e = self.expr()?;
                self.semi()?;
                Ok(S::Expr(e))
            }
        }
    }

    /// After `function`: optional name, parameters, body.
    fn func_rest(&mut self, expr: bool) -> PRes<Rc<FuncDef>> {
        let line = self.line();
        let name = if matches!(self.peek(), Tok::Id(_)) { Some(self.ident()?) } else if expr { None } else { return self.err("Expected a function name") };
        let (params, rest) = self.params()?;
        let body = self.block()?;
        Ok(Rc::new(FuncDef { name, params, rest, body, arrow: false, line }))
    }

    fn params(&mut self) -> PRes<(Vec<(Rc<str>, Option<Expr>)>, Option<Rc<str>>)> {
        self.expect("(")?;
        let mut ps = Vec::new();
        let mut rest = None;
        while !self.eat(")") {
            if self.eat("...") {
                rest = Some(self.ident()?);
                self.expect(")")?;
                break;
            }
            let n = self.ident()?;
            let d = if self.eat("=") { Some(self.assign()?) } else { None };
            ps.push((n, d));
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok((ps, rest))
    }

    fn expr(&mut self) -> PRes<Expr> {
        let e = self.assign()?;
        if self.is(",") {
            let mut v = vec![e];
            while self.eat(",") {
                v.push(self.assign()?);
            }
            return Ok(Expr::Seq(v));
        }
        Ok(e)
    }

    /// Is the `(` at the cursor the start of an arrow function's parameters?
    fn arrow_ahead(&self) -> bool {
        let mut depth = 0;
        let mut k = self.i;
        while k < self.t.len() {
            match &self.t[k].t {
                Tok::P("(") => depth += 1,
                Tok::P(")") => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(self.t.get(k + 1).map(|t| &t.t), Some(Tok::P("=>")));
                    }
                }
                Tok::Eof => return false,
                _ => {}
            }
            k += 1;
        }
        false
    }

    fn arrow_body(&mut self, params: Vec<(Rc<str>, Option<Expr>)>, rest: Option<Rc<str>>, line: u32) -> PRes<Expr> {
        self.expect("=>")?;
        let body = if self.is("{") {
            self.block()?
        } else {
            let l = self.line();
            let e = self.assign()?;
            vec![Stmt { line: l, s: S::Return(Some(e)) }]
        };
        Ok(Expr::Func(Rc::new(FuncDef { name: None, params, rest, body, arrow: true, line })))
    }

    fn assign(&mut self) -> PRes<Expr> {
        let line = self.line();
        if let (Tok::Id(n), Tok::P("=>")) = (self.peek().clone(), self.peek_at(1).clone()) {
            if !KEYWORDS.contains(&&*n) {
                self.next();
                return self.arrow_body(vec![(n, None)], None, line);
            }
        }
        if self.is("(") && self.arrow_ahead() {
            let (ps, rest) = self.params()?;
            return self.arrow_body(ps, rest, line);
        }
        let lhs = self.cond()?;
        const OPS: [&str; 13] = ["=", "+=", "-=", "*=", "/=", "%=", "**=", "<<=", ">>=", ">>>=", "&=", "|=", "^="];
        if let Tok::P(p) = self.peek().clone() {
            if OPS.contains(&p) {
                if !matches!(lhs, Expr::Id(..) | Expr::Member(..) | Expr::Index(..)) {
                    return self.err("Invalid left-hand side in assignment");
                }
                self.next();
                let rhs = self.assign()?;
                return Ok(Expr::Assign(p, Box::new(lhs), Box::new(rhs)));
            }
        }
        Ok(lhs)
    }

    fn cond(&mut self) -> PRes<Expr> {
        let c = self.bin(0)?;
        if self.eat("?") {
            let a = self.assign()?;
            self.expect(":")?;
            let b = self.assign()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn bin_op(&self) -> Option<(&'static str, u8)> {
        let p = match self.peek() {
            Tok::P(p) => *p,
            Tok::Id(s) if &**s == "instanceof" => "instanceof",
            Tok::Id(s) if &**s == "in" => "in",
            _ => return None,
        };
        let prec = match p {
            "??" => 1,
            "||" => 2,
            "&&" => 3,
            "|" => 4,
            "^" => 5,
            "&" => 6,
            "==" | "!=" | "===" | "!==" => 7,
            "<" | ">" | "<=" | ">=" | "instanceof" | "in" => 8,
            "<<" | ">>" | ">>>" => 9,
            "+" | "-" => 10,
            "*" | "/" | "%" => 11,
            "**" => 12,
            _ => return None,
        };
        Some((p, prec))
    }

    fn bin(&mut self, min: u8) -> PRes<Expr> {
        let mut l = self.unary()?;
        while let Some((op, prec)) = self.bin_op() {
            if prec < min {
                break;
            }
            self.next();
            // ** is right associative
            let r = self.bin(if op == "**" { prec } else { prec + 1 })?;
            l = if matches!(op, "&&" | "||" | "??") { Expr::Logic(op, Box::new(l), Box::new(r)) } else { Expr::Bin(op, Box::new(l), Box::new(r)) };
        }
        Ok(l)
    }

    fn unary(&mut self) -> PRes<Expr> {
        if let Tok::P(p) = self.peek().clone() {
            match p {
                "!" | "-" | "+" | "~" => {
                    self.next();
                    return Ok(Expr::Unary(p, Box::new(self.unary()?)));
                }
                "++" | "--" => {
                    self.next();
                    let e = self.unary()?;
                    return Ok(Expr::Update(p, true, Box::new(e)));
                }
                _ => {}
            }
        }
        for k in ["typeof", "void", "delete"] {
            if self.is_id(k) {
                self.next();
                let e = self.unary()?;
                return Ok(Expr::Unary(k, Box::new(e)));
            }
        }
        let e = self.postfix()?;
        Ok(e)
    }

    fn postfix(&mut self) -> PRes<Expr> {
        let e = self.call()?;
        if !self.t[self.i].nl {
            if self.is("++") || self.is("--") {
                let Tok::P(p) = self.next() else { unreachable!() };
                return Ok(Expr::Update(p, false, Box::new(e)));
            }
        }
        Ok(e)
    }

    fn args(&mut self) -> PRes<Vec<Expr>> {
        self.expect("(")?;
        let mut v = Vec::new();
        while !self.eat(")") {
            if self.eat("...") {
                v.push(Expr::Spread(Box::new(self.assign()?)));
            } else {
                v.push(self.assign()?);
            }
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        Ok(v)
    }

    fn prop_name(&mut self) -> PRes<Rc<str>> {
        match self.next() {
            Tok::Id(s) => Ok(s),
            _ => self.err("Expected a property name"),
        }
    }

    fn call(&mut self) -> PRes<Expr> {
        let mut e = if self.eat_id("new") {
            // new X.y(args) / new X
            let mut c = self.primary()?;
            loop {
                if self.eat(".") {
                    let n = self.prop_name()?;
                    c = Expr::Member(Box::new(c), n, false);
                } else if self.eat("[") {
                    let i = self.expr()?;
                    self.expect("]")?;
                    c = Expr::Index(Box::new(c), Box::new(i), false);
                } else {
                    break;
                }
            }
            let a = if self.is("(") { self.args()? } else { Vec::new() };
            Expr::New(Box::new(c), a)
        } else {
            self.primary()?
        };
        loop {
            if self.eat(".") {
                let n = self.prop_name()?;
                e = Expr::Member(Box::new(e), n, false);
            } else if self.eat("?.") {
                if self.is("(") {
                    let a = self.args()?;
                    e = Expr::Call(Box::new(e), a, true);
                } else if self.eat("[") {
                    let i = self.expr()?;
                    self.expect("]")?;
                    e = Expr::Index(Box::new(e), Box::new(i), true);
                } else {
                    let n = self.prop_name()?;
                    e = Expr::Member(Box::new(e), n, true);
                }
            } else if self.eat("[") {
                let i = self.expr()?;
                self.expect("]")?;
                e = Expr::Index(Box::new(e), Box::new(i), false);
            } else if self.is("(") {
                let a = self.args()?;
                e = Expr::Call(Box::new(e), a, false);
            } else if let Tok::Tmpl(..) = self.peek() {
                return self.err("Tagged templates aren't supported");
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> PRes<Expr> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Num(n) => {
                self.next();
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.next();
                Ok(Expr::Str(s))
            }
            Tok::Tmpl(parts, srcs) => {
                self.next();
                let mut es = Vec::new();
                for (s, l) in srcs {
                    let mut p = Parser { t: lex(&s, l)?, i: 0 };
                    let e = p.expr()?;
                    if !p.at_eof() {
                        return p.unexpected();
                    }
                    es.push(e);
                }
                Ok(Expr::Tmpl(parts, es))
            }
            Tok::P("(") => {
                self.next();
                let e = self.expr()?;
                self.expect(")")?;
                Ok(e)
            }
            Tok::P("[") => {
                self.next();
                let mut v = Vec::new();
                while !self.eat("]") {
                    if self.is(",") {
                        self.next();
                        v.push(Expr::Id(Rc::from("undefined"), line));
                        continue;
                    }
                    if self.eat("...") {
                        v.push(Expr::Spread(Box::new(self.assign()?)));
                    } else {
                        v.push(self.assign()?);
                    }
                    if !self.eat(",") {
                        self.expect("]")?;
                        break;
                    }
                }
                Ok(Expr::Array(v))
            }
            Tok::P("{") => {
                self.next();
                let mut v = Vec::new();
                while !self.eat("}") {
                    let key = match self.next() {
                        Tok::Id(s) => PropKey::Name(s),
                        Tok::Str(s) => PropKey::Name(s),
                        Tok::Num(n) => PropKey::Name(Rc::from(num_to_string(n))),
                        Tok::P("[") => {
                            let e = self.assign()?;
                            self.expect("]")?;
                            PropKey::Computed(e)
                        }
                        _ => return self.err("Expected a property name"),
                    };
                    let val = if self.eat(":") {
                        self.assign()?
                    } else if self.is("(") {
                        let l = self.line();
                        let (params, rest) = self.params()?;
                        let body = self.block()?;
                        let name = if let PropKey::Name(n) = &key { Some(n.clone()) } else { None };
                        Expr::Func(Rc::new(FuncDef { name, params, rest, body, arrow: false, line: l }))
                    } else if let PropKey::Name(n) = &key {
                        Expr::Id(n.clone(), line)
                    } else {
                        return self.err("Expected ':'");
                    };
                    v.push((key, val));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
                Ok(Expr::Object(v))
            }
            Tok::Id(s) => match &*s {
                "function" => {
                    self.next();
                    Ok(Expr::Func(self.func_rest(true)?))
                }
                "this" => {
                    self.next();
                    Ok(Expr::This)
                }
                k if KEYWORDS.contains(&k) => self.unexpected(),
                _ => {
                    self.next();
                    Ok(Expr::Id(s, line))
                }
            },
            _ => self.unexpected(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the interpreter

enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

pub struct Interp {
    pub global: Scope,
    /// Prototypes of the built-in types.
    pub object_proto: ObjRef,
    pub func_proto: ObjRef,
    pub array_proto: ObjRef,
    pub string_proto: ObjRef,
    pub number_proto: ObjRef,
    pub error_proto: ObjRef,
    /// The line of the statement running (runtime errors report it).
    pub line: u32,
    /// Steps left before the script is stopped.
    pub budget: u64,
    /// The host's own state (Scripter's: scripter::Host).
    pub host: Box<dyn std::any::Any>,
    /// Lines printed by console.log / the host's Trace.
    pub console: Vec<String>,
    depth: u32,
}

/// Nested calls a script may make (the interpreter recurses on the thread's stack).
pub const MAX_DEPTH: u32 = 48;

/// Steps one host call (HandleMIDI, ProcessMIDI …) may take.
pub const BUDGET: u64 = 2_000_000;

pub fn obj(proto: Option<ObjRef>, kind: Kind) -> ObjRef {
    Rc::new(RefCell::new(Obj { props: Vec::new(), proto, kind }))
}

impl Interp {
    pub fn new(host: Box<dyn std::any::Any>) -> Interp {
        let object_proto = obj(None, Kind::Plain);
        let func_proto = obj(Some(object_proto.clone()), Kind::Plain);
        let array_proto = obj(Some(object_proto.clone()), Kind::Plain);
        let string_proto = obj(Some(object_proto.clone()), Kind::Plain);
        let number_proto = obj(Some(object_proto.clone()), Kind::Plain);
        let error_proto = obj(Some(object_proto.clone()), Kind::Plain);
        let mut it = Interp { global: new_scope(None, true), object_proto, func_proto, array_proto, string_proto, number_proto, error_proto, line: 0, budget: BUDGET, host, console: Vec::new(), depth: 0 };
        crate::js_builtins::install(&mut it);
        it
    }

    // ---- making values

    pub fn new_object(&self) -> Value {
        Value::Obj(obj(Some(self.object_proto.clone()), Kind::Plain))
    }
    pub fn new_array(&self, v: Vec<Value>) -> Value {
        Value::Obj(obj(Some(self.array_proto.clone()), Kind::Array(v)))
    }
    pub fn native(&self, f: impl Fn(&mut Interp, &Value, &[Value]) -> Res + 'static) -> Value {
        Value::Obj(obj(Some(self.func_proto.clone()), Kind::Native(Rc::new(f))))
    }
    /// A native function with a `prototype` object (a constructor).
    pub fn native_ctor(&self, proto: ObjRef, f: impl Fn(&mut Interp, &Value, &[Value]) -> Res + 'static) -> Value {
        let c = self.native(f);
        if let Value::Obj(o) = &c {
            o.borrow_mut().set_own("prototype", Value::Obj(proto.clone()));
        }
        proto.borrow_mut().set_own("constructor", c.clone());
        c
    }
    pub fn set_global(&mut self, name: &str, v: Value) {
        self.global.borrow_mut().vars.insert(Rc::from(name), v);
    }
    pub fn get_global(&self, name: &str) -> Value {
        self.global.borrow().vars.get(name).cloned().unwrap_or_default()
    }
    pub fn has_global(&self, name: &str) -> bool {
        self.global.borrow().vars.contains_key(name)
    }
    pub fn error(&self, kind: &str, msg: &str) -> Abort {
        // TypeError, RangeError …: that constructor's prototype
        let proto = self.get_global(kind).as_obj().and_then(|c| c.borrow().get_own("prototype")).and_then(|p| p.as_obj().cloned()).unwrap_or_else(|| self.error_proto.clone());
        let e = obj(Some(proto), Kind::Plain);
        {
            let mut b = e.borrow_mut();
            b.set_own("name", Value::str(kind));
            b.set_own("message", Value::str(msg));
            b.set_own("line", Value::Num(self.line as f64));
        }
        Abort::Throw(Value::Obj(e))
    }
    pub fn type_error<T>(&self, msg: &str) -> Result<T, Abort> {
        Err(self.error("TypeError", msg))
    }

    /// An error as the console prints it: "TypeError: … (line 4)".
    pub fn describe(&mut self, a: &Abort) -> String {
        match a {
            Abort::Halt(m) => m.clone(),
            Abort::Throw(v) => {
                let line = v.as_obj().and_then(|o| o.borrow().get_own("line")).map(|l| self.to_num(&l) as u32);
                let s = self.to_string(v).unwrap_or_else(|_| "Error".into());
                match line {
                    Some(l) if l > 0 => format!("{s} (line {l})"),
                    _ => s,
                }
            }
        }
    }

    // ---- conversions

    pub fn to_num(&self, v: &Value) -> f64 {
        match v {
            Value::Undef => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => *b as u8 as f64,
            Value::Num(n) => *n,
            Value::Str(s) => str_to_num(s),
            Value::Obj(o) => {
                let b = o.borrow();
                match &b.kind {
                    Kind::Array(a) if a.is_empty() => 0.0,
                    Kind::Array(a) if a.len() == 1 => self.to_num(&a[0]),
                    _ => f64::NAN,
                }
            }
        }
    }

    pub fn to_string(&mut self, v: &Value) -> Result<String, Abort> {
        Ok(match v {
            Value::Undef => "undefined".into(),
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => num_to_string(*n),
            Value::Str(s) => s.to_string(),
            Value::Obj(o) => {
                let arr = if let Kind::Array(a) = &o.borrow().kind { Some(a.clone()) } else { None };
                if let Some(a) = arr {
                    let mut parts = Vec::new();
                    for x in &a {
                        parts.push(if matches!(x, Value::Undef | Value::Null) { String::new() } else { self.to_string(x)? });
                    }
                    return Ok(parts.join(","));
                }
                let ts = self.get(v, "toString")?;
                if ts.is_callable() {
                    let r = self.call(&ts, v, &[])?;
                    if !matches!(r, Value::Obj(_)) {
                        return self.to_string(&r);
                    }
                }
                if v.is_callable() { "function".into() } else { "[object Object]".into() }
            }
        })
    }

    fn to_primitive(&mut self, v: &Value) -> Res {
        if let Value::Obj(_) = v {
            return Ok(Value::Str(Rc::from(self.to_string(v)?)));
        }
        Ok(v.clone())
    }

    pub fn to_int(&self, v: &Value) -> i64 {
        let n = self.to_num(v);
        if n.is_finite() { n.trunc() as i64 } else { 0 }
    }

    // ---- properties

    fn proto_of(&self, v: &Value) -> Option<ObjRef> {
        match v {
            Value::Str(_) => Some(self.string_proto.clone()),
            Value::Num(_) => Some(self.number_proto.clone()),
            Value::Bool(_) => Some(self.object_proto.clone()),
            Value::Obj(o) => o.borrow().proto.clone(),
            _ => None,
        }
    }

    pub fn get(&mut self, v: &Value, k: &str) -> Res {
        match v {
            Value::Undef | Value::Null => return self.type_error(&format!("Cannot read property '{k}' of {}", if v.is_undef() { "undefined" } else { "null" })),
            Value::Str(s) => {
                if k == "length" {
                    return Ok(Value::Num(s.encode_utf16().count() as f64));
                }
                if let Ok(i) = k.parse::<usize>() {
                    return Ok(s.chars().nth(i).map(|c| Value::Str(Rc::from(c.to_string()))).unwrap_or_default());
                }
            }
            Value::Obj(o) => {
                let b = o.borrow();
                if let Kind::Array(a) = &b.kind {
                    if k == "length" {
                        return Ok(Value::Num(a.len() as f64));
                    }
                    if let Ok(i) = k.parse::<usize>() {
                        return Ok(a.get(i).cloned().unwrap_or_default());
                    }
                }
                if let Some(x) = b.get_own(k) {
                    return Ok(x);
                }
                if k == "length" {
                    if let Kind::Func(f, _) = &b.kind {
                        return Ok(Value::Num(f.params.len() as f64));
                    }
                }
            }
            _ => {}
        }
        let mut p = self.proto_of(v);
        while let Some(o) = p {
            if let Some(x) = o.borrow().get_own(k) {
                return Ok(x);
            }
            p = o.borrow().proto.clone();
        }
        Ok(Value::Undef)
    }

    pub fn get_index(&mut self, v: &Value, k: &Value) -> Res {
        if let (Value::Obj(o), Value::Num(n)) = (v, k) {
            if let Kind::Array(a) = &o.borrow().kind {
                if *n >= 0.0 && n.fract() == 0.0 {
                    return Ok(a.get(*n as usize).cloned().unwrap_or_default());
                }
            }
        }
        let key = self.to_string(k)?;
        self.get(v, &key)
    }

    pub fn put(&mut self, v: &Value, k: &str, x: Value) -> Result<(), Abort> {
        let Value::Obj(o) = v else {
            if matches!(v, Value::Undef | Value::Null) {
                return self.type_error(&format!("Cannot set property '{k}' of {}", if v.is_undef() { "undefined" } else { "null" }));
            }
            return Ok(());
        };
        let mut b = o.borrow_mut();
        if let Kind::Array(a) = &mut b.kind {
            if k == "length" {
                let n = self.to_num(&x);
                if n >= 0.0 && n.fract() == 0.0 && n < 1e7 {
                    a.resize(n as usize, Value::Undef);
                    return Ok(());
                }
                drop(b);
                return Err(self.error("RangeError", "Invalid array length"));
            }
            if let Ok(i) = k.parse::<usize>() {
                if i > 10_000_000 {
                    drop(b);
                    return Err(self.error("RangeError", "Array index too large"));
                }
                if i >= a.len() {
                    a.resize(i + 1, Value::Undef);
                }
                a[i] = x;
                return Ok(());
            }
        }
        b.set_own(k, x);
        Ok(())
    }

    fn put_index(&mut self, v: &Value, k: &Value, x: Value) -> Result<(), Abort> {
        if let (Value::Obj(o), Value::Num(n)) = (v, k) {
            if let Kind::Array(a) = &mut o.borrow_mut().kind {
                if *n >= 0.0 && n.fract() == 0.0 && *n < 1e7 {
                    let i = *n as usize;
                    if i >= a.len() {
                        a.resize(i + 1, Value::Undef);
                    }
                    a[i] = x;
                    return Ok(());
                }
            }
        }
        let key = self.to_string(k)?;
        self.put(v, &key, x)
    }

    // ---- scopes

    fn lookup(&self, sc: &Scope, n: &str) -> Option<Value> {
        let mut s = Some(sc.clone());
        while let Some(x) = s {
            if let Some(v) = x.borrow().vars.get(n) {
                return Some(v.clone());
            }
            s = x.borrow().parent.clone();
        }
        None
    }

    fn assign_var(&self, sc: &Scope, n: &Rc<str>, v: Value) {
        let mut s = Some(sc.clone());
        while let Some(x) = s {
            if let Some(slot) = x.borrow_mut().vars.get_mut(n) {
                *slot = v;
                return;
            }
            s = x.borrow().parent.clone();
        }
        // sloppy mode: an undeclared name becomes a global
        self.global.borrow_mut().vars.insert(n.clone(), v);
    }

    fn func_scope(sc: &Scope) -> Scope {
        let mut s = sc.clone();
        loop {
            if s.borrow().func {
                return s;
            }
            let p = s.borrow().parent.clone();
            match p {
                Some(p) => s = p,
                None => return s,
            }
        }
    }

    /// Function declarations (and `var` names) are known before the block runs.
    fn hoist(&mut self, body: &[Stmt], sc: &Scope) {
        for st in body {
            match &st.s {
                S::Func(f) => {
                    let v = self.make_func(f.clone(), sc);
                    if let Some(n) = &f.name {
                        sc.borrow_mut().vars.insert(n.clone(), v);
                    }
                }
                S::Var(true, list) => {
                    let fs = Self::func_scope(sc);
                    for (n, _) in list {
                        fs.borrow_mut().vars.entry(n.clone()).or_insert(Value::Undef);
                    }
                }
                _ => {}
            }
        }
    }

    fn make_func(&self, f: Rc<FuncDef>, sc: &Scope) -> Value {
        let arrow = f.arrow;
        let o = obj(Some(self.func_proto.clone()), Kind::Func(f, sc.clone()));
        if !arrow {
            let proto = obj(Some(self.object_proto.clone()), Kind::Plain);
            proto.borrow_mut().set_own("constructor", Value::Obj(o.clone()));
            o.borrow_mut().set_own("prototype", Value::Obj(proto));
        }
        Value::Obj(o)
    }

    // ---- running

    fn step(&mut self) -> Result<(), Abort> {
        if self.budget == 0 {
            return Err(Abort::Halt(format!("Script stopped: it ran too long (line {})", self.line)));
        }
        self.budget -= 1;
        Ok(())
    }

    /// Runs a program at the global level.
    pub fn run(&mut self, prog: &[Stmt]) -> Res {
        let g = self.global.clone();
        self.hoist(prog, &g);
        let mut last = Value::Undef;
        for st in prog {
            if let S::Expr(e) = &st.s {
                self.line = st.line;
                self.step()?;
                last = self.eval(e, &g)?;
                continue;
            }
            match self.exec(st, &g)? {
                Flow::Normal => {}
                _ => break,
            }
        }
        Ok(last)
    }

    /// Parses and runs source text; syntax errors come back as a thrown SyntaxError.
    pub fn eval_source(&mut self, src: &str) -> Res {
        match parse(src) {
            Ok(p) => self.run(&p),
            Err(e) => {
                self.line = e.line;
                Err(self.error("SyntaxError", &e.msg))
            }
        }
    }

    fn exec_block(&mut self, body: &[Stmt], sc: &Scope) -> Result<Flow, Abort> {
        let inner = new_scope(Some(sc.clone()), false);
        self.hoist(body, &inner);
        for st in body {
            match self.exec(st, &inner)? {
                Flow::Normal => {}
                f => return Ok(f),
            }
        }
        Ok(Flow::Normal)
    }

    fn exec(&mut self, st: &Stmt, sc: &Scope) -> Result<Flow, Abort> {
        self.line = st.line;
        self.step()?;
        match &st.s {
            S::Empty | S::Func(_) => {}
            S::Expr(e) => {
                self.eval(e, sc)?;
            }
            S::Var(is_var, list) => {
                for (n, init) in list {
                    let v = match init {
                        Some(e) => {
                            let v = self.eval(e, sc)?;
                            name_func(&v, n);
                            Some(v)
                        }
                        None => None,
                    };
                    if *is_var {
                        let fs = Self::func_scope(sc);
                        if let Some(v) = v {
                            fs.borrow_mut().vars.insert(n.clone(), v);
                        } else {
                            fs.borrow_mut().vars.entry(n.clone()).or_insert(Value::Undef);
                        }
                    } else {
                        sc.borrow_mut().vars.insert(n.clone(), v.unwrap_or_default());
                    }
                }
            }
            S::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, sc)?,
                    None => Value::Undef,
                };
                return Ok(Flow::Return(v));
            }
            S::If(c, a, b) => {
                if self.eval(c, sc)?.truthy() {
                    return self.exec(a, sc);
                } else if let Some(b) = b {
                    return self.exec(b, sc);
                }
            }
            S::Block(body) => return self.exec_block(body, sc),
            S::For(init, cond, upd, body) => {
                let ls = new_scope(Some(sc.clone()), false);
                if let Some(i) = init {
                    self.exec(i, &ls)?;
                }
                loop {
                    if let Some(c) = cond {
                        if !self.eval(c, &ls)?.truthy() {
                            break;
                        }
                    }
                    match self.exec(body, &ls)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if let Some(u) = upd {
                        self.step()?;
                        self.eval(u, &ls)?;
                    }
                }
            }
            S::ForIn(is_var, n, of, o, body) => {
                let ov = self.eval(o, sc)?;
                let items: Vec<Value> = if *of {
                    match &ov {
                        Value::Str(s) => s.chars().map(|c| Value::Str(Rc::from(c.to_string()))).collect(),
                        Value::Obj(x) => match &x.borrow().kind {
                            Kind::Array(a) => a.clone(),
                            _ => return self.type_error("Value is not iterable"),
                        },
                        _ => return self.type_error("Value is not iterable"),
                    }
                } else {
                    self.keys(&ov).into_iter().map(Value::Str).collect()
                };
                let ls = new_scope(Some(sc.clone()), false);
                for it in items {
                    if *is_var {
                        Self::func_scope(sc).borrow_mut().vars.insert(n.clone(), it);
                    } else {
                        ls.borrow_mut().vars.insert(n.clone(), it);
                    }
                    match self.exec(body, &ls)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
            }
            S::While(c, body) => {
                while self.eval(c, sc)?.truthy() {
                    match self.exec(body, sc)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
            }
            S::DoWhile(body, c) => loop {
                match self.exec(body, sc)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    _ => {}
                }
                if !self.eval(c, sc)?.truthy() {
                    break;
                }
            },
            S::Break => return Ok(Flow::Break),
            S::Continue => return Ok(Flow::Continue),
            S::Throw(e) => {
                let v = self.eval(e, sc)?;
                if let Value::Obj(o) = &v {
                    if o.borrow().get_own("line").is_none() && self.is_error(&v) {
                        o.borrow_mut().set_own("line", Value::Num(st.line as f64));
                    }
                }
                return Err(Abort::Throw(v));
            }
            S::Switch(d, cases) => {
                let dv = self.eval(d, sc)?;
                let mut hit = None;
                for (k, (t, _)) in cases.iter().enumerate() {
                    if let Some(t) = t {
                        let tv = self.eval(t, sc)?;
                        if strict_eq(&dv, &tv) {
                            hit = Some(k);
                            break;
                        }
                    }
                }
                let hit = hit.or_else(|| cases.iter().position(|c| c.0.is_none()));
                if let Some(h) = hit {
                    let inner = new_scope(Some(sc.clone()), false);
                    for (_, body) in &cases[h..] {
                        self.hoist(body, &inner);
                        for st in body {
                            match self.exec(st, &inner)? {
                                Flow::Normal => {}
                                Flow::Break => return Ok(Flow::Normal),
                                f => return Ok(f),
                            }
                        }
                    }
                }
            }
            S::Try(body, catch, fin) => {
                let r = self.exec_block(body, sc);
                let r = match (r, catch) {
                    (Err(Abort::Throw(v)), Some((name, cb))) => {
                        let cs = new_scope(Some(sc.clone()), false);
                        if let Some(n) = name {
                            cs.borrow_mut().vars.insert(n.clone(), v);
                        }
                        self.exec_block(cb, &cs)
                    }
                    (r, _) => r,
                };
                if let Some(f) = fin {
                    if let Err(Abort::Halt(m)) = &r {
                        return Err(Abort::Halt(m.clone()));
                    }
                    match self.exec_block(f, sc)? {
                        Flow::Normal => {}
                        f => return Ok(f),
                    }
                }
                return r;
            }
        }
        Ok(Flow::Normal)
    }

    pub fn is_error(&self, v: &Value) -> bool {
        let mut p = v.as_obj().and_then(|o| o.borrow().proto.clone());
        while let Some(o) = p {
            if Rc::ptr_eq(&o, &self.error_proto) {
                return true;
            }
            p = o.borrow().proto.clone();
        }
        false
    }

    /// An object's own enumerable keys (array indexes first).
    pub fn keys(&self, v: &Value) -> Vec<Rc<str>> {
        let Value::Obj(o) = v else {
            if let Value::Str(s) = v {
                return (0..s.chars().count()).map(|i| Rc::from(i.to_string())).collect();
            }
            return Vec::new();
        };
        let b = o.borrow();
        let mut out: Vec<Rc<str>> = Vec::new();
        if let Kind::Array(a) = &b.kind {
            out.extend((0..a.len()).map(|i| Rc::from(i.to_string())));
        }
        let skip_proto = matches!(b.kind, Kind::Func(..) | Kind::Native(_));
        out.extend(b.props.iter().filter(|p| !(skip_proto && &*p.0 == "prototype")).map(|p| p.0.clone()));
        out
    }

    fn eval_args(&mut self, args: &[Expr], sc: &Scope) -> Result<Vec<Value>, Abort> {
        let mut v = Vec::with_capacity(args.len());
        for a in args {
            if let Expr::Spread(e) = a {
                let x = self.eval(e, sc)?;
                match x.as_obj().map(|o| if let Kind::Array(a) = &o.borrow().kind { Some(a.clone()) } else { None }) {
                    Some(Some(a)) => v.extend(a),
                    _ => return self.type_error("Spread of a value that isn't an array"),
                }
            } else {
                v.push(self.eval(a, sc)?);
            }
        }
        Ok(v)
    }

    pub fn eval(&mut self, e: &Expr, sc: &Scope) -> Res {
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Tmpl(parts, es) => {
                let mut s = String::new();
                for (k, p) in parts.iter().enumerate() {
                    s.push_str(p);
                    if let Some(x) = es.get(k) {
                        let v = self.eval(x, sc)?;
                        s.push_str(&self.to_string(&v)?);
                    }
                }
                Value::Str(Rc::from(s))
            }
            Expr::Id(n, line) => match &**n {
                "undefined" => Value::Undef,
                "null" => Value::Null,
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "NaN" => Value::Num(f64::NAN),
                "Infinity" => Value::Num(f64::INFINITY),
                _ => match self.lookup(sc, n) {
                    Some(v) => v,
                    None => {
                        self.line = *line;
                        return Err(self.error("ReferenceError", &format!("Can't find variable: {n}")));
                    }
                },
            },
            Expr::This => self.lookup(sc, "this").unwrap_or_default(),
            Expr::Array(items) => {
                let v = self.eval_args(items, sc)?;
                self.new_array(v)
            }
            Expr::Spread(_) => return Err(self.error("SyntaxError", "Unexpected '...'")),
            Expr::Object(props) => {
                let o = self.new_object();
                for (k, ve) in props {
                    let key: Rc<str> = match k {
                        PropKey::Name(n) => n.clone(),
                        PropKey::Computed(e) => {
                            let kv = self.eval(e, sc)?;
                            Rc::from(self.to_string(&kv)?)
                        }
                    };
                    let v = self.eval(ve, sc)?;
                    name_func(&v, &key);
                    self.put(&o, &key, v)?;
                }
                o
            }
            Expr::Func(f) => self.make_func(f.clone(), sc),
            Expr::Unary(op, x) => {
                if *op == "typeof" {
                    if let Expr::Id(n, _) = &**x {
                        if !matches!(&**n, "undefined" | "null" | "true" | "false" | "NaN" | "Infinity") && self.lookup(sc, n).is_none() {
                            return Ok(Value::str("undefined"));
                        }
                    }
                    let v = self.eval(x, sc)?;
                    return Ok(Value::str(v.type_of()));
                }
                if *op == "delete" {
                    match &**x {
                        Expr::Member(o, k, _) => {
                            let ov = self.eval(o, sc)?;
                            if let Value::Obj(o) = ov {
                                o.borrow_mut().props.retain(|p| &*p.0 != &**k);
                            }
                        }
                        Expr::Index(o, k, _) => {
                            let ov = self.eval(o, sc)?;
                            let kv = self.eval(k, sc)?;
                            let ks = self.to_string(&kv)?;
                            if let Value::Obj(o) = ov {
                                let mut b = o.borrow_mut();
                                if let (Kind::Array(a), Ok(i)) = (&mut b.kind, ks.parse::<usize>()) {
                                    if i < a.len() {
                                        a[i] = Value::Undef;
                                    }
                                } else {
                                    b.props.retain(|p| *p.0 != *ks);
                                }
                            }
                        }
                        _ => {}
                    }
                    return Ok(Value::Bool(true));
                }
                let v = self.eval(x, sc)?;
                match *op {
                    "!" => Value::Bool(!v.truthy()),
                    "-" => Value::Num(-self.to_num(&v)),
                    "+" => Value::Num(self.to_num(&v)),
                    "~" => Value::Num(!(to_i32(self.to_num(&v))) as f64),
                    _ => Value::Undef,
                }
            }
            Expr::Update(op, prefix, target) => {
                let old = self.eval(target, sc)?;
                let n = self.to_num(&old);
                let new = if *op == "++" { n + 1.0 } else { n - 1.0 };
                self.store(target, Value::Num(new), sc)?;
                Value::Num(if *prefix { new } else { n })
            }
            Expr::Bin(op, a, b) => {
                let x = self.eval(a, sc)?;
                let y = self.eval(b, sc)?;
                self.binary(op, &x, &y)?
            }
            Expr::Logic(op, a, b) => {
                let x = self.eval(a, sc)?;
                match *op {
                    "&&" => {
                        if x.truthy() {
                            self.eval(b, sc)?
                        } else {
                            x
                        }
                    }
                    "||" => {
                        if x.truthy() {
                            x
                        } else {
                            self.eval(b, sc)?
                        }
                    }
                    _ => {
                        if matches!(x, Value::Undef | Value::Null) {
                            self.eval(b, sc)?
                        } else {
                            x
                        }
                    }
                }
            }
            Expr::Assign(op, target, rhs) => {
                let v = if *op == "=" {
                    let v = self.eval(rhs, sc)?;
                    if let Expr::Id(n, _) = &**target {
                        name_func(&v, n);
                    }
                    v
                } else {
                    let old = self.eval(target, sc)?;
                    let r = self.eval(rhs, sc)?;
                    self.binary(&op[..op.len() - 1], &old, &r)?
                };
                self.store(target, v.clone(), sc)?;
                v
            }
            Expr::Cond(c, a, b) => {
                if self.eval(c, sc)?.truthy() {
                    self.eval(a, sc)?
                } else {
                    self.eval(b, sc)?
                }
            }
            Expr::Seq(v) => {
                let mut last = Value::Undef;
                for x in v {
                    last = self.eval(x, sc)?;
                }
                last
            }
            Expr::Member(o, k, opt) => {
                let ov = self.eval(o, sc)?;
                if *opt && matches!(ov, Value::Undef | Value::Null) {
                    return Ok(Value::Undef);
                }
                self.get(&ov, k)?
            }
            Expr::Index(o, k, opt) => {
                let ov = self.eval(o, sc)?;
                if *opt && matches!(ov, Value::Undef | Value::Null) {
                    return Ok(Value::Undef);
                }
                let kv = self.eval(k, sc)?;
                self.get_index(&ov, &kv)?
            }
            Expr::Call(f, args, opt) => {
                let (fv, this) = match &**f {
                    Expr::Member(o, k, mopt) => {
                        let ov = self.eval(o, sc)?;
                        if *mopt && matches!(ov, Value::Undef | Value::Null) {
                            return Ok(Value::Undef);
                        }
                        let fv = self.get(&ov, k)?;
                        if !fv.is_callable() && !*opt {
                            return self.type_error(&format!("{} is not a function", describe_callee(f)));
                        }
                        (fv, ov)
                    }
                    Expr::Index(o, k, _) => {
                        let ov = self.eval(o, sc)?;
                        let kv = self.eval(k, sc)?;
                        (self.get_index(&ov, &kv)?, ov)
                    }
                    _ => (self.eval(f, sc)?, Value::Undef),
                };
                if *opt && matches!(fv, Value::Undef | Value::Null) {
                    return Ok(Value::Undef);
                }
                if !fv.is_callable() {
                    return self.type_error(&format!("{} is not a function", describe_callee(f)));
                }
                let a = self.eval_args(args, sc)?;
                let line = self.line;
                let r = self.call(&fv, &this, &a);
                self.line = line;
                r?
            }
            Expr::New(c, args) => {
                let cv = self.eval(c, sc)?;
                let a = self.eval_args(args, sc)?;
                if !cv.is_callable() {
                    return self.type_error(&format!("{} is not a constructor", describe_callee(c)));
                }
                self.construct(&cv, &a)?
            }
        })
    }

    pub fn construct(&mut self, cv: &Value, a: &[Value]) -> Res {
        let proto = match self.get(cv, "prototype")? {
            Value::Obj(p) => p,
            _ => self.object_proto.clone(),
        };
        let this = Value::Obj(obj(Some(proto), Kind::Plain));
        let r = self.call(cv, &this, a)?;
        Ok(if matches!(r, Value::Obj(_)) { r } else { this })
    }

    fn store(&mut self, target: &Expr, v: Value, sc: &Scope) -> Result<(), Abort> {
        match target {
            Expr::Id(n, _) => {
                self.assign_var(sc, n, v);
                Ok(())
            }
            Expr::Member(o, k, _) => {
                let ov = self.eval(o, sc)?;
                self.put(&ov, k, v)
            }
            Expr::Index(o, k, _) => {
                let ov = self.eval(o, sc)?;
                let kv = self.eval(k, sc)?;
                self.put_index(&ov, &kv, v)
            }
            _ => Err(self.error("SyntaxError", "Invalid assignment target")),
        }
    }

    pub fn binary(&mut self, op: &str, x: &Value, y: &Value) -> Res {
        Ok(match op {
            "+" => {
                let a = self.to_primitive(x)?;
                let b = self.to_primitive(y)?;
                if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
                    let mut s = self.to_string(&a)?;
                    s.push_str(&self.to_string(&b)?);
                    Value::Str(Rc::from(s))
                } else {
                    Value::Num(self.to_num(&a) + self.to_num(&b))
                }
            }
            "-" => Value::Num(self.to_num(x) - self.to_num(y)),
            "*" => Value::Num(self.to_num(x) * self.to_num(y)),
            "/" => Value::Num(self.to_num(x) / self.to_num(y)),
            "%" => Value::Num(self.to_num(x) % self.to_num(y)),
            "**" => Value::Num(self.to_num(x).powf(self.to_num(y))),
            "==" => Value::Bool(self.loose_eq(x, y)?),
            "!=" => Value::Bool(!self.loose_eq(x, y)?),
            "===" => Value::Bool(strict_eq(x, y)),
            "!==" => Value::Bool(!strict_eq(x, y)),
            "<" | ">" | "<=" | ">=" => {
                let a = self.to_primitive(x)?;
                let b = self.to_primitive(y)?;
                if let (Value::Str(s), Value::Str(t)) = (&a, &b) {
                    Value::Bool(match op {
                        "<" => s < t,
                        ">" => s > t,
                        "<=" => s <= t,
                        _ => s >= t,
                    })
                } else {
                    let (p, q) = (self.to_num(&a), self.to_num(&b));
                    Value::Bool(match op {
                        "<" => p < q,
                        ">" => p > q,
                        "<=" => p <= q,
                        _ => p >= q,
                    })
                }
            }
            "&" => Value::Num((to_i32(self.to_num(x)) & to_i32(self.to_num(y))) as f64),
            "|" => Value::Num((to_i32(self.to_num(x)) | to_i32(self.to_num(y))) as f64),
            "^" => Value::Num((to_i32(self.to_num(x)) ^ to_i32(self.to_num(y))) as f64),
            "<<" => Value::Num((to_i32(self.to_num(x)).wrapping_shl(to_i32(self.to_num(y)) as u32 & 31)) as f64),
            ">>" => Value::Num((to_i32(self.to_num(x)) >> (to_i32(self.to_num(y)) as u32 & 31)) as f64),
            ">>>" => Value::Num(((to_i32(self.to_num(x)) as u32) >> (to_i32(self.to_num(y)) as u32 & 31)) as f64),
            "instanceof" => {
                if !y.is_callable() {
                    return self.type_error("Right side of instanceof is not callable");
                }
                let Value::Obj(target) = self.get(y, "prototype")? else { return Ok(Value::Bool(false)) };
                let mut p = self.proto_of(x).filter(|_| matches!(x, Value::Obj(_)));
                while let Some(o) = p {
                    if Rc::ptr_eq(&o, &target) {
                        return Ok(Value::Bool(true));
                    }
                    p = o.borrow().proto.clone();
                }
                Value::Bool(false)
            }
            "in" => {
                let k = self.to_string(x)?;
                if !matches!(y, Value::Obj(_)) {
                    return self.type_error("'in' needs an object");
                }
                Value::Bool(self.keys(y).iter().any(|q| **q == *k) || !self.get(y, &k)?.is_undef())
            }
            _ => return self.type_error(&format!("Unknown operator {op}")),
        })
    }

    fn loose_eq(&mut self, x: &Value, y: &Value) -> Result<bool, Abort> {
        Ok(match (x, y) {
            (Value::Undef | Value::Null, Value::Undef | Value::Null) => true,
            (Value::Undef | Value::Null, _) | (_, Value::Undef | Value::Null) => false,
            (Value::Obj(_), Value::Obj(_)) => strict_eq(x, y),
            (Value::Obj(_), _) => {
                let p = self.to_primitive(x)?;
                self.loose_eq(&p, y)?
            }
            (_, Value::Obj(_)) => {
                let p = self.to_primitive(y)?;
                self.loose_eq(x, &p)?
            }
            (Value::Str(a), Value::Str(b)) => a == b,
            _ => self.to_num(x) == self.to_num(y),
        })
    }

    /// Calls a function value with `this` and arguments.
    pub fn call(&mut self, f: &Value, this: &Value, args: &[Value]) -> Res {
        self.step()?;
        let Value::Obj(o) = f else { return self.type_error("Not a function") };
        enum C {
            N(NativeFn),
            F(Rc<FuncDef>, Scope),
        }
        let c = match &o.borrow().kind {
            Kind::Native(n) => C::N(n.clone()),
            Kind::Func(d, s) => C::F(d.clone(), s.clone()),
            _ => return self.type_error("Not a function"),
        };
        if self.depth > MAX_DEPTH {
            return Err(self.error("RangeError", "Maximum call stack size exceeded"));
        }
        self.depth += 1;
        let r = match c {
            C::N(n) => n(self, this, args),
            C::F(d, closure) => {
                let fs = new_scope(Some(closure), true);
                {
                    let mut b = fs.borrow_mut();
                    if !d.arrow {
                        b.vars.insert(Rc::from("this"), this.clone());
                        b.vars.insert(Rc::from("arguments"), self.new_array(args.to_vec()));
                    }
                }
                let mut r = Ok(());
                for (k, (n, def)) in d.params.iter().enumerate() {
                    let mut v = args.get(k).cloned().unwrap_or_default();
                    if v.is_undef() {
                        if let Some(e) = def {
                            match self.eval(e, &fs) {
                                Ok(x) => v = x,
                                Err(e) => {
                                    r = Err(e);
                                    break;
                                }
                            }
                        }
                    }
                    fs.borrow_mut().vars.insert(n.clone(), v);
                }
                if let Some(rest) = &d.rest {
                    let extra = args.get(d.params.len()..).map(|s| s.to_vec()).unwrap_or_default();
                    let a = self.new_array(extra);
                    fs.borrow_mut().vars.insert(rest.clone(), a);
                }
                match r {
                    Err(e) => Err(e),
                    Ok(()) => {
                        self.hoist(&d.body, &fs);
                        let mut out = Ok(Value::Undef);
                        for st in &d.body {
                            match self.exec(st, &fs) {
                                Ok(Flow::Normal) => {}
                                Ok(Flow::Return(v)) => {
                                    out = Ok(v);
                                    break;
                                }
                                Ok(_) => break,
                                Err(e) => {
                                    out = Err(e);
                                    break;
                                }
                            }
                        }
                        out
                    }
                }
            }
        };
        self.depth -= 1;
        r
    }

    /// Calls a global function by name if the script defines one; None if it doesn't.
    pub fn call_global(&mut self, name: &str, args: &[Value]) -> Option<Res> {
        let f = self.get_global(name);
        if !f.is_callable() {
            return None;
        }
        Some(self.call(&f, &Value::Undef, args))
    }

    /// A value as the console shows it (strings bare, arrays and objects spelled out).
    pub fn show(&mut self, v: &Value) -> String {
        match v {
            Value::Obj(o) if !v.is_callable() => {
                let is_plain = matches!(o.borrow().kind, Kind::Plain) && !self.is_error(v);
                let has_own_ts = o.borrow().proto.as_ref().is_some_and(|p| !Rc::ptr_eq(p, &self.object_proto)) && self.get(v, "toString").ok().is_some_and(|f| f.is_callable());
                if is_plain && !has_own_ts {
                    crate::js_builtins::json(self, v, 0).unwrap_or_else(|| "[object Object]".into())
                } else if let Kind::Array(_) = o.borrow().kind {
                    crate::js_builtins::json(self, v, 0).unwrap_or_default()
                } else {
                    self.to_string(v).unwrap_or_default()
                }
            }
            _ => self.to_string(v).unwrap_or_default(),
        }
    }
}

fn describe_callee(e: &Expr) -> String {
    match e {
        Expr::Id(n, _) => n.to_string(),
        Expr::Member(o, k, _) => format!("{}.{k}", describe_callee(o)),
        _ => "value".into(),
    }
}

/// `var f = function () {}` names the function f (for messages).
fn name_func(_v: &Value, _n: &str) {}

pub fn strict_eq(x: &Value, y: &Value) -> bool {
    match (x, y) {
        (Value::Undef, Value::Undef) | (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Num(a), Value::Num(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Obj(a), Value::Obj(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

pub fn to_i32(n: f64) -> i32 {
    if !n.is_finite() {
        return 0;
    }
    (n.trunc() as i64 as u64 & 0xFFFF_FFFF) as u32 as i32
}

pub fn str_to_num(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(h, 16).map(|n| n as f64).unwrap_or(f64::NAN);
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ => {
            if t.chars().any(|c| c.is_alphabetic() && c != 'e' && c != 'E') {
                f64::NAN
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
    }
}

/// Writes a quoted JSON string.
pub fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
