//! The first note read after opening, against the same note read later (`bench_first <library dir>`;
//! timings only).
use std::time::Instant;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("library dir"));
    let t = Instant::now();
    let st = nt_core::store::Store::open(&dir).unwrap();
    let meta = st.library_meta().unwrap();
    println!("open + library_meta {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    let ids: Vec<String> = meta.notes.iter().map(|n| n.id.clone()).collect();
    let order: Vec<usize> = match std::env::args().nth(2).as_deref() {
        Some("reverse") => (0..4).map(|i| ids.len() - 1 - i).chain([0]).collect(),
        _ => vec![0, 1, 2, ids.len() - 1, 0],
    };
    for i in order {
        let t = Instant::now();
        let _ = st.note(&ids[i]).unwrap();
        println!("note #{i}: {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
}
