//! Importing a Mac Notes library, using the research's NoteStore (Bramble Research rows only) laid
//! out as a copied group.com.apple.notes folder.

mod common;

use std::path::{Path, PathBuf};

use nt_core::import;
use nt_core::model::*;

const ACCOUNT: &str = "LocalAccount";

/// The common container plus a preview image for 28 Image.
pub fn group_container(r: &Path, name: &str) -> PathBuf {
    let d = common::group_container(r, name);
    let pv = d.join("Accounts").join(ACCOUNT).join("Previews/EB5DCE00-5ED3-4664-ACDD-19C1D0C64547-1-384x240-0/1_BEA09F3B-0C42-4A3E-B857-F396423268B1");
    std::fs::create_dir_all(&pv).unwrap();
    std::fs::copy(r.join("sample-files/exports/markdown/Attachments/EB5DCE00-5ED3-4664-ACDD-19C1D0C64547.png"), pv.join("Preview.png")).unwrap();
    d
}

/// (path, size, mtime, content hash) of every file under `d`.
fn snapshot(d: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime, u64)> {
    use std::hash::{Hash, Hasher};
    let mut out = Vec::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let m = e.metadata().unwrap();
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::fs::read(&path).unwrap().hash(&mut h);
                out.push((path, m.len(), m.modified().unwrap(), h.finish()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn imports_every_note_with_research_text() {
    let r = research_or_skip!();
    let d = group_container(&r, "import-all");
    let before = snapshot(&d);
    let (lib, rep) = import::read_library_with_report(&d).unwrap();
    assert_eq!(snapshot(&d), before, "the Apple store folder must not change");

    assert_eq!(lib.accounts, vec![Account { id: ACCOUNT.into(), name: "On My Mac".into(), kind: 3 }]);
    assert_eq!(lib.notes.len(), 39);
    assert_eq!(rep.notes, 39);
    assert!(rep.unreadable.is_empty(), "{:?}", rep.unreadable);
    assert!(rep.missing_files.is_empty(), "{:?}", rep.missing_files);
    // every decoded sample's text, title and dates match its note
    let mut checked = 0;
    for e in std::fs::read_dir(r.join("format-analysis/decoded")).unwrap().flatten() {
        let j = common::json(&e.path());
        let row = &j["note_row"];
        let id = row["ZIDENTIFIER"].as_str().unwrap();
        let n = lib.note(id).unwrap_or_else(|| panic!("note {id} missing"));
        assert_eq!(n.body.text(), j["document"]["document"]["note"]["string"].as_str().unwrap_or(""), "{}", n.title);
        assert_eq!(n.title, row["ZTITLE1"].as_str().unwrap());
        assert_eq!(n.snippet, row["ZSNIPPET"].as_str().unwrap_or(""));
        let created = row["ZCREATIONDATE3"].as_f64().unwrap() + 978_307_200.0;
        assert!((n.created - created).abs() < 1e-3, "{}", n.title);
        assert!((n.modified - (row["ZMODIFICATIONDATE1"].as_f64().unwrap() + 978_307_200.0)).abs() < 1e-3);
        assert_eq!(n.pinned, row["ZISPINNED"].as_i64() == Some(1), "{}", n.title);
        checked += 1;
    }
    assert!(checked >= 37);
}

#[test]
fn folders_subfolders_smart_and_trash() {
    let r = research_or_skip!();
    let lib = import::read_library(&group_container(&r, "import-folders")).unwrap();
    let by_name = |n: &str| lib.folders.iter().find(|f| f.name == n).unwrap_or_else(|| panic!("folder {n}"));
    assert_eq!(lib.folders.len(), 5);
    assert!(by_name("Notes").default);
    assert_eq!(by_name("Recently Deleted").kind, FolderKind::Trash);
    let research = by_name("Bramble Research");
    let sub = by_name("B-Sub");
    assert_eq!(sub.parent.as_deref(), Some(research.id.as_str()));
    let smart = by_name("B-Smart Pinned");
    assert_eq!(smart.kind, FolderKind::Smart);
    assert_eq!(smart.smart.as_deref(), Some(r#"{"type":{"and":[{"deleted":false},{"and":[{"pinned":true}]}]},"entity":"note"}"#));
    assert!(lib.folders.iter().all(|f| f.account == ACCOUNT));
    // notes in Recently Deleted
    let deleted: Vec<_> = lib.notes.iter().filter(|n| n.deleted).map(|n| n.title.as_str()).collect();
    assert_eq!(deleted, ["01 Plain", "21 Tag"]);
    assert!(lib.notes.iter().filter(|n| n.deleted).all(|n| n.folder == by_name("Recently Deleted").id));
    let subnote = lib.notes.iter().find(|n| n.title == "B-Sub Note").unwrap();
    assert_eq!(subnote.folder, sub.id);
    assert_eq!(lib.tags, vec![Tag { account: ACCOUNT.into(), name: "brambletest".into() }]);
    let pinned: Vec<_> = lib.notes.iter().filter(|n| n.pinned).map(|n| n.title.as_str()).collect();
    assert_eq!(pinned, ["32 Pinned"]);
}

fn note<'a>(lib: &'a Library, title: &str) -> &'a Note {
    lib.notes.iter().find(|n| n.title == title && !n.deleted).unwrap_or_else(|| panic!("note {title}"))
}

#[test]
fn attachments_and_files() {
    let r = research_or_skip!();
    let d = group_container(&r, "import-attachments");
    let lib = import::read_library(&d).unwrap();
    let exp = r.join("sample-files/exports/markdown/Attachments");

    let img = &note(&lib, "28 Image").attachments;
    assert_eq!(img.len(), 1);
    assert_eq!((img[0].kind, img[0].name.as_str(), img[0].width, img[0].height), (AttachmentKind::Image, "bramble-gradient.png", 320.0, 200.0));
    assert_eq!(std::fs::read(img[0].file.as_ref().unwrap()).unwrap(), std::fs::read(exp.join("EB5DCE00-5ED3-4664-ACDD-19C1D0C64547.png")).unwrap());
    assert!(img[0].preview.as_ref().unwrap().ends_with("Preview.png"));

    let pdf = &note(&lib, "29 PDF").attachments[0];
    assert_eq!((pdf.kind, pdf.name.as_str()), (AttachmentKind::Pdf, "bramble-doc.pdf"));
    assert!(pdf.file.as_ref().unwrap().is_file());
    let file = &note(&lib, "30 File").attachments[0];
    assert_eq!((file.kind, file.name.as_str()), (AttachmentKind::File, "bramble-text.txt"));
    assert_eq!(std::fs::read_to_string(file.file.as_ref().unwrap()).unwrap(), std::fs::read_to_string(r.join("sample-files/inputs/bramble-text.txt")).unwrap());

    let draw = &note(&lib, "33 Drawing").attachments[0];
    assert_eq!(draw.kind, AttachmentKind::Drawing);
    assert!(draw.file.is_none());
    assert_eq!(note(&lib, "33 Drawing").thumbnail(), draw.fallback.as_deref());
    assert!(note(&lib, "28 Image").thumbnail().unwrap().ends_with("Preview.png"));
    assert!(note(&lib, "01 Plain").thumbnail().is_none());
    let research = lib.folders.iter().find(|f| f.name == "Bramble Research").unwrap();
    assert_eq!(lib.note_count(&research.id), 36, "39 notes minus 2 deleted and B-Sub Note");
    assert!(draw.fallback.as_ref().unwrap().ends_with("FallbackImage.png"));
    assert_eq!(draw.display_file(), draw.fallback.as_ref());

    let t = &note(&lib, "19 Table").attachments[0];
    assert_eq!(t.kind, AttachmentKind::Table);
    assert_eq!(t.table.as_ref().unwrap().text_grid(), vec![vec!["A1", "B1"], vec!["A2", ""]]);
    assert_eq!(t.summary, "A1\nB1\nA2\n\n");
    // every body token that is a block attachment has its attachment
    for n in &lib.notes {
        for tok in n.body.tokens().filter(|t| !t.is_inline()) {
            assert!(n.attachment(&tok.id).is_some(), "{}: {}", n.title, tok.id);
        }
    }
}

#[test]
fn inline_tokens_and_collapse() {
    let r = research_or_skip!();
    let lib = import::read_library(&group_container(&r, "import-tokens")).unwrap();
    let tok = |title: &str| note(&lib, title).body.tokens().cloned().collect::<Vec<_>>();
    let tag = &tok("21 Tag")[0];
    assert_eq!((tag.kind(), tag.alt.as_str(), tag.content.as_str()), (TokenKind::Tag, "#brambletest", "BRAMBLETEST"));
    assert_eq!(note(&lib, "21 Tag").tags(), ["brambletest"]);
    let link = &tok("25 Note Link")[0];
    assert_eq!(link.alt, "B-Target Zqx → Zqx Section");
    let target = note(&lib, "B-Target Zqx");
    let TokenKind::NoteLink { note: to, paragraph } = link.kind() else { panic!("{link:?}") };
    assert_eq!(to, target.id);
    // the section link points at the target's heading paragraph
    assert_eq!(paragraph.as_deref(), Some(target.body.paragraphs[1].id.as_str()));
    assert_eq!(tok("20 Divider")[0].kind(), TokenKind::Divider);
    assert_eq!(tok("20 Divider")[0].alt, "---");
    let math = tok("26 Math");
    assert_eq!(math.iter().map(|t| t.alt.as_str()).collect::<Vec<_>>(), ["\u{200e}\u{2009}=\u{2009}14", "\u{200e}\u{2009}=\u{2009}10"]);
    assert_eq!(math[0].kind(), TokenKind::Math);
    // collapsed heading
    let c = note(&lib, "23 Collapsed Heading");
    assert!(c.body.paragraphs[1].collapsed);
    assert_eq!(c.body.paragraphs[1].style, ParaStyle::Heading);
    assert!(!c.body.paragraphs[2].collapsed);
}

#[test]
fn bare_sqlite_and_wal_changes() {
    let r = research_or_skip!();
    // only the .sqlite: notes import, files are reported missing
    let d = common::scratch("import-bare");
    let db = d.join("NoteStore.sqlite");
    std::fs::copy(r.join("sample-files/store/NoteStore-BrambleResearch-only.sqlite"), &db).unwrap();
    let (lib, rep) = import::read_library_with_report(&db).unwrap();
    assert_eq!(lib.notes.len(), 39);
    assert_eq!(rep.missing_files.len(), 4, "{:?}", rep.missing_files);

    // a change that is still only in the -wal is seen, and neither file changes
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        c.execute("UPDATE ZICCLOUDSYNCINGOBJECT SET ZTITLE1 = 'Renamed in WAL' WHERE ZIDENTIFIER = '513397FD-587E-4E97-A4AC-19E1363AA28C'", [])
            .unwrap();
        // copy the store as Notes would leave it (connection still open, WAL not checkpointed)
        let live = common::scratch("import-wal");
        for f in ["NoteStore.sqlite", "NoteStore.sqlite-wal", "NoteStore.sqlite-shm"] {
            std::fs::copy(d.join(f), live.join(f)).unwrap();
        }
        let before = snapshot(&live);
        let lib = import::read_library(&live).unwrap();
        assert_eq!(lib.note("513397FD-587E-4E97-A4AC-19E1363AA28C").unwrap().title, "Renamed in WAL");
        assert_eq!(snapshot(&live), before);
    }
}

#[test]
fn not_a_notes_store() {
    let d = common::scratch("import-bad");
    let db = d.join("x.sqlite");
    rusqlite::Connection::open(&db).unwrap().execute_batch("CREATE TABLE Z_PRIMARYKEY (Z_ENT INTEGER, Z_NAME VARCHAR, Z_SUPER INTEGER, Z_MAX INTEGER);").unwrap();
    assert!(import::read_library(&db).is_err());
    assert!(import::read_library(&d).is_err(), "a folder without NoteStore.sqlite");
}
