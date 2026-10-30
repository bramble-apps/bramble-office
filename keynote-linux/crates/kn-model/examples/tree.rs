//! Print the slide tree: index, depth, collapsed, skipped.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    for (i, s) in d.slides().iter().enumerate() {
        if s.depth > 0 || s.collapsed {
            println!("{} depth={} collapsed={} skipped={}", i + 1, s.depth, s.collapsed, s.skipped);
        }
    }
}
