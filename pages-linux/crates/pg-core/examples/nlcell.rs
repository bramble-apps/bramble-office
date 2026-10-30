//! cargo run -p pg-core --example nlcell -- in.pages : write "Hello\nWorld" into the first table's B2 and print the body before / after
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut p = pg_core::Pages::open(&a[1]).unwrap();
    let mut h = kn_model::edit::History::default();
    let body = p.body_storage().unwrap();
    println!("before {:?}", p.doc.storage_text(body));
    let t = p.doc.file.objects_of("TST.TableInfoArchive").map(|x| x.0).next().unwrap();
    p.doc.begin(&mut h, "x");
    let r = p.doc.set_cell(&mut h, t, 1, 1, &kn_model::tablecells::CellInput::parse(&a.get(2).cloned().unwrap_or("Hello\nWorld".into())));
    p.doc.end(&mut h);
    println!("set {:?}", r.is_ok());
    println!("after  {:?}", p.doc.storage_text(body));
}
