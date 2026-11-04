//! Import an UpNote library and find the notes here it repeats (counts only, unless --titles):
//! `upnote_import <library dir> <UpNote folder | upnote.sqlite3 | .upnx> [--backup] [--dedupe]
//! [--apply] [--titles]`. --apply moves the repeated notes (those the UpNote copy has all of) to
//! Recently Deleted; --titles lists the ones kept for review.
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| a.iter().any(|x| x == f);
    let (lib, up) = (std::path::PathBuf::from(&a[0]), std::path::PathBuf::from(&a[1]));
    let t = Instant::now();
    if flag("--backup") {
        let b = nt_core::native::backup_library(&lib).expect("backup");
        println!("backup: {} ({:.1} s)", b.file_name().unwrap().to_string_lossy(), t.elapsed().as_secs_f64());
    }
    let mut st = nt_core::store::Store::open_library(&lib).expect("open");
    let before = st.library_meta().unwrap().notes.len();
    let r = nt_core::upnote::import(&mut st, &up).expect("import");
    println!(
        "imported {} notes ({} into Recently Deleted), {} already here; {} pictures, {} link cards, {} tables; {} -> {} notes ({:.1} s)",
        r.notes, r.trashed, r.skipped, r.pictures, r.links, r.tables, before, st.library_meta().unwrap().notes.len(), t.elapsed().as_secs_f64()
    );
    if flag("--verify") {
        let found = nt_core::upnote::fidelity(&st, &nt_core::upnote::read(&up).unwrap()).unwrap();
        let f: Vec<f64> = found.iter().map(|x| x.1).collect();
        if flag("--ids") {
            for (id, v) in found.iter().filter(|x| x.1 < 0.99999) {
                println!("  short: {id} {:.1}%", v * 100.0);
            }
        }
        let below = |x: f64| f.iter().filter(|v| **v < x).count();
        println!(
            "UpNote's own text found in the imported notes: {} notes checked; under 100%: {}, under 98%: {}, under 90%: {}; lowest {:.1}%",
            f.len(),
            below(0.99999),
            below(0.98),
            below(0.9),
            f.iter().cloned().fold(1.0, f64::min) * 100.0
        );
    }
    if !flag("--dedupe") {
        return;
    }
    let t = Instant::now();
    let dups = nt_core::upnote::duplicates(&st, &nt_core::upnote::imported_note_ids(&st)).expect("duplicates");
    let (gone, keep): (Vec<_>, Vec<_>) = dups.iter().partition(|d| d.keep.is_none());
    let exact = dups.iter().filter(|d| d.similarity >= 0.9999).count();
    println!(
        "repeated by UpNote (≥{:.0}% alike): {} ({} word for word); UpNote has all of {}; {} kept for review ({:.1} s)",
        nt_core::upnote::SIMILAR * 100.0,
        dups.len(),
        exact,
        gone.len(),
        keep.len(),
        t.elapsed().as_secs_f64()
    );
    let mut why: std::collections::BTreeMap<String, usize> = Default::default();
    for d in &keep {
        let k = d.keep.as_deref().unwrap_or_default();
        let kind = if k.contains("drawings") { "drawings, scans or files" } else if k.contains("pictures") { "more pictures" } else if k.contains("newer") { "newer, with words UpNote doesn't have" } else { "older, with words UpNote doesn't have" };
        *why.entry(kind.into()).or_default() += 1;
    }
    println!("kept for review, why: {why:?}");
    let sims: Vec<f64> = gone.iter().map(|d| d.similarity).collect();
    println!("moved ones, alike: lowest {:.1}%, under 95%: {}", sims.iter().cloned().fold(1.0, f64::min) * 100.0, sims.iter().filter(|s| **s < 0.95).count());
    if flag("--titles") {
        for d in &keep {
            println!("  review: {:?} ({:.0}% alike): {}", d.title, d.similarity * 100.0, d.keep.as_deref().unwrap_or_default());
        }
    }
    if flag("--apply") {
        for d in &gone {
            st.delete_note(&d.note).expect("to Recently Deleted");
        }
        println!("moved {} to Recently Deleted", gone.len());
    }
}
