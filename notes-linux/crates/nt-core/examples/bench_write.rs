//! Time the first writes after opening (`bench_write <library dir>`; on a copy).
use std::time::Instant;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let t = Instant::now();
    let mut st = nt_core::store::Store::open(&dir).unwrap();
    let open = t.elapsed().as_secs_f64() * 1e3;
    st.library_meta().unwrap();
    st.search("").unwrap();
    let mut times = Vec::new();
    for k in 0..6 {
        let t = Instant::now();
        let mut b = nt_core::model::Body::from_lines(&format!("bench {k}"), &["x"]);
        let n = st.create_note(None, &mut b).unwrap();
        let c = t.elapsed().as_secs_f64() * 1e3;
        let mut full = st.note(&n.id).unwrap();
        full.body.paragraphs.push(nt_core::model::Paragraph::plain(nt_core::model::ParaStyle::Body, "more"));
        let t = Instant::now();
        st.save_note(&mut full).unwrap();
        times.push(format!("create {c:.0} / save {:.0}", t.elapsed().as_secs_f64() * 1e3));
    }
    println!("open {open:.1} ms; {}", times.join(", "));
}
