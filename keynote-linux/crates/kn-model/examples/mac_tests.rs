//! Generate the files to verify in real Keynote on a Mac (each exercises one kind of write).
//! cargo run --release -p kn-model --example mac_tests -- <testdata dir> <out dir>
use std::path::Path;

use kn_model::edit::{History, ZMove};
use kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use kn_model::insert::{BasicShape, NewObject};
use kn_model::{Align, Color, Document, DrawableKind, Geometry};

fn text_ids(doc: &Document, i: usize) -> Vec<(u64, u64, String)> {
    let s = doc.slides();
    doc.scene(&s, i)
        .unwrap()
        .drawables
        .iter()
        .filter(|d| !d.from_template)
        .filter_map(|d| match &d.kind {
            DrawableKind::Shape(sh) => sh.text.as_ref().map(|t| (d.id, t.storage, t.text.clone())),
            _ => None,
        })
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let td = Path::new(&a[1]);
    let out = Path::new(&a[2]);
    std::fs::create_dir_all(out).unwrap();
    let mut notes = String::from("Keynote-for-Linux write tests. For each file: open in Keynote 15.4, report opens / damaged / warnings, check the expectation, re-save (File > Save) and say if Keynote complained.\n\n");
    let mut save = |doc: &mut Document, name: &str, expect: &str| {
        doc.save_as(out.join(name)).unwrap();
        notes.push_str(&format!("{name}\n  expect: {expect}\n\n"));
        println!("wrote {name}");
    };

    // T01: plain rewrite (no edits) — our IWA encoder uses real Snappy compression
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    save(&mut d, "T01-rewrite-unchanged.key", "identical to Keynote-Feature-Catalog.key (132 slides). Tests our Snappy-compressed IWA output.");

    // T02: text edits that keep formatting
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    d.begin(&mut h, "t");
    let body = text_ids(&d, 6).into_iter().find(|t| t.2.starts_with("Body placeholder")).unwrap();
    let at = d.storage_text(body.1).find("Second").unwrap();
    d.replace_text(&mut h, body.1, at, at, "Inserted bullet from Linux\n");
    let mixed = (0..d.slides().len()).flat_map(|i| text_ids(&d, i)).find(|t| t.2.starts_with("Mixed: word1")).unwrap();
    d.replace_text(&mut h, mixed.1, 0, 0, "Very ");
    let title = text_ids(&d, 0)[0].clone();
    d.set_text_by_diff(&mut h, title.1, "Keynote Feature Catalog (edited on Linux)");
    d.end(&mut h);
    save(&mut d, "T02-text-edits.key", "slide 1 title ends with '(edited on Linux)'; slide 7 has a new bullet 'Inserted bullet from Linux' between bullets 1 and 2 with the same bullet style; slide 19 reads 'Very Mixed: word1 word2...' with word1 still blue and word2 still Courier.");

    // T03: geometry
    let mut d = Document::open(td.join("02-shape.key")).unwrap();
    let mut h = History::default();
    let s = d.slides();
    let shape = d.scene(&s, 0).unwrap().drawables.iter().find(|x| x.placeholder.is_none() && !x.from_template).unwrap().clone();
    d.begin(&mut h, "t");
    d.set_frame(&mut h, shape.id, Geometry { x: 600.0, y: 300.0, w: 500.0, h: 250.0, angle: 30.0, hflip: true, ..shape.geom });
    d.set_locked(&mut h, shape.id, true);
    d.end(&mut h);
    save(&mut d, "T03-geometry.key", "the rectangle is at x=600 y=300, 500x250 pt, rotated 30 degrees, flipped horizontally and locked (Arrange inspector).");

    // T04: styles (variations)
    let mut d = Document::open(td.join("00-base.key")).unwrap();
    let mut h = History::default();
    let slide = d.slides()[0].slide;
    d.begin(&mut h, "t");
    let mut ids = Vec::new();
    for (i, sh) in [BasicShape::Rectangle, BasicShape::Oval, BasicShape::Star, BasicShape::RoundedRectangle].iter().enumerate() {
        let id = d.insert(&mut h, slide, &NewObject::Shape(*sh)).unwrap();
        d.move_by(&mut h, id, -600.0 + i as f32 * 400.0, 0.0);
        ids.push(id);
    }
    d.set_fill(&mut h, ids[0], &FillSpec::Color(Color { r: 1.0, g: 0.2, b: 0.2, a: 1.0 }));
    d.set_fill(&mut h, ids[1], &FillSpec::Gradient { from: Color { r: 0.1, g: 0.4, b: 1.0, a: 1.0 }, to: Color::WHITE, angle: 90.0 });
    d.set_stroke(&mut h, ids[2], Some(&StrokeSpec { color: Color::BLACK, width: 8.0, dash: Dash::Dashed }));
    d.set_shadow(&mut h, ids[3], true);
    d.set_opacity(&mut h, ids[3], 0.5);
    let t = d.insert(&mut h, slide, &NewObject::TextBox).unwrap();
    for p in [TextProp::Size(72.0), TextProp::Bold(true), TextProp::Color(Color { r: 0.0, g: 0.5, b: 0.0, a: 1.0 }), TextProp::Align(Align::Center)] {
        d.set_text_prop(&mut h, t, &p);
    }
    d.set_slide_background(&mut h, slide, &FillSpec::Color(Color { r: 1.0, g: 0.97, b: 0.85, a: 1.0 }));
    d.end(&mut h);
    save(&mut d, "T04-styles.key", "cream background; red rectangle; blue-to-white gradient oval; star with 8 pt black dashed border; rounded rectangle 50% opacity with shadow; a green bold 72 pt centred text box. Style inspector values should match.");

    // T05: inserting objects (incl. picture + charts)
    let mut d = Document::open(td.join("00-base.key")).unwrap();
    let mut h = History::default();
    let slide = d.slides()[0].slide;
    d.begin(&mut h, "t");
    for (i, sh) in BasicShape::ALL.iter().enumerate() {
        let id = d.insert(&mut h, slide, &NewObject::Shape(*sh)).unwrap();
        let mut g = kn_model::scene::geometry(d.file.obj(id).unwrap().find_super("TSD.DrawableArchive").unwrap());
        g.x = 40.0 + (i % 6) as f32 * 310.0;
        g.y = 200.0 + (i / 6) as f32 * 260.0;
        g.w = 260.0;
        g.h = 200.0;
        d.set_frame(&mut h, id, g);
    }
    let l = d.insert(&mut h, slide, &NewObject::Line { arrow: true }).unwrap();
    d.move_by(&mut h, l, 0.0, 350.0);
    if let Ok(png) = std::fs::read(td.join("../crates/kn-app/data/test-picture.png")).or_else(|_| std::fs::read("/usr/share/pixmaps/debian-logo.png")) {
        let p = d.insert(&mut h, slide, &NewObject::Image(png, "picture.png".into())).unwrap();
        d.move_by(&mut h, p, 700.0, 380.0);
    }
    d.end(&mut h);
    let _ = d.new_slide_after(&mut h, 0);
    let s2 = d.slides()[1].slide;
    d.begin(&mut h, "t");
    let c = d.insert(&mut h, s2, &NewObject::Chart { pie: false }).unwrap();
    d.move_by(&mut h, c, -400.0, 0.0);
    let p = d.insert(&mut h, s2, &NewObject::Chart { pie: true }).unwrap();
    d.move_by(&mut h, p, 450.0, 0.0);
    d.end(&mut h);
    save(&mut d, "T05-inserts.key", "slide 1: all 12 basic shapes in two rows, an arrow line, and (if present) a picture; slide 2: a 2D column chart and a pie chart with Keynote's sample data. All objects selectable and editable.");

    // T06: slide structure
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    d.begin(&mut h, "t");
    d.new_slide_after(&mut h, 6).unwrap(); // new "Title & Bullets" slide at 8
    d.duplicate_slide(&mut h, 18).unwrap(); // copy of "Text boxes & rich text" at 20
    d.delete_slides(&mut h, &[2]).unwrap(); // remove "Layout: Photo - Horizontal"
    d.move_slide(&mut h, 0, 4).unwrap(); // title slide moves to position 5
    d.end(&mut h);
    save(&mut d, "T06-slides.key", "133 slides. The title slide 'Keynote Feature Catalog' is now slide 5; 'Layout: Photo - Horizontal' is gone; after 'Layout: Title & Bullets' there is an EMPTY Title & Bullets slide (placeholders show prompts); 'Text boxes & rich text' appears twice in a row.");

    // T07: transitions, skip, notes, comments, z-order, delete
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    let s = d.slides();
    d.begin(&mut h, "t");
    d.set_transition(&mut h, s[0].slide, "apple:3D-cube", 2.0, 0.0, false);
    d.set_transition(&mut h, s[1].slide, "apple:dissolve", 1.0, 1.5, true);
    d.set_skipped(&mut h, s[3].node, true);
    d.set_notes(&mut h, s[0].slide, "Presenter notes written on Linux ✓");
    let shapes_slide = (0..s.len()).find(|i| text_ids(&d, *i).iter().any(|t| t.2.starts_with("Shapes (default"))).unwrap();
    let sc = d.scene(&s, shapes_slide).unwrap();
    let own: Vec<u64> = sc.drawables.iter().filter(|x| !x.from_template && x.placeholder.is_none()).map(|x| x.id).collect();
    d.reorder(&mut h, s[shapes_slide].slide, &own[..1], ZMove::Front);
    d.delete_drawables(&mut h, s[shapes_slide].slide, &own[own.len() - 1..]);
    if let Some(c) = (0..s.len()).flat_map(|i| d.scene(&s, i).unwrap().drawables).find_map(|x| d.comment(x.id)) {
        d.set_comment_text(&mut h, c.id, "Comment edited on Linux");
    }
    d.end(&mut h);
    save(&mut d, "T07-slide-settings.key", "slide 1 has a Cube transition 2.0 s on click, notes 'Presenter notes written on Linux ✓'; slide 2 has Dissolve 1.0 s starting automatically after 1.5 s; slide 4 is skipped; on 'Shapes (default shape + properties)' the first shape is now in front and the last one is deleted; the comment (slide ~123) reads 'Comment edited on Linux'.");

    // T08: a large real deck (testdata-private/real-deck.key), lightly edited (if available)
    let real = td.join("../testdata-private/real-deck.key");
    if real.exists() {
        let mut d = Document::open(&real).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "t");
        let t = text_ids(&d, 1)[0].clone();
        d.set_text_by_diff(&mut h, t.1, &format!("{} (Linux)", t.2.trim_end()));
        d.end(&mut h);
        save(&mut d, "T08-real-deck-edited.key", "the large real deck with slide 2's first text ending in '(Linux)'. Everything else (movies, builds, transitions, fonts) unchanged.");
    }
    // T09: group + ungroup
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    let s = d.slides();
    let i = (0..s.len()).find(|i| text_ids(&d, *i).iter().any(|t| t.2.starts_with("Shapes (default"))).unwrap();
    let sc = d.scene(&s, i).unwrap();
    let own: Vec<u64> = sc.drawables.iter().filter(|x| !x.from_template && x.placeholder.is_none()).map(|x| x.id).collect();
    d.begin(&mut h, "t");
    let g = d.group(&mut h, s[i].slide, &own[..2]).unwrap();
    let gg = kn_model::scene::geometry(d.file.obj(g).unwrap().find_super("TSD.DrawableArchive").unwrap());
    d.set_frame(&mut h, g, Geometry { angle: 20.0, ..gg });
    let g2 = d.group(&mut h, s[i].slide, &own[2..4]).unwrap();
    d.ungroup(&mut h, s[i].slide, g2).unwrap();
    d.end(&mut h);
    save(&mut d, "T09-group.key", "on 'Shapes (default shape + properties)': the first two shapes form ONE group (Arrange > Ungroup enabled when selected) rotated 20 degrees as a whole; shapes 3 and 4 were grouped then ungrouped and sit exactly where they were before.");

    // T10: formatting part of a text
    let mut d = Document::open(td.join("Keynote-Feature-Catalog.key")).unwrap();
    let mut h = History::default();
    let mixed = (0..d.slides().len()).flat_map(|i| text_ids(&d, i)).find(|t| t.2.starts_with("Mixed: word1")).unwrap();
    let w3 = d.storage_text(mixed.1).find("word3").unwrap();
    d.begin(&mut h, "t");
    d.set_text_prop_range(&mut h, mixed.1, w3, w3 + 5, &TextProp::Bold(true));
    d.set_text_prop_range(&mut h, mixed.1, w3 + 6, w3 + 11, &TextProp::Color(Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }));
    d.end(&mut h);
    save(&mut d, "T10-range-format.key", "slide 19 'Mixed: word1 word2 word3 word4': word1 still blue, word2 still Courier, word3 now BOLD, word4 now RED; 'Mixed:' unchanged. Selecting word3 shows Bold in the Text inspector.");

    // T11: the real deck saved through the lazy/streaming path (media read from the source on save)
    let real = td.join("../testdata-private/real-deck.key");
    if real.exists() {
        let mut d = Document::open(&real).unwrap();
        let mut h = History::default();
        let s = d.slides();
        d.begin(&mut h, "t");
        d.set_notes(&mut h, s[0].slide, "Saved by Keynote for Linux with lazy media");
        d.end(&mut h);
        save(&mut d, "T11-lecture-lazy-save.key", "the lecture deck, slide 1 notes = 'Saved by Keynote for Linux with lazy media'; all 10 movies play and all pictures present (media were streamed from the source file during save).");
    }
    // T12: typing values into a table Keynote made (R2 slide 2 is a fresh Basic table)
    let mut d = Document::open(td.join("ref/R2-table-values.key")).unwrap();
    let mut h = History::default();
    let s = d.slides();
    let t = d.scene(&s, 1).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    use kn_model::tablecells::CellInput::*;
    d.begin(&mut h, "t");
    for (r, c, v) in [(0, 0, Text("City".into())), (0, 1, Text("People".into())), (1, 0, Text("Montréal".into())), (1, 1, Number(1762949.0)), (2, 0, Text("Dunham".into())), (2, 1, Number(3488.0)), (3, 0, Text("Average".into())), (3, 1, Number(-12.25))] {
        d.set_cell(&mut h, t, r, c, &v).unwrap();
    }
    d.end(&mut h);
    save(&mut d, "T12-table-values.key", "slide 2 (the fresh Basic table) now reads: A1 City, B1 People, A2 Montréal, B2 1762949, A3 Dunham, B3 3488, A4 Average, B4 -12.25. Cells must be editable in Keynote; slide 1's table (with =SUM) unchanged.");
    // T13: one slide per build effect we can add (to confirm effect ids survive Keynote's re-save)
    let mut d = Document::open(td.join("02-shape.key")).unwrap();
    let mut h = History::default();
    let effects = kn_model::buildedit::EFFECTS;
    d.begin(&mut h, "t");
    for _ in 1..effects.len() {
        d.duplicate_slide(&mut h, 0).unwrap();
    }
    d.end(&mut h);
    let s = d.slides();
    let mut list = String::new();
    d.begin(&mut h, "t");
    for (i, (name, id, kinds, _)) in effects.iter().enumerate() {
        let sc = d.scene(&s, i).unwrap();
        let shape = sc.drawables.iter().find(|x| x.placeholder.is_none() && !x.from_template).unwrap().id;
        d.add_build(&mut h, s[i].slide, shape, kinds[0], id, 1.0).unwrap();
        list.push_str(&format!("slide {}: {} {:?} = {}\n", i + 1, name, kinds[0], id));
    }
    d.end(&mut h);
    save(&mut d, "T13-build-effects.key", &format!("one rectangle per slide with ONE build each. Please re-save in Keynote; we decode the resaved copy to see which effect ids Keynote kept. For a few slides, the Animate inspector should name the effect shown here:\n{list}"));
    std::fs::write(out.join("README-TESTS.txt"), notes).unwrap();
}
