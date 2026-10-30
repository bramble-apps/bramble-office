//! cargo run -p pg-core --example hfalign -- files... : each header/footer field's text and paragraph alignment
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(p) = pg_core::Pages::open(&path) else { continue };
        let k = p.file();
        let tag: String = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().chars().take(3).collect();
        for s in p.sections() {
            for t in [s.first, s.odd, s.even].into_iter().flatten() {
                for footer in [false, true] {
                    for (i, sid) in p.header_footer_storages(t, footer).iter().enumerate() {
                        let text = p.doc.storage_text(*sid);
                        if text.trim().is_empty() {
                            continue;
                        }
                        let Some(b) = pg_core::body::build(k, *sid, None, Default::default()) else { continue };
                        let al: Vec<String> = b.tb.paragraphs.iter().map(|q| format!("{:?}", q.align)).collect();
                        println!("{tag} {} field {i} align {:?} text {:?}", if footer { "footer" } else { "header" }, al, text.chars().take(30).collect::<String>().len());
                    }
                }
            }
        }
    }
}
