//! chartpresets file: the theme's chart style presets (TSCH.ChartStylePreset), the styles each
//! lists, and which preset each chart's styles come from.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let presets: Vec<_> = k.objects_of("TSCH.ChartStylePreset").map(|(id, m)| (id, m.clone())).collect();
    for (id, m) in &presets {
        let mut refs = Vec::new();
        m.collect_refs(&mut refs);
        println!("preset #{id} uuid? {:?} refs {:?}", m.msg("uuid").is_some(), refs);
    }
    for (id, m) in k.objects_of("TSCH.ChartDrawableArchive") {
        let Some(u) = m.msg("unity") else { continue };
        println!("chart #{id} preset_uuid {:?} chart_style {:?} paragraph_styles {:?}", u.msg("preset_uuid").is_some(), u.ref_id("chart_style"), u.ref_ids("paragraph_styles"));
        for p in u.ref_ids("paragraph_styles") {
            let cs = kn_model::style::char_style(&k, Some(p), None);
            print!(" [{} {}]", cs.font_name, cs.size);
        }
        println!();
    }
}
