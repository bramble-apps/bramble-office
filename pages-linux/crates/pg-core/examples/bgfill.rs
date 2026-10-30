//! cargo run -p pg-core --example bgfill -- file.pages : where its page backgrounds are stored
fn main() {
    let f = std::env::args().nth(1).unwrap();
    let p = pg_core::Pages::open(&f).unwrap();
    let k = p.file();
    let kind = |m: &pg_core::kn_format::Msg| m.msg("background_fill").map(|b| if b.msg("image").is_some() { "image" } else if b.msg("gradient").is_some() { "gradient" } else if b.msg("color").is_some() { "color" } else { "empty" });
    for (id, s) in k.objects_of("TP.SectionArchive") {
        println!("section {id}: {:?}", kind(s));
    }
    for (id, t) in k.objects_of("TP.PageTemplateArchive") {
        println!("page template {id} {:?}: {:?}", t.str("name"), kind(t));
    }
    for (id, t) in k.objects_of("TP.SectionTemplateArchive") {
        println!("section template {id}: {:?}", kind(t));
    }
}
