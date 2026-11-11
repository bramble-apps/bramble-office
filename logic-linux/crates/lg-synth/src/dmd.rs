//! Drum Machine Designer stand-in: 48 pads (C1–B4), each with its own sound — a synthesized drum
//! (any kit, any source note, its own tune/level/pan/decay/choke) or a sample the user drops on it
//! (Quick-Sampler style one-shot) — summed into one stereo out with a shared room.
//! The default layout follows Logic's "Drum Synth Kit" pad page (session-players/KIT-NOTES).

use std::sync::OnceLock;

use crate::drums::{Pad, StudioDrumKit};
use crate::quick::QuickSampler;
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const FIRST_PAD: u8 = 36;
pub const PADS: usize = 48;
pub const STYLES: &[&str] = &[
    "Drum Synth Kit",
    "Analog 808",
    "Analog 909",
    "Acoustic",
    "Electronic Pop",
];
const FIELDS: [&str; 7] = [
    "Kit",
    "Source Note",
    "Tune",
    "Level",
    "Pan",
    "Decay",
    "Choke Group",
];

/// Pad parameter id: 100 + pad × 10 + field.
pub fn pad_param(pad: usize, field: usize) -> u32 {
    (100 + pad * 10 + field) as u32
}

fn params() -> &'static [Param] {
    static P: OnceLock<Vec<Param>> = OnceLock::new();
    P.get_or_init(|| {
        let mut v = vec![
            Param::new(0, "Volume", -40.0, 12.0, 0.0, Unit::Db),
            Param::new(1, "Room", 0.0, 1.0, 0.15, Unit::Ratio),
            Param::menu(2, "Kit Style", STYLES, 0.0),
        ];
        let layout = default_layout(0);
        for k in 0..PADS {
            let note = FIRST_PAD as usize + k;
            let p = layout[note];
            for (f, name) in FIELDS.iter().enumerate() {
                let label: &'static str =
                    Box::leak(format!("Pad {} {}", note_name(note as u8), name).into_boxed_str());
                let id = pad_param(k, f);
                v.push(match f {
                    0 => Param::menu(id, label, crate::drums::KITS, p.kit as f32),
                    1 => Param::new(
                        id,
                        label,
                        0.0,
                        127.0,
                        if p.src == 0 {
                            note as f32
                        } else {
                            p.src as f32
                        },
                        Unit::None,
                    ),
                    2 => Param::new(id, label, -24.0, 24.0, p.tune, Unit::Semitones),
                    3 => Param::new(
                        id,
                        label,
                        -40.0,
                        12.0,
                        lg_dsp::gain_to_db(p.gain).max(-40.0),
                        Unit::Db,
                    ),
                    4 => Param::new(id, label, -1.0, 1.0, p.pan, Unit::Pan),
                    5 => Param::new(id, label, 0.1, 4.0, p.decay, Unit::Ratio),
                    _ => Param::new(id, label, 0.0, 8.0, p.choke as f32, Unit::None),
                });
            }
        }
        v
    })
}

fn note_name(n: u8) -> String {
    const N: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    // Logic naming: middle C (60) = C3
    format!("{}{}", N[n as usize % 12], n as i32 / 12 - 2)
}

/// The pads of a style. Style 0 = Logic's default "Drum Synth Kit" page layout.
pub fn default_layout(style: u8) -> [Pad; 128] {
    let mut p = [Pad::default(); 128];
    let kit = match style {
        1 => 4,
        2 => 5,
        3 => 0,
        _ => 6,
    };
    for (n, pad) in p.iter_mut().enumerate() {
        pad.kit = kit;
        pad.src = n as u8;
        if (42..=46).contains(&n) && n % 2 == 0 {
            pad.choke = 1;
        }
    }
    if style == 0 {
        // (note, kit, source note, tune) — sound names from the Drum Synth Kit page
        let map: &[(u8, u8, u8, f32)] = &[
            (36, 6, 36, 0.0),  // Kick 2
            (37, 6, 37, 0.0),  // Rim 1
            (38, 5, 38, 0.0),  // Snare 1
            (39, 6, 39, 0.0),  // Clap 1
            (40, 6, 38, 2.0),  // Snare 2
            (41, 6, 41, 0.0),  // Tom Low
            (42, 6, 42, 0.0),  // Hi-Hat 1
            (43, 6, 45, 0.0),  // Tom Mid
            (44, 6, 44, 0.0),  // Hi-Hat 2
            (45, 6, 48, 0.0),  // Tom High
            (46, 6, 46, 0.0),  // Hi-Hat 3
            (47, 4, 36, -3.0), // Kick Sub 1
            (48, 5, 36, 0.0),  // Kick 1
            (49, 6, 49, 0.0),  // Crash
            (50, 4, 37, 2.0),  // Rim 2
            (51, 6, 51, 0.0),  // Ride
            (52, 4, 40, 0.0),  // Snare 3
            (53, 4, 75, 0.0),  // Clave
            (54, 4, 42, 1.0),  // Hi-Hat 4
            (55, 5, 53, 5.0),  // Metal Hit
            (56, 5, 39, -2.0), // Clap 2
            (57, 4, 56, 0.0),  // Cowbell
            (58, 5, 56, -5.0), // Warped Cowbell
            (59, 4, 36, -6.0), // Kick Sub 2
            (60, 0, 36, 0.0),  // Kick 3
            (61, 6, 64, -5.0), // Deep Perc
            (62, 6, 82, 0.0),  // Shaker
        ];
        for &(n, k, src, t) in map {
            p[n as usize] = Pad {
                kit: k,
                src,
                tune: t,
                ..p[n as usize]
            };
        }
        p[54].choke = 1;
    }
    p
}

pub struct DrumMachineDesigner {
    params: crate::poly::Params,
    drums: StudioDrumKit,
    /// A pad with a sample plays it through its own Quick Sampler (One Shot by default).
    quick: Vec<Option<Box<QuickSampler>>>,
    /// Pad Mute / Solo (by note).
    pub muted: [bool; 128],
    pub soloed: [bool; 128],
    rate: f32,
    /// The editor's revision the pads came from.
    pub doc_rev: u64,
}

impl DrumMachineDesigner {
    pub fn new(rate: f32) -> DrumMachineDesigner {
        let mut d = DrumMachineDesigner {
            params: crate::poly::Params::new(params()),
            drums: StudioDrumKit::new(rate),
            quick: (0..128).map(|_| None).collect(),
            muted: [false; 128],
            soloed: [false; 128],
            rate,
            doc_rev: 0,
        };
        d.apply();
        d
    }
    pub fn with_style(rate: f32, style: u8) -> DrumMachineDesigner {
        let mut d = DrumMachineDesigner::new(rate);
        d.set(2, style as f32);
        d
    }
    fn apply(&mut self) {
        let mut pads = [Pad::default(); 128];
        for k in 0..PADS {
            let note = FIRST_PAD as usize + k;
            let g = |f| self.params.get(pad_param(k, f));
            pads[note] = Pad {
                kit: g(0).round() as u8,
                src: g(1).round() as u8,
                tune: g(2),
                gain: lg_dsp::db_to_gain(g(3)),
                pan: g(4),
                decay: g(5),
                choke: g(6).round() as u8,
            };
        }
        // notes outside the 48 pads play the style's GM sound
        let style = default_layout(self.params.get(2).round() as u8);
        for n in 0..128 {
            if !(FIRST_PAD as usize..FIRST_PAD as usize + PADS).contains(&n) {
                pads[n] = style[n];
            }
        }
        self.drums.set_pads(&pads);
        self.drums.set(1, self.params.get(0));
        self.drums.set(18, self.params.get(1));
        self.drums.set(20, 0.4);
    }
    /// Puts a sample on a pad: its own Quick Sampler, One Shot, at the pad's key.
    pub fn set_pad_sample(&mut self, note: u8, audio: &crate::wav::Audio) {
        let mut q = QuickSampler::new(self.rate);
        q.set_audio(audio.clone());
        q.set(0, 1.0);
        q.set(1, note as f32);
        q.set(15, 0.0);
        self.quick[note as usize & 127] = Some(Box::new(q));
    }
    /// A pad's Quick Sampler as it is set up (the pad editor).
    pub fn set_pad_quick(&mut self, note: u8, q: QuickSampler) {
        self.quick[note as usize & 127] = Some(Box::new(q));
    }
    pub fn pad_quick(&self, note: u8) -> Option<&QuickSampler> {
        self.quick.get(note as usize)?.as_deref()
    }
    pub fn pad_quick_mut(&mut self, note: u8) -> Option<&mut QuickSampler> {
        self.quick.get_mut(note as usize)?.as_deref_mut()
    }
    pub fn clear_pad_sample(&mut self, note: u8) {
        self.quick[note as usize & 127] = None;
    }
    /// The pad's choke (exclusive) group.
    pub fn pad_choke(&self, note: u8) -> u8 {
        let k = note as usize;
        if (FIRST_PAD as usize..FIRST_PAD as usize + PADS).contains(&k) { self.params.get(pad_param(k - FIRST_PAD as usize, 6)).round() as u8 } else { 0 }
    }
    /// Resample Pad: the pad's sound now (its drum or sample with its tuning), rendered into a
    /// sample that the pad then plays.
    pub fn resample_pad(&mut self, note: u8) -> crate::wav::Audio {
        let rate = self.rate;
        let mut tmp = DrumMachineDesigner::new(rate);
        for p in params() {
            tmp.params.set(p.id, self.params.get(p.id));
        }
        tmp.apply();
        if let Some(q) = self.pad_quick(note) {
            tmp.quick[note as usize] = Some(Box::new(q.duplicate()));
        }
        tmp.note_on(note, 127);
        let mut l = Vec::new();
        let mut r = Vec::new();
        let block = 256;
        let (mut bl, mut br) = (vec![0.0f32; block], vec![0.0f32; block]);
        let mut quiet = 0;
        for i in 0..(rate as usize * 8 / block) {
            bl.fill(0.0);
            br.fill(0.0);
            tmp.render(&mut bl, &mut br);
            if i == (rate as usize / 10) / block {
                tmp.note_off(note);
            }
            let pk = bl.iter().chain(br.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
            l.extend_from_slice(&bl);
            r.extend_from_slice(&br);
            quiet = if pk < 1e-4 { quiet + 1 } else { 0 };
            if quiet * block > rate as usize / 10 && i > 4 {
                break;
            }
        }
        let a = crate::wav::Audio { rate: rate as u32, channels: vec![l, r], ..Default::default() };
        self.set_pad_sample(note, &a);
        a
    }
    fn audible(&self, p: u8) -> bool {
        let k = p as usize & 127;
        !self.muted[k] && (!self.soloed.iter().any(|s| *s) || self.soloed[k])
    }
}

impl Plugin for DrumMachineDesigner {
    fn params(&self) -> &[Param] {
        params()
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            if id == 2 {
                // a style re-lays every pad
                let layout = default_layout(v.round() as u8);
                for k in 0..PADS {
                    let note = FIRST_PAD as usize + k;
                    let p = layout[note];
                    self.params.set(pad_param(k, 0), p.kit as f32);
                    self.params.set(
                        pad_param(k, 1),
                        if p.src == 0 {
                            note as f32
                        } else {
                            p.src as f32
                        },
                    );
                    self.params.set(pad_param(k, 2), p.tune);
                    self.params.set(pad_param(k, 3), 0.0);
                    self.params.set(pad_param(k, 4), 0.0);
                    self.params.set(pad_param(k, 5), 1.0);
                    self.params.set(pad_param(k, 6), p.choke as f32);
                }
            }
            self.apply();
        }
    }
    fn reset(&mut self) {
        self.drums.reset();
        for q in self.quick.iter_mut().flatten() {
            q.reset();
        }
        self.apply();
    }
    fn tail(&self) -> f32 {
        3.0
    }
}

impl Instrument for DrumMachineDesigner {
    fn note_on(&mut self, p: u8, v: u8) {
        if v == 0 {
            return self.note_off(p);
        }
        if !self.audible(p) {
            return;
        }
        // an exclusive group: this pad stops the others in it (samples and drums alike)
        let g = self.pad_choke(p);
        if g != 0 {
            for n in 0..128u8 {
                if n != p && self.pad_choke(n) == g {
                    if let Some(q) = self.quick[n as usize].as_mut() {
                        q.choke();
                    }
                }
            }
            self.drums.choke_group(g, p);
        }
        match self.quick[p as usize & 127].as_mut() {
            Some(q) => q.note_on(p, v),
            None => self.drums.note_on(p, v),
        }
    }
    fn note_off(&mut self, p: u8) {
        if let Some(q) = self.quick[p as usize & 127].as_mut() {
            q.note_off(p)
        }
    }
    fn controller(&mut self, cc: u8, v: u8) {
        self.drums.controller(cc, v);
        for q in self.quick.iter_mut().flatten() {
            q.controller(cc, v);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32]) {
        let _ = self.rate;
        self.drums.render(l, r);
        for q in self.quick.iter_mut().flatten() {
            q.render(l, r);
        }
    }
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}
