//! cargo run -p pg-core --example runsizes -- file.pages : first paragraphs' run and empty sizes
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let b = p.body_storage().unwrap();
    let body = pg_core::body::build(p.file(), b, None, Default::default()).unwrap();
    for (i, para) in body.tb.paragraphs.iter().enumerate().take(12) {
        let sizes: Vec<(f32, String)> = para.runs.iter().map(|r| (r.style.size, r.style.font_name.clone())).collect();
        println!("para {i} {:?} sizes {:?} spacing {:?} before {} after {}", &body.tb.text[para.start..para.end.min(para.start + 14)], sizes, para.line_spacing, para.space_before, para.space_after);
    }
}
