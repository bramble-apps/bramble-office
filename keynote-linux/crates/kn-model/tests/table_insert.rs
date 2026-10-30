mod common;
use std::collections::{HashMap, HashSet};

use common::*;

use kn_format::{KeyFile, Msg, Val};
use kn_model::edit::History;
use kn_model::insert::NewObject;
use kn_model::tablecells::CellInput;
use kn_model::{Document, DrawableKind};

fn uuid16(m: &Msg) -> Option<[u8; 16]> {
    let mut b = [0u8; 16];
    match m.type_name() {
        "TSP.UUID" => {
            b[..8].copy_from_slice(&m.u64("lower").unwrap_or(0).to_le_bytes());
            b[8..].copy_from_slice(&m.u64("upper").unwrap_or(0).to_le_bytes());
        }
        "TSP.CFUUIDArchive" => {
            for (i, w) in ["uuid_w0", "uuid_w1", "uuid_w2", "uuid_w3"].iter().enumerate() {
                b[i * 4..i * 4 + 4].copy_from_slice(&(m.u64(w).unwrap_or(0) as u32).to_le_bytes());
            }
        }
        _ => return None,
    }
    Some(b)
}

fn walk_uuids(m: &Msg, out: &mut Vec<[u8; 16]>) {
    if let Some(u) = uuid16(m) {
        out.push(u);
        return;
    }
    for f in &m.fields {
        if let Val::Msg(s) = &f.val {
            walk_uuids(s, out);
        }
    }
}

/// UUID families (all but the low 32 bits) used anywhere in the document's archives.
fn families(k: &KeyFile) -> HashSet<[u8; 12]> {
    let mut v = Vec::new();
    for (_, a) in k.archives() {
        for p in &a.payloads {
            walk_uuids(p, &mut v);
        }
        for e in k.package_metadata().unwrap().msgs("components").flat_map(|c| c.msgs("object_uuid_map_entries")) {
            if let Some(u) = e.msg("uuid") {
                walk_uuids(u, &mut v);
            }
        }
    }
    for (_, m) in k.objects_of("TST.TableModelArchive") {
        if let Some(u) = m.str("table_id").and_then(|s| uuid::Uuid::parse_str(s).ok()) {
            v.push(*u.as_bytes());
        }
    }
    v.into_iter().map(|b| b[4..].try_into().unwrap()).filter(|f| *f != [0u8; 12]).collect()
}

/// Uniqueness / consistency problems: object uuids, table ids, the engine's owner registry.
fn uuid_problems(k: &KeyFile) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut seen: HashMap<[u8; 16], u64> = HashMap::new();
    for c in k.package_metadata().unwrap().msgs("components") {
        for e in c.msgs("object_uuid_map_entries") {
            let u = e.msg("uuid").and_then(uuid16).unwrap();
            if let Some(o) = seen.insert(u, e.u64("identifier").unwrap()) {
                out.insert(format!("object uuid of #{} also used by #{o}", e.u64("identifier").unwrap()));
            }
        }
    }
    let mut ids = HashSet::new();
    let mut tables = HashSet::new();
    for (id, m) in k.objects_of("TST.TableModelArchive") {
        let t = m.str("table_id").unwrap().to_string();
        if !ids.insert(t.clone()) {
            out.insert(format!("table_id {t} of #{id} not unique"));
        }
        tables.insert(*uuid::Uuid::parse_str(&t).unwrap().as_bytes());
    }
    let (ce, cem) = k.first_of("TSCE.CalculationEngineArchive").unwrap();
    let tr = cem.msg("dependency_tracker").unwrap();
    let mut owners: HashMap<u64, [u8; 16]> = HashMap::new();
    let mut owner_uuids = HashSet::new();
    for e in tr.msg("owner_id_map").unwrap().msgs("map_entry") {
        let i = e.u64("internal_owner_id").unwrap();
        let u = uuid16(e.msg("owner_id").unwrap()).unwrap();
        if owners.insert(i, u).is_some() || !owner_uuids.insert(u) {
            out.insert(format!("owner {i} registered twice"));
        }
    }
    let mut fod_ids = HashSet::new();
    for f in tr.ref_ids("formula_owner_dependencies") {
        let Some(m) = k.obj(f) else {
            out.insert(format!("engine #{ce} -> missing owner dependencies #{f}"));
            continue;
        };
        let i = m.u64("internal_formula_owner_id").unwrap();
        if !fod_ids.insert(i) {
            out.insert(format!("owner {i} has two dependency archives"));
        }
        let u = uuid16(m.msg("formula_owner_uid").unwrap()).unwrap();
        if owners.get(&i) != Some(&u) {
            out.insert(format!("dependencies #{f}: owner {i} uuid doesn't match the owner map"));
        }
        for t in m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default() {
            if k.obj(t).and_then(|t| t.u64("internal_owner_id")) != Some(i) {
                out.insert(format!("tile #{t} of owner {i} has another owner"));
            }
        }
    }
    if let Some(h) = cem.ref_id("header_name_manager").and_then(|h| k.obj(h)) {
        let mut cols = HashSet::new();
        for p in h.msgs("per_tables") {
            let u = uuid16(p.msg("table_uid").unwrap()).unwrap();
            if !tables.contains(&u) {
                out.insert("header name entry for a table that doesn't exist".into());
            }
            if !cols.insert(p.msg("per_table_precedent").and_then(|c| c.u64("column"))) {
                out.insert("two tables share a header name precedent".into());
            }
        }
    }
    out
}

fn tables_on(doc: &Document, slide_index: usize) -> Vec<(u64, kn_model::TableData)> {
    let sc = doc.scene(&doc.slides(), slide_index).unwrap();
    sc.drawables
        .into_iter()
        .filter_map(|d| match d.kind {
            DrawableKind::Table(t) => Some((d.id, t)),
            _ => None,
        })
        .collect()
}

fn entry_bytes(k: &KeyFile) -> Vec<(String, Vec<u8>)> {
    k.entries
        .iter()
        .map(|e| {
            let b = match &e.data {
                kn_format::EntryData::Iwa(i) => i.encode_stream(),
                kn_format::EntryData::Raw(r) => r.len().to_le_bytes().to_vec(),
                kn_format::EntryData::Lazy(_, i) => i.to_le_bytes().to_vec(),
            };
            (e.name.clone(), b)
        })
        .collect()
}

/// Insert a Basic table on `slide_index`, check it end to end, return the saved document.
fn insert_table(mut doc: Document, slide_index: usize, twice: bool) -> Document {
    let before = problems(&doc.file);
    let before_uuid = uuid_problems(&doc.file);
    let fam_before = families(&doc.file);
    let donor = KeyFile::open(testdata("ref/R2-table-values.key")).unwrap();
    let fam_donor = families(&donor);
    let original = entry_bytes(&doc.file);
    let slides = doc.slides();
    let slide = slides[slide_index].slide;
    let n_tables = tables_on(&doc, slide_index).len();
    let mut h = History::default();
    let mut ids = Vec::new();
    for _ in 0..if twice { 2 } else { 1 } {
        doc.begin(&mut h, "Insert Table");
        ids.push(doc.insert(&mut h, slide, &NewObject::BASIC_TABLE).unwrap());
        doc.end(&mut h);
    }

    let mut doc2 = roundtrip(&doc, &before);
    let new_uuid: Vec<_> = uuid_problems(&doc2.file).difference(&before_uuid).cloned().collect();
    assert!(new_uuid.is_empty(), "uuid problems: {new_uuid:#?}");
    // every uuid the table brought is fresh (none of the donor's, none already in the document)
    let fresh: HashSet<_> = families(&doc2.file).difference(&fam_before).copied().collect();
    assert!(!fresh.is_empty());
    assert!(fresh.is_disjoint(&fam_donor), "donor uuids copied unchanged");

    let tabs = tables_on(&doc2, slide_index);
    assert_eq!(tabs.len(), n_tables + ids.len());
    let size = doc2.slide_size();
    for id in &ids {
        let (_, t) = tabs.iter().find(|(i, _)| i == id).expect("new table renders");
        assert_eq!((t.rows, t.cols, t.header_rows), (5, 4, 1));
        assert_eq!(t.cells.len(), 20);
        assert!(t.cells.iter().all(|c| c.is_empty()));
        let sc = doc2.scene(&doc2.slides(), slide_index).unwrap();
        let d = sc.drawables.iter().find(|d| d.id == *id).unwrap();
        assert!((d.geom.x + d.geom.w / 2.0 - size.w / 2.0).abs() < 2.0, "table not centred");
    }

    // cell values can be typed into it
    let mut h2 = History::default();
    doc2.begin(&mut h2, "Cell");
    doc2.set_cell(&mut h2, ids[0], 0, 0, &CellInput::Text("Item".into())).unwrap();
    doc2.set_cell(&mut h2, ids[0], 0, 1, &CellInput::Text("Count".into())).unwrap();
    doc2.set_cell(&mut h2, ids[0], 1, 0, &CellInput::Text("Apples".into())).unwrap();
    doc2.set_cell(&mut h2, ids[0], 1, 1, &CellInput::Number(42.0)).unwrap();
    doc2.end(&mut h2);
    let doc3 = roundtrip(&doc2, &before);
    let (_, t) = tables_on(&doc3, slide_index).into_iter().find(|(i, _)| *i == ids[0]).unwrap();
    assert_eq!(&t.cells[..2], &["Item".to_string(), "Count".to_string()]);
    assert_eq!(&t.cells[4..6], &["Apples".to_string(), "42".to_string()]);

    // undo takes every created object and file away again
    while doc.undo(&mut h) {}
    assert!(entry_bytes(&doc.file) == original, "undo didn't restore the document exactly");
    doc3
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_table_minimal() {
    let doc = Document::open(testdata("00-base.key")).unwrap();
    insert_table(doc, 0, true);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_table_catalog() {
    let doc = Document::open(testdata("Keynote-Feature-Catalog.key")).unwrap();
    insert_table(doc, 1, false);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn insert_table_lazy_media_deck() {
    let p = testdata("../testdata-private/real-deck.key");
    if !p.exists() {
        eprintln!("skipped: {} not present", p.display());
        return;
    }
    let doc = Document::open(p).unwrap();
    insert_table(doc, 0, false);
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn only_default_size_for_now() {
    let mut doc = Document::open(testdata("00-base.key")).unwrap();
    let slide = doc.slides()[0].slide;
    let mut h = History::default();
    doc.begin(&mut h, "Insert Table");
    assert!(doc.insert(&mut h, slide, &NewObject::Table { rows: 3, cols: 3 }).is_err());
    doc.end(&mut h);
}

