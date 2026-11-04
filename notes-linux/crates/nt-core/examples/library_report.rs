//! Import a Notes library (read-only) and print counts only, never note content, titles or file
//! names: what imports and what doesn't. For testing on a real library.
//!
//! cargo run --release -p nt-core --example library_report -- <group.com.apple.notes or NoteStore.sqlite> [password…]

use std::collections::BTreeMap;

use nt_core::import::{self, Options};
use nt_core::model::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: library_report <group.com.apple.notes folder | NoteStore.sqlite> [password…]");
        std::process::exit(2);
    };
    let passwords: Vec<String> = args.collect();
    let t = std::time::Instant::now();
    let (lib, rep) = match import::read_library_with(std::path::Path::new(&path), &Options::with_passwords(&passwords)) {
        Ok(x) => x,
        Err(e) => {
            println!("import failed: {e}");
            std::process::exit(1);
        }
    };
    println!("read in {:.1}s", t.elapsed().as_secs_f64());
    println!("accounts: {} ({})", lib.accounts.len(), lib.accounts.iter().map(|a| format!("type {}", a.kind)).collect::<Vec<_>>().join(", "));
    println!("  with a locked-notes password: {}", rep.password_hints.len());
    let kinds = |k: FolderKind| lib.folders.iter().filter(|f| f.kind == k).count();
    println!(
        "folders: {} (normal {}, smart {}, trash {}, nested {})",
        lib.folders.len(),
        kinds(FolderKind::Normal),
        kinds(FolderKind::Smart),
        kinds(FolderKind::Trash),
        lib.folders.iter().filter(|f| f.parent.is_some()).count()
    );
    let n = &lib.notes;
    println!(
        "notes: {} (in Recently Deleted {}, pinned {}, locked {}, of which opened {})",
        n.len(),
        n.iter().filter(|x| x.deleted).count(),
        n.iter().filter(|x| x.pinned).count(),
        n.iter().filter(|x| x.locked).count(),
        rep.unlocked
    );
    let empty = n.iter().filter(|x| !x.locked && x.body.paragraphs.is_empty()).count();
    println!("  unlocked notes with no decoded body: {empty} (iCloud bodies not downloaded on the Mac: {})", rep.not_downloaded);
    println!("  tags: {}, notes with tags: {}", lib.tags.len(), n.iter().filter(|x| !x.tags().is_empty()).count());
    let mut tokens: BTreeMap<String, usize> = BTreeMap::new();
    for t in n.iter().flat_map(|x| x.body.tokens()).filter(|t| t.is_inline()) {
        *tokens.entry(t.uti.rsplit('.').next().unwrap_or("").to_string()).or_default() += 1;
    }
    println!("  inline tokens: {tokens:?}");
    let all: Vec<&Attachment> = n.iter().flat_map(|x| x.attachments.iter().flat_map(|a| std::iter::once(a).chain(a.parts.iter()))).collect();
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut utis: BTreeMap<&str, usize> = BTreeMap::new();
    for a in &all {
        let e = by_kind.entry(format!("{:?}", a.kind)).or_default();
        e.0 += 1;
        if a.display_file().is_some() || a.parts.iter().any(|p| p.display_file().is_some()) || a.drawing.is_some() || a.table.is_some() || a.url.is_some() {
            e.1 += 1;
        }
        *utis.entry(a.uti.as_str()).or_default() += 1;
    }
    println!("attachments: {} (kind: total / with content on disk or decoded)", all.len());
    for (k, (t, ok)) in &by_kind {
        println!("  {k}: {t} / {ok}");
    }
    println!("  by UTI: {utis:?}");
    let drawings: Vec<&&Attachment> = all.iter().filter(|a| a.kind == AttachmentKind::Drawing).collect();
    println!(
        "drawings: {} (as strokes {}, picture only {}, nothing {})",
        drawings.len(),
        drawings.iter().filter(|a| a.drawing.is_some()).count(),
        drawings.iter().filter(|a| a.drawing.is_none() && a.display_file().is_some()).count(),
        drawings.iter().filter(|a| a.drawing.is_none() && a.display_file().is_none()).count()
    );
    let tables: Vec<&&Attachment> = all.iter().filter(|a| a.kind == AttachmentKind::Table).collect();
    println!(
        "tables: {} (decoded {}, cells {})",
        tables.len(),
        tables.iter().filter(|a| a.table.is_some()).count(),
        tables.iter().filter_map(|a| a.table.as_ref()).map(|t| t.rows.iter().map(Vec::len).sum::<usize>()).sum::<usize>()
    );
    println!("missing files: {} (attachments iCloud hadn't downloaded: {})", rep.missing_files.len(), rep.attachments_not_downloaded);
    let docs: Vec<&Attachment> = all.iter().copied().filter(|a| !a.pages.is_empty()).collect();
    println!("PDF documents with pages: {} ({} pages)", docs.len(), docs.iter().map(|a| a.pages.len()).sum::<usize>());
    // reasons only (the part after the title / id)
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    for u in &rep.unreadable {
        *reasons.entry(u.split_once(": ").map(|(_, r)| r.to_string()).unwrap_or_default()).or_default() += 1;
    }
    println!("unreadable: {} {reasons:?}", rep.unreadable.len());
    println!("locked notes not opened: {} (with the Mac login password, can't open here: {})", rep.locked.len(), rep.locked_with_login_password);
    nt_core::lock::forget_unlocked();
}
