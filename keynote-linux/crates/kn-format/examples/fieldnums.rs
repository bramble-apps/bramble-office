//! `fieldnums deck.key id`: field numbers and kinds of an object's top-level fields.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    let ar = k.archive(id).unwrap();
    for (pi, p) in ar.payloads.iter().enumerate() {
        for f in &p.fields {
            let kind = match &f.val {
                kn_format::Val::Msg(_) => "msg".to_string(),
                kn_format::Val::Bytes(b) => format!("bytes[{}] {:02x?}", b.len(), &b[..b.len().min(60)]),
                kn_format::Val::Varint(v) => format!("varint {v}"),
                _ => "other".into(),
            };
            println!("payload {pi} #{} {kind}", f.num);
        }
    }
    println!("{:?}", ar.info);
}
