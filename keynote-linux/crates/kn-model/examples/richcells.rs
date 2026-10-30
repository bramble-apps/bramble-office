//! cargo run -p kn-model --example richcells -- file.{key,pages,numbers} … : tables with rich
//! text cells, and what each such cell's runs are.
fn main() {
    for a in std::env::args().skip(1) {
        let Ok(k) = kn_format::KeyFile::open(&a) else { continue };
        let mut n = 0;
        for (_, info) in k.objects_of("TST.TableInfoArchive") {
            let Some(t) = kn_model::table::table(&k, info) else { continue };
            for (i, rt) in t.rich.iter().enumerate() {
                let Some(tb) = rt else { continue };
                n += 1;
                if n <= 3 {
                    let runs: Vec<String> = tb.paragraphs.iter().flat_map(|p| p.runs.iter()).map(|r| format!("{:?} {}pt{}{}", tb.text.get(r.start..r.end).unwrap_or(""), r.style.size, if r.style.bold { " bold" } else { "" }, if r.style.italic { " italic" } else { "" })).collect();
                    println!("  cell {}: {}", i, runs.join(" | "));
                    for p in &tb.paragraphs {
                        println!("    para {:?} spacing {:?} before {} after {}", tb.text.get(p.start..p.end).unwrap_or(""), p.line_spacing, p.space_before, p.space_after);
                    }
                }
            }
        }
        if n > 0 {
            println!("{a}: {n} rich cells");
        }
    }
}
