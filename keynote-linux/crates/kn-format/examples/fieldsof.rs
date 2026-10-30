//! fieldsof name-part: fields (in any type) whose name contains name-part, with their type.
fn main() {
    let want = std::env::args().nth(1).unwrap();
    let s = kn_format::schema::schema();
    for t in &s.types {
        for d in t.fields.values() {
            if d.name.contains(&want) {
                let ty = d.type_idx.map(|i| s.types[i as usize].name.as_str()).unwrap_or("");
                println!("{} . {} ({}) {}", t.name, d.name, d.num, ty);
            }
        }
    }
}
