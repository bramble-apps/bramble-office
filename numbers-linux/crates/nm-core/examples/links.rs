//! Every drawable's hyperlink. usage: links file.numbers
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    for t in ["TSWP.ShapeInfoArchive", "TSD.ImageArchive", "TSD.GroupArchive"] {
        for (id, m) in wb.file().objects_of(t) {
            if let Some(u) = m.find_super("TSD.DrawableArchive").and_then(|d| d.str("hyperlink_url")) {
                println!("{id} {t}: {u}");
            }
        }
    }
}
