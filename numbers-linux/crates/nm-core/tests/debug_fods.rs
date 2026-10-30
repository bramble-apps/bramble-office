#[test]
#[ignore]
fn fods() {
    use nm_core::kn_format::tables::{family, uuid_of_tsp, uuid_of_str};
    let home = std::env::var("HOME").unwrap();
    for f in [format!("{}/../../testdata/02-second-table.numbers", env!("CARGO_MANIFEST_DIR")), format!("{home}/CascadeProjects/numbers-research-src/out-N3/N13-reference-Numbers-own-table-delete.numbers")] {
        let wb = nm_core::Workbook::open(&f).unwrap();
        let k = wb.file();
        let fams: Vec<(String, [u8; 12])> = k.objects_of("TST.TableModelArchive").filter_map(|(_, m)| Some((m.str("table_name")?.to_string(), family(&uuid_of_str(m.str("table_id")?)?)?))).collect();
        let (_, ce) = k.first_of("TSCE.CalculationEngineArchive").unwrap();
        eprintln!("== {}", f.rsplit('/').next().unwrap());
        for id in ce.msg("dependency_tracker").unwrap().ref_ids("formula_owner_dependencies") {
            let m = k.obj(id).unwrap();
            let fam = m.msg("formula_owner_uid").map(|u| family(&uuid_of_tsp(u)));
            let who = fams.iter().find(|(_, f)| Some(Some(*f)) == fam).map(|x| x.0.clone()).unwrap_or("-".into());
            let recs: Vec<(u64, u64)> = m.msg("cell_dependencies").map(|c| c.msgs("cell_record").map(|r| (r.u64("row").unwrap_or(0), r.u64("column").unwrap_or(0))).collect()).unwrap_or_default();
            eprintln!("  #{id} kind {:?} owner_of {who} internal {:?} records {recs:?}", m.u64("owner_kind"), m.u64("internal_formula_owner_id"));
        }
    }
}
