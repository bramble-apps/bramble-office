//! The drawables on sheet n (id, type, frame). usage: drawables file.numbers sheet
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let n: usize = a[2].parse().unwrap();
    let s = wb.sheets().remove(n - 1);
    for id in wb.file().obj(s.id).unwrap().ref_ids("drawable_infos") {
        let g = wb.file().obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry);
        println!("{id} {} {:?}", wb.file().type_name(id), g.map(|g| (g.x, g.y, g.w, g.h)));
    }
}
