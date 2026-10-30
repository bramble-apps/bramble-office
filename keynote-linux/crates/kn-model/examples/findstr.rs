//! findstr file.key "text": every string field equal to / containing the text, with object and field.
fn walk(m: &kn_format::Msg, needle: &str, path: &str, out: &mut Vec<String>) {
    for f in &m.fields {
        match &f.val {
            kn_format::Val::Bytes(b) => {
                if let Ok(s) = std::str::from_utf8(b) {
                    if s.contains(needle) {
                        let name = f.num;
                        out.push(format!("{path}.{name} = {s:?}"));
                    }
                }
            }
            kn_format::Val::Msg(c) => walk(c, needle, &format!("{path}.{}", f.num), out),
            _ => {}
        }
    }
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for (file, ar) in k.archives() {
        let mut out = Vec::new();
        walk(ar.obj(), &a[2], k.type_name(ar.id()), &mut out);
        for o in out {
            println!("{file} #{} {o}", ar.id());
        }
    }
}
