//! Groups in a Numbers file: each TSD.GroupArchive, its parent and children, its geometry
//! and its children's, and whether the sheets list the group / its children in drawable_infos.
//! usage: groups file.numbers
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let k = wb.file();
    let geom = |id| k.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.msg("geometry")).map(|g| format!("{g:?}"));
    for (id, m) in k.objects_of("TSD.GroupArchive") {
        let d = m.find_super("TSD.DrawableArchive");
        let kids = m.ref_ids("children");
        println!("group {id}: parent {:?} children {kids:?} file {:?}", d.and_then(|d| d.ref_id("parent")), k.file_of(id));
        println!("  geometry {:?}", geom(id));
        for c in &kids {
            println!("  child {c} {} parent {:?} geometry {:?}", k.type_name(*c), k.obj(*c).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|d| d.ref_id("parent")), geom(*c));
        }
        for (s, sm) in k.objects_of("TN.SheetArchive") {
            let infos = sm.ref_ids("drawable_infos");
            if infos.contains(&id) || kids.iter().any(|c| infos.contains(c)) {
                println!("  sheet {s} drawable_infos: group {} children {:?}", infos.contains(&id), kids.iter().map(|c| infos.contains(c)).collect::<Vec<_>>());
            }
        }
    }
}
