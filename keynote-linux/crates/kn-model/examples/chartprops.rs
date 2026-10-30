//! chartprops file id...: the named property values in the `current` of the given chart style
//! objects (scalars by their schema kind; sub-messages by their own scalar fields).
use kn_format::schema::{schema, Kind};
use kn_format::Msg;

fn show(m: &Msg, indent: usize) {
    let sch = schema();
    let Some(t) = m.ty else { return };
    let ty = &sch.types[t as usize];
    let mut seen = Vec::new();
    for f in &m.fields {
        let Some(d) = ty.fields.get(&f.num) else { continue };
        if seen.contains(&f.num) {
            continue;
        }
        seen.push(f.num);
        let pad = " ".repeat(indent);
        match d.kind {
            Kind::Message => {
                println!("{pad}{}:", d.name);
                for sub in m.msgs(&d.name) {
                    show(sub, indent + 2);
                }
            }
            Kind::Float | Kind::Double => println!("{pad}{} = {:?}", d.name, m.f32(&d.name)),
            Kind::Bool => println!("{pad}{} = {:?}", d.name, m.bool(&d.name)),
            Kind::String => println!("{pad}{} = {:?}", d.name, m.str(&d.name)),
            Kind::Enum => println!("{pad}{} = {:?}", d.name, m.enum_name(&d.name)),
            _ => println!("{pad}{} = {:?}", d.name, m.i64(&d.name)),
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    for id in &a[2..] {
        let id: u64 = id.parse().unwrap();
        println!("#{id} {}", k.type_name(id));
        if let Some(c) = k.obj(id).and_then(|m| m.msg("current")) {
            show(c, 2);
        }
    }
}
