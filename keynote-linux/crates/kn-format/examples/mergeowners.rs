//! mergeowners <file.key>: tables whose merge owner holds 2+ formulas, and their owner's cell records.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let Ok(k) = kn_format::KeyFile::open(&a[1]) else { return };
    for (id, m) in k.objects_of("TST.TableModelArchive") {
        let n = m.msg("merge_owner").and_then(|o| o.msg("formula_store")).map(|f| f.msgs("formulas").count()).unwrap_or(0);
        if n >= 2 {
            println!("{} table model {id}: {n} merges", a[1]);
        }
    }
}
