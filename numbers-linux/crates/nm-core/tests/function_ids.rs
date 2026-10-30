//! The TSCE function index of every function, read from Numbers' own N-functions.numbers
//! (column A = name, column B = a formula using it), compared with the N1 table.

use std::collections::HashMap;

#[test]
#[ignore]
fn function_ids_from_numbers_file() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/");
    let wb = nm_core::Workbook::open(format!("{dir}N-functions.numbers")).unwrap();
    let k = wb.file();
    let s = wb.sheets().remove(0);
    let info = wb.tables(&s)[0].info;
    let model_id = k.obj(info).unwrap().ref_id("tableModel").unwrap();
    let model = k.obj(model_id).unwrap();
    let td = wb.table_data(info).unwrap();
    let ds = model.msg("base_data_store").unwrap();
    let flist = k.obj(ds.ref_id("formula_table").unwrap()).unwrap();
    let formulas: HashMap<u32, &nm_core::kn_format::Msg> = flist.msgs("entries").filter_map(|e| Some((e.u32("key")?, e.msg("formula")?))).collect();
    let raws: HashMap<(u32, u32), Vec<u8>> = nm_core::kn_model::table::raw_cell_records(k, model).into_iter().collect();
    let mut out = String::new();
    for r in 1..td.rows {
        let name = &td.cells[(r * td.cols) as usize];
        let Some(rec) = raws.get(&(r, 1)) else { continue };
        let flags = u32::from_le_bytes(rec[8..12].try_into().unwrap());
        if flags & 0x200 == 0 {
            continue;
        }
        let mut at = 12;
        for (bit, len) in [(1, 16), (2, 8), (4, 8), (8, 4), (0x10, 4), (0x20, 4), (0x40, 4), (0x80, 4), (0x100, 4)] {
            if flags & bit != 0 {
                at += len;
            }
        }
        let key = u32::from_le_bytes(rec[at..at + 4].try_into().unwrap());
        let Some(f) = formulas.get(&key) else { continue };
        let nodes: Vec<&nm_core::kn_format::Msg> = f.msg("AST_node_array").map(|a| a.msgs("AST_node").collect()).unwrap_or_default();
        // the outermost call is the last FUNCTION_NODE
        let idx = nodes.iter().rev().find_map(|n| n.u64("AST_function_node_index"));
        out.push_str(&format!("{name}\t{}\n", idx.map(|i| i.to_string()).unwrap_or_default()));
    }
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/function-ids-from-file.tsv"), out).unwrap();
}
