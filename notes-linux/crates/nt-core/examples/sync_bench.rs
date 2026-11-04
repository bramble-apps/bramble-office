//! Measuring sync on a slow link (counts and timings only):
//! `sync_bench make <dir> <notes> <big MB>` makes a library of made-up notes (every 10th with a
//! 200 KB picture, one with a big one); `sync_bench sync <dir> <url>` syncs it (NT_DAV_USER,
//! NT_DAV_PASSWORD), printing its progress every 2 s.
use std::time::Instant;

use nt_core::model::*;
use nt_core::store::Store;

fn picture(dir: &std::path::Path, name: &str, size: usize, seed: usize) -> std::path::PathBuf {
    let p = dir.join(name);
    let bytes: Vec<u8> = [b"\x89PNG\r\n\x1a\n".as_slice(), &(0..size).map(|i| ((i * 31 + seed * 7) % 251) as u8).collect::<Vec<u8>>()].concat();
    std::fs::write(&p, bytes).unwrap();
    p
}

fn add_picture(st: &mut Store, id: &str, file: &std::path::Path) {
    let pic = st.add_attachment(id, file).unwrap();
    let mut n = st.note(id).unwrap();
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&pic.id, &pic.uti)], ..Default::default() });
    st.save_note(&mut n).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::PathBuf::from(&a[1]);
    match a[0].as_str() {
        "make" => {
            let (n, big): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
            let scratch = dir.with_extension("pictures");
            std::fs::create_dir_all(&scratch).unwrap();
            let mut st = Store::open_library(&dir).unwrap();
            for i in 0..n {
                let mut b = Body::from_lines(&format!("Note {i}"), &[&format!("Some text {i}"), &"words ".repeat(200)]);
                let note = st.create_note(None, &mut b).unwrap();
                if i % 10 == 0 {
                    add_picture(&mut st, &note.id, &picture(&scratch, &format!("p{i}.png"), 200_000, i));
                }
                if i == 0 && big > 0 {
                    add_picture(&mut st, &note.id, &picture(&scratch, "big.png", big * 1_000_000, 999));
                }
            }
            println!("made {n} notes");
        }
        "sync" => {
            let s = nt_core::sync::Settings {
                on: true,
                url: a[2].clone(),
                user: std::env::var("NT_DAV_USER").unwrap_or_default(),
                password: std::env::var("NT_DAV_PASSWORD").unwrap_or_default(),
                folder: "Bramble Notes".into(),
            };
            let mut st = Store::open_library(&dir).unwrap();
            let t = Instant::now();
            let last = std::sync::Mutex::new(Instant::now());
            let r = nt_core::sync::sync_now_with(&mut st, &s, nt_core::sync::Force::None, &|p| {
                let mut l = last.lock().unwrap();
                if l.elapsed().as_secs_f64() >= 2.0 {
                    eprintln!("  {:5.0} s  {}", t.elapsed().as_secs_f64(), p.message());
                    *l = Instant::now();
                }
            });
            match r {
                Ok(r) => println!("synced in {:.1} s: {} ({:?})", t.elapsed().as_secs_f64(), r.summary(), r),
                Err(e) => println!("failed after {:.1} s: {e}", t.elapsed().as_secs_f64()),
            }
        }
        _ => panic!("make | sync"),
    }
}
