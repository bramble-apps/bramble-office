//! Autofill (dragging the yellow dot at a selection's edge, Keynote 15.4 R32): the selected
//! cells repeat into the cells dragged over. Numbers in two or more cells continue as a linear
//! series (1, 2 -> 3, 4, 5), "Item 1", "Item 2" continue their number, anything else repeats
//! in order; formulas are copied with their relative references moving along.

use kn_format::ObjId;

use crate::edit::History;
use crate::table::CellValue;
use crate::tablecells::CellInput;
use crate::Document;

/// "Item 12" -> ("Item ", 12).
fn numbered(t: &str) -> Option<(&str, i64)> {
    let digits = t.len() - t.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 || digits == t.len() {
        return None;
    }
    let (head, num) = t.split_at(t.len() - digits);
    Some((head, num.parse().ok()?))
}

/// Days since 2001-01-01 -> (year, month, day).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 11_323 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// (year, month, day) -> days since 2001-01-01.
fn days_of(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((i64::from(m) + 9) % 12) + 2) / 5 + i64::from(d) - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468 - 11_323
}

fn month_len(y: i64, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_of(ny, nm, 1) - days_of(y, m, 1)) as u32
}

/// Seed dates on the same day of the month (or all on a month's last day), a whole number of
/// months apart: that step in months (Numbers: Jan 31, Feb 28 -> Mar 31).
fn month_step(v: &[f64]) -> Option<i64> {
    if v.len() < 2 {
        return None;
    }
    let day = 86_400.0;
    let parts: Vec<((i64, u32, u32), f64)> = v.iter().map(|x| (civil(x.div_euclid(day) as i64), x.rem_euclid(day))).collect();
    let ((y0, m0, d0), t0) = parts[0];
    let last = |(y, m, d): (i64, u32, u32)| d == month_len(y, m);
    let same_day = parts.iter().all(|(c, t)| *t == t0 && (c.2 == d0 || (last(*c) && last(parts[0].0))));
    if !same_day {
        return None;
    }
    let months = |(y, m, _): (i64, u32, u32)| y * 12 + i64::from(m) - 1;
    let step = months(parts[1].0) - months((y0, m0, d0));
    (step != 0 && parts.windows(2).all(|w| months(w[1].0) - months(w[0].0) == step)).then_some(step)
}

/// `base` moved on by `k` months, the day kept (or the month's last day; a month-end seed stays
/// at month ends).
fn add_months(base: f64, k: i64) -> f64 {
    let day = 86_400.0;
    let (y, m, d) = civil(base.div_euclid(day) as i64);
    let end = d == month_len(y, m);
    let total = y * 12 + i64::from(m) - 1 + k;
    let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let nd = if end { month_len(ny, nm) } else { d.min(month_len(ny, nm)) };
    days_of(ny, nm, nd) as f64 * day + base.rem_euclid(day)
}

impl Document {
    /// Fill from `src` (r0, c0, r1, c1) to `to` (the same block grown down, right, up or left:
    /// Numbers' Table > Autofill Cells goes all four ways; a series counts on outwards, so
    /// 5, 6 filled up gives 4, 3).
    pub fn autofill(&mut self, h: &mut History, table: ObjId, src: (u32, u32, u32, u32), to: (u32, u32, u32, u32)) -> Result<(), kn_format::Error> {
        // along rows (down / up) or along columns (right / left); backwards for up / left
        let (down, back) = if to.2 > src.2 {
            (true, false)
        } else if to.3 > src.3 {
            (false, false)
        } else if to.0 < src.0 {
            (true, true)
        } else if to.1 < src.1 {
            (false, true)
        } else {
            return Ok(());
        };
        let values = self.cell_values(table);
        let formulas: std::collections::HashMap<(u32, u32), Vec<crate::formula::Node>> = self
            .formula_cells(table)
            .into_iter()
            .filter_map(|(rc, key)| Some((rc, self.formula_entry(table, key).and_then(crate::formula::read_nodes)?)))
            .collect();
        // each line along the fill: a column when filling down, a row when filling right
        let lines: Vec<u32> = if down { (src.1..=src.3).collect() } else { (src.0..=src.2).collect() };
        for line in lines {
            // the source cells in fill order (from the far end towards the new cells)
            let mut cells: Vec<(u32, u32)> = if down { (src.0..=src.2).map(|r| (r, line)).collect() } else { (src.1..=src.3).map(|c| (line, c)).collect() };
            if back {
                cells.reverse();
            }
            let n = cells.len();
            let nums: Option<Vec<f64>> = cells.iter().map(|rc| match values.get(rc) {
                Some(CellValue::Number(x)) => Some(*x),
                _ => None,
            }).collect();
            // dates: one goes on a day at a time, several by their step
            let dates: Option<Vec<f64>> = cells.iter().map(|rc| match values.get(rc) {
                Some(CellValue::Date(x)) => Some(*x),
                _ => None,
            }).collect();
            let texts: Vec<Option<(String, i64)>> = cells.iter().map(|rc| match values.get(rc) {
                Some(CellValue::Text(t)) => numbered(t).map(|(h, k)| (h.to_string(), k)),
                _ => None,
            }).collect();
            let targets: Vec<(u32, u32)> = match (down, back) {
                (true, false) => (src.2 + 1..=to.2).map(|r| (r, line)).collect(),
                (false, false) => (src.3 + 1..=to.3).map(|c| (line, c)).collect(),
                (true, true) => (to.0..src.0).rev().map(|r| (r, line)).collect(),
                (false, true) => (to.1..src.1).rev().map(|c| (line, c)).collect(),
            };
            for (k, &(r, c)) in targets.iter().enumerate() {
                let step_n = (n + k) as f64; // position in the series (0-based)
                let from = cells[(n + k) % n];
                // a formula: the same formula one cell further on
                if let Some(nodes) = formulas.get(&from) {
                    self.insert_formula_nodes(h, table, r, c, nodes)?;
                    continue;
                }
                if let Some(v) = &dates {
                    let step = if n == 1 { if back { -86_400.0 } else { 86_400.0 } } else { (v[n - 1] - v[0]) / (n - 1) as f64 };
                    let k = step_n - (n - 1) as f64; // how far past the last source cell
                    let next = match (n, month_step(v)) {
                        (2.., Some(ms)) => add_months(v[0], ms * step_n as i64),
                        _ => v[n - 1] + step * k,
                    };
                    let fmt = self.cell_format(table, from.0, from.1);
                    self.set_cell(h, table, r, c, &CellInput::Date(next))?;
                    if matches!(fmt, crate::tablefmt::CellFormat::DateTime { .. }) {
                        self.set_cells_format(h, table, (r, c, r, c), &fmt)?;
                    }
                    continue;
                }
                let input = match (&nums, n) {
                    (Some(v), 2..) => {
                        let step = (v[n - 1] - v[0]) / (n - 1) as f64;
                        CellInput::Number(v[0] + step * step_n)
                    }
                    _ => match (texts.iter().all(|t| t.is_some()) && n >= 2, &texts[0]) {
                        (true, Some((head, first))) if texts.iter().all(|t| t.as_ref().map(|x| &x.0) == Some(head)) => {
                            let last = texts[n - 1].as_ref().map(|x| x.1).unwrap_or(*first);
                            let step = (last - first) / (n as i64 - 1).max(1);
                            CellInput::Text(format!("{head}{}", first + step * step_n as i64))
                        }
                        _ => match values.get(&from) {
                            Some(CellValue::Number(x)) => CellInput::Number(*x),
                            Some(CellValue::Text(t)) => CellInput::Text(t.clone()),
                            _ => CellInput::Empty,
                        },
                    },
                };
                self.set_cell(h, table, r, c, &input)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn series_text_and_formulas() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R30-formulas-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Fill");
        // B2:B3 = 2, 3 -> 4, 5, 6 below (B4 was 5)
        d.autofill(&mut h, t, (1, 1, 2, 1), (1, 1, 5, 1)).unwrap();
        // a numbered text and a formula, filled down
        d.set_cell(&mut h, t, 1, 2, &CellInput::Text("Item 1".into())).unwrap();
        d.set_cell(&mut h, t, 2, 2, &CellInput::Text("Item 2".into())).unwrap();
        d.autofill(&mut h, t, (1, 2, 2, 2), (1, 2, 3, 2)).unwrap();
        d.set_formula_text(&mut h, t, 1, 3, "=B2*2").unwrap();
        d.autofill(&mut h, t, (1, 3, 1, 3), (1, 3, 3, 3)).unwrap();
        d.end(&mut h);
        let v = d.cell_values(t);
        let num = |r, c| match v.get(&(r, c)) {
            Some(CellValue::Number(x)) => *x,
            other => panic!("{other:?}"),
        };
        assert_eq!([num(3, 1), num(4, 1), num(5, 1)], [4.0, 5.0, 6.0]);
        assert_eq!(v.get(&(3, 2)), Some(&CellValue::Text("Item 3".into())));
        assert_eq!(d.formula_text(t, 3, 3).as_deref(), Some("=B4×2"));
        assert_eq!(num(3, 3), 8.0);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn fills_up_and_left() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R30-formulas-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Fill");
        // C4:C5 = 5, 6 filled up to C2: 4, 3
        d.set_cell(&mut h, t, 3, 2, &CellInput::Number(5.0)).unwrap();
        d.set_cell(&mut h, t, 4, 2, &CellInput::Number(6.0)).unwrap();
        d.autofill(&mut h, t, (3, 2, 4, 2), (1, 2, 4, 2)).unwrap();
        // a single cell repeats to its left: D6 into B6:C6
        d.set_cell(&mut h, t, 5, 3, &CellInput::Text("Week".into())).unwrap();
        d.autofill(&mut h, t, (5, 3, 5, 3), (5, 1, 5, 3)).unwrap();
        d.end(&mut h);
        let v = d.cell_values(t);
        assert_eq!(v.get(&(2, 2)), Some(&CellValue::Number(4.0)));
        assert_eq!(v.get(&(1, 2)), Some(&CellValue::Number(3.0)));
        assert_eq!(v.get(&(5, 1)), Some(&CellValue::Text("Week".into())));
        assert_eq!(v.get(&(5, 2)), Some(&CellValue::Text("Week".into())));
    }

    /// One date goes on a day at a time (back a day upwards), two by their step, keeping
    /// their date format.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn dates_continue() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R30-formulas-0-before.key");
        let mut d = Document::open(p).unwrap();
        let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
        let mut h = History::default();
        let day = 86_400.0;
        let start = 820_000_000.0_f64.div_euclid(day) * day;
        d.begin(&mut h, "Fill");
        d.set_cell(&mut h, t, 2, 1, &CellInput::Date(start)).unwrap();
        d.set_cells_format(&mut h, t, (2, 1, 2, 1), &crate::tablefmt::CellFormat::DateTime { pattern: "M/d".into() }).unwrap();
        d.autofill(&mut h, t, (2, 1, 2, 1), (1, 1, 4, 1)).unwrap();
        d.set_cell(&mut h, t, 1, 2, &CellInput::Date(start)).unwrap();
        d.set_cell(&mut h, t, 2, 2, &CellInput::Date(start + 7.0 * day)).unwrap();
        d.autofill(&mut h, t, (1, 2, 2, 2), (1, 2, 3, 2)).unwrap();
        d.end(&mut h);
        let v = d.cell_values(t);
        let date = |r, c| match v.get(&(r, c)) {
            Some(CellValue::Date(x)) => *x,
            other => panic!("{other:?}"),
        };
        assert_eq!([date(3, 1), date(4, 1)], [start + day, start + 2.0 * day]);
        assert_eq!(d.cell_format(t, 4, 1), crate::tablefmt::CellFormat::DateTime { pattern: "M/d".into() });
        assert_eq!(date(3, 2), start + 14.0 * day);
        // Jan 31, Feb 28 -> Mar 31, Apr 30 (months, at month ends)
        let jan31 = days_of(2027, 1, 31) as f64 * day;
        d.begin(&mut h, "Fill");
        d.set_cell(&mut h, t, 1, 3, &CellInput::Date(jan31)).unwrap();
        d.set_cell(&mut h, t, 2, 3, &CellInput::Date(days_of(2027, 2, 28) as f64 * day)).unwrap();
        d.autofill(&mut h, t, (1, 3, 2, 3), (1, 3, 4, 3)).unwrap();
        d.end(&mut h);
        let v = d.cell_values(t);
        let ymd = |r| match v.get(&(r, 3)) {
            Some(CellValue::Date(x)) => civil((x / day) as i64),
            other => panic!("{other:?}"),
        };
        assert_eq!([ymd(3), ymd(4)], [(2027, 3, 31), (2027, 4, 30)]);
        assert_eq!(civil(0), (2001, 1, 1));
    }
}
