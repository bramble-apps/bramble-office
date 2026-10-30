//! Undo of cell formatting: fills, text colour and borders each go back exactly, however many.
use nm_core::kn_model::edit::History;
use nm_core::kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use nm_core::kn_model::tablefmt::BorderEdges;
use nm_core::kn_model::Color;

#[test]
fn borders_fills_and_text_colour_undo() {
    nm_core::init();
    let k = nm_core::kn_format::KeyFile::from_zip(nm_core::BLANK).unwrap();
    let mut wb = nm_core::Workbook { doc: nm_core::kn_model::Document::from_file(k) };
    let sheet = wb.sheets()[0].clone();
    let t = wb.tables(&sheet)[0].info;
    let state = |wb: &nm_core::Workbook| {
        let d = wb.table_data(t).unwrap();
        (d.cell_styles.clone(), d.h_strokes.clone(), d.v_strokes.clone())
    };
    let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    let mut h = History::default();
    let mut states = vec![state(&wb)];
    for i in 0..30u32 {
        // overlapping blocks, so later borders change layers earlier ones made
        let b = (1 + i % 4, 1 + i % 3, 2 + i % 4, 2 + i % 3);
        wb.doc.begin(&mut h, "Format");
        match i % 3 {
            0 => wb.doc.set_cells_border(&mut h, t, b, BorderEdges::All, Some(&StrokeSpec { color: red, width: 1.0 + i as f32, dash: Dash::Solid })).unwrap(),
            1 => {
                wb.doc.set_cells_fill(&mut h, t, b, Some(&FillSpec::Color(red))).unwrap();
            }
            _ => {
                let _ = wb.doc.set_cells_text_prop(&mut h, t, b, &TextProp::Color(red));
            }
        }
        wb.doc.end(&mut h);
        states.push(state(&wb));
    }
    for i in (0..30).rev() {
        assert!(wb.doc.undo(&mut h));
        assert!(state(&wb) == states[i], "undo back to step {i} isn't what it was");
    }
}
