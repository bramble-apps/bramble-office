#[test]
#[ignore]
fn find() {
    for e in std::fs::read_dir(format!("{}/../../../keynote-linux/testdata", env!("CARGO_MANIFEST_DIR"))).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "key") { continue; }
        let Ok(k) = nm_core::kn_format::KeyFile::open(&p) else { continue };
        if k.archive(35180).is_some() {
            eprintln!("{}: #35180 {}", p.display(), k.type_name(35180));
        }
    }
}
