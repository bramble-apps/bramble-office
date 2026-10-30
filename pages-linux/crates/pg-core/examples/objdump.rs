//! cargo run -p pg-core --example objdump -- file.pages id : one object, as decoded
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    let k = p.file();
    let m = k.obj(id).unwrap();
    println!("{} {:#?}", m.type_name(), m);
}
