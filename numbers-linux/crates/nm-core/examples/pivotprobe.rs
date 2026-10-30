//! Pivot owners: who references them, their source table and the column uid -> index maps.
use nm_core::kn_format::tables::{uuid_of_str, uuid_of_tsp};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let wb = nm_core::Workbook::open(&a[1]).unwrap();
    let k = wb.file();
    for (pid, p) in k.objects_of("TST.PivotOwnerArchive") {
        println!("pivot owner {pid}");
        for (_, ar) in k.archives() {
            if ar.object_references(0).contains(&pid) {
                println!("  referenced by {} {}", ar.id(), k.type_name(ar.id()));
            }
        }
        let src = p.msg("source_table_uid").map(uuid_of_tsp);
        println!("  source uid {:02x?}", src);
        for (info, m) in k.objects_of("TST.TableInfoArchive") {
            let model = m.ref_id("tableModel").and_then(|t| k.obj(t)).unwrap();
            let tid = model.str("table_id").and_then(uuid_of_str);
            println!("  table info {info} model {:?} name {:?} table_id {:02x?} pivot? {:?}", m.ref_id("tableModel"), model.str("table_name"), tid, model.fields.iter().map(|f| f.num).filter(|n| *n > 40).collect::<Vec<_>>());
            if let Some(map) = model.ref_id("base_column_row_uids").and_then(|x| k.obj(x)) {
                let cols: Vec<String> = map.msgs("sorted_column_uids").zip(map.u64s("column_index_for_uid")).map(|(u, i)| format!("{}:{:02x?}", i, &uuid_of_tsp(u)[..4])).collect();
                println!("    cols {cols:?}");
            }
        }
        for g in ["grouping_columns_for_rows", "grouping_columns_for_columns"] {
            for gc in p.msg(g).into_iter().flat_map(|x| x.msgs("group_column")) {
                println!("  {g}: {:02x?}", &gc.msg("column_uid").map(uuid_of_tsp).unwrap()[..4]);
            }
        }
        for ag in p.msg("aggregate_columns").into_iter().flat_map(|x| x.msgs("aggregates")) {
            println!("  aggregate {:02x?} type {:?} show {:?}", &ag.msg("column_uid").map(uuid_of_tsp).unwrap()[..4], ag.i64("agg_type"), ag.i64("show_as_type"));
        }
    }
}
