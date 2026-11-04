//! Importing an UpNote library (a made-up one, in UpNote's own layout), and finding the notes it
//! repeats.

mod common;

use std::collections::HashSet;
use std::path::Path;

use nt_core::model::*;
use nt_core::store::{Store, DEFAULT_FOLDER, TRASH_FOLDER};
use nt_core::upnote;

/// An UpNote folder: upnote.sqlite3 (its tables as UpNote has them) and images/.
fn upnote_folder(dir: &Path, notes: &[(&str, &str, bool, bool, &str)]) {
    std::fs::create_dir_all(dir.join("images")).unwrap();
    std::fs::write(dir.join("images/PIC-1.png"), b"\x89PNG\r\n\x1a\nmade-up picture").unwrap();
    let c = rusqlite::Connection::open(dir.join("upnote.sqlite3")).unwrap();
    c.execute_batch(
        "CREATE TABLE notes (id TEXT PRIMARY KEY, html TEXT, text TEXT, title TEXT, pinned INTEGER, bookmarked INTEGER,
           trashed INTEGER, deleted INTEGER, createdAt REAL, updatedAt REAL, tagLinks TEXT, fileIds TEXT, hasTodo INTEGER, isTemplate INTEGER);
         CREATE TABLE files (id TEXT, updatedAt REAL, createdAt REAL, syncedAt REAL, synced INTEGER, deleted INTEGER, revision INTEGER,
           name TEXT, version INTEGER, downloadURL TEXT, tag TEXT);
         INSERT INTO files (id, name, downloadURL, deleted) VALUES ('PIC-2__png', 'PIC-2.png', 'https://example.invalid/PIC-2', 0);",
    )
    .unwrap();
    for (i, (id, html, pinned, trashed, tags)) in notes.iter().enumerate() {
        c.execute(
            "INSERT INTO notes (id, html, title, pinned, trashed, deleted, createdAt, updatedAt, tagLinks) VALUES (?1, ?2, '', ?3, ?4, 0, ?5, ?6, ?7)",
            rusqlite::params![id, html, pinned, trashed, 1_700_000_000_000.0 + i as f64 * 1000.0, 1_700_000_500_000.0 + i as f64 * 1000.0, tags],
        )
        .unwrap();
    }
    // a note deleted for good in UpNote: not taken
    c.execute("INSERT INTO notes (id, html, deleted, createdAt, updatedAt) VALUES ('GONE', '<div>gone</div>', 1, 0, 0)", []).unwrap();
}

const SHOPPING: &str = "<h2>Shopping list</h2><ul><li data-checked=\"true\">eggs and milk</li><li data-checked=\"false\">bread for the week</li></ul><div>Remember the <b>market</b> on <a href=\"https://example.invalid\">Saturday</a> <a data-upnote-tag=\"food\" href=\"upnote://x-callback-url/tag/view?tag=food\">#food</a></div>";
const TRIP: &str = "<h2>Trip plans</h2><div>Book the train early and pack light for the mountains this year</div><table><tbody><tr><td>Day</td><td>Place</td></tr><tr><td>1</td><td>Hut</td></tr></tbody></table><img src=\"http://localhost:9425/images/PIC-1.png\"><img src=\"http://localhost:9425/images/PIC-2.png\"><div><a href=\"upnote://x-callback-url/openNote?noteId=UP-1\" data-note-id=\"UP-1\">Shopping list</a></div>";

fn import_all(st: &mut Store, dir: &Path) -> upnote::UpNoteImport {
    upnote::import(st, dir).unwrap()
}

#[test]
fn an_upnote_library_comes_in_once() {
    let dir = common::scratch("upnote-import");
    let up = dir.join("UpNote");
    upnote_folder(&up, &[("UP-1", SHOPPING, true, false, "[\"food\",\"errands\"]"), ("UP-2", TRIP, false, false, "[]"), ("UP-3", "<div>Old idea</div>", false, true, "[]")]);
    let mut st = Store::open_library(&dir.join("lib")).unwrap();
    let r = import_all(&mut st, &up);
    assert_eq!((r.notes, r.trashed, r.skipped, r.pictures, r.links, r.tables), (3, 1, 0, 1, 1, 1));

    let lib = st.library().unwrap();
    let folder = lib.folders.iter().find(|f| f.name == "UpNote").unwrap();
    assert_eq!(folder.account, nt_core::store::ACCOUNT_ID);
    let shop = lib.note(&upnote::note_id("UP-1")).unwrap();
    assert_eq!(shop.folder, folder.id);
    assert!(shop.pinned);
    assert_eq!(shop.title, "Shopping list");
    assert_eq!(shop.created, 1_700_000_000.0);
    assert_eq!(shop.modified, 1_700_000_500.0);
    let mut tags = shop.tags();
    tags.sort();
    assert_eq!(tags, vec!["errands".to_string(), "food".into()]);
    let items: Vec<(ParaStyle, bool, String)> = shop.body.paragraphs.iter().filter(|p| p.style == ParaStyle::Checklist).map(|p| (p.style, p.checked, p.text())).collect();
    assert_eq!(items, vec![(ParaStyle::Checklist, true, "eggs and milk".into()), (ParaStyle::Checklist, false, "bread for the week".into())]);
    let line = shop.body.paragraphs.iter().find(|p| p.text().starts_with("Remember")).unwrap();
    assert!(line.spans.iter().any(|s| s.bold && s.text == "market"));
    assert!(line.spans.iter().any(|s| s.link.as_deref() == Some("https://example.invalid")));

    let trip = lib.note(&upnote::note_id("UP-2")).unwrap();
    let kinds: Vec<AttachmentKind> = trip.attachments.iter().map(|a| a.kind).collect();
    assert!(kinds.contains(&AttachmentKind::Table) && kinds.contains(&AttachmentKind::Image) && kinds.contains(&AttachmentKind::Url));
    let pic = trip.attachments.iter().find(|a| a.kind == AttachmentKind::Image).unwrap();
    assert_eq!(std::fs::read(pic.file.as_ref().unwrap()).unwrap(), std::fs::read(up.join("images/PIC-1.png")).unwrap());
    let table = trip.attachments.iter().find(|a| a.kind == AttachmentKind::Table).unwrap();
    assert_eq!(table.table.as_ref().unwrap().text_grid()[1], vec!["1".to_string(), "Hut".into()]);
    let link = trip.body.tokens().find(|t| t.uti == UTI_NOTE_LINK).unwrap();
    assert!(link.content.contains(&upnote::note_id("UP-1")), "links between notes point at the imported note");

    let old = lib.note(&upnote::note_id("UP-3")).unwrap();
    assert!(old.deleted && old.folder == TRASH_FOLDER, "trashed in UpNote: in Recently Deleted");
    assert!(lib.notes.iter().all(|n| n.title != "gone"));

    // again: nothing new, nothing twice (even a note erased here since stays out)
    st.erase_note(&upnote::note_id("UP-3")).unwrap();
    let r = import_all(&mut st, &up);
    assert_eq!((r.notes, r.skipped), (0, 3));
    assert_eq!(st.library().unwrap().notes.len(), 2);
    // a new note in UpNote since: only it comes in
    let c = rusqlite::Connection::open(up.join("upnote.sqlite3")).unwrap();
    c.execute("INSERT INTO notes (id, html, deleted, trashed, pinned, createdAt, updatedAt, tagLinks) VALUES ('UP-4', '<div>New one</div>', 0, 0, 0, 1, 1, '[]')", []).unwrap();
    drop(c);
    let r = import_all(&mut st, &up);
    assert_eq!((r.notes, r.skipped), (1, 3));

    // and the files say the same (the library rebuilt from them)
    drop(st);
    std::fs::remove_dir_all(dir.join("lib/index")).unwrap();
    let st = Store::open_library(&dir.join("lib")).unwrap();
    assert_eq!(st.library().unwrap().note(&upnote::note_id("UP-1")).unwrap().tags().len(), 2);
}

#[test]
fn notes_upnote_repeats_are_found() {
    let dir = common::scratch("upnote-dupes");
    let up = dir.join("UpNote");
    upnote_folder(&up, &[("UP-1", SHOPPING, false, false, "[]"), ("UP-2", TRIP, false, false, "[]")]);
    let mut st = Store::open_library(&dir.join("lib")).unwrap();
    // the Mac's notes: the same list (other case and punctuation), the trip with a line more and
    // newer, the list with a drawing, and one UpNote doesn't have
    let mac = |st: &mut Store, lines: &[&str]| -> String {
        let mut b = Body::from_lines(lines[0], &lines[1..]);
        st.create_note(Some(DEFAULT_FOLDER), &mut b).unwrap().id
    };
    let same = mac(&mut st, &["SHOPPING LIST", "Eggs and milk!", "Bread for the week.", "Remember the market on Saturday #food"]);
    let longer = mac(&mut st, &["Trip plans", "Book the train early and pack light for the mountains this year", "Day Place 1 Hut", "Shopping list", "Maps"]);
    let drawn = mac(&mut st, &["Shopping list", "eggs and milk", "bread for the week", "remember the market on saturday food"]);
    let mut n = st.note(&drawn).unwrap();
    let did = nt_core::pb::new_uuid();
    n.attachments.push(Attachment { id: did.clone(), uti: UTI_PAPER.into(), kind: AttachmentKind::Drawing, drawing: Some(Drawing { width: 100.0, height: 50.0, ..Default::default() }), width: 100.0, height: 50.0, ..Default::default() });
    n.body.paragraphs.push(Paragraph { spans: vec![Span::token(&did, UTI_PAPER)], ..Default::default() });
    st.save_note(&mut n).unwrap();
    let other = mac(&mut st, &["Recipes", "Something else entirely"]);
    // the longer one is newer than its UpNote copy
    import_all(&mut st, &up);

    let found = upnote::duplicates(&st, &upnote::imported_note_ids(&st)).unwrap();
    let get = |id: &str| found.iter().find(|d| d.note == id);
    let s = get(&same).expect("the same list");
    assert!(s.keep.is_none() && s.similarity >= upnote::SIMILAR && s.upnote == upnote::note_id("UP-1"), "{s:?}");
    let l = get(&longer).expect("the longer trip");
    assert!(l.keep.as_deref().is_some_and(|k| k.contains("newer") && k.contains("words")), "{l:?}");
    let d = get(&drawn).expect("the list with a drawing");
    assert!(d.keep.as_deref().is_some_and(|k| k.contains("drawings")), "{d:?}");
    assert!(get(&other).is_none());
    let imported: HashSet<String> = upnote::imported_note_ids(&st);
    assert!(found.iter().all(|d| !imported.contains(&d.note)), "UpNote's own never count");
}
