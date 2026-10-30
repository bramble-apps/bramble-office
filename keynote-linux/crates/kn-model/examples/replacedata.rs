//! replacedata in.key out.key id=file.jpg ...: replace pictures (by data id) and save.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut k = kn_format::KeyFile::open(&a[1]).unwrap();
    for spec in &a[3..] {
        let (id, f) = spec.split_once('=').unwrap();
        let bytes = std::fs::read(f).unwrap();
        let name = std::path::Path::new(f).file_name().unwrap().to_string_lossy().to_string();
        assert!(k.replace_data_bytes(id.parse().unwrap(), bytes, &name), "replace {id}");
    }
    k.save(&a[2]).unwrap();
    kn_model::Document::open(&a[2]).expect("reopens");
}
