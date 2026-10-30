//! What a typed entry means, past plain numbers: dates and times, as Numbers recognises them
//! (US order): "3/9", "03-09", "3/9/27", "2027-03-09", "March 9", "Mar 9, 2027",
//! "9 March 2027", "March 2027", "7:30 PM", "19:30", and a date followed by a time. The result is
//! the date (seconds since 2001-01-01, as Numbers stores dates) and a date format from the Data
//! Format menu that shows it the way it was typed.

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

/// Days from 2001-01-01 to y-m-d (proleptic Gregorian).
fn days(y: i64, m: u32, d: u32) -> i64 {
    // days from civil (H. Hinnant), relative to 1970-01-01, then moved to 2001
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468 - 11_323
}

fn month_len(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn month_named(w: &str) -> Option<(u32, bool)> {
    let w = w.trim_end_matches('.').to_lowercase();
    if w.len() < 3 {
        return None;
    }
    MONTHS.iter().position(|m| m.starts_with(&w)).map(|i| (i as u32 + 1, w.len() > 3))
}

/// "7:30 PM", "7 pm", "19:30", "19:30:15": seconds into the day and the time format.
fn time(s: &str) -> Option<(f64, &'static str)> {
    let t = s.trim().to_lowercase();
    let (body, ampm) = if let Some(b) = t.strip_suffix("am").or_else(|| t.strip_suffix("a.m.")) {
        (b.trim(), Some(false))
    } else if let Some(b) = t.strip_suffix("pm").or_else(|| t.strip_suffix("p.m.")) {
        (b.trim(), Some(true))
    } else {
        (t.as_str(), None)
    };
    let parts: Vec<&str> = body.split(':').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty() || p.len() > 2 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    // a bare hour needs am / pm ("7" is a number)
    if parts.len() == 1 && ampm.is_none() {
        return None;
    }
    let n: Vec<u32> = parts.iter().map(|p| p.parse().unwrap()).collect();
    let (mut h, m, sec) = (n[0], n.get(1).copied().unwrap_or(0), n.get(2).copied().unwrap_or(0));
    if m > 59 || sec > 59 {
        return None;
    }
    let fmt = match ampm {
        Some(pm) => {
            if h == 0 || h > 12 {
                return None;
            }
            h = h % 12 + if pm { 12 } else { 0 };
            match parts.len() {
                1 => "h a",
                2 => "h:mm a",
                _ => "h:mm:ss a",
            }
        }
        None => {
            if h > 23 {
                return None;
            }
            if parts.len() == 3 { "HH:mm:ss" } else { "HH:mm" }
        }
    };
    Some((f64::from(h * 3600 + m * 60 + sec), fmt))
}

fn year_of(s: &str) -> Option<(i64, bool)> {
    if !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match s.len() {
        2 => Some((2000 + s.parse::<i64>().ok()?, true)),
        4 => Some((s.parse().ok()?, false)),
        _ => None,
    }
}

/// A date alone: (year, month, day, format); `day_first` reads 3/9 as 3 September (regions
/// other than the US).
fn date(s: &str, this_year: i64, day_first: bool) -> Option<(i64, u32, u32, &'static str)> {
    let s = s.trim();
    let num = |x: &str| -> Option<u32> { (!x.is_empty() && x.len() <= 2 && x.bytes().all(|b| b.is_ascii_digit())).then(|| x.parse().ok()).flatten() };
    let ok = |y: i64, m: u32, d: u32| (1..=12).contains(&m) && d >= 1 && d <= month_len(y, m);
    // numeric: one kind of separator
    for sep in ['/', '-'] {
        let p: Vec<&str> = s.split(sep).collect();
        if p.len() < 2 || p.iter().any(|x| x.contains(['/', '-', '.', ' '])) {
            continue;
        }
        // 2027-03-09
        if p.len() == 3 && p[0].len() == 4 {
            let (y, m, d) = (p[0].parse().ok()?, num(p[1])?, num(p[2])?);
            return ok(y, m, d).then_some((y, m, d, "y-MM-dd"));
        }
        let (m, d) = if day_first { (num(p[1])?, num(p[0])?) } else { (num(p[0])?, num(p[1])?) };
        let zero = p[0].len() == 2 && p[1].len() == 2 && (p[0].starts_with('0') || p[1].starts_with('0'));
        let (y, fmt) = match p.get(2) {
            None => (this_year, if day_first { "d/M" } else { "M/d" }),
            Some(ys) => {
                let (y, short) = year_of(ys)?;
                (y, match (short, zero, day_first) {
                    (true, true, false) => "MM/dd/yy",
                    (true, false, false) => "M/d/yy",
                    (false, true, false) => "MM/dd/y",
                    (false, false, false) => "M/d/y",
                    (true, true, true) => "dd/MM/yy",
                    (true, false, true) => "d/M/yy",
                    (false, true, true) => "dd/MM/y",
                    (false, false, true) => "d/M/y",
                })
            }
        };
        if p.len() > 3 {
            return None;
        }
        return ok(y, m, d).then_some((y, m, d, fmt));
    }
    // with a month name: "March 9", "Mar 9, 2027", "9 March 2027", "March 2027"
    let words: Vec<&str> = s.split([' ', ',']).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [a, b] | [a, b, _] if month_named(a).is_some() => {
            let (m, full) = month_named(a)?;
            if let (Some((y, false)), 2) = (year_of(b), words.len()) {
                return Some((y, m, 1, "MMMM y"));
            }
            let d = num(b.trim_end_matches(|c: char| c.is_ascii_alphabetic()))?;
            let y = match words.get(2) {
                Some(ys) => year_of(ys)?.0,
                None => this_year,
            };
            let fmt = match (words.len(), full) {
                (2, _) => "MMMM d",
                (_, true) => "MMMM d, y",
                (_, false) => "MMM d, y",
            };
            ok(y, m, d).then_some((y, m, d, fmt))
        }
        [a, b] | [a, b, _] if month_named(b).is_some() => {
            let (m, full) = month_named(b)?;
            let d = num(a)?;
            let y = match words.get(2) {
                Some(ys) => year_of(ys)?.0,
                None => this_year,
            };
            let fmt = match (words.len(), full) {
                (2, _) => "MMMM d",
                (_, true) => "MMMM d, y",
                (_, false) => "MMM d, y",
            };
            ok(y, m, d).then_some((y, m, d, fmt))
        }
        _ => None,
    }
}

/// A typed date / time: (seconds since 2001-01-01, date format). `today` is the date a time
/// alone goes on (seconds since 2001-01-01, midnight).
pub fn parse_date(s: &str, today: f64) -> Option<(f64, String)> {
    parse_date_in(s, today, false)
}

/// As [`parse_date`], day first when `day_first` (the document's region isn't the US).
pub fn parse_date_in(s: &str, today: f64, day_first: bool) -> Option<(f64, String)> {
    let s = s.trim();
    if s.is_empty() || s.len() > 40 {
        return None;
    }
    let this_year = 2001 + (today / 86_400.0 / 365.2425).floor() as i64;
    if let Some((y, m, d, f)) = date(s, this_year, day_first) {
        return Some((days(y, m, d) as f64 * 86_400.0, f.to_string()));
    }
    if let Some((t, f)) = time(s) {
        return Some((today + t, f.to_string()));
    }
    // a date, then a time: split at each space from the right
    for (i, _) in s.match_indices(' ').collect::<Vec<_>>().into_iter().rev() {
        let (a, b) = (s[..i].trim_end_matches([',', ' ']), &s[i + 1..]);
        if let (Some((y, m, d, df)), Some((t, tf))) = (date(a, this_year, day_first), time(b)) {
            return Some((days(y, m, d) as f64 * 86_400.0 + t, format!("{df} {tf}")));
        }
    }
    None
}

/// Today (UTC) at midnight, in seconds since 2001-01-01.
pub fn today() -> f64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) - 978_307_200.0;
    (now / 86_400.0).floor() * 86_400.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TODAY: f64 = 846_115_200.0; // 2027-10-25

    fn ymd(s: &str) -> (String, String) {
        let (v, f) = parse_date(s, TODAY).unwrap();
        (kn_model::table::format_date(v, "y-MM-dd HH:mm"), f)
    }

    #[test]
    fn dates_as_typed() {
        assert_eq!(days(2001, 1, 1), 0);
        assert_eq!(ymd("03-09"), ("2027-03-09 00:00".into(), "M/d".into()));
        assert_eq!(ymd("3/9/27"), ("2027-03-09 00:00".into(), "M/d/yy".into()));
        assert_eq!(ymd("03/09/2026"), ("2026-03-09 00:00".into(), "MM/dd/y".into()));
        assert_eq!(ymd("2027-03-09"), ("2027-03-09 00:00".into(), "y-MM-dd".into()));
        assert_eq!(ymd("March 9"), ("2027-03-09 00:00".into(), "MMMM d".into()));
        assert_eq!(ymd("Mar 9, 2027"), ("2027-03-09 00:00".into(), "MMM d, y".into()));
        assert_eq!(ymd("9 March 2027"), ("2027-03-09 00:00".into(), "MMMM d, y".into()));
        assert_eq!(ymd("March 2027"), ("2027-03-01 00:00".into(), "MMMM y".into()));
        assert_eq!(ymd("7:30 PM"), ("2027-10-25 19:30".into(), "h:mm a".into()));
        assert_eq!(ymd("3/9/27 7:30 pm"), ("2027-03-09 19:30".into(), "M/d/yy h:mm a".into()));
        assert_eq!(ymd("2027-03-09 19:30"), ("2027-03-09 19:30".into(), "y-MM-dd HH:mm".into()));
        // day first (regions other than the US)
        let (v, f) = parse_date_in("3/9/27", TODAY, true).unwrap();
        assert_eq!((kn_model::table::format_date(v, "y-MM-dd"), f.as_str()), ("2027-09-03".to_string(), "d/M/yy"));
    }

    #[test]
    fn not_dates() {
        for s in ["7", "1.5", "13/40", "2/30", "Need to inquire...", "PUB/Restaurant", "Y", "4-12-2-1", "may be", "25:00", "Item 3"] {
            assert!(parse_date(s, TODAY).is_none(), "{s}");
        }
    }
}

/// Put what the user typed in cell (row, col): a recognised date / time becomes a date (with a
/// date format showing it as typed, unless the cell has a date format already); a cell
/// formatted as Text keeps the text; anything else as `CellInput::parse` reads it.
pub fn set_typed(d: &mut kn_model::Document, h: &mut kn_model::edit::History, table: kn_format::ObjId, row: u32, col: u32, text: &str) -> Result<(), kn_format::Error> {
    use kn_model::tablecells::CellInput;
    use kn_model::tablefmt::CellFormat;
    let input = CellInput::parse(text);
    let fmt = d.cell_format(table, row, col);
    if let (CellInput::Text(_), false) = (&input, fmt == CellFormat::Text) {
        // the document's region decides 3/9: 9 March in the US, 3 September elsewhere
        let day_first = crate::sheets::document_locale(d).is_some_and(|l| !l.ends_with("_US") && l.contains('_'));
        if let Some((secs, pattern)) = parse_date_in(text, today(), day_first) {
            d.set_cell(h, table, row, col, &CellInput::Date(secs))?;
            if !matches!(fmt, CellFormat::DateTime { .. }) {
                d.set_cells_format(h, table, (row, col, row, col), &CellFormat::DateTime { pattern })?;
            }
            return Ok(());
        }
    }
    d.set_cell(h, table, row, col, &input)
}
