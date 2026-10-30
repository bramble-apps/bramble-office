//! An object (and the objects it references, to a depth) with field names. usage:
//! objtree file.numbers <object id | type name> [depth]
use nm_core::kn_format::msg::{Msg, Val};
use nm_core::kn_format::schema::schema;

fn show(m: &Msg, ind: usize) {
    let t = m.ty.map(|t| schema().ty(t));
    for f in &m.fields {
        let name = t.and_then(|t| t.fields.get(&f.num)).map(|d| d.name.clone()).unwrap_or_else(|| f.num.to_string());
        let pad = "  ".repeat(ind);
        match &f.val {
            Val::Msg(s) => {
                println!("{pad}{name} {{");
                show(s, ind + 1);
                println!("{pad}}}");
            }
            Val::Bytes(b) => match std::str::from_utf8(b) {
                Ok(s) if s.chars().all(|c| !c.is_control() || c == '\n') => println!("{pad}{name}: {s:?}"),
                _ => println!("{pad}{name}: <{} bytes>", b.len()),
            },
            Val::I64(v) => println!("{pad}{name}: {} (f64)", f64::from_bits(*v)),
            Val::I32(v) => println!("{pad}{name}: {} (f32)", f32::from_bits(*v)),
            Val::Varint(v) => println!("{pad}{name}: {v}"),
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let k = wb.file();
    let ids: Vec<u64> = match a[2].parse() {
        Ok(id) => vec![id],
        Err(_) => k.objects_of(&a[2]).map(|(i, _)| i).collect(),
    };
    for id in ids {
        if let Some(m) = k.obj(id) {
            println!("== {id} {}", m.type_name());
            show(m, 1);
        }
    }
}
