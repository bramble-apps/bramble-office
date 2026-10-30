//! cargo run -p pg-core --example cline -- file.pages : the connection lines and how they draw
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    for (id, _) in k.objects_of("TSD.ConnectionLineArchive") {
        let d = pg_core::kn_model::scene::drawable(k, id, Default::default());
        println!("{id}: page {:?} {:#?}", p.floating_page(id), d.map(|d| (d.geom, d.kind)));
    }
}
