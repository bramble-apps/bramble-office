//! How long each note takes to open (`bench_notes <library dir>`; timings and kinds only).
use std::time::Instant;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("library dir"));
    let st = nt_core::store::Store::open(&dir).unwrap();
    let meta = st.library_meta().unwrap();
    let mut t: Vec<(f64, String)> = Vec::new();
    for n in &meta.notes {
        let s = Instant::now();
        let full = st.note(&n.id).unwrap();
        let ms = s.elapsed().as_secs_f64() * 1e3;
        let mut kinds: Vec<String> = full.attachments.iter().map(|a| format!("{:?}", a.kind)).collect();
        kinds.sort();
        kinds.dedup();
        t.push((ms, kinds.join("+")));
    }
    t.sort_by(|a, b| b.0.total_cmp(&a.0));
    let over = |x: f64| t.iter().filter(|v| v.0 > x).count();
    println!("{} notes: median {:.2} ms, >16 ms {}, >50 ms {}, >100 ms {}", t.len(), t[t.len() / 2].0, over(16.0), over(50.0), over(100.0));
    for (ms, k) in t.iter().take(8) {
        println!("  {ms:.1} ms  [{k}]");
    }
    // the slowest ones again, by part: their body alone
    let db = if dir.join("index/NoteStore.sqlite").exists() { dir.join("index/NoteStore.sqlite") } else { dir.join("NoteStore.sqlite") };
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut q = c.prepare("SELECT d.ZDATA FROM ZICNOTEDATA d ORDER BY LENGTH(d.ZDATA) DESC LIMIT 3").unwrap();
    for data in q.query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap().flatten() {
        let s = Instant::now();
        let b = nt_core::body::decode_note(&data).unwrap();
        let d = s.elapsed().as_secs_f64() * 1e3;
        let s = Instant::now();
        let _ = nt_core::body::encode_note(&b);
        println!("  body {} bytes: decode {d:.1} ms, encode {:.1} ms, {} paragraphs", data.len(), s.elapsed().as_secs_f64() * 1e3, b.paragraphs.len());
    }
}
