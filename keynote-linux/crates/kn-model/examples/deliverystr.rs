//! deliverystr <file.key>: print each slide's build delivery strings (checks).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    for s in d.slides() {
        for b in d.file.obj(s.slide).unwrap().ref_ids("builds") {
            println!("{:?}", d.file.obj(b).unwrap().str("delivery"));
        }
    }
}
