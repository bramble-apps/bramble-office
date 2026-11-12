//! The Smart Tempo Editor's Hint mode and Beat Marker mode (lg-audio hints): the Hint Mode
//! button (on by default), downbeat hints (a click in the main display's upper third, or D)
//! and beat hints (the lower third, or T) that steer a new analysis, time signature hints on
//! downbeat hints and a general one from the Signature display, moving (drag) and deleting
//! hints (double-click, or Delete Hint in the shortcut menu), Apply Changes (automatic by
//! default; with Apply Changes Automatically off, the button that turns orange after edits),
//! Delete Hints, the Convert commands, locked ranges (Lock Range / Unlock Range: darker, a lock
//! icon, kept through edits and analyses), and in Beat Marker mode the beat markers' handles
//! (Set Downbeat, Scale Selection, Scale Left / Move Right, Scale All, Move All).

use std::path::Path;

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_audio::hints::{self, Handle};
use lg_audio::tempo::BeatMap;

use crate::app::App;
use crate::look;

pub const COMMANDS: &[&str] = &["Delete Hints", "Convert all Beat Hints to Downbeat Hints", "Convert all Downbeat Hints to Beat Hints", "Undo Edit Hint Marker"];

/// What a tap in this playback pass did (tapping in a selection replaces its hints once).
#[derive(Clone, Debug, Default)]
pub struct State {
    pub tapped: bool,
}

const HINT: Color32 = Color32::from_rgb(0xf0, 0x9a, 0x37);

pub fn hint_mode(app: &App) -> bool {
    app.audio_edit.st.region_scope
}

fn seconds(app: &App, file: &Path) -> f64 {
    crate::audioedit::loaded_seconds(app, file).or_else(|| app.audio_edit.st.maps.get(file).and_then(|a| a.map.beats.last().copied())).unwrap_or(0.0)
}

/// A hint edit (one undo step); analysed at once when Apply Changes Automatically is on.
pub fn edit_hints(app: &mut App, file: &Path, f: impl FnOnce(&mut hints::Hints)) -> bool {
    let auto = app.audio_edit.st.apply_auto;
    let secs = seconds(app, file);
    let Some(base) = app.audio_edit.st.maps.get(file).map(|a| a.original.clone()) else { return false };
    crate::smarttempo::edit(app, file, |m| {
        f(&mut m.hints);
        if auto {
            let h = m.hints.clone();
            *m = hints::reanalyze(&base, m, &h, secs);
        }
    })
}

/// Apply Changes / Analyze Tempo with hints: a new analysis steered by the hints.
pub fn apply(app: &mut App, file: &Path) -> bool {
    edit_hints(app, file, |h| h.pending = true) && {
        if !app.audio_edit.st.apply_auto {
            let secs = seconds(app, file);
            let Some(base) = app.audio_edit.st.maps.get(file).map(|a| a.original.clone()) else { return false };
            if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
                let h = a.map.hints.clone();
                a.map = hints::reanalyze(&base, &a.map, &h, secs);
            }
        }
        true
    }
}

fn file_of(app: &App) -> Option<std::path::PathBuf> {
    crate::smarttempo::selected(app).map(|r| r.file)
}

/// The playhead's place in the file (seconds).
fn play_sec(app: &App) -> Option<f64> {
    let rt = crate::smarttempo::selected(app)?;
    Some(crate::smarttempo::tick_to_file(app, &rt, app.position()))
}

/// The selection (seconds), from the beat range.
fn sel_secs(app: &App, file: &Path) -> Option<(f64, f64)> {
    let (a, b) = app.audio_edit.st.range?;
    let m = &app.audio_edit.st.maps.get(file)?.map;
    Some((*m.beats.get(a.min(b))?, *m.beats.get(a.max(b))?))
}

/// Smart Tempo commands that hints change (run before smarttempo's own).
pub fn command(app: &mut App, name: &str) -> bool {
    if !app.playing() {
        app.audio_edit.st.hint.tapped = false;
    }
    let Some(f) = file_of(app) else { return false };
    if !app.audio_edit.st.maps.contains_key(&f) {
        return false;
    }
    // D or T in Beat Marker mode: Hint mode, and the tap adds a hint
    if matches!(name, "Tap Tempo - Smart Tempo Editor" | "Tap Tempo (Downbeat)") && !hint_mode(app) {
        app.audio_edit.st.region_scope = true;
    }
    match name {
        "Tap Tempo - Smart Tempo Editor" | "Tap Tempo (Downbeat)" if hint_mode(app) => {
            let down = name.contains("Downbeat");
            if !app.playing() {
                // D while stopped starts playback; later presses set hints
                app.play_stop();
                return true;
            }
            let Some(s) = play_sec(app) else { return false };
            let sel = sel_secs(app, &f).filter(|&(a, b)| s >= a && s <= b);
            let first = !app.audio_edit.st.hint.tapped;
            app.audio_edit.st.hint.tapped = true;
            edit_hints(app, &f, |h| {
                if let (true, Some((a, b))) = (first, sel) {
                    h.clear_range(a, b);
                }
                h.add(s, down);
            })
        }
        "Delete Hints" | "Clear Hints" => edit_hints(app, &f, |h| {
            h.list.clear();
            h.general_sig = None;
            h.pending = true;
        }),
        "Convert all Beat Hints to Downbeat Hints" => edit_hints(app, &f, |h| h.convert(true)),
        "Convert all Downbeat Hints to Beat Hints" => edit_hints(app, &f, |h| h.convert(false)),
        "Apply Changes" => apply(app, &f),
        "Analyze Tempo" if app.audio_edit.st.maps.get(&f).is_some_and(|a| !a.map.hints.list.is_empty() || a.map.hints.general_sig.is_some()) => apply(app, &f),
        "Lock Range" => {
            let Some((a, b)) = sel_secs(app, &f) else { return false };
            crate::smarttempo::edit(app, &f, |m| m.hints.lock(a, b))
        }
        "Unlock Range" => {
            let Some((a, b)) = sel_secs(app, &f).or_else(|| play_sec(app).map(|s| (s, s))) else { return false };
            crate::smarttempo::edit(app, &f, |m| {
                m.hints.unlock(a, b);
            })
        }
        _ => false,
    }
}

/// The Signature display: in Hint mode a general time signature hint; in Beat Marker mode the
/// signature for the whole file (signature hints removed).
pub fn set_signature(app: &mut App, file: &Path, n: u8, d: u8) {
    if hint_mode(app) {
        edit_hints(app, file, |h| {
            h.general_sig = Some((n, d));
            h.pending = true;
        });
    } else {
        crate::smarttempo::edit(app, file, |m| {
            m.beats_per_bar = n;
            m.beat_unit = d;
            m.hints.general_sig = None;
            for h in &mut m.hints.list {
                h.sig = None;
            }
        });
    }
}

/// The Apply Changes button (only with Apply Changes Automatically off; orange after edits).
pub fn apply_button(app: &mut App, ui: &mut egui::Ui, at: Pos2, file: &Path) {
    if app.audio_edit.st.apply_auto {
        return;
    }
    let pending = app.audio_edit.st.maps.get(file).is_some_and(|a| a.map.hints.pending);
    let r = Rect::from_min_size(at - vec2(0.0, 12.0), vec2(104.0, 24.0));
    let resp = ui.interact(r, ui.scope_id().with("st-apply"), Sense::click()).on_hover_text("Analyze again with the hints");
    let p = ui.painter();
    p.rect_filled(r, 6.0, if pending { Color32::from_rgb(0xe8, 0x86, 0x2a) } else { look::group_bg() });
    p.text(r.center(), Align2::CENTER_CENTER, "Apply Changes", look::regular(12.5), if pending { Color32::WHITE } else { look::text_dim() });
    if resp.clicked() {
        apply(app, file);
    }
}

/// Locked ranges: darker, with a lock icon (main display; `ov_x` maps seconds to the overview).
pub fn draw_locks(map: &BeatMap, p: &egui::Painter, main: Rect, x_of: &dyn Fn(f64) -> f32, ov: Option<(Rect, &dyn Fn(f64) -> f32)>) {
    for &(a, b) in &map.hints.locks {
        let r = Rect::from_min_max(pos2(x_of(a), main.min.y), pos2(x_of(b), main.max.y)).intersect(main);
        if r.width() > 0.0 {
            p.rect_filled(r, 0.0, Color32::from_black_alpha(110));
            crate::glyphs::paint(p, Rect::from_min_size(pos2(r.min.x + 4.0, r.min.y + 4.0), vec2(14.0, 14.0)), crate::glyphs::ph_fill::LOCK, 12.0, look::text());
        }
        if let Some((ovr, ox)) = ov {
            let r = Rect::from_min_max(pos2(ox(a), ovr.min.y), pos2(ox(b), ovr.max.y)).intersect(ovr);
            p.rect_filled(r, 0.0, Color32::from_black_alpha(110));
        }
    }
}

/// Hint mode's main display: draws the hints and takes the clicks and drags. False in Beat
/// Marker mode (the editor's own gestures run).
#[allow(clippy::too_many_arguments)]
pub fn hint_view(app: &mut App, ui: &mut egui::Ui, id: egui::Id, main: Rect, ruler: Rect, x_of: &dyn Fn(f64) -> f32, s_of: &dyn Fn(f32) -> f64, file: &Path, rt: Option<&crate::smarttempo::RegionTime>) -> bool {
    if !hint_mode(app) {
        return false;
    }
    let Some(map) = app.audio_edit.st.maps.get(file).map(|a| a.map.clone()) else { return false };
    let p = ui.painter_at(Rect::from_min_max(ruler.min, main.max));
    let third = main.height() / 3.0;
    let (upper, lower) = (main.min.y + third, main.max.y - third);
    // the thirds, faintly: downbeat hints above, beat hints below
    p.hline(main.x_range(), upper, Stroke::new(1.0, Color32::from_white_alpha(10)));
    p.hline(main.x_range(), lower, Stroke::new(1.0, Color32::from_white_alpha(10)));
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|h| main.contains(*h));
    let near = |px: f32| -> Option<usize> {
        let k = map.hints.nearest(s_of(px), f64::MAX)?;
        ((x_of(map.hints.list[k].sec) - px).abs() < 5.0).then_some(k)
    };
    let hovered = hover.and_then(|h| near(h.x));
    for (k, h) in map.hints.list.iter().enumerate() {
        let x = x_of(h.sec);
        if !main.x_range().contains(x) {
            continue;
        }
        let hot = hovered == Some(k);
        let w = if hot { 2.0 } else { 1.5 };
        if h.downbeat {
            p.vline(x, main.min.y..=lower, Stroke::new(w, HINT));
            p.circle_filled(pos2(x, main.min.y + 7.0), 5.0, HINT);
            if let Some((n, d)) = h.sig {
                p.text(pos2(x + 8.0, main.min.y + 7.0), Align2::LEFT_CENTER, format!("{n}/{d}"), look::semibold(11.0), HINT);
            }
        } else {
            p.vline(x, upper..=main.max.y, Stroke::new(w, HINT.gamma_multiply(0.8)));
            p.circle_stroke(pos2(x, main.max.y - 7.0), 4.5, Stroke::new(1.5, HINT));
        }
    }
    let resp = ui.interact(main, id.with("hints"), Sense::click_and_drag());
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    // a downbeat hint's signature indicator (hover), opening the time signature menu
    let sig_key = id.with("hint-sig");
    if let Some(k) = hovered.filter(|&k| map.hints.list[k].downbeat) {
        ui.data_mut(|d| d.insert_temp(sig_key, k));
    }
    if let Some(k) = ui.data(|d| d.get_temp::<usize>(sig_key)).filter(|&k| k < map.hints.list.len()) {
        let x = x_of(map.hints.list[k].sec);
        let r = Rect::from_min_size(pos2(x + 4.0, ruler.max.y - 16.0), vec2(30.0, 14.0));
        if hover.is_some_and(|h| (h.x - x).abs() < 40.0) {
            ui.painter().rect_filled(r, 3.0, HINT);
            let label = map.hints.list[k].sig.map_or("4/4".to_string(), |(n, d)| format!("{n}/{d}"));
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, look::regular(10.0), Color32::BLACK);
            let sresp = ui.interact(r, id.with(("hint-sig-b", k)), Sense::click()).on_hover_text("Time signature hint");
            egui::Popup::menu(&sresp).show(|ui| {
                for (n, d) in [(2u8, 4u8), (3, 4), (4, 4), (5, 4), (6, 4), (7, 4), (6, 8), (7, 8), (9, 8), (12, 8)] {
                    if ui.button(format!("{n}/{d}")).clicked() {
                        edit_hints(app, file, |h| {
                            if let Some(x) = h.list.get_mut(k) {
                                x.sig = Some((n, d));
                                h.pending = true;
                            }
                        });
                        ui.close();
                    }
                }
                if ui.button("No Signature Hint").clicked() {
                    edit_hints(app, file, |h| {
                        if let Some(x) = h.list.get_mut(k) {
                            x.sig = None;
                        }
                    });
                    ui.close();
                }
            });
        }
    }
    let drag_key = id.with("hint-drag");
    if resp.drag_started() {
        if let Some(pp) = ui.input(|i| i.pointer.press_origin()) {
            match near(pp.x) {
                Some(k) => {
                    // one undo step for the drag
                    crate::smarttempo::edit(app, file, |m| m.hints.pending = true);
                    ui.data_mut(|d| d.insert_temp(drag_key, k as i64));
                }
                None => {
                    let b = map.nearest_beat(s_of(pp.x)).unwrap_or(0);
                    ui.data_mut(|d| d.insert_temp(drag_key, -1 - b as i64));
                }
            }
        }
    }
    if let (true, Some(pp)) = (resp.dragged(), resp.interact_pointer_pos()) {
        let k = ui.data(|d| d.get_temp::<i64>(drag_key)).unwrap_or(i64::MIN);
        if k >= 0 {
            let mut s = s_of(pp.x).max(0.0);
            if app.audio_edit.st.snap_transients && !ui.input(|i| i.modifiers.shift) {
                let rate = crate::audioedit::rate_of(app, file).unwrap_or(48_000) as f64;
                if let Some(t) = crate::audioedit::transients_of(app, file).into_iter().map(|f| f as f64 / rate).min_by(|a, b| (*a - s).abs().total_cmp(&(*b - s).abs())) {
                    if (x_of(t) - pp.x).abs() < 6.0 {
                        s = t;
                    }
                }
            }
            if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
                let nk = a.map.hints.move_to(k as usize, s);
                ui.data_mut(|d| d.insert_temp(drag_key, nk as i64));
            }
        } else if k != i64::MIN {
            let from = (-1 - k) as usize;
            let to = map.nearest_beat(s_of(pp.x)).unwrap_or(from);
            app.audio_edit.st.range = Some((from.min(to), from.max(to)));
        }
    }
    if resp.drag_stopped() {
        let k = ui.data(|d| d.get_temp::<i64>(drag_key)).unwrap_or(-1);
        ui.data_mut(|d| d.remove::<i64>(drag_key));
        if k >= 0 && app.audio_edit.st.apply_auto {
            apply(app, file);
        }
    }
    if resp.double_clicked() {
        if let Some(k) = resp.interact_pointer_pos().and_then(|pp| near(pp.x)) {
            edit_hints(app, file, |h| h.remove(k));
        }
    } else if resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            let shift = ui.input(|i| i.modifiers.shift);
            if near(pp.x).is_some() {
            } else if shift && app.audio_edit.st.range.is_some() {
                // Shift-click beside a selection extends it
                let b = map.nearest_beat(s_of(pp.x)).unwrap_or(0);
                let (a0, b0) = app.audio_edit.st.range.unwrap();
                app.audio_edit.st.range = Some((a0.min(b), b0.max(b)));
            } else if pp.y < upper || pp.y > lower {
                let s = s_of(pp.x).max(0.0);
                edit_hints(app, file, |h| h.add(s, pp.y < upper));
            } else {
                app.audio_edit.st.range = None;
                if let Some(rt) = rt {
                    let tk = crate::smarttempo::file_to_tick(app, rt, s_of(pp.x));
                    if tk >= 0.0 {
                        app.locate(tk);
                    }
                }
            }
        }
    }
    let at_hint = resp.interact_pointer_pos().and_then(|pp| near(pp.x));
    let menu_key = id.with("hint-menu");
    if resp.secondary_clicked() {
        ui.data_mut(|d| d.insert_temp(menu_key, at_hint.map_or(-1, |k| k as i64)));
    }
    resp.context_menu(|ui| {
        ui.set_min_width(180.0);
        let k = ui.data(|d| d.get_temp::<i64>(menu_key)).unwrap_or(-1);
        if ui.add_enabled(k >= 0, egui::Button::new("Delete Hint")).clicked() {
            edit_hints(app, file, |h| h.remove(k as usize));
            ui.close();
        }
        if ui.button("Apply Changes").clicked() {
            apply(app, file);
            ui.close();
        }
        lock_items(app, ui, file);
    });
    true
}

/// Lock Range / Unlock Range in the main display's shortcut menu.
pub fn lock_items(app: &mut App, ui: &mut egui::Ui, file: &Path) {
    let sel = sel_secs(app, file);
    let locked = app.audio_edit.st.maps.get(file).is_some_and(|a| sel.is_some_and(|(x, y)| a.map.hints.locks.iter().any(|&(p, q)| q >= x && p <= y)));
    ui.separator();
    if locked {
        if ui.button("Unlock Range").clicked() {
            command(app, "Unlock Range");
            ui.close();
        }
    } else if ui.add_enabled(sel.is_some(), egui::Button::new("Lock Range")).clicked() {
        command(app, "Lock Range");
        ui.close();
    }
}

/// Beat Marker mode: the hovered (or selected) beat marker's handles, each with its tag.
#[allow(clippy::too_many_arguments)]
pub fn handles(app: &mut App, ui: &mut egui::Ui, id: egui::Id, main: Rect, x_of: &dyn Fn(f64) -> f32, s_of: &dyn Fn(f32) -> f64, file: &Path) {
    if hint_mode(app) {
        return;
    }
    let Some(map) = app.audio_edit.st.maps.get(file).map(|a| a.map.clone()) else { return };
    let drag_key = id.with("handle-drag");
    let active: Option<(usize, Handle, (usize, usize), BeatMap)> = ui.data(|d| d.get_temp(drag_key));
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|h| main.contains(*h));
    let beat = active.as_ref().map(|a| a.0).or_else(|| {
        let h = hover?;
        let k = map.nearest_beat(s_of(h.x))?;
        ((x_of(map.beats[k]) - h.x).abs() < 14.0).then_some(k)
    });
    let Some(i) = beat.filter(|&i| i < map.beats.len()) else { return };
    if map.hints.locked(map.beats[i]) {
        return;
    }
    let x = x_of(map.beats[i]);
    let p = ui.painter();
    for (n, h) in Handle::ALL.into_iter().enumerate() {
        if !hints::shows(&map, i, h) {
            continue;
        }
        let y = main.min.y + main.height() * (0.12 + 0.19 * n as f32);
        let c = pos2(x, y);
        let r = Rect::from_center_size(c, vec2(12.0, 12.0));
        let resp = ui.interact(r, id.with(("handle", n)), Sense::click_and_drag()).on_hover_text(h.tag());
        let on = resp.hovered() || active.as_ref().is_some_and(|a| a.1 == h);
        p.circle_filled(c, if on { 5.5 } else { 4.5 }, if on { Color32::WHITE } else { Color32::from_gray(0xc8) });
        p.circle_stroke(c, if on { 5.5 } else { 4.5 }, Stroke::new(1.0, Color32::from_gray(0x30)));
        if h == Handle::SetDownbeat && resp.clicked() {
            crate::smarttempo::edit(app, file, |m| {
                let (s, hh) = (m.beats[i], m.hints.clone());
                hints::drag(m, i, h, (i, i), s, &hh);
            });
        }
        if resp.drag_started() && h != Handle::SetDownbeat {
            let range = app.audio_edit.st.range.filter(|&(a, b)| i >= a.min(b) && i <= a.max(b)).unwrap_or_else(|| hints::default_range(&map, i, h));
            app.audio_edit.st.range = Some(range);
            // one undo step for the drag
            crate::smarttempo::edit(app, file, |m| m.variable = true);
            ui.data_mut(|d| d.insert_temp(drag_key, (i, h, range, map.clone())));
        }
    }
    if let Some((i, h, range, orig)) = active {
        let down = ui.input(|i| i.pointer.primary_down());
        if let Some(pp) = ui.input(|i| i.pointer.latest_pos()) {
            let s = s_of(pp.x);
            let mut m = orig.clone();
            let lk = orig.hints.clone();
            if hints::drag(&mut m, i, h, range, s, &lk) {
                if let Some(a) = app.audio_edit.st.maps.get_mut(file) {
                    a.map = m;
                }
                app.help_tag = Some(vec![(format!("{}:", h.tag()), format!("{:+.3} s", s - orig.beats[i]))]);
            }
        }
        if !down {
            ui.data_mut(|d| d.remove::<(usize, Handle, (usize, usize), BeatMap)>(drag_key));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hint_mode_is_the_default() {
        assert!(crate::smarttempo::State::default().region_scope);
    }
}
