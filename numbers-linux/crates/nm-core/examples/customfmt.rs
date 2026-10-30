//! The document's custom format list. usage: customfmt file.numbers
fn main() {
    let wb = nm_core::Workbook::open(std::env::args().nth(1).unwrap()).unwrap();
    let k = wb.file();
    let (_, d) = k.first_of("TN.DocumentArchive").unwrap();
    let sup = d.find_super("TSA.DocumentArchive");
    println!("tables_custom_format_list {:?}", sup.and_then(|s| s.ref_id("tables_custom_format_list")).map(|i| (i, k.type_name(i))));
    let id = d.ref_id("custom_format_list").or_else(|| sup.and_then(|s| s.ref_id("custom_format_list"))).or_else(|| sup.and_then(|s| s.ref_id("tables_custom_format_list")));
    println!("custom_format_list ref {id:?} type {:?}", id.map(|i| k.type_name(i)));
    if let Some(m) = id.and_then(|i| k.obj(i)) {
        println!("{}", format!("{m:?}").chars().take(1500).collect::<String>());
    }
}
