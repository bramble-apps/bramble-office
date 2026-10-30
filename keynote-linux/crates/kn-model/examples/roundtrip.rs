//! Lossless check on any deck: every .iwa re-encodes identically; an edit survives save+reload.
use kn_format::{EntryData, KeyFile};
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let raw = std::fs::read(&path).unwrap();
    let k = KeyFile::from_zip(&raw).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&raw)).unwrap();
    let (mut n, mut bad) = (0, 0);
    for e in &k.entries {
        if let EntryData::Iwa(iwa) = &e.data {
            let mut f = z.by_name(&e.name).unwrap();
            let mut orig = Vec::new();
            std::io::Read::read_to_end(&mut f, &mut orig).unwrap();
            n += 1;
            if iwa.encode_stream() != kn_format::wire::iwa_unframe(&orig).unwrap() {
                bad += 1;
                println!("DIFFERS: {}", e.name);
            }
        }
    }
    println!("{n} iwa files, {bad} differ");
    let mut doc = kn_model::Document::from_file(k);
    let slides = doc.slides();
    let st = (0..slides.len()).find_map(|i| doc.scene(&slides, i).unwrap().drawables.iter().find_map(|d| match &d.kind { kn_model::DrawableKind::Shape(s) => s.text.as_ref().filter(|t| !t.text.is_empty()).map(|t| t.storage), _ => None })).unwrap();
    let mut h = kn_model::edit::History::default();
    doc.begin(&mut h, "t");
    doc.replace_text(&mut h, st, 0, 0, "EDITED ");
    doc.end(&mut h);
    let out = std::env::temp_dir().join("kn-roundtrip-test.key");
    doc.save_as(&out).unwrap();
    let d2 = kn_model::Document::open(&out).unwrap();
    assert!(d2.storage_text(st).starts_with("EDITED "));
    println!("edit saved and reloaded OK ({} bytes)", std::fs::metadata(&out).unwrap().len());
    std::fs::remove_file(out).ok();
}
