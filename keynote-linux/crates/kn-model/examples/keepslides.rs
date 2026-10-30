//! keepslides in.key out.key 3 44: a copy of the deck with only those slides (1-based), in the
//! deck's order (to record one transition into a chosen next slide).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let keep: Vec<usize> = a[3..].iter().map(|s| s.parse::<usize>().unwrap() - 1).collect();
    let n = d.slides().len();
    let drop: Vec<usize> = (0..n).filter(|i| !keep.contains(i)).collect();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "keep");
    d.delete_slides(&mut h, &drop).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("{} slides", d.slides().len());
}
