#[test]
#[ignore]
fn count() {
    let home = std::env::var("HOME").unwrap();
    let mut files: Vec<String> = std::fs::read_dir(format!("{home}/CascadeProjects/keynote-linux/testdata")).unwrap().flatten().map(|e| e.path().to_string_lossy().into_owned()).filter(|p| p.ends_with(".key")).collect();
    files.sort();
    files.truncate(25);
    let _old = [
        format!("{home}/CascadeProjects/numbers-linux/testdata/02-second-table.numbers"),
        format!("{home}/CascadeProjects/numbers-research-src/out-N3/N13-reference-Numbers-own-table-delete.numbers"),
        format!("{home}/CascadeProjects/numbers-linux/testdata/29-base-data.numbers"),
        format!("{home}/CascadeProjects/numbers-linux/testdata/00-empty.numbers"),
        format!("{home}/CascadeProjects/numbers-linux/testdata/07-formula-same-table.numbers"),
        format!("{home}/CascadeProjects/numbers-research-src/out-N3/resaved/N15-insert-table-and-sheet-resaved-by-Numbers.numbers"),
    ];
    for f in files {
        let Ok(k) = nm_core::kn_format::KeyFile::open(&f) else { eprintln!("missing {f}"); continue };
        let k = &k;
        let Some((_, ce)) = k.first_of("TSCE.CalculationEngineArchive") else { continue };
        let dt = ce.msg("dependency_tracker").unwrap();
        let nf = dt.u64("number_of_formulas");
        let fods = dt.ref_ids("formula_owner_dependencies");
        let (mut recs, mut tile_recs, mut ranges) = (0, 0, 0);
        for f in &fods {
            let m = k.obj(*f).unwrap();
            recs += m.msg("cell_dependencies").map(|c| c.count("cell_record")).unwrap_or(0);
            for t in m.msg("tiled_cell_dependencies").map(|t| t.ref_ids("cell_record_tiles")).unwrap_or_default() {
                tile_recs += k.obj(t).map(|t| t.count("cell_records")).unwrap_or(0);
            }
            ranges += m.msg("range_dependencies").map(|r| r.count("back_dependency")).unwrap_or(0);
        }
        let kinds: Vec<u64> = fods.iter().filter_map(|f| k.obj(*f)?.u64("owner_kind")).collect();
        eprintln!("{}: nf={nf:?} fods={} cell_records={recs} tile_records={tile_recs} back_ranges={ranges} kinds={kinds:?}", f.rsplit('/').next().unwrap(), fods.len());
    }
}
