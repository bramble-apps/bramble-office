//! viewstate file.key: the archive types in Index/ViewState.iwa and the objects each refers to
//! (to see which saved selections could point at deleted objects).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for (file, ar) in k.archives() {
        if !file.contains("ViewState") {
            continue;
        }
        let id = ar.id();
        let mut ids = Vec::new();
        if let Some(m) = k.obj(id) {
            m.collect_refs(&mut ids);
        }
        let refs: Vec<String> = ids.into_iter().map(|r| format!("{r}:{}", k.type_name(r))).collect();
        println!("{id} {} -> {}", k.type_name(id), refs.join(" "));
    }
}
