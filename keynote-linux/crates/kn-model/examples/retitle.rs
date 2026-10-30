//! retitle theme.key out.key "Old Name" "New Name": in every slide's text, the theme's old name
//! (any capitalisation) becomes the new one and "Keynote theme" becomes "Keydeck theme".
use kn_model::{Drawable, DrawableKind};
fn walk(v: &[Drawable], out: &mut Vec<(u64, String)>) {
    for d in v {
        match &d.kind {
            DrawableKind::Shape(s) => {
                if let Some(t) = &s.text {
                    out.push((t.storage, t.text.clone()));
                }
            }
            DrawableKind::Group(c) => walk(c, out),
            _ => {}
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let (old, new) = (&a[3], &a[4]);
    let s = d.slides();
    let mut texts = Vec::new();
    for i in 0..s.len() {
        if let Some(sc) = d.scene(&s, i) {
            walk(&sc.drawables, &mut texts);
        }
    }
    texts.sort();
    texts.dedup();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "Retitle");
    let mut n = 0;
    for (st, t) in texts {
        let mut u = t.replace("Keynote theme", "Keydeck theme").replace("KEYNOTE THEME", "KEYDECK THEME");
        for (o, nw) in [(old.clone(), new.clone()), (old.to_uppercase(), new.to_uppercase())] {
            u = u.replace(&o, &nw);
        }
        if u != t {
            d.set_storage_text(&mut h, st, &u);
            n += 1;
        }
    }
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
    println!("{n} texts");
}
