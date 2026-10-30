//! cargo run -p pg-core --example checkall -- list.txt outdir : open, lay out, render page 1,
//! save a copy into outdir and reopen it, for every .pages path in list.txt (one per line).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let list = std::fs::read_to_string(&a[1]).unwrap();
    let out = std::path::PathBuf::from(&a[2]);
    std::fs::create_dir_all(&out).unwrap();
    let mut r = kn_render::Renderer::new();
    for (n, line) in list.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let t = std::time::Instant::now();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<String, String> {
            let mut p = pg_core::Pages::open(line).map_err(|e| format!("OPEN FAILED: {e}"))?;
            let mut cache = pg_core::flow::Cache::default();
            let l = pg_core::flow::layout(&p, &mut r.fonts, &mut cache, &Default::default());
            if let Some(pm) = pg_core::render::render_page(&p, &l, 0, &mut r, 0.3, Default::default()) {
                let _ = pm.save_png(out.join(format!("{n:03}.png")));
            }
            let mode = if p.setup().body { "WP" } else { "layout" };
            let objs: usize = l.pages.iter().map(|pg| pg.objects.len()).sum();
            let copy = out.join(format!("{n:03}.pages"));
            if p.doc.password.is_none() {
                p.save_as(&copy).map_err(|e| format!("SAVE FAILED: {e}"))?;
                let back = pg_core::Pages::open(&copy).map_err(|e| format!("REOPEN FAILED: {e}"))?;
                let (o, d) = pg_core::gc::orphans(back.file());
                if back.body_text() != p.body_text() {
                    return Err("TEXT CHANGED after save".into());
                }
                return Ok(format!("{mode} {} pages, {objs} objects, saved: {} orphans {} unused data", l.pages.len(), o.len(), d.len()));
            }
            Ok(format!("{mode} {} pages (password)", l.pages.len()))
        }));
        let msg = match res {
            Ok(Ok(m)) => format!("ok   {m}"),
            Ok(Err(e)) => format!("FAIL {e}"),
            Err(_) => "FAIL PANIC".to_string(),
        };
        println!("{n:03} {:>5.1}s {msg} | {}", t.elapsed().as_secs_f32(), line.rsplit('/').next().unwrap_or(line));
    }
}
