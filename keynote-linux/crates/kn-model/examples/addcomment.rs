//! Add a comment to the first object of slide N (Mac test file).
//! cargo run -p kn-model --example addcomment -- in.key out.key <slide> "text" "author"
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let n: usize = a[3].parse().unwrap();
    let obj = d.scene(&s, n - 1).unwrap().drawables.into_iter().find(|x| !x.from_template).unwrap().id;
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "Add Comment");
    d.add_comment(&mut h, obj, &a[4], &a[5]).unwrap();
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
