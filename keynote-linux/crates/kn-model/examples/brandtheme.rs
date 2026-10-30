//! brandtheme theme.key "Name": slide 1's empty title / subtitle get "Name" / "Keydeck theme",
//! and the theme's own name (theme_identifier, template_identifier) becomes "Name".
use kn_model::DrawableKind;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let name = &a[2];
    let mut d = kn_model::Document::open(&a[1]).unwrap();
    let s = d.slides();
    let sc = d.scene(&s, 0).unwrap();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "brand");
    for x in &sc.drawables {
        let DrawableKind::Shape(sh) = &x.kind else { continue };
        let Some(t) = &sh.text else { continue };
        if !t.text.trim().is_empty() {
            continue;
        }
        match x.placeholder {
            Some(kn_model::PlaceholderKind::Title) => d.set_storage_text(&mut h, t.storage, name),
            Some(kn_model::PlaceholderKind::Body) => d.set_storage_text(&mut h, t.storage, "Keydeck theme"),
            _ => {}
        }
    }
    d.end(&mut h);
    let ident = format!("keydeck_{}", name.to_lowercase().replace(' ', "_"));
    if let Some((id, _)) = d.file.first_of("KN.ThemeArchive") {
        if let Some(t) = d.file.obj_mut(id).and_then(|m| m.find_super_mut("TSS.ThemeArchive")) {
            t.set_str("theme_identifier", &ident);
        }
    }
    if let Some((id, _)) = d.file.first_of("KN.DocumentArchive") {
        if let Some(t) = d.file.obj_mut(id).and_then(|m| m.find_super_mut("TSA.DocumentArchive")) {
            // Keynote's own Save Theme drops it (R50)
            t.remove("template_identifier");
        }
    }
    d.save_as(&a[1]).unwrap();
    println!("{name}: {}", kn_model::Document::open(&a[1]).unwrap().theme_name());
}
