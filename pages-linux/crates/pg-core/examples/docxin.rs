//! cargo run -p pg-core --example docxin -- file.docx… : what a Word file becomes in Letters
//! (opened read only; nothing is written)
fn main() {
    let mut r = kn_render::Renderer::new();
    for f in std::env::args().skip(1) {
        let p = match pg_core::import::import_file(std::path::Path::new(&f)) {
            Ok(p) => p,
            Err(e) => {
                println!("{f}: FAILED {e}");
                continue;
            }
        };
        let mut cache = pg_core::flow::Cache::default();
        let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
        let s = &l.setup;
        let k = p.file();
        let tables = k.objects_of("TST.TableInfoArchive").count();
        let images = k.objects_of("TSD.ImageArchive").count();
        let floating = p.floating().len();
        let body = p.body_storage().map(|b| p.doc.storage_text(b)).unwrap_or_default();
        let name = std::path::Path::new(&f).file_name().unwrap().to_string_lossy().to_string();
        println!("{name}: pages {} | {}x{} margins {}/{}/{}/{} | chars {} | tables {tables} images {images} (floating {floating}) | bg {:?}", l.pages.len(), s.width, s.height, s.top, s.right, s.bottom, s.left, body.chars().count(), p.sections().first().and_then(|x| x.background.clone()).map(|_| "yes"));
        if let Some(pg) = l.pages.first() {
            for hf in pg.headers.iter().chain(&pg.footers) {
                let t = p.doc.storage_text(hf.storage);
                if !t.trim().is_empty() {
                    println!("   {} {}: {:?}", if hf.footer { "footer" } else { "header" }, hf.index, t.chars().take(60).collect::<String>());
                }
            }
        }
    }
}
