//! Alchemy's source edit window, main page, laid out as Logic's (it covers the window below the
//! Name bar): the inspector on the left — SOURCE (A B C D, On, the source's audio name with ‹ ›,
//! Solo), GROUP (+ Empty / Import…, the group list with Trig / Fade / Poly, double-click to rename
//! and edit, Control-click Delete; the Starts rules with their condition menus and the Keysw
//! knob), ZONE (+, Select MIDI, the zone pop-up, Key + Learn, Loop Mode, Vol / Tune / Pan, Lo / Hi
//! Key and Vel, Zone Fade, Left / Right / Bottom / Top) — and on the right the Main / Additive /
//! Spectral buttons, the close button, the keymap editor (zones by key and velocity, edge drags,
//! the zone shortcut menu, the audition keyboard, scroll bar with zoom ends) and the zone waveform
//! editor (S / E, loop markers and region, Loop XFade with its XF handle, the play position, the
//! time line, scroll bar with zoom ends).

use std::sync::Mutex;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_synth::alchemy_zones::{self as az, Cond, Ctrl, Ks, Rule, Zone, ZoneDoc};

use crate::alchemyrack::{self as rack, g, Ix, St, BLUE, ORANGE};
use crate::look;
use crate::synthui::{rgb, Pane};

/// Figure scale: Logic's edit window drawn at 1068 × 640 covers 1146 × 692 points.
const K: f32 = 1.073;
const INSP: Color32 = rgb(0x2b2f36);
const RIGHT: Color32 = rgb(0x25292f);
const KEYMAP: Color32 = rgb(0x2d4660);
const WAVE_BG: Color32 = rgb(0x0f1822);
const WAVE: Color32 = rgb(0x6c8db6);
const LINE: Color32 = rgb(0x3b3f47);

static NOTES: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Notes played (Musical Typing, MIDI) for Select MIDI and Learn.
pub fn note_played(pitch: u8) {
    if let Ok(mut n) = NOTES.lock() {
        if n.len() < 64 {
            n.push(pitch);
        }
    }
}

pub fn take_notes() -> Vec<u8> {
    NOTES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// A keymap zone edge (0 Lo Key, 1 Hi Key, 2 Lo Vel, 3 Hi Vel) and the steps applied so far.
    Edge(u8, i32),
    /// A waveform marker: 0 S, 1 E, 2 loop start, 3 loop end, 4 XF; the view before the drag.
    Marker(u8, (f32, f32)),
    /// The scroll bar thumbs (0 keymap, 1 waveform): the pointer's offset in the thumb.
    Thumb(u8, f32),
    Key,
}

/// The edit window's own state.
#[derive(Clone, Default)]
pub struct Ed {
    pub open: bool,
    pub src: usize,
    /// 0 Main, 1 Additive, 2 Spectral.
    pub page: u8,
    pub group: usize,
    pub sel: Vec<(usize, usize)>,
    pub clip: Vec<Zone>,
    pub learn: bool,
    pub select_midi: bool,
    pub preview: bool,
    /// The keymap's visible keys (lo, hi) and the waveform's visible part (0–1); zero: all.
    pub km: (f32, f32),
    pub wf: (f32, f32),
    pub drag: Option<Drag>,
    /// The doc changed this frame (the window stores it, with an undo step).
    pub dirty: bool,
    pub rename: Option<(usize, String)>,
}

impl Ed {
    pub fn focus(&self) -> Option<(usize, usize)> {
        self.sel.last().copied()
    }
    fn km(&self) -> (f32, f32) {
        if self.km.1 > self.km.0 { self.km } else { (0.0, 128.0) }
    }
    fn wf(&self) -> (f32, f32) {
        if self.wf.1 > self.wf.0 { self.wf } else { (0.0, 1.0) }
    }
}

fn fr(p: &Pane, x: f32, y: f32, w: f32, h: f32) -> Rect {
    p.rect(x * K, 46.0 + (y - 150.0) * K, w * K, h * K)
}

fn ft(p: &Pane, x: f32, y: f32, align: Align2, s: &str, size: f32, col: Color32) {
    p.text(x * K, 46.0 + (y - 150.0) * K, align, s, size, col);
}

fn label(p: &Pane, x: f32, y: f32, s: &str) {
    ft(p, x, y, Align2::LEFT_CENTER, s, 9.5, p.skin.dim);
}

fn head(p: &Pane, x: f32, y: f32, s: &str) {
    ft(p, x, y, Align2::LEFT_CENTER, s, 11.5, p.skin.dim);
}

/// A number field with a stepper: drag up / down, the stepper halves, or the wheel.
fn num(p: &mut Pane, r: Rect, v: i32, lo: i32, hi: i32, text: &str, salt: impl std::hash::Hash + std::fmt::Debug + Copy) -> Option<i32> {
    let id = egui::Id::unique(("alch-ed-num", p.t, salt));
    let resp = p.ui.interact(r, id, Sense::click_and_drag());
    let pt = p.ui.painter();
    pt.rect_filled(r, 3.0, rack::FIELD);
    let sr = Rect::from_min_max(pos2(r.max.x - 12.0, r.min.y), r.max);
    pt.with_clip_rect(r).text(pos2((r.min.x + sr.min.x) / 2.0, r.center().y), Align2::CENTER_CENTER, text, look::regular(11.0), p.skin.text);
    crate::glyphs::paint(pt, Rect::from_center_size(pos2(sr.center().x, r.center().y - 3.5), vec2(8.0, 7.0)), crate::glyphs::ph::CARET_UP, 7.5, p.skin.dim);
    crate::glyphs::paint(pt, Rect::from_center_size(pos2(sr.center().x, r.center().y + 3.5), vec2(8.0, 7.0)), crate::glyphs::ph::CARET_DOWN, 7.5, p.skin.dim);
    let acc = egui::Id::unique(("alch-ed-num-acc", p.t, salt));
    let mut nv = v;
    if resp.drag_started() {
        p.ui.data_mut(|d| d.insert_temp(acc, 0.0f32));
    }
    if resp.dragged() {
        let a: f32 = p.ui.data(|d| d.get_temp(acc)).unwrap_or(0.0) - resp.drag_delta().y / 4.0;
        let steps = a.trunc() as i32;
        p.ui.data_mut(|d| d.insert_temp(acc, a - steps as f32));
        nv += steps;
    } else if resp.clicked()
        && let Some(pp) = resp.interact_pointer_pos()
        && sr.contains(pp)
    {
        nv += if pp.y < r.center().y { 1 } else { -1 };
    }
    if resp.hovered() {
        let sc = p.ui.input(|i| i.smooth_scroll_delta.y);
        if sc.abs() > 0.5 {
            nv += sc.signum() as i32;
        }
    }
    let nv = nv.clamp(lo, hi);
    (nv != v).then_some(nv)
}

/// A pop-up field listing `items`; returns the chosen index.
fn pick(p: &mut Pane, r: Rect, cur: &str, items: &[String], salt: impl std::hash::Hash + std::fmt::Debug + Copy) -> Option<usize> {
    let resp = rack::field_btn(p, r, cur, ("ed-pick", salt), true);
    let mut out = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for (k, it) in items.iter().enumerate() {
                if ui.selectable_label(it == cur, it).clicked() {
                    out = Some(k);
                    ui.close();
                }
            }
        });
    });
    out
}

fn small_btn(p: &mut Pane, r: Rect, label: &str, lit: bool, salt: impl std::hash::Hash + std::fmt::Debug) -> bool {
    rack::flat(p, r, label, lit, true, ("ed-btn", salt))
}

fn plus(p: &mut Pane, r: Rect, salt: impl std::hash::Hash + std::fmt::Debug) -> egui::Response {
    let resp = p.ui.interact(r, egui::Id::unique(("alch-ed-plus", p.t, salt)), Sense::click());
    p.ui.painter().rect_filled(r, 3.0, if resp.hovered() { rgb(0x4a4f58) } else { rack::BTN });
    crate::glyphs::paint(p.ui.painter(), r, crate::glyphs::ph::PLUS, 11.0, p.skin.text);
    resp
}

/// The source's name in the inspector: its first zone's sample, else "No Audio".
pub fn source_name(doc: &ZoneDoc, s: usize) -> String {
    doc.src[s].first_zone().map(|z| z.name.clone()).unwrap_or_else(|| "No Audio".into())
}

fn sel_valid(ed: &mut Ed, doc: &ZoneDoc) {
    let src = &doc.src[ed.src];
    ed.sel.retain(|&(g, z)| src.zone(g, z).is_some());
    if ed.group >= src.groups.len() {
        ed.group = src.groups.len().saturating_sub(1);
    }
}

/// The whole window below the Name bar.
pub fn body(p: &mut Pane, ix: &Ix, st: &mut St, ed: &mut Ed, doc: &mut ZoneDoc, pos: Option<(usize, usize, f64)>, notes: &[u8]) {
    sel_valid(ed, doc);
    // notes played: Select MIDI picks the zones they play, Learn takes the root key
    for &n in notes {
        if ed.learn {
            for &(g, z) in &ed.sel {
                if let Some(zn) = doc.src[ed.src].zone_mut(g, z) {
                    zn.key = n;
                }
            }
            ed.learn = false;
            ed.dirty = true;
        }
        if ed.select_midi {
            let hit = doc.src[ed.src].zones_at(n, 100);
            if !hit.is_empty() {
                ed.group = hit[0].0;
                ed.sel = hit;
            }
        }
    }
    let full = fr(p, 0.0, 150.0, 1068.0, 640.0);
    p.ui.painter().rect_filled(full, 0.0, RIGHT);
    p.ui.painter().rect_filled(fr(p, 0.0, 150.0, 228.0, 640.0), 0.0, INSP);
    // a click anywhere here stays here (not on the pages below)
    p.ui.interact(full, egui::Id::unique(("alch-ed-bg", p.t)), Sense::click());
    inspector_source(p, ix, ed, doc);
    inspector_group(p, ix, st, ed, doc);
    inspector_zone(p, ix, ed, doc);
    // Main / Additive / Spectral and the close button
    for (k, n) in ["Main", "Additive", "Spectral"].iter().enumerate() {
        if small_btn(p, fr(p, 243.0 + k as f32 * 75.0, 167.0, 74.0, 17.0), n, ed.page == k as u8, ("page", k)) {
            ed.page = k as u8;
        }
    }
    let xr = fr(p, 1036.0, 162.0, 20.0, 20.0);
    let xresp = p.ui.interact(xr, egui::Id::unique(("alch-ed-close", p.t)), Sense::click());
    crate::glyphs::paint(p.ui.painter(), xr, crate::glyphs::ph::X, 15.0, if xresp.hovered() { Color32::WHITE } else { p.skin.text });
    if xresp.clicked() {
        ed.open = false;
        p.actions.push("zaud-stop".into());
    }
    if ed.page != 0 {
        let r = fr(p, 244.0, 197.0, 810.0, 579.0);
        p.ui.painter().rect_filled(r, 2.0, WAVE_BG);
        p.ui.painter().text(r.center(), Align2::CENTER_CENTER, if ed.page == 1 { "Additive data: no partials — import audio in Additive mode" } else { "Spectral data: no spectrum — import audio in Spectral mode" }, look::regular(12.0), p.skin.dim);
        return;
    }
    keymap(p, ed, doc);
    waveform(p, ix, ed, doc, pos);
}

fn inspector_source(p: &mut Pane, ix: &Ix, ed: &mut Ed, doc: &ZoneDoc) {
    head(p, 17.0, 173.0, "SOURCE");
    for k in 0..4 {
        if small_btn(p, fr(p, 138.0 + k as f32 * 18.5, 165.0, 17.0, 17.0), ["A", "B", "C", "D"][k], ed.src == k, ("src", k)) && ed.src != k {
            ed.src = k;
            ed.sel.clear();
            ed.group = 0;
        }
    }
    let s = ed.src as u32;
    let on = g(p, ix, 277 + s) >= 0.5;
    if small_btn(p, fr(p, 16.0, 198.0, 26.0, 17.0), "On", on, "on") {
        p.put(277 + s, if on { 0.0 } else { 1.0 });
    }
    let name = source_name(doc, ed.src);
    let r = fr(p, 46.0, 198.0, 122.0, 17.0);
    let resp = rack::field_btn(p, r, &name, ("ed-src", s), false);
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if ui.button("Import Audio…").clicked() {
            p.actions.push(format!("zgimport:{s}"));
            ui.close();
        }
        if ui.button("Clear").clicked() {
            p.actions.push(format!("zclear:{s}"));
            ui.close();
        }
    });
    let d = rack::arrows(p, fr(p, 170.0, 198.0, 22.0, 17.0), ("ed-step", s));
    if d != 0 {
        p.actions.push(format!("zstep:{s}:{d}"));
    }
    let solo = crate::alchemywin::soloed(p) == Some(ed.src + 1);
    if small_btn(p, fr(p, 196.0, 198.0, 16.0, 17.0), "S", solo, "solo") {
        crate::alchemywin::solo(p, ed.src + 1);
    }
    p.ui.painter().hline(fr(p, 0.0, 222.0, 228.0, 1.0).x_range(), fr(p, 0.0, 222.0, 1.0, 1.0).min.y, Stroke::new(1.0, LINE));
}

fn inspector_group(p: &mut Pane, ix: &Ix, st: &mut St, ed: &mut Ed, doc: &mut ZoneDoc) {
    let s = ed.src;
    head(p, 17.0, 242.0, "GROUP");
    let presp = plus(p, fr(p, 66.0, 234.0, 16.0, 16.0), ("grp-plus", s));
    egui::Popup::menu(&presp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        if ui.button("Empty").clicked() {
            ed.group = doc.src[s].add_empty();
            ed.dirty = true;
            ui.close();
        }
        if ui.button("Import…").clicked() {
            p.actions.push(format!("zgimport:{s}"));
            ui.close();
        }
    });
    for (x, t) in [(114.0, "Trig"), (160.0, "Fade"), (190.0, "Poly")] {
        ft(p, x, 255.0, Align2::LEFT_CENTER, t, 9.0, p.skin.dim);
    }
    let list = fr(p, 16.0, 262.0, 196.0, 90.0);
    p.ui.painter().rect_filled(list, 0.0, rgb(0x1f2228));
    let row_h = 18.0 * K;
    let ng = doc.src[s].groups.len();
    let mut delete: Option<usize> = None;
    for gi in 0..ng.min(5) {
        let r = Rect::from_min_size(list.min + vec2(0.0, gi as f32 * row_h), vec2(list.width(), row_h));
        let resp = p.ui.interact(r, egui::Id::unique(("alch-ed-grow", p.t, s, gi)), Sense::click());
        let sel = ed.group == gi;
        if sel {
            p.ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), 0.0, BLUE);
        }
        let gr = &doc.src[s].groups[gi];
        let editing = ed.rename.as_ref().is_some_and(|x| x.0 == gi);
        let tx = |p: &Pane, x: f32, t: &str| p.ui.painter().with_clip_rect(r).text(pos2(list.min.x + x * K, r.center().y), Align2::LEFT_CENTER, t, look::regular(10.5), Color32::WHITE);
        if !editing {
            tx(p, 4.0, &gr.name);
            tx(p, 96.0, az::TRIGGERS[gr.trigger as usize & 1]);
            tx(p, 146.0, &format!("{}", gr.fade.round()));
            tx(p, 176.0, &if gr.poly == 0 { "-".to_string() } else { gr.poly.to_string() });
        }
        if resp.clicked() {
            ed.group = gi;
        }
        if resp.double_clicked() {
            ed.rename = Some((gi, gr.name.clone()));
        }
        resp.context_menu(|ui| {
            if ui.button("Delete").clicked() {
                delete = Some(gi);
                ui.close();
            }
        });
        if editing {
            // rename, Trigger, Fade, Poly
            let mut name = ed.rename.as_ref().map(|x| x.1.clone()).unwrap_or_default();
            let nr = Rect::from_min_size(r.min + vec2(2.0, 1.0), vec2(88.0 * K, row_h - 2.0));
            let te = p.ui.put(nr, egui::TextEdit::singleline(&mut name).font(look::regular(10.5)));
            if te.changed() {
                ed.rename = Some((gi, name.clone()));
            }
            let tr = Rect::from_min_size(pos2(list.min.x + 92.0 * K, r.min.y + 1.0), vec2(48.0 * K, row_h - 2.0));
            let cur = az::TRIGGERS[doc.src[s].groups[gi].trigger as usize & 1].to_string();
            if let Some(k) = pick(p, tr, &cur, &az::TRIGGERS.iter().map(|x| x.to_string()).collect::<Vec<_>>(), ("trig", s, gi)) {
                doc.src[s].groups[gi].trigger = k as u8;
                ed.dirty = true;
            }
            let fd = doc.src[s].groups[gi].fade.round() as i32;
            if let Some(v) = num(p, Rect::from_min_size(pos2(list.min.x + 142.0 * K, r.min.y + 1.0), vec2(28.0 * K, row_h - 2.0)), fd, 0, 100, &fd.to_string(), ("fade", s, gi)) {
                doc.src[s].groups[gi].fade = v as f32;
                ed.dirty = true;
            }
            let po = doc.src[s].groups[gi].poly as i32;
            if let Some(v) = num(p, Rect::from_min_size(pos2(list.min.x + 172.0 * K, r.min.y + 1.0), vec2(24.0 * K, row_h - 2.0)), po, 0, az::POLY_MAX as i32, &if po == 0 { "-".to_string() } else { po.to_string() }, ("poly", s, gi)) {
                doc.src[s].groups[gi].poly = v as u8;
                ed.dirty = true;
            }
            let done = te.lost_focus() || p.ui.input(|i| i.key_pressed(egui::Key::Enter));
            if done || (p.ui.input(|i| i.pointer.any_pressed()) && !p.ui.rect_contains_pointer(r)) {
                if let Some((_, n)) = ed.rename.take()
                    && !n.trim().is_empty()
                    && n != doc.src[s].groups[gi].name
                {
                    doc.src[s].groups[gi].name = n;
                    ed.dirty = true;
                }
            }
        }
    }
    if let Some(gi) = delete {
        doc.src[s].groups.remove(gi);
        ed.sel.clear();
        ed.group = ed.group.min(doc.src[s].groups.len().saturating_sub(1));
        ed.dirty = true;
    }
    // Starts
    label(p, 17.0, 369.0, "Starts");
    let box_r = fr(p, 16.0, 376.0, 196.0, 129.0);
    p.ui.painter().rect_filled(box_r, 0.0, rgb(0x1f2228));
    if ng == 0 {
        return;
    }
    let gi = ed.group.min(ng - 1);
    let rules = doc.src[s].groups[gi].rules.clone();
    let rule_names: Vec<String> = az::RULES.iter().map(|x| x.to_string()).collect();
    let mut y = 381.0;
    for (i, line) in rules.iter().enumerate() {
        if let Some(m) = pick(p, fr(p, 20.0, y, 76.0, 16.0), az::RULES[line.rule.menu_index()], &rule_names, ("rule", s, gi, i)) {
            doc.src[s].set_rule(gi, i, m);
            ed.dirty = true;
            break;
        }
        match line.rule {
            Rule::RoundRobin(o) => {
                if let Some(v) = num(p, fr(p, 100.0, y, 36.0, 16.0), o, 1, 99, &o.to_string(), ("order", s, gi, i)) {
                    doc.src[s].groups[gi].rules[i].rule = Rule::RoundRobin(v);
                    ed.dirty = true;
                }
            }
            Rule::Keyswitch(a, b) => {
                let all = Ks::all();
                let names: Vec<String> = all.iter().map(|k| k.name()).collect();
                if let Some(k) = pick(p, fr(p, 98.0, y, 34.0, 16.0), &a.name(), &names, ("ks1", s, gi, i)) {
                    doc.src[s].set_ks_first(gi, i, all[k]);
                    ed.dirty = true;
                }
                // the second field offers values of the first one's kind
                let same: Vec<Ks> = all.iter().copied().filter(|k| std::mem::discriminant(k) == std::mem::discriminant(&a)).collect();
                let names: Vec<String> = same.iter().map(|k| k.name()).collect();
                if let Some(k) = pick(p, fr(p, 134.0, y, 34.0, 16.0), &b.name(), &names, ("ks2", s, gi, i)) {
                    doc.src[s].groups[gi].rules[i].rule = Rule::Keyswitch(a, same[k]);
                    ed.dirty = true;
                }
            }
            Rule::Controller(c, lo, hi) => {
                let all = Ctrl::all();
                let names: Vec<String> = all.iter().map(|k| k.name()).collect();
                if let Some(k) = pick(p, fr(p, 98.0, y, 30.0, 16.0), &c.name(), &names, ("ctl", s, gi, i)) {
                    doc.src[s].groups[gi].rules[i].rule = Rule::Controller(all[k], lo, hi);
                    ed.dirty = true;
                }
                if let Some(v) = num(p, fr(p, 129.0, y, 20.0, 16.0), lo as i32, 0, 100, &format!("{}", lo as i32), ("clo", s, gi, i)) {
                    doc.src[s].groups[gi].rules[i].rule = Rule::Controller(c, v as f32, hi);
                    ed.dirty = true;
                }
                if let Some(v) = num(p, fr(p, 150.0, y, 20.0, 16.0), hi as i32, 0, 100, &format!("{}", hi as i32), ("chi", s, gi, i)) {
                    doc.src[s].groups[gi].rules[i].rule = Rule::Controller(c, lo, v as f32);
                    ed.dirty = true;
                }
            }
            _ => {}
        }
        let conds: Vec<String> = az::CONDS.iter().map(|x| x.to_string()).collect();
        let ci = match line.cond {
            Cond::And => 0,
            Cond::Or => 1,
            Cond::Not => 2,
        };
        if let Some(k) = pick(p, fr(p, 172.0, y, 38.0, 16.0), az::CONDS[ci], &conds, ("cond", s, gi, i)) {
            doc.src[s].groups[gi].rules[i].cond = [Cond::And, Cond::Or, Cond::Not][k];
            ed.dirty = true;
        }
        y += 19.0;
        if y > 470.0 {
            break;
        }
    }
    // an empty rule menu below adds a rule
    if y <= 470.0
        && let Some(m) = pick(p, fr(p, 20.0, y, 76.0, 16.0), "", &rule_names[..5], ("rule-new", s, gi))
    {
        doc.src[s].set_rule(gi, rules.len(), m);
        ed.dirty = true;
    }
    // the source Keyswitch knob, when a rule uses KEYSW values
    if doc.src[s].groups.iter().any(|g| g.uses_knob()) {
        rack::knob(p, ix, st, 190.0 * K, 46.0 + (468.0 - 150.0) * K, 22.0, az::KEYSW + s as u32, "Keysw");
    }
    p.ui.painter().hline(fr(p, 0.0, 515.0, 228.0, 1.0).x_range(), fr(p, 0.0, 515.0, 1.0, 1.0).min.y, Stroke::new(1.0, LINE));
}

fn note_name(n: u8) -> String {
    lg_synth::sampler_doc::note_name(n)
}

fn inspector_zone(p: &mut Pane, ix: &Ix, ed: &mut Ed, doc: &mut ZoneDoc) {
    let s = ed.src;
    head(p, 17.0, 535.0, "ZONE");
    if plus(p, fr(p, 66.0, 527.0, 16.0, 16.0), ("zone-plus", s)).clicked() {
        p.actions.push(format!("zadd:{s}:{}", ed.group));
    }
    if small_btn(p, fr(p, 138.0, 527.0, 74.0, 16.0), "Select MIDI", ed.select_midi, "selmidi") {
        ed.select_midi = !ed.select_midi;
    }
    // the zone pop-up: every zone of the source
    let all: Vec<(usize, usize)> = doc.src[s].groups.iter().enumerate().flat_map(|(g, gr)| (0..gr.zones.len()).map(move |z| (g, z))).collect();
    let names: Vec<String> = all.iter().map(|&(g, z)| format!("{} — {}", doc.src[s].groups[g].name, doc.src[s].groups[g].zones[z].name)).collect();
    let cur = match ed.sel.len() {
        0 => String::new(),
        1 => doc.src[s].zone(ed.sel[0].0, ed.sel[0].1).map(|z| z.name.clone()).unwrap_or_default(),
        _ => "Multiple".to_string(),
    };
    if let Some(k) = pick(p, fr(p, 16.0, 557.0, 196.0, 16.0), &cur, &names, ("zones", s)) {
        ed.sel = vec![all[k]];
        ed.group = all[k].0;
        if ed.preview {
            p.actions.push(format!("zaud-zone:{s}:{}:{}", all[k].0, all[k].1));
        }
    }
    let Some((fg, fz)) = ed.focus() else {
        label(p, 17.0, 600.0, "No zone selected");
        return;
    };
    let Some(z0) = doc.src[s].zone(fg, fz).cloned() else { return };
    let sel = ed.sel.clone();
    let each = |doc: &mut ZoneDoc, f: &dyn Fn(&mut Zone)| {
        for &(g, z) in &sel {
            if let Some(zn) = doc.src[s].zone_mut(g, z) {
                f(zn);
            }
        }
    };
    label(p, 17.0, 587.0, "Key");
    label(p, 120.0, 587.0, "Loop Mode");
    if let Some(v) = num(p, fr(p, 16.0, 594.0, 40.0, 16.0), z0.key as i32, 0, 127, &note_name(z0.key), ("key", s)) {
        each(doc, &|z| z.key = v as u8);
        ed.dirty = true;
    }
    if small_btn(p, fr(p, 61.0, 594.0, 50.0, 16.0), "Learn", ed.learn, "learn") {
        ed.learn = !ed.learn;
    }
    p.menu(118.0 * K, 46.0 + (594.0 - 150.0) * K, 94.0 * K, 16.0 * K, az::LOOP_MODE + s as u32);
    let _ = ix;
    // Vol, Tune, Pan
    let knob = |p: &mut Pane, cx: f32, v: &mut f32, lo: f32, hi: f32, def: f32, l: &str, shown: &dyn Fn(f32) -> String, salt: &str| -> bool {
        let r = Rect::from_center_size(pos2(p.o.x + cx * K, p.o.y + 46.0 + (655.0 - 150.0) * K), vec2(40.0, 58.0));
        crate::widgets::knob_ex(p.ui, r, egui::Id::unique(("alch-ed-knob", p.t, s, salt)), v, lo, hi, def, l, shown, 16.0)
    };
    let mut vol = z0.vol_db;
    if knob(p, 58.0, &mut vol, -48.0, 12.0, 0.0, "Vol", &|v| format!("{v:.1} dB"), "vol") {
        each(doc, &|z| z.vol_db = vol);
        ed.dirty = true;
    }
    let mut tune = z0.coarse + z0.fine / 100.0;
    if knob(p, 114.0, &mut tune, -24.0, 24.0, 0.0, "Tune", &|v| {
        let c = v.round();
        format!("{c:+} st {:+.0} c", (v - c) * 100.0)
    }, "tune")
    {
        let c = tune.round();
        let f = ((tune - c) * 100.0).round();
        each(doc, &|z| {
            z.coarse = c;
            z.fine = f;
        });
        ed.dirty = true;
    }
    let mut pan = z0.pan;
    if knob(p, 170.0, &mut pan, -1.0, 1.0, 0.0, "Pan", &|v| format!("{:+.0}", v * 64.0), "pan") {
        each(doc, &|z| z.pan = pan);
        ed.dirty = true;
    }
    for (k, l) in ["Lo Key", "Hi Key", "Lo Vel", "Hi Vel"].iter().enumerate() {
        label(p, 17.0 + k as f32 * 52.0, 691.0, l);
    }
    let fields = [
        (z0.lo_key as i32, 0, z0.hi_key as i32, note_name(z0.lo_key)),
        (z0.hi_key as i32, z0.lo_key as i32, 127, note_name(z0.hi_key)),
        (z0.lo_vel as i32, 1, z0.hi_vel as i32, z0.lo_vel.to_string()),
        (z0.hi_vel as i32, z0.lo_vel as i32, 127, z0.hi_vel.to_string()),
    ];
    for (k, (v, lo, hi, t)) in fields.iter().enumerate() {
        if let Some(nv) = num(p, fr(p, 16.0 + k as f32 * 52.0, 699.0, 40.0, 17.0), *v, *lo, *hi, t, ("range", s, k)) {
            let d = nv - v;
            doc.src[s].move_edge(&sel, k as u8, d);
            ed.dirty = true;
        }
    }
    label(p, 17.0, 736.0, "Zone Fade");
    let fades: Vec<String> = az::ZONE_FADES.iter().map(|x| x.to_string()).collect();
    if let Some(k) = pick(p, fr(p, 68.0, 728.0, 92.0, 16.0), az::ZONE_FADES[z0.curve as usize & 1], &fades, ("zfade", s)) {
        each(doc, &|z| z.curve = k as u8);
        ed.dirty = true;
    }
    let fv = [z0.left, z0.right, z0.bottom, z0.top];
    for (k, l) in ["Left", "Right", "Bottom", "Top"].iter().enumerate() {
        label(p, 17.0 + k as f32 * 52.0, 757.0, l);
        let max = if k < 2 { 127 } else { 126 };
        if let Some(v) = num(p, fr(p, 16.0 + k as f32 * 52.0, 764.0, 40.0, 17.0), fv[k] as i32, 0, max, &fv[k].to_string(), ("zf", s, k)) {
            each(doc, &|z| match k {
                0 => z.left = v as u8,
                1 => z.right = v as u8,
                2 => z.bottom = v as u8,
                _ => z.top = v as u8,
            });
            ed.dirty = true;
        }
    }
}

fn is_black(n: u8) -> bool {
    matches!(n % 12, 1 | 3 | 6 | 8 | 10)
}

/// A scroll bar with zoom controls at its ends; returns the new view.
fn scroll(p: &mut Pane, ed: &mut Ed, r: Rect, which: u8, view: (f32, f32), total: f32, min_span: f32) -> (f32, f32) {
    let pt = p.ui.painter().clone();
    pt.rect_filled(r, 2.0, rgb(0x1b1e23));
    let zr = [Rect::from_min_size(r.min, vec2(r.height() + 4.0, r.height())), Rect::from_min_size(pos2(r.max.x - r.height() - 4.0, r.min.y), vec2(r.height() + 4.0, r.height()))];
    let mut v = view;
    let span = v.1 - v.0;
    for (k, zrect) in zr.iter().enumerate() {
        let resp = p.ui.interact(*zrect, egui::Id::unique(("alch-ed-zoom", p.t, which, k)), Sense::click());
        crate::glyphs::paint(&pt, *zrect, if k == 0 { crate::glyphs::ph::MINUS } else { crate::glyphs::ph::PLUS }, 8.0, if resp.hovered() { Color32::WHITE } else { p.skin.dim });
        if resp.clicked() {
            let c = (v.0 + v.1) / 2.0;
            let ns = if k == 0 { (span * 2.0).min(total) } else { (span / 2.0).max(min_span) };
            let lo = (c - ns / 2.0).clamp(0.0, total - ns);
            v = (lo, lo + ns);
        }
    }
    let track = Rect::from_min_max(pos2(zr[0].max.x + 2.0, r.min.y + 1.0), pos2(zr[1].min.x - 2.0, r.max.y - 1.0));
    let x = |t: f32| track.min.x + t / total * track.width();
    let thumb = Rect::from_min_max(pos2(x(v.0), track.min.y), pos2(x(v.1).max(x(v.0) + 6.0), track.max.y));
    pt.rect_filled(thumb, 3.0, rgb(0x5a5f68));
    let resp = p.ui.interact(track, egui::Id::unique(("alch-ed-thumb", p.t, which)), Sense::drag());
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        ed.drag = Some(Drag::Thumb(which, (pp.x - thumb.min.x).clamp(0.0, thumb.width())));
    }
    if resp.dragged()
        && let (Some(Drag::Thumb(w, off)), Some(pp)) = (ed.drag, resp.interact_pointer_pos())
        && w == which
    {
        let sp = v.1 - v.0;
        let lo = ((pp.x - off - track.min.x) / track.width() * total).clamp(0.0, total - sp);
        v = (lo, lo + sp);
    }
    if resp.drag_stopped() {
        ed.drag = None;
    }
    v
}

fn keymap(p: &mut Pane, ed: &mut Ed, doc: &mut ZoneDoc) {
    let s = ed.src;
    let area = fr(p, 244.0, 197.0, 810.0, 233.0);
    let kb = fr(p, 244.0, 430.0, 810.0, 42.0);
    let (lo, hi) = ed.km();
    let kx = |k: f32| area.min.x + (k - lo) / (hi - lo) * area.width();
    let vy = |v: f32| area.max.y - v / 127.0 * area.height();
    let pt = p.ui.painter().clone().with_clip_rect(area.union(kb));
    pt.rect_filled(area, 0.0, KEYMAP);
    for k in lo.floor() as i32..=hi.ceil() as i32 {
        let x = kx(k as f32);
        pt.vline(x, area.y_range(), Stroke::new(1.0, rgb(0x3d5b78)));
    }
    let ng = doc.src[s].groups.len();
    let gi = ed.group.min(ng.saturating_sub(1));
    let zones: Vec<Zone> = doc.src[s].groups.get(gi).map(|g| g.zones.clone()).unwrap_or_default();
    let zr = |z: &Zone| Rect::from_min_max(pos2(kx(z.lo_key as f32), vy(z.hi_vel as f32)), pos2(kx(z.hi_key as f32 + 1.0), vy(z.lo_vel as f32 - 1.0)));
    for (zi, z) in zones.iter().enumerate() {
        let r = zr(z);
        let selected = ed.sel.contains(&(gi, zi));
        pt.rect_filled(r, 0.0, if selected { rgb(0x6f8fb0).gamma_multiply(0.85) } else { rgb(0x4b6a89).gamma_multiply(0.7) });
        pt.rect_stroke(r, 0.0, Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { Color32::WHITE } else { rgb(0x7f9cb9) }), egui::StrokeKind::Inside);
        if r.width() > 40.0 {
            pt.text(r.left_top() + vec2(4.0, 3.0), Align2::LEFT_TOP, &z.name, look::regular(10.0), Color32::from_white_alpha(200));
        }
    }
    let resp = p.ui.interact(area, egui::Id::unique(("alch-ed-keymap", p.t)), Sense::click_and_drag());
    let hover = resp.hover_pos();
    // which edge of a selected zone is under the pointer
    let edge_at = |pp: egui::Pos2| -> Option<u8> {
        for &(g, z) in &ed.sel {
            if g != gi {
                continue;
            }
            let Some(zn) = zones.get(z) else { continue };
            let r = zr(zn).expand(4.0);
            if !r.contains(pp) {
                continue;
            }
            let r = zr(zn);
            if (pp.x - r.min.x).abs() <= 4.0 {
                return Some(0);
            }
            if (pp.x - r.max.x).abs() <= 4.0 {
                return Some(1);
            }
            if (pp.y - r.max.y).abs() <= 4.0 {
                return Some(2);
            }
            if (pp.y - r.min.y).abs() <= 4.0 {
                return Some(3);
            }
        }
        None
    };
    if let Some(pp) = hover {
        match edge_at(pp) {
            Some(0 | 1) => p.ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal),
            Some(_) => p.ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical),
            None => {}
        }
    }
    let zone_at = |pp: egui::Pos2| zones.iter().enumerate().rev().find(|(_, z)| zr(z).contains(pp)).map(|(i, _)| i);
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
        && let Some(e) = edge_at(pp)
    {
        ed.drag = Some(Drag::Edge(e, 0));
    }
    if resp.dragged()
        && let (Some(Drag::Edge(e, done)), Some(pp), Some(o)) = (ed.drag, resp.interact_pointer_pos(), p.ui.input(|i| i.pointer.press_origin()))
    {
        let steps = if e < 2 { ((pp.x - o.x) / area.width() * (hi - lo)).round() as i32 } else { ((o.y - pp.y) / area.height() * 127.0).round() as i32 };
        if steps != done {
            doc.src[s].move_edge(&ed.sel.clone(), e, steps - done);
            ed.drag = Some(Drag::Edge(e, steps));
            ed.dirty = true;
        }
    }
    if resp.drag_stopped() && matches!(ed.drag, Some(Drag::Edge(..))) {
        ed.drag = None;
    }
    if resp.clicked()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let shift = p.ui.input(|i| i.modifiers.shift);
        match zone_at(pp) {
            Some(zi) => {
                if shift {
                    if let Some(k) = ed.sel.iter().position(|x| *x == (gi, zi)) {
                        ed.sel.remove(k);
                    } else {
                        ed.sel.push((gi, zi));
                    }
                } else {
                    ed.sel = vec![(gi, zi)];
                    if ed.preview {
                        p.actions.push(format!("zaud-zone:{s}:{gi}:{zi}"));
                    }
                }
            }
            None if !shift => ed.sel.clear(),
            None => {}
        }
    }
    if resp.secondary_clicked()
        && let Some(pp) = resp.interact_pointer_pos()
        && let Some(zi) = zone_at(pp)
        && !ed.sel.contains(&(gi, zi))
    {
        ed.sel = vec![(gi, zi)];
    }
    let groups: Vec<String> = doc.src[s].groups.iter().map(|g| g.name.clone()).collect();
    resp.context_menu(|ui| {
        let has = !ed.sel.is_empty();
        if ui.add_enabled(has, egui::Button::new("Cut")).clicked() {
            ed.clip = doc.src[s].take(&ed.sel);
            ed.sel.clear();
            ed.dirty = true;
            ui.close();
        }
        if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
            ed.clip = doc.src[s].copy(&ed.sel);
            ui.close();
        }
        ui.add_enabled_ui(!ed.clip.is_empty(), |ui| {
            ui.menu_button("Paste to existing group", |ui| {
                for (k, n) in groups.iter().enumerate() {
                    if ui.button(n).clicked() {
                        doc.src[s].paste(ed.clip.clone(), Some(k));
                        ed.group = k;
                        ed.dirty = true;
                        ui.close();
                    }
                }
            });
            if ui.button("Paste to new group").clicked() {
                ed.group = doc.src[s].paste(ed.clip.clone(), None);
                ed.dirty = true;
                ui.close();
            }
        });
        if ui.add_enabled(has, egui::Button::new("Delete")).clicked() {
            doc.src[s].take(&ed.sel);
            ed.sel.clear();
            ed.dirty = true;
            ui.close();
        }
        if ui.add_enabled(ed.sel.len() == 1, egui::Button::new("Exchange sample…")).clicked() {
            let (g, z) = ed.sel[0];
            p.actions.push(format!("zexchange:{s}:{g}:{z}"));
            ui.close();
        }
        ui.add_enabled_ui(has, |ui| {
            ui.menu_button("Assign to existing group", |ui| {
                for (k, n) in groups.iter().enumerate() {
                    if ui.button(n).clicked() {
                        let moved: Vec<(usize, usize)> = ed.sel.clone();
                        doc.src[s].assign(&moved, Some(k));
                        ed.sel.clear();
                        ed.group = k.min(doc.src[s].groups.len().saturating_sub(1));
                        ed.dirty = true;
                        ui.close();
                    }
                }
            });
            if ui.button("Assign to new group").clicked() {
                let moved: Vec<(usize, usize)> = ed.sel.clone();
                ed.group = doc.src[s].assign(&moved, None);
                ed.sel.clear();
                ed.dirty = true;
                ui.close();
            }
        });
    });
    // the keyboard: a click auditions the key (low velocity near the top, high near the bottom)
    let kpt = p.ui.painter().clone().with_clip_rect(kb);
    kpt.rect_filled(kb, 0.0, Color32::WHITE);
    let kw = kx(1.0) - kx(0.0);
    for n in lo.floor().max(0.0) as i32..=(hi.ceil() as i32).min(127) {
        let x = kx(n as f32);
        if is_black(n as u8) {
            kpt.rect_filled(Rect::from_min_size(pos2(x, kb.min.y), vec2(kw, kb.height() * 0.62)), 1.0, rgb(0x111111));
        } else {
            kpt.vline(x, kb.y_range(), Stroke::new(0.6, rgb(0x777777)));
        }
    }
    let kresp = p.ui.interact(kb, egui::Id::unique(("alch-ed-kbd", p.t)), Sense::click_and_drag());
    if kresp.is_pointer_button_down_on()
        && ed.drag.is_none()
        && let Some(pp) = kresp.interact_pointer_pos()
    {
        let n = (lo + (pp.x - kb.min.x) / kb.width() * (hi - lo)).floor().clamp(0.0, 127.0) as u8;
        let v = (1.0 + (pp.y - kb.min.y) / kb.height() * 126.0).round().clamp(1.0, 127.0) as u8;
        ed.drag = Some(Drag::Key);
        p.actions.push(format!("zaud-key:{s}:{n}:{v}"));
    }
    if !kresp.is_pointer_button_down_on() && ed.drag == Some(Drag::Key) {
        ed.drag = None;
        p.actions.push("zaud-stop".into());
    }
    // octave names under the keyboard
    for o in 0..11 {
        let n = o * 12;
        if (n as f32) >= lo && (n as f32) < hi {
            p.ui.painter().text(pos2(kx(n as f32) + 1.0, kb.max.y + 5.0 * K), Align2::LEFT_CENTER, &note_name(n as u8), look::regular(9.5), p.skin.text);
        }
    }
    let nv = scroll(p, ed, fr(p, 244.0, 486.0, 810.0, 9.0), 0, (lo, hi), 128.0, 12.0);
    if nv != (lo, hi) {
        ed.km = nv;
    }
}

fn waveform(p: &mut Pane, ix: &Ix, ed: &mut Ed, doc: &mut ZoneDoc, pos: Option<(usize, usize, f64)>) {
    let s = ed.src;
    let su = s as u32;
    // > : preview on zone selection (and once now)
    let pr = fr(p, 244.0, 511.0, 16.0, 16.0);
    let presp = p.ui.interact(pr, egui::Id::unique(("alch-ed-prev", p.t)), Sense::click());
    p.ui.painter().rect_filled(pr, 2.0, if ed.preview { BLUE } else { rack::BTN });
    crate::glyphs::paint(p.ui.painter(), pr, crate::glyphs::ph_fill::PLAY, 9.0, Color32::WHITE);
    if presp.clicked() {
        ed.preview = !ed.preview;
        if let Some((g, z)) = ed.focus() {
            p.actions.push(format!("zaud-zone:{s}:{g}:{z}"));
        }
    }
    let mode = g(p, ix, az::LOOP_MODE + su).round() as u8;
    let samp = g(p, ix, az::SAMP_ON + su) >= 0.5;
    let Some((fg, fz)) = ed.focus() else {
        let r = fr(p, 244.0, 542.0, 810.0, 234.0);
        p.ui.painter().rect_filled(r, 2.0, WAVE_BG);
        return;
    };
    let Some(z) = doc.src[s].zone(fg, fz).cloned() else { return };
    // Loop XFade: Sampler element only, in Continuous / Sustain
    if samp {
        let can = matches!(mode, az::L_CONT | az::L_SUSTAIN);
        if rack::flat(p, fr(p, 330.0, 512.0, 74.0, 16.0), "Loop XFade", z.xfade_on, can, ("xfade", s)) {
            let on = !z.xfade_on;
            for &(g, zi) in &ed.sel.clone() {
                if let Some(zn) = doc.src[s].zone_mut(g, zi) {
                    zn.xfade_on = on;
                    if on && zn.xfade == 0 {
                        zn.xfade = ((zn.loop_end - zn.loop_start) / 10).max(1);
                    }
                }
            }
            ed.dirty = true;
        }
    }
    let ruler = fr(p, 244.0, 542.0, 810.0, 16.0);
    let lstrip = fr(p, 244.0, 558.0, 810.0, 8.0);
    let area = fr(p, 244.0, 566.0, 810.0, 198.0);
    let pt = p.ui.painter().clone().with_clip_rect(ruler.union(area));
    pt.rect_filled(ruler.union(area), 0.0, WAVE_BG);
    let n = z.frames().max(1) as f32;
    let (v0, v1) = ed.wf();
    let fx = |f: f32| area.min.x + (f / n - v0) / (v1 - v0) * area.width();
    let xf = |x: f32| ((x - area.min.x) / area.width() * (v1 - v0) + v0) * n;
    let secs = z.seconds().max(1e-3);
    // the time line in seconds
    let vis = secs * (v1 - v0);
    let step = [0.01f32, 0.02, 0.05, 0.1, 0.2, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0].into_iter().find(|s| vis / s <= 10.0).unwrap_or(60.0);
    let mut t = (secs * v0 / step).ceil() * step;
    while t <= secs * v1 + 1e-4 {
        let x = fx(t / secs * n);
        pt.text(pos2(x + 2.0, ruler.center().y), Align2::LEFT_CENTER, &format!("{}", (t * 1000.0).round() / 1000.0), look::regular(9.5), p.skin.text);
        pt.vline(x, Rect::from_min_max(ruler.min, pos2(ruler.max.x, ruler.max.y)).y_range(), Stroke::new(1.0, rgb(0x2c3540)));
        t += step;
    }
    pt.rect_filled(lstrip, 0.0, rgb(0x1a2430));
    // outside S…E darker; the loop region lighter in looping modes
    let looping = matches!(mode, az::L_CONT | az::L_SUSTAIN | az::L_FWDBACK);
    if looping {
        pt.rect_filled(Rect::from_x_y_ranges(fx(z.loop_start as f32)..=fx(z.loop_end as f32), area.y_range()), 0.0, rgb(0x1e2a37));
    }
    // the waveform: min / max per column
    if let Some(a) = z.audio.as_ref() {
        let d = &a.ch[0];
        let mid = area.center().y;
        let h = area.height() / 2.0 * 0.95;
        let cols = area.width() as usize;
        for c in 0..cols {
            let f0 = xf(area.min.x + c as f32) as isize;
            let f1 = (xf(area.min.x + c as f32 + 1.0) as isize).max(f0 + 1);
            if f1 <= 0 || f0 >= d.len() as isize {
                continue;
            }
            let (mut mn, mut mx) = (0.0f32, 0.0f32);
            let stride = ((f1 - f0) as usize / 64).max(1);
            for i in (f0.max(0) as usize..(f1 as usize).min(d.len())).step_by(stride) {
                mn = mn.min(d[i]);
                mx = mx.max(d[i]);
            }
            let x = area.min.x + c as f32 + 0.5;
            pt.vline(x, (mid - mx * h)..=(mid - mn * h + 0.5), Stroke::new(1.0, WAVE));
        }
        pt.hline(area.x_range(), mid, Stroke::new(0.6, rgb(0x5b7aa0)));
    }
    let shade = Color32::from_black_alpha(110);
    pt.rect_filled(Rect::from_x_y_ranges(area.min.x..=fx(z.start as f32), area.y_range()), 0.0, shade);
    pt.rect_filled(Rect::from_x_y_ranges(fx(z.end as f32)..=area.max.x, area.y_range()), 0.0, shade);
    // the play position
    if let Some((g, zi, at)) = pos
        && (g, zi) == (fg, fz)
    {
        pt.vline(fx(at as f32), area.y_range(), Stroke::new(1.0, Color32::WHITE));
    }
    // markers: S, E (bright blue), loop start / end (faint orange), XF
    let sb = rgb(0x3fb6ef);
    let mut marks: Vec<(u8, f32)> = vec![(0, z.start as f32), (1, z.end as f32)];
    for (k, f) in [(0u8, z.start as f32), (1, z.end as f32)] {
        let x = fx(f);
        pt.vline(x, area.y_range(), Stroke::new(1.5, sb));
        let fl = if k == 0 { Rect::from_min_size(pos2(x, area.min.y + 4.0), vec2(16.0, 14.0)) } else { Rect::from_min_size(pos2(x - 16.0, area.min.y + 18.0), vec2(16.0, 14.0)) };
        pt.rect_filled(fl, 2.0, sb.gamma_multiply(0.35));
        pt.rect_stroke(fl, 2.0, Stroke::new(1.0, sb), egui::StrokeKind::Inside);
        pt.text(fl.center(), Align2::CENTER_CENTER, if k == 0 { "S" } else { "E" }, look::regular(9.5), Color32::WHITE);
    }
    if looping {
        let lo = rgb(0xd08a3a);
        for (k, f) in [(2u8, z.loop_start as f32), (3, z.loop_end as f32)] {
            let x = fx(f);
            pt.vline(x, area.y_range(), Stroke::new(1.0, lo.gamma_multiply(0.6)));
            let tri = if k == 2 { vec![pos2(x, lstrip.min.y), pos2(x + 7.0, lstrip.center().y), pos2(x, lstrip.max.y)] } else { vec![pos2(x, lstrip.min.y), pos2(x - 7.0, lstrip.center().y), pos2(x, lstrip.max.y)] };
            pt.add(egui::Shape::convex_polygon(tri, lo, Stroke::NONE));
            marks.push((k, f));
        }
        if z.xfade_on && samp && matches!(mode, az::L_CONT | az::L_SUSTAIN) {
            let f = z.loop_end.saturating_sub(z.xfade) as f32;
            let x = fx(f);
            pt.vline(x, area.y_range(), Stroke::new(1.0, Color32::WHITE));
            let b = Rect::from_min_size(pos2(x - 9.0, area.min.y + 4.0), vec2(18.0, 13.0));
            pt.rect_filled(b, 1.0, WAVE_BG);
            pt.rect_stroke(b, 1.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
            pt.text(b.center(), Align2::CENTER_CENTER, "XF", look::regular(8.5), Color32::WHITE);
            marks.push((4, f));
        }
    }
    let resp = p.ui.interact(lstrip.union(area), egui::Id::unique(("alch-ed-wave", p.t)), Sense::drag());
    if resp.drag_started()
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let near = marks.iter().filter(|(_, f)| (fx(*f) - pp.x).abs() <= 6.0).min_by(|a, b| (fx(a.1) - pp.x).abs().total_cmp(&(fx(b.1) - pp.x).abs())).map(|m| m.0);
        if let Some(m) = near {
            ed.drag = Some(Drag::Marker(m, ed.wf()));
            // holding a loop marker zooms in around it
            if m == 2 || m == 3 {
                let c = pp.x;
                let at = xf(c) / n;
                let sp = ((v1 - v0) / 4.0).max(0.01);
                let lo = (at - sp / 2.0).clamp(0.0, 1.0 - sp);
                ed.wf = (lo, lo + sp);
            }
        }
    }
    if resp.dragged()
        && let (Some(Drag::Marker(m, _)), Some(pp)) = (ed.drag, resp.interact_pointer_pos())
    {
        let (v0, v1) = ed.wf();
        let f = (((pp.x - area.min.x) / area.width() * (v1 - v0) + v0) * n).clamp(0.0, n) as u32;
        let sel = ed.sel.clone();
        for &(g, zi) in &sel {
            if let Some(zn) = doc.src[s].zone_mut(g, zi) {
                match m {
                    0 => zn.start = f.min(zn.end.saturating_sub(1)),
                    1 => zn.end = f.max(zn.start + 1).min(zn.frames()),
                    2 => zn.set_loop(f, zn.loop_end),
                    3 => {
                        // the XF handle moves with the loop end
                        zn.set_loop(zn.loop_start, f.max(zn.loop_start));
                        zn.xfade = zn.xfade.min(zn.loop_end - zn.loop_start);
                    }
                    _ => zn.xfade = zn.loop_end.saturating_sub(f).min(zn.loop_end - zn.loop_start),
                }
            }
        }
        ed.dirty = true;
    }
    if resp.drag_stopped()
        && let Some(Drag::Marker(m, view)) = ed.drag
    {
        if m == 2 || m == 3 {
            ed.wf = view;
        }
        ed.drag = None;
    }
    let nv = scroll(p, ed, fr(p, 244.0, 766.0, 810.0, 9.0), 1, ed.wf(), 1.0, 0.002);
    if nv != ed.wf() {
        ed.wf = nv;
    }
    let _ = ORANGE;
}
