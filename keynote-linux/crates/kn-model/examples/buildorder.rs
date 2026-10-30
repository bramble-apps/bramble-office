//! buildorder <file.key> <slide>: the slide's build order (checks).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let sc = d.scene(&s, i).unwrap();
    for c in &sc.build_order {
        println!("{c:?}");
    }
    for b in &sc.builds {
        println!("build {} on {} {} {} dur {} delay? {:?}", b.id, b.drawable, b.kind, b.effect, b.duration, b.params);
    }
}
