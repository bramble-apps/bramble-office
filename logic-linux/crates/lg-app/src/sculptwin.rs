//! Sculpture's window, laid out as Logic's: the global strip across the top; the sound engine
//! below it (objects and the pickup display on the left, the Waveshaper above and the filter
//! below the circular Material Pad in the middle, the delay, the amplitude envelope with the
//! level and limiter, and the body EQ on the right); the modulation area under it (LFOs,
//! vibrato, jitter, note-on random and velocity sources in one pane, the Morph Pad, and the two
//! control envelopes with the Morph Envelope); the controller assignments along the bottom edge.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Shape, Stroke};
use lg_synth::sculpture as s;
use lg_synth::sculpture::Sculpture;
use lg_synth::sculpture_fx as fx;

use crate::app::App;
use crate::instwin;
use crate::look;
use crate::synthui::{self, rgb, Pane, Skin};

const SKIN: Skin = Skin::new(0x34373c, 0x2a2d31, 0x7fb0e0).silver();
const ORANGE: Color32 = rgb(0xf0a030);
const GREEN: Color32 = rgb(0x6fce7a);
const BLUE: Color32 = rgb(0x5aa0e8);
const RED: Color32 = rgb(0xe05050);
const STRING: Color32 = rgb(0x7ad070);
const EXT: [u32; 5] = [s::MONO_MODE, s::MONO_BEND, s::RENDER_MODE, s::SURR_RANGE, s::SURR_DIV];
const SIZE: egui::Vec2 = vec2(1120.0, 742.0);

/// What the engine shows right now (read before the panel is drawn).
#[derive(Default, Clone)]
struct Live {
    shape: Option<Vec<f32>>,
    morph_now: Option<(f32, f32)>,
    menv: Option<f32>,
    env: [Option<f32>; 2],
    learning: Option<u32>,
    dirty: bool,
}

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let live = instwin::with_inst::<Sculpture, _>(app, t, |sc| {
        let mut v = vec![0.0f32; 160];
        let shape = if sc.string_shape(&mut v) { Some(v) } else { None };
        Live { shape, morph_now: sc.morph_now(), menv: sc.menv_progress(), env: [sc.env_progress(0), sc.env_progress(1)], learning: sc.learning(), dirty: sc.take_dirty() }
    })
    .unwrap_or_default();
    let acts = synthui::window(app, ctx, t, "Sculpture", SIZE, SKIN, &EXT, |p| body(p, &live));
    if live.dirty {
        // the instrument changed its own parameters (a recording ended): keep them in the project
        if let Some((ps, vs)) = instwin::values(app, t) {
            let pairs: Vec<(u32, f32)> = ps.iter().zip(vs).map(|(p, v)| (p.id, v)).collect();
            instwin::set_params(app, t, &pairs);
        }
    }
    for a in acts {
        if a == "rnd" {
            randomize(app, t);
        } else if let Some(id) = a.strip_prefix("learn:").and_then(|x| x.parse::<u32>().ok()) {
            instwin::with_inst::<Sculpture, _>(app, t, |sc| sc.learn(id));
        }
    }
}

/// The Rnd button: randomises the chosen morph points in the chosen group by the intensity.
fn randomize(app: &mut App, t: usize) {
    let Some((ps, vs)) = instwin::values(app, t) else { return };
    let get = |id: u32| ps.iter().position(|p| p.id == id).map(|i| vs[i]).unwrap_or(0.0);
    let sel = get(s::MORPH_SEL).round() as usize;
    let points: Vec<usize> = match get(s::MORPH_POINTS).round() as usize {
        0 => vec![sel],
        1 => vec![0, 1, 2, 3],
        _ => vec![0, 1, 2, 3, 4],
    };
    let group = get(s::MORPH_RNDSET).round() as usize;
    let int = get(s::MORPH_INT);
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7);
    instwin::checkpoint(app, t, "Randomize Morph Points");
    instwin::with_inst::<Sculpture, _>(app, t, |sc| sc.randomize(&points, group, int, seed));
    if let Some((ps2, vs2)) = instwin::values(app, t) {
        let pairs: Vec<(u32, f32)> = ps2.iter().zip(vs2).map(|(p, v)| (p.id, v)).collect();
        instwin::set_params(app, t, &pairs);
    }
}

// ---------------------------------------------------------------------------------------------
// small widgets

fn frac(pr: &lg_dsp::Param, v: f32) -> f32 {
    ((v - pr.min) / (pr.max - pr.min).max(1e-9)).clamp(0.0, 1.0)
}

fn alt(p: &Pane) -> bool {
    p.ui.input(|i| i.modifiers.alt)
}

/// A slider in a rectangle (long side is the travel); Option-click or double-click resets.
fn slider(p: &mut Pane, r: Rect, id: u32, vertical: bool, col: Color32) {
    let pr = p.prm(id);
    let v = p.get(id);
    let resp = p.ui.interact(r, p.id("scs", id), Sense::click_and_drag()).on_hover_text(format!("{}: {}", pr.name, p.shown(id, v)));
    let f = frac(&pr, v);
    let pt = p.painter();
    let track = if vertical { Rect::from_center_size(r.center(), vec2(4.0, r.height() - 8.0)) } else { Rect::from_center_size(r.center(), vec2(r.width() - 8.0, 4.0)) };
    pt.rect_filled(track, 2.0, Color32::from_gray(0x16));
    let zero = if pr.min < 0.0 && pr.max > 0.0 { -pr.min / (pr.max - pr.min) } else { 0.0 };
    if vertical {
        let (cy, zy) = (track.max.y - track.height() * f, track.max.y - track.height() * zero);
        pt.rect_filled(Rect::from_min_max(pos2(track.min.x, cy.min(zy)), pos2(track.max.x, cy.max(zy))), 2.0, col.gamma_multiply(0.8));
        pt.rect_filled(Rect::from_center_size(pos2(r.center().x, cy), vec2(r.width().min(14.0), 7.0)), 2.0, Color32::from_gray(0xdc));
    } else {
        let (cx, zx) = (track.min.x + track.width() * f, track.min.x + track.width() * zero);
        pt.rect_filled(Rect::from_min_max(pos2(cx.min(zx), track.min.y), pos2(cx.max(zx), track.max.y)), 2.0, col.gamma_multiply(0.8));
        pt.rect_filled(Rect::from_center_size(pos2(cx, r.center().y), vec2(7.0, r.height().min(14.0))), 2.0, Color32::from_gray(0xdc));
    }
    if resp.double_clicked() || (resp.clicked() && alt(p)) {
        p.put(id, pr.default);
    } else if resp.dragged() || resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            let f = if vertical { (track.max.y - pp.y) / track.height() } else { (pp.x - track.min.x) / track.width() };
            p.put(id, pr.min + f.clamp(0.0, 1.0) * (pr.max - pr.min));
        }
    }
}

/// A caption above or beside a control; `morph` marks the value bar orange (morphable).
fn cap(p: &Pane, x: f32, y: f32, text: &str, morph: bool) {
    p.text(x, y, Align2::LEFT_CENTER, text, 10.0, if morph { ORANGE } else { p.skin.dim });
}

/// A labelled horizontal slider; the caption sits above it.
fn hs(p: &mut Pane, x: f32, y: f32, w: f32, id: u32, label: &str, morph: bool) {
    cap(p, x, y - 1.0, label, morph);
    let r = p.rect(x, y + 5.0, w, 14.0);
    slider(p, r, id, false, if morph { ORANGE } else { p.skin.accent });
}

/// A knob whose label turns orange when it can be morphed.
fn kn(p: &mut Pane, x: f32, y: f32, d: f32, id: u32, label: &str, morph: bool) {
    p.knob(x, y, d, id, label);
    if morph {
        let r = p.rect(x - d * 0.5, y + d + 15.0, d, 2.0);
        p.painter().rect_filled(r, 1.0, ORANGE);
    }
}

/// A pop-up of text items under a button showing `label`; returns the chosen index.
fn popup(p: &mut Pane, r: Rect, salt: (u32, u32), label: &str, items: &[String], cur: usize) -> Option<usize> {
    let resp = p.ui.interact(r, egui::Id::unique(("sc-pop", p.t, salt)), Sense::click());
    let pt = p.painter();
    pt.rect_filled(r, 4.0, if resp.hovered() { Color32::from_gray(0x30) } else { Color32::from_gray(0x22) });
    pt.with_clip_rect(r).text(pos2(r.min.x + 6.0, r.center().y), Align2::LEFT_CENTER, label, look::regular(10.5), p.skin.text);
    crate::glyphs::paint(&pt, Rect::from_center_size(pos2(r.max.x - 8.0, r.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
            for (i, c) in items.iter().enumerate() {
                if ui.add(egui::Button::selectable(i == cur, c.as_str())).clicked() {
                    pick = Some(i);
                    ui.close();
                }
            }
        });
    });
    pick
}

/// A tempo note value pop-up for a parameter holding an index into `NOTE_VALUES`.
fn note_menu(p: &mut Pane, x: f32, y: f32, w: f32, id: u32) {
    let cur = p.geti(id).min(fx::NOTE_VALUES.len() - 1);
    let items: Vec<String> = fx::NOTE_VALUES.iter().map(|n| n.0.to_string()).collect();
    let r = p.rect(x, y, w, 16.0);
    if let Some(i) = popup(p, r, (id, 1), fx::NOTE_VALUES[cur].0, &items, cur) {
        p.put(id, i as f32);
    }
}

/// Copy / Paste / Clear on a right-click (Control-click) of an area: `ids` are the parameters
/// it holds, `clear` the values Clear puts back.
fn clip_menu(p: &mut Pane, r: Rect, salt: u32, ids: &[u32], clear: &[(u32, f32)]) {
    let resp = p.ui.interact(r, egui::Id::unique(("sc-clip", p.t, salt)), Sense::click());
    let key = egui::Id::unique(("sculpt-clip", salt / 100));
    let vals: Vec<f32> = ids.iter().map(|&i| p.get(i)).collect();
    let mut act = 0;
    resp.context_menu(|ui| {
        if ui.button("Copy").clicked() {
            act = 1;
            ui.close();
        }
        if ui.button("Paste").clicked() {
            act = 2;
            ui.close();
        }
        if ui.button("Clear").clicked() {
            act = 3;
            ui.close();
        }
    });
    match act {
        1 => {
            p.ui.ctx().data_mut(|d| d.insert_temp(key, vals));
        }
        2 => {
            if let Some(v) = p.ui.ctx().data(|d| d.get_temp::<Vec<f32>>(key)).filter(|v| v.len() == ids.len()) {
                for (i, x) in ids.iter().zip(v) {
                    p.put(*i, x);
                }
            }
        }
        3 => {
            for &(i, v) in clear {
                p.put(i, v);
            }
        }
        _ => {}
    }
}

fn mem_get<T: Clone + Send + Sync + 'static>(p: &Pane, key: &str, default: T) -> T {
    let id = egui::Id::unique(("sculpt-ui", p.t, key.to_string()));
    p.ui.ctx().data(|d| d.get_temp::<T>(id)).unwrap_or(default)
}

fn mem_set<T: Clone + Send + Sync + 'static>(p: &Pane, key: &str, v: T) {
    let id = egui::Id::unique(("sculpt-ui", p.t, key.to_string()));
    p.ui.ctx().data_mut(|d| d.insert_temp(id, v));
}

fn tabs(p: &mut Pane, x: f32, y: f32, w: f32, key: &str, names: &[&str]) -> usize {
    let cur: usize = mem_get(p, key, 0usize);
    for (k, n) in names.iter().enumerate() {
        let r = p.rect(x + k as f32 * (w + 3.0), y, w, 18.0);
        if p.button(r, n, k == cur, (key, k)) {
            mem_set(p, key, k);
        }
    }
    mem_get(p, key, 0usize)
}

// ---------------------------------------------------------------------------------------------

fn body(p: &mut Pane, live: &Live) {
    let learning = live.learning;
    global_strip(p);
    objects(p);
    pickups(p, live);
    waveshaper(p);
    material_pad(p, live);
    filter(p);
    delay(p);
    amp_and_level(p);
    body_eq(p);
    // the lower area
    modulation(p);
    morph_pad(p, live);
    right_lower(p, live);
    controllers(p, learning);
}

fn global_strip(p: &mut Pane) {
    p.group(8.0, 6.0, 1104.0, 30.0, "");
    let mut x = 18.0;
    for (id, label, w, step) in [(s::GLIDE, "Glide", 56.0, 0.01), (s::TUNE, "Tune", 56.0, 1.0), (s::WARMTH, "Warmth", 56.0, 0.01), (s::TRANSPOSE, "Transpose", 64.0, 1.0)] {
        let lw = label.len() as f32 * 6.3 + 8.0;
        p.text(x, 21.0, Align2::LEFT_CENTER, label, 10.5, p.skin.dim);
        p.field(x + lw, 11.0, w, 20.0, id, step);
        x += lw + w + 14.0;
    }
    p.seg(x, 12.0, 150.0, 18.0, s::KBMODE, &["Poly", "Mono", "Legato"], false);
    x += 166.0;
    p.text(x, 21.0, Align2::LEFT_CENTER, "Bender Range Up", 10.5, p.skin.dim);
    p.field(x + 94.0, 11.0, 44.0, 20.0, s::BEND_UP, 1.0);
    p.text(x + 148.0, 21.0, Align2::LEFT_CENTER, "Down", 10.5, p.skin.dim);
    p.field(x + 182.0, 11.0, 44.0, 20.0, s::BEND_DOWN, 1.0);
    p.toggle(x + 232.0, 12.0, 54.0, 18.0, s::BEND_LINK, "Linked");
    x += 300.0;
    p.text(x, 21.0, Align2::LEFT_CENTER, "Voices", 10.5, p.skin.dim);
    p.field(x + 40.0, 11.0, 40.0, 20.0, s::POLY, 1.0);
}

fn objects(p: &mut Pane) {
    p.group(8.0, 40.0, 372.0, 236.0, "OBJECTS");
    for o in 0..3usize {
        let x0 = 14.0 + o as f32 * 122.0;
        p.toggle(x0, 58.0, 32.0, 18.0, s::obj(o, s::O_ON), &format!("{}", o + 1));
        p.menu(x0 + 36.0, 58.0, 80.0, 18.0, s::obj(o, s::O_TYPE));
        p.menu(x0, 80.0, 116.0, 16.0, s::obj(o, s::O_GATE));
        kn(p, x0 + 58.0, 98.0, 50.0, s::obj(o, s::O_STR), "Strength", true);
        hs(p, x0, 172.0, 114.0, s::obj(o, s::O_TIM), "Timbre", true);
        hs(p, x0, 198.0, 114.0, s::obj(o, s::O_VAR), "Variation", true);
        let kind = s::kind_of(o, p.geti(s::obj(o, s::O_TYPE)));
        let vel_ok = o < 2 && (1..=8).contains(&kind);
        if vel_ok {
            hs(p, x0, 224.0, 114.0, s::obj(o, s::O_VEL), "VeloSens", false);
        } else {
            // (this object type has no velocity sensitivity: the slider shows dimmed)
            cap(p, x0, 223.0, "VeloSens", false);
            p.painter().rect_filled(p.rect(x0 + 4.0, 234.0, 106.0, 4.0), 2.0, Color32::from_gray(0x26));
        }
    }
}

fn pickups(p: &mut Pane, live: &Live) {
    p.group(8.0, 280.0, 372.0, 184.0, "PICKUPS");
    let area = p.rect(16.0, 300.0, 356.0, 128.0);
    p.painter().rect_filled(area, 5.0, Color32::from_gray(0x12));
    let (x0, x1) = (area.min.x + 12.0, area.max.x - 12.0);
    let ym = area.center().y;
    let xp = |f: f32| x0 + (x1 - x0) * f;
    // background: Control-click toggles the string animation
    let bg = p.ui.interact(area, egui::Id::unique(("sc-string", p.t)), Sense::click());
    let anim: bool = mem_get(p, "anim", false);
    bg.context_menu(|ui| {
        if ui.button(if anim { "Disable String Animation" } else { "Enable String Animation" }).clicked() {
            mem_set_ctx(ui.ctx(), p.t, "anim", !anim);
            ui.close();
        }
    });
    let pt = p.painter().with_clip_rect(area);
    // the pickups as transparent bell curves
    let (pa, pb) = (p.get(s::PICK_A), p.get(s::PICK_B));
    for (pos, col) in [(pa, rgb(0x6aa8e8)), (pb, rgb(0xe8a86a))] {
        let pts: Vec<egui::Pos2> = (0..=40).map(|k| {
            let f = k as f32 / 40.0;
            pos2(xp(f), ym - 34.0 * (-((f - pos) / 0.07).powi(2)).exp())
        }).collect();
        pt.add(Shape::line(pts, Stroke::new(1.2, col.gamma_multiply(0.8))));
        pt.line_segment([pos2(xp(pos), ym - 36.0), pos2(xp(pos), ym)], Stroke::new(1.0, rgb(0xf0a030).gamma_multiply(0.7)));
    }
    // the string: thicker as the stiffness grows (the animation shows it vibrating)
    let thick = 1.0 + 4.0 * p.get(s::STIFFNESS);
    match (&live.shape, anim) {
        (Some(sh), true) => {
            let n = sh.len();
            let pts: Vec<egui::Pos2> = (0..n).map(|k| pos2(xp(k as f32 / (n - 1) as f32), ym - sh[k].clamp(-1.0, 1.0) * 40.0)).collect();
            pt.add(Shape::line(pts, Stroke::new(thick, STRING)));
        }
        _ => {
            pt.line_segment([pos2(x0, ym), pos2(x1, ym)], Stroke::new(thick, STRING));
        }
    }
    // the objects: orange lines (their thickness and brightness show the strength), numbered arrows
    for o in 0..3usize {
        let id = s::obj(o, s::O_POS);
        let on = p.on(s::obj(o, s::O_ON));
        let pos = p.get(id);
        let st = p.get(s::obj(o, s::O_STR));
        let a = if on { 0.3 + 0.7 * st } else { 0.15 };
        pt.line_segment([pos2(xp(pos), area.min.y + 8.0), pos2(xp(pos), area.max.y - 8.0)], Stroke::new(1.0 + 3.0 * st, ORANGE.gamma_multiply(a)));
        let kind = s::kind_of(o, p.geti(s::obj(o, s::O_TYPE)));
        // object 2 has two arrows: it can excite (below) or damp (above)
        let mut spots = vec![(area.max.y - 6.0, true)];
        if o == 1 {
            spots.push((area.min.y + 6.0, false));
        }
        for (k, (y, below)) in spots.into_iter().enumerate() {
            let hr = Rect::from_center_size(pos2(xp(pos), y), vec2(16.0, 14.0));
            let resp = p.ui.interact(hr, egui::Id::unique(("sc-obj", p.t, o, k)), Sense::click_and_drag()).on_hover_text(format!("Object {} position {:.2}", o + 1, pos));
            let tri = if below { [pos2(hr.center().x, hr.min.y), pos2(hr.min.x + 1.0, hr.max.y), pos2(hr.max.x - 1.0, hr.max.y)] } else { [pos2(hr.center().x, hr.max.y), pos2(hr.min.x + 1.0, hr.min.y), pos2(hr.max.x - 1.0, hr.min.y)] };
            let lit = if o == 1 { (kind >= 10) != below } else { true };
            p.painter().add(Shape::convex_polygon(tri.to_vec(), if on && lit { ORANGE } else { Color32::from_gray(0x70) }, Stroke::NONE));
            p.painter().text(hr.center() + vec2(0.0, if below { 2.0 } else { -2.0 }), Align2::CENTER_CENTER, format!("{}", o + 1), look::regular(8.5), Color32::BLACK);
            if resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    p.put(id, ((pp.x - x0) / (x1 - x0)).clamp(0.0, 1.0));
                }
            }
            if resp.double_clicked() || (resp.clicked() && alt(p)) {
                p.put(id, p.prm(id).default);
            }
        }
    }
    // the pickup handles: A at the top, B at the bottom
    for (k, (id, y, col)) in [(s::PICK_A, area.min.y + 18.0, rgb(0x6aa8e8)), (s::PICK_B, area.max.y - 20.0, rgb(0xe8a86a))].into_iter().enumerate() {
        let pos = p.get(id);
        let hr = Rect::from_center_size(pos2(xp(pos), y), vec2(14.0, 10.0));
        let resp = p.ui.interact(hr, egui::Id::unique(("sc-pick", p.t, k)), Sense::click_and_drag()).on_hover_text(format!("Pickup {} position {:.2}", if k == 0 { "A" } else { "B" }, pos));
        p.painter().rect_filled(hr, 3.0, col);
        p.painter().text(hr.center(), Align2::CENTER_CENTER, if k == 0 { "A" } else { "B" }, look::regular(8.5), Color32::BLACK);
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                p.put(id, ((pp.x - x0) / (x1 - x0)).clamp(0.0, 1.0));
            }
        }
        if resp.double_clicked() || (resp.clicked() && alt(p)) {
            p.put(id, p.prm(id).default);
        }
    }
    p.toggle(24.0, 434.0, 50.0, 18.0, s::PICK_INVERT, "Invert");
    // the position sliders (all morphable)
    let mut x = 84.0;
    for (id, label) in [(s::obj(0, s::O_POS), "Obj 1"), (s::obj(1, s::O_POS), "Obj 2"), (s::obj(2, s::O_POS), "Obj 3"), (s::PICK_A, "Pickup A"), (s::PICK_B, "Pickup B")] {
        hs(p, x, 432.0, 54.0, id, label, true);
        x += 58.0;
    }
}

fn mem_set_ctx(ctx: &egui::Context, t: usize, key: &str, v: bool) {
    let id = egui::Id::unique(("sculpt-ui", t, key.to_string()));
    ctx.data_mut(|d| d.insert_temp(id, v));
}

fn waveshaper(p: &mut Pane) {
    p.group(388.0, 40.0, 312.0, 78.0, "WAVESHAPER");
    p.toggle(396.0, 58.0, 34.0, 18.0, s::WS_ON, "On");
    p.menu(396.0, 80.0, 90.0, 18.0, s::WS_TYPE);
    kn(p, 522.0, 52.0, 32.0, s::WS_INPUT, "Input Scale", true);
    kn(p, 580.0, 52.0, 32.0, s::WS_VAR, "Variation", true);
    // the curve
    let r = p.rect(620.0, 56.0, 74.0, 56.0);
    p.painter().rect_filled(r, 4.0, Color32::from_gray(0x12));
    let (ty, inp, var) = (p.geti(s::WS_TYPE), p.get(s::WS_INPUT), p.get(s::WS_VAR));
    let pts: Vec<egui::Pos2> = (0..=48).map(|k| {
        let x = (k as f32 / 48.0) * 2.0 - 1.0;
        let y = fx::waveshape(ty, inp, var, x * 0.5) * 2.0;
        pos2(r.min.x + (x * 0.5 + 0.5) * r.width(), r.center().y - y.clamp(-1.0, 1.0) * r.height() * 0.45)
    }).collect();
    p.painter().with_clip_rect(r).add(Shape::line(pts, Stroke::new(1.4, if p.on(s::WS_ON) { ORANGE } else { Color32::from_gray(0x70) })));
}

/// The circular Material Pad: Stiffness across, Inner Loss up; Resolution, Media Loss and
/// Tension Mod around it; Keyscale and Release views add handles for the low and high notes.
fn material_pad(p: &mut Pane, live: &Live) {
    p.group(388.0, 122.0, 312.0, 246.0, "STRING");
    let pad = p.rect(452.0, 148.0, 192.0, 192.0);
    let c = pad.center();
    let rad = pad.width() * 0.5;
    let pt = p.painter();
    pt.circle_filled(c, rad + 6.0, Color32::from_gray(0x1c));
    // the four material corners shade the matrix
    for gy in 0..16 {
        for gx in 0..16 {
            let (fx_, fy) = ((gx as f32 + 0.5) / 16.0, (gy as f32 + 0.5) / 16.0);
            let cell = Rect::from_min_size(pad.min + vec2(fx_ * pad.width() - 6.0, (1.0 - fy) * pad.height() - 6.0), vec2(12.0, 12.0));
            let d = ((fx_ - 0.5) * (fx_ - 0.5) + (fy - 0.5) * (fy - 0.5)).sqrt();
            if d < 0.5 {
                // Logic's pad: dark slate, lighter towards the middle
                let col = look::mix(rgb(0x3b4b5a), rgb(0x18222c), (d * 2.0).powf(1.4));
                pt.rect_filled(cell, 0.0, col);
            }
        }
    }
    pt.circle_stroke(c, rad, Stroke::new(1.5, Color32::from_gray(0x60)));
    for (txt, fx_, fy) in [("Steel", 0.2, 0.2), ("Glass", 0.8, 0.2), ("Nylon", 0.2, 0.8), ("Wood", 0.8, 0.8)] {
        pt.text(pad.min + vec2(fx_ * pad.width(), (1.0 - fy) * pad.height()), Align2::CENTER_CENTER, txt, look::regular(10.0), Color32::from_gray(0xa8));
    }
    // the pad's name and its axes, as Logic labels them
    pt.text(c, Align2::CENTER_CENTER, "MATERIAL", look::regular(17.0), Color32::from_white_alpha(70));
    pt.text(pos2(c.x, pad.max.y - 8.0), Align2::CENTER_CENTER, "STIFFNESS", look::regular(8.5), Color32::from_gray(0x90));
    let gal = pt.layout_no_wrap("INNER LOSS".into(), look::regular(8.5), Color32::from_gray(0x90));
    let gw = gal.size();
    pt.add(egui::epaint::TextShape::new(pos2(pad.min.x + 4.0, c.y + gw.x / 2.0), gal, Color32::from_gray(0x90)).with_angle(-std::f32::consts::FRAC_PI_2));
    let view = p.geti(s::KS_VIEW);
    let (st, il) = (p.get(s::STIFFNESS), p.get(s::INNER_LOSS));
    let at = |sx: f32, sy: f32| pos2(pad.min.x + sx.clamp(0.0, 1.0) * pad.width(), pad.max.y - sy.clamp(0.0, 1.0) * pad.height());
    // the ghost ball: where the morph is now
    if let Some((mx, my)) = live.morph_now {
        let pts: [[f32; s::MORPH_N]; 5] = std::array::from_fn(|i| std::array::from_fn(|k| p.get(s::ms(i, k))));
        let v = s::morph_values(&pts, mx, my);
        pt.circle_stroke(at(v[15], v[14]), 6.0, Stroke::new(1.5, RED.gamma_multiply(0.8)));
    }
    // dragging the pad (below the handles so these stay grabbable)
    let ball0 = at(st, il);
    let pad_resp = p.ui.interact(pad, egui::Id::unique(("sc-pad", p.t)), Sense::click_and_drag());
    let near = pad_resp.interact_pointer_pos().map(|q| (q - ball0).length() < 14.0).unwrap_or(false);
    if (pad_resp.dragged() || pad_resp.clicked()) && (view == 0 || near) {
        if let Some(pp) = pad_resp.interact_pointer_pos() {
            p.put(s::STIFFNESS, ((pp.x - pad.min.x) / pad.width()).clamp(0.0, 1.0));
            p.put(s::INNER_LOSS, ((pad.max.y - pp.y) / pad.height()).clamp(0.0, 1.0));
        }
    }
    // the Keyscale / Release handles (a diamond with a crosshair)
    let mut handles: Vec<(u32, u32, Color32, f32)> = Vec::new();
    if view == 1 {
        handles.push((s::ST_KS_LO, s::IL_KS_LO, GREEN, 0.0));
        handles.push((s::ST_KS_HI, s::IL_KS_HI, BLUE, 0.0));
    } else if view == 2 {
        handles.push((u32::MAX, s::IL_REL, BLUE, 0.0));
    }
    for (hi, (idx, idy, col, _)) in handles.into_iter().enumerate() {
        let (kx, ky) = (if idx == u32::MAX { 0.0 } else { p.get(idx) }, if idy == s::IL_REL { p.get(idy) - 1.0 } else { p.get(idy) });
        let pos = at(st + kx * 0.25, il + ky * 0.25);
        let pr_x = if idx == u32::MAX { None } else { Some(p.prm(idx)) };
        let pr_y = p.prm(idy);
        // the crosshair lines: each moves one axis only
        let hline = Rect::from_center_size(pos2(pad.center().x, pos.y), vec2(pad.width(), 8.0));
        let vline = Rect::from_center_size(pos2(pos.x, pad.center().y), vec2(8.0, pad.height()));
        p.painter().with_clip_rect(pad.expand(2.0)).line_segment([pos2(pad.min.x, pos.y), pos2(pad.max.x, pos.y)], Stroke::new(1.0, col.gamma_multiply(0.9)));
        if idx != u32::MAX {
            p.painter().with_clip_rect(pad.expand(2.0)).line_segment([pos2(pos.x, pad.min.y), pos2(pos.x, pad.max.y)], Stroke::new(1.0, col.gamma_multiply(0.9)));
        }
        let rh = p.ui.interact(hline, egui::Id::unique(("sc-ksh", p.t, hi)), Sense::click_and_drag());
        if rh.dragged() {
            if let Some(pp) = rh.interact_pointer_pos() {
                let d = ((pad.max.y - pp.y) / pad.height() - il) / 0.25 + if idy == s::IL_REL { 1.0 } else { 0.0 };
                p.put(idy, d.clamp(pr_y.min, pr_y.max));
            }
        }
        if rh.double_clicked() || (rh.clicked() && alt(p)) {
            p.put(idy, pr_y.default);
        }
        if let Some(px) = &pr_x {
            let rv = p.ui.interact(vline, egui::Id::unique(("sc-ksv", p.t, hi)), Sense::click_and_drag());
            if rv.dragged() {
                if let Some(pp) = rv.interact_pointer_pos() {
                    p.put(idx, (((pp.x - pad.min.x) / pad.width() - st) / 0.25).clamp(px.min, px.max));
                }
            }
            if rv.double_clicked() || (rv.clicked() && alt(p)) {
                p.put(idx, px.default);
            }
        }
        // the diamond moves both (vertical only in the Release view)
        let dr = Rect::from_center_size(pos, vec2(14.0, 14.0));
        let rd = p.ui.interact(dr, egui::Id::unique(("sc-ksd", p.t, hi)), Sense::click_and_drag()).on_hover_text("Option-click puts it under the ball");
        let dia = vec![pos + vec2(0.0, -6.0), pos + vec2(6.0, 0.0), pos + vec2(0.0, 6.0), pos + vec2(-6.0, 0.0)];
        p.painter().add(Shape::convex_polygon(dia, col, Stroke::new(1.0, Color32::BLACK)));
        if rd.dragged() {
            if let Some(pp) = rd.interact_pointer_pos() {
                p.put(idy, (((pad.max.y - pp.y) / pad.height() - il) / 0.25 + if idy == s::IL_REL { 1.0 } else { 0.0 }).clamp(pr_y.min, pr_y.max));
                if let Some(px) = &pr_x {
                    p.put(idx, (((pp.x - pad.min.x) / pad.width() - st) / 0.25).clamp(px.min, px.max));
                }
            }
        }
        if rd.double_clicked() || (rd.clicked() && alt(p)) {
            p.put(idy, if idy == s::IL_REL { 1.0 } else { 0.0 });
            if let Some(px) = &pr_x {
                p.put(idx, px.default);
            }
        }
    }
    // the ball
    let ball = at(st, il);
    p.painter().circle_filled(ball, 7.0, Color32::from_gray(0xe8));
    p.painter().circle_stroke(ball, 7.0, Stroke::new(1.5, ORANGE));
    // Resolution above, Media Loss to the left, Tension Mod to the right
    let ring = |p: &mut Pane, r: Rect, id: u32, vertical: bool, col: Color32| slider(p, r, id, vertical, col);
    p.text(448.0, 143.0, Align2::RIGHT_CENTER, "Resolution", 9.0, ORANGE);
    ring(p, p.rect(452.0, 138.0, 192.0, 12.0), s::RESOLUTION, false, ORANGE);
    ring(p, p.rect(398.0, 148.0, 12.0, 192.0), s::MEDIA_LOSS, true, ORANGE);
    ring(p, p.rect(662.0, 148.0, 12.0, 192.0), s::TENS_MOD, true, ORANGE);
    p.text(412.0, 346.0, Align2::CENTER_CENTER, "Media Loss", 9.0, ORANGE);
    p.text(662.0, 346.0, Align2::CENTER_CENTER, "Tension Mod", 9.0, ORANGE);
    if view == 1 {
        // low (green) and high (blue) key scaling of Resolution, Media Loss and Tension Mod
        ring(p, p.rect(452.0, 126.0, 96.0, 10.0), s::RES_KS_LO, false, GREEN);
        ring(p, p.rect(548.0, 126.0, 96.0, 10.0), s::RES_KS_HI, false, BLUE);
        ring(p, p.rect(412.0, 148.0, 10.0, 96.0), s::ML_KS_LO, true, GREEN);
        ring(p, p.rect(412.0, 244.0, 10.0, 96.0), s::ML_KS_HI, true, BLUE);
        ring(p, p.rect(676.0, 148.0, 10.0, 96.0), s::TM_KS_LO, true, GREEN);
        ring(p, p.rect(676.0, 244.0, 10.0, 96.0), s::TM_KS_HI, true, BLUE);
    } else if view == 2 {
        ring(p, p.rect(412.0, 148.0, 10.0, 192.0), s::ML_REL, true, BLUE);
    }
    p.seg(452.0, 348.0, 192.0, 16.0, s::KS_VIEW, &["Hide", "Keyscale", "Release"], false);
}

fn filter(p: &mut Pane) {
    p.group(388.0, 372.0, 312.0, 92.0, "FILTER");
    p.toggle(396.0, 388.0, 30.0, 16.0, s::F_ON, "On");
    p.seg(432.0, 388.0, 260.0, 16.0, s::F_TYPE, &["HiPass", "LoPass", "Peak", "BandPass", "Notch"], false);
    kn(p, 424.0, 408.0, 30.0, s::CUTOFF, "Cutoff", true);
    kn(p, 490.0, 408.0, 30.0, s::RESONANCE, "Resonance", true);
    kn(p, 556.0, 408.0, 30.0, s::F_KEY, "Key", false);
    kn(p, 622.0, 408.0, 30.0, s::F_VELO, "Velo Sens", false);
}

fn delay(p: &mut Pane) {
    p.group(708.0, 40.0, 404.0, 160.0, "DELAY");
    p.toggle(716.0, 56.0, 30.0, 16.0, s::D_ON, "On");
    p.toggle(750.0, 56.0, 36.0, 16.0, s::D_SYNC, "Sync");
    // Time: note values when synced, milliseconds when free
    if p.on(s::D_SYNC) {
        note_menu(p, 792.0, 56.0, 70.0, s::D_NOTE);
    } else {
        p.field(792.0, 56.0, 70.0, 16.0, s::D_TIME, 1.0);
    }
    let tr = p.rect(868.0, 58.0, 120.0, 14.0);
    slider(p, tr, if p.on(s::D_SYNC) { s::D_NOTE } else { s::D_TIME }, false, p.skin.accent);
    kn(p, 744.0, 80.0, 34.0, s::D_FB, "Feedback", false);
    kn(p, 806.0, 80.0, 34.0, s::D_XFEED, "Xfeed", false);
    kn(p, 868.0, 80.0, 34.0, s::D_WET, "Wet Level", false);
    hs(p, 716.0, 140.0, 130.0, s::D_LOCUT, "Lo Cut", false);
    hs(p, 716.0, 166.0, 130.0, s::D_HICUT, "Hi Cut", false);
    hs(p, 856.0, 140.0, 120.0, s::D_INBAL, "Input Balance", false);
    hs(p, 856.0, 166.0, 120.0, s::D_WIDTH, "Output Width", false);
    groove_pad(p);
}

/// The Groove Pad: Groove across, Spread up; the diamond moves both, its lines one each.
fn groove_pad(p: &mut Pane) {
    let r = p.rect(1000.0, 62.0, 104.0, 104.0);
    clip_menu(p, r, 100, &[s::D_ON, s::D_TIME, s::D_NOTE, s::D_SYNC, s::D_FB, s::D_XFEED, s::D_WET, s::D_LOCUT, s::D_HICUT, s::D_INBAL, s::D_WIDTH, s::D_SPREAD, s::D_GROOVE], &[(s::D_ON, 0.0), (s::D_TIME, 300.0), (s::D_FB, 0.3), (s::D_XFEED, 0.0), (s::D_WET, 0.3), (s::D_SPREAD, 0.0), (s::D_GROOVE, 0.0), (s::D_INBAL, 0.0), (s::D_WIDTH, 1.0)]);
    p.painter().rect_filled(r, 5.0, Color32::from_gray(0x12));
    let (gr, sp) = (p.get(s::D_GROOVE), p.get(s::D_SPREAD));
    let c = pos2(r.center().x + gr * r.width() * 0.5, r.center().y - sp * r.height() * 0.5);
    let pt = p.painter().with_clip_rect(r);
    pt.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.0, Color32::from_gray(0x30)));
    pt.line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, Color32::from_gray(0x30)));
    pt.line_segment([pos2(r.min.x, c.y), pos2(r.max.x, c.y)], Stroke::new(1.0, BLUE));
    pt.line_segment([pos2(c.x, r.min.y), pos2(c.x, r.max.y)], Stroke::new(1.0, BLUE));
    p.painter().text(r.min + vec2(4.0, 4.0), Align2::LEFT_TOP, "Spread", look::regular(8.5), p.skin.dim);
    p.painter().text(r.max - vec2(4.0, 4.0), Align2::RIGHT_BOTTOM, "Groove", look::regular(8.5), p.skin.dim);
    let set = |p: &mut Pane, pp: egui::Pos2, gx: bool, sy: bool| {
        if gx {
            p.put(s::D_GROOVE, ((pp.x - r.center().x) / (r.width() * 0.5)).clamp(-1.0, 1.0));
        }
        if sy {
            p.put(s::D_SPREAD, (-(pp.y - r.center().y) / (r.height() * 0.5)).clamp(-1.0, 1.0));
        }
    };
    let hl = p.ui.interact(Rect::from_center_size(pos2(r.center().x, c.y), vec2(r.width(), 8.0)), egui::Id::unique(("sc-gh", p.t)), Sense::click_and_drag());
    if hl.dragged() {
        if let Some(pp) = hl.interact_pointer_pos() {
            set(p, pp, false, true);
        }
    }
    let vl = p.ui.interact(Rect::from_center_size(pos2(c.x, r.center().y), vec2(8.0, r.height())), egui::Id::unique(("sc-gv", p.t)), Sense::click_and_drag());
    if vl.dragged() {
        if let Some(pp) = vl.interact_pointer_pos() {
            set(p, pp, true, false);
        }
    }
    let dr = Rect::from_center_size(c, vec2(14.0, 14.0));
    let dd = p.ui.interact(dr, egui::Id::unique(("sc-gd", p.t)), Sense::click_and_drag()).on_hover_text(format!("Groove {:+.0}%  Spread {:+.0}%", gr * 100.0, sp * 100.0));
    let dia = vec![c + vec2(0.0, -6.0), c + vec2(6.0, 0.0), c + vec2(0.0, 6.0), c + vec2(-6.0, 0.0)];
    p.painter().add(Shape::convex_polygon(dia, rgb(0x7fd0ff), Stroke::new(1.0, Color32::BLACK)));
    if dd.dragged() {
        if let Some(pp) = dd.interact_pointer_pos() {
            set(p, pp, true, true);
        }
    }
    if dd.double_clicked() || (dd.clicked() && alt(p)) {
        p.put(s::D_GROOVE, 0.0);
        p.put(s::D_SPREAD, 0.0);
    }
}

fn amp_and_level(p: &mut Pane) {
    p.group(708.0, 204.0, 404.0, 116.0, "AMPLIFIER");
    // Attack: Soft and Hard; the bar between them moves both
    let sr = p.rect(716.0, 224.0, 14.0, 70.0);
    let hr = p.rect(736.0, 224.0, 14.0, 70.0);
    slider(p, sr, s::A_SOFT, true, p.skin.accent);
    slider(p, hr, s::A_HARD, true, p.skin.accent);
    let bar = p.rect(729.0, 240.0, 8.0, 40.0);
    let resp = p.ui.interact(bar, egui::Id::unique(("sc-att", p.t)), Sense::click_and_drag()).on_hover_text("Drag to move both Attack sliders");
    p.painter().rect_filled(Rect::from_center_size(bar.center(), vec2(8.0, 20.0)), 2.0, Color32::from_gray(0x80));
    if resp.dragged() {
        let d = -resp.drag_delta().y / 70.0;
        let (a, b) = (p.get(s::A_SOFT), p.get(s::A_HARD));
        let pr = p.prm(s::A_SOFT);
        p.put(s::A_SOFT, (a + d * pr.max).clamp(pr.min, pr.max));
        p.put(s::A_HARD, (b + d * pr.max).clamp(pr.min, pr.max));
    }
    p.text(733.0, 306.0, Align2::CENTER_CENTER, "A  Soft | Hard", 9.5, p.skin.dim);
    for (k, (id, l)) in [(s::A_DECAY, "D"), (s::A_SUSTAIN, "S"), (s::A_RELEASE, "R")].into_iter().enumerate() {
        let r = p.rect(768.0 + k as f32 * 28.0, 224.0, 14.0, 70.0);
        slider(p, r, id, true, p.skin.accent);
        p.text(775.0 + k as f32 * 28.0, 306.0, Align2::CENTER_CENTER, l, 10.0, p.skin.dim);
    }
    kn(p, 892.0, 222.0, 40.0, s::LEVEL, "Level", false);
    p.text(950.0, 222.0, Align2::LEFT_CENTER, "Limiter", 10.0, p.skin.dim);
    p.seg(950.0, 230.0, 156.0, 16.0, s::LIMITER, &["Off", "Mono", "Poly", "Both"], false);
    hs(p, 950.0, 254.0, 156.0, s::SPREAD_PICK, "Spread Pickup", false);
    hs(p, 950.0, 282.0, 156.0, s::SPREAD_KEY, "Spread Key", false);
}

fn body_eq(p: &mut Pane) {
    p.group(708.0, 324.0, 404.0, 140.0, "BODY EQ");
    p.toggle(716.0, 340.0, 30.0, 16.0, s::B_ON, "On");
    p.menu(750.0, 340.0, 110.0, 16.0, s::B_MODEL);
    let model = p.geti(s::B_MODEL);
    // changing between the Basic EQ and a model renames the knobs: give them sensible starts
    let last: usize = mem_get(p, "bmodel", model);
    if (last == 0) != (model == 0) {
        let d = if model == 0 { [0.0, 0.0, 0.0, 0.5] } else { [1.0, 0.0, 0.0, 0.0] };
        for (id, v) in [s::B_P1, s::B_P2, s::B_P3, s::B_P4].into_iter().zip(d) {
            p.put(id, v);
        }
    }
    mem_set(p, "bmodel", model);
    let (p1, p2, p3, p4) = (p.get(s::B_P1), p.get(s::B_P2), p.get(s::B_P3), p.get(s::B_P4));
    // the graphic: the response on a log axis; dragging edits the model's or the basic EQ's values
    let g = p.rect(716.0, 362.0, 236.0, 94.0);
    p.painter().rect_filled(g, 4.0, Color32::from_gray(0x12));
    let pts: Vec<egui::Pos2> = (0..=80).map(|k| {
        let hz = 30.0 * (20000.0f32 / 30.0).powf(k as f32 / 80.0);
        let db = fx::body_response_db(model, p1, p2, p3, p4, hz).clamp(-24.0, 24.0);
        pos2(g.min.x + g.width() * k as f32 / 80.0, g.center().y - db / 24.0 * g.height() * 0.45)
    }).collect();
    p.painter().line_segment([g.left_center(), g.right_center()], Stroke::new(1.0, Color32::from_gray(0x30)));
    p.painter().with_clip_rect(g).add(Shape::line(pts, Stroke::new(1.6, if p.on(s::B_ON) { ORANGE } else { Color32::from_gray(0x70) })));
    let resp = p.ui.interact(g, egui::Id::unique(("sc-beq", p.t)), Sense::click_and_drag());
    if resp.dragged() {
        if let Some(pp) = resp.interact_pointer_pos() {
            let fx_ = ((pp.x - g.min.x) / g.width()).clamp(0.0, 1.0);
            let fy = (-(pp.y - g.center().y) / (g.height() * 0.45)).clamp(-1.0, 1.0);
            if model == 0 {
                // left third sets Low, the centre Mid (and its frequency), the right High
                if fx_ < 0.33 {
                    p.put(s::B_P1, fy);
                } else if fx_ < 0.66 {
                    p.put(s::B_P2, fy);
                    p.put(s::B_P4, ((fx_ - 0.33) / 0.33).clamp(0.0, 1.0));
                } else {
                    p.put(s::B_P3, fy);
                }
            } else {
                p.put(s::B_P1, fy);
                p.put(s::B_P2, (fx_ * 2.0 - 1.0) * 0.5);
            }
        }
    }
    let (l1, l2, l3, l4) = if model == 0 { ("Low", "Mid", "High", "Mid Frequency") } else { ("Intensity", "Shift", "Stretch", "Fine Structure") };
    kn(p, 986.0, 342.0, 30.0, s::B_P1, l1, false);
    kn(p, 1038.0, 342.0, 30.0, s::B_P2, l2, false);
    kn(p, 1090.0, 342.0, 30.0, s::B_P3, l3, false);
    hs(p, 962.0, 424.0, 142.0, s::B_P4, l4, false);
    if model != 0 {
        cap(p, 962.0, 446.0, "(high Fine Structure uses more CPU)", false);
    }
}

// ---------------------------------------------------------------------------------------------
// the modulation area: LFOs, vibrato, jitter, note-on random, velocity

fn modulation(p: &mut Pane) {
    p.group(8.0, 468.0, 420.0, 238.0, "");
    let sel = tabs(p, 12.0, 472.0, 56.0, "mod", &["LFO 1", "LFO 2", "Vibrato", "Jitter", "Random", "Velocity", "Ctrl A/B"]);
    // active sources have lit buttons' labels: drawn dotted under the tab
    let active = [p.on(s::lfo(0, s::L_ON)), p.on(s::lfo(1, s::L_ON)), p.get(s::VIB_AMT) > 0.0, p.on(s::jit(0, s::J_ON)) || p.on(s::jit(1, s::J_ON)), p.on(s::nr(0, s::N_ON)) || p.on(s::nr(1, s::N_ON)), p.on(s::vmod(0, s::V_ON)) || p.on(s::vmod(1, s::V_ON)), p.on(s::cab(0, s::C_ON)) || p.on(s::cab(1, s::C_ON))];
    for (k, a) in active.iter().enumerate() {
        if *a {
            p.painter().circle_filled(p.rect(12.0 + k as f32 * 59.0 + 28.0, 493.0, 0.0, 0.0).min, 2.0, ORANGE);
        }
    }
    match sel {
        0 | 1 => lfo_pane(p, sel),
        2 => vibrato_pane(p),
        3 => jitter_pane(p),
        4 => random_pane(p),
        5 => velocity_pane(p),
        _ => ctrl_pane(p),
    }
}

/// The LFO's wave as drawn: two cycles of the wave with the curve applied.
fn wave_display(p: &mut Pane, r: Rect, wave: usize, curve: f32) {
    p.painter().rect_filled(r, 4.0, Color32::from_gray(0x12));
    let mut st = fx::LfoState::new(3);
    let n = 120;
    let mut pts = Vec::new();
    for k in 0..n {
        let v = st.step(wave, curve, 2.0 / n as f32, 0.0);
        let uni = wave == 4;
        let y = if uni { v * 2.0 - 1.0 } else { v };
        pts.push(pos2(r.min.x + r.width() * k as f32 / (n - 1) as f32, r.center().y - y.clamp(-1.0, 1.0) * r.height() * 0.42));
    }
    p.painter().with_clip_rect(r).add(Shape::line(pts, Stroke::new(1.4, GREEN)));
}

fn route_row(p: &mut Pane, y: f32, ids: [u32; 5], n: usize) {
    p.toggle(16.0, y, 22.0, 16.0, ids[0], &format!("{n}"));
    p.menu(42.0, y, 126.0, 16.0, ids[1]);
    p.menu(172.0, y, 104.0, 16.0, ids[2]);
    let via = p.geti(ids[2]) > 0;
    let w = if via { 66.0 } else { 140.0 };
    cap(p, 282.0, y - 2.0, "Amt", false);
    let r = p.rect(282.0, y + 3.0, w, 14.0);
    slider(p, r, ids[3], false, p.skin.accent);
    if via {
        cap(p, 352.0, y - 2.0, "Via amt", false);
        let r = p.rect(352.0, y + 3.0, 66.0, 14.0);
        slider(p, r, ids[4], false, ORANGE);
    }
}

fn lfo_pane(p: &mut Pane, i: usize) {
    p.toggle(16.0, 498.0, 34.0, 16.0, s::lfo(i, s::L_ON), "On");
    p.menu(54.0, 498.0, 108.0, 16.0, s::lfo(i, s::L_WAVE));
    p.seg(168.0, 498.0, 80.0, 16.0, s::lfo(i, s::L_SYNC), &["Free", "Sync"], false);
    if p.on(s::lfo(i, s::L_SYNC)) {
        note_menu(p, 254.0, 498.0, 70.0, s::lfo(i, s::L_NOTE));
    } else {
        p.field(254.0, 498.0, 70.0, 16.0, s::lfo(i, s::L_RATE), 0.05);
    }
    let r = p.rect(16.0, 522.0, 150.0, 50.0);
    wave_display(p, r, p.geti(s::lfo(i, s::L_WAVE)), p.get(s::lfo(i, s::L_CURVE)));
    kn(p, 196.0, 520.0, 30.0, s::lfo(i, s::L_CURVE), "Curve", false);
    kn(p, 244.0, 520.0, 30.0, s::lfo(i, s::L_ENV), "Envelope", false);
    kn(p, 292.0, 520.0, 30.0, s::lfo(i, s::L_PHASE), "Phase", false);
    p.menu(334.0, 522.0, 88.0, 16.0, s::lfo(i, s::L_RMSRC));
    hs(p, 334.0, 544.0, 88.0, s::lfo(i, s::L_RMAMT), "RateMod", false);
    for rt in 0..2usize {
        route_row(p, 590.0 + rt as f32 * 40.0, [s::lfo_r(i, rt, s::R_ON), s::lfo_r(i, rt, s::R_TARGET), s::lfo_r(i, rt, s::R_VIA), s::lfo_r(i, rt, s::R_AMT), s::lfo_r(i, rt, s::R_VIAAMT)], rt + 1);
    }
}

fn vibrato_pane(p: &mut Pane) {
    cap(p, 16.0, 506.0, "Vibrato LFO (hard-wired to pitch)", false);
    p.menu(16.0, 520.0, 120.0, 16.0, s::VIB_WAVE);
    p.seg(142.0, 520.0, 80.0, 16.0, s::VIB_SYNC, &["Free", "Sync"], false);
    if p.on(s::VIB_SYNC) {
        note_menu(p, 228.0, 520.0, 70.0, s::VIB_NOTE);
    } else {
        p.field(228.0, 520.0, 70.0, 16.0, s::VIB_RATE, 0.1);
    }
    let r = p.rect(16.0, 544.0, 150.0, 50.0);
    wave_display(p, r, p.geti(s::VIB_WAVE), p.get(s::VIB_CURVE));
    kn(p, 196.0, 542.0, 30.0, s::VIB_CURVE, "Curve", false);
    kn(p, 250.0, 542.0, 30.0, s::VIB_PHASE, "Phase", false);
    hs(p, 16.0, 612.0, 190.0, s::VIB_AMT, "Amt (the most the vibrato gets)", false);
    hs(p, 222.0, 612.0, 190.0, s::VIB_VIA, "Via (from the Vib Depth Ctrl controller)", false);
}

fn jitter_pane(p: &mut Pane) {
    for g in 0..2usize {
        let y = 504.0 + g as f32 * 100.0;
        p.toggle(16.0, y, 34.0, 16.0, s::jit(g, s::J_ON), &format!("J{}", g + 1));
        kn(p, 76.0, y - 4.0, 34.0, s::jit(g, s::J_RATE), "Rate", false);
        p.menu(110.0, y, 150.0, 16.0, s::jit(g, s::J_T1));
        let r = p.rect(266.0, y + 2.0, 150.0, 14.0);
        slider(p, r, s::jit(g, s::J_A1), false, p.skin.accent);
        p.menu(110.0, y + 28.0, 150.0, 16.0, s::jit(g, s::J_T2));
        let r = p.rect(266.0, y + 30.0, 150.0, 14.0);
        slider(p, r, s::jit(g, s::J_A2), false, p.skin.accent);
        cap(p, 110.0, y + 54.0, "Target 1 / 2 and their amounts; the jitter is an LFO with a noise wave", false);
    }
}

fn random_pane(p: &mut Pane) {
    cap(p, 16.0, 506.0, "Note-on random: a new value for each note, kept until its release", false);
    for k in 0..2usize {
        let y = 530.0 + k as f32 * 40.0;
        p.toggle(16.0, y, 34.0, 16.0, s::nr(k, s::N_ON), &format!("{}", k + 1));
        p.menu(56.0, y, 170.0, 16.0, s::nr(k, s::N_TARGET));
        cap(p, 236.0, y - 2.0, "Amount", false);
        let r = p.rect(236.0, y + 3.0, 180.0, 14.0);
        slider(p, r, s::nr(k, s::N_AMT), false, p.skin.accent);
    }
}

fn ctrl_pane(p: &mut Pane) {
    cap(p, 16.0, 506.0, "Controller A and B: two targets (Continue follows the controller, Note On keeps its value from the note start)", false);
    for k in 0..2usize {
        let y = 524.0 + k as f32 * 60.0;
        p.toggle(16.0, y, 34.0, 16.0, s::cab(k, s::C_ON), &format!("{}", k + 1));
        p.menu(56.0, y, 150.0, 16.0, s::cab(k, s::C_TARGET));
        p.menu(212.0, y, 70.0, 16.0, s::cab(k, s::C_SRC));
        p.seg(288.0, y, 128.0, 16.0, s::cab(k, s::C_MODE), &["Continue", "Note On"], false);
        cap(p, 56.0, y + 20.0, "Amount", false);
        let r = p.rect(56.0, y + 26.0, 360.0, 14.0);
        slider(p, r, s::cab(k, s::C_AMT), false, p.skin.accent);
    }
}

fn velocity_pane(p: &mut Pane) {
    cap(p, 16.0, 506.0, "Velocity modulation (the objects and the filter have their own sensitivity)", false);
    for k in 0..2usize {
        let y = 530.0 + k as f32 * 52.0;
        p.toggle(16.0, y, 34.0, 16.0, s::vmod(k, s::V_ON), &format!("{}", k + 1));
        p.menu(56.0, y, 170.0, 16.0, s::vmod(k, s::V_TARGET));
        cap(p, 236.0, y - 2.0, "Amount", false);
        let r = p.rect(236.0, y + 3.0, 180.0, 14.0);
        slider(p, r, s::vmod(k, s::V_AMT), false, p.skin.accent);
        p.seg(56.0, y + 22.0, 170.0, 16.0, s::vmod(k, s::V_CURVE), &["Concave", "Linear", "Convex"], false);
    }
}

// ---------------------------------------------------------------------------------------------
// the Morph Pad

fn morph_pad(p: &mut Pane, live: &Live) {
    p.group(436.0, 468.0, 264.0, 238.0, "MORPH");
    let pad = p.rect(446.0, 490.0, 176.0, 176.0);
    p.painter().rect_filled(pad, 6.0, Color32::from_gray(0x16));
    let pos = |px: f32, py: f32| pos2(pad.min.x + 14.0 + px * (pad.width() - 28.0), pad.max.y - 14.0 - py * (pad.height() - 28.0));
    let sel = p.geti(s::MORPH_SEL).min(4);
    let resp = p.ui.interact(pad, egui::Id::unique(("sc-morph", p.t)), Sense::click_and_drag());
    let (bx, by) = (p.get(s::MORPH_X), p.get(s::MORPH_Y));
    let mut act = 0;
    resp.context_menu(|ui| {
        for (k, l) in ["Copy selected Point", "Copy current Pad Position", "Paste to selected Point", "Exchange selected Point", "Paste to all Points"].iter().enumerate() {
            if ui.button(*l).clicked() {
                act = k + 1;
                ui.close();
            }
        }
    });
    let key = egui::Id::unique(("sculpt-morph-clip", p.t));
    let point: Vec<f32> = (0..s::MORPH_N).map(|k| p.get(s::ms(sel, k))).collect();
    let pts: [[f32; s::MORPH_N]; 5] = std::array::from_fn(|i| std::array::from_fn(|k| p.get(s::ms(i, k))));
    match act {
        1 => {
            p.ui.ctx().data_mut(|d| d.insert_temp(key, point.clone()));
        }
        2 => {
            let v = s::morph_values(&pts, bx, by).to_vec();
            p.ui.ctx().data_mut(|d| d.insert_temp(key, v));
        }
        3 | 4 | 5 => {
            if let Some(v) = p.ui.ctx().data(|d| d.get_temp::<Vec<f32>>(key)).filter(|v| v.len() == s::MORPH_N) {
                match act {
                    3 => (0..s::MORPH_N).for_each(|k| p.put(s::ms(sel, k), v[k])),
                    4 => {
                        // swap: the point takes the copy, the copy becomes what the point was
                        (0..s::MORPH_N).for_each(|k| p.put(s::ms(sel, k), v[k]));
                        p.ui.ctx().data_mut(|d| d.insert_temp(key, point.clone()));
                    }
                    _ => {
                        for pt_i in 0..5 {
                            (0..s::MORPH_N).for_each(|k| p.put(s::ms(pt_i, k), v[k]));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    // drag the ball: with Auto Select the nearest point follows
    let mut moved = None;
    if resp.dragged() || resp.clicked() {
        if let Some(pp) = resp.interact_pointer_pos() {
            // a click inside the circles round a point selects that point (and moves the ball to it)
            let hit = (0..5usize).find(|&k| {
                let (qx, qy) = s::point_pos(k);
                (pos(qx, qy) - pp).length() < 14.0
            });
            let (nx, ny) = match (hit, resp.clicked()) {
                (Some(k), true) => s::point_pos(k),
                _ => (((pp.x - pad.min.x - 14.0) / (pad.width() - 28.0)).clamp(0.0, 1.0), ((pad.max.y - 14.0 - pp.y) / (pad.height() - 28.0)).clamp(0.0, 1.0)),
            };
            p.put(s::MORPH_X, nx);
            p.put(s::MORPH_Y, ny);
            moved = Some((nx, ny));
            if let (Some(k), true) = (hit, resp.clicked()) {
                p.put(s::MORPH_SEL, k as f32);
            } else if p.on(s::MORPH_AUTO) {
                let near = s::nearest_point(nx, ny);
                if near != sel {
                    p.put(s::MORPH_SEL, near as f32);
                }
            }
        }
    }
    // in Point Set / Point Solo the ball edits the selected envelope node
    if let Some((nx, ny)) = moved {
        if p.geti(s::MENV_MODE) == 3 && p.geti(s::MENV_SUB) >= 1 {
            let n = p.geti(s::MENV_NSEL).min(8);
            p.put(s::mn(n, 1), nx);
            p.put(s::mn(n, 2), ny);
        }
    }
    // the five points: A B C D Center, the selected one with two circles
    for k in 0..5usize {
        let (qx, qy) = s::point_pos(k);
        let c = pos(qx, qy);
        p.painter().circle_stroke(c, 12.0, Stroke::new(1.0, Color32::from_gray(0x50)));
        if k == sel {
            p.painter().circle_stroke(c, 12.0, Stroke::new(2.0, ORANGE));
            p.painter().circle_stroke(c, 8.0, Stroke::new(1.5, ORANGE));
        }
        p.painter().text(c, Align2::CENTER_CENTER, s::MORPH_POINT_NAMES[k].chars().next().unwrap_or('?').to_string(), look::regular(10.0), Color32::from_gray(0xc0));
    }
    // the recorded path of the envelope, the ball, the dot of the sounding voice
    if p.geti(s::MENV_MODE) >= 2 {
        let path: Vec<egui::Pos2> = (0..9).map(|i| pos(p.get(s::mn(i, 1)), p.get(s::mn(i, 2)))).collect();
        p.painter().add(Shape::line(path, Stroke::new(1.0, BLUE.gamma_multiply(0.7))));
    }
    p.painter().circle_filled(pos(bx, by), 7.0, Color32::from_gray(0xd0));
    p.painter().circle_stroke(pos(bx, by), 7.0, Stroke::new(1.5, RED));
    if let Some((mx, my)) = live.morph_now {
        p.painter().circle_filled(pos(mx, my), 3.5, RED);
    }
    // Point buttons, Auto Select, the intensity slider and Rnd
    p.text(632.0, 488.0, Align2::LEFT_CENTER, "Points", 10.0, p.skin.dim);
    for (k, l) in ["•", "••••", "•••••"].iter().enumerate() {
        let r = p.rect(632.0, 496.0 + k as f32 * 20.0, 62.0, 18.0);
        let lit = p.geti(s::MORPH_POINTS) == k;
        if p.button(r, l, lit, ("mp", k)) {
            p.put(s::MORPH_POINTS, k as f32);
        }
    }
    p.toggle(632.0, 560.0, 62.0, 18.0, s::MORPH_AUTO, "Auto Select");
    hs(p, 632.0, 584.0, 62.0, s::MORPH_INT, "Int", false);
    let rr = p.rect(632.0, 612.0, 62.0, 22.0);
    let rresp = p.ui.interact(rr, egui::Id::unique(("sc-rnd", p.t)), Sense::click());
    p.painter().rect_filled(rr, 4.0, if rresp.hovered() { Color32::from_gray(0x56) } else { Color32::from_gray(0x44) });
    p.painter().text(rr.center(), Align2::CENTER_CENTER, "Rnd", look::semibold(11.0), p.skin.text);
    if rresp.clicked() {
        p.actions.push("rnd".to_string());
    }
    // Control-click Rnd: which parameters are randomised
    let cur = p.geti(s::MORPH_RNDSET);
    let mut pick = None;
    rresp.context_menu(|ui| {
        for (k, l) in s::MORPH_RND.iter().enumerate() {
            if ui.add(egui::Button::selectable(k == cur, *l)).clicked() {
                pick = Some(k);
                ui.close();
            }
        }
    });
    if let Some(k) = pick {
        p.put(s::MORPH_RNDSET, k as f32);
    }
    cap(p, 446.0, 676.0, s::MORPH_RND[cur.min(4)], false);
}

// ---------------------------------------------------------------------------------------------
// the control envelopes and the Morph Envelope

fn right_lower(p: &mut Pane, live: &Live) {
    p.group(708.0, 468.0, 404.0, 238.0, "");
    let sel = tabs(p, 714.0, 472.0, 128.0, "env", &["Envelope 1", "Envelope 2", "Morph Envelope"]);
    if sel < 2 {
        control_envelope(p, sel, live);
    } else {
        morph_envelope(p, live);
    }
}

fn control_envelope(p: &mut Pane, i: usize, live: &Live) {
    let on = p.on(s::env(i, s::E_ENV)) || p.on(s::env(i, s::E_CTRL));
    p.toggle(716.0, 494.0, 34.0, 16.0, s::env(i, s::E_CTRL), "Ctrl");
    p.toggle(754.0, 494.0, 34.0, 16.0, s::env(i, s::E_ENV), "Env");
    p.menu(794.0, 494.0, 104.0, 16.0, s::env(i, s::E_SUS));
    p.seg(902.0, 494.0, 64.0, 16.0, s::env(i, s::E_SYNC), &["ms", "Sync"], false);
    p.field(970.0, 494.0, 54.0, 16.0, s::env(i, s::E_SCALE), 1.0);
    let rec = p.on(s::env(i, s::E_REC));
    let rr = p.rect(1030.0, 494.0, 24.0, 16.0);
    if p.button(rr, "R", rec, ("envrec", i)) {
        p.put(s::env(i, s::E_REC), if rec { 0.0 } else { 1.0 });
    }
    p.menu(1058.0, 494.0, 48.0, 16.0, s::env(i, s::E_TRIG));
    cap(p, 716.0, 520.0, "A-Time Velosens", false);
    let r = p.rect(716.0, 524.0, 84.0, 12.0);
    slider(p, r, s::env(i, s::E_ATV), false, p.skin.accent);
    // VariMod (recorded envelopes only)
    let has = p.on(s::env(i, s::E_HASREC));
    if has {
        p.menu(808.0, 520.0, 100.0, 16.0, s::env(i, s::E_VMSRC));
        let r = p.rect(912.0, 522.0, 56.0, 12.0);
        slider(p, r, s::env(i, s::E_VMAMT), false, ORANGE);
        let use_rec = p.on(s::env(i, s::E_USEREC));
        let cr = p.rect(974.0, 520.0, 64.0, 16.0);
        if p.button(cr, "Compare", use_rec, ("cmp", i)) {
            p.put(s::env(i, s::E_USEREC), if use_rec { 0.0 } else { 1.0 });
        }
    }
    let azk = format!("autozoom{i}");
    let az: bool = mem_get(p, &azk, true);
    let ar = p.rect(1044.0, 520.0, 60.0, 16.0);
    if p.button(ar, "Autozoom", az, ("az", i)) {
        mem_set(p, &azk, !az);
    }
    env_display(p, i, on, live);
    for rt in 0..2usize {
        let y = 650.0 + rt as f32 * 24.0;
        p.toggle(716.0, y, 20.0, 16.0, s::env_r(i, rt, s::R_ON), &format!("{}", rt + 1));
        p.menu(740.0, y, 112.0, 16.0, s::env_r(i, rt, s::R_TARGET));
        p.menu(856.0, y, 90.0, 16.0, s::env_r(i, rt, s::R_VIA));
        let via = p.geti(s::env_r(i, rt, s::R_VIA)) > 0;
        let r = p.rect(952.0, y + 1.0, if via { 70.0 } else { 150.0 }, 14.0);
        slider(p, r, s::env_r(i, rt, s::R_AMT), false, p.skin.accent);
        if via {
            let r = p.rect(1028.0, y + 1.0, 70.0, 14.0);
            slider(p, r, s::env_r(i, rt, s::R_VIAAMT), false, ORANGE);
        }
    }
}

fn env_display(p: &mut Pane, i: usize, active: bool, live: &Live) {
    let r = p.rect(716.0, 542.0, 388.0, 100.0);
    p.painter().rect_filled(r, 5.0, Color32::from_gray(if active { 0x12 } else { 0x1c }));
    // Control-click: Copy, Paste, Clear (envelopes move between 1 and 2, settings and instances)
    let bgr = p.ui.interact(r, egui::Id::unique(("sc-envbg", p.t, i)), Sense::click_and_drag());
    let key = egui::Id::unique(("sculpt-env-clip", 0));
    let fields: Vec<u32> = (0..64u32).filter(|f| !(14..24).contains(f)).collect();
    let vals: Vec<f32> = fields.iter().map(|f| p.get(s::env(i, *f))).collect();
    let mut act = 0;
    bgr.context_menu(|ui| {
        for (k, l) in ["Copy", "Paste", "Clear"].iter().enumerate() {
            if ui.button(*l).clicked() {
                act = k + 1;
                ui.close();
            }
        }
    });
    match act {
        1 => {
            p.ui.ctx().data_mut(|d| d.insert_temp(key, vals));
        }
        2 => {
            if let Some(v) = p.ui.ctx().data(|d| d.get_temp::<Vec<f32>>(key)).filter(|v| v.len() == fields.len()) {
                for (f, x) in fields.iter().zip(v) {
                    p.put(s::env(i, *f), x);
                }
            }
        }
        3 => {
            let dt = [20.0, 250.0, 400.0, 600.0];
            let dl = [0.0, 1.0, 0.6, 0.4, 0.0];
            for k in 0..4 {
                p.put(s::env_t(i, k), dt[k]);
            }
            for k in 0..5 {
                p.put(s::env_l(i, k), dl[k]);
            }
            for k in 0..s::REC_POINTS {
                p.put(s::env_rec(i, k), 0.0);
            }
            p.put(s::env(i, s::E_HASREC), 0.0);
            p.put(s::env(i, s::E_USEREC), 0.0);
        }
        _ => {}
    }
    if !active {
        p.painter().text(r.center(), Align2::CENTER_CENTER, "Switch Mode to Env or Ctrl+Env to use the envelope", look::regular(10.5), p.skin.dim);
        return;
    }
    let scale = p.get(s::env(i, s::E_SCALE)) * 0.01;
    let has = p.on(s::env(i, s::E_HASREC));
    let use_rec = has && p.on(s::env(i, s::E_USEREC));
    let mut cum = [0.0f32; 5];
    for k in 0..4 {
        cum[k + 1] = cum[k] + p.get(s::env_t(i, k)) * scale;
    }
    let total = if use_rec { (p.get(s::env(i, s::E_RECLEN)) * scale).max(cum[4]) } else { cum[4] };
    let azk = format!("autozoom{i}");
    let zk = format!("zoom{i}");
    let az: bool = mem_get(p, &azk, true);
    let mut visible: f32 = if az { total * 1.08 } else { mem_get(p, &zk, total.max(100.0) * 1.08) };
    visible = visible.max(40.0);
    let maxms = 40_000.0;
    // grid every 100 ms (1000 ms for very long envelopes)
    let step = if visible > 8000.0 { 1000.0 } else { 100.0 };
    let pt = p.painter().with_clip_rect(r);
    let mut g = 0.0;
    while g < visible {
        let x = r.min.x + g / visible * r.width();
        pt.line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], Stroke::new(1.0, Color32::from_gray(0x24)));
        g += step;
    }
    let px = |ms: f32| r.min.x + ms / visible * r.width();
    let py = |l: f32| r.max.y - 8.0 - l.clamp(0.0, 1.0) * (r.height() - 20.0);
    if has {
        let n = s::REC_POINTS;
        let pts: Vec<egui::Pos2> = (0..n).map(|k| pos2(px(total * k as f32 / (n - 1) as f32), py(p.get(s::env_rec(i, k))))).collect();
        pt.add(Shape::line(pts, Stroke::new(if use_rec { 2.0 } else { 1.0 }, if use_rec { GREEN } else { GREEN.gamma_multiply(0.35) })));
    }
    let nodes: Vec<egui::Pos2> = (0..5).map(|k| pos2(px(cum[k]), py(p.get(s::env_l(i, k))))).collect();
    pt.add(Shape::line(nodes.clone(), Stroke::new(1.5, if use_rec { ORANGE.gamma_multiply(0.5) } else { ORANGE })));
    pt.text(r.max - vec2(4.0, 10.0), Align2::RIGHT_CENTER, if az { format!("{:.0} ms", total) } else { format!("{:.0} ms shown", visible) }, look::regular(9.5), p.skin.dim);
    if let Some(prog) = live.env[i] {
        let x = px(prog * total);
        pt.line_segment([pos2(x, r.min.y), pos2(x, r.max.y)], Stroke::new(1.0, RED));
    }
    // dragging the background zooms by hand (and turns Autozoom off)
    if bgr.dragged() {
        let d = bgr.drag_delta().x;
        if d != 0.0 {
            visible = (visible * (1.0 - d / 200.0)).clamp(40.0, maxms);
            mem_set(p, &zk, visible);
            mem_set(p, &azk, false);
        }
    }
    // the nodes: start, attack, loop (L), sustain (S), end
    let labels = ["", "", "L", "S", ""];
    for k in 0..5usize {
        let c = nodes[k];
        let hr = Rect::from_center_size(c, vec2(14.0, 14.0));
        let resp = p.ui.interact(hr, egui::Id::unique(("sc-envnode", p.t, i, k)), Sense::click_and_drag());
        let seg = if k > 0 { p.get(s::env_t(i, k - 1)) } else { 0.0 };
        let resp = resp.on_hover_text(if k > 0 { format!("Segment {}: {:.0} ms", k, seg) } else { "Start level".to_string() });
        p.painter().circle_filled(c, 4.5, if resp.hovered() || resp.dragged() { Color32::WHITE } else { ORANGE });
        if !labels[k].is_empty() {
            p.painter().text(c + vec2(0.0, -10.0), Align2::CENTER_CENTER, labels[k], look::semibold(9.0), p.skin.text);
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                p.put(s::env_l(i, k), ((r.max.y - 8.0 - pp.y) / (r.height() - 20.0)).clamp(0.0, 1.0));
                if k > 0 {
                    // the node moves along the time axis: the segment before it changes length and
                    // every node after it moves with it
                    let ms_here = (pp.x - r.min.x) / r.width() * visible;
                    p.put(s::env_t(i, k - 1), ((ms_here - cum[k - 1]) / scale.max(1e-3)).clamp(1.0, 40000.0));
                }
                if has {
                    p.put(s::env(i, s::E_USEREC), 0.0);
                }
            }
        }
    }
}

fn morph_envelope(p: &mut Pane, live: &Live) {
    p.seg(716.0, 494.0, 236.0, 16.0, s::MENV_MODE, &["Off", "Pad only", "Env only", "Env + Pad"], false);
    p.menu(958.0, 494.0, 100.0, 16.0, s::MENV_SUB);
    let rec = p.on(s::MENV_REC);
    let rr = p.rect(1064.0, 494.0, 24.0, 16.0);
    if p.button(rr, "R", rec, "menvrec") {
        p.put(s::MENV_REC, if rec { 0.0 } else { 1.0 });
    }
    p.menu(716.0, 516.0, 130.0, 16.0, s::MENV_TRIG);
    p.menu(852.0, 516.0, 110.0, 16.0, s::MENV_SUSMODE);
    p.seg(968.0, 516.0, 64.0, 16.0, s::MENV_SYNC, &["ms", "Sync"], false);
    p.field(1038.0, 516.0, 54.0, 16.0, s::MENV_SCALE, 1.0);
    // the display: nine nodes, eight segments; X (orange) and Y (blue) of the morph position
    let r = p.rect(716.0, 540.0, 388.0, 90.0);
    p.painter().rect_filled(r, 5.0, Color32::from_gray(0x12));
    let scale = p.get(s::MENV_SCALE) * 0.01;
    let mut cum = [0.0f32; 9];
    for k in 1..9 {
        cum[k] = cum[k - 1] + p.get(s::mn(k, 0)) * scale;
    }
    let total = cum[8].max(10.0);
    let visible = total * 1.06;
    let px = |ms: f32| r.min.x + ms / visible * r.width();
    let py = |v: f32| r.max.y - 8.0 - v.clamp(0.0, 1.0) * (r.height() - 16.0);
    let pt = p.painter().with_clip_rect(r);
    let step = if visible > 8000.0 { 1000.0 } else { 100.0 };
    let mut g = 0.0;
    while g < visible {
        pt.line_segment([pos2(px(g), r.min.y), pos2(px(g), r.max.y)], Stroke::new(1.0, Color32::from_gray(0x24)));
        g += step;
    }
    let xs: Vec<egui::Pos2> = (0..9).map(|k| pos2(px(cum[k]), py(p.get(s::mn(k, 1))))).collect();
    let ys: Vec<egui::Pos2> = (0..9).map(|k| pos2(px(cum[k]), py(p.get(s::mn(k, 2))))).collect();
    pt.add(Shape::line(xs.clone(), Stroke::new(1.4, ORANGE)));
    pt.add(Shape::line(ys.clone(), Stroke::new(1.4, BLUE)));
    pt.text(r.max - vec2(4.0, 10.0), Align2::RIGHT_CENTER, format!("{:.0} ms", total), look::regular(9.5), p.skin.dim);
    if let Some(prog) = live.menv {
        pt.line_segment([pos2(px(prog * total), r.min.y), pos2(px(prog * total), r.max.y)], Stroke::new(1.0, RED));
    }
    let (lp, sp, ns) = (p.geti(s::MENV_LOOP).min(8), p.geti(s::MENV_SUS).min(8), p.geti(s::MENV_NSEL).min(8));
    for k in 0..9usize {
        let c = xs[k];
        let hr = Rect::from_center_size(pos2(c.x, r.center().y), vec2(12.0, r.height()));
        let resp = p.ui.interact(hr, egui::Id::unique(("sc-mnode", p.t, k)), Sense::click_and_drag());
        let seg = if k > 0 { p.get(s::mn(k, 0)) } else { 0.0 };
        let resp = resp.on_hover_text(if k > 0 { format!("Segment {}: {:.0} ms", k, seg) } else { "Start".to_string() });
        if resp.clicked() || resp.dragged() {
            p.put(s::MENV_NSEL, k as f32);
        }
        if resp.dragged() && k > 0 {
            if let Some(pp) = resp.interact_pointer_pos() {
                let ms_here = (pp.x - r.min.x) / r.width() * visible;
                p.put(s::mn(k, 0), ((ms_here - cum[k - 1]) / scale.max(1e-3)).clamp(1.0, 40000.0));
            }
        }
        let col = if k == ns { Color32::WHITE } else { Color32::from_gray(0xa0) };
        p.painter().circle_filled(xs[k], 3.5, if k == ns { Color32::WHITE } else { ORANGE });
        p.painter().circle_filled(ys[k], 3.5, if k == ns { Color32::WHITE } else { BLUE });
        let mark = if k == lp && k == sp { "LS" } else if k == lp { "L" } else if k == sp { "S" } else { "" };
        if !mark.is_empty() {
            p.painter().text(pos2(xs[k].x, r.min.y + 7.0), Align2::CENTER_CENTER, mark, look::semibold(9.0), col);
        }
    }
    hs(p, 716.0, 634.0, 84.0, s::MENV_DEPTH, "Depth", false);
    hs(p, 806.0, 634.0, 84.0, s::MENV_MOD, "Modulation", false);
    p.menu(896.0, 640.0, 94.0, 16.0, s::MENV_MODSRC);
    hs(p, 996.0, 634.0, 100.0, s::MENV_TRANS, "Transition (orig. – linear – stepped)", false);
    p.text(716.0, 672.0, Align2::LEFT_CENTER, "Loop node", 10.0, p.skin.dim);
    p.field(776.0, 664.0, 40.0, 16.0, s::MENV_LOOP, 1.0);
    p.text(826.0, 672.0, Align2::LEFT_CENTER, "Sustain node", 10.0, p.skin.dim);
    p.field(896.0, 664.0, 40.0, 16.0, s::MENV_SUS, 1.0);
    cap(p, 716.0, 692.0, "Selected node follows the morph ball in Env + Pad: Point Set / Point Solo", false);
}

// ---------------------------------------------------------------------------------------------

fn controllers(p: &mut Pane, learning: Option<u32>) {
    p.group(8.0, 710.0, 1104.0, 28.0, "");
    let mut x = 16.0;
    for (id, label) in [(s::CTRL_VIB, "Vib Depth Ctrl"), (s::CTRL_A, "Ctrl A"), (s::CTRL_B, "Ctrl B"), (s::CTRL_ENV1, "CtrlEnv 1"), (s::CTRL_ENV2, "CtrlEnv 2"), (s::CTRL_MX, "Morph X"), (s::CTRL_MY, "Morph Y")] {
        let lw = label.len() as f32 * 5.6 + 6.0;
        p.text(x, 725.0, Align2::LEFT_CENTER, label, 9.5, p.skin.dim);
        let r = p.rect(x + lw, 715.0, 64.0, 18.0);
        let cur = p.geti(id).min(128);
        let mut items: Vec<String> = (0..=128u32).map(s::cc_name).collect();
        items.insert(0, "Learn".to_string());
        let shown = if learning == Some(id) { "learning…".to_string() } else { s::cc_name(cur as u32) };
        if let Some(k) = popup(p, r, (id, 3), &shown, &items, cur + 1) {
            if k == 0 {
                p.actions.push(format!("learn:{id}"));
            } else {
                p.put(id, (k - 1) as f32);
            }
        }
        x += lw + 72.0;
    }
    p.text(x + 4.0, 725.0, Align2::LEFT_CENTER, "Mode", 9.5, p.skin.dim);
    p.menu(x + 36.0, 715.0, 112.0, 18.0, s::CTRL_MODE);
}
