//! Formula text for what Keydeck's formula reader leaves alone: references to other tables
//! and sheets ("Table 2::B2", "Sheet 2::Table 1::B2"), whole columns, functions it doesn't
//! know. Rendered straight from the stored RPN AST (research NUMBERS-FILE-FORMAT §4); any node
//! not handled here gives None, so nothing wrong is ever shown. Display only: editing such a
//! formula keeps it unless the new text parses.

use std::collections::HashMap;
use std::sync::OnceLock;

use kn_format::tables::{family, uuid_of_cf, uuid_of_str};
use kn_format::{KeyFile, Msg, ObjId};

/// Function names by TSCE index (data/functions.tsv, read from Numbers' own file).
fn function_name(index: u64) -> Option<&'static str> {
    static NAMES: OnceLock<HashMap<u64, &'static str>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            include_str!("../data/functions.tsv")
                .lines()
                .skip(1)
                .filter_map(|l| {
                    let f: Vec<&str> = l.split('\t').collect();
                    Some((f.get(2)?.parse().ok()?, *f.first()?))
                })
                .collect()
        })
        .get(&index)
        .copied()
}

/// One function of the catalog (Numbers 15.4's Function Browser, research N1/N4).
#[derive(Debug, Clone, Copy)]
pub struct FunctionInfo {
    pub name: &'static str,
    pub category: &'static str,
    pub syntax: &'static str,
    pub description: &'static str,
}

/// The whole catalog, alphabetical.
pub fn catalog() -> &'static [FunctionInfo] {
    static ALL: OnceLock<Vec<FunctionInfo>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<FunctionInfo> = include_str!("../data/functions.tsv")
            .lines()
            .skip(1)
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                Some(FunctionInfo { name: f.first()?, category: f.get(1)?, syntax: f.get(3)?, description: f.get(4).copied().unwrap_or("") })
            })
            .collect();
        v.sort_by_key(|f| f.name);
        v
    })
}

/// Every function: (name, syntax), alphabetical (formula editor suggestions).
pub fn functions() -> &'static [(&'static str, &'static str)] {
    static ALL: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut v: Vec<(&str, &str)> = include_str!("../data/functions.tsv")
            .lines()
            .skip(1)
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                Some((*f.first()?, *f.get(3)?))
            })
            .collect();
        v.sort_by_key(|f| f.0);
        v
    })
}

fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// The FORMULA list key a storage_version 5 cell record refers to.
fn record_formula_key(rec: &[u8]) -> Option<u32> {
    if rec.first() != Some(&5) || rec.len() < 12 {
        return None;
    }
    let flags = u32::from_le_bytes(rec[8..12].try_into().ok()?);
    if flags & 0x200 == 0 {
        return None;
    }
    let mut at = 12;
    for (bit, len) in [(1, 16), (2, 8), (4, 8), (8, 4), (0x10, 4), (0x20, 4), (0x40, 4), (0x80, 4), (0x100, 4)] {
        if flags & bit != 0 {
            at += len;
        }
    }
    u32::from_le_bytes(rec.get(at..at + 4)?.try_into().ok()?).into()
}

/// The FORMULA list entry of cell (row, col) of table drawable `info`.
pub fn formula_msg(k: &KeyFile, info: ObjId, row: u32, col: u32) -> Option<Msg> {
    let model = k.obj(info)?.ref_id("tableModel").and_then(|t| k.obj(t))?;
    let ds = model.msg("base_data_store")?;
    let rec = kn_model::table::raw_cell_records(k, model).into_iter().find(|(rc, _)| *rc == (row, col))?.1;
    let key = record_formula_key(&rec)?;
    let list = k.obj(ds.ref_id("formula_table")?)?;
    list.msgs("entries").find(|e| e.u32("key") == Some(key)).and_then(|e| e.msg("formula")).cloned()
}

/// Every table in the document by uuid family -> (sheet name, table name, sheet id). A formula
/// names another table by that table's formula owner uid, of the table's family (N9).
fn tables(k: &KeyFile) -> HashMap<[u8; 12], (String, String, ObjId)> {
    let mut out = HashMap::new();
    let Some((_, d)) = k.first_of("TN.DocumentArchive") else { return out };
    for s in d.ref_ids("sheets") {
        let Some(sm) = k.obj(s) else { continue };
        let sname = sm.str("name").unwrap_or("").to_string();
        for di in sm.ref_ids("drawable_infos") {
            let Some(model) = k.obj(di).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)) else { continue };
            if let Some(f) = model.str("table_id").and_then(uuid_of_str).and_then(|u| family(&u)) {
                out.insert(f, (sname.clone(), model.str("table_name").unwrap_or("").to_string(), s));
            }
        }
    }
    out
}

/// "=…" text of the formula in (row, col) of table `info`, or None.
pub fn formula_text(k: &KeyFile, info: ObjId, row: u32, col: u32) -> Option<String> {
    let f = formula_msg(k, info, row, col)?;
    let my_sheet = k.obj(info)?.find_super("TSD.DrawableArchive").and_then(|d| d.ref_id("parent"));
    let tabs = tables(k);
    let prefix = |n: &Msg| -> Option<String> {
        let Some(x) = n.msg("AST_cross_table_reference_extra_info") else { return Some(String::new()) };
        let (sheet, table, sid) = tabs.get(&family(&uuid_of_cf(x.msg("table_id")?))?)?;
        Some(if Some(*sid) == my_sheet { format!("{table}::") } else { format!("{sheet}::{table}::") })
    };
    let coord = |base: u32, v: i64, abs: bool| -> Option<(u32, bool)> { Some((if abs { u32::try_from(v).ok()? } else { u32::try_from(base as i64 + v).ok()? }, abs)) };
    // (text, precedence)
    let mut st: Vec<(String, u8)> = Vec::new();
    let paren = |(t, p): (String, u8), min: u8| if p < min { format!("({t})") } else { t };
    for n in f.msg("AST_node_array")?.msgs("AST_node") {
        let ty = n.enum_name("AST_node_type")?;
        let bin = |op: &str, prec: u8, st: &mut Vec<(String, u8)>| -> Option<()> {
            let b = st.pop()?;
            let a = st.pop()?;
            st.push((format!("{}{op}{}", paren(a, prec), paren(b, prec + 1)), prec));
            Some(())
        };
        match ty {
            "ADDITION_NODE" => bin("+", 3, &mut st)?,
            "SUBTRACTION_NODE" => bin("−", 3, &mut st)?,
            "MULTIPLICATION_NODE" => bin("×", 4, &mut st)?,
            "DIVISION_NODE" => bin("÷", 4, &mut st)?,
            "POWER_NODE" => bin("^", 5, &mut st)?,
            "CONCATENATION_NODE" => bin("&", 2, &mut st)?,
            "GREATER_THAN_NODE" => bin(">", 1, &mut st)?,
            "GREATER_THAN_OR_EQUAL_TO_NODE" => bin("≥", 1, &mut st)?,
            "LESS_THAN_NODE" => bin("<", 1, &mut st)?,
            "LESS_THAN_OR_EQUAL_TO_NODE" => bin("≤", 1, &mut st)?,
            "EQUAL_TO_NODE" => bin("=", 1, &mut st)?,
            "NOT_EQUAL_TO_NODE" => bin("≠", 1, &mut st)?,
            "NEGATION_NODE" => {
                let a = st.pop()?;
                st.push((format!("−{}", paren(a, 6)), 6));
            }
            "PLUS_SIGN_NODE" => {
                let a = st.pop()?;
                st.push((format!("+{}", paren(a, 6)), 6));
            }
            "PERCENT_NODE" => {
                let a = st.pop()?;
                st.push((format!("{}%", paren(a, 7)), 7));
            }
            "NUMBER_NODE" => st.push((kn_model::table::fmt_num(n.f64("AST_number_node_number")?), 9)),
            "BOOLEAN_NODE" => st.push((if n.bool("AST_boolean_node_boolean")? { "TRUE" } else { "FALSE" }.into(), 9)),
            "STRING_NODE" => st.push((format!("\"{}\"", n.str("AST_string_node_string").unwrap_or("").replace('"', "\"\"")), 9)),
            "EMPTY_ARGUMENT_NODE" => st.push((String::new(), 9)),
            "PREPEND_WHITESPACE_NODE" | "APPEND_WHITESPACE_NODE" | "BEGIN_THUNK_NODE" | "END_THUNK_NODE" => {}
            "LIST_NODE" => {
                let k = n.u32("AST_list_node_numArgs").unwrap_or(1) as usize;
                let args: Vec<String> = st.split_off(st.len().checked_sub(k)?).into_iter().map(|a| a.0).collect();
                st.push((format!("({})", args.join(", ")), 9));
            }
            "FUNCTION_NODE" => {
                let name = function_name(n.u64("AST_function_node_index")?)?;
                let k = n.u32("AST_function_node_numArgs").unwrap_or(0) as usize;
                let args: Vec<String> = st.split_off(st.len().checked_sub(k)?).into_iter().map(|a| a.0).collect();
                st.push((format!("{name}({})", args.join(", ")), 9));
            }
            "CELL_REFERENCE_NODE" => {
                let c = n.msg("AST_column")?;
                let (cc, cabs) = coord(col, c.i64("column")?, c.bool("absolute") == Some(true))?;
                let cs = format!("{}{}", if cabs { "$" } else { "" }, col_name(cc));
                let text = match n.msg("AST_row") {
                    Some(r) => {
                        let (rr, rabs) = coord(row, r.i64("row")?, r.bool("absolute") == Some(true))?;
                        format!("{cs}{}{}", if rabs { "$" } else { "" }, rr + 1)
                    }
                    None => cs, // a whole column
                };
                st.push((format!("{}{text}", prefix(n)?), 9));
            }
            "COLON_TRACT_NODE" => {
                let t = n.msg("AST_colon_tract")?;
                let range = |rel: &str, abs: &str, base: u32| -> Option<Option<(u32, u32, bool)>> {
                    if let Some(r) = t.msgs(rel).next() {
                        let b = r.i64("range_begin")?;
                        let e = r.i64("range_end").unwrap_or(b);
                        return Some(Some((u32::try_from(base as i64 + b).ok()?, u32::try_from(base as i64 + e).ok()?, false)));
                    }
                    if let Some(r) = t.msgs(abs).next() {
                        let b = r.u64("range_begin")? as u32;
                        return Some(Some((b, r.u64("range_end").map(|e| e as u32).unwrap_or(b), true)));
                    }
                    Some(None)
                };
                let cols = range("relative_column", "absolute_column", col)?;
                let rows = range("relative_row", "absolute_row", row)?;
                let text = match (cols, rows) {
                    (Some((c0, c1, ca)), Some((r0, r1, ra))) => {
                        let d = |a: bool| if a { "$" } else { "" };
                        format!("{}{}{}{}:{}{}{}{}", d(ca), col_name(c0), d(ra), r0 + 1, d(ca), col_name(c1), d(ra), r1 + 1)
                    }
                    (Some((c0, c1, _)), None) => format!("{}:{}", col_name(c0), col_name(c1)),
                    (None, Some((r0, r1, _))) => format!("{}:{}", r0 + 1, r1 + 1),
                    (None, None) => return None,
                };
                st.push((format!("{}{text}", prefix(n)?), 9));
            }
            _ => return None,
        }
    }
    (st.len() == 1).then(|| format!("={}", st.pop().unwrap().0))
}

/// Recalculate formulas that read one other table (Keydeck's recalculation leaves them alone):
/// their references are evaluated in that table and the result stored as the cell's cached
/// value. Mixed or unreadable formulas stay as they are (Numbers recalculates on open).
/// Returns the number of cells changed.
pub fn recalc_cross(doc: &mut kn_model::Document, h: &mut kn_model::edit::History) -> usize {
    use kn_model::formula::Value;
    use kn_model::table::CellValue;
    use kn_model::tablecells::CellInput;
    let mut changed = 0;
    for _ in 0..3 {
        let k = &doc.file;
        let infos: Vec<ObjId> = k.objects_of("TST.TableInfoArchive").map(|(id, _)| id).collect();
        let mut by_family: HashMap<[u8; 12], ObjId> = HashMap::new();
        for i in &infos {
            if let Some(f) = k.obj(*i).and_then(|m| m.ref_id("tableModel")).and_then(|t| k.obj(t)).and_then(|m| m.str("table_id")).and_then(uuid_of_str).and_then(|u| family(&u)) {
                by_family.insert(f, *i);
            }
        }
        let values_of = |t: ObjId| -> HashMap<(u32, u32), CellValue> {
            let Some(model) = k.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|x| k.obj(x)) else { return HashMap::new() };
            kn_model::table::cell_records(k, model).into_iter().map(|(rc, r)| (rc, r.value)).collect()
        };
        let mut updates: Vec<(ObjId, u32, u32, CellInput)> = Vec::new();
        for &t in &infos {
            let Some(model) = k.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|x| k.obj(x)) else { continue };
            let Some(list) = model.msg("base_data_store").and_then(|d| d.ref_id("formula_table")).and_then(|l| k.obj(l)) else { continue };
            let entries: HashMap<u32, &Msg> = list.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.msg("formula")?))).collect();
            let mine = values_of(t);
            for ((r, c), rec) in kn_model::table::raw_cell_records(k, model) {
                let Some(key) = record_formula_key(&rec) else { continue };
                let Some(f) = entries.get(&key) else { continue };
                // only formulas Keydeck declines because of their table references
                if kn_model::formula::read_nodes(f).is_some() {
                    continue;
                }
                let nodes: Vec<&Msg> = f.msg("AST_node_array").map(|a| a.msgs("AST_node").collect()).unwrap_or_default();
                let targets: Vec<Option<[u8; 12]>> = nodes
                    .iter()
                    .filter(|n| matches!(n.enum_name("AST_node_type"), Some("CELL_REFERENCE_NODE" | "COLON_TRACT_NODE")))
                    .map(|n| n.msg("AST_cross_table_reference_extra_info").and_then(|x| x.msg("table_id")).and_then(|u| family(&uuid_of_cf(u))))
                    .collect();
                let Some(Some(target)) = targets.first().copied() else { continue };
                if targets.iter().any(|x| *x != Some(target)) {
                    continue;
                }
                let Some(&other) = by_family.get(&target) else { continue };
                let mut plain = (*f).clone();
                if let Some(arr) = plain.msg_mut("AST_node_array") {
                    for n in arr.msgs_mut("AST_node") {
                        n.remove("AST_cross_table_reference_extra_info");
                    }
                }
                let Some(nodes) = kn_model::formula::read_nodes(&plain) else { continue };
                let input = match kn_model::formula::eval(&nodes, r, c, &values_of(other)) {
                    Value::Num(v) => CellInput::Formula(v, key),
                    Value::Text(s) => CellInput::FormulaText(s, key),
                    Value::Bool(b) => CellInput::FormulaText(if b { "TRUE" } else { "FALSE" }.into(), key),
                    _ => continue,
                };
                let same = match (&input, mine.get(&(r, c))) {
                    (CellInput::Formula(v, _), Some(CellValue::Number(x))) => (v - x).abs() <= 1e-9 * v.abs().max(1.0),
                    (CellInput::FormulaText(s, _), Some(CellValue::Text(x))) => s == x,
                    _ => false,
                };
                if !same {
                    updates.push((t, r, c, input));
                }
            }
        }
        if updates.is_empty() {
            break;
        }
        for (t, r, c, input) in updates {
            if doc.set_cell(h, t, r, c, &input).is_ok() {
                changed += 1;
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> crate::Workbook {
        crate::Workbook::open(std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).join(name)).unwrap()
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn cross_table_and_sheet() {
        let wb = sample("10-formula-cross-table.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        assert_eq!(formula_text(wb.file(), t, 1, 1).as_deref(), Some("=T2::B2×2"));
        let wb = sample("11-formula-cross-sheet.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        assert_eq!(formula_text(wb.file(), t, 1, 1).as_deref(), Some("=S2::Table 1::B2+1"));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn cross_table_recalc() {
        use kn_model::tablecells::CellInput;
        // 10: Table 1 B2 = T2::B2×2 with T2 B2 = 7 (cached 14); set T2 B2 to 10 -> 20
        let mut wb = sample("10-formula-cross-table.numbers");
        let s = wb.sheets().remove(0);
        let tabs = wb.tables(&s);
        let (t1, t2) = (tabs[0].info, tabs[1].info);
        let mut h = kn_model::edit::History::default();
        wb.doc.begin(&mut h, "x");
        wb.doc.set_cell(&mut h, t2, 1, 1, &CellInput::Number(10.0)).unwrap();
        assert_eq!(recalc_cross(&mut wb.doc, &mut h), 1);
        wb.doc.end(&mut h);
        let td = wb.table_data(t1).unwrap();
        assert_eq!(td.cells[(td.cols + 1) as usize], "20");
        // the formula itself is untouched
        assert_eq!(formula_text(wb.file(), t1, 1, 1).as_deref(), Some("=T2::B2×2"));
        // nothing left to do
        wb.doc.begin(&mut h, "y");
        assert_eq!(recalc_cross(&mut wb.doc, &mut h), 0);
        wb.doc.end(&mut h);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn same_table_whole_column_and_functions() {
        let wb = sample("07-formula-same-table.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        assert_eq!(formula_text(wb.file(), t, 3, 1).as_deref(), Some("=B2+B3"));
        let wb = sample("09-formula-whole-column.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let txt = formula_text(wb.file(), t, 1, 2).unwrap_or_default();
        assert!(txt.contains("B:B") || txt.contains("B"), "{txt}");
        // every catalog formula either renders or is declined (no panics, no garbage)
        let wb = sample("N-functions.numbers");
        let s = wb.sheets().remove(0);
        let t = wb.tables(&s)[0].info;
        let rendered = (1..356).filter(|r| formula_text(wb.file(), t, *r, 1).is_some()).count();
        assert!(rendered > 250, "only {rendered} of 355 catalog formulas render");
    }
}
