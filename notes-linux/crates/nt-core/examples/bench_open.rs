//! Time opening a library: `bench_open <library dir>` (counts and timings only).
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("library dir"));
    for round in 1..=2 {
        let t = Instant::now();
        let st = nt_core::store::Store::open(&dir).unwrap();
        let open = ms(t);
        let mut best = f64::MAX;
        let mut meta = st.library_meta().unwrap();
        for _ in 0..5 {
            let t = Instant::now();
            meta = st.library_meta().unwrap();
            best = best.min(ms(t));
        }
        let m = best;
        let thumbs = meta.notes.iter().filter(|n| n.thumbnail().is_some()).count();
        let tagged = meta.notes.iter().filter(|n| !n.tags().is_empty()).count();
        println!("round {round}: Store::open {open:.1} ms, library_meta() {m:.1} ms ({} notes, {thumbs} with a thumbnail, {tagged} tagged)", meta.notes.len());
        let ids: Vec<String> = meta.notes.iter().filter(|n| !n.locked).map(|n| n.id.clone()).take(10).collect();
        let t = Instant::now();
        st.note(&ids[5]).unwrap();
        println!("  the first note() on this store: {:.1} ms", ms(t));
        let t = Instant::now();
        for id in &ids {
            st.note(id).unwrap();
        }
        let first = ms(t) / ids.len() as f64;
        let t = Instant::now();
        for id in &ids {
            st.note(id).unwrap();
        }
        println!("  note(id): {first:.2} ms first open, {:.2} ms cached", ms(t) / ids.len() as f64);
        let t = Instant::now();
        let hits = st.search("the").unwrap().len();
        let first = ms(t);
        let t = Instant::now();
        st.search("e").unwrap();
        println!("  search: {first:.1} ms ({hits} hits), then {:.1} ms", ms(t));
        let t = Instant::now();
        let atts = st.attachments().unwrap();
        println!("  attachments(): {:.1} ms ({} attachments)", ms(t), atts.len());
        // a save (on the copy given): one note's rows only
        let mut st = st;
        let mut n = st.note(&ids[0]).unwrap();
        n.body.paragraphs.push(nt_core::model::Paragraph { spans: vec![nt_core::model::Span::plain(format!("bench {round}"))], ..Default::default() });
        let t = Instant::now();
        st.save_note(&mut n).unwrap();
        println!("  save_note: {:.1} ms", ms(t));
        let t = Instant::now();
        let full = st.library().unwrap();
        let fl = ms(t);
        let by: std::collections::HashMap<&str, &nt_core::model::Note> = meta.notes.iter().map(|n| (n.id.as_str(), n)).collect();
        let (mut thumb_diff, mut tag_diff, mut kinds) = (0, 0, std::collections::BTreeMap::new());
        for a in &full.notes {
            let Some(b) = by.get(a.id.as_str()) else { continue };
            if a.thumbnail() != b.thumbnail() {
                thumb_diff += 1;
                *kinds.entry(format!("full {} / list {}", a.thumbnail().is_some(), b.thumbnail().is_some())).or_insert(0) += 1;
            }
            if a.tags() != b.tags() {
                tag_diff += 1;
            }
        }
        println!("  library() (everything decoded): {fl:.1} ms; list differs from full: thumbnails {thumb_diff} {kinds:?}, tags {tag_diff}");
    }
}
