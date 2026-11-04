//! Math Notes: typed notes whose lines are evaluated top to bottom as you type (iOS 18).
//! "expr =" shows a result, "name = expr" defines a variable, "y = …x…" / "f(x) = …" is graphable,
//! text before a colon is a label, and everything else is prose and shows nothing.

use crate::expr::{eval, tokenize, Ctx, EvalError, Tok};
use crate::format::{format_value, NumberStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Digits shown in a result.
const MAX_DIGITS: usize = 12;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: u64,
    pub text: String,
    /// Unix seconds.
    pub modified: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Notes {
    pub notes: Vec<Note>,
    #[serde(default)]
    next_id: u64,
}

impl Notes {
    /// Adds an empty note and returns its id.
    pub fn create(&mut self, time: i64) -> u64 {
        let id = self.next_id.max(self.notes.iter().map(|n| n.id + 1).max().unwrap_or(0));
        self.next_id = id + 1;
        self.notes.push(Note { id, text: String::new(), modified: time });
        id
    }

    pub fn set_text(&mut self, id: u64, text: String, time: i64) {
        if let Some(n) = self.notes.iter_mut().find(|n| n.id == id) {
            if n.text != text {
                n.text = text;
                n.modified = time;
            }
        }
    }

    pub fn delete(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
    }

    pub fn get(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    /// A missing or unreadable file gives no notes.
    pub fn load(path: &Path) -> Notes {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Writes a temporary file next to `path` and renames it over.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::history::save_json(self, path)
    }

    /// Newest modified first.
    pub fn sorted(&self) -> Vec<&Note> {
        let mut v: Vec<&Note> = self.notes.iter().collect();
        v.sort_by(|a, b| b.modified.cmp(&a.modified).then(b.id.cmp(&a.id)));
        v
    }
}

/// The first non-empty line, or "New Note".
pub fn title(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("New Note").to_string()
}

/// The second non-empty line, or "".
pub fn preview(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).nth(1).unwrap_or("").to_string()
}

/// What a line produces. `kind` is "result" (text after the "="), "error" ("Error"), "assign"
/// (a variable was defined; text is its value) or "graph" (text is the display label).
/// `var` is the defined variable, or the function name of a graph ("y", "f").
#[derive(Clone, Debug, Serialize)]
pub struct LineResult {
    pub line: usize,
    pub kind: &'static str,
    pub text: String,
    pub var: Option<String>,
}

/// A line split into its parts (label and trailing "=" removed).
enum Line<'a> {
    /// "expr ="
    Expr(&'a str),
    /// "name = expr", `show` when it also ends with "="
    Assign { name: &'a str, rhs: &'a str, show: bool },
    /// "f(x) = expr": the name and the right side
    Func { name: &'a str, rhs: &'a str },
    None,
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_alphabetic()) && c.all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn parse(line: &str) -> Line<'_> {
    let mut s = line.trim();
    if let Some(i) = s.find(':') {
        s = s[i + 1..].trim();
    }
    let show = s.ends_with('=');
    let body = if show { s[..s.len() - 1].trim_end() } else { s };
    if let Some(i) = body.find('=') {
        let (lhs, rhs) = (body[..i].trim(), body[i + 1..].trim());
        if rhs.is_empty() || rhs.contains('=') {
            return Line::None;
        }
        if is_ident(lhs) {
            return Line::Assign { name: lhs, rhs, show };
        }
        let compact: String = lhs.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(name) = compact.strip_suffix("(x)") {
            if is_ident(name) && !show {
                let name = &lhs[..lhs.find('(').unwrap_or(lhs.len())];
                return Line::Func { name: name.trim(), rhs };
            }
        }
        return Line::None;
    }
    if show && !body.is_empty() {
        Line::Expr(body)
    } else {
        Line::None
    }
}

fn value(src: &str, vars: &HashMap<String, f64>, rad: bool) -> Result<f64, EvalError> {
    let toks = tokenize(src, &|w| vars.contains_key(w))?;
    if toks.is_empty() {
        return Err(EvalError::Syntax);
    }
    eval(&toks, &Ctx { rad, vars: Some(vars) })
}

/// The tokens of a graph's right side when it is a function of an undefined x.
fn graph_toks(rhs: &str, vars: &HashMap<String, f64>, rad: bool) -> Option<Vec<Tok>> {
    if vars.contains_key("x") {
        return None;
    }
    let toks = tokenize(rhs, &|w| w == "x" || vars.contains_key(w)).ok()?;
    if !toks.iter().any(|t| matches!(t, Tok::Var(v) if v == "x")) {
        return None;
    }
    let mut vs = vars.clone();
    vs.insert("x".into(), 1.0);
    match eval(&toks, &Ctx { rad, vars: Some(&vs) }) {
        Err(EvalError::Syntax | EvalError::Unknown(_)) => None,
        _ => Some(toks),
    }
}

/// Walks the lines up to `stop` (exclusive), collecting results and variables.
fn run(text: &str, rad: bool, style: NumberStyle, stop: usize) -> (Vec<LineResult>, HashMap<String, f64>) {
    let mut vars: HashMap<String, f64> = HashMap::new();
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate().take(stop) {
        let res = |kind, text: String, var: Option<&str>| LineResult { line: n, kind, text, var: var.map(String::from) };
        match parse(line) {
            Line::Expr(e) => match value(e, &vars, rad) {
                Ok(v) => out.push(res("result", format_value(v, MAX_DIGITS, style), None)),
                Err(_) => out.push(res("error", "Error".into(), None)),
            },
            Line::Assign { name, rhs, show } => {
                if name == "y" && graph_toks(rhs, &vars, rad).is_some() {
                    out.push(res("graph", label(name, rhs), Some(name)));
                    continue;
                }
                match value(rhs, &vars, rad) {
                    Ok(v) => {
                        let kind = if show { "result" } else { "assign" };
                        out.push(res(kind, format_value(v, MAX_DIGITS, style), Some(name)));
                        vars.insert(name.to_string(), v);
                    }
                    Err(_) if show => out.push(res("error", "Error".into(), Some(name))),
                    Err(_) => {}
                }
            }
            Line::Func { name, rhs } => {
                if graph_toks(rhs, &vars, rad).is_some() {
                    out.push(res("graph", label(&format!("{name}(x)"), rhs), Some(name)));
                }
            }
            Line::None => {}
        }
    }
    (out, vars)
}

fn label(lhs: &str, rhs: &str) -> String {
    format!("{lhs} = {rhs}")
}

/// Results of the lines that produce something, top to bottom.
pub fn evaluate(text: &str, rad: bool, style: NumberStyle) -> Vec<LineResult> {
    run(text, rad, style, usize::MAX).0
}

/// y values of a graphable line for xs evenly spaced in [x0,x1] (n samples), using variables
/// defined above that line; None where undefined.
pub fn graph(text: &str, line: usize, rad: bool, x0: f64, x1: f64, n: usize) -> Vec<Option<f64>> {
    let (_, vars) = run(text, rad, NumberStyle::default(), line);
    let rhs = match text.lines().nth(line).map(parse) {
        Some(Line::Assign { name: "y", rhs, .. } | Line::Func { rhs, .. }) => rhs,
        _ => return vec![None; n],
    };
    let Some(toks) = graph_toks(rhs, &vars, rad) else { return vec![None; n] };
    let mut vs = vars;
    (0..n)
        .map(|i| {
            let x = if n < 2 { x0 } else { x0 + (x1 - x0) * i as f64 / (n - 1) as f64 };
            vs.insert("x".into(), x);
            eval(&toks, &Ctx { rad, vars: Some(&vs) }).ok()
        })
        .collect()
}

/// The graphable lines: (line index, display label like "y = Quantity × (0.5^(x / HalfLife))").
pub fn graphs(text: &str) -> Vec<(usize, String)> {
    evaluate(text, false, NumberStyle::default()).into_iter().filter(|r| r.kind == "graph").map(|r| (r.line, r.text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAFFEINE: &str = "Caffeine Half-Life\nIf you consume 100mg of caffeine, what is the six hour half life of the caffeine in your body?\nQuantity = 100mg\nHalfLife = 6\ny= Quantity × (0.5^(x / HalfLife))";

    fn ev(s: &str) -> Vec<LineResult> {
        evaluate(s, false, NumberStyle::Comma)
    }

    #[test]
    fn caffeine_note() {
        let r = ev(CAFFEINE);
        assert!(r.iter().all(|r| r.line >= 2), "prose lines produce nothing");
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].kind, r[0].text.as_str(), r[0].var.as_deref()), ("assign", "100", Some("Quantity")));
        assert_eq!((r[1].kind, r[1].text.as_str()), ("assign", "6"));
        assert_eq!((r[2].line, r[2].kind), (4, "graph"));
        assert_eq!(graphs(CAFFEINE), vec![(4, "y = Quantity × (0.5^(x / HalfLife))".to_string())]);
        let ys = graph(CAFFEINE, 4, false, 0.0, 12.0, 3);
        assert_eq!(ys, vec![Some(100.0), Some(50.0), Some(25.0)]);
        assert!(graph(CAFFEINE, 3, false, 0.0, 1.0, 2).iter().all(Option::is_none));
    }

    #[test]
    fn results_and_variables() {
        let r = ev("a = 5\nb = a × 2\nb + 1 =");
        let last = r.last().unwrap();
        assert_eq!((last.line, last.kind, last.text.as_str()), (2, "result", "11"));
        assert_eq!(ev("12×4 =")[0].text, "48");
        assert_eq!(ev("12×4 =  ")[0].text, "48");
        assert_eq!(ev("1/0 =")[0].kind, "error");
        assert_eq!(ev("1/0 =")[0].text, "Error");
        let r = ev("a = 2\nb = 3\ntotal = a + b =");
        assert_eq!((r[2].kind, r[2].text.as_str(), r[2].var.as_deref()), ("result", "5", Some("total")));
        assert_eq!(ev("Rent: 1200 × 12 =")[0].text, "14,400");
        assert_eq!(ev("1/3 =")[0].text, "0.333333333333");
        // later references only
        assert_eq!(ev("c × 2 =\nc = 4")[0].kind, "error");
    }

    #[test]
    fn prose_is_silent() {
        assert!(ev("Note = hello world").is_empty());
        assert!(ev("Shopping list\nmilk, eggs\nWhat is this? Nothing.").is_empty());
        assert!(ev("a == b").is_empty());
        assert!(ev("g = x + 1").is_empty());
    }

    #[test]
    fn graph_forms() {
        let t = "k = 3\nf(x) = k x + 1\ny = 2";
        let r = ev(t);
        assert_eq!((r[1].kind, r[1].text.as_str(), r[1].var.as_deref()), ("graph", "f(x) = k x + 1", Some("f")));
        assert_eq!((r[2].kind, r[2].text.as_str()), ("assign", "2"));
        assert_eq!(graph(t, 1, false, 0.0, 2.0, 2), vec![Some(1.0), Some(7.0)]);
        // a defined x makes y = …x… a plain assignment
        assert_eq!(ev("x = 2\ny = 3x")[1].kind, "assign");
    }

    #[test]
    fn notes_store() {
        let mut n = Notes::default();
        let a = n.create(10);
        let b = n.create(20);
        n.set_text(a, "Title\n\nSecond line\nthird".into(), 30);
        assert_eq!(n.sorted()[0].id, a);
        assert_eq!(title(&n.get(a).unwrap().text), "Title");
        assert_eq!(preview(&n.get(a).unwrap().text), "Second line");
        assert_eq!(title(""), "New Note");
        assert_eq!(preview("one"), "");
        let dir = std::env::temp_dir().join(format!("calc-notes-test-{}", std::process::id()));
        let path = dir.join("notes.json");
        n.save(&path).unwrap();
        let mut l = Notes::load(&path);
        assert_eq!(l.notes.len(), 2);
        assert_eq!(l.get(a).unwrap().text, n.get(a).unwrap().text);
        l.delete(b);
        assert!(l.get(b).is_none());
        assert_eq!(l.create(40), 2);
        assert!(Notes::load(&dir.join("missing.json")).notes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
