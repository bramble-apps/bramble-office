//! chartstyles file.key: each chart's style objects (chart / axes / series private, theme, non-style)
//! with their parents and the property names set in their `current`.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = kn_format::KeyFile::open(&a[1]).unwrap();
    let show = |label: &str, id: kn_format::ObjId| {
        let Some(m) = k.obj(id) else { return };
        let parent = m.msg("super").and_then(|s| s.ref_id("parent"));
        let sch = kn_format::schema::schema();
        let names: Vec<String> = m
            .msg("current")
            .map(|c| c.fields.iter().filter_map(|f| c.ty.and_then(|t| sch.types[t as usize].fields.get(&f.num)).map(|d| d.name.clone())).collect())
            .unwrap_or_default();
        println!("  {label} #{id:?} {} parent {:?}: {}", k.type_name(id), parent, names.join(" "));
    };
    for (id, m) in k.objects_of("TSCH.ChartDrawableArchive") {
        println!("chart #{id:?}");
        let Some(u) = m.msg("unity") else { continue };
        for f in ["chart_style", "chart_non_style", "legend_style", "legend_non_style"] {
            if let Some(r) = u.ref_id(f) { show(f, r) }
        }
        for f in ["value_axis_styles", "value_axis_nonstyles", "category_axis_styles", "category_axis_nonstyles", "series_theme_styles"] {
            for r in u.ref_ids(f) { show(f, r) }
        }
        for f in ["series_private_styles", "series_non_styles"] {
            for e in u.msg(f).into_iter().flat_map(|a| a.msgs("entries")) {
                if let Some(r) = e.ref_id("reference") { show(&format!("{f}[{}]", e.u64("index").unwrap_or(0)), r) }
            }
        }
    }
}
