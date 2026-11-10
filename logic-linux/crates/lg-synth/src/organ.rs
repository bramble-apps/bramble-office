//! "Tonewheel B3": our tonewheel organ, a stand-in for Logic's Vintage B3 Organ.
//!
//! The organ is modelled the way the real one works rather than as voices: 91 tonewheels spin all
//! the time (frequencies from the real gear ratios, so the slightly-off equal temperament is
//! there), and each key closes nine contacts that connect wheels to the drawbar busses. The sum
//! goes through the single-trigger percussion, the vibrato/chorus scanner (a modulated delay line),
//! a tube preamp and a rotary speaker (horn and drum rotors with Doppler, amplitude modulation and
//! two microphones). Key click comes from the contacts closing (velocity-sensitive amount).
//! Polyphony is unlimited (all keys share the wheels), as on the real instrument.

use crate::dsp::{self, Biquad, CHUNK, Delay, OnePole, Rng};
use crate::poly::Channel;
use lg_dsp::{Instrument, Param, Plugin, Unit};

const VIB: &[&str] = &["Off", "V1", "V2", "V3", "C1", "C2", "C3"];
const ROTOR: &[&str] = &["Off", "Brake", "Chorale", "Tremolo"];
const PERC_H: &[&str] = &["Second", "Third"];

pub static PARAMS: &[Param] = &[
    Param::new(0, "Drawbar 16'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(1, "Drawbar 5 1/3'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(2, "Drawbar 8'", 0.0, 8.0, 8.0, Unit::None),
    Param::new(3, "Drawbar 4'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(4, "Drawbar 2 2/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(5, "Drawbar 2'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(6, "Drawbar 1 3/5'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(7, "Drawbar 1 1/3'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(8, "Drawbar 1'", 0.0, 8.0, 0.0, Unit::None),
    Param::new(9, "Percussion", 0.0, 1.0, 0.0, Unit::OnOff),
    Param::menu(10, "Percussion Harmonic", PERC_H, 1.0),
    Param::new(11, "Percussion Decay", 0.1, 3.0, 0.5, Unit::Seconds),
    Param::new(12, "Percussion Volume", -24.0, 6.0, 0.0, Unit::Db),
    Param::new(13, "Click Min", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(14, "Click Max", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::menu(15, "Vibrato/Chorus", VIB, 6.0),
    Param::new(16, "Leakage", 0.0, 1.0, 0.3, Unit::Ratio),
    Param::new(17, "Drive", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::menu(18, "Rotor Speed", ROTOR, 2.0),
    Param::new(19, "Horn Slow", 0.1, 3.0, 0.83, Unit::Hz),
    Param::new(20, "Horn Fast", 2.0, 10.0, 6.75, Unit::Hz),
    Param::new(21, "Drum Slow", 0.1, 3.0, 0.7, Unit::Hz),
    Param::new(22, "Drum Fast", 2.0, 10.0, 5.9, Unit::Hz),
    Param::new(23, "Acceleration", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(24, "Mic Width", 0.0, 1.0, 0.8, Unit::Ratio),
    Param::new(25, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(26, "Tonal Balance", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(27, "Mod Wheel to Rotor", 0.0, 1.0, 1.0, Unit::OnOff),
    Param::new(28, "Bend Range", 0.0, 12.0, 0.0, Unit::Semitones),
    Param::new(29, "Rotor Mix", 0.0, 1.0, 1.0, Unit::Ratio),
];

const WHEELS: usize = 92; // index 1..=91
/// Footage offsets in semitones from the 8' pitch.
const FOOT: [i32; 9] = [-12, 7, 0, 12, 19, 24, 28, 31, 36];

/// Real tonewheel gear ratios for C..B (driving/driven teeth); frequency = 20 Hz × ratio × teeth.
const GEARS: [(f32, f32); 12] = [
    (85.0, 104.0),
    (71.0, 82.0),
    (67.0, 73.0),
    (105.0, 108.0),
    (103.0, 100.0),
    (84.0, 77.0),
    (74.0, 64.0),
    (98.0, 80.0),
    (96.0, 74.0),
    (88.0, 64.0),
    (67.0, 46.0),
    (108.0, 70.0),
];

fn wheel_hz(w: usize) -> f32 {
    // wheel 1 = C at 32.7 Hz; 12 wheels per octave with 2,4,8..192 teeth
    let i = w - 1;
    let note = i % 12;
    let oct = i / 12;
    let teeth = [2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 192.0][oct.min(7)];
    // the top wheels (85..91) use the 192-tooth wheels with the gears of the first octave of that set
    let (a, b) = GEARS[note];
    let mut f = 20.0 * a / b * teeth;
    if oct >= 7 {
        // 192 teeth = 1.5 × 128: the real organ uses different gears there; keep the octave relation
        f = 20.0 * a / b * 256.0;
    }
    f
}

struct Rotor {
    angle: f32,
    speed: f32,
}

pub struct TonewheelB3 {
    params: Params,
    ch: Channel,
    rate: f32,
    phase: [f32; WHEELS],
    inc: [f32; WHEELS],
    amp: [f32; WHEELS],
    target: [f32; WHEELS],
    perc_amp: [f32; WHEELS],
    perc_target: [f32; WHEELS],
    keys: [bool; 128],
    sustained: [bool; 128],
    keys_down: usize,
    perc_env: f32,
    perc_coef: f32,
    contact_coef: f32,
    click: f32,
    click_f: Biquad,
    rng: Rng,
    leak_level: f32,
    // scanner
    scan: Delay,
    scan_phase: f32,
    // preamp
    pre_hp: OnePole,
    tone_lo: Biquad,
    tone_hi: Biquad,
    // rotary
    xover: OnePole,
    horn: Rotor,
    drum: Rotor,
    horn_dl: [Delay; 2],
    drum_dl: [Delay; 2],
    horn_tone: [OnePole; 2],
    db: [f32; 9],
    gain: f32,
}

use crate::poly::Params;

impl TonewheelB3 {
    pub fn new(rate: f32) -> TonewheelB3 {
        dsp::warm_tables();
        let mut inc = [0.0; WHEELS];
        for (w, x) in inc.iter_mut().enumerate().skip(1) {
            *x = wheel_hz(w) / rate;
        }
        let mut phase = [0.0; WHEELS];
        let mut rng = Rng::new(77);
        for p in phase.iter_mut() {
            *p = rng.unit();
        }
        let mut s = TonewheelB3 {
            params: Params::new(PARAMS),
            ch: Channel::new(rate),
            rate,
            phase,
            inc,
            amp: [0.0; WHEELS],
            target: [0.0; WHEELS],
            perc_amp: [0.0; WHEELS],
            perc_target: [0.0; WHEELS],
            keys: [false; 128],
            sustained: [false; 128],
            keys_down: 0,
            perc_env: 0.0,
            perc_coef: 0.0,
            contact_coef: dsp::tau_coef(0.0006, rate),
            click: 0.0,
            click_f: Biquad::default(),
            rng,
            leak_level: 0.0,
            scan: Delay::new((rate * 0.004) as usize),
            scan_phase: 0.0,
            pre_hp: OnePole::default(),
            tone_lo: Biquad::default(),
            tone_hi: Biquad::default(),
            xover: OnePole::default(),
            horn: Rotor {
                angle: 0.0,
                speed: 0.83,
            },
            drum: Rotor {
                angle: 0.3,
                speed: 0.7,
            },
            horn_dl: [
                Delay::new((rate * 0.01) as usize),
                Delay::new((rate * 0.01) as usize),
            ],
            drum_dl: [
                Delay::new((rate * 0.01) as usize),
                Delay::new((rate * 0.01) as usize),
            ],
            horn_tone: [OnePole::default(); 2],
            db: [0.0; 9],
            gain: 1.0,
        };
        s.click_f.bandpass(2500.0, 0.7, rate);
        s.pre_hp.lowpass_hz(40.0, rate);
        s.xover.lowpass_hz(800.0, rate);
        s.update();
        s
    }

    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }

    fn update(&mut self) {
        let perc_on = self.p(9) >= 0.5;
        for i in 0..9 {
            let v = self.p(i as u32).round();
            // 3 dB per drawbar step; percussion takes the 1' bus as on the original
            self.db[i] = if v <= 0.0 || (perc_on && i == 8) {
                0.0
            } else {
                10f32.powf(-3.0 * (8.0 - v) / 20.0)
            };
        }
        self.perc_coef = dsp::t60_coef(self.p(11), self.rate);
        self.tone_lo.low_shelf(250.0, -self.p(26) * 6.0, self.rate);
        self.tone_hi.high_shelf(3000.0, self.p(26) * 6.0, self.rate);
        self.ch.bend_up = self.p(28);
        self.ch.bend_down = self.p(28);
        self.ch.update_bend();
        self.gain = lg_dsp::db_to_gain(self.p(25)) * 0.035;
        self.recompute_targets();
    }

    fn wheel_for(sem: i32) -> usize {
        // sounding MIDI note → wheel (MIDI 24 = wheel 1), folding at both ends
        let mut w = sem - 23;
        while w < 1 {
            w += 12;
        }
        while w > 91 {
            w -= 12;
        }
        w as usize
    }

    fn recompute_targets(&mut self) {
        self.target = [0.0; WHEELS];
        self.perc_target = [0.0; WHEELS];
        let perc_h = if self.p(10) >= 0.5 { 19 } else { 12 };
        for k in 0..128 {
            if !self.keys[k] {
                continue;
            }
            // fold the key into the 61-note manual (C2..C7 in MIDI terms 36..96)
            let mut n = k as i32;
            while n < 36 {
                n += 12;
            }
            while n > 96 {
                n -= 12;
            }
            for (i, f) in FOOT.iter().enumerate() {
                if self.db[i] > 0.0 {
                    let w = Self::wheel_for(n + f);
                    // gentle high-wheel taper as on the real generator
                    let taper = if w > 60 {
                        1.0 - (w - 60) as f32 * 0.008
                    } else {
                        1.0
                    };
                    self.target[w] += self.db[i] * taper;
                }
            }
            self.perc_target[Self::wheel_for(n + perc_h)] = 1.0;
        }
    }

    fn key_down(&mut self, note: u8, vel: u8) {
        self.sustained[note as usize] = false;
        if self.keys[note as usize] {
            return;
        }
        self.keys[note as usize] = true;
        if self.keys_down == 0 && self.p(9) >= 0.5 {
            self.perc_env = 1.0; // single trigger: only when playing detached
        }
        self.keys_down += 1;
        let v = vel as f32 / 127.0;
        self.click += dsp::lerp(self.p(13), self.p(14), v) * 0.6;
        self.recompute_targets();
    }

    fn key_up(&mut self, note: u8) {
        if !self.keys[note as usize] {
            return;
        }
        self.keys[note as usize] = false;
        self.keys_down = self.keys_down.saturating_sub(1);
        self.click += 0.25 * self.p(13);
        self.recompute_targets();
    }

    fn rotor_targets(&self) -> (f32, f32) {
        let mode = self.p(18).round() as i32;
        let fast = mode == 3 || (self.p(27) >= 0.5 && self.ch.ctl.mod_wheel > 0.5);
        if mode == 1 && !fast {
            return (0.0, 0.0);
        }
        if fast {
            (self.p(20), self.p(22))
        } else {
            (self.p(19), self.p(21))
        }
    }
}

impl Plugin for TonewheelB3 {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, value: f32) {
        if self.params.set(id, value) {
            self.update();
        }
    }
    fn reset(&mut self) {
        self.keys = [false; 128];
        self.keys_down = 0;
        self.amp = [0.0; WHEELS];
        self.perc_amp = [0.0; WHEELS];
        self.perc_env = 0.0;
        self.click = 0.0;
        self.scan.clear();
        for d in self.horn_dl.iter_mut().chain(self.drum_dl.iter_mut()) {
            d.clear();
        }
        self.ch.reset();
        self.update();
    }
    fn tail(&self) -> f32 {
        0.3
    }
}

impl Instrument for TonewheelB3 {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return self.note_off(pitch);
        }
        self.key_down(pitch.min(127), velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        if self.ch.ctl.sustain {
            if self.keys[pitch.min(127) as usize] {
                self.sustained[pitch.min(127) as usize] = true;
            }
            return;
        }
        self.key_up(pitch.min(127));
    }
    fn controller(&mut self, cc: u8, value: u8) {
        if cc == 123 || cc == 120 {
            self.keys = [false; 128];
            self.keys_down = 0;
            self.recompute_targets();
            return;
        }
        if let Some(false) = self.ch.cc(cc, value) {
            for k in 0..128u8 {
                if self.sustained[k as usize] {
                    self.sustained[k as usize] = false;
                    self.key_up(k);
                }
            }
        }
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let rate = self.rate;
        let (gl, gr) = self.ch.out_gains();
        let vib = self.p(15).round() as usize;
        let (vdepth, vmix) = match vib {
            1 => (0.25, 1.0),
            2 => (0.5, 1.0),
            3 => (0.85, 1.0),
            4 => (0.25, 0.5),
            5 => (0.5, 0.5),
            6 => (0.85, 0.5),
            _ => (0.0, 0.0),
        };
        let vdepth = vdepth * 0.001 * rate;
        let drive = self.p(17);
        let rotary_on = self.p(18).round() as i32 != 0;
        let rotor_mix = self.p(29);
        let width = self.p(24);
        let accel = self.p(23);
        let (ht, dt) = self.rotor_targets();
        let leak = self.p(16);
        let perc_vol = lg_dsp::db_to_gain(self.p(12)) * 0.9;
        let bend = self.ch.ctl.bend_ratio;
        let any_key = self.keys_down > 0;
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            // rotor speeds follow their targets (the horn is lighter and gets there faster)
            let hc = dsp::tau_coef(0.6 * accel, rate / m as f32);
            let dc = dsp::tau_coef(2.5 * accel, rate / m as f32);
            self.horn.speed = ht + (self.horn.speed - ht) * hc;
            self.drum.speed = dt + (self.drum.speed - dt) * dc;
            let lt = if any_key { leak * 0.0025 } else { 0.0 };
            self.leak_level += (lt - self.leak_level) * 0.05;
            for i in 0..m {
                // generator
                let mut x = 0.0;
                let mut px = 0.0;
                let cc = self.contact_coef;
                for w in 1..WHEELS {
                    let a = self.target[w] + (self.amp[w] - self.target[w]) * cc;
                    self.amp[w] = dsp::flush(a);
                    let pa = self.perc_target[w] + (self.perc_amp[w] - self.perc_target[w]) * cc;
                    self.perc_amp[w] = dsp::flush(pa);
                    let ph = self.phase[w] + self.inc[w] * bend;
                    self.phase[w] = ph - ph.floor();
                    if a > 1e-6 || pa > 1e-6 || self.leak_level > 1e-6 {
                        let s = dsp::sin_cycles(self.phase[w]);
                        x += s * (a + self.leak_level);
                        px += s * pa;
                    }
                }
                x += px * self.perc_env * perc_vol;
                self.perc_env *= self.perc_coef;
                if self.click > 1e-5 {
                    x += self.click_f.run(self.rng.bipolar()) * self.click;
                    self.click *= 0.992;
                }
                // vibrato / chorus scanner
                self.scan.push(x);
                if vdepth > 0.0 {
                    let t = self.scan_phase;
                    let tri = if t < 0.5 {
                        4.0 * t - 1.0
                    } else {
                        3.0 - 4.0 * t
                    };
                    let wet = self.scan.read(vdepth * (0.5 + 0.5 * tri) + 1.0);
                    x = wet * vmix + x * (1.0 - vmix);
                    self.scan_phase += 6.9 / rate;
                    if self.scan_phase >= 1.0 {
                        self.scan_phase -= 1.0;
                    }
                }
                // tone and tube preamp
                x = self.tone_hi.run(self.tone_lo.run(x)) * self.gain;
                if drive > 0.0 {
                    let g = 1.0 + drive * 8.0;
                    x = dsp::soft_clip(x * g + 0.05 * drive) / g.sqrt() * (1.0 + drive);
                    x -= self.pre_hp.lp(x);
                }
                let (mut ol, mut or) = (x, x);
                if rotary_on {
                    let low = self.xover.lp(x);
                    let high = x - low;
                    let hw = self.horn.angle * std::f32::consts::TAU;
                    let dw = self.drum.angle * std::f32::consts::TAU;
                    let mut out = [0.0f32; 2];
                    for c in 0..2 {
                        let mic =
                            if c == 0 { -1.0 } else { 1.0 } * width * 0.25 * std::f32::consts::PI;
                        let hcos = (hw + mic).cos();
                        let dcos = (dw + mic).cos();
                        self.horn_dl[c].push(high);
                        self.drum_dl[c].push(low);
                        let hd = self.horn_dl[c].read((0.0018 + 0.0007 * hcos) * rate);
                        let dd = self.drum_dl[c].read((0.0022 + 0.0004 * dcos) * rate);
                        // horn is directional: louder and brighter facing the mic
                        let ha = 0.65 + 0.35 * hcos;
                        self.horn_tone[c].lowpass_hz(2500.0 + 3500.0 * (0.5 + 0.5 * hcos), rate);
                        let hs = self.horn_tone[c].lp(hd) * ha;
                        let ds = dd * (0.85 + 0.15 * dcos);
                        out[c] = hs * 1.1 + ds;
                    }
                    ol = x * (1.0 - rotor_mix) + out[0] * rotor_mix;
                    or = x * (1.0 - rotor_mix) + out[1] * rotor_mix;
                    self.horn.angle = (self.horn.angle + self.horn.speed / rate).fract();
                    self.drum.angle = (self.drum.angle + self.drum.speed / rate).fract();
                }
                left[pos + i] += ol * gl;
                right[pos + i] += or * gr;
            }
            pos += m;
        }
    }
}
