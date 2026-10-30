//! Tables (TST). Grid sizes from the header buckets; cell values from the packed cell records
//! in TST.Tile rows (layout as documented by libetonyek's IWAParser::parseTileDefinition),
//! formatted with the cell's TSK.FormatStructArchive; looks from the table style, the region
//! (header row/column, footer, body) styles and per-cell overrides.

use std::collections::HashMap;

use kn_format::{KeyFile, Msg, ObjId};

use crate::style::{self, chain, lookup};
use crate::types::*;

fn header_sizes(k: &KeyFile, storage: Option<&Msg>, bucket_ref: Option<ObjId>, count: u32, default: f32) -> Vec<f32> {
    let mut sizes = vec![default; count as usize];
    let mut buckets: Vec<ObjId> = storage.map(|s| s.ref_ids("buckets")).unwrap_or_default();
    buckets.extend(bucket_ref);
    for b in buckets {
        let Some(bm) = k.obj(b) else { continue };
        for h in bm.msgs("headers") {
            let i = h.u32("index").unwrap_or(u32::MAX) as usize;
            if let (Some(s), Some(slot)) = (h.f32("size"), sizes.get_mut(i)) {
                if s > 0.0 {
                    *slot = s;
                }
            }
        }
    }
    sizes
}

static ALL_ROWS_FIT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Per app: every row grows to fit its lines, its own height being the least it takes
/// (Keydeck: Keynote's tables keep their rows' sizes from frame resizes, yet grow as lines are
/// typed). Off (Values, Letters): a row with a height of its own keeps it.
pub fn set_all_rows_fit(on: bool) {
    ALL_ROWS_FIT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Which rows fit their content. Keynote / Numbers / Pages write the default height into a
/// row's header as soon as it holds a cell, so only a height of its own (resized by hand, or an
/// Excel row height) keeps a row as it is; rows at the default height grow to fit their text.
pub fn auto_rows(k: &KeyFile, model: &Msg) -> Vec<bool> {
    let rows = model.u32("number_of_rows").unwrap_or(0) as usize;
    let mut auto = vec![true; rows];
    if ALL_ROWS_FIT.load(std::sync::atomic::Ordering::Relaxed) {
        return auto;
    }
    let default = model.f64("default_row_height").unwrap_or(0.0) as f32;
    let buckets = model.msg("base_data_store").and_then(|d| d.msg("rowHeaders")).map(|r| r.ref_ids("buckets")).unwrap_or_default();
    for b in buckets {
        for h in k.obj(b).into_iter().flat_map(|m| m.msgs("headers")) {
            let i = h.u32("index").unwrap_or(u32::MAX) as usize;
            if let (Some(s), Some(slot)) = (h.f32("size"), auto.get_mut(i)) {
                if s > 0.0 && (s - default).abs() > 0.01 {
                    *slot = false;
                }
            }
        }
    }
    auto
}

/// Entries of a TST.TableDataList (and its segments).
fn data_list_entries(k: &KeyFile, id: Option<ObjId>) -> Vec<&Msg> {
    let Some(m) = id.and_then(|i| k.obj(i)) else { return Vec::new() };
    let mut out: Vec<&Msg> = m.msgs("entries").collect();
    for seg in m.ref_ids("segments") {
        if let Some(sm) = k.obj(seg) {
            out.extend(sm.msgs("entries"));
        }
    }
    out
}

fn data_list(k: &KeyFile, id: Option<ObjId>) -> HashMap<u32, String> {
    data_list_entries(k, id)
        .into_iter()
        .filter_map(|e| Some((e.u32("key")?, e.str("string")?.to_string())))
        .collect()
}

/// Style list: key -> referenced style object.
fn ref_list(k: &KeyFile, id: Option<ObjId>) -> HashMap<u32, ObjId> {
    data_list_entries(k, id).into_iter().filter_map(|e| Some((e.u32("key")?, e.ref_id("reference")?))).collect()
}

/// Format list: key -> TSK.FormatStructArchive.
fn format_list(k: &KeyFile, id: Option<ObjId>) -> HashMap<u32, &Msg> {
    data_list_entries(k, id).into_iter().filter_map(|e| Some((e.u32("key")?, e.msg("format")?))).collect()
}

/// Rich text list: key -> (the cell's text, its TSWP storage).
fn rich_text_list(k: &KeyFile, id: Option<ObjId>) -> HashMap<u32, (String, ObjId)> {
    let mut out = HashMap::new();
    for e in data_list_entries(k, id) {
        let (Some(key), Some(payload)) = (e.u32("key"), e.ref_id("rich_text_payload")) else { continue };
        let Some(sid) = k.obj(payload).and_then(|p| p.ref_id("storage")) else { continue };
        if let Some(s) = k.obj(sid) {
            out.insert(key, (s.strs("text").concat(), sid));
        }
    }
    out
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn f64le(b: &[u8], at: usize) -> Option<f64> {
    Some(f64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

// ---------------------------------------------------------------------------------- cell records

/// A decoded cell value (formula cells carry their cached result).
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    Number(f64),
    /// Seconds since 2001-01-01 00:00 (floating, shown as stored).
    Date(f64),
    /// Seconds.
    Duration(f64),
    Bool(bool),
    Text(String),
}

/// One cell record: value plus the ids it refers to in the data store lists.
#[derive(Debug, Clone, PartialEq)]
pub struct CellRecord {
    pub value: CellValue,
    pub cell_style: Option<u32>,
    pub text_style: Option<u32>,
    /// Format id that applies to the value (the slot matching the cell's current format kind).
    pub format: Option<u32>,
    pub has_formula: bool,
    /// Conditional highlighting rules (key into the table's conditional style list).
    pub conditional: Option<u32>,
    /// Decoded from the pre-BNC (storage_version 4) layout; `format` then refers to
    /// `format_table_pre_bnc`.
    pub legacy: bool,
    /// A rich text cell (mixed fonts, sizes, styles in one cell): its TSWP storage.
    pub rich: Option<ObjId>,
}

/// Decimal128 as stored by TSCE: 112-bit mantissa in 7 u16, then a u16 holding the mantissa's
/// top bit, the biased exponent (x2, bias 6176) and the sign.
fn decimal128(rec: &[u8], at: usize) -> Option<f64> {
    let mut mantissa = 0f64;
    let mut scale = 1f64;
    for i in 0..7 {
        mantissa += scale * f64::from(u16le(rec, at + 2 * i)?);
        scale *= 65536.0;
    }
    let mut exp = u16le(rec, at + 14)?;
    if exp & 1 != 0 {
        mantissa += scale;
        exp &= 0xfffe;
    }
    if exp & 0x8000 != 0 {
        mantissa = -mantissa;
        exp &= 0x7fff;
    }
    let e = (i32::from(exp) - 12352) / 2;
    // divide for negative exponents so 375e-3 comes out as exactly 0.375
    Some(if e < 0 { mantissa / 10f64.powi(-e) } else { mantissa * 10f64.powi(e) })
}

/// Decode one new-format (storage_version 5) cell record.
///
/// Header: byte 0 version, byte 1 cell type (2 number, 3 text, 5 date, 6 bool, 7 duration,
/// 8 error, 9 rich text, 10 currency), u32 flags at 8; then the optional fields in flag order.
pub fn cell_record(rec: &[u8], strings: &HashMap<u32, String>, rich: &HashMap<u32, (String, ObjId)>) -> Option<CellRecord> {
    let ctype = *rec.get(1)?;
    let flags = u32le(rec, 8)?;
    let mut at = 12;
    let mut value = CellValue::Empty;
    let mut out =
        CellRecord { value: CellValue::Empty, cell_style: None, text_style: None, format: None, has_formula: false, conditional: None, legacy: false, rich: None };
    let next_u32 = |at: &mut usize| -> Option<u32> {
        let v = u32le(rec, *at);
        *at += 4;
        v
    };
    if flags & 1 != 0 {
        value = CellValue::Number(decimal128(rec, at)?);
        at += 16;
    }
    if flags & 2 != 0 {
        let v = f64le(rec, at)?;
        value = match ctype {
            6 => CellValue::Bool(v != 0.0),
            7 => CellValue::Duration(v),
            _ => CellValue::Number(v),
        };
        at += 8;
    }
    if flags & 4 != 0 {
        value = CellValue::Date(f64le(rec, at)?);
        at += 8;
    }
    if flags & 8 != 0 {
        if let Some(s) = strings.get(&next_u32(&mut at)?) {
            value = CellValue::Text(s.clone());
        }
    }
    if flags & 0x10 != 0 {
        if let Some((s, sid)) = rich.get(&next_u32(&mut at)?) {
            value = CellValue::Text(s.clone());
            out.rich = Some(*sid);
        }
    }
    if flags & 0x20 != 0 {
        out.cell_style = next_u32(&mut at);
    }
    if flags & 0x40 != 0 {
        out.text_style = next_u32(&mut at);
    }
    if flags & 0x80 != 0 {
        out.conditional = next_u32(&mut at);
    }
    // applied rule, formula, control spec, syntax error
    for bit in [0x100, 0x200, 0x400, 0x800] {
        if flags & bit != 0 {
            at += 4;
        }
    }
    out.has_formula = flags & 0x200 != 0;
    // current format kind: 1 number, 2 currency, 3 date, 4 duration, 5 text, 6 bool
    let mut kind = 0;
    if flags & 0x1000 != 0 {
        kind = next_u32(&mut at).unwrap_or(0);
    }
    if kind == 0 {
        kind = match ctype {
            10 => 2,
            5 => 3,
            7 => 4,
            3 | 9 => 5,
            6 => 6,
            _ => 1,
        };
    }
    for slot in 0..6u32 {
        if flags & (0x2000 << slot) != 0 {
            let id = next_u32(&mut at);
            if slot + 1 == kind {
                out.format = id;
            }
        }
    }
    out.value = value;
    Some(out)
}

/// Decode one pre-BNC (storage_version 4) cell record: byte 1 cell type, u16 flags at 4,
/// fields from byte 12 in this order: cell style, text style, conditional styles, format,
/// formula, comment, string, number, date, rich text.
pub fn legacy_cell_record(rec: &[u8], strings: &HashMap<u32, String>, rich: &HashMap<u32, (String, ObjId)>) -> Option<CellRecord> {
    let ctype = *rec.get(1)?;
    let flags = u32::from(u16le(rec, 4)?);
    let mut at = 12;
    let mut out =
        CellRecord { value: CellValue::Empty, cell_style: None, text_style: None, format: None, has_formula: false, conditional: None, legacy: true, rich: None };
    let mut next = |bit: u32| -> Option<u32> {
        if flags & bit == 0 {
            return None;
        }
        let v = u32le(rec, at);
        at += 4;
        v
    };
    out.cell_style = next(0x2);
    out.text_style = next(0x80);
    next(0x800);
    next(0x400);
    out.format = next(0x4);
    out.has_formula = next(0x8).is_some();
    next(0x1000);
    let string = next(0x10);
    let mut value = CellValue::Empty;
    if flags & 0x20 != 0 {
        let v = f64le(rec, at)?;
        at += 8;
        value = match ctype {
            6 => CellValue::Bool(v != 0.0),
            7 => CellValue::Duration(v),
            _ => CellValue::Number(v),
        };
    }
    if flags & 0x40 != 0 {
        value = CellValue::Date(f64le(rec, at)?);
        at += 8;
    }
    if let Some(s) = string.and_then(|id| strings.get(&id)) {
        value = CellValue::Text(s.clone());
    }
    if flags & 0x200 != 0 {
        if let Some((s, sid)) = u32le(rec, at).and_then(|id| rich.get(&id)) {
            value = CellValue::Text(s.clone());
            out.rich = Some(*sid);
        }
    }
    out.value = value;
    Some(out)
}

// ---------------------------------------------------------------------------------- formatting

/// Plain ("automatic") number: integers as is, otherwise up to 10 decimals, trimmed.
pub fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// |v| with fixed or automatic (253) decimals and optional thousands separators.
fn fmt_abs(v: f64, places: Option<u32>, thousands: bool) -> String {
    let v = v.abs();
    let s = match places {
        Some(p) if p < 253 => format!("{v:.*}", p as usize),
        _ => fmt_num(v),
    };
    if !thousands {
        return s;
    }
    let (int, frac) = s.split_once('.').map(|(a, b)| (a, Some(b))).unwrap_or((&s, None));
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    match frac {
        Some(f) => format!("{grouped}.{f}"),
        None => grouped,
    }
}

/// Apply the negative style (0 "-1", 1 red "1", 2 "(1)", 3 red "(1)"; colour is not modelled).
fn signed(neg: bool, body: String, style: u32) -> String {
    match (neg, style) {
        (false, _) | (true, 1) => body,
        (true, 2 | 3) => format!("({body})"),
        _ => format!("-{body}"),
    }
}

fn currency_symbol(code: &str) -> String {
    match code {
        "" | "USD" | "CAD" | "AUD" | "NZD" | "MXN" | "SGD" | "HKD" => "$".into(),
        "EUR" => "€".into(),
        "GBP" => "£".into(),
        "JPY" | "CNY" => "¥".into(),
        "INR" => "₹".into(),
        "KRW" => "₩".into(),
        other => format!("{other} "),
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Closest fraction: positive accuracy = fixed denominator, -n (as u32) = up to n digits.
fn fmt_fraction(v: f64, accuracy: u32) -> String {
    let a = v.abs();
    let mut whole = a.trunc();
    let frac = a - whole;
    let acc = accuracy as i32;
    let (mut num, den) = if acc > 0 {
        ((frac * f64::from(acc)).round(), f64::from(acc))
    } else {
        let max_den = 10f64.powi((-acc).clamp(1, 3)) - 1.0;
        let mut best = (0.0, 1.0, f64::MAX);
        let mut d = 1.0;
        while d <= max_den {
            let n = (frac * d).round();
            let err = (frac - n / d).abs();
            if err < best.2 - 1e-12 {
                best = (n, d, err);
            }
            d += 1.0;
        }
        (best.0, best.1)
    };
    if num >= den {
        whole += 1.0;
        num = 0.0;
    }
    // a fixed denominator (Halves, Quarters...) is kept: 2/4 stays 2/4 (Keynote, R34)
    let g = if acc > 0 { 1.0 } else { gcd(num as u64, den as u64).max(1) as f64 };
    let (n, d) = (num / g, den / g);
    let body = match (whole as i64, num as i64) {
        (0, 0) => "0".to_string(),
        (w, 0) => w.to_string(),
        (0, _) => format!("{n}/{d}"),
        (w, _) => format!("{w} {n}/{d}"),
    };
    if v < 0.0 && body != "0" { format!("-{body}") } else { body }
}

fn fmt_scientific(v: f64, places: Option<u32>) -> String {
    let s = match places {
        Some(p) if p < 253 => format!("{v:.*e}", p as usize),
        _ => {
            let s = format!("{v:.9e}");
            let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
            let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
            format!("{m}e{e}")
        }
    };
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

fn fmt_base(v: f64, f: &Msg) -> String {
    let base = f.u32("base").unwrap_or(10).clamp(2, 36);
    let places = f.u32("base_places").unwrap_or(0) as usize;
    // rounded to a whole number first (Keynote shows 1234.5678 as 04D3, R34)
    let mut n = v.round().abs() as u64;
    let mut digits = Vec::new();
    while n > 0 {
        digits.push(std::char::from_digit((n % u64::from(base)) as u32, base).unwrap_or('?').to_ascii_uppercase());
        n /= u64::from(base);
    }
    while digits.len() < places.max(1) {
        digits.push('0');
    }
    let s: String = digits.iter().rev().collect();
    if v < 0.0 { format!("-{s}") } else { s }
}

/// (year, month 1-12, day, weekday 0 = Sunday) for days since 1970-01-01.
fn civil(days: i64) -> (i64, i64, i64, i64) {
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, (days + 4).rem_euclid(7))
}

const MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Format a date with a Unicode (ICU) pattern such as "M/d/yy h:mm a" or "EEEE, MMMM d, y".
pub fn format_date(secs_since_2001: f64, pattern: &str) -> String {
    let total = secs_since_2001.round() as i64;
    let days = total.div_euclid(86400) + 11323; // days since 1970-01-01
    let tod = total.rem_euclid(86400);
    let (y, mo, d, wd) = civil(days);
    let (hh, mi, ss) = (tod / 3600, tod / 60 % 60, tod % 60);
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        if !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        i += n;
        let pad = |v: i64| if n >= 2 { format!("{v:02}") } else { v.to_string() };
        match c {
            'y' | 'Y' | 'u' => out.push_str(&if n == 2 { format!("{:02}", y.rem_euclid(100)) } else { y.to_string() }),
            'M' | 'L' => match n {
                1 | 2 => out.push_str(&pad(mo)),
                3 => out.push_str(&MONTHS[(mo - 1) as usize][..3]),
                _ => out.push_str(MONTHS[(mo - 1) as usize]),
            },
            'd' => out.push_str(&pad(d)),
            'E' | 'c' | 'e' => {
                let name = WEEKDAYS[wd as usize];
                out.push_str(if n >= 4 { name } else { &name[..3] });
            }
            'h' => out.push_str(&pad(if hh % 12 == 0 { 12 } else { hh % 12 })),
            'K' => out.push_str(&pad(hh % 12)),
            'H' => out.push_str(&pad(hh)),
            'k' => out.push_str(&pad(if hh == 0 { 24 } else { hh })),
            'm' => out.push_str(&pad(mi)),
            's' => out.push_str(&pad(ss)),
            'a' => out.push_str(if hh < 12 { "AM" } else { "PM" }),
            _ => {}
        }
    }
    out
}

/// Duration units (bitmask value, seconds, short, singular, plural).
const DURATION_UNITS: [(u32, f64, &str, &str, &str); 6] = [
    (1, 604_800.0, "w", "week", "weeks"),
    (2, 86_400.0, "d", "day", "days"),
    (4, 3600.0, "h", "hour", "hours"),
    (8, 60.0, "m", "minute", "minutes"),
    (16, 1.0, "s", "second", "seconds"),
    (32, 0.001, "ms", "millisecond", "milliseconds"),
];

/// Durations: style 0 "1:30:00", 1 "1h 30m", 2 "1 hour 30 minutes".
fn fmt_duration(secs: f64, f: &Msg) -> String {
    let style = f.u32("duration_style").unwrap_or(1);
    let mut largest = f.u32("duration_unit_largest").unwrap_or(4);
    let mut smallest = f.u32("duration_unit_smallest").unwrap_or(16);
    let mut rest = secs.abs();
    if f.bool("use_automatic_duration_units").unwrap_or(true) {
        // largest non-zero unit down to the last unit that still has a remainder
        largest = DURATION_UNITS.iter().find(|u| rest >= u.1).map(|u| u.0).unwrap_or(16);
        smallest = largest;
        let mut r = rest;
        // (down to milliseconds when the seconds have a fraction: Keynote, R33)
        for u in DURATION_UNITS.iter().filter(|u| u.0 >= largest) {
            let q = if u.0 == 32 { (r / u.1).round() } else { (r / u.1 + 1e-6).floor() };
            r -= q * u.1;
            if q > 0.0 {
                smallest = u.0;
            }
        }
    }
    let units: Vec<_> = DURATION_UNITS.iter().filter(|u| u.0 >= largest && u.0 <= smallest).collect();
    let mut parts = Vec::new();
    for (i, u) in units.iter().enumerate() {
        let q = if i + 1 == units.len() { (rest / u.1).round() } else { (rest / u.1 + 1e-9).floor() };
        rest -= q * u.1;
        parts.push((q as i64, *u));
    }
    let body = match style {
        0 => parts
            .iter()
            .enumerate()
            .map(|(i, (q, _))| if i == 0 { q.to_string() } else { format!("{q:02}") })
            .collect::<Vec<_>>()
            .join(":"),
        1 => parts.iter().map(|(q, u)| format!("{q}{}", u.2)).collect::<Vec<_>>().join(" "),
        _ => parts.iter().map(|(q, u)| format!("{q} {}", if *q == 1 { u.3 } else { u.4 })).collect::<Vec<_>>().join(" "),
    };
    if secs < 0.0 { format!("-{body}") } else { body }
}

/// Numbers with a TSK.FormatStructArchive (format_type 256 decimal, 257 currency, 258 percent,
/// 259 scientific, 260 text, 262 fraction, 263 checkbox, 264-266 stepper/slider/menu,
/// 267 rating, 268 duration, 269 base).
fn fmt_number(v: f64, f: &Msg) -> String {
    let places = f.u32("decimal_places");
    let thousands = f.bool("show_thousands_separator").unwrap_or(false);
    let neg_style = f.u32("negative_style").unwrap_or(0);
    let neg = v < 0.0;
    match f.u32("format_type").unwrap_or(0) {
        256 => signed(neg, fmt_abs(v, places, thousands), neg_style),
        257 => {
            let sym = currency_symbol(f.str("currency_code").unwrap_or("USD"));
            if f.bool("use_accounting_style").unwrap_or(false) {
                // accounting: the symbol at the cell's left edge, the number at its right,
                // negatives in brackets (Keynote stores "€" U+200E TAB "1234.57", R34)
                let n = fmt_abs(v, places, thousands);
                format!("{sym}\u{200E}\t{}", if neg { format!("({n})") } else { n })
            } else {
                signed(neg, format!("{sym}{}", fmt_abs(v, places, thousands)), neg_style)
            }
        }
        258 => signed(neg, format!("{}%", fmt_abs(v * 100.0, places, thousands)), neg_style),
        259 => fmt_scientific(v, places),
        262 => fmt_fraction(v, f.u32("fraction_accuracy").unwrap_or(u32::MAX - 2)),
        263 => if v != 0.0 { "☑".into() } else { "☐".into() },
        267 => "★".repeat(v.clamp(0.0, 5.0).round() as usize),
        268 => fmt_duration(v, f),
        269 => fmt_base(v, f),
        _ => fmt_num(v),
    }
}

/// The document's custom formats (Format > Create Custom Format): uid -> the format each
/// stands for (its default format), from TSK.CustomFormatListArchive (uuids[i] ->
/// custom_formats[i]). Built once per table read.
fn custom_formats(k: &KeyFile) -> HashMap<(Option<u64>, Option<u64>), Msg> {
    let mut out = HashMap::new();
    for (_, list) in k.objects_of("TSK.CustomFormatListArchive") {
        for (u, cf) in list.msgs("uuids").zip(list.msgs("custom_formats")) {
            if let Some(f) = cf.msg("default_format") {
                out.entry((u.u64("lower"), u.u64("upper"))).or_insert_with(|| f.clone());
            }
        }
    }
    out
}

/// The format a custom format (format_type 270 number, 271 date, 272 text... with a
/// custom_uid) stands for, from [`custom_formats`].
fn custom_format<'a>(customs: &'a HashMap<(Option<u64>, Option<u64>), Msg>, f: &Msg) -> Option<&'a Msg> {
    if !matches!(f.u32("format_type"), Some(270..=272)) {
        return None;
    }
    let uid = f.msg("custom_uid")?;
    customs.get(&(uid.u64("lower"), uid.u64("upper")))
}

/// The date pattern a cell format shows dates with ("M/d/yyyy", or a custom date format's
/// own, looked up by its custom_uid): None for formats that aren't dates.
pub fn date_pattern(k: &KeyFile, f: &Msg) -> Option<String> {
    date_pattern_with(&custom_formats(k), f)
}

/// The document's custom formats by uid, built once (for [`date_pattern_with`] over many cells).
pub type CustomFormats = HashMap<(Option<u64>, Option<u64>), Msg>;

pub fn custom_format_map(k: &KeyFile) -> CustomFormats {
    custom_formats(k)
}

/// [`date_pattern`] with the custom formats already looked up.
pub fn date_pattern_with(customs: &CustomFormats, f: &Msg) -> Option<String> {
    let f = custom_format(customs, f).unwrap_or(f);
    f.str("date_time_format").or_else(|| f.str("custom_format_string")).map(str::to_string)
}

/// Display text of a cell value with its format (None = automatic).
pub fn format_value(v: &CellValue, f: Option<&Msg>) -> String {
    match v {
        CellValue::Empty => String::new(),
        CellValue::Text(s) => s.clone(),
        CellValue::Bool(b) => match f.and_then(|f| f.u32("format_type")) {
            Some(263) => if *b { "☑".into() } else { "☐".into() },
            _ => if *b { "TRUE".into() } else { "FALSE".into() },
        },
        // a custom date format keeps its pattern in custom_format_string
        CellValue::Date(s) => format_date(*s, f.and_then(|f| f.str("date_time_format").or_else(|| f.str("custom_format_string"))).unwrap_or("M/d/yyyy")),
        CellValue::Duration(s) => match f {
            Some(f) if f.u32("format_type") == Some(268) => fmt_duration(*s, f),
            _ => fmt_duration(*s, &Msg::untyped()),
        },
        CellValue::Number(n) => f.map(|f| fmt_number(*n, f)).unwrap_or_else(|| fmt_num(*n)),
    }
}

// ---------------------------------------------------------------------------------- styles

#[derive(Clone, Copy, PartialEq)]
enum Region {
    HeaderRow,
    HeaderCol,
    Footer,
    Body,
}

/// Region of a cell; header rows win over footers, which win over header columns.
fn region(t: &TableData, r: u32, c: u32) -> Region {
    if r < t.header_rows {
        Region::HeaderRow
    } else if r >= t.rows.saturating_sub(t.footer_rows) {
        Region::Footer
    } else if c < t.header_cols {
        Region::HeaderCol
    } else {
        Region::Body
    }
}

/// Look of a cell from its region's base styles plus optional per-cell style overrides
/// (a per-cell fill beats row banding, which beats the region fill).
fn cell_style(
    k: &KeyFile,
    base: (Option<ObjId>, Option<ObjId>),
    own: (Option<ObjId>, Option<ObjId>),
    band: Option<&Fill>,
    numeric: bool,
) -> TableCellStyle {
    let base_chain = chain(k, base.0);
    let own_chain: Vec<&Msg> =
        chain(k, own.0).into_iter().take_while(|m| !base_chain.iter().any(|b| std::ptr::eq(*b, *m))).collect();
    let own_props: Vec<&Msg> = own_chain.iter().filter_map(|m| m.msg("cell_properties")).collect();
    let props: Vec<&Msg> = own_chain.iter().chain(base_chain.iter()).filter_map(|m| m.msg("cell_properties")).collect();
    let fill = lookup(&own_props, |p| p.msg("cell_fill").map(style::fill))
        .or_else(|| band.cloned())
        .or_else(|| lookup(&props, |p| p.msg("cell_fill").map(style::fill)))
        .unwrap_or(Fill::None);
    let valign = match lookup(&props, |p| p.i64("vertical_alignment")) {
        Some(0) => VAlign::Top,
        Some(2) => VAlign::Bottom,
        _ => VAlign::Middle,
    };
    let padding = lookup(&props, |p| {
        p.msg("padding").map(|pd| {
            [pd.f32("left").unwrap_or(4.0), pd.f32("top").unwrap_or(4.0), pd.f32("right").unwrap_or(4.0), pd.f32("bottom").unwrap_or(4.0)]
        })
    })
    .unwrap_or([4.0; 4]);
    let para_chain: Vec<&Msg> = chain(k, own.1).into_iter().chain(chain(k, base.1)).collect();
    let para: Vec<&Msg> = para_chain.iter().filter_map(|m| m.msg("para_properties")).collect();
    let align = match lookup(&para, |p| p.enum_name("alignment")) {
        Some("TATvalue0") => Align::Left,
        Some("TATvalue1") => Align::Right,
        Some("TATvalue2") => Align::Center,
        Some("TATvalue3") => Align::Justify,
        // natural: numbers right, text left
        _ if numeric => Align::Right,
        _ => Align::Left,
    };
    TableCellStyle {
        fill,
        text: style::char_style(k, base.1, own.1),
        align,
        valign,
        padding,
        wrap: lookup(&props, |p| p.bool("text_wrap")).unwrap_or(true),
        para: {
            let pp = style::para_props_of(&para);
            crate::CellPara {
                line_spacing: pp.line_spacing,
                space_before: pp.space_before,
                space_after: pp.space_after,
                left_indent: pp.left_indent,
                right_indent: pp.right_indent,
                first_line_indent: pp.first_line_indent,
            }
        },
    }
}

/// Evaluate a conditional-highlighting predicate (postfix formula over the cell's own value:
/// comparisons with numbers, text and booleans; anything else counts as not holding).
pub(crate) fn predicate_holds(f: &Msg, cell: &CellValue) -> bool {
    #[derive(Clone, PartialEq)]
    enum V {
        N(f64),
        S(String),
        B(bool),
    }
    let mut stack: Vec<V> = Vec::new();
    let Some(nodes) = f.msg("AST_node_array") else { return false };
    for n in nodes.msgs("AST_node") {
        let ty = n.enum_name("AST_node_type").unwrap_or("");
        match ty {
            "LINKED_CELL_REF_NODE" | "LOCAL_CELL_REFERENCE_NODE" => stack.push(match cell {
                CellValue::Number(v) | CellValue::Duration(v) | CellValue::Date(v) => V::N(*v),
                CellValue::Bool(b) => V::B(*b),
                CellValue::Text(s) => V::S(s.clone()),
                _ => V::S(String::new()),
            }),
            "NUMBER_NODE" => stack.push(V::N(n.f64("AST_number_node_number").unwrap_or(0.0))),
            "BOOLEAN_NODE" => stack.push(V::B(n.bool("AST_token_node_boolean").unwrap_or(false))),
            "STRING_NODE" => stack.push(V::S(n.str("AST_string_node_string").unwrap_or("").to_string())),
            "GREATER_THAN_NODE" | "GREATER_THAN_OR_EQUAL_TO_NODE" | "LESS_THAN_NODE" | "LESS_THAN_OR_EQUAL_TO_NODE" | "EQUAL_TO_NODE" | "NOT_EQUAL_TO_NODE" => {
                let (Some(b), Some(a)) = (stack.pop(), stack.pop()) else { return false };
                let ord = match (&a, &b) {
                    (V::N(x), V::N(y)) => x.partial_cmp(y),
                    (V::S(x), V::S(y)) => Some(x.to_lowercase().cmp(&y.to_lowercase())),
                    (V::B(x), V::B(y)) => Some(x.cmp(y)),
                    _ => None,
                };
                use std::cmp::Ordering::*;
                let r = match (ty, ord) {
                    ("NOT_EQUAL_TO_NODE", o) => o != Some(Equal),
                    (_, None) => false,
                    ("GREATER_THAN_NODE", Some(o)) => o == Greater,
                    ("GREATER_THAN_OR_EQUAL_TO_NODE", Some(o)) => o != Less,
                    ("LESS_THAN_NODE", Some(o)) => o == Less,
                    ("LESS_THAN_OR_EQUAL_TO_NODE", Some(o)) => o != Greater,
                    (_, Some(o)) => o == Equal,
                };
                stack.push(V::B(r));
            }
            _ => return false,
        }
    }
    stack.pop() == Some(V::B(true))
}

/// Merged ranges: TST.MergeRegionMapArchive (older files) and the merge owner's formula store
/// (one absolute colon-tract range per merge, ends inclusive).
fn merges(k: &KeyFile, model: &Msg) -> Vec<CellRange> {
    let mut out = Vec::new();
    let ds = model.msg("base_data_store");
    if let Some(mm) = ds.and_then(|d| d.ref_id("merge_region_map")).and_then(|id| k.obj(id)) {
        for r in mm.msgs("cell_range") {
            let (Some(o), Some(s)) = (r.msg("origin"), r.msg("size")) else { continue };
            let po = o.u32("packedData").unwrap_or(0);
            let ps = s.u32("packedData").unwrap_or(0);
            out.push(CellRange {
                row: po >> 16,
                col: po & 0xffff,
                rows: s.u32("num_rows").unwrap_or(ps >> 16),
                cols: s.u32("num_columns").unwrap_or(ps & 0xffff),
            });
        }
    }
    let store = model.msg("merge_owner").and_then(|m| m.msg("formula_store"));
    for f in store.map(|s| s.msgs("formulas").collect::<Vec<_>>()).unwrap_or_default() {
        // (a colon tract, or two cell references and a colon in older files)
        if let Some(r) = crate::tablemerge::formula_range(f) {
            out.push(r);
        }
    }
    out.retain(|m| m.rows * m.cols > 1);
    // (a file may list a merge both ways)
    let mut seen = Vec::new();
    out.retain(|m| {
        let k = (m.row, m.col, m.rows, m.cols);
        !seen.contains(&k) && {
            seen.push(k);
            true
        }
    });
    out
}

/// Stroke for an edge whose neighbouring cell lies in `reg`, by region-specific style names
/// (header row, footer, header column, body).
fn region_stroke(st: &dyn Fn(&str) -> Option<Stroke>, reg: Region, names: [&str; 4]) -> Option<Stroke> {
    st(match reg {
        Region::HeaderRow => names[0],
        Region::Footer => names[1],
        Region::HeaderCol => names[2],
        Region::Body => names[3],
    })
}

/// Grid strokes from the table style's region strokes and visibility switches, then the
/// stroke sidecar's explicit per-edge runs (applied in `order`).
fn strokes(k: &KeyFile, model: &Msg, t: &TableData) -> (Vec<Option<Stroke>>, Vec<Option<Stroke>>) {
    let (rows, cols, hr, hc) = (t.rows, t.cols, t.header_rows, t.header_cols);
    let footer_start = rows.saturating_sub(t.footer_rows);
    let ch = chain(k, model.ref_id("table_style"));
    let props: Vec<&Msg> = ch.iter().filter_map(|m| m.msg("table_properties")).collect();
    let flag = |name: &str| lookup(&props, |p| p.bool(name)).unwrap_or(false);
    let st = |name: &str| lookup(&props, |p| p.msg(name).map(style::stroke)).flatten();
    let border = flag("table_border_visible");
    let mut h = vec![None; ((rows + 1) * cols) as usize];
    let mut v = vec![None; (rows * (cols + 1)) as usize];
    for b in 0..=rows {
        for c in 0..cols {
            let reg = region(t, b.min(rows.saturating_sub(1)), c);
            h[(b * cols + c) as usize] = if b == 0 || b == rows {
                let names = ["header_row_border_stroke", "footer_row_border_stroke", "header_column_border_stroke", "table_body_horizontal_border_stroke"];
                if border { region_stroke(&st, reg, names) } else { None }
            } else if b == hr {
                if flag("hr_separator_visible") { st("header_row_separator_stroke") } else { None }
            } else if b == footer_start {
                if flag("footer_separator_visible") { st("footer_row_separator_stroke") } else { None }
            } else if flag("h_strokes_visible") {
                let names = ["header_row_horizontal_stroke", "footer_row_horizontal_stroke", "header_column_horizontal_stroke", "table_body_horizontal_stroke"];
                region_stroke(&st, reg, names)
            } else {
                None
            };
        }
    }
    for r in 0..rows {
        for b in 0..=cols {
            let reg = region(t, r, b.min(cols.saturating_sub(1)));
            v[(r * (cols + 1) + b) as usize] = if b == 0 || b == cols {
                let names = ["header_row_border_stroke", "footer_row_border_stroke", "header_column_border_stroke", "table_body_vertical_border_stroke"];
                if border { region_stroke(&st, reg, names) } else { None }
            } else if b == hc && r >= hr && r < footer_start {
                if flag("hc_separator_visible") { st("header_column_separator_stroke") } else { None }
            } else if flag("v_strokes_visible") {
                let names = ["header_row_vertical_stroke", "footer_row_vertical_stroke", "header_column_vertical_stroke", "table_body_vertical_stroke"];
                region_stroke(&st, reg, names)
            } else {
                None
            };
        }
    }
    // explicit strokes (Format > Cell > Border)
    let Some(sc) = model.ref_id("stroke_sidecar").and_then(|id| k.obj(id)) else { return (h, v) };
    let mut runs: Vec<(u32, bool, u32, u32, u32, Option<Stroke>)> = Vec::new();
    for (field, horizontal, offset) in [
        ("top_row_stroke_layers", true, 0),
        ("bottom_row_stroke_layers", true, 1),
        ("left_column_stroke_layers", false, 0),
        ("right_column_stroke_layers", false, 1),
    ] {
        for layer in sc.ref_ids(field).into_iter().filter_map(|id| k.obj(id)) {
            let idx = layer.u32("row_column_index").unwrap_or(0) + offset;
            for run in layer.msgs("stroke_runs") {
                let origin = run.i64("origin").unwrap_or(0).max(0) as u32;
                let stroke = run.msg("stroke").and_then(style::stroke);
                runs.push((run.u32("order").unwrap_or(0), horizontal, idx, origin, run.u32("length").unwrap_or(0), stroke));
            }
        }
    }
    runs.sort_by_key(|r| r.0);
    for (_, horizontal, idx, origin, len, s) in runs {
        for i in origin..origin.saturating_add(len) {
            let slot = if horizontal {
                if idx > rows || i >= cols { None } else { h.get_mut((idx * cols + i) as usize) }
            } else if idx > cols || i >= rows {
                None
            } else {
                v.get_mut((i * (cols + 1) + idx) as usize)
            };
            if let Some(slot) = slot {
                *slot = s.clone();
            }
        }
    }
    (h, v)
}

// ---------------------------------------------------------------------------------- table

/// Every decodable cell record of a TST.TableModelArchive: ((row, col), record). Rows saved
/// only in the pre-BNC layout (older files) are decoded with [`legacy_cell_record`].
pub fn cell_records(k: &KeyFile, model: &Msg) -> Vec<((u32, u32), CellRecord)> {
    let mut out = Vec::new();
    let Some(ds) = model.msg("base_data_store") else { return out };
    let rows = model.u32("number_of_rows").unwrap_or(0);
    let cols = model.u32("number_of_columns").unwrap_or(0);
    let strings = data_list(k, ds.ref_id("stringTable"));
    let rich = rich_text_list(k, ds.ref_id("rich_text_table"));
    let tiles = ds.msg("tiles");
    let tile_size = tiles.and_then(|t| t.u32("tile_size")).unwrap_or(256);
    for t in tiles.map(|t| t.msgs("tiles").collect::<Vec<_>>()).unwrap_or_default() {
        let tile_id = t.u32("tileid").unwrap_or(0);
        let Some(tile) = t.ref_id("tile").and_then(|id| k.obj(id)) else { continue };
        for ri in tile.msgs("rowInfos") {
            let row = tile_id * tile_size + ri.u32("tile_row_index").unwrap_or(0);
            let new = (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets"));
            let legacy = new.0.is_none();
            let pair = if legacy { (ri.bytes("cell_storage_buffer_pre_bnc"), ri.bytes("cell_offsets_pre_bnc")) } else { new };
            let (Some(buf), Some(offs)) = pair else { continue };
            let factor = if !legacy && ri.bool("has_wide_offsets").unwrap_or(false) { 4 } else { 1 };
            let offsets: Vec<Option<usize>> = offs
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .map(|o| (o != 0xffff).then_some(o as usize * factor))
                .collect();
            for (col, off) in offsets.iter().enumerate() {
                let Some(start) = *off else { continue };
                if row >= rows || col as u32 >= cols {
                    continue;
                }
                let end = offsets[col + 1..].iter().flatten().next().copied().unwrap_or(buf.len());
                let decode = if legacy { legacy_cell_record } else { cell_record };
                if let Some(rec) = buf.get(start..end.max(start)).and_then(|r| decode(r, &strings, &rich)) {
                    out.push(((row, col as u32), rec));
                }
            }
        }
    }
    out
}

/// The raw (storage_version 5) cell records of a table model by (row, col).
pub fn raw_cell_records(k: &KeyFile, model: &Msg) -> Vec<((u32, u32), Vec<u8>)> {
    let mut out = Vec::new();
    let Some(tiles) = model.msg("base_data_store").and_then(|d| d.msg("tiles")) else { return out };
    let tile_size = tiles.u32("tile_size").unwrap_or(256);
    for t in tiles.msgs("tiles") {
        let tile_id = t.u32("tileid").unwrap_or(0);
        let Some(tile) = t.ref_id("tile").and_then(|id| k.obj(id)) else { continue };
        for ri in tile.msgs("rowInfos") {
            let row = tile_id * tile_size + ri.u32("tile_row_index").unwrap_or(0);
            let (Some(buf), Some(offs)) = (ri.bytes("cell_storage_buffer"), ri.bytes("cell_offsets")) else { continue };
            let factor = if ri.bool("has_wide_offsets").unwrap_or(false) { 4 } else { 1 };
            let offsets: Vec<Option<usize>> =
                offs.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).map(|o| (o != 0xffff).then_some(o as usize * factor)).collect();
            for (col, off) in offsets.iter().enumerate() {
                let Some(start) = *off else { continue };
                let end = offsets[col + 1..].iter().flatten().next().copied().unwrap_or(buf.len());
                if let Some(r) = buf.get(start..end.max(start)) {
                    out.push(((row, col as u32), r.to_vec()));
                }
            }
        }
    }
    out
}

/// A table's size, header and footer counts and merged cells only (the rest left empty): what
/// cell edits need to know, without laying out the whole table as [`table`] does (~10 ms on a
/// 65×21 table).
pub fn table_shape(k: &KeyFile, info: &Msg) -> Option<TableData> {
    let model = k.obj(info.ref_id("tableModel")?)?;
    let rows = model.u32("number_of_rows").unwrap_or(0);
    let cols = model.u32("number_of_columns").unwrap_or(0);
    Some(TableData {
        rows,
        cols,
        header_rows: model.u32("number_of_header_rows").unwrap_or(0).min(rows),
        header_cols: model.u32("number_of_header_columns").unwrap_or(0).min(cols),
        footer_rows: model.u32("number_of_footer_rows").unwrap_or(0).min(rows),
        merges: merges(k, model),
        ..Default::default()
    })
}

pub fn table(k: &KeyFile, info: &Msg) -> Option<TableData> {
    let model = k.obj(info.ref_id("tableModel")?)?;
    let customs = custom_formats(k);
    let rows = model.u32("number_of_rows").unwrap_or(0);
    let cols = model.u32("number_of_columns").unwrap_or(0);
    let ds = model.msg("base_data_store");
    let row_heights = header_sizes(
        k,
        ds.and_then(|d| d.msg("rowHeaders")),
        None,
        rows,
        model.f64("default_row_height").unwrap_or(40.0) as f32,
    );
    let col_widths = header_sizes(
        k,
        None,
        ds.and_then(|d| d.ref_id("columnHeaders")),
        cols,
        model.f64("default_column_width").unwrap_or(120.0) as f32,
    );
    let mut t = TableData {
        rows,
        cols,
        header_rows: model.u32("number_of_header_rows").unwrap_or(0).min(rows),
        header_cols: model.u32("number_of_header_columns").unwrap_or(0).min(cols),
        footer_rows: model.u32("number_of_footer_rows").unwrap_or(0).min(rows),
        row_heights,
        col_widths,
        name: model.str("table_name").unwrap_or("").to_string(),
        cells: vec![String::new(); (rows * cols) as usize],
        ..Default::default()
    };
    t.merges = merges(k, model);
    t.auto_rows = auto_rows(k, model);

    let styles = ref_list(k, ds.and_then(|d| d.ref_id("styleTable")));
    let conditionals = ref_list(k, ds.and_then(|d| d.ref_id("conditionalstyletable")));
    let formats = format_list(k, ds.and_then(|d| d.ref_id("format_table")));
    let legacy_formats = format_list(k, ds.and_then(|d| d.ref_id("format_table_pre_bnc")));
    let mut records: HashMap<(u32, u32), CellRecord> = cell_records(k, model).into_iter().collect();
    let tchain = chain(k, model.ref_id("table_style"));
    let tprops: Vec<&Msg> = tchain.iter().filter_map(|m| m.msg("table_properties")).collect();
    let banded = lookup(&tprops, |p| p.bool("banded_rows")).unwrap_or(false);
    let band_fill = lookup(&tprops, |p| p.msg("banded_fill").map(style::fill));

    for r in 0..rows {
        for c in 0..cols {
            let reg = region(&t, r, c);
            let (base_cell, base_text) = match reg {
                Region::HeaderRow => ("header_row_style", "header_row_text_style"),
                Region::Footer => ("footer_row_style", "footer_row_text_style"),
                Region::HeaderCol => ("header_column_style", "header_column_text_style"),
                Region::Body => ("body_cell_style", "body_text_style"),
            };
            let rec = records.remove(&(r, c));
            let numeric =
                matches!(rec.as_ref().map(|x| &x.value), Some(CellValue::Number(_) | CellValue::Date(_) | CellValue::Duration(_)));
            let band = (banded && reg == Region::Body && (r - t.header_rows) % 2 == 1).then_some(band_fill.as_ref()).flatten();
            let own = |id: Option<u32>| id.and_then(|i| styles.get(&i)).copied();
            t.cell_styles.push(cell_style(
                k,
                (model.ref_id(base_cell), model.ref_id(base_text)),
                (own(rec.as_ref().and_then(|x| x.cell_style)), own(rec.as_ref().and_then(|x| x.text_style))),
                band,
                numeric,
            ));
            // Format > Conditional Highlighting: the first rule that holds restyles the cell
            if let Some(set) = rec.as_ref().and_then(|x| x.conditional).and_then(|i| conditionals.get(&i)).and_then(|id| k.obj(*id)) {
                let value = &rec.as_ref().unwrap().value;
                if let Some(rule) = set.msgs("rules_prepivot").find(|r| r.msg("predicate").and_then(|p| p.msg("formula")).is_some_and(|f| predicate_holds(f, value))) {
                    let st = t.cell_styles.last_mut().unwrap();
                    let cell_props = chain(k, rule.ref_id("cell_style")).into_iter().filter_map(|m| m.msg("cell_properties")).collect::<Vec<_>>();
                    if let Some(f) = lookup(&cell_props, |p| p.msg("cell_fill").map(style::fill)) {
                        st.fill = f;
                    }
                    for m in chain(k, rule.ref_id("text_style")).into_iter().rev() {
                        let Some(cp) = m.msg("char_properties") else { continue };
                        if let Some(c) = cp.msg("font_color") {
                            st.text.color = style::color(c);
                        }
                        if let Some(b) = cp.bool("bold") {
                            st.text.bold = b;
                        }
                        if let Some(i) = cp.bool("italic") {
                            st.text.italic = i;
                        }
                        if let Some(s) = cp.enum_name("strikethru") {
                            st.text.strike = s != "kNoStrikethru";
                        }
                        if let Some(u) = cp.enum_name("underline") {
                            st.text.underline = u != "kNoUnderline";
                        }
                    }
                }
            }
            if let Some(rec) = rec {
                let list = if rec.legacy { &legacy_formats } else { &formats };
                let f = rec.format.and_then(|id| list.get(&id)).copied();
                // a custom format (Format > Create Custom Format) shows as its default format
                let f = f.and_then(|f| custom_format(&customs, f)).or(f);
                t.cells[(r * cols + c) as usize] = format_value(&rec.value, f);
                // rich text: the cell's own runs (its text style the default for what they leave)
                if let Some(sid) = rec.rich {
                    let para_style = own(rec.text_style).or(model.ref_id(base_text));
                    let tb = crate::text::text_box(k, sid, &crate::text::TextDefaults { para_style }, style::text_layout(k, None, None), Default::default());
                    if let Some(tb) = tb {
                        if t.rich.is_empty() {
                            t.rich = vec![None; (rows * cols) as usize];
                        }
                        t.rich[(r * cols + c) as usize] = Some(tb);
                    }
                }
                // negative styles 1 and 3 show negative numbers in red (Keynote's red)
                if let (CellValue::Number(x), Some(f)) = (&rec.value, f) {
                    if *x < 0.0 && matches!(f.u32("negative_style"), Some(1 | 3)) && matches!(f.u32("format_type"), Some(256..=258)) {
                        if let Some(st) = t.cell_styles.get_mut((r * cols + c) as usize) {
                            st.text.color = Color { r: 1.0, g: 0.149, b: 0.0, a: 1.0 };
                        }
                    }
                }
            }
        }
    }
    let (h, v) = strokes(k, model, &t);
    t.h_strokes = h;
    t.v_strokes = v;
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Older files keep a merge as "A1:C1": two cell references and a colon (Pages booklets).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn old_style_merges() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let k = KeyFile::open(dir.join("K-avg.key")).unwrap();
        let mut model = Msg::new("TST.TableModelArchive");
        let store = model.msg_mut_or_create("merge_owner").msg_mut_or_create("formula_store");
        for (r0, c0, r1, c1) in [(0, 0, 0, 3), (5, 0, 5, 2)] {
            let arr = store.push_new("formulas").msg_mut_or_create("formula").msg_mut_or_create("AST_node_array");
            for (r, c) in [(r0, c0), (r1, c1)] {
                let n = arr.push_new("AST_node");
                n.set_enum("AST_node_type", "CELL_REFERENCE_NODE");
                n.msg_mut_or_create("AST_row").set_i64("row", r);
                n.msg_mut_or_create("AST_column").set_i64("column", c);
            }
            arr.push_new("AST_node").set_enum("AST_node_type", "COLON_NODE");
            arr.push_new("AST_node").set_enum("AST_node_type", "FUNCTION_NODE");
        }
        // through the wire, so the sint32 zig-zag is exercised
        let model = Msg::parse(&model.encode(), kn_format::schema::schema().type_idx("TST.TableModelArchive")).unwrap();
        assert_eq!(merges(&k, &model), [CellRange { row: 0, col: 0, rows: 1, cols: 4 }, CellRange { row: 5, col: 0, rows: 1, cols: 3 }]);
    }

    /// table_shape gives the same size, header / footer counts and merges as table(), on every
    /// table of the reference files (merged ones among them).
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn shape_matches_table() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut checked = 0;
        let mut merged = 0;
        for name in ["R13-merge-1-merged.key", "R13-merge-0-before.key", "K-formula-0-before.key", "K-avg.key"] {
            let k = KeyFile::open(dir.join(name)).unwrap();
            for (_, info) in k.objects_of("TST.TableInfoArchive") {
                let (Some(full), Some(shape)) = (table(&k, info), table_shape(&k, info)) else { continue };
                assert_eq!((shape.rows, shape.cols, shape.header_rows, shape.header_cols, shape.footer_rows), (full.rows, full.cols, full.header_rows, full.header_cols, full.footer_rows), "{name}");
                assert_eq!(shape.merges, full.merges, "{name}");
                merged += usize::from(!full.merges.is_empty());
                checked += 1;
            }
        }
        assert!(checked >= 4 && merged >= 1, "{checked} tables, {merged} with merges");
    }

    fn fmt(m: &[(&str, u64)]) -> Msg {
        let mut f = Msg::new("TSK.FormatStructArchive");
        for (k, v) in m {
            f.set_u64(k, *v);
        }
        f
    }

    #[test]
    fn number_formats() {
        let mut cur = fmt(&[("format_type", 257), ("decimal_places", 2)]);
        cur.set_str("currency_code", "USD");
        assert_eq!(fmt_number(1250.0, &cur), "$1250.00");
        assert_eq!(fmt_number(-5.5, &cur), "-$5.50");
        let mut th = fmt(&[("format_type", 256), ("decimal_places", 2)]);
        th.set_bool("show_thousands_separator", true);
        assert_eq!(fmt_number(1234567.891, &th), "1,234,567.89");
        assert_eq!(fmt_number(0.256, &fmt(&[("format_type", 258), ("decimal_places", 253)])), "25.6%");
        assert_eq!(fmt_number(6.02e23, &fmt(&[("format_type", 259), ("decimal_places", 253)])), "6.02E+23");
        assert_eq!(fmt_number(0.375, &fmt(&[("format_type", 262), ("fraction_accuracy", 4294967293)])), "3/8");
        assert_eq!(fmt_number(1.5, &fmt(&[("format_type", 262), ("fraction_accuracy", 4294967295)])), "1 1/2");
    }

    #[test]
    fn dates_and_durations() {
        assert_eq!(format_date(813_508_200.0, "M/d/yy h:mm\u{202f}a"), "10/12/26 2:30\u{202f}PM");
        assert_eq!(format_date(0.0, "EEEE, MMMM d, yyyy"), "Monday, January 1, 2001");
        let mut d = fmt(&[("format_type", 268), ("duration_style", 1), ("duration_unit_largest", 4), ("duration_unit_smallest", 8)]);
        d.set_bool("use_automatic_duration_units", true);
        assert_eq!(fmt_duration(5400.0, &d), "1h 30m");
        let mut d0 = fmt(&[("format_type", 268), ("duration_style", 0), ("duration_unit_largest", 4), ("duration_unit_smallest", 16)]);
        d0.set_bool("use_automatic_duration_units", false);
        assert_eq!(fmt_duration(5400.0, &d0), "1:30:00");
    }

    #[test]
    fn decimal_record() {
        // 0.375 stored as 375e-3, number format slot id 2
        let mut rec = vec![5u8, 2, 0, 0, 0, 0, 1, 0, 0x01, 0x30, 0, 0];
        rec.extend_from_slice(&[0x77, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x3a, 0x30]);
        rec.extend_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0]);
        let r = cell_record(&rec, &HashMap::new(), &HashMap::new()).unwrap();
        assert_eq!(r.value, CellValue::Number(0.375));
        assert_eq!(r.format, Some(2));
    }

    /// The first table on a catalog slide (1-based slide number).
    fn catalog_table(slide: usize) -> TableData {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let doc = crate::Document::open(path).unwrap();
        let slides = doc.slides();
        let scene = doc.scene(&slides, slide - 1).unwrap();
        scene
            .drawables
            .into_iter()
            .find_map(|d| if let DrawableKind::Table(t) = d.kind { Some(t) } else { None })
            .expect("table on slide")
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn catalog_formulas_and_currency() {
        let t = catalog_table(26);
        let cell = |r: u32, c: u32| t.cells[(r * t.cols + c) as usize].as_str();
        assert_eq!((t.rows, t.cols, t.header_rows, t.header_cols, t.footer_rows), (6, 5, 1, 1, 1));
        assert_eq!(cell(0, 0), "Region");
        assert_eq!(cell(1, 1), "$1250.00");
        assert_eq!(cell(1, 3), "$2225.00"); // formula result
        assert_eq!(cell(5, 3), "$20300.00"); // SUM in the footer
        // header row fill, per-cell fill and text colour overrides, banding
        let style = |r: u32, c: u32| &t.cell_styles[(r * t.cols + c) as usize];
        assert!(matches!(style(0, 1).fill, Fill::Color(c) if c.b > 0.45 && c.r < 0.05));
        assert!(matches!(style(1, 1).fill, Fill::Color(c) if c.b > 0.99 && c.r > 0.9 && c.r < 0.95));
        assert!(style(1, 2).text.color.g > 0.5 && style(1, 2).text.color.r < 0.1);
        assert_ne!(style(1, 3).fill, style(2, 3).fill);
        assert_eq!(style(1, 0).text.font_name, "Georgia");
        // only the footer separator is visible
        assert!(t.h_strokes[(5 * t.cols + 2) as usize].is_some());
        assert!(t.h_strokes[(2 * t.cols + 2) as usize].is_none());
    }

    #[test]
    fn legacy_records() {
        // number with a cell style and a format (pre-BNC layout)
        let mut rec = vec![4u8, 2, 0, 0, 0x26, 0, 0, 0, 0, 0, 0, 0];
        rec.extend_from_slice(&[7, 0, 0, 0, 3, 0, 0, 0]);
        rec.extend_from_slice(&1.5f64.to_le_bytes());
        let r = legacy_cell_record(&rec, &HashMap::new(), &HashMap::new()).unwrap();
        assert_eq!((r.value, r.cell_style, r.format, r.legacy), (CellValue::Number(1.5), Some(7), Some(3), true));
        // string
        let mut rec = vec![4u8, 3, 0, 0, 0x10, 0, 0, 0, 0, 0, 0, 0];
        rec.extend_from_slice(&[9, 0, 0, 0]);
        let strings = HashMap::from([(9, "hello".to_string())]);
        assert_eq!(legacy_cell_record(&rec, &strings, &HashMap::new()).unwrap().value, CellValue::Text("hello".into()));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn conditional_highlighting_rule() {
        // Keynote's "greater than 5 -> Red Fill" rule on the Value column (Mac round 13)
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-condhl-1-rule.key");
        let k = KeyFile::open(path).unwrap();
        let (_, info) = k.first_of("TST.TableInfoArchive").unwrap();
        let t = table(&k, info).unwrap();
        let red = |r: u32| matches!(t.cell_styles[(r * t.cols + 1) as usize].fill, Fill::Color(c) if c.r > 0.9 && c.g < 0.7);
        assert_eq!((1..=8).map(red).collect::<Vec<_>>(), [false, false, false, true, true, true, true, true]);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn catalog_cell_formats_and_merge() {
        let t = catalog_table(27);
        let value = |r: u32| t.cells[(r * t.cols + 1) as usize].as_str();
        let expect = [
            "auto 123", "1234.5678", "99.95", "25.6%", "10/12/26 2:30\u{202f}PM", "1h 30m", "3/8", "6.02E+23", "00123", "1", "4",
            "50", "7", "Option",
        ];
        for (i, e) in expect.iter().enumerate() {
            assert_eq!(value(i as u32 + 1), *e, "row {}", i + 1);
        }
        assert_eq!(t.merges, vec![CellRange { row: 2, col: 2, rows: 6, cols: 1 }]);
    }
}
