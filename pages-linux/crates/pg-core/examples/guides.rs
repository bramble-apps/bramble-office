//! cargo run -p pg-core --example guides -- file.pages… : user guides stored in each file
fn main() {
    for f in std::env::args().skip(1) {
        let Ok(p) = pg_core::Pages::open(&f) else { continue };
        let k = p.file();
        for (sid, s) in k.objects_of("TP.SectionArchive") {
            let Some(map) = s.ref_id("user_defined_guide_storage").and_then(|m| k.obj(m)) else { continue };
            for e in map.msgs("user_defined_guide_storages") {
                let gs = e.ref_id("guide_storage").and_then(|g| k.obj(g));
                let n: Vec<String> = gs.map(|g| g.msgs("userDefinedGuides").map(|u| format!("{:?} {:?}", u.enum_name("type"), u.f32("position"))).collect()).unwrap_or_default();
                println!("{f}: section {sid} ({}) page_index {:?}: {n:?}", map.type_name(), e.u64("page_index"));
            }
        }
        for (tid, t) in k.objects_of("TP.PageTemplateArchive") {
            if let Some(g) = t.ref_id("guide_storage").and_then(|g| k.obj(g)) {
                let n = g.count("userDefinedGuides");
                if n > 0 {
                    println!("{f}: template {tid} {:?}: {n} guides", t.str("name"));
                }
            }
        }
    }
}
