//! cargo run -p pg-core --example redact -- in.pages out.pages [password] : every annotation author named "Test Author"
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut p = match a.get(3) {
        Some(pw) => pg_core::Pages::open_with_password(&a[1], pw, "").unwrap(),
        None => pg_core::Pages::open(&a[1]).unwrap(),
    };
    let ids: Vec<u64> = p.file().archives().filter(|(_, ar)| ar.obj().type_name() == "TSK.AnnotationAuthorArchive").map(|(_, ar)| ar.id()).collect();
    for id in &ids {
        if let Some(m) = p.doc.file.obj_mut(*id) {
            println!("{id}: {:?} -> Test Author", m.str("name"));
            m.set_str("name", "Test Author");
        }
    }
    p.save_as(&a[2]).unwrap();
}
