//! Bramble's Channel EQ (the stand-in for Logic's, same parameters in the same order: Logic
//! research E04/E05 — eight bands of [frequency Hz, gain dB (cut filters: slope), Q, on]):
//! Low Cut, Low Shelf, four Bells, High Shelf, High Cut. Standard biquads (RBJ cookbook).
//! The window: the response curve on a log frequency scale, a coloured handle per band to drag
//! (frequency across, gain up and down), the bands' on / off buttons and readouts, as Logic's.

use egui::{pos2, vec2, Align2, Color32, Sense, Stroke};

pub const RATE: f32 = crate::audio::RATE as f32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub freq: f32,
    /// dB for shelves and bells; for the cut filters, the slope (Logic's value; 12 dB/oct here)
    pub gain: f32,
    pub q: f32,
    pub on: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub bands: [Band; 8],
}

impl Default for Params {
    /// Logic's Channel EQ default (#default.pst as read in E04), with the cuts off so a new EQ is
    /// flat.
    fn default() -> Self {
        let b = |freq, gain, q, on| Band { freq, gain, q, on };
        Params { bands: [b(20.0, 2.0, 0.71, false), b(75.0, 0.0, 1.0, true), b(100.0, 0.0, 0.6, true), b(250.0, 0.0, 0.3, true), b(1040.0, 0.0, 0.41, true), b(2500.0, 0.0, 0.2, true), b(7500.0, 0.0, 1.0, true), b(20000.0, 4.0, 0.71, false)] }
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
        Some(p)
    }

    pub fn is_flat(&self) -> bool {
        self.bands.iter().enumerate().all(|(i, b)| !b.on || ((1..7).contains(&i) && b.gain.abs() < 0.01))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: [f32; 2],
    z2: [f32; 2],
}

/// Logic's band types, by position.
fn kind(i: usize) -> &'static str {
    match i {
        0 => "lowcut",
        1 => "lowshelf",
        6 => "highshelf",
        7 => "highcut",
        _ => "bell",
    }
}

impl Biquad {
    fn design(i: usize, b: &Band) -> Biquad {
        let w = std::f32::consts::TAU * b.freq.min(RATE * 0.45) / RATE;
        let (sw, cw) = (w.sin(), w.cos());
        let a = 10f32.powf(b.gain / 40.0);
        let q = if i == 0 || i == 7 { 0.707 } else { (b.q * 2.0).max(0.2) };
        let alpha = sw / (2.0 * q);
        let (b0, b1, b2, a0, a1, a2) = match kind(i) {
            "lowcut" => ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            "highcut" => ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            "lowshelf" => {
                let s = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) - (a - 1.0) * cw + s), 2.0 * a * ((a - 1.0) - (a + 1.0) * cw), a * ((a + 1.0) - (a - 1.0) * cw - s), (a + 1.0) + (a - 1.0) * cw + s, -2.0 * ((a - 1.0) + (a + 1.0) * cw), (a + 1.0) + (a - 1.0) * cw - s)
            }
            "highshelf" => {
                let s = 2.0 * a.sqrt() * alpha;
                (a * ((a + 1.0) + (a - 1.0) * cw + s), -2.0 * a * ((a - 1.0) + (a + 1.0) * cw), a * ((a + 1.0) + (a - 1.0) * cw - s), (a + 1.0) - (a - 1.0) * cw + s, 2.0 * ((a - 1.0) - (a + 1.0) * cw), (a + 1.0) - (a - 1.0) * cw - s)
            }
            _ => (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a),
        };
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: [0.0; 2], z2: [0.0; 2] }
    }

    fn run(&mut self, x: f32, ch: usize) -> f32 {
        let y = self.b0 * x + self.z1[ch];
        self.z1[ch] = self.b1 * x - self.a1 * y + self.z2[ch];
        self.z2[ch] = self.b2 * x - self.a2 * y;
        y
    }

    /// The magnitude in dB at `f`.
    fn response(&self, f: f32) -> f32 {
        let w = std::f32::consts::TAU * f / RATE;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = -(self.b1 * s1 + self.b2 * s2);
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = -(self.a1 * s1 + self.a2 * s2);
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).max(1e-12).log10()
    }
}

/// A running Channel EQ (its filters keep their state between blocks).
#[derive(Clone, Debug)]
pub struct Eq {
    pub params: Params,
    filters: Vec<(usize, Biquad)>,
}

impl Eq {
    pub fn new(params: Params) -> Eq {
        let mut e = Eq { params, filters: Vec::new() };
        e.set(params);
        e
    }

    pub fn set(&mut self, params: Params) {
        self.params = params;
        self.filters = params.bands.iter().enumerate().filter(|(i, b)| b.on && !((1..7).contains(i) && b.gain.abs() < 0.01)).map(|(i, b)| (i, Biquad::design(i, b))).collect();
    }

    pub fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        for (_, f) in &mut self.filters {
            for x in l.iter_mut() {
                *x = f.run(*x, 0);
            }
            for x in r.iter_mut() {
                *x = f.run(*x, 1);
            }
        }
    }
}

/// The whole EQ's response in dB at `f` (the window's curve).
pub fn response(p: &Params, f: f32) -> f32 {
    p.bands.iter().enumerate().filter(|(_, b)| b.on).map(|(i, b)| Biquad::design(i, b).response(f)).sum()
}

const BAND_COLORS: [u32; 8] = [0xe0524c, 0xe8913a, 0xe8d03a, 0x5ac85a, 0x3ac8c0, 0x3a8ee8, 0x8a5ae8, 0xd05ac8];

/// The Channel EQ window; true when the settings changed.
pub fn window(ctx: &egui::Context, open: &mut bool, title: &str, p: &mut Params) -> bool {
    let mut changed = false;
    egui::Window::new(format!("{title} — Channel EQ")).id(egui::Id::unique("channel-eq")).open(open).collapsible(false).resizable(false).default_width(640.0).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(620.0, 240.0), Sense::hover());
        let pa = ui.painter_at(rect);
        pa.rect_filled(rect, 6.0, Color32::from_rgb(0x14, 0x16, 0x1a));
        let x_of = |f: f32| rect.min.x + (f.log10() - 20f32.log10()) / (20000f32.log10() - 20f32.log10()) * rect.width();
        let f_of = |x: f32| 10f32.powf(20f32.log10() + (x - rect.min.x) / rect.width() * (20000f32.log10() - 20f32.log10()));
        let y_of = |db: f32| rect.center().y - db / 24.0 * (rect.height() / 2.0 - 10.0);
        // the grid: decades and ±6 / 12 / 18 dB
        for f in [30.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
            pa.vline(x_of(f), rect.y_range(), Stroke::new(1.0, Color32::from_gray(0x2a)));
            let t = if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") };
            pa.text(pos2(x_of(f) + 2.0, rect.max.y - 12.0), Align2::LEFT_CENTER, t, crate::look::regular(9.5), Color32::from_gray(0x70));
        }
        for db in [-18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0] {
            pa.hline(rect.x_range(), y_of(db), Stroke::new(1.0, if db == 0.0 { Color32::from_gray(0x44) } else { Color32::from_gray(0x24) }));
        }
        // the curve
        let pts: Vec<egui::Pos2> = (0..=300).map(|k| {
            let x = rect.min.x + k as f32 / 300.0 * rect.width();
            pos2(x, y_of(response(p, f_of(x)).clamp(-24.0, 24.0)))
        }).collect();
        pa.add(egui::Shape::line(pts, Stroke::new(2.0, Color32::from_rgb(0x9c, 0xe0, 0xff))));
        // the band handles
        for i in 0..8 {
            let b = p.bands[i];
            let c = crate::look::mix(kn_ui::theme::rgb(BAND_COLORS[i]), Color32::BLACK, if b.on { 0.0 } else { 0.6 });
            let gy = if (1..7).contains(&i) { b.gain } else { 0.0 };
            let at = pos2(x_of(b.freq), y_of(gy));
            pa.circle_filled(at, 6.0, c);
            let resp = ui.interact(egui::Rect::from_center_size(at, vec2(16.0, 16.0)), ui.scope_id().with(("eqband", i)), Sense::click_and_drag());
            if resp.dragged() {
                if let Some(pp) = resp.interact_pointer_pos() {
                    p.bands[i].freq = f_of(pp.x.clamp(rect.min.x, rect.max.x)).clamp(20.0, 20000.0);
                    if (1..7).contains(&i) {
                        p.bands[i].gain = (-(pp.y - rect.center().y) / (rect.height() / 2.0 - 10.0) * 24.0).clamp(-24.0, 24.0);
                    }
                    p.bands[i].on = true;
                    changed = true;
                }
            }
            if resp.double_clicked() {
                p.bands[i].on = !p.bands[i].on;
                changed = true;
            }
        }
        // the bands' buttons and readouts
        ui.horizontal(|ui| {
            for i in 0..8 {
                let b = &mut p.bands[i];
                ui.vertical(|ui| {
                    ui.set_width(70.0);
                    let label = ["Low Cut", "Low Shelf", "Bell 1", "Bell 2", "Bell 3", "Bell 4", "High Shelf", "High Cut"][i];
                    if kn_ui::theme::checkbox(ui, &mut b.on, label).changed() {
                        changed = true;
                    }
                    let f = if b.freq >= 1000.0 { format!("{:.2} kHz", b.freq / 1000.0) } else { format!("{:.0} Hz", b.freq) };
                    ui.label(egui::RichText::new(f).font(crate::look::regular(11.0)));
                    if (1..7).contains(&i) {
                        ui.label(egui::RichText::new(format!("{:+.1} dB", b.gain)).font(crate::look::regular(11.0)));
                    }
                });
            }
        });
    });
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
