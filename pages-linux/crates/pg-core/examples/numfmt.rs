//! cargo run -p pg-core --example numfmt -- files... : page number / count fields' formats
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(p) = pg_core::Pages::open(&path) else { continue };
        for (id, m) in p.file().objects_of("TSWP.NumberAttachmentArchive") {
            let kind = m.msg("super").and_then(|s| s.enum_name("kind")).unwrap_or("?").to_string();
            println!("{} {id} {kind} format {:?} name {:?}", std::path::Path::new(&path).file_name().unwrap().to_string_lossy().chars().take(3).collect::<String>(), m.u64("number_format"), m.str("number_format_name"));
        }
    }
}
