//! Compact dump of package-metadata components.
//! cargo run -p kn-format --example comp -- file.key [component-id | locator-substring]...
use kn_format::KeyFile;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let k = KeyFile::open(&a[1]).unwrap();
    let pm = k.package_metadata().unwrap();
    for c in pm.msgs("components") {
        let id = c.u64("identifier").unwrap_or(0);
        let loc = c.str("locator").or(c.str("preferred_locator")).unwrap_or("");
        if a.len() > 2 && !a[2..].iter().any(|w| w.parse::<u64>().ok() == Some(id) || (w.parse::<u64>().is_err() && loc.contains(w.as_str()))) {
            continue;
        }
        let fields: Vec<String> = c
            .fields
            .iter()
            .filter_map(|f| {
                let d = c.ty.map(|t| kn_format::schema::schema().ty(t)).and_then(|t| t.fields.get(&f.num))?;
                (!matches!(d.name.as_str(), "external_references" | "object_uuid_map_entries" | "data_references" | "identifier")).then(|| d.name.clone())
            })
            .collect();
        let xr: Vec<String> = c
            .msgs("external_references")
            .map(|x| {
                let o = x.u64("object_identifier").map(|o| format!(":{o}({})", k.type_name(o))).unwrap_or_default();
                format!("{}{o}{}", x.u64("component_identifier").unwrap_or(0), if x.bool("is_weak") == Some(true) { "w" } else { "" })
            })
            .collect();
        let uu: Vec<String> = c.msgs("object_uuid_map_entries").map(|e| format!("{}({})", e.u64("identifier").unwrap_or(0), k.type_name(e.u64("identifier").unwrap_or(0)))).collect();
        println!("comp {id} loc={loc} other={fields:?}");
        for f in ["save_token", "document_read_version", "document_write_version", "is_stored_outside_object_archive"] {
            if c.has(f) {
                println!("   {f} = {:?}", c.u64s(f));
            }
        }
        println!("   xrefs[{}]: {}", xr.len(), xr.join(" "));
        println!("   uuids[{}]: {}", uu.len(), uu.join(" "));
        for d in c.msgs("data_references") {
            println!("   data {} -> {:?}", d.u64("data_identifier").unwrap_or(0), d.msgs("object_reference_list").map(|o| o.u64("object_identifier")).collect::<Vec<_>>());
        }
    }
}
