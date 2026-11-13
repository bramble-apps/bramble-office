//! The built-in objects of the Scripter interpreter (js.rs): Math, String, Number, Array,
//! Object, JSON.stringify, Error, console, parseInt and friends.

use crate::js::{num_to_string, obj, quote, Abort, Interp, Kind, ObjRef, Res, Value};
use std::rc::Rc;

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

fn method(it: &Interp, o: &ObjRef, name: &str, f: impl Fn(&mut Interp, &Value, &[Value]) -> Res + 'static) {
    let v = it.native(f);
    o.borrow_mut().set_own(name, v);
}

fn num(it: &Interp, a: &[Value], i: usize) -> f64 {
    it.to_num(&arg(a, i))
}

/// The array behind `this` (a copy), or a TypeError.
fn this_array(it: &Interp, this: &Value) -> Result<Vec<Value>, Abort> {
    if let Value::Obj(o) = this {
        if let Kind::Array(a) = &o.borrow().kind {
            return Ok(a.clone());
        }
    }
    it.type_error("Not an array")
}

fn with_array<T>(it: &Interp, this: &Value, f: impl FnOnce(&mut Vec<Value>) -> T) -> Result<T, Abort> {
    if let Value::Obj(o) = this {
        if let Kind::Array(a) = &mut o.borrow_mut().kind {
            return Ok(f(a));
        }
    }
    it.type_error("Not an array")
}

fn this_str(it: &mut Interp, this: &Value) -> Result<String, Abort> {
    it.to_string(this)
}

/// Relative index (negative counts from the end), clamped to 0..=len.
fn rel(n: f64, len: usize) -> usize {
    if n.is_nan() {
        return 0;
    }
    let n = n.trunc();
    if n < 0.0 { (len as f64 + n).max(0.0) as usize } else { (n as usize).min(len) }
}

/// Stable merge sort with a comparator that may fail (no panics on a bad comparator).
fn sort_values(it: &mut Interp, v: Vec<Value>, cmp: &Value) -> Result<Vec<Value>, Abort> {
    if v.len() <= 1 {
        return Ok(v);
    }
    let mid = v.len() / 2;
    let mut right = v;
    let left = right.drain(..mid).collect::<Vec<_>>();
    let left = sort_values(it, left, cmp)?;
    let right = sort_values(it, right, cmp)?;
    let mut out = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let gt = compare(it, &left[i], &right[j], cmp)? > 0.0;
        if gt {
            out.push(right[j].clone());
            j += 1;
        } else {
            out.push(left[i].clone());
            i += 1;
        }
    }
    out.extend_from_slice(&left[i..]);
    out.extend_from_slice(&right[j..]);
    Ok(out)
}

fn compare(it: &mut Interp, a: &Value, b: &Value, cmp: &Value) -> Result<f64, Abort> {
    // undefined sorts last
    match (a.is_undef(), b.is_undef()) {
        (true, true) => return Ok(0.0),
        (true, false) => return Ok(1.0),
        (false, true) => return Ok(-1.0),
        _ => {}
    }
    if cmp.is_callable() {
        let r = it.call(cmp, &Value::Undef, &[a.clone(), b.clone()])?;
        let n = it.to_num(&r);
        return Ok(if n.is_nan() { 0.0 } else { n });
    }
    let (s, t) = (it.to_string(a)?, it.to_string(b)?);
    Ok(match s.cmp(&t) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    })
}

/// JSON text for a value (None for undefined / functions); `ind` spaces of indent per level.
pub fn json(it: &mut Interp, v: &Value, ind: usize) -> Option<String> {
    fn go(it: &mut Interp, v: &Value, ind: usize, lvl: usize, seen: &mut Vec<*const ()>) -> Option<String> {
        match v {
            Value::Undef => None,
            Value::Null => Some("null".into()),
            Value::Bool(b) => Some(b.to_string()),
            Value::Num(n) => Some(if n.is_finite() { num_to_string(*n) } else { "null".into() }),
            Value::Str(s) => Some(quote(s)),
            Value::Obj(o) => {
                if v.is_callable() {
                    return None;
                }
                let p = Rc::as_ptr(o) as *const ();
                if seen.contains(&p) || lvl > 20 {
                    return Some("\"[Circular]\"".into());
                }
                seen.push(p);
                let (nl, pad, pad_end) = if ind > 0 { ("\n", " ".repeat(ind * (lvl + 1)), " ".repeat(ind * lvl)) } else { ("", String::new(), String::new()) };
                let arr = if let Kind::Array(a) = &o.borrow().kind { Some(a.clone()) } else { None };
                let out = if let Some(a) = arr {
                    if a.is_empty() {
                        "[]".to_string()
                    } else {
                        let parts: Vec<String> = a.iter().map(|x| format!("{pad}{}", go(it, x, ind, lvl + 1, seen).unwrap_or_else(|| "null".into()))).collect();
                        format!("[{nl}{}{nl}{pad_end}]", parts.join(&format!(",{nl}")))
                    }
                } else {
                    let props = o.borrow().props.clone();
                    let sep = if ind > 0 { ": " } else { ":" };
                    let parts: Vec<String> = props.iter().filter_map(|(k, x)| go(it, x, ind, lvl + 1, seen).map(|s| format!("{pad}{}{sep}{s}", quote(k)))).collect();
                    if parts.is_empty() { "{}".to_string() } else { format!("{{{nl}{}{nl}{pad_end}}}", parts.join(&format!(",{nl}"))) }
                };
                seen.pop();
                Some(out)
            }
        }
    }
    go(it, v, ind, 0, &mut Vec::new())
}

/// A tiny JSON reader for JSON.parse.
fn json_parse(it: &mut Interp, s: &str) -> Res {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    fn ws(c: &[char], i: &mut usize) {
        while *i < c.len() && c[*i].is_whitespace() {
            *i += 1;
        }
    }
    fn val(it: &mut Interp, c: &[char], i: &mut usize) -> Option<Value> {
        ws(c, i);
        let ch = *c.get(*i)?;
        match ch {
            '{' => {
                *i += 1;
                let o = it.new_object();
                ws(c, i);
                if c.get(*i) == Some(&'}') {
                    *i += 1;
                    return Some(o);
                }
                loop {
                    ws(c, i);
                    let Value::Str(k) = val(it, c, i)? else { return None };
                    ws(c, i);
                    if c.get(*i) != Some(&':') {
                        return None;
                    }
                    *i += 1;
                    let v = val(it, c, i)?;
                    it.put(&o, &k, v).ok()?;
                    ws(c, i);
                    match c.get(*i) {
                        Some(',') => *i += 1,
                        Some('}') => {
                            *i += 1;
                            return Some(o);
                        }
                        _ => return None,
                    }
                }
            }
            '[' => {
                *i += 1;
                let mut v = Vec::new();
                ws(c, i);
                if c.get(*i) == Some(&']') {
                    *i += 1;
                    return Some(it.new_array(v));
                }
                loop {
                    v.push(val(it, c, i)?);
                    ws(c, i);
                    match c.get(*i) {
                        Some(',') => *i += 1,
                        Some(']') => {
                            *i += 1;
                            return Some(it.new_array(v));
                        }
                        _ => return None,
                    }
                }
            }
            '"' => {
                *i += 1;
                let mut s = String::new();
                while *i < c.len() && c[*i] != '"' {
                    if c[*i] == '\\' {
                        *i += 1;
                        match c.get(*i)? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            'u' => {
                                let h: String = c.get(*i + 1..*i + 5)?.iter().collect();
                                s.push(char::from_u32(u32::from_str_radix(&h, 16).ok()?)?);
                                *i += 4;
                            }
                            o => s.push(*o),
                        }
                    } else {
                        s.push(c[*i]);
                    }
                    *i += 1;
                }
                *i += 1;
                Some(Value::Str(Rc::from(s)))
            }
            't' if c[*i..].starts_with(&['t', 'r', 'u', 'e']) => {
                *i += 4;
                Some(Value::Bool(true))
            }
            'f' if c[*i..].starts_with(&['f', 'a', 'l', 's', 'e']) => {
                *i += 5;
                Some(Value::Bool(false))
            }
            'n' if c[*i..].starts_with(&['n', 'u', 'l', 'l']) => {
                *i += 4;
                Some(Value::Null)
            }
            _ => {
                let st = *i;
                while *i < c.len() && (c[*i].is_ascii_digit() || "+-.eE".contains(c[*i])) {
                    *i += 1;
                }
                let t: String = c[st..*i].iter().collect();
                t.parse::<f64>().ok().map(Value::Num)
            }
        }
    }
    match val(it, &c, &mut i) {
        Some(v) => {
            ws(&c, &mut i);
            if i < c.len() {
                return Err(it.error("SyntaxError", "JSON Parse error: Unexpected text"));
            }
            Ok(v)
        }
        None => Err(it.error("SyntaxError", "JSON Parse error")),
    }
}

fn parse_int(s: &str, radix: u32) -> f64 {
    let t = s.trim_start();
    let (neg, t) = if let Some(r) = t.strip_prefix('-') { (true, r) } else { (false, t.strip_prefix('+').unwrap_or(t)) };
    let (radix, t) = if (radix == 16 || radix == 0) && (t.starts_with("0x") || t.starts_with("0X")) { (16, &t[2..]) } else { (if radix == 0 { 10 } else { radix }, t) };
    let digits: String = t.chars().take_while(|c| c.is_digit(radix)).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut n = 0.0;
    for d in digits.chars() {
        n = n * radix as f64 + d.to_digit(radix).unwrap_or(0) as f64;
    }
    if neg { -n } else { n }
}

fn parse_float(s: &str) -> f64 {
    let t = s.trim_start();
    if t.starts_with("Infinity") || t.starts_with("+Infinity") {
        return f64::INFINITY;
    }
    if t.starts_with("-Infinity") {
        return f64::NEG_INFINITY;
    }
    let mut end = 0;
    let mut seen_e = false;
    let mut seen_dot = false;
    let b = t.as_bytes();
    while end < b.len() {
        let ch = b[end] as char;
        let ok = ch.is_ascii_digit() || ((ch == '+' || ch == '-') && (end == 0 || matches!(b[end - 1], b'e' | b'E'))) || (ch == '.' && !seen_dot && !seen_e) || ((ch == 'e' || ch == 'E') && !seen_e && end > 0);
        if !ok {
            break;
        }
        seen_e |= ch == 'e' || ch == 'E';
        seen_dot |= ch == '.';
        end += 1;
    }
    let mut s = &t[..end];
    while !s.is_empty() && s.parse::<f64>().is_err() {
        s = &s[..s.len() - 1];
    }
    s.parse::<f64>().unwrap_or(f64::NAN)
}

/// A random number source for Math.random (xorshift; seeded from the clock).
fn rand01(state: &std::cell::Cell<u64>) -> f64 {
    let mut x = state.get();
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    state.set(x);
    (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
}

pub fn install(it: &mut Interp) {
    // ---- Object.prototype, Function.prototype
    let op = it.object_proto.clone();
    method(it, &op, "hasOwnProperty", |it, this, a| {
        let k = it.to_string(&arg(a, 0))?;
        Ok(Value::Bool(it.keys(this).iter().any(|q| **q == *k)))
    });
    method(it, &op, "toString", |it, this, _| Ok(Value::str(if it.is_error(this) { "Error" } else { "[object Object]" })));
    let fp = it.func_proto.clone();
    method(it, &fp, "call", |it, this, a| {
        let t = arg(a, 0);
        it.call(this, &t, a.get(1..).unwrap_or(&[]))
    });
    method(it, &fp, "apply", |it, this, a| {
        let t = arg(a, 0);
        let list = match arg(a, 1) {
            Value::Obj(o) => match &o.borrow().kind {
                Kind::Array(v) => v.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        it.call(this, &t, &list)
    });
    method(it, &fp, "bind", |it, this, a| {
        let f = this.clone();
        let t = arg(a, 0);
        let pre: Vec<Value> = a.get(1..).map(|s| s.to_vec()).unwrap_or_default();
        Ok(it.native(move |it, _, more| {
            let mut all = pre.clone();
            all.extend_from_slice(more);
            it.call(&f, &t, &all)
        }))
    });

    // ---- Error
    let ep = it.error_proto.clone();
    ep.borrow_mut().set_own("name", Value::str("Error"));
    ep.borrow_mut().set_own("message", Value::str(""));
    method(it, &ep, "toString", |it, this, _| {
        let n = it.get(this, "name")?;
        let m = it.get(this, "message")?;
        let (n, m) = (it.to_string(&n)?, it.to_string(&m)?);
        Ok(Value::Str(Rc::from(if m.is_empty() { n } else { format!("{n}: {m}") })))
    });
    for kind in ["Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError"] {
        let proto = if kind == "Error" { ep.clone() } else { obj(Some(ep.clone()), Kind::Plain) };
        proto.borrow_mut().set_own("name", Value::str(kind));
        let p2 = proto.clone();
        let c = it.native_ctor(proto, move |it, this, a| {
            let this = if this.as_obj().is_some_and(|o| o.borrow().proto.as_ref().is_some_and(|p| Rc::ptr_eq(p, &p2))) { this.clone() } else { Value::Obj(obj(Some(p2.clone()), Kind::Plain)) };
            if !arg(a, 0).is_undef() {
                let m = it.to_string(&arg(a, 0))?;
                it.put(&this, "message", Value::Str(Rc::from(m)))?;
            }
            it.put(&this, "line", Value::Num(it.line as f64))?;
            Ok(this)
        });
        it.set_global(kind, c);
    }

    // ---- Math
    let math = it.new_object();
    let m = math.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
    m.borrow_mut().set_own("PI", Value::Num(std::f64::consts::PI));
    m.borrow_mut().set_own("E", Value::Num(std::f64::consts::E));
    m.borrow_mut().set_own("LN2", Value::Num(std::f64::consts::LN_2));
    m.borrow_mut().set_own("LN10", Value::Num(std::f64::consts::LN_10));
    m.borrow_mut().set_own("SQRT2", Value::Num(std::f64::consts::SQRT_2));
    let one: [(&str, fn(f64) -> f64); 20] = [
        ("abs", f64::abs),
        ("floor", f64::floor),
        ("ceil", f64::ceil),
        ("round", |x| (x + 0.5).floor()),
        ("trunc", f64::trunc),
        ("sign", |x| if x > 0.0 { 1.0 } else if x < 0.0 { -1.0 } else { x }),
        ("sqrt", f64::sqrt),
        ("cbrt", f64::cbrt),
        ("exp", f64::exp),
        ("log", f64::ln),
        ("log2", f64::log2),
        ("log10", f64::log10),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("atan", f64::atan),
        ("sinh", f64::sinh),
        ("tanh", f64::tanh),
    ];
    for (n, f) in one {
        method(it, &m, n, move |it, _, a| Ok(Value::Num(f(num(it, a, 0)))));
    }
    method(it, &m, "atan2", |it, _, a| Ok(Value::Num(num(it, a, 0).atan2(num(it, a, 1)))));
    method(it, &m, "pow", |it, _, a| Ok(Value::Num(num(it, a, 0).powf(num(it, a, 1)))));
    method(it, &m, "min", |it, _, a| Ok(Value::Num(a.iter().map(|x| it.to_num(x)).fold(f64::INFINITY, |p, q| if p.is_nan() || q.is_nan() { f64::NAN } else { p.min(q) }))));
    method(it, &m, "max", |it, _, a| Ok(Value::Num(a.iter().map(|x| it.to_num(x)).fold(f64::NEG_INFINITY, |p, q| if p.is_nan() || q.is_nan() { f64::NAN } else { p.max(q) }))));
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(99) | 1;
    let state = std::cell::Cell::new(seed);
    method(it, &m, "random", move |_, _, _| Ok(Value::Num(rand01(&state))));
    it.set_global("Math", math);

    // ---- globals
    let f = it.native(|it, _, a| {
        let s = it.to_string(&arg(a, 0))?;
        let r = if arg(a, 1).is_undef() { 0 } else { it.to_int(&arg(a, 1)) as u32 };
        Ok(Value::Num(if r == 1 || r > 36 { f64::NAN } else { parse_int(&s, r) }))
    });
    it.set_global("parseInt", f);
    let f = it.native(|it, _, a| {
        let s = it.to_string(&arg(a, 0))?;
        Ok(Value::Num(parse_float(&s)))
    });
    it.set_global("parseFloat", f);
    let f = it.native(|it, _, a| Ok(Value::Bool(num(it, a, 0).is_nan())));
    it.set_global("isNaN", f);
    let f = it.native(|it, _, a| Ok(Value::Bool(num(it, a, 0).is_finite())));
    it.set_global("isFinite", f);

    // ---- Number
    let np = it.number_proto.clone();
    method(it, &np, "toFixed", |it, this, a| {
        let d = it.to_int(&arg(a, 0)).clamp(0, 100) as usize;
        let n = it.to_num(this);
        Ok(Value::Str(Rc::from(if n.is_finite() { format!("{n:.d$}") } else { num_to_string(n) })))
    });
    method(it, &np, "toString", |it, this, a| {
        let n = it.to_num(this);
        let r = if arg(a, 0).is_undef() { 10 } else { it.to_int(&arg(a, 0)) };
        if r == 10 || !n.is_finite() || n.fract() != 0.0 || !(2..=36).contains(&r) {
            return Ok(Value::Str(Rc::from(num_to_string(n))));
        }
        let (neg, mut k) = (n < 0.0, n.abs() as u64);
        let mut s = Vec::new();
        loop {
            s.push(std::char::from_digit((k % r as u64) as u32, r as u32).unwrap_or('0'));
            k /= r as u64;
            if k == 0 {
                break;
            }
        }
        if neg {
            s.push('-');
        }
        s.reverse();
        Ok(Value::Str(Rc::from(s.into_iter().collect::<String>())))
    });
    let nc = it.native_ctor(np, |it, _, a| Ok(Value::Num(if a.is_empty() { 0.0 } else { num(it, a, 0) })));
    {
        let c = nc.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
        method(it, &c, "isInteger", |_, _, a| Ok(Value::Bool(matches!(arg(a, 0), Value::Num(n) if n.is_finite() && n.fract() == 0.0))));
        method(it, &c, "isFinite", |_, _, a| Ok(Value::Bool(matches!(arg(a, 0), Value::Num(n) if n.is_finite()))));
        method(it, &c, "isNaN", |_, _, a| Ok(Value::Bool(matches!(arg(a, 0), Value::Num(n) if n.is_nan()))));
        let mut b = c.borrow_mut();
        b.set_own("MAX_SAFE_INTEGER", Value::Num(9007199254740991.0));
        b.set_own("MIN_SAFE_INTEGER", Value::Num(-9007199254740991.0));
        b.set_own("MAX_VALUE", Value::Num(f64::MAX));
        b.set_own("EPSILON", Value::Num(f64::EPSILON));
        b.set_own("POSITIVE_INFINITY", Value::Num(f64::INFINITY));
        b.set_own("NEGATIVE_INFINITY", Value::Num(f64::NEG_INFINITY));
    }
    it.set_global("Number", nc);
    let bc = it.native(|_, _, a| Ok(Value::Bool(arg(a, 0).truthy())));
    it.set_global("Boolean", bc);

    // ---- String
    let sp = it.string_proto.clone();
    method(it, &sp, "toString", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?))));
    method(it, &sp, "charAt", |it, this, a| {
        let s = this_str(it, this)?;
        let i = it.to_int(&arg(a, 0));
        Ok(Value::Str(Rc::from(if i < 0 { String::new() } else { s.chars().nth(i as usize).map(|c| c.to_string()).unwrap_or_default() })))
    });
    method(it, &sp, "charCodeAt", |it, this, a| {
        let s = this_str(it, this)?;
        let i = it.to_int(&arg(a, 0));
        Ok(Value::Num(if i < 0 { f64::NAN } else { s.encode_utf16().nth(i as usize).map(|c| c as f64).unwrap_or(f64::NAN) }))
    });
    method(it, &sp, "indexOf", |it, this, a| {
        let s: Vec<char> = this_str(it, this)?.chars().collect();
        let t: Vec<char> = it.to_string(&arg(a, 0))?.chars().collect();
        let from = rel(num(it, a, 1).max(0.0), s.len());
        Ok(Value::Num((from..=s.len().saturating_sub(t.len())).find(|&i| s.len() >= t.len() && s[i..i + t.len()] == t[..]).map(|i| i as f64).unwrap_or(-1.0)))
    });
    method(it, &sp, "lastIndexOf", |it, this, a| {
        let s: Vec<char> = this_str(it, this)?.chars().collect();
        let t: Vec<char> = it.to_string(&arg(a, 0))?.chars().collect();
        if t.len() > s.len() {
            return Ok(Value::Num(-1.0));
        }
        Ok(Value::Num((0..=s.len() - t.len()).rev().find(|&i| s[i..i + t.len()] == t[..]).map(|i| i as f64).unwrap_or(-1.0)))
    });
    method(it, &sp, "includes", |it, this, a| {
        let s = this_str(it, this)?;
        let t = it.to_string(&arg(a, 0))?;
        Ok(Value::Bool(s.contains(&t)))
    });
    method(it, &sp, "startsWith", |it, this, a| {
        let s = this_str(it, this)?;
        let t = it.to_string(&arg(a, 0))?;
        Ok(Value::Bool(s.starts_with(&t)))
    });
    method(it, &sp, "endsWith", |it, this, a| {
        let s = this_str(it, this)?;
        let t = it.to_string(&arg(a, 0))?;
        Ok(Value::Bool(s.ends_with(&t)))
    });
    method(it, &sp, "slice", |it, this, a| {
        let s: Vec<char> = this_str(it, this)?.chars().collect();
        let from = rel(num(it, a, 0), s.len());
        let to = if arg(a, 1).is_undef() { s.len() } else { rel(num(it, a, 1), s.len()) };
        Ok(Value::Str(Rc::from(if from < to { s[from..to].iter().collect::<String>() } else { String::new() })))
    });
    method(it, &sp, "substring", |it, this, a| {
        let s: Vec<char> = this_str(it, this)?.chars().collect();
        let cl = |n: f64| if n.is_nan() { 0 } else { (n.max(0.0) as usize).min(s.len()) };
        let from = cl(num(it, a, 0));
        let to = if arg(a, 1).is_undef() { s.len() } else { cl(num(it, a, 1)) };
        let (x, y) = (from.min(to), from.max(to));
        Ok(Value::Str(Rc::from(s[x..y].iter().collect::<String>())))
    });
    method(it, &sp, "substr", |it, this, a| {
        let s: Vec<char> = this_str(it, this)?.chars().collect();
        let from = rel(num(it, a, 0), s.len());
        let n = if arg(a, 1).is_undef() { s.len() } else { it.to_int(&arg(a, 1)).max(0) as usize };
        let to = (from + n).min(s.len());
        Ok(Value::Str(Rc::from(s[from..to].iter().collect::<String>())))
    });
    method(it, &sp, "toUpperCase", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?.to_uppercase()))));
    method(it, &sp, "toLowerCase", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?.to_lowercase()))));
    method(it, &sp, "trim", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?.trim()))));
    method(it, &sp, "trimStart", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?.trim_start()))));
    method(it, &sp, "trimEnd", |it, this, _| Ok(Value::Str(Rc::from(this_str(it, this)?.trim_end()))));
    method(it, &sp, "repeat", |it, this, a| {
        let s = this_str(it, this)?;
        let n = it.to_int(&arg(a, 0));
        if !(0..=100_000).contains(&n) || s.len() * n as usize > 10_000_000 {
            return Err(it.error("RangeError", "Invalid count value"));
        }
        Ok(Value::Str(Rc::from(s.repeat(n as usize))))
    });
    for (name, start) in [("padStart", true), ("padEnd", false)] {
        method(it, &sp, name, move |it, this, a| {
            let s = this_str(it, this)?;
            let n = it.to_int(&arg(a, 0)).clamp(0, 100_000) as usize;
            let pad = if arg(a, 1).is_undef() { " ".to_string() } else { it.to_string(&arg(a, 1))? };
            let len = s.chars().count();
            if n <= len || pad.is_empty() {
                return Ok(Value::Str(Rc::from(s)));
            }
            let fill: String = pad.chars().cycle().take(n - len).collect();
            Ok(Value::Str(Rc::from(if start { fill + &s } else { s + &fill })))
        });
    }
    method(it, &sp, "split", |it, this, a| {
        let s = this_str(it, this)?;
        let parts: Vec<Value> = if arg(a, 0).is_undef() {
            vec![Value::Str(Rc::from(s))]
        } else {
            let sep = it.to_string(&arg(a, 0))?;
            if sep.is_empty() { s.chars().map(|c| Value::Str(Rc::from(c.to_string()))).collect() } else { s.split(sep.as_str()).map(|p| Value::Str(Rc::from(p))).collect() }
        };
        Ok(it.new_array(parts))
    });
    for (name, all) in [("replace", false), ("replaceAll", true)] {
        method(it, &sp, name, move |it, this, a| {
            let s = this_str(it, this)?;
            let pat = it.to_string(&arg(a, 0))?;
            let rep = arg(a, 1);
            let mut out = String::new();
            let mut rest = s.as_str();
            let mut base = 0;
            while let Some(k) = rest.find(&pat) {
                out.push_str(&rest[..k]);
                let r = if rep.is_callable() {
                    let v = it.call(&rep, &Value::Undef, &[Value::Str(Rc::from(pat.as_str())), Value::Num((base + k) as f64), Value::Str(Rc::from(s.as_str()))])?;
                    it.to_string(&v)?
                } else {
                    it.to_string(&rep)?
                };
                out.push_str(&r);
                let adv = k + pat.len().max(1).min(rest.len() - k);
                if pat.is_empty() {
                    out.push_str(&rest[k..adv]);
                }
                base += adv;
                rest = &rest[adv..];
                if !all || rest.is_empty() {
                    break;
                }
            }
            out.push_str(rest);
            Ok(Value::Str(Rc::from(out)))
        });
    }
    method(it, &sp, "concat", |it, this, a| {
        let mut s = this_str(it, this)?;
        for x in a {
            s.push_str(&it.to_string(x)?);
        }
        Ok(Value::Str(Rc::from(s)))
    });
    let sc = it.native_ctor(sp, |it, _, a| Ok(Value::Str(Rc::from(if a.is_empty() { String::new() } else { it.to_string(&a[0])? }))));
    {
        let c = sc.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
        method(it, &c, "fromCharCode", |it, _, a| Ok(Value::Str(Rc::from(String::from_utf16_lossy(&a.iter().map(|x| it.to_int(x) as u16).collect::<Vec<u16>>())))));
    }
    it.set_global("String", sc);

    // ---- Array
    let ap = it.array_proto.clone();
    method(it, &ap, "push", |it, this, a| {
        let a = a.to_vec();
        with_array(it, this, |v| {
            v.extend(a);
            Value::Num(v.len() as f64)
        })
    });
    method(it, &ap, "pop", |it, this, _| with_array(it, this, |v| v.pop().unwrap_or_default()));
    method(it, &ap, "shift", |it, this, _| with_array(it, this, |v| if v.is_empty() { Value::Undef } else { v.remove(0) }));
    method(it, &ap, "unshift", |it, this, a| {
        let a = a.to_vec();
        with_array(it, this, |v| {
            v.splice(0..0, a);
            Value::Num(v.len() as f64)
        })
    });
    method(it, &ap, "splice", |it, this, a| {
        let len = this_array(it, this)?.len();
        let start = rel(num(it, a, 0), len);
        let del = if a.len() < 2 { len - start } else { (it.to_int(&a[1]).max(0) as usize).min(len - start) };
        let ins: Vec<Value> = a.get(2..).map(|s| s.to_vec()).unwrap_or_default();
        let removed = with_array(it, this, |v| v.splice(start..start + del, ins).collect::<Vec<_>>())?;
        Ok(it.new_array(removed))
    });
    method(it, &ap, "slice", |it, this, a| {
        let v = this_array(it, this)?;
        let from = rel(num(it, a, 0), v.len());
        let to = if arg(a, 1).is_undef() { v.len() } else { rel(num(it, a, 1), v.len()) };
        Ok(it.new_array(if from < to { v[from..to].to_vec() } else { Vec::new() }))
    });
    method(it, &ap, "concat", |it, this, a| {
        let mut v = this_array(it, this)?;
        for x in a {
            match x.as_obj().map(|o| if let Kind::Array(q) = &o.borrow().kind { Some(q.clone()) } else { None }) {
                Some(Some(q)) => v.extend(q),
                _ => v.push(x.clone()),
            }
        }
        Ok(it.new_array(v))
    });
    method(it, &ap, "join", |it, this, a| {
        let v = this_array(it, this)?;
        let sep = if arg(a, 0).is_undef() { ",".to_string() } else { it.to_string(&arg(a, 0))? };
        let mut parts = Vec::new();
        for x in &v {
            parts.push(if matches!(x, Value::Undef | Value::Null) { String::new() } else { it.to_string(x)? });
        }
        Ok(Value::Str(Rc::from(parts.join(&sep))))
    });
    method(it, &ap, "toString", |it, this, _| Ok(Value::Str(Rc::from(it.to_string(this)?))));
    method(it, &ap, "reverse", |it, this, _| {
        with_array(it, this, |v| v.reverse())?;
        Ok(this.clone())
    });
    method(it, &ap, "indexOf", |it, this, a| {
        let v = this_array(it, this)?;
        let t = arg(a, 0);
        Ok(Value::Num(v.iter().position(|x| crate::js::strict_eq(x, &t)).map(|i| i as f64).unwrap_or(-1.0)))
    });
    method(it, &ap, "lastIndexOf", |it, this, a| {
        let v = this_array(it, this)?;
        let t = arg(a, 0);
        Ok(Value::Num(v.iter().rposition(|x| crate::js::strict_eq(x, &t)).map(|i| i as f64).unwrap_or(-1.0)))
    });
    method(it, &ap, "includes", |it, this, a| {
        let v = this_array(it, this)?;
        let t = arg(a, 0);
        Ok(Value::Bool(v.iter().any(|x| crate::js::strict_eq(x, &t) || (matches!((x, &t), (Value::Num(p), Value::Num(q)) if p.is_nan() && q.is_nan())))))
    });
    method(it, &ap, "fill", |it, this, a| {
        let x = arg(a, 0);
        let len = this_array(it, this)?.len();
        let from = if arg(a, 1).is_undef() { 0 } else { rel(num(it, a, 1), len) };
        let to = if arg(a, 2).is_undef() { len } else { rel(num(it, a, 2), len) };
        with_array(it, this, |v| {
            for k in from..to.max(from) {
                v[k] = x.clone();
            }
        })?;
        Ok(this.clone())
    });
    method(it, &ap, "sort", |it, this, a| {
        let v = this_array(it, this)?;
        let s = sort_values(it, v, &arg(a, 0))?;
        with_array(it, this, |v| *v = s)?;
        Ok(this.clone())
    });
    // iteration methods: callback(element, index, array)
    for name in ["forEach", "map", "filter", "some", "every", "find", "findIndex"] {
        method(it, &ap, name, move |it, this, a| {
            let v = this_array(it, this)?;
            let f = arg(a, 0);
            if !f.is_callable() {
                return it.type_error(&format!("Array.{name} needs a function"));
            }
            let t = arg(a, 1);
            let mut out = Vec::new();
            for (i, x) in v.iter().enumerate() {
                let r = it.call(&f, &t, &[x.clone(), Value::Num(i as f64), this.clone()])?;
                match name {
                    "map" => out.push(r),
                    "filter" if r.truthy() => out.push(x.clone()),
                    "some" if r.truthy() => return Ok(Value::Bool(true)),
                    "every" if !r.truthy() => return Ok(Value::Bool(false)),
                    "find" if r.truthy() => return Ok(x.clone()),
                    "findIndex" if r.truthy() => return Ok(Value::Num(i as f64)),
                    _ => {}
                }
            }
            Ok(match name {
                "map" | "filter" => it.new_array(out),
                "some" => Value::Bool(false),
                "every" => Value::Bool(true),
                "findIndex" => Value::Num(-1.0),
                _ => Value::Undef,
            })
        });
    }
    for (name, right) in [("reduce", false), ("reduceRight", true)] {
        method(it, &ap, name, move |it, this, a| {
            let mut v: Vec<(usize, Value)> = this_array(it, this)?.into_iter().enumerate().collect();
            if right {
                v.reverse();
            }
            let f = arg(a, 0);
            let mut iter = v.into_iter();
            let mut acc = if a.len() >= 2 {
                a[1].clone()
            } else {
                match iter.next() {
                    Some((_, x)) => x,
                    None => return it.type_error("Reduce of empty array with no initial value"),
                }
            };
            for (i, x) in iter {
                acc = it.call(&f, &Value::Undef, &[acc, x, Value::Num(i as f64), this.clone()])?;
            }
            Ok(acc)
        });
    }
    let ac = it.native_ctor(ap, |it, _, a| {
        Ok(if a.len() == 1 && matches!(a[0], Value::Num(_)) {
            let n = it.to_num(&a[0]);
            if n < 0.0 || n.fract() != 0.0 || n > 1e7 {
                return Err(it.error("RangeError", "Invalid array length"));
            }
            it.new_array(vec![Value::Undef; n as usize])
        } else {
            it.new_array(a.to_vec())
        })
    });
    {
        let c = ac.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
        method(it, &c, "isArray", |_, _, a| Ok(Value::Bool(arg(a, 0).as_obj().is_some_and(|o| matches!(o.borrow().kind, Kind::Array(_))))));
        method(it, &c, "of", |it, _, a| Ok(it.new_array(a.to_vec())));
        method(it, &c, "from", |it, _, a| {
            let src = arg(a, 0);
            let v: Vec<Value> = match &src {
                Value::Str(s) => s.chars().map(|c| Value::Str(Rc::from(c.to_string()))).collect(),
                Value::Obj(o) => {
                    let arr = if let Kind::Array(q) = &o.borrow().kind { Some(q.clone()) } else { None };
                    match arr {
                        Some(q) => q,
                        None => {
                            let n = it.get(&src, "length")?;
                            let n = it.to_int(&n).clamp(0, 10_000_000) as usize;
                            let mut v = Vec::with_capacity(n);
                            for i in 0..n {
                                v.push(it.get(&src, &i.to_string())?);
                            }
                            v
                        }
                    }
                }
                _ => Vec::new(),
            };
            let f = arg(a, 1);
            if f.is_callable() {
                let mut out = Vec::with_capacity(v.len());
                for (i, x) in v.into_iter().enumerate() {
                    out.push(it.call(&f, &Value::Undef, &[x, Value::Num(i as f64)])?);
                }
                return Ok(it.new_array(out));
            }
            Ok(it.new_array(v))
        });
    }
    it.set_global("Array", ac);

    // ---- Object
    let opc = it.object_proto.clone();
    let oc = it.native_ctor(opc, |it, _, a| Ok(if let Value::Obj(_) = arg(a, 0) { arg(a, 0) } else { it.new_object() }));
    {
        let c = oc.as_obj().cloned().unwrap_or_else(|| obj(None, Kind::Plain));
        method(it, &c, "keys", |it, _, a| {
            let k = it.keys(&arg(a, 0));
            Ok(it.new_array(k.into_iter().map(Value::Str).collect()))
        });
        method(it, &c, "values", |it, _, a| {
            let o = arg(a, 0);
            let mut v = Vec::new();
            for k in it.keys(&o) {
                v.push(it.get(&o, &k)?);
            }
            Ok(it.new_array(v))
        });
        method(it, &c, "entries", |it, _, a| {
            let o = arg(a, 0);
            let mut v = Vec::new();
            for k in it.keys(&o) {
                let x = it.get(&o, &k)?;
                v.push(it.new_array(vec![Value::Str(k), x]));
            }
            Ok(it.new_array(v))
        });
        method(it, &c, "assign", |it, _, a| {
            let t = arg(a, 0);
            for s in a.iter().skip(1) {
                for k in it.keys(s) {
                    let x = it.get(s, &k)?;
                    it.put(&t, &k, x)?;
                }
            }
            Ok(t)
        });
        method(it, &c, "create", |it, _, a| Ok(Value::Obj(obj(arg(a, 0).as_obj().cloned(), Kind::Plain)).clone()).map(|v| {
            let _ = it;
            v
        }));
        method(it, &c, "freeze", |_, _, a| Ok(arg(a, 0)));
    }
    it.set_global("Object", oc);

    // ---- JSON, console
    let j = it.new_object();
    if let Some(jo) = j.as_obj() {
        method(it, jo, "stringify", |it, _, a| {
            let ind = if arg(a, 2).is_undef() { 0 } else { it.to_int(&arg(a, 2)).clamp(0, 10) as usize };
            Ok(json(it, &arg(a, 0), ind).map(|s| Value::Str(Rc::from(s))).unwrap_or_default())
        });
        method(it, jo, "parse", |it, _, a| {
            let s = it.to_string(&arg(a, 0))?;
            json_parse(it, &s)
        });
    }
    it.set_global("JSON", j);
    let con = it.new_object();
    if let Some(co) = con.as_obj() {
        method(it, co, "log", |it, _, a| {
            let parts: Vec<String> = a.iter().map(|x| it.show(x)).collect();
            it.console.push(parts.join(" "));
            Ok(Value::Undef)
        });
    }
    it.set_global("console", con);
}
