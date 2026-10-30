//! Undo of cell formatting on an imported Excel workbook: each step reverts exactly, many
//! steps all revert. usage: undoprobe file.xlsx
use nm_core::kn_model::edit::History;
use nm_core::kn_model::format::{Dash, FillSpec, StrokeSpec, TextProp};
use nm_core::kn_model::tablefmt::BorderEdges;
use nm_core::kn_model::Color;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut wb = nm_core::xlsx::open_xlsx(std::path::Path::new(&a[1])).unwrap().workbook;
    let sheet = wb.sheets()[0].clone();
    let t = wb.tables(&sheet)[0].info;
    let state = |wb: &nm_core::Workbook| {
        let d = wb.table_data(t).unwrap();
        (d.cells.clone(), d.cell_styles.clone(), d.h_strokes.clone(), d.v_strokes.clone())
    };
    let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
    let mut h = History::default();
    let mut states = vec![state(&wb)];
    for i in 0..60u32 {
        let b = (2 + i % 5, 1 + i % 4, 3 + i % 5, 2 + i % 4);
        wb.doc.begin(&mut h, "Edit");
        let r = match i % 3 {
            0 => wb.doc.set_cells_fill(&mut h, t, b, Some(&FillSpec::Color(Color { g: i as f32 / 60.0, ..red }))).map(|_| ()),
            1 => wb.doc.set_cells_text_prop(&mut h, t, b, &TextProp::Color(Color { b: i as f32 / 60.0, ..red })).map(|_| ()),
            _ => wb.doc.set_cells_border(&mut h, t, b, BorderEdges::All, Some(&StrokeSpec { color: red, width: 1.0 + i as f32 / 30.0, dash: Dash::Solid })),
        };
        wb.doc.end(&mut h);
        if let Err(e) = r {
            println!("edit {i} failed: {e}");
        }
        let s = state(&wb);
        if s == *states.last().unwrap() {
            println!("edit {i} ({}) changed nothing visible", ["fill", "text colour", "border"][(i % 3) as usize]);
        }
        states.push(s);
    }
    println!("undo steps recorded: {}", h.undo_len());
    let mut bad = 0;
    for i in (0..60).rev() {
        if !wb.doc.undo(&mut h) {
            println!("undo {i}: nothing to undo");
            bad += 1;
            continue;
        }
        let now = state(&wb);
        if now != states[i] {
            let e = &states[i];
            let diff = |a: usize| a;
            let nc = now.0.iter().zip(&e.0).filter(|(x, y)| x != y).count();
            let ns = now.1.iter().zip(&e.1).filter(|(x, y)| x != y).count();
            let first = now.1.iter().zip(&e.1).position(|(x, y)| x != y);
            let nh = now.2.iter().zip(&e.2).filter(|(x, y)| x != y).count();
            let nv = now.3.iter().zip(&e.3).filter(|(x, y)| x != y).count();
            if bad < 2 {
                if let Some(k) = first {
                    println!("  style {k}: now {:?}\n  want {:?}", now.1[k], e.1[k]);
                }
            }
            println!("undo back to step {i}: state differs ({}) cells {} styles {} h {} v {} lens {:?}", ["fill", "text colour", "border"][(i % 3) as usize], diff(nc), ns, nh, nv, (now.1.len(), e.1.len()));
            bad += 1;
        }
    }
    println!("{} of 60 undos wrong", bad);
}
