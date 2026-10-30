//! maketheme White.key out.key "Name" bg text accent_degrees [title_font body_font]
//!
//! A theme of our own made from the built-in White theme (original artwork only): every
//! neutral colour (white backgrounds, black and grey text, table lines and fills) is mapped
//! from White's black..white onto `text`..`bg` (hex, e.g. 1c1f26), clearly coloured ones
//! (chart and shape accents) turn by `accent_degrees` and, on a dark background, get lighter;
//! Helvetica Neue becomes the given fonts (PostScript names: "Caladea", "Carlito",
//! "HelveticaNeue"). Slide 1's empty title and subtitle say the name and "Keydeck theme".
use kn_format::{EntryData, Msg, Val};

fn hex(s: &str) -> [f32; 3] {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).expect("hex colour");
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

struct Spec {
    bg: [f32; 3],
    text: [f32; 3],
    deg: f32,
    title_font: String,
    body_font: String,
}

fn hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    let d = mx - mn;
    let s = if d < 1e-4 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-4) };
    let h = if d < 1e-4 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) * 60.0
    } else if mx == g {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    (h, s, l)
}

fn rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    ((r + m).clamp(0.0, 1.0), (g + m).clamp(0.0, 1.0), (b + m).clamp(0.0, 1.0))
}

fn recolour(m: &mut Msg, sp: &Spec, n: &mut usize) {
    if m.type_name() == "TSP.Color" {
        if let (Some(r), Some(g), Some(b)) = (m.f32("r"), m.f32("g"), m.f32("b")) {
            let (h, s, l) = hsl(r, g, b);
            let (r1, g1, b1) = if s < 0.25 || (r.max(g).max(b) - r.min(g).min(b)) < 0.12 {
                // a neutral: black..white onto text..bg
                let t = 0.2126 * r + 0.7152 * g + 0.0722 * b;
                let mix = |a: f32, z: f32| a + (z - a) * t;
                (mix(sp.text[0], sp.bg[0]), mix(sp.text[1], sp.bg[1]), mix(sp.text[2], sp.bg[2]))
            } else {
                // an accent: turned, and lighter on a dark background
                let dark_bg = 0.2126 * sp.bg[0] + 0.7152 * sp.bg[1] + 0.0722 * sp.bg[2] < 0.4;
                rgb(h + sp.deg, s, if dark_bg { (l + 0.12).min(0.75) } else { l })
            };
            m.set_f32("r", r1);
            m.set_f32("g", g1);
            m.set_f32("b", b1);
            *n += 1;
        }
        return;
    }
    if let Some(f) = m.str("font_name").map(str::to_string) {
        let to = if f.contains("Medium") || f.contains("Bold") { &sp.title_font } else { &sp.body_font };
        let italic = f.contains("Italic");
        let name = match (to.as_str(), italic, f.contains("Medium") || f.contains("Bold")) {
            ("HelveticaNeue", _, _) => f.clone(),
            (fam, true, true) => format!("{fam}-BoldItalic"),
            (fam, true, false) => format!("{fam}-Italic"),
            (fam, false, true) => format!("{fam}-Bold"),
            (fam, false, false) => fam.to_string(),
        };
        m.set_str("font_name", &name);
    }
    for f in &mut m.fields {
        if let Val::Msg(c) = &mut f.val {
            recolour(c, sp, n);
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 7 {
        eprintln!("maketheme White.key out.key Name bg text accent_degrees [title_font body_font]");
        std::process::exit(2);
    }
    let sp = Spec {
        bg: hex(&a[4]),
        text: hex(&a[5]),
        deg: a[6].parse().expect("degrees"),
        title_font: a.get(7).cloned().unwrap_or_else(|| "HelveticaNeue".into()),
        body_font: a.get(8).cloned().unwrap_or_else(|| "HelveticaNeue".into()),
    };
    let mut k = kn_format::KeyFile::open(&a[1]).unwrap();
    let mut n = 0;
    for e in &mut k.entries {
        if let EntryData::Iwa(iwa) = &mut e.data {
            for ar in &mut iwa.archives {
                for p in &mut ar.payloads {
                    recolour(p, &sp, &mut n);
                }
            }
        }
    }
    let tmp = std::env::temp_dir().join(format!("maketheme-{}.key", std::process::id()));
    k.save(&tmp).unwrap();
    // the name: slide 1's empty title / subtitle and the theme's identifier
    let name = &a[3];
    let mut d = kn_model::Document::open(&tmp).unwrap();
    let s = d.slides();
    let sc = d.scene(&s, 0).unwrap();
    let mut h = kn_model::edit::History::default();
    d.begin(&mut h, "name");
    for x in &sc.drawables {
        let kn_model::DrawableKind::Shape(sh) = &x.kind else { continue };
        let Some(t) = &sh.text else { continue };
        // (White's placeholders hold "TEMPLATE:Title" and the like)
        if !t.text.trim().is_empty() && !t.text.starts_with("TEMPLATE:") && t.text.trim() != "Body" {
            continue;
        }
        match x.placeholder {
            Some(kn_model::PlaceholderKind::Title) => d.set_storage_text(&mut h, t.storage, name),
            Some(kn_model::PlaceholderKind::Body) => d.set_storage_text(&mut h, t.storage, "Keydeck theme"),
            _ => {}
        }
    }
    d.end(&mut h);
    let ident = format!("keydeck_{}", name.to_lowercase().replace(' ', "_"));
    if let Some((id, _)) = d.file.first_of("KN.ThemeArchive")
        && let Some(t) = d.file.obj_mut(id).and_then(|m| m.find_super_mut("TSS.ThemeArchive"))
    {
        t.set_str("theme_identifier", &ident);
    }
    if let Some((id, _)) = d.file.first_of("KN.DocumentArchive")
        && let Some(t) = d.file.obj_mut(id).and_then(|m| m.find_super_mut("TSA.DocumentArchive"))
    {
        // (as Keynote's own Save Theme does)
        t.remove("template_identifier");
    }
    d.save_as(&a[2]).unwrap();
    let _ = std::fs::remove_file(&tmp);
    kn_model::Document::open(&a[2]).expect("reopens");
    println!("{name}: {n} colours");
}
