//! cells <file.key> [slide]: the first table's cells as shown (row, column, text).
use kn_model::{Document, DrawableKind};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = Document::open(&a[1]).unwrap();
    let i: usize = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1) - 1;
    let sc = d.scene(&d.slides(), i).unwrap();
    let Some(t) = sc.drawables.iter().find_map(|x| if let DrawableKind::Table(t) = &x.kind { Some(t) } else { None }) else { return };
    for r in 0..t.rows {
        for c in 0..t.cols {
            let s = &t.cells[(r * t.cols + c) as usize];
            if !s.is_empty() {
                println!("r{r} c{c}: {s:?}");
            }
        }
    }
}
