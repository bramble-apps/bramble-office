//! Raw cell record and the table's format list. usage: cellraw file.numbers table row col
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let k = wb.file();
    let t: u64 = a[2].parse().unwrap();
    let (r, c): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let model = k.obj(t).and_then(|m| m.ref_id("tableModel")).and_then(|m| k.obj(m)).unwrap();
    let rec = nm_core::kn_model::table::raw_cell_records(k, model).into_iter().find(|(rc, _)| *rc == (r, c));
    println!("{:02x?}", rec.map(|x| x.1));
    let ds = model.msg("base_data_store").unwrap();
    for name in ["format_table", "custom_format_table"] {
        if let Some(f) = ds.ref_id(name).and_then(|f| k.obj(f)) {
            for e in f.msgs("entries").take(40) {
                println!("{name} key {:?}: {:?}", e.u32("key"), e.msg("format").or(e.msg("custom_format")).map(|m| format!("{m:?}").chars().take(300).collect::<String>()));
            }
        }
    }
}
