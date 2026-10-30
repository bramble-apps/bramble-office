//! Count paragraph separator characters used in text storages.
fn main() {
    let k = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    let (mut n, mut r, mut p, mut ls) = (0, 0, 0, 0);
    for (_, m) in k.objects_of("TSWP.StorageArchive") {
        let t = m.strs("text").concat();
        n += t.matches('\n').count();
        r += t.matches('\r').count();
        p += t.matches('\u{2029}').count();
        ls += t.matches('\u{2028}').count();
    }
    println!("\\n={n} \\r={r} U+2029={p} U+2028(line break)={ls}");
}
