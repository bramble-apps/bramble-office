fn main() {
    use kn_model::DrawableKind;
    let mut d = kn_model::Document::open("testdata/ref/R13-merge-0-before.key").unwrap();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "dup");
    d.duplicate_slide(&mut h, 0).unwrap();
    d.end(&mut h);
    let s = d.slides();
    for i in 0..s.len() {
        let sc = d.scene(&s, i).unwrap();
        let t: Vec<u64> = sc.drawables.iter().filter(|x| matches!(x.kind, DrawableKind::Table(_))).map(|x| x.id).collect();
        println!("slide {i} tables {t:?}");
    }
}
