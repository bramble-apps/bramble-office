//! Open a copy of a deck, edit, save over the SAME file, reopen and compare every data file.
fn main() {
    let src = std::env::args().nth(1).unwrap();
    let tmp = std::env::temp_dir().join("kn-lazy-same-path.key");
    std::fs::copy(&src, &tmp).unwrap();
    let before: Vec<(String, usize)> = {
        let k = kn_format::KeyFile::open(&src).unwrap();
        k.entries.iter().filter(|e| e.name.starts_with("Data/")).map(|e| (e.name.clone(), k.entry_bytes(e).unwrap().len())).collect()
    };
    let lazy = { let k = kn_format::KeyFile::open(&tmp).unwrap(); k.entries.iter().filter(|e| matches!(e.data, kn_format::EntryData::Lazy(..))).count() };
    let mut doc = kn_model::Document::open(&tmp).unwrap();
    let mut h = kn_model::edit::History::default();
    let s = doc.slides();
    doc.begin(&mut h, "t");
    doc.set_skipped(&mut h, s[0].node, true);
    doc.end(&mut h);
    doc.save_as(&tmp).unwrap(); // same path the lazy entries read from
    doc.save_as(&tmp).unwrap(); // and again (now reading from the replaced file's old inode)
    let k2 = kn_format::KeyFile::open(&tmp).unwrap();
    let mut ok = 0;
    for (name, len) in &before {
        let e = k2.entry(name).unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(k2.entry_bytes(e).unwrap().len(), *len, "{name}");
        ok += 1;
    }
    println!("{lazy} entries lazy; {ok} data files intact after two saves over the same path");
    std::fs::remove_file(tmp).ok();
}
