//! `retheme deck.key theme.key out.key`: File > Change Theme, headless.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let t0 = std::time::Instant::now();
    let (mut nd, problems) = d.with_theme(kn_format::KeyFile::open(&a[2]).unwrap()).unwrap();
    println!("{} -> {} slides in {:?}; problems {:?}", d.slides().len(), nd.slides().len(), t0.elapsed(), problems);
    nd.save_as(&a[3]).unwrap();
    let back = kn_model::Document::open(&a[3]).unwrap();
    let s = back.slides();
    let ok = (0..s.len()).filter(|i| back.scene(&s, *i).is_some()).count();
    println!("reopened: {ok}/{} scenes", s.len());
}
