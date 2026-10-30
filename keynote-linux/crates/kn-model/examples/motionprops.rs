//! motionprops <file.key>: each dynamic background preset's numeric properties.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    for (id, name) in d.motion_presets() {
        let Some(mb) = kn_model::motionbg::read(&d.file, id) else { continue };
        let all = std::env::var_os("ALL").is_some();
        let keep: Vec<String> = mb.nums.iter().filter(|(k, _)| all || k.contains("speed") || k.contains("movement") || k.contains("zoom") || **k == "scale").map(|(k, v)| format!("{k}={v:.2}")).collect();
        println!("{:<14} {:<26} {}", name, mb.family, keep.join(" "));
        if std::env::var_os("COLORS").is_some() {
            for (f, c) in &mb.colors {
                println!("    {f:.2}: {:.2} {:.2} {:.2}", c.r, c.g, c.b);
            }
        }
    }
}
