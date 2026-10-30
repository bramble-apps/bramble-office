//! cargo run -p pg-core --example rtcheck -- outdir file.pages|file.docx … : Word round trips.
//! A .pages: exported to Word, read back, compared. A .docx: read, written again, read again,
//! compared with the first reading. Prints what differs.
use pg_core::Pages;

struct Sum {
    text: String,
    pages: usize,
    setup: String,
    hf: Vec<String>,
    tables: usize,
    images: usize,
    floating: usize,
    bg: String,
}

fn sum(p: &Pages, r: &mut kn_render::Renderer) -> Sum {
    let mut cache = pg_core::flow::Cache::default();
    let l = pg_core::flow::layout(p, &mut r.fonts, &mut cache, &Default::default());
    let s = &l.setup;
    let k = p.file();
    let mut hf = Vec::new();
    if let Some(pg) = l.pages.first() {
        for h in pg.headers.iter().chain(&pg.footers) {
            let t = pg_core::text_plain(&p.doc.storage_text(h.storage));
            if !t.trim().is_empty() {
                hf.push(format!("{}{}:{}", if h.footer { "F" } else { "H" }, h.index, t.trim()));
            }
        }
    }
    let body = p.body_storage().map(|b| pg_core::text_plain(&p.doc.storage_text(b))).unwrap_or_default();
    // the drawables people see: pictures in the text and on the pages
    // (a table continued over pages counts once)
    let mut shown: Vec<_> = l.pages.iter().flat_map(|pg| pg.objects.iter().map(|o| o.id)).collect();
    shown.sort();
    shown.dedup();
    Sum {
        text: body.split_whitespace().collect::<Vec<_>>().join(" "),
        pages: l.pages.len(),
        setup: format!("{:.0}x{:.0} m{:.0}/{:.0}/{:.0}/{:.0} hf{}{}", s.width, s.height, s.top, s.right, s.bottom, s.left, s.headers as u8, s.footers as u8),
        hf,
        tables: shown.iter().filter(|i| k.type_name(**i) == "TST.TableInfoArchive").count(),
        images: shown.iter().filter(|i| matches!(k.type_name(**i), "TSD.ImageArchive")).count(),
        floating: p.floating().len(),
        bg: format!("{:?}", p.sections().first().and_then(|s| s.background.clone()).map(|b| match b {
            kn_model::Fill::Color(_) => "colour",
            kn_model::Fill::Image { .. } => "image",
            _ => "other",
        })),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let out = std::path::PathBuf::from(&a[1]);
    std::fs::create_dir_all(&out).unwrap();
    let mut r = kn_render::Renderer::new();
    for f in &a[2..] {
        let path = std::path::Path::new(f);
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let first = if path.extension().is_some_and(|e| e == "docx") { pg_core::import::import_file(path) } else { Pages::open(path) };
        let Ok(mut first) = first else {
            println!("{name}: OPEN FAILED");
            continue;
        };
        let docx = out.join(format!("{name}.rt.docx"));
        if let Err(e) = pg_core::wordmode::save_docx(&mut first, &docx, false) {
            println!("{name}: EXPORT FAILED {e}");
            continue;
        }
        let back = match pg_core::import::import_file(&docx) {
            Ok(b) => b,
            Err(e) => {
                println!("{name}: REIMPORT FAILED {e}");
                continue;
            }
        };
        let (s1, s2) = (sum(&first, &mut r), sum(&back, &mut r));
        let mut diffs = Vec::new();
        if s1.text != s2.text {
            let common = s1.text.chars().zip(s2.text.chars()).take_while(|(x, y)| x == y).count();
            diffs.push(format!("text differs at {common}: {:?} vs {:?}", s1.text.chars().skip(common).take(40).collect::<String>(), s2.text.chars().skip(common).take(40).collect::<String>()));
        }
        if s1.pages != s2.pages {
            diffs.push(format!("pages {} -> {}", s1.pages, s2.pages));
        }
        if s1.setup != s2.setup {
            diffs.push(format!("setup {} -> {}", s1.setup, s2.setup));
        }
        if s1.hf != s2.hf {
            diffs.push(format!("headers {:?} -> {:?}", s1.hf, s2.hf));
        }
        if s1.tables != s2.tables || s1.images != s2.images {
            diffs.push(format!("tables {}->{} pictures {}->{}", s1.tables, s2.tables, s1.images, s2.images));
        }
        if s1.floating != s2.floating {
            diffs.push(format!("floating {}->{}", s1.floating, s2.floating));
        }
        if s1.bg != s2.bg {
            diffs.push(format!("background {} -> {}", s1.bg, s2.bg));
        }
        println!("{name}: {} pages, {} chars{}", s1.pages, s1.text.len(), if diffs.is_empty() { " — same".to_string() } else { format!("\n    {}", diffs.join("\n    ")) });
    }
}
