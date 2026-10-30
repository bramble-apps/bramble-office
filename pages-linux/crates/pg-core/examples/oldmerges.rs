//! cargo run -p pg-core --example oldmerges -- file.pages : merges stored the older way (cell ref : cell ref formulas)
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    for (id, t) in k.objects_of("TST.TableInfoArchive") {
        let Some(model) = t.ref_id("tableModel").and_then(|m| k.obj(m)) else { continue };
        let store = model.msg("merge_owner").and_then(|m| m.msg("formula_store"));
        let mut out = Vec::new();
        for f in store.map(|s| s.msgs("formulas").collect::<Vec<_>>()).unwrap_or_default() {
            let nodes: Vec<_> = f.msg("formula").and_then(|f| f.msg("AST_node_array")).map(|n| n.msgs("AST_node").collect()).unwrap_or_default();
            let refs: Vec<(i64, i64)> = nodes
                .iter()
                .filter(|n| n.enum_name("AST_node_type") == Some("CELL_REFERENCE_NODE"))
                .filter_map(|n| Some((n.msg("AST_row")?.i64("row")?, n.msg("AST_column")?.i64("column")?)))
                .collect();
            let colon = nodes.iter().any(|n| n.enum_name("AST_node_type") == Some("COLON_NODE"));
            out.push((refs, colon, nodes.iter().map(|n| n.enum_name("AST_node_type").unwrap_or("?")).collect::<Vec<_>>()));
        }
        if !out.is_empty() {
            println!("table {id}: {} formulas", out.len());
            for o in out.iter().take(30) {
                println!("  {:?}", o);
            }
        }
    }
}
