//! lzcheck in out: open a package (any .iwa may be LZFSE-compressed), report, save a copy, reopen.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let n = k.archives().count();
    let marks = k.package_metadata().map(|pm| ["components", "versioned_components"].iter().map(|l| pm.msgs(l).filter(|c| c.has("compression_algorithm")).count()).sum::<usize>()).unwrap_or(0);
    println!("opened: {n} archives, {marks} compression marks left");
    k.save(&a[2]).unwrap();
    let back = kn_format::KeyFile::open(&a[2]).unwrap();
    println!("saved and reopened: {} archives", back.archives().count());
}
