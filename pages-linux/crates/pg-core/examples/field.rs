//! cargo run -p pg-core --example field -- file.pages object id : the top-level fields of `object` that refer to `id`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let (obj, id): (u64, u64) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let m = p.file().obj(obj).unwrap();
    for f in &m.fields {
        let s = format!("{:?}", f);
        if s.contains(&format!("Varint({id})")) {
            println!("field {}: {}", f.num, &s[..s.len().min(600)]);
        }
    }
}
