//! cargo run -p pg-core --example tmodel -- file.pages table_id : the table model's merge-related structure (no cell text)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    let t = k.obj(a[2].parse().unwrap()).unwrap();
    let model = t.ref_id("tableModel").and_then(|id| k.obj(id)).unwrap();
    println!("model type {}", model.type_name());
    let dbg = format!("{:#?}", model);
    for line in dbg.lines() {
        let l = line.trim();
        if l.contains("merge") || l.contains("Merge") {
            println!("{}", &line[..line.len().min(160)]);
        }
    }
    if let Some(mo) = model.msg("merge_owner") {
        println!("merge_owner: {}", &format!("{:?}", mo)[..format!("{:?}", mo).len().min(1500)]);
    }
    let ds = model.msg("base_data_store");
    println!("data store merge_region_map {:?}", ds.and_then(|d| d.ref_id("merge_region_map")));
}
