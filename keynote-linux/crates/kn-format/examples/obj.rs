//! Dump raw objects with field names.
//! cargo run -p kn-format --example obj -- file.key <id|TypeName|pm|files> [max-depth]
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
    let k = KeyFile::open(&a[1]).unwrap();
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(99);
    let what = a[2].as_str();
    if what == "files" {
        for e in &k.entries {
            let n = match &e.data {
                kn_format::EntryData::Iwa(i) => format!("{} archives", i.archives.len()),
                kn_format::EntryData::Raw(r) => format!("{} bytes", r.len()),
                kn_format::EntryData::Lazy(..) => "(on disk)".to_string(),
            };
            println!("{} {n}", e.name);
        }
        return;
    }
    if what == "pm" {
        print(k.package_metadata().unwrap(), 0, max);
        return;
    }
    let ids: Vec<u64> = match what.parse::<u64>() {
        Ok(id) => vec![id],
        Err(_) => k.objects_of(what).map(|(i, _)| i).collect(),
    };
    for id in ids {
        let ar = k.archive(id).unwrap();
        println!("=== #{id} {} in {} refs {:?} data {:?}", k.type_name(id), k.file_of(id).unwrap(), ar.object_references(0), ar.data_references(0));
        for p in &ar.payloads {
            print(p, 1, max);
        }
    }
}
