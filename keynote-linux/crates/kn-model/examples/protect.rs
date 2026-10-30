//! Save a copy of a deck with a password: `protect in.key out.key password hint`
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    d.password = Some((a[3].clone(), a[4].clone()));
    d.save_as(&a[2]).unwrap();
    println!("saved {}", a[2]);
}
