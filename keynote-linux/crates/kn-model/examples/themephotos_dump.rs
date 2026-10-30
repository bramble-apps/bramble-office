//! themephotos_dump theme.key outdir: write the theme's photos (jpeg/png > 150 kB) as files named by data id.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let pm = k.package_metadata().unwrap();
    for d in pm.msgs("datas") {
        let Some(id) = d.u64("identifier") else { continue };
        let Some((name, b)) = k.data(id) else { continue };
        if b.len() < 150_000 || kn_format::objects::image_size(&b).is_none() { continue }
        let ext = name.rsplit('.').next().unwrap_or("jpg");
        std::fs::write(format!("{}/{id}.{ext}", a[2]), &b[..]).unwrap();
    }
}
