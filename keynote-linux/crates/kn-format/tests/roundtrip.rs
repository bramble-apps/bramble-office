use kn_format::{EntryData, IwaFile, KeyFile};
use std::path::PathBuf;

fn testdata() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

fn samples() -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(testdata())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "key"))
        .collect();
    v.sort();
    v
}

/// Every .iwa decodes and re-encodes to the identical decompressed stream.
#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn lossless_roundtrip_all_samples() {
    for path in samples() {
        let raw = std::fs::read(&path).unwrap();
        let k = KeyFile::from_zip(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&raw)).unwrap();
        for e in &k.entries {
            if let EntryData::Iwa(iwa) = &e.data {
                let mut f = z.by_name(&e.name).unwrap();
                let mut orig = Vec::new();
                std::io::Read::read_to_end(&mut f, &mut orig).unwrap();
                let orig = kn_format::wire::iwa_unframe(&orig).unwrap();
                assert!(iwa.encode_stream() == orig, "{}: {} differs", path.display(), e.name);
            }
        }
        // full write -> read -> write is stable
        let bytes = k.to_zip().unwrap();
        let k2 = KeyFile::from_zip(&bytes).unwrap();
        assert_eq!(k2.entries.len(), k.entries.len());
        assert!(k2.to_zip().unwrap() == bytes, "{} not stable", path.display());
    }
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn every_object_type_is_known() {
    let k = KeyFile::open(testdata().join("Keynote-Feature-Catalog.key")).unwrap();
    let mut unknown = 0;
    let mut total = 0;
    for (_, a) in k.archives() {
        for p in &a.payloads {
            total += 1;
            if p.ty.is_none() {
                unknown += 1;
            }
        }
    }
    assert!(total > 3000, "only {total} objects");
    // the research found exactly 3 messages with type 0 (unregistered)
    assert!(unknown <= 3, "{unknown} of {total} payloads have unknown types");
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn reads_slide_tree() {
    let k = KeyFile::open(testdata().join("Keynote-Feature-Catalog.key")).unwrap();
    let (_, show) = k.first_of("KN.ShowArchive").unwrap();
    let nodes = show.msg("slideTree").unwrap().ref_ids("slides");
    assert_eq!(nodes.len(), 132);
    let node = k.obj(nodes[0]).unwrap();
    let slide = k.obj(node.ref_id("slide").unwrap()).unwrap();
    assert_eq!(slide.type_name(), "KN.SlideArchive");
    let size = show.msg("size").unwrap();
    assert_eq!((size.f32("width"), size.f32("height")), (Some(1920.0), Some(1080.0)));
}

#[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
#[test]
fn edit_text_survives_save() {
    let mut k = KeyFile::open(testdata().join("01-text-box.key")).unwrap();
    let id = k
        .objects_of("TSWP.StorageArchive")
        .find(|(_, m)| m.strs("text").concat() == "Hello")
        .map(|(id, _)| id);
    let Some(id) = id else {
        let texts: Vec<_> = k.objects_of("TSWP.StorageArchive").map(|(_, m)| m.strs("text").concat()).collect();
        panic!("text box not found; storages: {texts:?}");
    };
    k.obj_mut(id).unwrap().set_strs("text", &["Grüße 👋"]);
    let k2 = KeyFile::from_zip(&k.to_zip().unwrap()).unwrap();
    assert_eq!(k2.obj(id).unwrap().strs("text"), vec!["Grüße 👋"]);
    let _ = IwaFile::default();
}
