//! cargo run -p pg-core --example dropcaps -- file.pages : the body's drop cap style table (no text)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    let st = k.obj(p.body_storage().unwrap()).unwrap();
    let Some(t) = st.msg("table_drop_cap_style") else { println!("no table"); return };
    for e in t.msgs("entries") {
        let i = e.u64("character_index").unwrap_or(0);
        let o = e.ref_id("object");
        let desc = o.and_then(|id| k.obj(id)).map(|m| {
            let s = format!("{:?}", m.msg("drop_cap_properties"));
            format!("{} name {:?} parent {:?} props {}", m.type_name(), m.msg("super").and_then(|s| s.str("name")), m.msg("super").and_then(|s| s.ref_id("parent")), &s[..s.len().min(300)])
        });
        println!("at {i}: {o:?} {desc:?}");
        if std::env::var_os("FULL").is_some() && o.is_some() {
            if let Some(m) = o.and_then(|id| k.obj(id)) {
                println!("  drop_cap {:?}", m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")));
                println!("  fill {:?}", m.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap_shape_fill")));
                println!("  char {:?}", m.msg("char_properties"));
                let mut par = m.msg("super").and_then(|s| s.ref_id("parent"));
                while let Some(pid) = par { let pm = k.obj(pid).unwrap(); println!("  parent {pid} {} dc {:?} fill {:?} char {:?}", pm.type_name(), pm.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap")), pm.msg("drop_cap_properties").and_then(|p| p.msg("drop_cap_shape_fill")), pm.msg("char_properties").map(|c| format!("{c:?}").chars().take(400).collect::<String>())); par = pm.msg("super").and_then(|s| s.ref_id("parent")); }
            }
            break;
        }
    }
}
