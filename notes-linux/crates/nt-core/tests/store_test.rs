//! The app's own NoteStore-format library.

mod common;

use std::path::Path;

use nt_core::model::*;
use nt_core::store::{self, Store, DEFAULT_FOLDER, TRASH_FOLDER};
use nt_core::{body, import};

fn sql_set(db: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut st = c.prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%'").unwrap();
    let mut v: Vec<String> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    v.sort();
    v
}

/// Z_MAX of every entity tree root is at least the largest Z_PK of its table.
fn check_z_max(db: &Path) {
    let c = rusqlite::Connection::open(db).unwrap();
    let q = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
    for (ent, table) in [("ICCloudSyncingObject", "ZICCLOUDSYNCINGOBJECT"), ("ICCloudState", "ZICCLOUDSTATE"), ("ICNoteData", "ZICNOTEDATA")] {
        let max = q(&format!("SELECT Z_MAX FROM Z_PRIMARYKEY WHERE Z_NAME = '{ent}'"));
        let pk = q(&format!("SELECT COALESCE(MAX(Z_PK), 0) FROM {table}"));
        assert!(max >= pk, "{ent}: Z_MAX {max} < max Z_PK {pk}");
    }
    // every object row's Z_ENT is a known entity
    let bad = q("SELECT COUNT(*) FROM ZICCLOUDSYNCINGOBJECT WHERE Z_ENT NOT IN (SELECT Z_ENT FROM Z_PRIMARYKEY)");
    assert_eq!(bad, 0);
}

#[test]
fn fresh_store_matches_notes_schema() {
    let r = research_or_skip!();
    let d = common::scratch("store-fresh");
    let s = Store::open(&d).unwrap();
    let research = r.join("sample-files/store/NoteStore-BrambleResearch-only.sqlite");
    assert_eq!(sql_set(&s.path()), sql_set(&research), "same tables and indexes as Notes 4.13");
    let c = rusqlite::Connection::open_with_flags(&s.path(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let rc = rusqlite::Connection::open_with_flags(&research, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let plist = |c: &rusqlite::Connection| c.query_row("SELECT Z_PLIST FROM Z_METADATA", [], |r| r.get::<_, Vec<u8>>(0)).unwrap();
    assert_eq!(plist(&c), plist(&rc), "Z_METADATA must be Notes' (a hash change triggers migration)");
    let ents = |c: &rusqlite::Connection| {
        let mut st = c.prepare("SELECT Z_ENT, Z_NAME, Z_SUPER FROM Z_PRIMARYKEY ORDER BY Z_ENT").unwrap();
        st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))).unwrap().map(Result::unwrap).collect::<Vec<_>>()
    };
    assert_eq!(ents(&c), ents(&rc));
    let lib = s.library().unwrap();
    assert_eq!(lib.accounts, vec![Account { id: "LocalAccount".into(), name: "On My Mac".into(), kind: 3 }]);
    let names: Vec<_> = lib.folders.iter().map(|f| (f.id.as_str(), f.name.as_str(), f.kind)).collect();
    assert_eq!(names, [(DEFAULT_FOLDER, "Notes", FolderKind::Normal), (TRASH_FOLDER, "Recently Deleted", FolderKind::Trash)]);
    drop(s);
    // reopening doesn't add another account
    assert_eq!(Store::open(&d).unwrap().library().unwrap().folders.len(), 2);
    check_z_max(&d.join("NoteStore.sqlite"));
}

#[test]
fn notes_and_folders_lifecycle() {
    let d = common::scratch("store-life");
    let mut s = Store::open(&d).unwrap();
    let work = s.create_folder("Work", None).unwrap();
    let sub = s.create_folder("Projects", Some(&work)).unwrap();
    let mut b = Body::from_lines("Plan", &["first line", "second"]);
    let n = s.create_note(Some(&sub), &mut b).unwrap();
    assert_eq!((n.title.as_str(), n.snippet.as_str(), n.folder.as_str()), ("Plan", "first line", sub.as_str()));
    assert_eq!(n.body, b, "reads back what was written (ids filled in)");
    assert!(b.paragraphs.iter().all(|p| !p.id.is_empty()));
    assert!(n.created > 1.7e9 && n.modified >= n.created);

    // edit
    b.paragraphs[0].spans[0].text = "Plan B".into();
    b.paragraphs.push(Paragraph { style: ParaStyle::Checklist, spans: vec![Span::plain("call")], ..Default::default() });
    b.paragraphs[1].spans[0].highlight = Some(2);
    s.update_note(&n.id, &mut b).unwrap();
    let n2 = s.note(&n.id).unwrap();
    assert_eq!(n2.title, "Plan B");
    assert_eq!(n2.body, b);
    let c = rusqlite::Connection::open(s.path()).unwrap();
    let flags: (i64, i64, i64, i64, String) = c
        .query_row(
            "SELECT ZHASCHECKLIST, ZHASCHECKLISTINPROGRESS, ZHASEMPHASIS, ZMINIMUMSUPPORTEDNOTESVERSION, ZWIDGETSNIPPET FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1",
            [&n.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(flags, (1, 1, 1, 14, "Plan B\nfirst line".into()));

    // pin, move, rename, folder moves
    s.set_pinned(&n.id, true).unwrap();
    assert!(s.note(&n.id).unwrap().pinned);
    s.move_note(&n.id, &work).unwrap();
    assert_eq!(s.note(&n.id).unwrap().folder, work);
    s.rename_folder(&sub, "Ideas").unwrap();
    assert!(s.move_folder(&work, Some(&sub)).is_err(), "no folder inside its own subfolder");
    s.move_folder(&sub, None).unwrap();
    let lib = s.library().unwrap();
    let f = lib.folder(&sub).unwrap();
    assert_eq!((f.name.as_str(), f.parent.as_ref()), ("Ideas", None));

    // smart folder
    let q = r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#;
    let smart = s.create_smart_folder("Pinned", q).unwrap();
    let lib = s.library().unwrap();
    assert_eq!(lib.folder(&smart).unwrap().kind, FolderKind::Smart);
    assert_eq!(lib.folder(&smart).unwrap().smart.as_deref(), Some(q));

    // Recently Deleted
    s.delete_note(&n.id).unwrap();
    let gone = s.note(&n.id).unwrap();
    assert!(gone.deleted);
    assert_eq!(gone.folder, TRASH_FOLDER);
    s.recover_note(&n.id, None).unwrap();
    assert_eq!(s.note(&n.id).unwrap().folder, DEFAULT_FOLDER);
    // deleting a folder sends its notes to Recently Deleted
    let mut b2 = Body::from_lines("In Work", &[]);
    let n3 = s.create_note(Some(&work), &mut b2).unwrap();
    s.delete_folder(&work).unwrap();
    assert!(s.library().unwrap().folder(&work).is_none());
    assert!(s.note(&n3.id).unwrap().deleted);
    assert!(s.delete_folder(DEFAULT_FOLDER).is_err());
    assert_eq!(s.empty_trash().unwrap(), 1);
    assert!(s.note(&n3.id).is_err());
    s.erase_note(&n.id).unwrap();
    assert!(s.library().unwrap().notes.is_empty());
    let left: i64 = c.query_row("SELECT COUNT(*) FROM ZICNOTEDATA", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 0);
    check_z_max(&s.path());
}

#[test]
fn attachments_tables_tokens() {
    let r = research_or_skip!();
    let d = common::scratch("store-att");
    let mut s = Store::open(&d).unwrap();
    let mut b = Body::from_lines("Pics", &["see:"]);
    let n = s.create_note(None, &mut b).unwrap();
    let png = r.join("sample-files/inputs/bramble-gradient.png");
    let a = s.add_attachment(&n.id, &png).unwrap();
    assert_eq!((a.kind, a.uti.as_str(), a.name.as_str(), a.width, a.height), (AttachmentKind::Image, "public.png", "bramble-gradient.png", 320.0, 200.0));
    let f = a.file.clone().unwrap();
    assert!(f.starts_with(d.join("Accounts/LocalAccount/Media")));
    assert_eq!(std::fs::read(&f).unwrap(), std::fs::read(&png).unwrap());
    let t = s.add_table(&n.id, &Table::from_text(&[&["a", "b"], &["c", ""]])).unwrap();
    b.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, &a.uti)], ..Default::default() });
    b.paragraphs.push(Paragraph { spans: vec![Span::token(&t, UTI_TABLE)], ..Default::default() });
    // inline tokens: a tag and a divider
    let mut tag = Span::token("", UTI_HASHTAG);
    tag.token.as_mut().unwrap().alt = "#work".into();
    let mut div = Span::token("", UTI_DIVIDER);
    div.token.as_mut().unwrap().alt = "---".into();
    b.paragraphs.push(Paragraph { spans: vec![Span::plain("x "), tag, Span::plain(" "), div], ..Default::default() });
    s.update_note(&n.id, &mut b).unwrap();
    let n2 = s.note(&n.id).unwrap();
    assert_eq!(n2.attachments.len(), 2);
    assert_eq!(n2.attachment(&t).unwrap().table.as_ref().unwrap().text_grid(), vec![vec!["a", "b"], vec!["c", ""]]);
    assert_eq!(n2.attachment(&t).unwrap().summary, "a\nb\nc\n\n");
    assert_eq!(n2.tags(), ["work"]);
    assert_eq!(s.library().unwrap().tags, vec![Tag { account: "LocalAccount".into(), name: "work".into() }]);
    let toks: Vec<_> = n2.body.tokens().map(|t| (t.uti.as_str(), t.alt.as_str(), t.content.as_str())).collect();
    assert_eq!(toks[2], (UTI_HASHTAG, "#work", "WORK"));
    assert_eq!(toks[3], (UTI_DIVIDER, "---", ""));
    s.update_table(&t, &Table::from_text(&[&["z"]])).unwrap();
    assert_eq!(s.note(&n.id).unwrap().attachment(&t).unwrap().table.as_ref().unwrap().text_grid(), vec![vec!["z"]]);
    // removing the image token marks the attachment deleted; putting it back restores it
    let img_para = b.paragraphs.remove(2);
    s.update_note(&n.id, &mut b).unwrap();
    assert!(s.note(&n.id).unwrap().attachment(&a.id).is_none());
    b.paragraphs.insert(2, img_para);
    s.update_note(&n.id, &mut b).unwrap();
    assert!(s.note(&n.id).unwrap().attachment(&a.id).is_some());
    // erase removes the media files
    s.erase_note(&n.id).unwrap();
    assert!(!f.exists());
    check_z_max(&s.path());
}

#[test]
fn every_sample_body_survives_the_store() {
    let r = research_or_skip!();
    let lib = import::read_library(&r.join("sample-files/store/NoteStore-BrambleResearch-only.sqlite")).unwrap();
    let d = common::scratch("store-samples");
    let mut s = Store::open(&d).unwrap();
    for n in &lib.notes {
        let mut b = n.body.clone();
        let made = s.create_note(None, &mut b).unwrap();
        assert_eq!(made.body.text(), n.body.text(), "{}", n.title);
        assert_eq!(made.body, b, "{}", n.title);
        let (t, _) = body::title_and_snippet(&b);
        assert_eq!(made.title, t);
    }
    check_z_max(&s.path());
}

#[test]
fn import_a_mac_library() {
    let r = research_or_skip!();
    let src = common::group_container(&r, "store-import-src");
    let mac = import::read_library(&src).unwrap();
    let d = common::scratch("store-import");
    let mut s = Store::open(&d).unwrap();
    let sum = s.import_library(&mac).unwrap();
    assert_eq!(sum.notes, 39);
    assert_eq!(sum.attachments, 5);
    assert_eq!(sum.skipped_locked, 0);
    let lib = s.library().unwrap();
    // the Mac's On My Mac account is this library's own: Bramble Research > B-Sub at its top (no
    // "On My Mac" folder), B-Smart Pinned
    assert!(!lib.folders.iter().any(|f| f.name == "On My Mac"));
    let research = lib.folders.iter().find(|f| f.name == "Bramble Research").unwrap();
    assert_eq!((research.parent.as_deref(), research.account.as_str()), (None, store::ACCOUNT_ID));
    let bsub = lib.folders.iter().find(|f| f.name == "B-Sub").unwrap();
    assert_eq!(bsub.parent.as_deref(), Some(research.id.as_str()));
    assert_eq!(lib.folders.iter().find(|f| f.name == "B-Smart Pinned").unwrap().kind, FolderKind::Smart);
    assert_eq!(lib.folders.iter().filter(|f| f.kind == FolderKind::Trash).count(), 1);
    for n in &mac.notes {
        let m = lib.note(&n.id).unwrap_or_else(|| panic!("{}", n.title));
        assert_eq!(m.body.text(), n.body.text(), "{}", n.title);
        assert_eq!(m.title, n.title);
        assert_eq!((m.pinned, m.deleted), (n.pinned, n.deleted), "{}", n.title);
        assert!((m.created - n.created).abs() < 1e-3 && (m.modified - n.modified).abs() < 1e-3, "{}", n.title);
        // inline tokens kept their text
        let a: Vec<_> = n.body.tokens().map(|t| (&t.alt, &t.content)).collect();
        let b: Vec<_> = m.body.tokens().map(|t| (&t.alt, &t.content)).collect();
        assert_eq!(a, b, "{}", n.title);
        assert_eq!(m.attachments.len(), n.attachments.len(), "{}", n.title);
        for (x, y) in n.attachments.iter().zip(&m.attachments) {
            assert_eq!((&x.id, &x.uti, &x.table), (&y.id, &y.uti, &y.table));
            let bytes = |p: Option<&std::path::PathBuf>| p.map(|p| std::fs::read(p).unwrap());
            assert_eq!(bytes(x.display_file()), bytes(y.display_file()), "{}: {}", n.title, x.id);
            if let Some(f) = y.display_file() {
                assert!(f.starts_with(&d), "copied into the app's library");
            }
        }
        if n.title == "23 Collapsed Heading" {
            assert!(m.body.paragraphs[1].collapsed);
        }
    }
    // again: nothing new
    let again = s.import_library(&mac).unwrap();
    assert_eq!((again.notes, again.skipped_existing, again.folders), (0, 39, 0));
    check_z_max(&s.path());
}

#[test]
fn default_dir_is_xdg() {
    let p = store::default_dir();
    assert!(p.ends_with("bramble-notes"));
}

#[test]
fn save_note_as_the_editor_holds_it() {
    let r = research_or_skip!();
    let d = common::scratch("store-save");
    let mut s = Store::open(&d).unwrap();
    let mut b = Body::from_lines("Editor", &["text"]);
    let mut n = s.create_note(None, &mut b).unwrap();
    // the editor attaches a file and a table by pushing them onto the note
    let png = r.join("sample-files/inputs/bramble-gradient.png");
    let img = Attachment { id: pb_uuid(), kind: AttachmentKind::Image, uti: "public.png".into(), file: Some(png.clone()), ..Default::default() };
    let tbl = Attachment { id: pb_uuid(), kind: AttachmentKind::Table, uti: UTI_TABLE.into(), table: Some(Table::from_text(&[&["1", "2"]])), ..Default::default() };
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&img.id, &img.uti)], ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&tbl.id, &tbl.uti)], ..Default::default() });
    n.attachments.push(img.clone());
    n.attachments.push(tbl.clone());
    s.save_note(&mut n).unwrap();
    let a = n.attachment(&img.id).unwrap();
    assert!(a.file.as_ref().unwrap().starts_with(&d), "path now points at the library's copy");
    assert_eq!((a.width, a.height, a.name.as_str()), (320.0, 200.0, "bramble-gradient.png"));
    // edit the table in place and save again
    n.attachments.iter_mut().find(|x| x.id == tbl.id).unwrap().table = Some(Table::from_text(&[&["1", "2"], &["3", "4"]]));
    s.save_note(&mut n).unwrap();
    let back = s.note(&n.id).unwrap();
    assert_eq!(back.attachment(&tbl.id).unwrap().table.as_ref().unwrap().text_grid(), vec![vec!["1", "2"], vec!["3", "4"]]);
    assert_eq!(back.attachment(&tbl.id).unwrap().summary, "1\n2\n3\n4\n");
    assert_eq!(back.body, n.body);
    assert_eq!(back.attachments.len(), 2);
    check_z_max(&s.path());
}

fn pb_uuid() -> String {
    nt_core::pb::new_uuid()
}

/// Everything under `dir` (files only), relative.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path().strip_prefix(dir).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn removed_attachments_leave_nothing_behind() {
    let dir = common::scratch("store-unused-files");
    let mut st = Store::open(&dir).unwrap();
    let src = dir.join("in");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.txt"), b"hello").unwrap();
    std::fs::write(src.join("render.png"), b"\x89PNG\r\n\x1a\none").unwrap();
    let mut body = Body::from_lines("Files", &[]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let file = Attachment { id: pb_uuid(), kind: AttachmentKind::File, uti: "public.plain-text".into(), file: Some(src.join("a.txt")), ..Default::default() };
    let ink = Drawing { width: 768.0, height: 100.0, strokes: vec![Stroke { id: "s".into(), ink: InkKind::Pen, color: [0.0, 0.0, 0.0, 1.0], width: 3.0, points: (0..4).map(|i| InkPoint { x: 10.0 * i as f32, y: 20.0, t: 0.01 * i as f32, size: 3.0, force: 1.0, azimuth: 0.0, altitude: 1.5, opacity: 1.0 }).collect(), transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], mask: None }], shapes: Vec::new() };
    let draw = Attachment { id: pb_uuid(), kind: AttachmentKind::Drawing, uti: UTI_PAPER.into(), drawing: Some(ink), fallback: Some(src.join("render.png")), width: 768.0, height: 100.0, ..Default::default() };
    for a in [&file, &draw] {
        note.attachments.push(a.clone());
        note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, &a.uti)], ..Default::default() });
    }
    st.save_note(&mut note).unwrap();
    let acct = dir.join("Accounts/LocalAccount");
    let before = files_under(&acct);
    assert!(before.iter().any(|f| f.starts_with("Media/") && f.ends_with("a.txt")), "{before:?}");
    assert!(before.iter().any(|f| f.starts_with(&format!("FallbackImages/{}/", draw.id))));
    assert!(before.iter().any(|f| f.starts_with("Paper/Bundles/")));

    // redrawn twice: one fallback generation is kept
    for k in ["two", "three"] {
        std::fs::write(src.join(format!("{k}.png")), k).unwrap();
        let a = note.attachments.iter_mut().find(|a| a.id == draw.id).unwrap();
        a.drawing.as_mut().unwrap().strokes[0].points.pop();
        a.fallback = Some(src.join(format!("{k}.png")));
        st.save_note(&mut note).unwrap();
    }
    let gens: Vec<String> = files_under(&acct).into_iter().filter(|f| f.starts_with("FallbackImages/")).collect();
    assert_eq!(gens.len(), 1, "{gens:?}");
    assert!(gens[0].contains("/3_"), "{gens:?}");

    // the drawing deleted from the body (and the editor's list): its rows and files go
    note.body.paragraphs.retain(|p| !p.spans.iter().any(|s| s.token.as_ref().is_some_and(|t| t.id == draw.id)));
    note.attachments.retain(|a| a.id != draw.id);
    st.save_note(&mut note).unwrap();
    let left = files_under(&acct);
    assert!(!left.iter().any(|f| f.contains(&draw.id) || f.starts_with("Paper/")), "{left:?}");
    assert!(st.note(&note.id).unwrap().attachment(&draw.id).is_none());
    let rows: i64 = rusqlite::Connection::open(dir.join("NoteStore.sqlite"))
        .unwrap()
        .query_row("SELECT COUNT(*) FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1", [&draw.id], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0);

    // a body-only update just marks a dropped attachment (an undo can bring it back)
    let mut body = Body::from_lines("Files", &["no more attachments"]);
    st.update_note(&note.id, &mut body).unwrap();
    assert!(files_under(&acct).iter().any(|f| f.ends_with("a.txt")));
    // and the next save drops it with its media folder
    note.body = Body::from_lines("Files", &["no more attachments"]);
    note.attachments.clear();
    st.save_note(&mut note).unwrap();
    assert_eq!(files_under(&acct), Vec::<String>::new());
    assert!(st.note(&note.id).unwrap().attachments.is_empty());
}

#[test]
fn not_downloaded_attachments_stay_as_they_are() {
    let dir = common::scratch("store-not-downloaded");
    let mut st = Store::open(&dir).unwrap();
    let mut body = Body::from_lines("iCloud", &[]);
    let mut note = st.create_note(None, &mut body).unwrap();
    let gone = |uti: &str| Attachment { id: pb_uuid(), uti: uti.into(), kind: AttachmentKind::from_uti(uti), not_downloaded: true, ..Default::default() };
    for a in [gone(UTI_TABLE), gone("com.apple.notes.gallery"), gone(UTI_PAPER)] {
        note.attachments.push(a.clone());
        note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, &a.uti)], ..Default::default() });
    }
    st.save_note(&mut note).unwrap();
    for _ in 0..2 {
        let back = st.note(&note.id).unwrap();
        assert_eq!(back.attachments.len(), 3);
        assert!(back.attachments.iter().all(|a| a.not_downloaded && a.display_file().is_none() && a.table.is_none() && a.drawing.is_none()), "{:?}", back.attachments);
        // saving the note again leaves them be
        let mut again = back.clone();
        st.save_note(&mut again).unwrap();
    }
}

#[test]
fn the_list_loads_without_bodies_and_notes_open_fresh() {
    let dir = common::scratch("store-meta");
    let mut st = Store::open(&dir).unwrap();
    let png = dir.join("p.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03\x08\x02\0\0\0px").unwrap();
    let mut ids = Vec::new();
    for k in 0..5 {
        let mut body = Body::from_lines(&format!("Note {k}"), &["some text"]);
        let n = st.create_note(None, &mut body).unwrap();
        let mut note = st.note(&n.id).unwrap();
        if k % 2 == 0 {
            let a = Attachment { id: pb_uuid(), uti: "public.png".into(), file: Some(png.clone()), ..Default::default() };
            note.attachments.push(a.clone());
            note.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, &a.uti)], ..Default::default() });
        }
        let mut tag = Span::token("", UTI_HASHTAG);
        tag.token.as_mut().unwrap().alt = format!("#t{k}");
        note.body.paragraphs.push(Paragraph { spans: vec![tag], ..Default::default() });
        st.save_note(&mut note).unwrap();
        ids.push(n.id);
    }
    for (k, checked) in [(0, [true, true]), (1, [true, false])] {
        let mut n = st.note(&ids[k]).unwrap();
        for c in checked {
            n.body.paragraphs.push(Paragraph { style: ParaStyle::Checklist, checked: c, spans: vec![Span::plain("item")], ..Default::default() });
        }
        st.save_note(&mut n).unwrap();
    }
    let full = st.library().unwrap();
    let meta = st.library_meta().unwrap();
    assert_eq!(meta.notes.len(), full.notes.len());
    for (m, f) in meta.notes.iter().zip(&full.notes) {
        assert!(m.light && m.body.paragraphs.is_empty() && m.attachments.is_empty());
        assert_eq!((&m.id, &m.title, &m.snippet, m.pinned, m.locked, &m.folder), (&f.id, &f.title, &f.snippet, f.pinned, f.locked, &f.folder));
        assert_eq!((m.thumbnail(), m.tags()), (f.thumbnail(), f.tags()));
        let mut mk = m.attachment_kinds();
        let mut fk = f.attachment_kinds();
        mk.sort_by_key(|k| format!("{k:?}"));
        fk.sort_by_key(|k| format!("{k:?}"));
        fk.dedup();
        assert_eq!(mk, fk);
        let (fa, fd) = f.checklist_counts();
        assert_eq!(m.checklist_counts(), if fa == 0 { (0, 0) } else if fd == fa { (1, 1) } else { (1, 0) });
    }
    assert_eq!(meta.notes.iter().filter(|n| n.thumbnail().is_some()).count(), 3);

    // a note opened, edited, opened again: the cache follows every change
    let id = &ids[1];
    assert_eq!(st.load_body(id).unwrap().text(), st.note(id).unwrap().body.text());
    let mut n = st.note(id).unwrap();
    n.body = Body::from_lines("Changed", &["new text"]);
    st.save_note(&mut n).unwrap();
    assert!(st.note(id).unwrap().body.text().contains("new text"));
    let mut b = Body::from_lines("Again", &["by update_note"]);
    st.update_note(id, &mut b).unwrap();
    assert!(st.note(id).unwrap().body.text().contains("by update_note"));
    st.set_pinned(id, true).unwrap();
    assert!(st.note(id).unwrap().pinned);
    st.delete_note(id).unwrap();
    assert!(st.note(id).unwrap().deleted);
    let tid = st.add_table(&ids[2], &Table::from_text(&[&["a"]])).unwrap();
    let mut n2 = st.note(&ids[2]).unwrap();
    n2.body.paragraphs.push(Paragraph { spans: vec![Span::token(&tid, UTI_TABLE)], ..Default::default() });
    st.save_note(&mut n2).unwrap();
    st.update_table(&tid, &Table::from_text(&[&["b"]])).unwrap();
    assert_eq!(st.note(&ids[2]).unwrap().attachment(&tid).unwrap().table.as_ref().unwrap().text_grid(), vec![vec!["b"]]);
    assert!(st.load_attachments(&ids[2]).unwrap().iter().any(|a| a.id == tid));
}

#[test]
fn search_finds_note_text_and_follows_edits() {
    let dir = common::scratch("store-search");
    let mut st = Store::open(&dir).unwrap();
    let mut b1 = Body::from_lines("Groceries", &["Bramble JELLY and bread"]);
    let n1 = st.create_note(None, &mut b1).unwrap();
    let mut b2 = Body::from_lines("Plans", &["walk in the hills"]);
    let n2 = st.create_note(None, &mut b2).unwrap();
    assert_eq!(st.search("jelly").unwrap(), vec![n1.id.clone()]);
    assert_eq!(st.search("PLANS").unwrap(), vec![n2.id.clone()]);
    assert!(st.search("nowhere").unwrap().is_empty());
    // an edit is seen by the next search
    let mut b = Body::from_lines("Plans", &["walk in the hills, then jelly"]);
    st.update_note(&n2.id, &mut b).unwrap();
    assert_eq!(st.search("jelly").unwrap(), vec![n1.id.clone(), n2.id.clone()]);
    // locked: by title only
    st.set_password("pw", None).unwrap();
    st.lock_note(&n1.id, "pw").unwrap();
    assert_eq!(st.search("jelly").unwrap(), vec![n2.id.clone()]);
    assert_eq!(st.search("groceries").unwrap(), vec![n1.id.clone()]);
    // erased: gone
    st.erase_note(&n2.id).unwrap();
    assert!(st.search("hills").unwrap().is_empty());
}

#[test]
fn convert_a_folder_to_a_smart_folder() {
    let dir = common::scratch("store-convert-smart");
    let mut st = Store::open(&dir).unwrap();
    let f = st.create_folder("Gym Stats", None).unwrap();
    let mut ids = Vec::new();
    for (i, lines) in [vec!["Monday", "squats 5x5"], vec!["Tuesday"]].iter().enumerate() {
        let mut b = Body::from_lines(lines[0], &lines[1..]);
        let n = st.create_note(Some(&f), &mut b).unwrap();
        if i == 0 {
            st.add_tag(&n.id, "#GymStats").unwrap();
        }
        ids.push(n.id);
    }
    let q = |t: &str| format!(r#"{{"type":{{"and":[{{"deleted":false}},{{"and":[{{"tags":{{"included":["{t}"]}}}}]}}]}},"entity":"note"}}"#);
    assert_eq!(st.convert_to_smart_folder(&f, q).unwrap(), "GymStats");
    let lib = st.library().unwrap();
    for id in &ids {
        let n = lib.note(id).unwrap();
        assert_eq!((n.folder.as_str(), n.tags()), (store::DEFAULT_FOLDER, vec!["GymStats".to_string()]), "tagged once, in Notes");
    }
    // on the end of the last line
    let last = lib.note(&ids[0]).unwrap().body.paragraphs.last().unwrap().clone();
    assert!(last.spans.first().unwrap().text.starts_with("squats 5x5") && last.spans.last().unwrap().token.as_ref().is_some_and(|t| t.alt == "#GymStats"));
    assert!(lib.folder(&f).unwrap().smart.as_deref().is_some_and(|s| s.contains("GymStats")));
    let kind: i64 = rusqlite::Connection::open(dir.join("NoteStore.sqlite")).unwrap().query_row("SELECT ZFOLDERTYPE FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1", [&f], |r| r.get(0)).unwrap();
    assert_eq!(kind, 2);
    // refused: subfolders, locked notes
    let g = st.create_folder("Parent", None).unwrap();
    st.create_folder("Child", Some(&g)).unwrap();
    assert!(st.convert_to_smart_folder(&g, q).is_err());
    let h = st.create_folder("Private", None).unwrap();
    let mut b = Body::from_lines("Secret", &["x"]);
    let n = st.create_note(Some(&h), &mut b).unwrap();
    st.set_password("pw", None).unwrap();
    st.lock_note(&n.id, "pw").unwrap();
    assert!(st.convert_to_smart_folder(&h, q).is_err());
    assert!(st.add_tag(&n.id, "x").is_err());
    // set_smart_query alone makes a folder smart too
    let k = st.create_folder("Plain", None).unwrap();
    st.set_smart_query(&k, &q("x")).unwrap();
    let kind: i64 = rusqlite::Connection::open(dir.join("NoteStore.sqlite")).unwrap().query_row("SELECT ZFOLDERTYPE FROM ZICCLOUDSYNCINGOBJECT WHERE ZIDENTIFIER = ?1", [&k], |r| r.get(0)).unwrap();
    assert_eq!(kind, 2);
}

#[test]
fn trash_purge_and_attachments_browser() {
    let dir = common::scratch("store-purge");
    let mut st = Store::open(&dir).unwrap();
    let png = dir.join("p.png");
    std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x03\x08\x02\0\0\0px").unwrap();
    let mut ids = Vec::new();
    for k in 0..3 {
        let mut b = Body::from_lines(&format!("N{k}"), &["x"]);
        let n = st.create_note(None, &mut b).unwrap();
        let a = st.add_attachment(&n.id, &png).unwrap();
        let mut full = st.note(&n.id).unwrap();
        full.body.paragraphs.push(Paragraph { spans: vec![Span::token(&a.id, &a.uti)], ..Default::default() });
        st.save_note(&mut full).unwrap();
        ids.push(n.id);
    }
    st.delete_note(&ids[0]).unwrap();
    st.delete_note(&ids[1]).unwrap();
    // one of them went to the trash 40 days ago
    rusqlite::Connection::open(dir.join("NoteStore.sqlite"))
        .unwrap()
        .execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZFOLDERMODIFICATIONDATE = ZFOLDERMODIFICATIONDATE - 40 * 86400 WHERE ZIDENTIFIER = ?1", [&ids[0]])
        .unwrap();
    let meta = st.library_meta().unwrap();
    let ago = |id: &str| {
        let n = meta.note(id).unwrap();
        (body::now_unix() - n.trashed_at.unwrap()) / 86400.0
    };
    assert!(ago(&ids[0]) > 39.9 && ago(&ids[1]) < 0.1);
    assert!(meta.note(&ids[2]).unwrap().trashed_at.is_none());
    assert_eq!(st.note(&ids[1]).unwrap().trashed_at.is_some(), true);
    // the browser: attachments of notes not deleted
    let atts = st.attachments().unwrap();
    assert_eq!(atts.len(), 1);
    assert!(atts[0].0 == ids[2] && atts[0].1.kind == AttachmentKind::Image && atts[0].1.file.as_ref().is_some_and(|f| f.exists()));
    // the purge: only the old one, files and all
    let file = st.note(&ids[0]).unwrap().attachments[0].file.clone().unwrap();
    assert_eq!(st.purge_trash(30.0).unwrap(), 1);
    assert!(st.note(&ids[0]).is_err() && st.note(&ids[1]).is_ok() && !file.exists());
    assert_eq!(st.purge_trash(30.0).unwrap(), 0);
}
