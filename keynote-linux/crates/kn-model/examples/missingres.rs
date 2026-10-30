//! missingres deck.key: theme artwork the file names (document_resource_locator) but Keydeck can't find, with its digest.
fn main() {
    let k = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    let pm = k.package_metadata().unwrap();
    for d in pm.msgs("datas") {
        let (Some(id), Some(loc)) = (d.u64("identifier"), d.str("document_resource_locator")) else { continue };
        if k.data(id).is_some() { continue }
        let dig = d.bytes("digest").map(|b| b.iter().map(|x| format!("{x:02x}")).collect::<String>()).unwrap_or_default();
        println!("MISSING {loc} {dig}");
    }
}
