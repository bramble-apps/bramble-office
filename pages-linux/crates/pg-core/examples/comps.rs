//! cargo run -p pg-core --example comps -- file.pages : .iwa files and whether the package lists them as components
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let p = pg_core::Pages::open(&a[1]).unwrap();
    let k = p.file();
    for e in &k.entries {
        if e.name.ends_with(".iwa") && e.name.contains(a.get(2).map(String::as_str).unwrap_or("")) {
            println!("{} component {:?}", e.name, k.component_for_file(&e.name));
        }
    }
    if let Some(pm) = k.package_metadata() {
        for c in pm.msgs("components").filter(|c| c.str("locator").or(c.str("preferred_locator")).is_some_and(|l| l.contains("Operation"))) {
            println!("listed: {:?} {:?} {:?}", c.u64("identifier"), c.str("preferred_locator"), c.str("locator"));
        }
        println!("versioned_components: {}", pm.count("versioned_components"));
    }
}
