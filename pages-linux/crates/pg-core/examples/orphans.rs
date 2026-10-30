//! cargo run -p pg-core --example orphans -- file.pages… : unreachable objects and unused data (the Mac's roots)
fn main() {
    for f in std::env::args().skip(1) {
        let p = pg_core::Pages::open(&f).unwrap();
        let (o, d) = pg_core::gc::orphans(p.file());
        let n = p.file().archives().count();
        println!("{f}: {n} objects, {} unreachable, {} unused data {:?}", o.len(), d.len(), o.iter().take(5).map(|i| p.file().type_name(*i)).collect::<Vec<_>>());
    }
}
