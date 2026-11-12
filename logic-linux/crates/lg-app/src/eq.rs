//! The track's Channel EQ (its EQ slot): the settings (eight bands of [on, frequency, gain —
//! the cuts' slope —, Q] as Logic's block, Master Gain, the window's options) and the
//! processing, which is lg-fx's Channel EQ (Logic-matched slopes and shelves, Gain-Q coupling,
//! HQ, Processing modes, the analyzer). The window is Logic's plug-in window: the shared header
//! (plughead.rs: bypass, Settings, ‹ ›, Compare, Copy | Paste, Undo | Redo, View Editor /
//! Controls and size, Link), the Channel EQ view (eqview.rs) and the footer.

use egui::vec2;
use lg_dsp::{Effect, Plugin};

pub const RATE: f32 = crate::audio::RATE as f32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub freq: f32,
    /// dB for shelves and bells; for the cut filters, the slope (Logic's value × 6 dB/oct)
    pub gain: f32,
    pub q: f32,
    pub on: bool,
}

/// The window's options and extras by id from 1000 (lg_fx::channel_eq's ANALYZER …).
pub const OPTS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub bands: [Band; 8],
    pub master: f32,
    pub opts: [f32; OPTS],
    pub bypass: bool,
}

impl Default for Params {
    /// Logic's Channel EQ default (#default.pst as read in E04), with the cuts off so a new EQ is
    /// flat.
    fn default() -> Self {
        let b = |freq, gain, q, on| Band { freq, gain, q, on };
        let fx = lg_fx::channel_eq::ChannelEq::new(RATE);
        let mut opts = [0.0; OPTS];
        for (i, o) in opts.iter_mut().enumerate() {
            *o = fx.get(1000 + i as u32);
        }
        Params { bands: [b(20.0, 2.0, 0.71, false), b(75.0, 0.0, 1.0, true), b(100.0, 0.0, 0.6, true), b(250.0, 0.0, 0.3, true), b(1040.0, 0.0, 0.41, true), b(2500.0, 0.0, 0.2, true), b(7500.0, 0.0, 1.0, true), b(20000.0, 4.0, 0.71, false)], master: 0.0, opts, bypass: false }
    }
}

impl Params {
    /// From Logic's saved parameter block: 8 × [on, frequency, gain (or slope), Q], then Master
    /// Gain at 32 (lg-fx's reading, confirmed on E05's "band 5 off").
    pub fn from_logic(v: &[f32]) -> Option<Params> {
        if v.len() < 32 {
            return None;
        }
        let mut p = Params::default();
        for (i, b) in p.bands.iter_mut().enumerate() {
            let f = &v[4 * i..4 * i + 4];
            *b = Band { on: f[0] > 0.5, freq: f[1].clamp(10.0, 22000.0), gain: f[2], q: f[3].max(0.1) };
        }
        p.master = v.get(32).copied().filter(|g| g.is_finite()).unwrap_or(0.0);
        Some(p)
    }

    pub fn is_flat(&self) -> bool {
        self.master.abs() < 0.01 && self.bands.iter().enumerate().all(|(i, b)| !b.on || ((1..7).contains(&i) && b.gain.abs() < 0.01))
    }

    /// The value of a Channel EQ parameter id.
    pub fn value(&self, id: u32) -> f32 {
        match id {
            0..=31 => {
                let b = &self.bands[id as usize / 4];
                [if b.on { 1.0 } else { 0.0 }, b.freq, b.gain, b.q][id as usize % 4]
            }
            32 => self.master,
            1000.. => self.opts.get(id as usize - 1000).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    pub fn set_value(&mut self, id: u32, v: f32) {
        match id {
            0..=31 => {
                let b = &mut self.bands[id as usize / 4];
                match id % 4 {
                    0 => b.on = v >= 0.5,
                    1 => b.freq = v.clamp(20.0, 20000.0),
                    2 => b.gain = v,
                    _ => b.q = v.clamp(0.1, 100.0),
                }
            }
            32 => self.master = v.clamp(-24.0, 24.0),
            1000.. => {
                if let Some(o) = self.opts.get_mut(id as usize - 1000) {
                    *o = v;
                }
            }
            _ => {}
        }
    }

    /// A Channel EQ with these settings (the processing and its curves).
    pub fn unit(&self) -> lg_fx::channel_eq::ChannelEq {
        let mut fx = lg_fx::channel_eq::ChannelEq::new(RATE);
        let ids: Vec<u32> = fx.params().iter().map(|p| p.id).collect();
        for id in ids {
            fx.set(id, self.value(id));
        }
        fx
    }
}

/// A running Channel EQ (its filters keep their state between blocks).
pub struct Eq {
    pub params: Params,
    fx: lg_fx::channel_eq::ChannelEq,
}

impl Eq {
    pub fn new(params: Params) -> Eq {
        let mut fx = params.unit();
        fx.reset();
        Eq { params, fx }
    }

    pub fn set(&mut self, params: Params) {
        for id in self.fx.params().iter().map(|p| p.id).collect::<Vec<_>>() {
            if params.value(id) != self.params.value(id) {
                self.fx.set(id, params.value(id));
            }
        }
        self.params = params;
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        if !self.params.bypass {
            self.fx.process(l, r);
        }
    }

    /// The analyzer's bands (dB), for the window.
    pub fn analyzer(&self) -> Vec<f32> {
        self.fx.curve("analyzer")
    }
}

/// The whole EQ's response in dB at `f` (the strip's thumbnail curve).
pub fn response(p: &Params, f: f32) -> f32 {
    p.unit().response_db(f) - p.master
}

type History = (Vec<Params>, Vec<Params>);

/// The track's Channel EQ window (track `t`); true when the settings changed.
pub fn window(ctx: &egui::Context, open: &mut bool, t: usize, title: &str, p: &mut Params, analyzer: &[f32]) -> bool {
    use crate::plughead::{Act, Head};
    let before = *p;
    let id = egui::Id::unique(("chan-eq-win", t));
    let hist_id = id.with("hist");
    let (undo_n, redo_n) = ctx.data(|d| d.get_temp::<History>(hist_id)).map(|h| (h.0.len(), h.1.len())).unwrap_or((0, 0));
    let orig_id = id.with("orig");
    let back_id = id.with("back");
    let clip_id = egui::Id::unique("chan-eq-clipboard");
    let name_id = id.with("setting");
    let view_id = id.with("editor");
    let editor = ctx.data(|d| d.get_temp::<bool>(view_id)).unwrap_or(true);
    let edited = ctx.data(|d| d.get_temp::<Params>(orig_id)).is_some();
    let can_back = ctx.data(|d| d.get_temp::<Params>(back_id)).is_some();
    let clip: Option<Params> = ctx.data(|d| d.get_temp(clip_id));
    let setting: String = ctx.data(|d| d.get_temp(name_id)).unwrap_or_else(|| "Default Preset".into());
    let saved = crate::pluginwin::saved_settings("Channel EQ");
    let unit = p.unit();
    let params: Vec<lg_dsp::Param> = unit.params().to_vec();
    let values: Vec<f32> = params.iter().map(|q| p.value(q.id)).collect();
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut act = None;
    let mut load: Option<(String, Vec<(u32, f32)>)> = None;
    let mut save: Option<String> = None;
    let scale = crate::plughead::scale(ctx, "Channel EQ");
    let resp = egui::Window::new(title).id(id).open(open).collapsible(false).resizable(true).default_size(vec2(1014.0, 540.0)).show(ctx, |ui| {
        let head = Head { id: id.with("head"), plugin: "Channel EQ", bypassed: p.bypass, setting: &setting, comparing: edited, can_compare: edited || can_back, can_paste: clip.is_some(), can_undo: undo_n > 0, can_redo: redo_n > 0, side_chain: false, editor: Some(editor) };
        let out = crate::plughead::header(ui, &head);
        act = out.act;
        egui::Popup::menu(&out.setting).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
            ui.set_min_width(220.0);
            if ui.button("Copy Setting").clicked() {
                act = Some(Act::Copy);
                ui.close();
            }
            if ui.add_enabled(clip.is_some(), egui::Button::new("Paste Setting")).clicked() {
                act = Some(Act::Paste);
                ui.close();
            }
            if ui.button("Reset Setting").clicked() {
                load = Some(("Default Preset".into(), params.iter().map(|q| (q.id, q.default)).collect()));
                ui.close();
            }
            ui.separator();
            ui.horizontal(|ui| {
                let typed_id = id.with("typed");
                let mut typed: String = ui.data(|d| d.get_temp(typed_id)).unwrap_or_default();
                ui.add(egui::TextEdit::singleline(&mut typed).hint_text("Setting name").desired_width(110.0));
                if ui.add_enabled(!typed.trim().is_empty(), egui::Button::new("Save Setting As…")).clicked() {
                    save = Some(typed.trim().to_string());
                    typed.clear();
                    ui.close();
                }
                ui.data_mut(|d| d.insert_temp(typed_id, typed));
            });
            if !saved.is_empty() {
                ui.separator();
            }
            for (n, path) in &saved {
                if ui.add(egui::Button::selectable(*n == setting, n.as_str())).clicked() {
                    if let Some(v) = crate::pluginwin::read_setting(path) {
                        load = Some((n.clone(), v));
                    }
                    ui.close();
                }
            }
        });
        ui.add_space(4.0);
        if editor {
            crate::eqview::view(ui, &params, &values, analyzer, (usize::MAX, t), &mut set);
        } else {
            let texts = vec![None; params.len()];
            set.extend(crate::pluginwin::controls(ui, &params, &values, &texts, "chan-eq"));
        }
        let ext_id = id.with("ext");
        let mut ext_open = ui.ctx().data(|d| d.get_temp::<bool>(ext_id)).unwrap_or(false);
        crate::plughead::footer(ui, "Channel EQ", Some(&mut ext_open));
        ui.ctx().data_mut(|d| d.insert_temp(ext_id, ext_open));
        if ext_open {
            let i = params.iter().position(|q| q.name == "Analyzer Decay");
            if let Some(i) = i {
                set.extend(crate::pluginwin::controls(ui, &params[i..=i], &values[i..=i], &[None], "chan-eq-ext"));
            }
        }
    });
    if let Some(r) = &resp {
        crate::plughead::apply_scale(ctx, r.response.layer_id, r.response.rect.min, scale);
    }
    match act {
        Some(Act::Bypass) => p.bypass = !p.bypass,
        Some(Act::Editor(e)) => {
            ctx.data_mut(|d| d.insert_temp(view_id, e));
        }
        Some(Act::Copy) => {
            ctx.data_mut(|d| d.insert_temp(clip_id, *p));
        }
        Some(Act::Paste) => {
            if let Some(c) = clip {
                let bypass = p.bypass;
                *p = c;
                p.bypass = bypass;
            }
        }
        Some(Act::Compare) => {
            // swap between the edited setting and the one before the first edit
            if let Some(o) = ctx.data_mut(|d| d.remove_temp::<Params>(orig_id)) {
                ctx.data_mut(|d| d.insert_temp(back_id, *p));
                *p = o;
            } else if let Some(b) = ctx.data_mut(|d| d.remove_temp::<Params>(back_id)) {
                ctx.data_mut(|d| d.insert_temp(orig_id, *p));
                *p = b;
            }
        }
        Some(Act::Undo) | Some(Act::Redo) => {
            let redo = act == Some(Act::Redo);
            let now = *p;
            if let Some(v) = ctx.data_mut(|d| {
                let h = d.get_temp_mut_or_default::<History>(hist_id);
                let (from, to) = if redo { (&mut h.1, &mut h.0) } else { (&mut h.0, &mut h.1) };
                let v = from.pop()?;
                to.push(now);
                Some(v)
            }) {
                *p = v;
            }
            return *p != before;
        }
        Some(Act::Prev) | Some(Act::Next) if !saved.is_empty() => {
            let at = saved.iter().position(|(n, _)| *n == setting);
            let n = saved.len() as i32;
            let next = match at {
                Some(i) => (i as i32 + if act == Some(Act::Next) { 1 } else { -1 }).rem_euclid(n) as usize,
                None => 0,
            };
            if let Some(v) = crate::pluginwin::read_setting(&saved[next].1) {
                load = Some((saved[next].0.clone(), v));
            }
        }
        _ => {}
    }
    if let Some((n, pairs)) = load {
        for (pid, v) in pairs {
            p.set_value(pid, v);
        }
        ctx.data_mut(|d| d.insert_temp(name_id, n));
    }
    if let Some(n) = save
        && crate::pluginwin::write_setting("Channel EQ", &n, &params, &values).is_some()
    {
        ctx.data_mut(|d| d.insert_temp(name_id, n));
    }
    for (pid, v) in set {
        p.set_value(pid, v);
    }
    let changed = *p != before;
    if changed && !matches!(act, Some(Act::Compare)) {
        // the first edit keeps the setting as it was (Compare); each gesture is one undo step
        ctx.data_mut(|d| {
            if d.get_temp::<Params>(orig_id).is_none() {
                d.insert_temp(orig_id, before);
            }
            d.remove::<Params>(back_id);
        });
        let gesture_id = id.with("gesture");
        let in_gesture = ctx.data(|d| d.get_temp::<bool>(gesture_id)).unwrap_or(false);
        if !in_gesture {
            ctx.data_mut(|d| {
                let h = d.get_temp_mut_or_default::<History>(hist_id);
                h.0.push(before);
                h.1.clear();
            });
        }
        let down = ctx.input(|i| i.pointer.any_down());
        ctx.data_mut(|d| d.insert_temp(gesture_id, down));
    } else if !ctx.input(|i| i.pointer.any_down()) {
        ctx.data_mut(|d| d.insert_temp(id.with("gesture"), false));
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_by_default_and_bells_boost() {
        let p = Params::default();
        assert!(p.is_flat());
        assert!(response(&p, 1000.0).abs() < 0.01);
        let mut b = p;
        b.bands[4].gain = 6.0;
        assert!((response(&b, 1040.0) - 6.0).abs() < 0.5, "{}", response(&b, 1040.0));
        // the low cut takes the lows away
        let mut c = p;
        c.bands[0] = Band { freq: 200.0, gain: 2.0, q: 0.71, on: true };
        assert!(response(&c, 30.0) < -15.0);
        // Logic's saved block as E05 holds it: per band [on, frequency, gain, Q]; the cuts off
        let v = [0.0, 20.0, 2.0, 0.71, 1.0, 75.0, 0.0, 1.0, 1.0, 100.0, 0.0, 0.6, 1.0, 250.0, 0.0, 0.3, 1.0, 1040.0, 0.0, 0.41, 1.0, 2500.0, 0.0, 0.2, 1.0, 7500.0, 0.0, 1.0, 0.0, 20000.0, 4.0, 0.71];
        let l = Params::from_logic(&v).unwrap();
        assert!(!l.bands[0].on && l.bands[1].on && !l.bands[7].on && l.bands[4].freq == 1040.0 && l.bands[1].freq == 75.0);
    }

    #[test]
    fn filters_a_signal() {
        let mut p = Params::default();
        p.bands[0] = Band { freq: 500.0, gain: 2.0, q: 0.71, on: true };
        let mut eq = Eq::new(p);
        // a 50 Hz sine through a 500 Hz low cut: much quieter
        let n = 4800;
        let mut l: Vec<f32> = (0..n).map(|k| (std::f32::consts::TAU * 50.0 * k as f32 / RATE).sin()).collect();
        let mut r = l.clone();
        eq.process(&mut l, &mut r);
        let peak = l[n / 2..].iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!(peak < 0.05, "{peak}");
    }
}
