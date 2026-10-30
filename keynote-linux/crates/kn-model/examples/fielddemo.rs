//! fielddemo <in.key> <out.key> <text object id> <prefix>: type text in front of a text that
//! starts with a field (checks).
use kn_model::edit::History;
use kn_model::{Document, DrawableKind};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut d = Document::open(&a[1]).unwrap();
    let id: u64 = a[3].parse().unwrap();
    let s = d.slides();
    let st = (0..s.len())
        .find_map(|i| d.scene(&s, i)?.drawables.into_iter().find(|x| x.id == id).and_then(|x| match x.kind {
            DrawableKind::Shape(sh) => sh.text.map(|t| t.storage),
            _ => None,
        }))
        .unwrap();
    let mut h = History::default();
    d.begin(&mut h, "Typing");
    d.replace_text(&mut h, st, 0, 0, &a[4]);
    d.end(&mut h);
    d.save_as(&a[2]).unwrap();
}
