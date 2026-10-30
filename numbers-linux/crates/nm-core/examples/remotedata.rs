//! The remote data store (stock quotes) of a Numbers file. usage: remotedata file.numbers
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    for (id, m) in wb.file().objects_of("TSCE.RemoteDataStoreArchive") {
        println!("{:?}", m.msg("value_map").and_then(|v| v.msgs("entry").next()).and_then(|e| e.msg("value")).and_then(|v| v.msg("number_value")));
        println!("store {id} in {:?}, {} stocks", wb.file().file_of(id), m.msgs("stocks").count());
        for e in m.msg("value_map").into_iter().flat_map(|v| v.msgs("entry")) {
            let s = e.msg("specifier").unwrap();
            let v = e.msg("value").unwrap();
            let n = v.msg("number_value").or(v.msg("string_value"));
            let f = n.and_then(|n| n.msg("format"));
            println!(
                "  fn {:?} {:?} attr {:?} date {:?} type {:?} -> {:?}/{:?} fmt {:?} dp {:?} cur {:?} ncur {:?}",
                s.u32("function_index"), s.str("symbol"), s.u32("attribute"), s.f64("date"), v.u32("cell_value_type"),
                n.and_then(|n| n.f64("value")), n.and_then(|n| n.str("value")),
                f.and_then(|f| f.u32("format_type")), f.and_then(|f| f.u32("decimal_places")), f.and_then(|f| f.str("currency_code")), n.and_then(|n| n.str("currency_code"))
            );
        }
    }
    let mut r: Vec<_> = wb.doc.remote_values().into_iter().collect();
    r.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    for (k, v) in r {
        println!("{k:?} = {v:?}");
    }
}
