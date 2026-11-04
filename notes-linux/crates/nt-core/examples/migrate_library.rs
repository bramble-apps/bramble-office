//! Convert a library to the native format (`migrate_library <library dir>`): counts and timings
//! only. Run on a copy first.
use std::time::Instant;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("library dir"));
    // what the window would show meanwhile, once a second
    std::thread::spawn(|| {
        let mut last = None;
        loop {
            let m = nt_core::native::opening().message();
            if m.is_some() && m != last {
                eprintln!("  {}", m.as_deref().unwrap_or_default());
            }
            last = m;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    let t = Instant::now();
    let st = nt_core::store::Store::open_library(&dir).unwrap();
    let open = t.elapsed().as_secs_f64();
    match st.migration() {
        Some(Ok(m)) => println!(
            "converted in {:.1} s: {} notes, {} folders, {} attachments, {} locked, {} tags; backup {}",
            m.seconds, m.notes, m.folders, m.attachments, m.locked, m.tags, m.backup.file_name().unwrap().to_string_lossy()
        ),
        Some(Err(e)) => println!("NOT converted: {e}"),
        None => println!("already native"),
    }
    println!("open_library {open:.2} s, native {}", st.is_native());
    drop(st);
    for _ in 0..2 {
        let t = Instant::now();
        let st = nt_core::store::Store::open_library(&dir).unwrap();
        let o = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let lib = st.library_meta().unwrap();
        let m = t.elapsed().as_secs_f64() * 1e3;
        let id = lib.notes.iter().find(|n| !n.locked).map(|n| n.id.clone()).unwrap();
        let t = Instant::now();
        st.note(&id).unwrap();
        println!("reopen {o:.1} ms, library_meta {m:.1} ms ({} notes), first note {:.1} ms", lib.notes.len(), t.elapsed().as_secs_f64() * 1e3);
    }
    let du = |p: &std::path::Path| std::process::Command::new("du").args(["-sh", "--apparent-size"]).arg(p).output().map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().next().unwrap_or("").to_string()).unwrap_or_default();
    println!("sizes: notes {} files {} index {}", du(&dir.join("notes")), du(&dir.join("files")), du(&dir.join("index")));
}
