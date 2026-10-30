//! Stock quotes a document keeps for STOCK / STOCKH: TSCE.RemoteDataStoreArchive, linked from
//! the calculation engine (`remote_data_store`). Numbers stores each fetched value under
//! {function_index 298 STOCK / 299 STOCKH, symbol, attribute, date} and reads formulas from it
//! until the next refresh (Mac N23).

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::formula::{RemoteValues, Value};
use crate::Document;

/// A quote to store: a number (with its currency code, if it's money), a fraction shown as a
/// percentage, or text.
#[derive(Clone, Debug, PartialEq)]
pub enum Quote {
    Number(f64, Option<String>),
    Percent(f64),
    Text(String),
}

fn key(spec: &Msg) -> Option<(u32, String, u32, Option<i64>)> {
    let func = spec.u32("function_index")?;
    let symbol = spec.str("symbol")?.to_uppercase();
    let attr = spec.u32("attribute")?;
    // STOCKH dates are whole days (seconds since 2001)
    let date = spec.f64("date").map(|d| d.round() as i64);
    Some((func, symbol, attr, date))
}

fn value(v: &Msg) -> Option<Value> {
    match v.u32("cell_value_type")? {
        4 => Some(Value::Num(v.msg("number_value")?.f64("value")?)),
        5 => Some(Value::Text(v.msg("string_value")?.str("value")?.to_string())),
        2 => Some(Value::Bool(v.msg("boolean_value").and_then(|b| b.u64("value")).unwrap_or(0) != 0)),
        _ => None,
    }
}

fn cell_value(q: &Quote) -> Msg {
    let mut v = Msg::new("TSCE.CellValueArchive");
    match q {
        Quote::Number(x, _) | Quote::Percent(x) => {
            let cur = match q {
                Quote::Number(_, c) => c.as_ref(),
                _ => None,
            };
            v.set_u64("cell_value_type", 4);
            let n = v.msg_mut_or_create("number_value");
            n.set_f64("value", *x);
            n.set_u64("unit_index", 360);
            let f = n.msg_mut_or_create("format");
            match cur {
                // currency, two places, as Numbers stores STOCK("AAPL","price") (N23)
                Some(c) => {
                    f.set_u64("format_type", 257);
                    f.set_u64("decimal_places", 2);
                    f.set_str("currency_code", c);
                    for k in ["negative_style", "show_thousands_separator", "use_accounting_style"] {
                        f.set_u64(k, 0);
                    }
                    n.set_str("currency_code", c);
                }
                None if matches!(q, Quote::Percent(_)) => {
                    f.set_u64("format_type", 258);
                    f.set_u64("decimal_places", 2);
                }
                None => f.set_u64("format_type", 256),
            }
            n.set_u64("format_is_explicit", 0);
            let (lo, hi) = crate::tablecond::decimal_parts(*x);
            n.set_u64("decimal_low", lo);
            n.set_u64("decimal_high", hi);
        }
        Quote::Text(s) => {
            v.set_u64("cell_value_type", 5);
            let t = v.msg_mut_or_create("string_value");
            t.set_str("value", s);
            t.msg_mut_or_create("format").set_u64("format_type", 260);
        }
    }
    v
}

impl Document {
    /// The document's remote data store, if it has one.
    fn remote_store(&self) -> Option<ObjId> {
        let (_, ce) = self.file.first_of("TSCE.CalculationEngineArchive")?;
        ce.ref_id("remote_data_store").filter(|id| self.file.obj(*id).is_some())
    }

    /// The quotes the document holds, for [`crate::formula::eval_with`].
    pub fn remote_values(&self) -> RemoteValues {
        let mut out = RemoteValues::new();
        let Some(store) = self.remote_store().and_then(|s| self.file.obj(s)) else { return out };
        for e in store.msg("value_map").into_iter().flat_map(|m| m.msgs("entry")) {
            if let (Some(k), Some(v)) = (e.msg("specifier").and_then(key), e.msg("value").and_then(value)) {
                out.insert(k, v);
            }
        }
        out
    }

    /// Store a fetched quote (creating the store if the document has none) and recalculate the
    /// tables' formulas. `date` is STOCKH's day (days since 1970); None for STOCK.
    pub fn set_remote_value(&mut self, h: &mut History, func: u32, symbol: &str, attribute: u32, date: Option<i64>, q: &Quote) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (engine, _) = self.file.first_of("TSCE.CalculationEngineArchive").ok_or(err("no calculation engine"))?;
        let symbol = symbol.trim().to_uppercase();
        let mut spec = Msg::new("TSCE.RemoteDataSpecifierArchive");
        spec.set_u64("function_index", func as u64);
        spec.set_str("symbol", &symbol);
        spec.set_u64("attribute", attribute as u64);
        if let Some(d) = date {
            spec.set_f64("date", crate::formula::date_key(d) as f64);
        }
        let want = key(&spec);
        let val = cell_value(q);
        let put = move |store: &mut Msg| {
            let map = store.msg_mut_or_create("value_map");
            if let Some(e) = map.msgs_mut("entry").find(|e| e.msg("specifier").and_then(key) == want) {
                *e.msg_mut_or_create("value") = val;
            } else {
                let mut e = Msg::new("TSCE.RemoteDataValueMapArchive.RemoteDataMapEntry");
                e.push_msg("specifier", spec);
                e.push_msg("value", val);
                map.push_msg("entry", e);
            }
        };
        if let Some(store) = self.remote_store() {
            self.touch(h, store);
            put(self.file.obj_mut(store).ok_or(err("no store"))?);
        } else {
            let file = self.file.file_of(engine).ok_or(err("no calculation engine file"))?.to_string();
            self.structural(h, &[engine], |k| {
                let id = k.alloc_ids(1)[0];
                let mut store = Msg::new("TSCE.RemoteDataStoreArchive");
                store.msg_mut_or_create("value_map");
                put(&mut store);
                k.insert_archive(&file, Archive::new_object(id, store));
                if let Some(ce) = k.obj_mut(engine) {
                    ce.set_ref("remote_data_store", id);
                }
                k.refresh_refs(engine);
            });
        }
        let tables: Vec<ObjId> = self.file.objects_of("TST.TableInfoArchive").map(|(id, _)| id).collect();
        for t in tables {
            if !self.formula_cells(t).is_empty() {
                self.recalc_formulas(h, t);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::CellValue;
    use crate::DrawableKind;

    fn first_table(d: &Document) -> ObjId {
        let s = d.slides();
        d.scene(&s, 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id
    }

    fn doc() -> Document {
        Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/K-formula-0-before.key")).unwrap()
    }

    fn num(d: &Document, t: ObjId, r: u32, c: u32) -> Option<f64> {
        match d.cell_values(t).get(&(r, c)) {
            Some(CellValue::Number(x)) => Some(*x),
            _ => None,
        }
    }

    /// The attribute table has every name once, and the numbers N23 confirmed.
    #[test]
    fn attributes() {
        let mut names: Vec<_> = crate::formula::STOCK_ATTRIBUTES.iter().map(|(n, _)| *n).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), crate::formula::STOCK_ATTRIBUTES.len());
        assert_eq!(crate::formula::stock_attribute("Price"), Some(24));
        assert_eq!(crate::formula::stock_attribute(" change "), Some(7));
        assert_eq!(crate::formula::stock_attribute("name"), Some(18));
        assert_eq!(crate::formula::stock_attribute("close"), Some(23));
        assert_eq!(crate::formula::stock_attribute("colour"), None);
        // argument order: N23's popover, price first
        let args = crate::formula::STOCK_ARGS;
        assert_eq!(args.len(), 22, "the popover's 22 items");
        assert_eq!(args[0], (0, "Price", Some(24)));
        assert_eq!(args[21], (25, "Symbol", Some(26)));
        assert!(args.iter().all(|(c, _, _)| ![13, 14, 17, 18].contains(c)));
        assert_eq!(crate::formula::STOCKH_ARGS[1], (1, "Open", Some(20)));
        // 2026-10-16 is N23's STOCKH date key
        assert_eq!(crate::formula::date_key(20_742), 813_801_600);
    }

    /// STOCK / STOCKH read the stored quote; missing ones are errors; storing is undoable and
    /// survives a save.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn stock_quotes() {
        let mut d = doc();
        let t = first_table(&d);
        let mut h = History::default();
        assert!(d.remote_values().is_empty());
        d.begin(&mut h, "Formula");
        d.set_formula_text(&mut h, t, 4, 1, "=STOCK(\"aapl\",\"price\")").unwrap();
        d.set_formula_text(&mut h, t, 4, 2, "=STOCKH(\"AAPL\",\"close\",DATE(2026,10,16))").unwrap();
        d.end(&mut h);
        assert_eq!(num(&d, t, 4, 1), Some(0.0));
        let nodes = crate::formula::parse("=STOCK(\"aapl\",\"price\")", 4, 1).unwrap();
        assert!(matches!(crate::formula::eval_with(&nodes, 4, 1, &d.cell_values(t), &d.remote_values()), crate::formula::Value::Err(e) if e.contains("fetched")));

        d.begin(&mut h, "Stock Quote");
        d.set_remote_value(&mut h, 298, "AAPL", 24, None, &Quote::Number(343.46, Some("USD".into()))).unwrap();
        d.set_remote_value(&mut h, 299, "aapl", 23, Some(20_742), &Quote::Number(340.0, Some("USD".into()))).unwrap();
        d.set_remote_value(&mut h, 298, "AAPL", 18, None, &Quote::Text("Apple Inc.".into())).unwrap();
        d.end(&mut h);
        assert_eq!(num(&d, t, 4, 1), Some(343.46));
        assert_eq!(num(&d, t, 4, 2), Some(340.0));
        assert_eq!(d.remote_values().get(&(298, "AAPL".into(), 18, None)), Some(&Value::Text("Apple Inc.".into())));
        // Numbers' picker writes the attribute as a number (0 = price; STOCKH 0 = close)
        let v = |d: &Document, f: &str| crate::formula::eval_with(&crate::formula::parse(f, 4, 3).unwrap(), 4, 3, &d.cell_values(t), &d.remote_values());
        assert_eq!(v(&d, "=STOCK(\"AAPL\",0)"), Value::Num(343.46));
        assert_eq!(v(&d, "=STOCK(\"AAPL\",1)"), Value::Text("Apple Inc.".into()));
        assert_eq!(v(&d, "=STOCKH(\"AAPL\",0,DATE(2026,10,16))"), Value::Num(340.0));
        // codes with gaps (N24): 14 is unused, 16 is beta; STOCKH 1 is the open
        assert!(matches!(v(&d, "=STOCK(\"AAPL\",14)"), Value::Err(_)));
        assert!(matches!(v(&d, "=STOCK(\"AAPL\",99)"), Value::Err(_)));
        assert!(matches!(v(&d, "=STOCK(\"AAPL\",0.5)"), Value::Err(_)));
        assert!(matches!(v(&d, "=STOCK(\"AAPL\",16)"), Value::Err(e) if e.contains("fetched")), "beta: a known code, not fetched here");
        assert!(matches!(v(&d, "=STOCKH(\"AAPL\",5,DATE(2026,10,16))"), Value::Err(e) if !e.contains("fetched")));
        d.begin(&mut h, "Stock Quote");
        d.set_remote_value(&mut h, 298, "AAPL", 4, None, &Quote::Number(1.069, None)).unwrap();
        d.set_remote_value(&mut h, 299, "AAPL", 20, Some(20_742), &Quote::Number(346.09, Some("USD".into()))).unwrap();
        d.end(&mut h);
        assert_eq!(v(&d, "=STOCK(\"AAPL\",16)"), Value::Num(1.069));
        assert_eq!(v(&d, "=STOCKH(\"AAPL\",1,DATE(2026,10,16))"), Value::Num(346.09));
        assert!(d.undo(&mut h));
        // another date has no quote
        let other = crate::formula::parse("=STOCKH(\"AAPL\",\"close\",DATE(2026,10,15))", 4, 2).unwrap();
        assert!(matches!(crate::formula::eval_with(&other, 4, 2, &d.cell_values(t), &d.remote_values()), crate::formula::Value::Err(_)));

        // a refresh replaces the value in place
        d.begin(&mut h, "Refresh");
        d.set_remote_value(&mut h, 298, "AAPL", 24, None, &Quote::Number(350.0, Some("USD".into()))).unwrap();
        d.end(&mut h);
        assert_eq!(num(&d, t, 4, 1), Some(350.0));
        assert_eq!(d.remote_values().len(), 3);

        // save and reopen
        let r = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(r.remote_values(), d.remote_values());
        let (_, ce) = r.file.first_of("TSCE.CalculationEngineArchive").unwrap();
        assert!(ce.ref_id("remote_data_store").is_some());

        // undo
        assert!(d.undo(&mut h));
        assert_eq!(num(&d, t, 4, 1), Some(343.46));
        assert!(d.undo(&mut h));
        assert!(d.remote_values().is_empty());
        assert!(d.file.objects_of("TSCE.RemoteDataStoreArchive").next().is_none());
        assert_eq!(num(&d, t, 4, 1), Some(0.0));
        assert!(d.redo(&mut h));
        assert_eq!(num(&d, t, 4, 1), Some(343.46));
    }
}
