//! cargo run -p kn-model --example rawbuilds -- file.key slide# : raw build / build chunk / movie /
//! transition messages of one slide (field names from the schema), for decoding animation data.
use kn_format::schema::schema;
use kn_format::{Msg, Val};
use kn_model::Document;

fn print(m: &Msg, indent: usize) {
    let s = schema();
    let td = m.ty.map(|t| s.ty(t));
    for f in &m.fields {
        let name = td.and_then(|t| t.fields.get(&f.num)).map(|d| d.name.clone()).unwrap_or_else(|| format!("#{}", f.num));
        let pad = " ".repeat(indent);
        match &f.val {
            Val::Msg(sub) => {
                println!("{pad}{name} {{");
                print(sub, indent + 2);
                println!("{pad}}}");
            }
            Val::Bytes(b) => println!("{pad}{name}: {:?}", String::from_utf8_lossy(b)),
            Val::I64(v) => println!("{pad}{name}: {}", f64::from_bits(*v)),
            Val::I32(v) => println!("{pad}{name}: {}", f32::from_bits(*v)),
            Val::Varint(v) => println!("{pad}{name}: {v}"),
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let doc = Document::open(&a[1]).unwrap();
    let slides = doc.slides();
    let i: usize = a[2].parse::<usize>().unwrap() - 1;
    let k = &doc.file;
    let slide = k.obj(slides[i].slide).unwrap();
    if let Some(t) = slide.msg("transition") {
        println!("TRANSITION");
        print(t, 2);
    }
    for id in slide.ref_ids("builds") {
        println!("BUILD #{id}");
        print(k.obj(id).unwrap(), 2);
    }
    for id in slide.ref_ids("buildChunks") {
        println!("CHUNK #{id}");
        print(k.obj(id).unwrap(), 2);
    }
    for id in slide.ref_ids("drawables_z_order") {
        if k.type_name(id) == "TSD.MovieArchive" {
            println!("MOVIE #{id}");
            print(k.obj(id).unwrap(), 2);
        }
    }
}
