//! Import a Mac library into the app's library the way File > Import to Notes… does
//! (`import::read_library_with` + `Store::import_library`), printing counts only (no note text, titles
//! or names).
//!
//! `cargo run --release -p nt-core --example import_into -- <Mac library> [app library dir] [pw…]`

use std::collections::BTreeMap;
use std::path::PathBuf;

use nt_core::import::{self, Options};
use nt_core::model::*;
use nt_core::store::Store;

fn kinds(lib: &Library, ids: Option<&std::collections::HashSet<String>>) -> BTreeMap<String, (usize, usize)> {
    let mut k: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for n in lib.notes.iter().filter(|n| ids.is_none_or(|s| s.contains(&n.id))) {
        for a in &n.attachments {
            let e = k.entry(format!("{:?}", a.kind)).or_default();
            e.0 += 1;
            if a.display_file().is_some() || a.parts.iter().any(|p| p.display_file().is_some()) || a.drawing.is_some() || a.table.is_some() || a.url.is_some() {
                e.1 += 1;
            }
        }
    }
    k
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(src) = args.first() else {
        eprintln!("usage: import_into <Mac library> [app library dir] [password…]");
        std::process::exit(2);
    };
    let dir = args.get(1).map(PathBuf::from).unwrap_or_else(nt_core::store::default_dir);
    let opts = Options::with_passwords(&args.iter().skip(2).cloned().collect::<Vec<_>>());
    let (lib, rep) = import::read_library_with(std::path::Path::new(src), &opts).expect("read the Mac library");
    let mut st = Store::open(&dir).expect("open the app library");
    let before = st.library().expect("read the app library");
    let sum = st.import_library(&lib).expect("import");
    let after = st.library().expect("read the app library");
    println!("Mac library: {} notes, {} attachments, locked not opened {}, iCloud not downloaded {}", rep.notes, rep.attachments, rep.locked.len(), rep.not_downloaded);
    println!(
        "imported: {} notes, {} folders, {} attachments; skipped {} already there, {} locked, {} not downloaded",
        sum.notes, sum.folders, sum.attachments, sum.skipped_existing, sum.skipped_locked, sum.skipped_not_downloaded
    );
    println!("app library: notes {} -> {}, folders {} -> {}, tags {} -> {}", before.notes.len(), after.notes.len(), before.folders.len(), after.folders.len(), before.tags.len(), after.tags.len());
    let ids: std::collections::HashSet<String> = lib.notes.iter().map(|n| n.id.clone()).collect();
    let (want, got) = (kinds(&lib, None), kinds(&after, Some(&ids)));
    println!("attachments by kind (Mac total/with content -> in the app total/with content):");
    for (k, w) in &want {
        let g = got.get(k).copied().unwrap_or_default();
        println!("  {k}: {}/{} -> {}/{}", w.0, w.1, g.0, g.1);
    }
    let strokes = |l: &Library, ids: Option<&std::collections::HashSet<String>>| -> usize {
        l.notes.iter().filter(|n| ids.is_none_or(|s| s.contains(&n.id))).flat_map(|n| n.attachments.iter()).filter(|a| a.drawing.is_some()).count()
    };
    println!("drawings as strokes: {} -> {}", strokes(&lib, None), strokes(&after, Some(&ids)));
    let bodies = |l: &Library, ids: Option<&std::collections::HashSet<String>>| -> usize {
        l.notes.iter().filter(|n| ids.is_none_or(|s| s.contains(&n.id)) && !n.body.paragraphs.is_empty()).count()
    };
    println!("notes with a body: {} -> {}", bodies(&lib, None), bodies(&after, Some(&ids)));
}
