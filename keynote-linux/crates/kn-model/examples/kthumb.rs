//! Save Keynote's own thumbnail(s) of slide N (from its slide node) as files: kthumb deck.key N out_prefix
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let n: usize = a[2].parse().unwrap();
    let node = d.file.obj(s[n - 1].node).unwrap();
    let mut k = 0;
    for f in ["thumbnails", "thumbnailData"] {
        for r in node.msgs(f) {
            let Some(id) = r.u64("identifier") else { continue };
            if let Some((name, bytes)) = d.file.data(id) {
                let ext = name.rsplit('.').next().unwrap_or("bin");
                let p = format!("{}-{k}.{ext}", a[3]);
                std::fs::write(&p, &bytes[..]).unwrap();
                println!("{p} ({name}, {} bytes)", bytes.len());
                k += 1;
            }
        }
    }
    if k == 0 {
        println!("no thumbnail fields; node fields: {:?}", node.fields.iter().map(|f| f.num).collect::<Vec<_>>());
    }
}
