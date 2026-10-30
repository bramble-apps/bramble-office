//! cargo run -p pg-core --example fingerprint -- a.pages b.pages : opens each (read only), lays
//! it out and prints pages, objects, body length and archive count, to compare two copies.
fn main() {
    let mut fonts = kn_render::Renderer::new();
    for f in std::env::args().skip(1) {
        match pg_core::Pages::open(&f) {
            Ok(p) => {
                let mut cache = pg_core::flow::Cache::default();
                let l = pg_core::flow::layout(&p, &mut fonts.fonts, &mut cache, &Default::default());
                let objs: usize = l.pages.iter().map(|pg| pg.objects.len()).sum();
                let body = p.body_storage().map(|b| p.doc.storage_text(b).len()).unwrap_or(0);
                let n = p.file().archives().count();
                let data = p.file().entries.iter().filter(|e| e.name.starts_with("Data/")).count();
                println!("pages {} objects {objs} body {body} archives {n} data {data}", l.pages.len());
            }
            Err(e) => println!("OPEN FAILED: {e}"),
        }
    }
}
