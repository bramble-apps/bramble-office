//! tweaktheme in.key out.key degrees: rotate the hue of every clearly coloured colour in a
//! theme (styles, presets, palettes) by `degrees`; greys, black and white stay.
use kn_format::{EntryData, Msg, Val};

fn hue_rotate(m: &mut Msg, deg: f32, n: &mut usize) {
    if m.type_name() == "TSP.Color" {
        if let (Some(r), Some(g), Some(b)) = (m.f32("r"), m.f32("g"), m.f32("b")) {
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let l = (mx + mn) / 2.0;
            let d = mx - mn;
            let s = if d < 1e-4 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-4) };
            if s > 0.3 && d > 0.12 {
                let mut h = if mx == r { ((g - b) / d).rem_euclid(6.0) } else if mx == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 } * 60.0;
                h = (h + deg).rem_euclid(360.0);
                let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
                let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
                let (r1, g1, b1) = match (h / 60.0) as u32 { 0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x), 3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x) };
                let mm = l - c / 2.0;
                m.set_f32("r", (r1 + mm).clamp(0.0, 1.0));
                m.set_f32("g", (g1 + mm).clamp(0.0, 1.0));
                m.set_f32("b", (b1 + mm).clamp(0.0, 1.0));
                *n += 1;
            }
        }
        return;
    }
    for f in &mut m.fields {
        if let Val::Msg(c) = &mut f.val {
            hue_rotate(c, deg, n);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut k = kn_format::KeyFile::open(&a[1]).unwrap();
    let deg: f32 = a[3].parse().unwrap();
    let mut n = 0;
    for e in &mut k.entries {
        // the stylesheets and the theme (not the slides' own direct formatting)
        if !(e.name.contains("Stylesheet") || e.name.ends_with("Document.iwa")) {
            continue;
        }
        if let EntryData::Iwa(iwa) = &mut e.data {
            for ar in &mut iwa.archives {
                for p in &mut ar.payloads {
                    hue_rotate(p, deg, &mut n);
                }
            }
        }
    }
    k.save(&a[2]).unwrap();
    kn_model::Document::open(&a[2]).expect("reopens");
    println!("{n} colours");
}
