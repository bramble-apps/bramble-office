//! The Multiple Rest dialog (double-click a multiple bar rest): Auto Length or a number of
//! bars, and the Type buttons, modern (an H-bar) or church style.

use crate::app::App;
use lg_core::score::{MultiRestText, SymbolKind};

#[derive(Clone, Debug)]
struct Win {
    id: u64,
    index: usize,
    auto: bool,
    bars: u32,
    church: bool,
}

fn win_id() -> egui::Id {
    egui::Id::unique("score-multi-rest")
}

/// Opens the dialog on a track's multiple rest (symbol `index`), showing `bars` it covers now.
pub fn open(ctx: &egui::Context, id: u64, index: usize, text: &str, bars: u32) {
    let m = MultiRestText::parse(text);
    ctx.data_mut(|d| d.insert_temp(win_id(), Some(Win { id, index, auto: m.bars.is_none(), bars: m.bars.unwrap_or(bars).max(1), church: m.church })));
}

/// Applies the dialog's settings to the symbol (one undo step).
pub fn apply(app: &mut App, id: u64, index: usize, auto: bool, bars: u32, church: bool) {
    app.checkpoint("Multiple Rest");
    if let Some(s) = app.project.as_mut().and_then(|p| p.score.track_mut(id).symbols.get_mut(index)).filter(|s| s.kind == SymbolKind::MultiRest) {
        s.text = MultiRestText { bars: (!auto).then_some(bars.max(1)), church }.text();
    }
    app.after_edit();
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut w) = ctx.data(|m| m.get_temp::<Option<Win>>(win_id())).flatten() else { return };
    let mut open = true;
    let mut done: Option<bool> = None;
    egui::Window::new("Multiple Rest").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        ui.checkbox(&mut w.auto, "Auto Length");
        ui.horizontal(|ui| {
            ui.add_enabled(!w.auto, egui::Label::new("Bars:"));
            ui.add_enabled(!w.auto, egui::DragValue::new(&mut w.bars).range(1..=999));
        });
        ui.horizontal(|ui| {
            ui.label("Type:");
            if ui.selectable_label(!w.church, "Modern").clicked() {
                w.church = false;
            }
            if ui.selectable_label(w.church, "Church").clicked() {
                w.church = true;
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
        });
    });
    if done == Some(true) {
        apply(app, w.id, w.index, w.auto, w.bars, w.church);
    }
    if !open || done.is_some() {
        ctx.data_mut(|d| d.remove_temp::<Option<Win>>(win_id()));
    } else {
        ctx.data_mut(|d| d.insert_temp(win_id(), Some(w)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lg_core::score::Symbol;

    #[test]
    fn dialog_sets_length_and_type_and_score_spans_follow() {
        let mut app = crate::tm::test_app();
        let p = app.project.as_mut().unwrap();
        let t = p.tracks.iter().position(|t| t.regions.iter().any(|r| r.audio.is_none())).unwrap();
        let id = p.tracks[t].extra.id;
        // one MIDI region of eight bars with a note in bar 7 (0-based 6); the rest from bar 2
        let bar = lg_core::project::bar_ticks(p.signature);
        let tr = &mut p.tracks[t];
        tr.regions.retain(|r| r.audio.is_none());
        tr.regions.truncate(1);
        tr.regions[0].start = 0;
        tr.regions[0].length = bar * 8;
        tr.regions[0].notes = vec![lg_core::events::Note { channel: 0, pitch: 60, velocity: 90, pos: bar * 6, len: 480, muted: false }];
        p.score.track_mut(id).symbols.push(Symbol { tick: bar, kind: SymbolKind::MultiRest, text: "A 0".into(), ..Default::default() });
        let si = p.score.track(id).unwrap().symbols.len() - 1;
        let m = crate::score::multi_rests(&app, t, bar, 8);
        assert_eq!((m[0].0, m[0].1, m[0].2), (1, 5, false));
        apply(&mut app, id, si, false, 3, true);
        let m = crate::score::multi_rests(&app, t, bar, 8);
        assert_eq!((m[0].1, m[0].2), (3, true));
        app.undo();
        assert_eq!(crate::score::multi_rests(&app, t, bar, 8)[0].1, 5);
    }
}
