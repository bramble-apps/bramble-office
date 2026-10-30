//! opentime deck.key: how long Document::open takes (remote files go through the local cache).
fn main() {
    let p = std::env::args().nth(1).unwrap();
    println!("remote: {}", kn_format::keyfile::is_remote(std::path::Path::new(&p)));
    let t = std::time::Instant::now();
    let d = kn_model::Document::open(&p).unwrap();
    println!("open {:?}", t.elapsed());
    let s = d.slides();
    let t = std::time::Instant::now();
    let n = (0..s.len()).filter_map(|i| d.scene(&s, i)).count();
    println!("{n} scenes {:?}", t.elapsed());
}
