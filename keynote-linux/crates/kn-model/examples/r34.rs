//! r34 <out.key>: R33-0-before.key with every Format > Cell / Table option Keydeck writes
//! (round R34 for real Keynote).
use kn_model::edit::History;
use kn_model::format::{Dash, FillSpec, StrokeSpec};
use kn_model::tablecells::CellInput;
use kn_model::tablefmt::{BorderEdges, CellFormat};
use kn_model::tablelook::Gridline;
use kn_model::{Color, Document, DrawableKind};

fn main() {
    let out = std::env::args().nth(1).expect("out.key");
    let mut d = Document::open("testdata/r33/R33-0-before.key").unwrap();
    let t = d.scene(&d.slides(), 0).unwrap().drawables.iter().find(|x| matches!(x.kind, DrawableKind::Table(_))).unwrap().id;
    let mut h = History::default();
    d.begin(&mut h, "R34");
    for r in 1..=10 {
        d.set_cell(&mut h, t, r, 1, &CellInput::Number(if r == 2 { -1234.5678 } else { 1234.5678 })).unwrap();
    }
    let formats = [
        (1, CellFormat::Number { decimals: Some(2), thousands: true, negative: 0 }),
        (2, CellFormat::Number { decimals: Some(1), thousands: false, negative: 3 }),
        (3, CellFormat::Currency { code: "GBP".into(), decimals: Some(2), thousands: true, negative: 0, accounting: false }),
        (4, CellFormat::Currency { code: "EUR".into(), decimals: Some(2), thousands: false, negative: 0, accounting: true }),
        (5, CellFormat::Percent { decimals: Some(0), thousands: false, negative: 0 }),
        (6, CellFormat::Fraction { accuracy: 4 }),
        (7, CellFormat::Scientific { decimals: Some(3) }),
        (8, CellFormat::Base { base: 16, places: 4, minus: true }),
        (9, CellFormat::Duration { style: 2, largest: 2, smallest: 8, automatic: false }),
        (10, CellFormat::Text),
    ];
    for (r, f) in &formats {
        d.set_cells_format(&mut h, t, (*r, 1, *r, 1), f).unwrap();
    }
    let red = Color { r: 1.0, g: 0.23, b: 0.19, a: 1.0 };
    d.set_cells_fill(&mut h, t, (1, 2, 2, 2), Some(&FillSpec::Color(red))).unwrap();
    d.set_cells_fill(&mut h, t, (3, 2, 3, 2), Some(&FillSpec::Gradient { from: Color { r: 0.34, g: 0.76, b: 1.0, a: 1.0 }, to: Color { r: 0.0, g: 0.46, b: 0.73, a: 1.0 }, angle: 270.0 })).unwrap();
    let blue = StrokeSpec { color: Color { r: 0.0, g: 0.63, b: 1.0, a: 1.0 }, width: 2.0, dash: Dash::Solid };
    d.set_cells_border(&mut h, t, (5, 1, 7, 2), BorderEdges::Outside, Some(&blue)).unwrap();
    d.set_cells_border(&mut h, t, (5, 1, 7, 2), BorderEdges::Inside, Some(&StrokeSpec { color: Color::BLACK, width: 1.0, dash: Dash::Dashed })).unwrap();
    d.set_cells_border(&mut h, t, (9, 2, 9, 2), BorderEdges::Bottom, None).unwrap();
    d.set_table_gridline(&mut h, t, Gridline::BodyColumns, true);
    d.set_table_outline(&mut h, t, Some(&StrokeSpec { color: Color::BLACK, width: 3.0, dash: Dash::Solid }));
    d.set_table_band_color(&mut h, t, Color { r: 0.85, g: 0.95, b: 0.85, a: 1.0 });
    d.bump_table_font_size(&mut h, t, 2.0);
    d.end(&mut h);
    d.save_as(&out).unwrap();
    let d = Document::open(&out).unwrap();
    let DrawableKind::Table(tb) = d.scene(&d.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == t).unwrap().kind else { panic!() };
    for r in 1..=10 {
        println!("B{}: {:?}", r + 1, tb.cells[(r * tb.cols + 1) as usize]);
    }
}
