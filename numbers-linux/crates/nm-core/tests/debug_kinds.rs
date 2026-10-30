#[test]
#[ignore]
fn dump_kinds() {
    for f in ["28-chart-make.numbers", "26-shape-on-sheet.numbers", "27-text-box-on-sheet.numbers"] {
        let p = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata")).join(f);
        let wb = nm_core::Workbook::open(&p).unwrap();
        for s in wb.sheets() {
            for id in &s.drawables {
                eprintln!("{f}: #{id} {}", wb.file().type_name(*id));
            }
            for d in wb.drawables(&s) {
                let k = match &d.kind {
                    nm_core::kn_model::DrawableKind::Shape(sh) => format!("Shape fill={:?} stroke={:?}", sh.fill, sh.stroke.as_ref().map(|s| s.width)),
                    nm_core::kn_model::DrawableKind::Table(_) => "Table".into(),
                    nm_core::kn_model::DrawableKind::Chart(c) => format!("Chart series={}", c.series.len()),
                    nm_core::kn_model::DrawableKind::Unsupported(n) => format!("Unsupported {n}"),
                    _ => "other".into(),
                };
                eprintln!("   {} {:?} {}", d.id, (d.geom.x, d.geom.y, d.geom.w, d.geom.h), k);
            }
        }
    }
}
