//! Alchemy's modulation section, laid out as Logic's (advanced view): the rack of the current
//! target at the left (Target pop-up with the active targets, Smooth, up to ten slots of On /
//! source / Edit / ModMap / Depth, Back after Show Modulation), the six modulator panels in the
//! centre (LFO, AHDSR, MSEG, Sequencer, Env Follower, ModMap — each with its Current pop-up and
//! arrows, File menu: Preset, Save, Copy, Paste, Clear and, where Logic has it, Randomize) and
//! the Mod Targets menu. Any knob that is a modulation target: click it to show its rack;
//! Control-click (right-click) for Add / Clear / Copy / Paste / Show Modulation; it shows an
//! orange arc for its modulation and the blue ring while it is the current target.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_synth::alchemy_mod::{self as am, *};

use crate::look;
use crate::synthui::{rgb, Pane};

pub const ORANGE: Color32 = rgb(0xf0a038);
pub const BLUE: Color32 = rgb(0x46a6d6);
pub const FIELD: Color32 = rgb(0x15181d);
pub const BTN: Color32 = rgb(0x3a3e46);

/// The window's own state (by track).
#[derive(Clone, Default)]
pub struct St {
    /// 0 Advanced, 1 Simple, 2 Browse.
    pub view: u8,
    /// 0 Global, 1–4 sources A–D, 5 Morph.
    pub page: u8,
    /// 0 Perform, 1 Arp, 2 Effects.
    pub lower: u8,
    pub target: u32,
    pub back: Vec<u32>,
    pub tab: u8,
    pub cur: [usize; 6],
    pub rack_scroll: usize,
    pub sel: Vec<usize>,
    pub seq_scroll: usize,
    pub mseg_view: (f32, f32),
    pub search: String,
    pub hist: Vec<(f32, f32)>,
    pub save_name: String,
    pub rect_sel: Option<Pos2>,
}

/// Live values read from the running instrument for the displays.
#[derive(Clone, Copy, Default)]
pub struct Live {
    pub levels: [f32; 9],
    pub envf: [f32; 16],
    pub time: f64,
}

/// Parameter id → index, built once per frame (the window reads hundreds of values).
pub struct Ix(pub HashMap<u32, usize>);

impl Ix {
    pub fn new(params: &[lg_dsp::Param]) -> Ix {
        Ix(params.iter().enumerate().map(|(i, p)| (p.id, i)).collect())
    }
}

/// A value now (this frame's changes count).
pub fn g(p: &Pane, ix: &Ix, id: u32) -> f32 {
    if let Some(x) = p.set.iter().rev().find(|x| x.0 == id) {
        return x.1;
    }
    ix.0.get(&id).map(|&i| p.vals[i]).unwrap_or(0.0)
}

fn gi(p: &Pane, ix: &Ix, id: u32) -> usize {
    g(p, ix, id).round().max(0.0) as usize
}

/// The pool entries modulating a target, in rack order.
pub fn routes_of(p: &Pane, ix: &Ix, target: u32) -> Vec<usize> {
    (0..ROUTES).filter(|&n| gi(p, ix, route(n, RT_TARGET)) as u32 == target && g(p, ix, route(n, RT_SRC)) > 0.0).collect()
}

fn free_route(p: &Pane, ix: &Ix) -> Option<usize> {
    (0..ROUTES).find(|&n| g(p, ix, route(n, RT_TARGET)) < 0.5)
}

pub fn exists(p: &Pane, ix: &Ix, kind: u32, i: usize) -> bool {
    match kind {
        K_LFO | K_AHD | K_MSEG | K_SEQ | K_ENVF | K_MMAP => g(p, ix, field(kind, i, 0)) >= 0.5,
        _ => true,
    }
}

/// A target's name as the Target pop-up shows it.
pub fn target_name(p: &Pane, ix: &Ix, id: u32) -> String {
    if id >= DEPTH_TGT {
        let n = (id - DEPTH_TGT) as usize;
        let t = gi(p, ix, route(n, RT_TARGET)) as u32;
        let slot = routes_of(p, ix, t).iter().position(|&x| x == n).map(|k| k + 1).unwrap_or(1);
        return format!("{} Depth {slot}", target_name(p, ix, t));
    }
    ix.0.get(&id).map(|&i| p.params[i].name.to_string()).unwrap_or_default()
}

pub fn src_name(p: &Pane, ix: &Ix, c: f32) -> String {
    source_name(c, |i| gi(p, ix, lfo(i, LF_SHAPE)))
}

/// Can this parameter be a modulation target? (Menus, switches and the Perform section can't.)
pub fn modulatable(p: &Pane, ix: &Ix, id: u32) -> bool {
    if id >= DEPTH_TGT {
        return true;
    }
    if (1..=18).contains(&id) {
        return false;
    }
    ix.0.get(&id).is_some_and(|&i| !matches!(p.params[i].unit, lg_dsp::Unit::Menu | lg_dsp::Unit::OnOff))
}

/// Assigns a source to a target in a new rack slot.
pub fn add_route(p: &mut Pane, ix: &Ix, target: u32, src: f32) -> bool {
    let limit = if target >= DEPTH_TGT { PER_DEPTH } else { PER_TARGET };
    if routes_of(p, ix, target).len() >= limit {
        return false;
    }
    let Some(n) = free_route(p, ix) else { return false };
    let smooth = routes_of(p, ix, target).first().map(|&r| g(p, ix, route(r, RT_SMOOTH))).unwrap_or(0.0);
    let (k, _) = decode(src);
    // an AHDSR on Master Vol works at full depth
    let depth = if target == 94 && k == K_AHD { 1.0 } else { 0.0 };
    for (f, v) in [(RT_TARGET, target as f32), (RT_SRC, src), (RT_ON, 1.0), (RT_DEPTH, depth), (RT_SMOOTH, smooth), (RT_MMAP, 0.0)] {
        p.put(route(n, f), v);
    }
    true
}

/// Removes a rack entry (and whatever modulates its depth).
pub fn remove_route(p: &mut Pane, ix: &Ix, n: usize) {
    for q in routes_of(p, ix, DEPTH_TGT + n as u32) {
        remove_route(p, ix, q);
    }
    p.put(route(n, RT_TARGET), 0.0);
    p.put(route(n, RT_SRC), 0.0);
    p.put(route(n, RT_DEPTH), 0.0);
    p.put(route(n, RT_MMAP), 0.0);
}

/// Creates a new instance of a modulator kind (the source menus' New); its index.
pub fn new_instance(p: &mut Pane, ix: &Ix, kind: u32) -> Option<usize> {
    let i = (0..N).find(|&i| !exists(p, ix, kind, i))?;
    for f in fields(kind) {
        let id = field(kind, i, f);
        if let Some(&k) = ix.0.get(&id) {
            let d = p.params[k].default;
            p.put(id, d);
        }
    }
    p.put(field(kind, i, 0), 1.0);
    Some(i)
}

/// Deletes an instance and every rack entry it is in.
pub fn delete_instance(p: &mut Pane, ix: &Ix, kind: u32, i: usize) {
    p.put(field(kind, i, 0), 0.0);
    for n in 0..ROUTES {
        if kind == K_MMAP {
            if gi(p, ix, route(n, RT_MMAP)) == i + 1 {
                p.put(route(n, RT_MMAP), 0.0);
            }
        } else if g(p, ix, route(n, RT_TARGET)) > 0.5 && decode(g(p, ix, route(n, RT_SRC))) == (kind, i) {
            remove_route(p, ix, n);
        }
    }
}

const KINDS: [u32; 5] = [K_LFO, K_AHD, K_MSEG, K_SEQ, K_ENVF];

/// The Modulation Source menu (and Add Modulation): what was picked.
#[derive(Clone, Copy, PartialEq)]
pub enum Pick {
    None,
    Src(f32),
    New(u32),
    Del(u32, usize),
}

pub fn source_menu(ui: &mut egui::Ui, p: &Pane, ix: &Ix, with_none: bool) -> Option<Pick> {
    let mut pick = None;
    if with_none && ui.button("None").clicked() {
        pick = Some(Pick::None);
    }
    for kind in KINDS {
        ui.menu_button(kind_name(kind), |ui| {
            for i in 0..N {
                if exists(p, ix, kind, i) && ui.button(src_name(p, ix, code(kind, i))).clicked() {
                    pick = Some(Pick::Src(code(kind, i)));
                }
            }
            if (0..N).any(|i| !exists(p, ix, kind, i)) && ui.button("New").clicked() {
                pick = Some(Pick::New(kind));
            }
            ui.separator();
            for i in 0..N {
                if exists(p, ix, kind, i) && ui.button(format!("Del {} {}", kind_name(kind), i + 1)).clicked() {
                    pick = Some(Pick::Del(kind, i));
                }
            }
        });
    }
    for (kind, list) in [(K_MIDI, MIDI_SOURCES), (K_NOTE, NOTE_SOURCES), (K_PERF, PERF_SOURCES)] {
        ui.menu_button(kind_name(kind), |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (i, n) in list.iter().enumerate() {
                    if ui.button(*n).clicked() {
                        pick = Some(Pick::Src(code(kind, i)));
                    }
                }
            });
        });
    }
    if pick.is_some() {
        ui.close();
    }
    pick
}

/// Applies a source pick to a rack slot (`slot`: the entry, or None for the empty row).
fn apply_pick(p: &mut Pane, ix: &Ix, st: &mut St, target: u32, slot: Option<usize>, pick: Pick) {
    let src = match pick {
        Pick::Src(c) => Some(c),
        Pick::New(k) => new_instance(p, ix, k).map(|i| code(k, i)),
        Pick::Del(k, i) => {
            delete_instance(p, ix, k, i);
            None
        }
        Pick::None => {
            if let Some(n) = slot {
                remove_route(p, ix, n);
            }
            None
        }
    };
    if let Some(c) = src {
        match slot {
            Some(n) => p.put(route(n, RT_SRC), c),
            None => {
                add_route(p, ix, target, c);
            }
        }
        // the new modulator's panel
        let (k, i) = decode(c);
        if let Some(tab) = tab_of(k) {
            st.tab = tab;
            st.cur[tab as usize] = i;
        }
    }
}

fn tab_of(kind: u32) -> Option<u8> {
    match kind {
        K_LFO => Some(0),
        K_AHD => Some(1),
        K_MSEG => Some(2),
        K_SEQ => Some(3),
        K_ENVF => Some(4),
        K_MMAP => Some(5),
        _ => None,
    }
}
const TAB_KINDS: [u32; 6] = [K_LFO, K_AHD, K_MSEG, K_SEQ, K_ENVF, K_MMAP];

/// The modulation clipboard (Copy / Paste Modulation) and a modulator's (File > Copy / Paste).
fn clip_id() -> egui::Id {
    egui::Id::unique("alchemy-mod-clipboard")
}
fn mclip_id() -> egui::Id {
    egui::Id::unique("alchemy-modulator-clipboard")
}

/// A knob that is a modulation target: the knob, its modulation arc, the current-target ring,
/// click to show its rack, the shortcut menu.
pub fn knob(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, d: f32, id: u32, label: &str) {
    p.knob(x, y, d, id, label);
    let c = p.o + vec2(x, y + d / 2.0 + 2.0);
    let kr = Rect::from_center_size(c, vec2(d, d));
    target_marks(p, ix, st, id, c, d);
    target_sense(p, ix, st, kr, id);
}

/// The orange arc for a modulated control and the blue ring for the current target.
pub fn target_marks(p: &Pane, ix: &Ix, st: &St, id: u32, c: Pos2, d: f32) {
    let rs = routes_of(p, ix, id);
    let pt = p.ui.painter();
    let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
    let sweep = std::f32::consts::PI * 5.0 / 3.0;
    if !rs.is_empty() {
        let prm = p.prm(id);
        let frac = ((g(p, ix, id) - prm.min) / (prm.max - prm.min).max(1e-9)).clamp(0.0, 1.0);
        let amt: f32 = rs.iter().filter(|&&n| g(p, ix, route(n, RT_ON)) >= 0.5).map(|&n| g(p, ix, route(n, RT_DEPTH))).sum::<f32>().clamp(-1.0, 1.0);
        let to = if is_volume(id) { frac * (1.0 - amt.abs()) } else { (frac + amt).clamp(0.0, 1.0) };
        let (a0, a1) = (start + sweep * frac.min(to), start + sweep * frac.max(to));
        let r = d / 2.0 + 2.5;
        let pts: Vec<Pos2> = (0..=24).map(|k| {
            let a = a0 + (a1 - a0) * k as f32 / 24.0;
            c + vec2(a.cos(), a.sin()) * r
        }).collect();
        pt.add(egui::Shape::line(pts, Stroke::new(2.5, ORANGE)));
    }
    if st.target == id {
        pt.circle_stroke(c, d / 2.0 + 4.0, Stroke::new(1.2, BLUE));
    }
}

/// Click: this knob becomes the current target; double-click: its default; Control-click: menu.
pub fn target_sense(p: &mut Pane, ix: &Ix, st: &mut St, kr: Rect, id: u32) {
    let pressed = p.ui.input(|i| i.pointer.primary_pressed() && i.pointer.interact_pos().is_some_and(|q| kr.contains(q)));
    if pressed && modulatable(p, ix, id) && st.target != id {
        st.target = id;
        st.rack_scroll = 0;
        // the first slot's modulator panel
        if let Some(&n) = routes_of(p, ix, id).first() {
            let (k, i) = decode(g(p, ix, route(n, RT_SRC)));
            if let Some(tab) = tab_of(k) {
                st.tab = tab;
                st.cur[tab as usize] = i;
            }
        }
    }
    let resp = p.ui.interact(kr, p.id("aknob-sense", id), Sense::click());
    if resp.double_clicked() {
        let d = p.prm(id).default;
        p.put(id, d);
    }
    if !modulatable(p, ix, id) {
        return;
    }
    let mut act: Option<(u8, Option<Pick>)> = None;
    resp.context_menu(|ui| {
        ui.menu_button("Add Modulation", |ui| {
            if let Some(pk) = source_menu(ui, p, ix, false) {
                act = Some((0, Some(pk)));
            }
        });
        if ui.button("Clear Modulation").clicked() {
            act = Some((1, None));
            ui.close();
        }
        if ui.button("Copy Modulation").clicked() {
            act = Some((2, None));
            ui.close();
        }
        let have = ui.data(|d| d.get_temp::<Vec<[f32; 4]>>(clip_id())).is_some();
        if ui.add_enabled(have, egui::Button::new("Paste Modulation")).clicked() {
            act = Some((3, None));
            ui.close();
        }
        ui.separator();
        if ui.button("Show Modulation").clicked() {
            act = Some((4, None));
            ui.close();
        }
    });
    match act {
        Some((0, Some(pk))) => {
            st.target = id;
            apply_pick(p, ix, st, id, None, pk);
        }
        Some((1, _)) => {
            for n in routes_of(p, ix, id) {
                remove_route(p, ix, n);
            }
        }
        Some((2, _)) => {
            let list: Vec<[f32; 4]> = routes_of(p, ix, id).iter().map(|&n| [g(p, ix, route(n, RT_SRC)), g(p, ix, route(n, RT_DEPTH)), g(p, ix, route(n, RT_SMOOTH)), g(p, ix, route(n, RT_MMAP))]).collect();
            p.ui.data_mut(|d| d.insert_temp(clip_id(), list));
        }
        Some((3, _)) => {
            let list: Vec<[f32; 4]> = p.ui.data(|d| d.get_temp(clip_id())).unwrap_or_default();
            for e in list {
                if let Some(n) = free_route(p, ix) {
                    if routes_of(p, ix, id).len() >= PER_TARGET {
                        break;
                    }
                    for (f, v) in [(RT_TARGET, id as f32), (RT_SRC, e[0]), (RT_ON, 1.0), (RT_DEPTH, e[1]), (RT_SMOOTH, e[2]), (RT_MMAP, e[3])] {
                        p.put(route(n, f), v);
                    }
                }
            }
        }
        Some((4, _)) => {
            if st.target != id {
                st.back.push(st.target);
            }
            st.target = id;
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- small controls

/// A dark field with text (a pop-up's face); returns its response.
pub fn field_btn(p: &mut Pane, r: Rect, text: &str, salt: impl std::hash::Hash + std::fmt::Debug, caret: bool) -> egui::Response {
    let resp = p.ui.interact(r, egui::Id::unique(("alch-field", p.t, salt)), Sense::click());
    let pt = p.ui.painter();
    pt.rect_filled(r, 3.0, if resp.hovered() { rgb(0x23272e) } else { FIELD });
    let tx = if caret { r.center().x - 6.0 } else { r.center().x };
    pt.with_clip_rect(r).text(pos2(tx, r.center().y), Align2::CENTER_CENTER, text, look::regular(11.0), p.skin.text);
    if caret {
        crate::glyphs::paint(pt, Rect::from_center_size(pos2(r.max.x - 9.0, r.center().y), vec2(10.0, 10.0)), crate::glyphs::ph::CARET_DOWN, 9.0, p.skin.dim);
    }
    resp
}

/// A flat button (lit: blue); true when clicked.
pub fn flat(p: &mut Pane, r: Rect, label: &str, lit: bool, enabled: bool, salt: impl std::hash::Hash + std::fmt::Debug) -> bool {
    let resp = p.ui.interact(r, egui::Id::unique(("alch-flat", p.t, salt)), if enabled { Sense::click() } else { Sense::hover() });
    let pt = p.ui.painter();
    let fill = if lit { BLUE } else if resp.hovered() && enabled { rgb(0x4a4f58) } else { BTN };
    pt.rect_filled(r, 3.0, fill);
    let col = if !enabled { rgb(0x6a6e75) } else if lit { Color32::WHITE } else { p.skin.text };
    pt.text(r.center(), Align2::CENTER_CENTER, label, look::regular(11.0), col);
    enabled && resp.clicked()
}

/// ‹ › arrows at r (two halves); −1, 0 or 1.
pub fn arrows(p: &mut Pane, r: Rect, salt: impl std::hash::Hash + Copy + std::fmt::Debug) -> i32 {
    let mut out = 0;
    for (k, (icon, by)) in [(crate::glyphs::ph::CARET_LEFT, -1), (crate::glyphs::ph::CARET_RIGHT, 1)].into_iter().enumerate() {
        let h = Rect::from_min_size(pos2(r.min.x + k as f32 * r.width() / 2.0, r.min.y), vec2(r.width() / 2.0, r.height()));
        let resp = p.ui.interact(h, egui::Id::unique(("alch-arrow", p.t, salt, k)), Sense::click());
        crate::glyphs::paint(p.ui.painter(), h, icon, 11.0, if resp.hovered() { Color32::WHITE } else { p.skin.dim });
        if resp.clicked() {
            out = by;
        }
    }
    out
}

/// A parameter pop-up with ‹ › arrows after it.
pub fn menu_arrows(p: &mut Pane, ix: &Ix, x: f32, y: f32, w: f32, id: u32) {
    p.menu(x, y, w, 20.0, id);
    let r = p.rect(x + w + 2.0, y, 30.0, 20.0);
    let d = arrows(p, r, ("ma", id));
    if d != 0 {
        let prm = p.prm(id);
        let v = (g(p, ix, id) + d as f32).clamp(prm.min, prm.max);
        p.put(id, v);
    }
}

fn small_label(p: &Pane, x: f32, y: f32, s: &str) {
    p.text(x, y, Align2::LEFT_CENTER, s, 10.0, p.skin.dim);
}

// ---------------------------------------------------------------- the section

/// The modulation section at (x0, y0), w × h.
pub fn section(p: &mut Pane, ix: &Ix, st: &mut St, live: &Live, x0: f32, y0: f32, w: f32, h: f32) {
    let pt = p.painter();
    pt.rect_filled(p.rect(x0, y0, w, h), 0.0, rgb(0x1d2127));
    pt.text(p.o + vec2(x0 + 15.0, y0 + 17.0), Align2::LEFT_CENTER, "MODULATION", look::regular(12.5), p.skin.text);
    if st.target == 0 {
        st.target = 94;
    }
    rack(p, ix, st, x0 + 10.0, y0 + 36.0, 330.0, h - 44.0);
    // the type buttons
    let tabs = ["LFO", "AHDSR", "MSEG", "SEQUENCER", "ENV FOLLOWER", "MODMAP"];
    let tx = x0 + 345.0;
    pt.rect_filled(p.rect(tx, y0, 612.0, 28.0), 0.0, rgb(0x16191e));
    for (k, n) in tabs.iter().enumerate() {
        let r = p.rect(tx + 4.0 + k as f32 * 101.0, y0 + 2.0, 100.0, 24.0);
        let resp = p.ui.interact(r, egui::Id::unique(("alch-modtab", p.t, k)), Sense::click());
        let lit = st.tab as usize == k;
        if lit {
            pt.rect_filled(r, 0.0, rgb(0x262a31));
        }
        pt.text(r.center(), Align2::CENTER_CENTER, *n, look::regular(11.5), if lit { ORANGE } else if resp.hovered() { Color32::WHITE } else { p.skin.text });
        if resp.clicked() {
            st.tab = k as u8;
        }
    }
    mod_targets(p, ix, st, p.rect(x0 + w - 86.0, y0 + 6.0, 78.0, 20.0));
    let px = tx + 4.0;
    let py = y0 + 36.0;
    let pw = x0 + w - px - 10.0;
    match st.tab {
        0 => lfo_panel(p, ix, st, live, px, py, pw),
        1 => ahd_panel(p, ix, st, px, py, pw),
        2 => mseg_panel(p, ix, st, px, py, pw, h - 44.0),
        3 => seq_panel(p, ix, st, px, py, pw, h - 44.0),
        4 => envf_panel(p, ix, st, live, px, py, pw),
        _ => mmap_panel(p, ix, st, px, py, pw, h - 44.0),
    }
}

fn rack(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, w: f32, h: f32) {
    let pt = p.painter();
    pt.rect_filled(p.rect(x, y, w, h), 0.0, rgb(0x22262d));
    let target = st.target;
    // Back (after Show Modulation)
    let mut tx = x + 8.0;
    if !st.back.is_empty() {
        let r = p.rect(x + 4.0, y + 6.0, 22.0, 22.0);
        let resp = p.ui.interact(r, egui::Id::unique(("alch-back", p.t)), Sense::click()).on_hover_text("Back");
        crate::glyphs::paint(p.ui.painter(), r, crate::glyphs::ph::CARET_LEFT, 13.0, if resp.hovered() { Color32::WHITE } else { p.skin.dim });
        if resp.clicked()
            && let Some(b) = st.back.pop()
        {
            st.target = b;
        }
        tx += 22.0;
    }
    small_label(p, tx, y + 17.0, "Target");
    let name = target_name(p, ix, target);
    let tr = p.rect(tx + 38.0, y + 7.0, 160.0, 20.0);
    let resp = field_btn(p, tr, &name, "target", true);
    // the active targets
    let mut active: Vec<u32> = (0..ROUTES).filter(|&n| g(p, ix, route(n, RT_SRC)) > 0.0).map(|n| gi(p, ix, route(n, RT_TARGET)) as u32).filter(|&t| t > 0).collect();
    active.sort();
    active.dedup();
    let names: Vec<(u32, String)> = active.iter().map(|&t| (t, target_name(p, ix, t))).collect();
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        if names.is_empty() {
            ui.label("No modulations");
        }
        for (t, n) in &names {
            if ui.add(egui::Button::selectable(*t == target, n.as_str())).clicked() {
                pick = Some(*t);
            }
        }
    });
    if let Some(t) = pick {
        st.target = t;
        st.rack_scroll = 0;
    }
    // Smooth (for the whole target)
    let rs = routes_of(p, ix, target);
    small_label(p, x + w - 78.0, y + 17.0, "Smooth");
    let mut sm = rs.first().map(|&n| g(p, ix, route(n, RT_SMOOTH))).unwrap_or(0.0);
    let kr = p.rect(x + w - 40.0, y + 3.0, 28.0, 28.0 + 18.0);
    if rs.is_empty() {
        crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-smooth-off", p.t)), &mut sm, 0.0, 1.0, 0.0, "", &|v| format!("{:.0} %", v * 100.0));
    } else if crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-smooth", p.t)), &mut sm, 0.0, 1.0, 0.0, "", &|v| format!("{:.0} %", v * 100.0)) {
        for &n in &rs {
            p.put(route(n, RT_SMOOTH), sm);
        }
    }
    // slots: the assigned ones and one empty (up to ten)
    let limit = if target >= DEPTH_TGT { PER_DEPTH } else { PER_TARGET };
    let mut rows: Vec<Option<usize>> = rs.iter().map(|&n| Some(n)).collect();
    if rows.len() < limit {
        rows.push(None);
    }
    let row_h = 39.0;
    let top = y + 36.0;
    let visible = (((h - 40.0) / row_h).floor() as usize).max(1);
    // scrolling when the slots don't fit
    let area = p.rect(x, top, w, h - 36.0);
    if rows.len() > visible {
        if p.ui.rect_contains_pointer(area) {
            let dy = p.ui.input(|i| i.smooth_scroll_delta.y);
            if dy < -1.0 {
                st.rack_scroll += 1;
            } else if dy > 1.0 {
                st.rack_scroll = st.rack_scroll.saturating_sub(1);
            }
        }
        st.rack_scroll = st.rack_scroll.min(rows.len() - visible);
        let bar = p.rect(x + w - 5.0, top + st.rack_scroll as f32 / rows.len() as f32 * (h - 40.0), 3.0, visible as f32 / rows.len() as f32 * (h - 40.0));
        p.ui.painter().rect_filled(bar, 1.5, rgb(0x5a5f68));
    } else {
        st.rack_scroll = 0;
    }
    for (k, slot) in rows.iter().enumerate().skip(st.rack_scroll).take(visible) {
        let ry = top + (k - st.rack_scroll) as f32 * row_h;
        p.ui.painter().rect_filled(p.rect(x + 2.0, ry, w - 8.0, row_h - 3.0), 0.0, rgb(0x1f2329));
        rack_row(p, ix, st, target, *slot, x, ry + 8.0);
    }
}

fn rack_row(p: &mut Pane, ix: &Ix, st: &mut St, target: u32, slot: Option<usize>, x: f32, y: f32) {
    let on = slot.is_some_and(|n| g(p, ix, route(n, RT_ON)) >= 0.5);
    if flat(p, p.rect(x + 8.0, y, 30.0, 20.0), "On", on, slot.is_some(), ("rt-on", slot)) {
        let n = slot.unwrap_or(0);
        p.put(route(n, RT_ON), if on { 0.0 } else { 1.0 });
    }
    let src = slot.map(|n| g(p, ix, route(n, RT_SRC))).unwrap_or(0.0);
    let name = if slot.is_some() { src_name(p, ix, src) } else { String::new() };
    let resp = field_btn(p, p.rect(x + 44.0, y, 118.0, 20.0), &name, ("rt-src", slot, target), true);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        pick = source_menu(ui, p, ix, true);
    });
    if let Some(pk) = pick {
        apply_pick(p, ix, st, target, slot, pk);
    }
    // Edit: shows the slot's modulator panel
    let (k, i) = decode(src);
    let tab = tab_of(k);
    let shown = tab.is_some_and(|tb| st.tab == tb && st.cur[tb as usize] == i);
    if flat(p, p.rect(x + 166.0, y, 18.0, 20.0), "E", false, slot.is_some() && tab.is_some() && !shown, ("rt-e", slot))
        && let Some(tb) = tab
    {
        st.tab = tb;
        st.cur[tb as usize] = i;
    }
    // ModMap
    let mm = slot.map(|n| gi(p, ix, route(n, RT_MMAP))).unwrap_or(0);
    let mresp = field_btn(p, p.rect(x + 188.0, y, 38.0, 20.0), &if mm == 0 { "-".to_string() } else { mm.to_string() }, ("rt-mm", slot), true);
    let mut mpick: Option<i32> = None;
    if let Some(n) = slot {
        egui::Popup::menu(&mresp).show(|ui| {
            if ui.button("-").clicked() {
                mpick = Some(0);
            }
            for i in 0..N {
                if exists(p, ix, K_MMAP, i) && ui.button(format!("ModMap {}", i + 1)).clicked() {
                    mpick = Some(i as i32 + 1);
                }
            }
            if ui.button("New").clicked() {
                mpick = Some(-1);
            }
            ui.menu_button("Delete", |ui| {
                for i in 0..N {
                    if exists(p, ix, K_MMAP, i) && ui.button(format!("ModMap {}", i + 1)).clicked() {
                        mpick = Some(-100 - i as i32);
                        ui.close();
                    }
                }
            });
        });
        match mpick {
            Some(-1) => {
                if let Some(i) = new_instance(p, ix, K_MMAP) {
                    p.put(route(n, RT_MMAP), i as f32 + 1.0);
                    st.tab = 5;
                    st.cur[5] = i;
                }
            }
            Some(v) if v <= -100 => delete_instance(p, ix, K_MMAP, (-100 - v) as usize),
            Some(v) => p.put(route(n, RT_MMAP), v as f32),
            None => {}
        }
    }
    // Depth (fixed at 100 % for an AHDSR on Master Vol)
    p.text(x + 248.0, y + 10.0, Align2::LEFT_CENTER, "Depth", 11.0, p.skin.text);
    if let Some(n) = slot {
        let id = route(n, RT_DEPTH);
        let fixed = target == 94 && k == K_AHD;
        let mut v = g(p, ix, id);
        let kr = p.rect(x + 286.0, y - 4.0, 28.0, 28.0 + 18.0);
        let c = kr.min + vec2(14.0, 16.0);
        if fixed {
            let mut one = 1.0;
            crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-depth-fixed", p.t, n)), &mut one, -1.0, 1.0, 0.0, "", &|_| "100 % (fixed)".into());
        } else {
            if crate::widgets::knob(p.ui, kr, egui::Id::unique(("alch-depth", p.t, n)), &mut v, -1.0, 1.0, 0.0, "", &|v| format!("{:.0} %", v * 100.0)) {
                p.put(id, v);
            }
            let dt = DEPTH_TGT + n as u32;
            target_marks(p, ix, st, dt, c, 24.0);
            // the depth's own rack: Control-click > Show Modulation
            let resp = p.ui.interact(Rect::from_center_size(c, vec2(24.0, 24.0)), egui::Id::unique(("alch-depth-sense", p.t, n)), Sense::click());
            if resp.double_clicked() {
                p.put(id, 0.0);
            }
            let mut show = false;
            resp.context_menu(|ui| {
                if ui.button("Show Modulation").clicked() {
                    show = true;
                    ui.close();
                }
            });
            if show {
                st.back.push(st.target);
                st.target = dt;
            }
        }
    }
}

/// The Mod Targets menu.
fn mod_targets(p: &mut Pane, ix: &Ix, st: &mut St, r: Rect) {
    let resp = p.ui.interact(r, egui::Id::unique(("alch-modtargets", p.t)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { BTN });
    p.ui.painter().text(r.center(), Align2::CENTER_CENTER, "Mod Targets", look::regular(11.0), p.skin.text);
    let used: Vec<(usize, u32, f32, usize)> = (0..ROUTES).filter(|&n| g(p, ix, route(n, RT_SRC)) > 0.0 && g(p, ix, route(n, RT_TARGET)) > 0.5).map(|n| (n, gi(p, ix, route(n, RT_TARGET)) as u32, g(p, ix, route(n, RT_SRC)), gi(p, ix, route(n, RT_MMAP)))).collect();
    let cur_kind = TAB_KINDS[st.tab as usize];
    let cur_i = st.cur[st.tab as usize];
    let mut pick: Option<u32> = None;
    let mut remove_unused = false;
    egui::Popup::menu(&resp).show(|ui| {
        ui.menu_button("Current Modulator Targets", |ui| {
            for &(_, t, s, mm) in &used {
                let hit = if cur_kind == K_MMAP { mm == cur_i + 1 } else { decode(s) == (cur_kind, cur_i) };
                if hit && ui.button(target_name(p, ix, t)).clicked() {
                    pick = Some(t);
                }
            }
        });
        ui.menu_button("All Targets", |ui| {
            for kind in [K_LFO, K_AHD, K_MSEG, K_SEQ, K_ENVF, K_MIDI, K_NOTE, K_PERF] {
                let mine: Vec<&(usize, u32, f32, usize)> = used.iter().filter(|u| decode(u.2).0 == kind).collect();
                if mine.is_empty() {
                    continue;
                }
                ui.menu_button(kind_name(kind), |ui| {
                    let mut srcs: Vec<f32> = mine.iter().map(|u| u.2).collect();
                    srcs.sort_by(|a, b| a.total_cmp(b));
                    srcs.dedup();
                    for s in srcs {
                        ui.menu_button(src_name(p, ix, s), |ui| {
                            for u in mine.iter().filter(|u| u.2 == s) {
                                if ui.button(target_name(p, ix, u.1)).clicked() {
                                    pick = Some(u.1);
                                }
                            }
                        });
                    }
                });
            }
        });
        ui.menu_button("ModMap", |ui| {
            for i in 0..N {
                let mine: Vec<&(usize, u32, f32, usize)> = used.iter().filter(|u| u.3 == i + 1).collect();
                if mine.is_empty() {
                    continue;
                }
                ui.menu_button(format!("ModMap {}", i + 1), |ui| {
                    for u in mine {
                        if ui.button(target_name(p, ix, u.1)).clicked() {
                            pick = Some(u.1);
                        }
                    }
                });
            }
        });
        ui.separator();
        if ui.button("Remove Unused Modulations").clicked() {
            remove_unused = true;
            ui.close();
        }
        if pick.is_some() {
            ui.close();
        }
    });
    if let Some(t) = pick {
        st.target = t;
    }
    if remove_unused {
        remove_unused_mods(p, ix);
    }
}

/// Remove Unused Modulations: entries on sources that are off, Max modulations (baked into the
/// knob's value), then modulators and ModMaps no entry uses.
pub fn remove_unused_mods(p: &mut Pane, ix: &Ix) {
    for n in 0..ROUTES {
        let t = gi(p, ix, route(n, RT_TARGET)) as u32;
        if t == 0 || g(p, ix, route(n, RT_SRC)) <= 0.0 {
            continue;
        }
        let name = target_name(p, ix, t);
        // a target in a source that's switched off
        let off_src = ["A", "B", "C", "D"].iter().enumerate().any(|(s, l)| name.ends_with(&format!(" {l}")) && g(p, ix, 277 + s as u32) < 0.5);
        if off_src {
            remove_route(p, ix, n);
            continue;
        }
        if decode(g(p, ix, route(n, RT_SRC))) == (K_NOTE, 18) && t < DEPTH_TGT && routes_of(p, ix, DEPTH_TGT + n as u32).is_empty() {
            let prm = p.prm(t);
            let d = g(p, ix, route(n, RT_DEPTH));
            let base = g(p, ix, t);
            let v = if is_volume(t) { base * if d >= 0.0 { 1.0 } else { 1.0 + d } } else { base + d * (prm.max - prm.min) };
            p.put(t, v.clamp(prm.min, prm.max));
            remove_route(p, ix, n);
        }
    }
    for kind in [K_LFO, K_AHD, K_MSEG, K_SEQ, K_ENVF] {
        for i in 0..N {
            let in_use = (0..ROUTES).any(|n| g(p, ix, route(n, RT_TARGET)) > 0.5 && decode(g(p, ix, route(n, RT_SRC))) == (kind, i));
            if exists(p, ix, kind, i) && !in_use {
                p.put(field(kind, i, 0), 0.0);
            }
        }
    }
    for i in 0..N {
        let in_use = (0..ROUTES).any(|n| g(p, ix, route(n, RT_TARGET)) > 0.5 && gi(p, ix, route(n, RT_MMAP)) == i + 1);
        if exists(p, ix, K_MMAP, i) && !in_use {
            p.put(mmap(i, MM_EXISTS), 0.0);
        }
    }
}

// ---------------------------------------------------------------- the panels' shared top

/// Current n ‹ ›, File menu; returns the instance shown (None when there is none).
fn panel_head(p: &mut Pane, ix: &Ix, st: &mut St, kind: u32, x: f32, y: f32) -> Option<usize> {
    let tab = tab_of(kind).unwrap_or(0) as usize;
    let have: Vec<usize> = (0..N).filter(|&i| exists(p, ix, kind, i)).collect();
    if have.is_empty() {
        p.text(x, y + 10.0, Align2::LEFT_CENTER, &format!("No {}", kind_name(kind)), 11.0, p.skin.dim);
        return None;
    }
    if !have.contains(&st.cur[tab]) {
        st.cur[tab] = have[0];
    }
    let i = st.cur[tab];
    let resp = field_btn(p, p.rect(x, y, 40.0, 20.0), &(i + 1).to_string(), ("cur", kind), false);
    egui::Popup::menu(&resp).show(|ui| {
        for &k in &have {
            if ui.add(egui::Button::selectable(k == i, (k + 1).to_string())).clicked() {
                st.cur[tab] = k;
            }
        }
    });
    let d = arrows(p, p.rect(x + 42.0, y, 30.0, 20.0), ("cur-ar", kind));
    if d != 0 {
        let at = have.iter().position(|&k| k == i).unwrap_or(0) as i32;
        st.cur[tab] = have[(at + d).rem_euclid(have.len() as i32) as usize];
    }
    small_label(p, x, y + 30.0, "Current");
    file_menu(p, ix, st, kind, st.cur[tab], p.rect(x + 78.0, y, 52.0, 20.0));
    Some(st.cur[tab])
}

fn mod_dir(ext: &str) -> Option<std::path::PathBuf> {
    crate::pluginwin::settings_dir("Alchemy").map(|d| d.join("modulators").join(ext))
}

/// The saved files of a modulator kind (name, path).
fn saved(kind: u32) -> Vec<(String, std::path::PathBuf)> {
    let ext = file_ext(kind);
    let Some(dir) = mod_dir(ext) else { return Vec::new() };
    let mut v: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).map(|p| (p.file_stem().unwrap_or_default().to_string_lossy().into_owned(), p)).collect();
    v.sort();
    v
}

fn read_fields(path: &std::path::Path) -> Vec<(u32, f32)> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| {
        let mut w = l.split_whitespace();
        Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
    }).collect()
}

fn instance_fields(p: &Pane, ix: &Ix, kind: u32, i: usize) -> Vec<(u32, f32)> {
    fields(kind).filter(|&f| f != 0).filter(|&f| ix.0.contains_key(&field(kind, i, f))).map(|f| (f, g(p, ix, field(kind, i, f)))).collect()
}

fn put_fields(p: &mut Pane, kind: u32, i: usize, list: &[(u32, f32)]) {
    for &(f, v) in list {
        if f != 0 || kind == K_MMAP {
            p.put(field(kind, i, f), v);
        }
    }
}

/// The File button: Preset ▸ (Studio's and the saved ones), Save…, Copy, Paste, Clear, Randomize.
fn file_menu(p: &mut Pane, ix: &Ix, st: &mut St, kind: u32, i: usize, r: Rect) {
    let resp = p.ui.interact(r, egui::Id::unique(("alch-file", p.t, kind)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { BTN });
    p.ui.painter().text(r.center(), Align2::CENTER_CENTER, "File", look::regular(11.0), p.skin.text);
    let mut load: Option<Vec<(u32, f32)>> = None;
    let mut act = 0u8;
    let files = saved(kind);
    let mut name = std::mem::take(&mut st.save_name);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.menu_button("Preset", |ui| {
            for (n, list) in presets(kind) {
                if ui.button(n).clicked() {
                    load = Some(list);
                    ui.close();
                }
            }
            if !files.is_empty() {
                ui.separator();
            }
            for (n, path) in &files {
                if ui.button(n).clicked() {
                    load = Some(read_fields(path));
                    ui.close();
                }
            }
        });
        ui.menu_button("Save…", |ui| {
            let te = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name").desired_width(150.0));
            let enter = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Save").clicked() || enter) && !name.trim().is_empty() {
                act = 1;
                ui.close();
            }
        });
        ui.separator();
        if ui.button("Copy").clicked() {
            act = 2;
            ui.close();
        }
        let have = ui.data(|d| d.get_temp::<(u32, Vec<(u32, f32)>)>(mclip_id())).is_some_and(|c| c.0 == kind);
        if ui.add_enabled(have, egui::Button::new("Paste")).clicked() {
            act = 3;
            ui.close();
        }
        if ui.button("Clear").clicked() {
            act = 4;
            ui.close();
        }
        if matches!(kind, K_AHD | K_MSEG | K_SEQ) && ui.button("Randomize").clicked() {
            act = 5;
            ui.close();
        }
        if kind == K_SEQ {
            ui.separator();
            for (k, n) in ["Import Velocity…", "Import Note…", "Import Groove…"].iter().enumerate() {
                if ui.button(*n).clicked() {
                    p.actions.push(format!("seq-import:{k}:{i}"));
                    ui.close();
                }
            }
        }
    });
    match act {
        1 => {
            let body: String = instance_fields(p, ix, kind, i).iter().map(|(f, v)| format!("{f} {v}\n")).collect();
            if let Some(dir) = mod_dir(file_ext(kind)) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join(format!("{}.{}", name.trim().replace('/', "-"), file_ext(kind))), body);
            }
            name.clear();
        }
        2 => {
            let list = instance_fields(p, ix, kind, i);
            p.ui.data_mut(|d| d.insert_temp(mclip_id(), (kind, list)));
        }
        3 => {
            if let Some((_, list)) = p.ui.data(|d| d.get_temp::<(u32, Vec<(u32, f32)>)>(mclip_id())) {
                put_fields(p, kind, i, &list);
            }
        }
        4 => {
            let list: Vec<(u32, f32)> = fields(kind).filter(|&f| f != 0).filter_map(|f| ix.0.get(&field(kind, i, f)).map(|&k| (f, p.params[k].default))).collect();
            put_fields(p, kind, i, &list);
            if kind == K_MMAP {
                p.put(mmap(i, MM_EXISTS), 1.0);
            }
        }
        5 => {
            let seed = (p.ui.input(|i| i.time) * 1000.0) as u32;
            let list = randomize(kind, seed, &|f| g(p, ix, field(kind, i, f)));
            put_fields(p, kind, i, &list);
        }
        _ => {}
    }
    if let Some(list) = load {
        put_fields(p, kind, i, &list);
    }
    st.save_name = name;
}

/// A knob in a modulator panel (a target too: its rack via Control-click > Show Modulation).
fn pknob(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, id: u32, label: &str) {
    knob(p, ix, st, x, y, 40.0, id, label);
}

fn display_bg(p: &Pane, r: Rect) {
    p.ui.painter().rect_filled(r, 0.0, rgb(0x0f1216));
}

// ---------------------------------------------------------------- LFO

fn lfo_panel(p: &mut Pane, ix: &Ix, st: &mut St, live: &Live, x: f32, y: f32, w: f32) {
    let Some(i) = panel_head(p, ix, st, K_LFO, x + 18.0, y + 2.0) else { return };
    p.menu(x + 18.0, y + 40.0, 64.0, 20.0, lfo(i, LF_TRIG));
    small_label(p, x + 18.0, y + 70.0, "Trigger");
    let sync = g(p, ix, lfo(i, LF_SYNC)) >= 0.5;
    if flat(p, p.rect(x + 96.0, y + 40.0, 52.0, 20.0), "Sync", sync, true, ("lfo-sync", i)) {
        p.put(lfo(i, LF_SYNC), if sync { 0.0 } else { 1.0 });
    }
    let bip = g(p, ix, lfo(i, LF_BIP)) >= 0.5;
    if flat(p, p.rect(x + 18.0, y + 84.0, 64.0, 20.0), "Bipolar", bip, true, ("lfo-bip", i)) {
        p.put(lfo(i, LF_BIP), if bip { 0.0 } else { 1.0 });
    }
    // the shape: categories, ‹ ›
    let dx = x + 180.0;
    let dw = w - 190.0;
    let shape = gi(p, ix, lfo(i, LF_SHAPE)).min(LFO_SHAPES.len() - 1);
    let resp = field_btn(p, p.rect(dx, y + 2.0, 170.0, 20.0), LFO_SHAPES[shape], ("lfo-shape", i), true);
    let mut pick = None;
    egui::Popup::menu(&resp).show(|ui| {
        for (cat, a, b) in LFO_CATEGORIES {
            ui.menu_button(*cat, |ui| {
                for s in *a..*b {
                    if ui.add(egui::Button::selectable(s == shape, LFO_SHAPES[s])).clicked() {
                        pick = Some(s);
                        ui.close();
                    }
                }
            });
        }
    });
    let d = arrows(p, p.rect(dx + 172.0, y + 2.0, 30.0, 20.0), ("lfo-shape-ar", i));
    if d != 0 {
        pick = Some((shape as i32 + d).rem_euclid(LFO_SHAPES.len() as i32) as usize);
    }
    if let Some(s) = pick {
        p.put(lfo(i, LF_SHAPE), s as f32);
    }
    let r = p.rect(dx, y + 26.0, dw, 92.0);
    display_bg(p, r);
    let ph = g(p, ix, lfo(i, LF_PHASE));
    let pts: Vec<(f32, f32)> = (0..=200).map(|k| {
        let x = k as f32 / 200.0;
        let v = shape_value(shape, x * 2.0 + ph, (0.3, 0.8));
        (x, 0.1 + v * 0.8)
    }).collect();
    p.curve(r, &pts, ORANGE);
    // where a free-running LFO is now
    let rate = if sync { 2.0 } else { g(p, ix, lfo(i, LF_RATE)).max(0.01) } as f64;
    let now = ((live.time * rate / shape_multiple(shape) as f64).fract() as f32) * 0.5;
    p.ui.painter().line_segment([pos2(r.min.x + now * r.width(), r.min.y), pos2(r.min.x + now * r.width(), r.max.y)], Stroke::new(1.0, rgb(0x5a5f68)));
    p.ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    let ky = y + 124.0;
    pknob(p, ix, st, dx + 22.0, ky, lfo(i, LF_DELAY), "Delay");
    pknob(p, ix, st, dx + 72.0, ky, lfo(i, LF_ATTACK), "Attack");
    pknob(p, ix, st, dx + 122.0, ky, lfo(i, LF_PHASE), "Phase");
    if sync {
        p.knob_menu(dx + 172.0, ky, 40.0, lfo(i, LF_DIV), "Rate");
    } else {
        pknob(p, ix, st, dx + 172.0, ky, lfo(i, LF_RATE), "Rate");
    }
}

// ---------------------------------------------------------------- AHDSR

fn ahd_panel(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, w: f32) {
    let Some(i) = panel_head(p, ix, st, K_AHD, x + 18.0, y + 2.0) else { return };
    p.menu(x + 18.0, y + 40.0, 64.0, 20.0, ahd(i, AH_TRIG));
    small_label(p, x + 18.0, y + 70.0, "Trigger");
    let sync = g(p, ix, ahd(i, AH_SYNC)) >= 0.5;
    if flat(p, p.rect(x + 96.0, y + 40.0, 52.0, 20.0), "Sync", sync, true, ("ahd-sync", i)) {
        p.put(ahd(i, AH_SYNC), if sync { 0.0 } else { 1.0 });
    }
    let dx = x + 180.0;
    let r = p.rect(dx, y, w - 190.0, 116.0);
    display_bg(p, r);
    let (a, h, d, s, rl) = (g(p, ix, ahd(i, AH_A)), g(p, ix, ahd(i, AH_H)), g(p, ix, ahd(i, AH_D)), g(p, ix, ahd(i, AH_S)), g(p, ix, ahd(i, AH_R)));
    let sus_w = ((a + h + d + rl) * 0.25).max(0.25);
    let end = a + h + d + sus_w + rl;
    let total = end.max(1.0);
    // the ruler: seconds, or beats when synced
    let ruler = Rect::from_min_size(r.min, vec2(r.width(), 16.0));
    let plot = Rect::from_min_max(pos2(r.min.x + 6.0, r.min.y + 20.0), pos2(r.max.x - 6.0, r.max.y - 6.0));
    let step = [0.1f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0].into_iter().find(|s| total / s <= 9.0).unwrap_or(20.0);
    let mut t = 0.0;
    while t <= total {
        let px = plot.min.x + t / total * plot.width();
        p.ui.painter().text(pos2(px, ruler.center().y), Align2::CENTER_CENTER, format!("{}", (t * 100.0).round() / 100.0), look::regular(10.0), p.skin.text);
        t += step;
    }
    if sync {
        p.ui.painter().text(pos2(r.max.x - 4.0, ruler.center().y), Align2::RIGHT_CENTER, "beats", look::regular(9.5), p.skin.dim);
    }
    let tx = |t: f32| plot.min.x + t / total * plot.width();
    let ly = |l: f32| plot.max.y - l * plot.height();
    let (ca, cd, cr) = (g(p, ix, ahd(i, AH_CA)), g(p, ix, ahd(i, AH_CD)), g(p, ix, ahd(i, AH_CR)));
    let segs = [(0.0, a, 0.0, 1.0, ca, AH_CA), (a, a + h, 1.0, 1.0, 0.0, 0), (a + h, a + h + d, 1.0, s, cd, AH_CD), (a + h + d, a + h + d + sus_w, s, s, 0.0, 0), (a + h + d + sus_w, end, s, 0.0, cr, AH_CR)];
    let mut poly = vec![pos2(tx(0.0), ly(0.0))];
    for &(t0, t1, l0, l1, c, _) in &segs {
        for k in 1..=24 {
            let f = k as f32 / 24.0;
            poly.push(pos2(tx(t0 + (t1 - t0) * f), ly(seg(l0, l1, f, c))));
        }
    }
    let fill = poly.clone();
    for wnd in fill.windows(2) {
        p.ui.painter().add(egui::Shape::convex_polygon(vec![wnd[0], wnd[1], pos2(wnd[1].x, ly(0.0)), pos2(wnd[0].x, ly(0.0))], ORANGE.gamma_multiply(0.18), Stroke::NONE));
    }
    p.ui.painter().add(egui::Shape::line(poly, Stroke::new(1.5, ORANGE)));
    // dragging segments bends them (Option-click: straight)
    let fine = if p.ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
    for (k, &(t0, t1, _, _, c, f)) in segs.iter().enumerate() {
        if f == 0 || t1 <= t0 {
            continue;
        }
        let sr = Rect::from_min_max(pos2(tx(t0) + 6.0, plot.min.y), pos2(tx(t1) - 6.0, plot.max.y));
        if sr.width() < 4.0 {
            continue;
        }
        let resp = p.ui.interact(sr, egui::Id::unique(("ahd-seg", p.t, i, k)), Sense::click_and_drag());
        if resp.clicked() && p.ui.input(|i| i.modifiers.alt) {
            p.put(ahd(i, f), 0.0);
        } else if resp.dragged() {
            // up: more convex, down: more concave
            let nc = (c - resp.drag_delta().y / 80.0 * fine).clamp(-1.0, 1.0);
            p.put(ahd(i, f), nc);
        }
    }
    // the points: peak (attack), hold end, decay end / sustain, release end
    let handles = [(a, 1.0, AH_A, 0.0), (a + h, 1.0, AH_H, a), (a + h + d, s, AH_D, a + h), (end, 0.0, AH_R, a + h + d + sus_w)];
    for (k, &(t, l, f, from)) in handles.iter().enumerate() {
        let c = pos2(tx(t), ly(l));
        p.ui.painter().circle_filled(c, 5.0, ORANGE);
        let resp = p.ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), egui::Id::unique(("ahd-pt", p.t, i, k)), Sense::drag());
        if resp.dragged() {
            let dlt = resp.drag_delta() * fine;
            let dtime = dlt.x / plot.width() * total;
            let cur = g(p, ix, ahd(i, f));
            let _ = from;
            p.put(ahd(i, f), (cur + dtime).clamp(0.0, 20.0));
            if f == AH_D {
                p.put(ahd(i, AH_S), (s - dlt.y / plot.height()).clamp(0.0, 1.0));
            }
        }
        if resp.hovered() || resp.dragged() {
            p.ui.painter().text(c + vec2(8.0, -10.0), Align2::LEFT_BOTTOM, format!("{:.2}", g(p, ix, ahd(i, f))), look::regular(10.0), Color32::WHITE);
        }
    }
    let ky = y + 124.0;
    let names = [(AH_A, "Attack"), (AH_H, "Hold"), (AH_D, "Decay"), (AH_S, "Sustain"), (AH_R, "Release")];
    for (k, (f, n)) in names.iter().enumerate() {
        pknob(p, ix, st, dx + 22.0 + k as f32 * 50.0, ky, ahd(i, *f), n);
    }
}

// ---------------------------------------------------------------- MSEG

fn snap_to(v: f32, n: u32) -> f32 {
    if n == 0 { v } else { (v * n as f32).round() / n as f32 }
}

fn mseg_panel(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, w: f32, h: f32) {
    let Some(i) = panel_head(p, ix, st, K_MSEG, x + 18.0, y + 2.0) else { return };
    p.menu(x + 18.0, y + 40.0, 64.0, 20.0, mseg(i, MS_TRIG));
    small_label(p, x + 18.0, y + 70.0, "Trigger");
    let sync = g(p, ix, mseg(i, MS_SYNC)) >= 0.5;
    if flat(p, p.rect(x + 96.0, y + 40.0, 52.0, 20.0), "Sync", sync, true, ("ms-sync", i)) {
        p.put(mseg(i, MS_SYNC), if sync { 0.0 } else { 1.0 });
    }
    p.menu(x + 18.0, y + 84.0, 130.0, 20.0, mseg(i, MS_LOOP));
    small_label(p, x + 18.0, y + 114.0, "Loop Mode");
    p.menu(x + 18.0, y + 126.0, 64.0, 20.0, mseg(i, MS_EDIT));
    small_label(p, x + 18.0, y + 156.0, "Edit Mode");
    menu_arrows(p, ix, x + 88.0, y + 126.0, 40.0, mseg(i, MS_SNAPY));
    small_label(p, x + 88.0, y + 156.0, "Snap Y");
    let snap_y = SNAP_N[gi(p, ix, mseg(i, MS_SNAPY)).min(SNAP_N.len() - 1)];
    let mode = gi(p, ix, mseg(i, MS_EDIT));
    let dx = x + 180.0;
    let r = p.rect(dx, y, w - 190.0, h - 26.0);
    display_bg(p, r);
    let n = gi(p, ix, mseg(i, MS_N)).clamp(2, MSEG_PTS);
    let pts: Vec<(f32, f32, f32)> = (0..n).map(|k| {
        let b = MS_PT + k as u32 * 3;
        (if k == 0 { 0.0 } else { g(p, ix, mseg(i, b)) }, g(p, ix, mseg(i, b + 1)), g(p, ix, mseg(i, b + 2)))
    }).collect();
    let end = pts.last().map(|q| q.0).unwrap_or(1.0).max(0.25);
    // the visible part (scroll bar with zoom ends below)
    let (mut v0, mut span) = st.mseg_view;
    if span <= 0.0 {
        (v0, span) = (0.0, end * 1.15);
    }
    let ruler = Rect::from_min_size(r.min, vec2(r.width(), 16.0));
    let plot = Rect::from_min_max(pos2(r.min.x + 6.0, r.min.y + 20.0), pos2(r.max.x - 6.0, r.max.y - 6.0));
    let tx = |t: f32| plot.min.x + (t - v0) / span * plot.width();
    let xt = |px: f32| v0 + (px - plot.min.x) / plot.width() * span;
    let ly = |l: f32| plot.max.y - l * plot.height();
    let pt = p.painter();
    let pc = pt.with_clip_rect(r);
    // grid (beats when synced)
    let step = if sync { 0.25 } else { [0.05f32, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0].into_iter().find(|s| span / s <= 10.0).unwrap_or(10.0) };
    let mut t = (v0 / step).ceil() * step;
    while t <= v0 + span {
        let px = tx(t);
        pc.line_segment([pos2(px, plot.min.y), pos2(px, plot.max.y)], Stroke::new(1.0, rgb(0x1c2026)));
        if !sync || (t * 4.0).round() as i32 % 4 == 0 {
            pc.text(pos2(px, ruler.center().y), Align2::CENTER_CENTER, format!("{}", (t * 100.0).round() / 100.0), look::regular(9.5), p.skin.dim);
        }
        t += step;
    }
    // loop markers: start (right-facing handle), end (left-facing)
    let ls = gi(p, ix, mseg(i, MS_LS)).min(n - 1);
    let le = gi(p, ix, mseg(i, MS_LE)).clamp(ls, n - 1);
    for (k, at, dir) in [(MS_LS, ls, 1.0f32), (MS_LE, le, -1.0)] {
        let px = tx(pts[at].0);
        pc.line_segment([pos2(px, ruler.max.y), pos2(px, plot.max.y)], Stroke::new(1.0, rgb(0x8a8f96)));
        let tri = vec![pos2(px, ruler.min.y + 2.0), pos2(px + 8.0 * dir, ruler.min.y + 8.0), pos2(px, ruler.max.y - 1.0)];
        pc.add(egui::Shape::convex_polygon(tri, rgb(0xb0b4ba), Stroke::NONE));
        let resp = p.ui.interact(Rect::from_center_size(pos2(px + 4.0 * dir, ruler.center().y), vec2(12.0, 16.0)), egui::Id::unique(("ms-loop", p.t, i, k)), Sense::drag());
        if resp.dragged()
            && let Some(q) = resp.interact_pointer_pos()
        {
            // the nearest point; never past the other marker
            let tq = xt(q.x);
            let near = (0..n).min_by(|&a, &b| (pts[a].0 - tq).abs().total_cmp(&(pts[b].0 - tq).abs())).unwrap_or(0);
            let v = if k == MS_LS { near.min(le.saturating_sub(1)) } else { near.max(ls + 1).min(n - 1) };
            p.put(mseg(i, k), v as f32);
        }
    }
    // the line
    let mut line = Vec::new();
    for k in 0..n.saturating_sub(1) {
        for s in 0..=16 {
            let f = s as f32 / 16.0;
            line.push(pos2(tx(pts[k].0 + (pts[k + 1].0 - pts[k].0) * f), ly(seg(pts[k].1, pts[k + 1].1, f, pts[k].2))));
        }
    }
    pc.add(egui::Shape::line(line.clone(), Stroke::new(1.5, ORANGE)));
    let fine = if p.ui.input(|i| i.modifiers.shift) { 0.25 } else { 1.0 };
    let cmd = p.ui.input(|i| i.modifiers.command);
    // points
    let mut dragged_pt = false;
    for k in 0..n {
        let c = pos2(tx(pts[k].0), ly(pts[k].1));
        if !r.contains(c) {
            continue;
        }
        let sel = st.sel.contains(&k);
        pc.circle_filled(c, 4.5, if sel { Color32::WHITE } else { ORANGE });
        let resp = p.ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), egui::Id::unique(("ms-pt", p.t, i, k)), Sense::click_and_drag());
        if resp.double_clicked() && k > 0 && n > 2 {
            // remove: later points move down one
            for j in k..n - 1 {
                for o in 0..3 {
                    p.put(mseg(i, MS_PT + j as u32 * 3 + o), [pts[j + 1].0, pts[j + 1].1, pts[j + 1].2][o as usize]);
                }
            }
            for (mk, v) in [(MS_LS, ls), (MS_LE, le)] {
                if v >= k && v > 0 {
                    p.put(mseg(i, mk), (v - 1) as f32);
                }
            }
            p.put(mseg(i, MS_N), (n - 1) as f32);
            st.sel.clear();
            continue;
        }
        if resp.drag_started() {
            // Command before the drag: horizontal only
            p.ui.data_mut(|d| d.insert_temp(egui::Id::unique(("ms-hlock", p.t)), cmd));
            if !sel {
                st.sel = vec![k];
            }
        }
        if resp.dragged() {
            dragged_pt = true;
            let hlock: bool = p.ui.data(|d| d.get_temp(egui::Id::unique(("ms-hlock", p.t)))).unwrap_or(false);
            // Command pressed after the drag began: vertical only
            let vlock = cmd && !hlock;
            let dl = resp.drag_delta() * fine;
            let dt = if vlock { 0.0 } else { dl.x / plot.width() * span };
            let dv = if hlock { 0.0 } else { -dl.y / plot.height() };
            let group: Vec<usize> = if st.sel.contains(&k) { st.sel.clone() } else { vec![k] };
            for &j in &group {
                let b = MS_PT + j as u32 * 3;
                let mut lv = (pts[j].1 + dv).clamp(0.0, 1.0);
                if fine == 1.0 && snap_y > 0 {
                    lv = snap_to(lv, snap_y);
                }
                p.put(mseg(i, b + 1), lv);
            }
            if dt != 0.0 && !group.contains(&0) {
                match mode {
                    // Slide: every later point moves too
                    1 => {
                        let first = *group.iter().min().unwrap_or(&k);
                        let lo = pts[first - 1].0;
                        let d = dt.max(lo - pts[first].0);
                        for j in first..n {
                            p.put(mseg(i, MS_PT + j as u32 * 3), (pts[j].0 + d).max(0.0));
                        }
                    }
                    // Stretch: the sides compress / stretch, the length stays
                    2 if k < n - 1 => {
                        let old = pts[k].0;
                        let new = (old + dt).clamp(pts[1.max(k) - 1].0.min(old) + 1e-3, end - 1e-3);
                        for j in 1..n - 1 {
                            let tj = pts[j].0;
                            let v = if tj <= old { tj / old.max(1e-6) * new } else { new + (tj - old) / (end - old).max(1e-6) * (end - new) };
                            p.put(mseg(i, MS_PT + j as u32 * 3), v);
                        }
                    }
                    _ => {
                        for &j in &group {
                            let lo = if j > 0 { pts[j - 1].0 } else { 0.0 };
                            let hi = if j + 1 < n && !group.contains(&(j + 1)) { pts[j + 1].0 } else { f32::MAX };
                            let mut v = (pts[j].0 + dt).clamp(lo, hi);
                            if sync && fine == 1.0 {
                                v = (v * 4.0).round() / 4.0;
                            }
                            p.put(mseg(i, MS_PT + j as u32 * 3), v.clamp(lo, hi));
                        }
                    }
                }
            }
        }
        if resp.hovered() || resp.dragged() {
            pc.text(c + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{:.2} {}, {:.0} %", pts[k].0, if sync { "beats" } else { "s" }, pts[k].1 * 100.0), look::regular(10.0), Color32::WHITE);
        }
    }
    // the background: click near the line adds a point; drag a segment to bend it (Option-click
    // straightens it); drag elsewhere to select points
    if !dragged_pt {
        let resp = p.ui.interact(plot, egui::Id::unique(("ms-bg", p.t, i)), Sense::click_and_drag());
        let hit_line = |q: Pos2| line.iter().any(|l| l.distance(q) < 8.0);
        if let Some(q) = resp.interact_pointer_pos() {
            let tq = xt(q.x).max(0.0);
            let segk = (0..n.saturating_sub(1)).find(|&k| tq >= pts[k].0 && tq <= pts[k + 1].0);
            if resp.clicked() && hit_line(q) {
                if p.ui.input(|i| i.modifiers.alt) {
                    if let Some(k) = segk {
                        p.put(mseg(i, MS_PT + k as u32 * 3 + 2), 0.0);
                    }
                } else if n < MSEG_PTS {
                    let at = segk.map(|k| k + 1).unwrap_or(n);
                    for j in (at..n).rev() {
                        for o in 0..3 {
                            p.put(mseg(i, MS_PT + (j + 1) as u32 * 3 + o), [pts[j].0, pts[j].1, pts[j].2][o as usize]);
                        }
                    }
                    let mut lv = ((plot.max.y - q.y) / plot.height()).clamp(0.0, 1.0);
                    if snap_y > 0 {
                        lv = snap_to(lv, snap_y);
                    }
                    let b = MS_PT + at as u32 * 3;
                    p.put(mseg(i, b), tq);
                    p.put(mseg(i, b + 1), lv);
                    p.put(mseg(i, b + 2), 0.0);
                    p.put(mseg(i, MS_N), (n + 1) as f32);
                    for (mk, v) in [(MS_LS, ls), (MS_LE, le)] {
                        if v >= at {
                            p.put(mseg(i, mk), (v + 1) as f32);
                        }
                    }
                }
            } else if resp.clicked() {
                st.sel.clear();
            }
            if resp.drag_started() {
                st.rect_sel = if hit_line(q) { None } else { Some(q) };
                p.ui.data_mut(|d| d.insert_temp(egui::Id::unique(("ms-seg", p.t)), segk.unwrap_or(usize::MAX)));
            }
            if resp.dragged() {
                if let Some(a) = st.rect_sel {
                    let sr = Rect::from_two_pos(a, q);
                    pc.rect_stroke(sr, 0.0, Stroke::new(1.0, rgb(0x8a8f96)), egui::StrokeKind::Inside);
                    st.sel = (0..n).filter(|&k| sr.contains(pos2(tx(pts[k].0), ly(pts[k].1)))).collect();
                } else {
                    let k: usize = p.ui.data(|d| d.get_temp(egui::Id::unique(("ms-seg", p.t)))).unwrap_or(usize::MAX);
                    if k + 1 < n {
                        let dc = -resp.drag_delta().y / 80.0 * fine;
                        p.put(mseg(i, MS_PT + k as u32 * 3 + 2), (pts[k].2 + dc).clamp(-1.0, 1.0));
                    }
                }
            }
            if resp.drag_stopped() {
                st.rect_sel = None;
            }
        }
    }
    // the scroll bar: drag it to scroll, its ends to zoom
    let sb = p.rect(dx, y + h - 22.0, w - 190.0, 10.0);
    p.ui.painter().rect_filled(sb, 5.0, rgb(0x15181d));
    let full = (end * 1.15).max(span + v0);
    let bx0 = sb.min.x + v0 / full * sb.width();
    let bx1 = sb.min.x + (v0 + span) / full * sb.width();
    let thumb = Rect::from_min_max(pos2(bx0, sb.min.y), pos2(bx1.max(bx0 + 12.0), sb.max.y));
    p.ui.painter().rect_filled(thumb, 5.0, rgb(0x4a4f58));
    for (k, er) in [Rect::from_min_size(thumb.min, vec2(6.0, 10.0)), Rect::from_min_size(pos2(thumb.max.x - 6.0, thumb.min.y), vec2(6.0, 10.0)), thumb.shrink2(vec2(6.0, 0.0))].into_iter().enumerate() {
        let resp = p.ui.interact(er, egui::Id::unique(("ms-sb", p.t, k)), Sense::drag());
        if resp.dragged() {
            let d = resp.drag_delta().x / sb.width() * full;
            match k {
                0 => {
                    let nv = (v0 + d).clamp(0.0, v0 + span - 0.05);
                    span += v0 - nv;
                    v0 = nv;
                }
                1 => span = (span + d).max(0.05),
                _ => v0 = (v0 + d).max(0.0),
            }
        }
    }
    st.mseg_view = (v0, span);
}

// ---------------------------------------------------------------- sequencer

fn seq_panel(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, w: f32, h: f32) {
    let Some(i) = panel_head(p, ix, st, K_SEQ, x + 18.0, y + 2.0) else { return };
    p.menu(x + 18.0, y + 40.0, 64.0, 20.0, seq(i, SQ_TRIG));
    small_label(p, x + 18.0, y + 70.0, "Trigger");
    p.menu(x + 88.0, y + 40.0, 60.0, 20.0, seq(i, SQ_EDIT));
    small_label(p, x + 88.0, y + 70.0, "Edit Mode");
    menu_arrows(p, ix, x + 18.0, y + 84.0, 34.0, seq(i, SQ_RATE));
    small_label(p, x + 18.0, y + 114.0, "Rate");
    menu_arrows(p, ix, x + 88.0, y + 84.0, 26.0, seq(i, SQ_SNAP));
    small_label(p, x + 88.0, y + 114.0, "Value Snap");
    let ky = y + 122.0;
    for (k, (f, n)) in [(SQ_SWING, "Swing"), (SQ_ATTACK, "Attack"), (SQ_GATE, "Gate"), (SQ_RELEASE, "Release")].iter().enumerate() {
        knob(p, ix, st, x + 34.0 + k as f32 * 40.0, ky, 30.0, seq(i, *f), n);
    }
    let edit = gi(p, ix, seq(i, SQ_EDIT));
    let snap = SNAP_N[gi(p, ix, seq(i, SQ_SNAP)).min(SNAP_N.len() - 1)];
    let len = gi(p, ix, seq(i, SQ_LEN)).clamp(1, SEQ_STEPS);
    let dx = x + 180.0;
    let dw = w - 190.0;
    let show = 32usize;
    let cols = (len + 1).clamp(16, SEQ_STEPS).min(show).max(16);
    let total = (len + 4).min(SEQ_STEPS).max(cols);
    st.seq_scroll = st.seq_scroll.min(total.saturating_sub(cols));
    let first = st.seq_scroll;
    let cw = dw / cols as f32;
    // the pattern length bar (drag its end)
    let lb = p.rect(dx, y, dw, 10.0);
    p.ui.painter().rect_filled(lb, 2.0, rgb(0x15181d));
    let lx = dx + (len as f32 - first as f32) * cw;
    let lbar = Rect::from_min_max(lb.min, pos2((p.o.x + lx).clamp(lb.min.x, lb.max.x), lb.max.y));
    p.ui.painter().rect_filled(lbar, 2.0, rgb(0x6a6f78));
    let resp = p.ui.interact(lb, egui::Id::unique(("sq-len", p.t, i)), Sense::click_and_drag());
    if (resp.dragged() || resp.clicked())
        && let Some(q) = resp.interact_pointer_pos()
    {
        let nl = (first as f32 + (q.x - lb.min.x) / cw).round().clamp(1.0, SEQ_STEPS as f32);
        p.put(seq(i, SQ_LEN), nl);
    }
    // the step numbers, then the steps
    for c in 0..cols {
        p.text(dx + (c as f32 + 0.5) * cw, y + 19.0, Align2::CENTER_CENTER, &(first + c + 1).to_string(), 9.0, p.skin.dim);
    }
    let r = p.rect(dx, y + 26.0, dw, h - 78.0);
    display_bg(p, r);
    let resp = p.ui.interact(r, egui::Id::unique(("sq-steps", p.t, i)), Sense::click_and_drag());
    if let Some(q) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let k = first + ((q.x - r.min.x) / cw).floor().max(0.0) as usize;
        if k < SEQ_STEPS {
            let f = ((r.max.y - q.y) / r.height()).clamp(0.0, 1.0);
            match edit {
                1 => p.put(seq(i, SQ_SLEN + k as u32), f),
                2 => p.put(seq(i, SQ_SW + k as u32), f * 2.0),
                _ => p.put(seq(i, SQ_VAL + k as u32), snap_to(f, snap)),
            }
        }
    }
    let pc = p.ui.painter().with_clip_rect(r);
    for c in 0..cols {
        let k = first + c;
        if k >= SEQ_STEPS {
            break;
        }
        let cx = r.min.x + c as f32 * cw;
        let used = k < len;
        let (v, col) = match edit {
            1 => (g(p, ix, seq(i, SQ_SLEN + k as u32)), BLUE),
            2 => (g(p, ix, seq(i, SQ_SW + k as u32)) / 2.0, rgb(0x8ad06a)),
            _ => (g(p, ix, seq(i, SQ_VAL + k as u32)), ORANGE),
        };
        let col = if used { col } else { col.gamma_multiply(0.3) };
        if edit == 2 {
            let mid = r.center().y;
            let yv = r.max.y - v * r.height();
            pc.rect_filled(Rect::from_min_max(pos2(cx + 2.0, yv.min(mid)), pos2(cx + cw - 2.0, yv.max(mid) + 1.0)), 1.0, col);
        } else if v > 0.0 {
            pc.rect_filled(Rect::from_min_max(pos2(cx + 2.0, r.max.y - v * r.height()), pos2(cx + cw - 2.0, r.max.y)), 1.0, col);
        }
        // the tie button under the step
        let tr = Rect::from_min_size(pos2(cx + 2.0, r.max.y + 3.0), vec2(cw - 4.0, 9.0));
        let tie = g(p, ix, seq(i, SQ_TIE + k as u32)) >= 0.5;
        p.ui.painter().rect_filled(tr, 2.0, if tie { BLUE } else { rgb(0x2a2e35) });
        if p.ui.interact(tr, egui::Id::unique(("sq-tie", p.t, i, k)), Sense::click()).on_hover_text("Tie").clicked() {
            p.put(seq(i, SQ_TIE + k as u32), if tie { 0.0 } else { 1.0 });
        }
    }
    // scroll for steps beyond the view
    if total > cols {
        let sb = p.rect(dx, y + h - 38.0, dw, 8.0);
        p.ui.painter().rect_filled(sb, 4.0, rgb(0x15181d));
        let th = Rect::from_min_size(pos2(sb.min.x + first as f32 / total as f32 * sb.width(), sb.min.y), vec2(cols as f32 / total as f32 * sb.width(), 8.0));
        p.ui.painter().rect_filled(th, 4.0, rgb(0x4a4f58));
        let resp = p.ui.interact(sb, egui::Id::unique(("sq-sb", p.t)), Sense::click_and_drag());
        if let Some(q) = resp.interact_pointer_pos()
            && (resp.dragged() || resp.clicked())
        {
            st.seq_scroll = (((q.x - sb.min.x) / sb.width() * total as f32) as usize).saturating_sub(cols / 2).min(total - cols);
        }
    }
}

// ---------------------------------------------------------------- envelope follower

fn envf_panel(p: &mut Pane, ix: &Ix, st: &mut St, live: &Live, x: f32, y: f32, w: f32) {
    let Some(i) = panel_head(p, ix, st, K_ENVF, x + 18.0, y + 2.0) else { return };
    p.menu(x + 18.0, y + 40.0, 130.0, 20.0, envf(i, EF_SRC));
    small_label(p, x + 18.0, y + 70.0, "Source");
    let dx = x + 180.0;
    let r = p.rect(dx, y, w - 190.0, 116.0);
    display_bg(p, r);
    // the input dimmed, the follower's output bright (only while this panel shows)
    let src = gi(p, ix, envf(i, EF_SRC)).min(8);
    let scale = g(p, ix, envf(i, EF_SCALE));
    st.hist.push((live.levels[src].min(1.0), (live.envf[i] * scale).min(1.0)));
    if st.hist.len() > 240 {
        st.hist.remove(0);
    }
    let n = st.hist.len().max(2) as f32;
    let a: Vec<(f32, f32)> = st.hist.iter().enumerate().map(|(k, v)| (k as f32 / (n - 1.0), v.0 * 0.95)).collect();
    let b: Vec<(f32, f32)> = st.hist.iter().enumerate().map(|(k, v)| (k as f32 / (n - 1.0), v.1 * 0.95)).collect();
    p.curve(r, &a, ORANGE.gamma_multiply(0.35));
    p.curve(r, &b, ORANGE);
    p.ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
    let ky = y + 124.0;
    for (k, (f, n)) in [(EF_ATTACK, "Attack"), (EF_RELEASE, "Release"), (EF_SCALE, "Scale")].iter().enumerate() {
        pknob(p, ix, st, dx + 22.0 + k as f32 * 50.0, ky, envf(i, *f), n);
    }
}

// ---------------------------------------------------------------- ModMap

fn mmap_panel(p: &mut Pane, ix: &Ix, st: &mut St, x: f32, y: f32, w: f32, h: f32) {
    let Some(i) = panel_head(p, ix, st, K_MMAP, x + 18.0, y + 2.0) else {
        if flat(p, p.rect(x + 18.0, y + 40.0, 80.0, 20.0), "New ModMap", false, true, "mm-new")
            && let Some(i) = new_instance(p, ix, K_MMAP)
        {
            st.cur[5] = i;
        }
        return;
    };
    menu_arrows(p, ix, x + 18.0, y + 40.0, 40.0, mmap(i, MM_SNAPX));
    small_label(p, x + 18.0, y + 70.0, "Snap X");
    menu_arrows(p, ix, x + 18.0, y + 84.0, 40.0, mmap(i, MM_SNAPY));
    small_label(p, x + 18.0, y + 114.0, "Snap Y");
    let sx = gi(p, ix, mmap(i, MM_SNAPX));
    let key = sx == SNAPS_X.len() - 1;
    let snap_x = if key { 127 } else { SNAP_N[sx.min(SNAP_N.len() - 1)] };
    let snap_y = SNAP_N[gi(p, ix, mmap(i, MM_SNAPY)).min(SNAP_N.len() - 1)];
    let dx = x + 180.0;
    let side = (h - 10.0).min(w - 190.0);
    let r = p.rect(dx, y, w - 190.0, side);
    display_bg(p, r);
    let plot = r.shrink(8.0);
    let pc = p.ui.painter().with_clip_rect(r);
    if key {
        // the keyboard layout: one column per note, black keys darker
        for k in 0..128 {
            let x0 = plot.min.x + k as f32 / 128.0 * plot.width();
            let black = [1, 3, 6, 8, 10].contains(&(k % 12));
            pc.rect_filled(Rect::from_min_max(pos2(x0, plot.max.y - 14.0), pos2(x0 + plot.width() / 128.0, plot.max.y)), 0.0, if black { rgb(0x2a2d33) } else { rgb(0x9a9ca0) });
        }
    }
    for k in 1..4 {
        let f = k as f32 / 4.0;
        pc.line_segment([pos2(plot.min.x + f * plot.width(), plot.min.y), pos2(plot.min.x + f * plot.width(), plot.max.y)], Stroke::new(1.0, rgb(0x1c2026)));
        pc.line_segment([pos2(plot.min.x, plot.max.y - f * plot.height()), pos2(plot.max.x, plot.max.y - f * plot.height())], Stroke::new(1.0, rgb(0x1c2026)));
    }
    let n = gi(p, ix, mmap(i, MM_N)).clamp(2, MMAP_PTS);
    let pts: Vec<(f32, f32, f32)> = (0..n).map(|k| {
        let b = MM_PT + k as u32 * 3;
        (if k == 0 { 0.0 } else if k == n - 1 { 1.0 } else { g(p, ix, mmap(i, b)) }, g(p, ix, mmap(i, b + 1)), g(p, ix, mmap(i, b + 2)))
    }).collect();
    let to = |x: f32, y: f32| pos2(plot.min.x + x * plot.width(), plot.max.y - y * plot.height());
    let mut line = Vec::new();
    for k in 0..n - 1 {
        for s in 0..=16 {
            let f = s as f32 / 16.0;
            line.push(to(pts[k].0 + (pts[k + 1].0 - pts[k].0) * f, seg(pts[k].1, pts[k + 1].1, f, pts[k].2)));
        }
    }
    pc.add(egui::Shape::line(line.clone(), Stroke::new(1.5, ORANGE)));
    let fine = p.ui.input(|i| i.modifiers.shift);
    let mut on_point = false;
    for k in 0..n {
        let c = to(pts[k].0, pts[k].1);
        pc.circle_filled(c, 4.5, ORANGE);
        let resp = p.ui.interact(Rect::from_center_size(c, vec2(12.0, 12.0)), egui::Id::unique(("mm-pt", p.t, i, k)), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            on_point = true;
        }
        if resp.double_clicked() && k > 0 && k < n - 1 {
            for j in k..n - 1 {
                for o in 0..3 {
                    p.put(mmap(i, MM_PT + j as u32 * 3 + o), [pts[j + 1].0, pts[j + 1].1, pts[j + 1].2][o as usize]);
                }
            }
            p.put(mmap(i, MM_N), (n - 1) as f32);
            continue;
        }
        if resp.dragged()
            && let Some(q) = resp.interact_pointer_pos()
        {
            let mut fx = ((q.x - plot.min.x) / plot.width()).clamp(0.0, 1.0);
            let mut fy = ((plot.max.y - q.y) / plot.height()).clamp(0.0, 1.0);
            if !fine {
                fx = snap_to(fx, snap_x);
                fy = snap_to(fy, snap_y);
            }
            let b = MM_PT + k as u32 * 3;
            if k > 0 && k < n - 1 {
                p.put(mmap(i, b), fx.clamp(pts[k - 1].0, pts[k + 1].0));
            }
            p.put(mmap(i, b + 1), fy);
        }
        if resp.dragged() || resp.hovered() {
            pc.text(c + vec2(8.0, -8.0), Align2::LEFT_BOTTOM, format!("{:.2}, {:.2}", pts[k].0, pts[k].1), look::regular(10.0), Color32::WHITE);
        }
    }
    if !on_point {
        let resp = p.ui.interact(plot, egui::Id::unique(("mm-bg", p.t, i)), Sense::click());
        if resp.clicked()
            && n < MMAP_PTS
            && let Some(q) = resp.interact_pointer_pos()
            && line.iter().any(|l| l.distance(q) < 8.0)
        {
            let mut fx = ((q.x - plot.min.x) / plot.width()).clamp(0.0, 1.0);
            let mut fy = ((plot.max.y - q.y) / plot.height()).clamp(0.0, 1.0);
            if !fine {
                fx = snap_to(fx, snap_x);
                fy = snap_to(fy, snap_y);
            }
            let at = (0..n - 1).find(|&k| fx >= pts[k].0 && fx <= pts[k + 1].0).map(|k| k + 1).unwrap_or(n - 1);
            for j in (at..n).rev() {
                for o in 0..3 {
                    p.put(mmap(i, MM_PT + (j + 1) as u32 * 3 + o), [pts[j].0, pts[j].1, pts[j].2][o as usize]);
                }
            }
            let b = MM_PT + at as u32 * 3;
            p.put(mmap(i, b), fx);
            p.put(mmap(i, b + 1), fy);
            p.put(mmap(i, b + 2), 0.0);
            p.put(mmap(i, MM_N), (n + 1) as f32);
        }
    }
    let _ = am::N;
}
