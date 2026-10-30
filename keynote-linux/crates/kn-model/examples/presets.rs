//! The theme's style preset lists (every ThemePresetsArchive). `presets deck.key`
use kn_format::{Msg, Val};
fn main() {
    let d = kn_model::Document::open(std::env::args().nth(1).unwrap()).unwrap();
    let (_, theme) = d.file.first_of("KN.ThemeArchive").unwrap();
    let t = theme.find_super("TSS.ThemeArchive").unwrap();
    let ty = kn_format::schema::schema().type_idx("TSD.ThemePresetsArchive");
    for f in &t.fields {
        let m = match &f.val {
            Val::Msg(m) if m.type_name() == "TSD.ThemePresetsArchive" => Some(m.clone()),
            Val::Bytes(b) => Msg::parse(b, ty).ok(),
            _ => None,
        };
        if let Some(m) = m {
            let names: Vec<String> = m.fields.iter().map(|x| format!("#{}", x.num)).collect();
            println!("field {}: shape {} textbox {} fields {:?}", f.num, m.ref_ids("shape_style_presets").len(), m.ref_ids("textbox_style_presets").len(), names);
        }
    }
}
