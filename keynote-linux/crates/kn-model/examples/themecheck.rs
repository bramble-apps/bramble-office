//! themecheck dir: each theme makes a new presentation that saves and reopens.
fn main() {
    let dir = std::env::args().nth(1).unwrap();
    let mut entries: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "key")).collect();
    entries.sort();
    for p in entries {
        let name = p.file_stem().unwrap().to_string_lossy().to_string();
        let r = (|| -> Result<usize, String> {
            let f = kn_format::KeyFile::open(&p).map_err(|e| e.to_string())?;
            let mut d = kn_model::Document::new_from_theme(f).map_err(|e| e.to_string())?;
            let out = std::env::temp_dir().join(format!("kd-themecheck-{name}.key"));
            d.save_as(&out).map_err(|e| e.to_string())?;
            let back = kn_model::Document::open(&out).map_err(|e| e.to_string())?;
            let n = back.slides().len();
            let _ = std::fs::remove_file(&out);
            Ok(n)
        })();
        println!("{name}: {}", match r { Ok(n) => format!("ok ({n} slides)"), Err(e) => format!("FAIL {e}") });
    }
}
