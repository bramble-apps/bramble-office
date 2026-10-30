//! r38 <outdir>: R37-3 with each newer Keydeck edit, one file per edit (round R38 for real
//! Keynote: does it open them, look the same, and keep them after a save).
use kn_model::edit::History;
use kn_model::{Color, Document, DrawableKind, Shadow};

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("outdir"));
    std::fs::create_dir_all(&out).unwrap();
    let src = "testdata/r37/R37-3-image-before-style.key";
    let find = |d: &Document, slide: usize, table: bool| {
        d.scene(&d.slides(), slide).unwrap().drawables.iter().find(|x| if table { matches!(x.kind, DrawableKind::Table(_)) } else { matches!(x.kind, DrawableKind::Image(_)) && !x.from_template }).map(|x| (x.id, x.geom.w, x.geom.h)).unwrap()
    };
    type Step = Box<dyn Fn(&mut Document, &mut History)>;
    let steps: Vec<(&str, Step)> = vec![
        ("R38-1-tablestyle-orange", Box::new(move |d, h| {
            let (t, ..) = find(d, 1, true);
            let p = d.table_style_presets()[2].0;
            assert!(d.apply_table_style_preset(h, t, p));
        })),
        ("R38-2-mask-rect", Box::new(move |d, h| {
            let (i, w, hh) = find(d, 2, false);
            assert!(d.set_mask_rect(h, i, w * 0.2, hh * 0.1, w * 0.5, hh * 0.6));
        })),
        ("R38-3-shadow-contact", Box::new(move |d, h| {
            let (i, ..) = find(d, 2, false);
            let s = Shadow { color: Color::BLACK, angle: 315.0, offset: 0.0, blur: 8.0, opacity: 0.6, kind: 1, height: 0.25, curve: 0.6 };
            assert!(d.set_shadow_spec(h, i, Some(&s)));
        })),
        ("R38-4-shadow-curved", Box::new(move |d, h| {
            let (i, ..) = find(d, 2, false);
            let s = Shadow { color: Color::BLACK, angle: 315.0, offset: 4.0, blur: 10.0, opacity: 0.7, kind: 2, height: 0.2, curve: 0.8 };
            assert!(d.set_shadow_spec(h, i, Some(&s)));
        })),
        ("R38-5-reflection", Box::new(move |d, h| {
            let (i, ..) = find(d, 2, false);
            assert!(d.set_reflection_amount(h, i, 0.4));
        })),
        ("R38-6-imagestyle-frame", Box::new(move |d, h| {
            let (i, ..) = find(d, 2, false);
            let p = d.image_style_presets()[5];
            assert!(d.apply_shape_style(h, i, p));
        })),
    ];
    for (name, f) in steps {
        let mut d = Document::open(src).unwrap();
        let mut h = History::default();
        d.begin(&mut h, name);
        f(&mut d, &mut h);
        d.end(&mut h);
        let p = out.join(format!("{name}.key"));
        d.save_as(&p).unwrap();
        Document::open(&p).expect("reopens");
        println!("{}", p.display());
    }
}
