//! themephotos theme.key: the pictures a theme carries (data id, file, size, pixel size, locator).
fn main() {
    let k = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    let pm = k.package_metadata().unwrap();
    for d in pm.msgs("datas") {
        let Some(id) = d.u64("identifier") else { continue };
        let name = d.str("preferred_file_name").or(d.str("file_name")).unwrap_or("?").to_string();
        let loc = d.str("document_resource_locator").unwrap_or("");
        let (len, dims) = match k.data(id) {
            Some((_, b)) => (b.len(), kn_format::objects::image_size(&b)),
            None => (0, None),
        };
        println!("{id}\t{name}\t{len}\t{dims:?}\t{loc}");
    }
}
