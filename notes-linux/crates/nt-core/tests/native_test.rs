//! The native library: notes as files, the database their index (rebuilt from the files alone),
//! and the old layout converted losslessly.

mod common;

use std::path::Path;

use nt_core::import;
use nt_core::model::*;
use nt_core::native;
use nt_core::store::Store;

/// A note's content, everything but where its files are (paths differ by library).
fn content(st: &Store, n: &Note) -> String {
    let n = if n.light { st.note(&n.id).unwrap() } else { n.clone() };
    // a file by its content: (length, byte sum)
    let bytes = |p: &Option<std::path::PathBuf>| p.as_ref().and_then(|p| std::fs::read(p).ok()).map(|b| (b.len(), b.iter().map(|x| u64::from(*x)).sum::<u64>()));
    let att = |a: &Attachment| {
        format!(
            "{} {} {:?} {} {:?} {:?} {:?} {:?} {:?} {:?} {} {}",
            a.id,
            a.uti,
            a.kind,
            a.name,
            a.table,
            a.drawing.as_ref().map(|d| (d.strokes.len(), d.shapes.len())),
            bytes(&a.file),
            bytes(&a.fallback),
            a.parts.iter().map(|p| (p.id.clone(), bytes(&p.file))).collect::<Vec<_>>(),
            a.view_size,
            a.pages.len(),
            a.not_downloaded
        )
    };
    let mut atts: Vec<String> = n.attachments.iter().map(att).collect();
    atts.sort();
    format!(
        "{} | {} | {:?} | folder {} pinned {} deleted {} locked {} | {:.0} {:.0} | {:?} | {}",
        n.id,
        n.title,
        n.body,
        n.folder,
        n.pinned,
        n.deleted,
        n.locked,
        n.created,
        n.modified,
        n.tags(),
        atts.join(" ; ")
    )
}

fn everything(st: &Store) -> (Vec<String>, Vec<String>) {
    let lib = st.library().unwrap();
    let mut notes: Vec<String> = lib.notes.iter().map(|n| content(st, n)).collect();
    notes.sort();
    let mut folders: Vec<String> = lib.folders.iter().filter(|f| f.kind != FolderKind::Trash && !f.default).map(|f| format!("{} {} {:?} {:?}", f.id, f.name, f.parent, f.smart)).collect();
    folders.sort();
    (notes, folders)
}

/// Some of everything: text with a tag, a picture, a table, a drawing, a folder, a Smart Folder,
/// a pinned note, a locked note, a note in Recently Deleted.
fn fill(st: &mut Store, dir: &Path) -> Vec<String> {
    let png = dir.join("in.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03\x08\x02\0\0\0pixels").unwrap();
    let mut b = Body::from_lines("Groceries", &["jelly and bread"]);
    let n1 = st.create_note(None, &mut b).unwrap();
    let pic = st.add_attachment(&n1.id, &png).unwrap();
    let tid = st.add_table(&n1.id, &Table::from_text(&[&["a", "b"], &["c", ""]])).unwrap();
    let mut n = st.note(&n1.id).unwrap();
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&pic.id, &pic.uti)], ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&tid, UTI_TABLE)], ..Default::default() });
    let ink = Drawing { width: 768.0, height: 100.0, strokes: vec![Stroke { id: "s".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 2.6, points: (0..4).map(|i| InkPoint { x: 10.0 * i as f32, y: 20.0, t: 0.01 * i as f32, size: 2.6, force: 1.0, azimuth: 0.0, altitude: 1.5, opacity: 1.0 }).collect(), transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask: None }], shapes: Vec::new() };
    let did = nt_core::pb::new_uuid();
    n.attachments.push(Attachment { id: did.clone(), uti: UTI_PAPER.into(), kind: AttachmentKind::Drawing, drawing: Some(ink), width: 768.0, height: 100.0, ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&did, UTI_PAPER)], ..Default::default() });
    n.body.paragraphs.push(Paragraph { style: ParaStyle::Checklist, checked: true, spans: vec![Span::plain("done")], ..Default::default() });
    st.save_note(&mut n).unwrap();
    st.add_tag(&n1.id, "food").unwrap();
    let f = st.create_folder("Work", None).unwrap();
    st.create_smart_folder("Pinned", r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#).unwrap();
    let mut b = Body::from_lines("Plans", &["walk in the hills"]);
    let n2 = st.create_note(Some(&f), &mut b).unwrap();
    st.set_pinned(&n2.id, true).unwrap();
    let mut b = Body::from_lines("Secret", &["the code is 1234"]);
    let n3 = st.create_note(None, &mut b).unwrap();
    st.set_password("pw", None).unwrap();
    st.lock_note(&n3.id, "pw").unwrap();
    let mut b = Body::from_lines("Old", &["thrown away"]);
    let n4 = st.create_note(None, &mut b).unwrap();
    st.delete_note(&n4.id).unwrap();
    vec![n1.id, n2.id, n3.id, n4.id]
}

#[test]
fn a_new_library_is_files() {
    let dir = common::scratch("native-new");
    let lib = dir.join("Bramble Notes");
    let mut st = Store::open_library(&lib).unwrap();
    assert!(st.is_native() && native::is_native(&lib) && lib.join("index/NoteStore.sqlite").is_file());
    let ids = fill(&mut st, &dir);
    // every note is a file, every attachment's content is in files/
    for id in &ids {
        let j: serde_json::Value = serde_json::from_slice(&std::fs::read(lib.join(format!("notes/{id}.json"))).unwrap()).unwrap();
        assert_eq!(j["note"]["id"], *id);
    }
    let secret = std::fs::read(lib.join(format!("notes/{}.json", ids[2]))).unwrap();
    assert!(!secret.windows(4).any(|w| w == b"1234"), "the locked note is written encrypted");
    // the picture, by its content
    let png_sha = nt_core::sync::content_hash(&std::fs::read(dir.join("in.png")).unwrap());
    assert!(lib.join("files").join(&png_sha).is_file());
    let libj: serde_json::Value = serde_json::from_slice(&std::fs::read(lib.join("library.json")).unwrap()).unwrap();
    assert_eq!((libj["format"].as_str(), libj["version"].as_u64()), (Some("bramble-notes"), Some(1)));
    assert_eq!(libj["folders"].as_array().unwrap().len(), 2);
    assert!(libj["tags"].as_array().unwrap().iter().any(|t| t == "food"));
    let want = everything(&st);

    // an edit touches only that note's file
    let others: Vec<_> = ids.iter().skip(1).map(|id| std::fs::metadata(lib.join(format!("notes/{id}.json"))).unwrap().modified().unwrap()).collect();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut b = Body::from_lines("Groceries", &["jelly, bread and milk"]);
    st.update_note(&ids[0], &mut b).unwrap();
    let after: Vec<_> = ids.iter().skip(1).map(|id| std::fs::metadata(lib.join(format!("notes/{id}.json"))).unwrap().modified().unwrap()).collect();
    assert_eq!(others, after);
    assert!(String::from_utf8_lossy(&std::fs::read(lib.join(format!("notes/{}.json", ids[0]))).unwrap()).len() > 100);
    let want_edited = everything(&st);
    assert_ne!(want, want_edited);

    // erased: a tombstone
    st.erase_note(&ids[3]).unwrap();
    let t: serde_json::Value = serde_json::from_slice(&std::fs::read(lib.join(format!("notes/{}.json", ids[3]))).unwrap()).unwrap();
    assert_eq!(t["deleted"], true);
    let want = everything(&st);
    drop(st);

    // reopened: nothing to do
    let mut st = Store::open_library(&lib).unwrap();
    assert_eq!(st.refresh().unwrap(), native::Refresh::default());
    assert_eq!(everything(&st), want);
    drop(st);

    // the index is only a cache: rebuilt from the files, the same library
    std::fs::remove_dir_all(lib.join("index")).unwrap();
    let st = Store::open_library(&lib).unwrap();
    assert_eq!(everything(&st), want);
    assert!(st.unlock_note(&ids[2], "pw").unwrap().body.text().contains("1234"));
    assert!(st.check_password("pw").unwrap() || !st.has_password().unwrap(), "the account password is the index's");
    nt_core::lock::forget_unlocked();
}

#[test]
fn a_note_file_changed_outside_is_read_in() {
    let dir = common::scratch("native-outside");
    let (a, b) = (dir.join("a"), dir.join("b"));
    let mut sa = Store::open_library(&a).unwrap();
    let ids = fill(&mut sa, &dir);
    drop(sa);
    // a copy of the library (another device), edited there
    copy(&a, &b);
    std::fs::remove_dir_all(b.join("index")).unwrap();
    let mut sb = Store::open_library(&b).unwrap();
    let mut body = Body::from_lines("Plans", &["walk in the hills, then tea"]);
    sb.update_note(&ids[1], &mut body).unwrap();
    sb.erase_note(&ids[3]).unwrap();
    let mut nb = Body::from_lines("New there", &["made on b"]);
    let made = sb.create_note(None, &mut nb).unwrap();
    drop(sb);
    // its files come here (as sync would copy them)
    for name in [format!("{}.json", ids[1]), format!("{}.json", ids[3]), format!("{}.json", made.id)] {
        std::fs::copy(b.join("notes").join(&name), a.join("notes").join(&name)).unwrap();
    }
    let mut sa = Store::open_library(&a).unwrap();
    assert!(sa.note(&ids[1]).unwrap().body.text().contains("tea"));
    assert!(sa.note(&ids[3]).is_err());
    assert!(sa.note(&made.id).unwrap().body.text().contains("made on b"));
    assert_eq!(sa.refresh().unwrap(), native::Refresh::default());
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = to.join(e.file_name());
        if e.path().is_dir() {
            copy(&e.path(), &p);
        } else {
            std::fs::copy(e.path(), p).unwrap();
        }
    }
}

#[test]
fn the_old_layout_converts_losslessly() {
    let dir = common::scratch("native-migrate");
    let lib = dir.join("bramble-notes");
    let mut st = Store::open(&lib).unwrap();
    fill(&mut st, &dir);
    // plus every feature the Mac research has (tables, drawings, checklists, math, links, collapsed
    // headings, scans, PDFs…)
    if let Some(r) = common::research() {
        let mac = import::read_library(&common::group_container(&r, "native-migrate-mac")).unwrap();
        st.import_library(&mac).unwrap();
    }
    let want = everything(&st);
    let n_notes = want.0.len();
    drop(st);

    let st = Store::open_library(&lib).unwrap();
    let m = st.migration().unwrap().clone().unwrap();
    assert_eq!(m.notes, n_notes);
    assert!(m.backup.join("NoteStore.sqlite").is_file() && m.backup.to_string_lossy().contains("bramble-notes-backup-"));
    assert!(!lib.join("NoteStore.sqlite").exists() && lib.join("index/NoteStore.sqlite").is_file());
    assert_eq!(everything(&st), want, "the same library after converting");
    drop(st);
    // the backup opens as the old library
    assert_eq!(everything(&Store::open(&m.backup).unwrap()).0.len(), n_notes);

    // and from the files alone: the round trip
    std::fs::remove_dir_all(lib.join("index")).unwrap();
    let st = Store::open_library(&lib).unwrap();
    assert!(st.migration().is_none());
    let got = everything(&st);
    for (a, b) in want.0.iter().zip(&got.0) {
        if a != b {
            let i = a.bytes().zip(b.bytes()).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
            let from = i.saturating_sub(120);
            panic!("differs at {i}:\n want …{}\n got  …{}", &a[from..(i + 200).min(a.len())], &b[from..(i + 200).min(b.len())]);
        }
    }
    assert_eq!(got, want, "rebuilt from the files");
}

#[test]
fn a_conversion_cut_short_finishes_on_the_next_open() {
    let dir = common::scratch("native-migrate-cut");
    let lib = dir.join("bramble-notes");
    let mut st = Store::open(&lib).unwrap();
    fill(&mut st, &dir);
    let want = everything(&st);
    drop(st);
    drop(Store::open_library(&lib).unwrap());
    // as if the app quit after the database moved into index/, before library.json and the
    // attachment files followed
    std::fs::remove_file(lib.join("library.json")).unwrap();
    std::fs::remove_file(lib.join("index/manifest.json")).unwrap();
    std::fs::write(lib.join("library.json.new"), b"{").unwrap();
    std::fs::rename(lib.join("index/Accounts"), lib.join("Accounts")).unwrap();
    let st = Store::open_library(&lib).unwrap();
    assert!(st.is_native() && lib.join("library.json").is_file() && !lib.join("library.json.new").exists());
    assert!(!lib.join("Accounts").exists() && lib.join("index/Accounts").is_dir());
    assert_eq!(everything(&st), want, "the same library after finishing");
    drop(st);
    std::fs::remove_dir_all(lib.join("index")).unwrap();
    assert_eq!(everything(&Store::open_library(&lib).unwrap()), want, "and from its files");
}

#[test]
fn a_failed_conversion_leaves_the_old_library() {
    let dir = common::scratch("native-migrate-fail");
    let lib = dir.join("bramble-notes");
    let mut st = Store::open(&lib).unwrap();
    fill(&mut st, &dir);
    let want = everything(&st);
    drop(st);
    // notes/ can't be made: a file is in the way
    std::fs::write(lib.join("notes"), b"not a folder").unwrap();
    let st = Store::open_library(&lib).unwrap();
    assert!(st.migration().unwrap().is_err() && !st.is_native());
    assert!(lib.join("NoteStore.sqlite").is_file() && !lib.join("library.json").exists() && !lib.join("index").exists());
    assert_eq!(everything(&st), want);
}

#[cfg(target_os = "linux")]
#[test]
fn a_library_open_elsewhere_isnt_converted() {
    let dir = common::scratch("native-in-use");
    let lib = dir.join("bramble-notes");
    let mut st = Store::open(&lib).unwrap();
    fill(&mut st, &dir);
    drop(st);
    // another program holding the database open (a running older app)
    let mut child = std::process::Command::new("sh").arg("-c").arg(format!("exec 3<'{}'; exec sleep 30", lib.join("NoteStore.sqlite").display())).spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    let st = Store::open_library(&lib).unwrap();
    let _ = child.kill();
    let _ = child.wait();
    assert!(st.migration().unwrap().as_ref().is_err_and(|e| e.contains("open in another program")));
    assert!(lib.join("NoteStore.sqlite").is_file() && !lib.join("index").exists());
    drop(st);
    // once it's closed, the next open converts
    let st = Store::open_library(&lib).unwrap();
    assert!(st.migration().unwrap().is_ok() && st.is_native());
}

#[test]
fn bnotes_export_and_import_losslessly() {
    let dir = common::scratch("native-bnotes");
    let mut a = Store::open_library(&dir.join("a")).unwrap();
    fill(&mut a, &dir);
    let want = everything(&a);
    let file = dir.join("All notes.bnotes");
    assert_eq!(native::export_bnotes(&a, &file).unwrap(), want.0.len());
    // into an empty library: the same notes and folders
    let mut b = Store::open_library(&dir.join("b")).unwrap();
    let r = native::import_bnotes(&mut b, &file).unwrap();
    assert_eq!((r.notes, r.skipped), (want.0.len(), 0));
    assert_eq!(everything(&b), want);
    // again: nothing new
    let r = native::import_bnotes(&mut b, &file).unwrap();
    assert_eq!((r.notes, r.skipped), (0, want.0.len()));
    // into an old-layout library too
    let mut c = Store::open(&dir.join("c")).unwrap();
    native::import_bnotes(&mut c, &file).unwrap();
    assert_eq!(everything(&c), want);
    // not a library: refused
    std::fs::write(dir.join("junk.bnotes"), b"PK not really").unwrap();
    assert!(native::import_bnotes(&mut c, &dir.join("junk.bnotes")).is_err());
}

#[test]
fn a_native_library_backs_up_whole() {
    let dir = common::scratch("native-backup");
    let lib = dir.join("bramble-notes");
    let mut st = Store::open_library(&lib).unwrap();
    fill(&mut st, &dir);
    let want = everything(&st);
    let to = native::backup_library(&lib).unwrap();
    assert!(to.file_name().unwrap().to_string_lossy().starts_with("bramble-notes-backup-"));
    // changed afterwards: the backup keeps what was
    let mut b = Body::from_lines("After the backup", &["new"]);
    st.create_note(None, &mut b).unwrap();
    drop(st);
    assert_eq!(everything(&Store::open_library(&to).unwrap()), want);
    // and from its files alone
    std::fs::remove_dir_all(to.join("index")).unwrap();
    assert_eq!(everything(&Store::open_library(&to).unwrap()), want);
}
