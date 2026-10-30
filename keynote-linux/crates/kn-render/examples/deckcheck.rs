//! Health check of one deck, for batch runs over real presentations:
//! open (lazy), every .iwa re-encodes identically, every slide's scene builds and renders
//! (small) without panicking, and a no-op save reopens with the same slide count.
//! Prints one line: OK/FAIL and the problems. `deckcheck deck.key [png_dir]`
use kn_format::{EntryData, KeyFile};
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let out_dir = std::env::args().nth(2);
    let t0 = Instant::now();
    let mut problems: Vec<String> = Vec::new();
    let k = match KeyFile::open(&path) {
        Ok(k) => k,
        Err(e) => {
            println!("FAIL open: {e}");
            return;
        }
    };
    // lossless re-encode of every .iwa
    let mut z = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let mut differ = 0;
    for e in &k.entries {
        if let EntryData::Iwa(iwa) = &e.data {
            let Ok(mut f) = z.by_name(&e.name) else { continue };
            let mut orig = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut orig).unwrap();
            if kn_format::wire::iwa_unframe(&orig).map(|o| o != iwa.encode_stream()).unwrap_or(true) {
                differ += 1;
            }
        }
    }
    if differ > 0 {
        problems.push(format!("{differ} iwa re-encode differently"));
    }
    let doc = kn_model::Document::from_file(k);
    let slides = doc.slides();
    let mut r = kn_render::Renderer::new();
    let mut slow = (0usize, 0f32);
    let mut panics = 0;
    for i in 0..slides.len() {
        let t = Instant::now();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let sc = doc.scene(&slides, i)?;
            r.render_scene(&doc, &sc, 480)
        }));
        match res {
            Ok(Some(pm)) => {
                if let Some(d) = &out_dir {
                    let _ = std::fs::create_dir_all(d);
                    let _ = pm.save_png(format!("{d}/slide-{:03}.png", i + 1));
                }
            }
            Ok(None) => problems.push(format!("slide {} has no scene", i + 1)),
            Err(_) => {
                panics += 1;
                problems.push(format!("slide {} PANICKED", i + 1));
            }
        }
        let s = t.elapsed().as_secs_f32();
        if s > slow.1 {
            slow = (i + 1, s);
        }
    }
    // save (no edits) and reopen
    let tmp = std::env::temp_dir().join(format!("deckcheck-{}.key", std::process::id()));
    match doc.file.save(&tmp).map_err(|e| e.to_string()).and_then(|_| kn_model::Document::open(&tmp).map_err(|e| e.to_string())) {
        Ok(d2) if d2.slides().len() == slides.len() => {}
        Ok(d2) => problems.push(format!("saved copy has {} slides, not {}", d2.slides().len(), slides.len())),
        Err(e) => problems.push(format!("save/reopen failed: {e}")),
    }
    let _ = std::fs::remove_file(&tmp);
    println!(
        "{} {} slides in {:.1}s (slowest slide {} {:.2}s){}{}",
        if problems.is_empty() { "OK  " } else { "FAIL" },
        slides.len(),
        t0.elapsed().as_secs_f32(),
        slow.0,
        slow.1,
        if panics > 0 { " " } else { "" },
        if problems.is_empty() { String::new() } else { format!(" — {}", problems.join("; ")) }
    );
}
