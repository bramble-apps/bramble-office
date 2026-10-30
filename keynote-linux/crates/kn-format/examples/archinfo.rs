//! archinfo <file.key> <id>: print an object's ArchiveInfo (message infos, field infos) and
//! the type of every payload.
use kn_format::schema::{schema, Kind};
use kn_format::{KeyFile, Msg, Val};

fn print(m: &Msg, depth: usize, max: usize) {
    let s = schema();
    let t = m.ty.map(|t| s.ty(t));
    for f in &m.fields {
        let fd = t.and_then(|t| t.fields.get(&f.num));
        let name = fd.map(|d| d.name.as_str()).unwrap_or("?");
        let pad = "  ".repeat(depth);
        match &f.val {
            Val::Msg(sub) => {
                if depth >= max {
                    println!("{pad}{name} {{…}}");
                } else {
                    println!("{pad}{name} {{  [{}]", sub.type_name());
                    print(sub, depth + 1, max);
                    println!("{pad}}}");
                }
            }
            Val::Varint(v) => {
                let e = fd.filter(|d| d.kind == Kind::Enum).and_then(|d| d.type_idx).and_then(|ti| s.ty(ti).enum_values.get(&(*v as i64)));
                println!("{pad}{name} = {v}{}", e.map(|e| format!(" ({e})")).unwrap_or_default());
            }
            Val::I32(v) => println!("{pad}{name} = {} (f32)", f32::from_bits(*v)),
            Val::I64(v) => println!("{pad}{name} = {} (f64)", f64::from_bits(*v)),
            Val::Bytes(b) => match std::str::from_utf8(b) {
                Ok(st) if fd.map(|d| d.kind == Kind::String).unwrap_or(false) => println!("{pad}{name} = {st:?}"),
                _ => println!("{pad}{name} = bytes[{}] {:02x?}", b.len(), &b[..b.len().min(24)]),
            },
        }
    }
}


fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let id: u64 = a[2].parse().unwrap();
    let ar = k.archive(id).expect("object");
    print(&ar.info, 1, 12);
    for (i, p) in ar.payloads.iter().enumerate() {
        println!("payload {i}: {}", p.type_name());
        print(p, 1, 3);
    }
}
