//! gradangles file.key: every stored gradient angle (to check Keynote's units).
fn walk(m: &kn_format::Msg, out: &mut Vec<f32>) {
    if m.type_name() == "TSD.AngleGradientArchive" {
        if let Some(a) = m.f32("gradientangle") {
            out.push(a);
        }
    }
    for f in &m.fields {
        if let kn_format::Val::Msg(c) = &f.val {
            walk(c, out);
        }
    }
}
fn main() {
    let k = kn_format::KeyFile::open(std::env::args().nth(1).unwrap()).unwrap();
    let mut out = Vec::new();
    for (_, a) in k.archives() {
        walk(a.obj(), &mut out);
    }
    let mut v: Vec<String> = out.iter().map(|a| format!("{a:.4}")).collect();
    v.sort();
    v.dedup();
    println!("{} angles: {}", out.len(), v.join(" "));
}
