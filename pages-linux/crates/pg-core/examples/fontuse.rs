//! cargo run -p pg-core --example fontuse -- file.pages… : body text per font (characters), and
//! whether each font is installed
fn main() {
    let mut fonts = kn_render::Renderer::new().fonts;
    let mut use_: std::collections::HashMap<String, usize> = Default::default();
    for f in std::env::args().skip(1) {
        let Ok(p) = pg_core::Pages::open(&f) else { continue };
        let Some(b) = p.body_storage().and_then(|s| pg_core::body::build(p.file(), s, None, Default::default())) else { continue };
        for para in &b.tb.paragraphs {
            for r in &para.runs {
                *use_.entry(r.style.font_name.clone()).or_default() += r.end.saturating_sub(r.start);
            }
        }
    }
    let mut v: Vec<_> = use_.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    for (name, n) in v.iter().take(40) {
        println!("{n:8}  {name}  {}", if fonts.is_installed(name) { "installed" } else { "MISSING" });
    }
}
