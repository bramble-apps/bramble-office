//! ES2's Tutorial Settings (the Settings pop-up's Tutorial Settings folder): Studio's own
//! settings, each built to the user guide's description of the tutorial example (the waves,
//! the oscillator mix, the router channels, the Vector Envelope), on top of Analog Saw Init.

use crate::es2::{MM_SOURCES, MM_TARGETS, PARAMS};

/// The tutorial settings in the folder's order.
pub const NAMES: &[&str] = &[
    "Analog Saw Init",
    "Analog Bass Clean",
    "Analog Bass Distorted",
    "Analog Saw 3 Osc",
    "Analog Saw Unison",
    "FM Start",
    "FM Envelope",
    "FM Drive",
    "FM Digiwave",
    "FM Megafat",
    "FM Out of Tune",
    "Sync Start",
    "PWM Start",
    "PWM Slow",
    "PWM Scaled",
    "PWM 2 Osc",
    "PWM Soft Strings",
    "Ringmod Start",
    "Crescendo Brass",
    "MW-Pad-Creator",
    "Slapped StratENV",
    "Vector Start",
    "Vector Envelope",
    "Vector Loop",
    "Vector Kick",
    "Vector Perc Synth",
    "Wheelrocker",
    "Wheelsyncer",
];

/// A router channel's target / source index by name.
pub fn target(n: &str) -> f32 {
    MM_TARGETS.iter().position(|t| *t == n).unwrap_or_else(|| panic!("ES2 target {n}")) as f32
}
pub fn source(n: &str) -> f32 {
    MM_SOURCES.iter().position(|t| *t == n).unwrap_or_else(|| panic!("ES2 source {n}")) as f32
}

/// Router channel `ch` (1–10)'s parameter ids: target, source, via, intensity, via intensity.
pub fn mm_ids(ch: u32) -> [u32; 5] {
    let b = 41 + (ch - 1) * 5;
    [b, b + 1, b + 2, b + 3, b + 4]
}

struct S(Vec<(u32, f32)>);

impl S {
    fn put(&mut self, id: u32, v: f32) -> &mut Self {
        match self.0.iter_mut().find(|e| e.0 == id) {
            Some(e) => e.1 = v,
            None => self.0.push((id, v)),
        }
        self
    }
    /// Router channel `ch`: source → target (via, with the intensity range).
    fn mm(&mut self, ch: u32, src: &str, tgt: &str, via: &str, int: f32, via_int: f32) -> &mut Self {
        let [t, s, v, i, vi] = mm_ids(ch);
        self.put(t, target(tgt)).put(s, source(src)).put(v, source(via)).put(i, int).put(vi, via_int)
    }
    /// The Triangle: only these oscillators heard (1, 2, 3), equally.
    fn mix(&mut self, oscs: &[u32]) -> &mut Self {
        let has = |o| oscs.contains(&o);
        let (x, y) = match (has(1), has(2), has(3)) {
            (true, false, false) => (0.5, 1.0),
            (false, true, false) => (0.0, 0.0),
            (false, false, true) => (1.0, 0.0),
            (true, true, false) => (0.0, 0.5),
            (true, false, true) => (1.0, 0.5),
            (false, true, true) => (0.5, 0.0),
            _ => (0.5, 1.0 / 3.0),
        };
        self.put(26, x).put(27, y)
    }
}

/// LFO 2 Rate for a synced note value (e.g. "2 bars").
pub fn lfo2_sync(value: &str) -> f32 {
    let s = crate::synthkit::SYNC;
    let k = s.iter().position(|e| e.0 == value).unwrap_or(0);
    -1.0 + (k as f32 + 0.5) / s.len() as f32
}

/// A tutorial setting's full parameter list (every ES2 parameter), or None.
pub fn setting(name: &str) -> Option<Vec<(u32, f32)>> {
    if !NAMES.contains(&name) {
        return None;
    }
    let mut s = S(PARAMS.iter().map(|p| (p.id, p.default)).collect());
    // Analog Saw Init: one unfiltered sawtooth (oscillator 1), the amplitude envelope an organ
    // gate, no modulation; Env 2 pre-routed to the filters (at no intensity) as the filter envelope
    s.put(14, 2.0).put(16, 0.0).put(21, 0.0).mix(&[1]);
    s.put(32, 1.0).put(33, 0.0).put(36, 1.0).put(37, 0.0).put(3, 0.0);
    s.put(108, 0.0).put(109, 0.0).put(110, 0.0).put(112, 1.0).put(113, 0.1);
    for ch in 1..=10 {
        let [t, sr, v, i, vi] = mm_ids(ch);
        s.put(t, 0.0).put(sr, 0.0).put(v, 0.0).put(i, 0.0).put(vi, 0.0);
    }
    s.mm(1, "ENV2", "Cut 1+2", "Off", 0.0, 0.0);
    match name {
        "Analog Bass Clean" => {
            // one rectangle an octave down through Filter 2 (Blend fully right); legato glides
            s.put(14, 3.0).put(12, -12.0).put(30, 1.0).put(36, 0.3).put(37, 0.2);
            s.mm(1, "ENV2", "Cutoff 2", "Off", 0.35, 0.0);
            s.put(103, 0.35).put(105, 0.2);
            s.put(1, 1.0).put(8, 2.0).put(7, 0.25);
        }
        "Analog Bass Distorted" => {
            // Filter 1 with high Drive and Distortion
            s.put(14, 2.0).put(12, -12.0).put(30, -1.0).put(31, 0.0).put(32, 0.35).put(33, 0.35);
            s.put(28, 0.85).put(117, 0.75).put(116, 0.0);
            s.mm(1, "ENV2", "Cutoff 1", "Off", 0.4, 0.0);
            s.put(103, 0.3).put(105, 0.15);
            s.put(1, 1.0).put(8, 1.0);
        }
        "Analog Saw 3 Osc" => {
            s.put(19, 2.0).put(16, 1.0).put(24, 2.0).put(21, 1.0).mix(&[1, 2, 3]);
            s.put(13, -9.0).put(18, 8.0).put(22, 0.0).put(23, 3.0).put(3, 0.3);
        }
        "Analog Saw Unison" => {
            // three heavily detuned, unfiltered saws in monophonic Unison; Filter 2 Cutoff at 0
            // brings in the preset filter envelope
            s.put(19, 2.0).put(16, 1.0).put(24, 2.0).put(21, 1.0).mix(&[1, 2, 3]);
            s.put(13, -35.0).put(18, 33.0).put(22, 0.0).put(23, -18.0).put(3, 0.6);
            s.put(1, 1.0).put(8, 1.0).put(9, 1.0);
            s.put(30, 1.0);
            s.mm(1, "ENV2", "Cutoff 2", "Off", 0.75, 0.0);
            s.put(103, 0.55).put(105, 0.25);
        }
        "FM Start" | "FM Envelope" | "FM Drive" | "FM Digiwave" | "FM Megafat" | "FM Out of Tune" => {
            // oscillator 1 a sine (the carrier), oscillator 2 a sine (the modulator) at level 0
            s.put(14, 0.0).put(19, 0.0).put(16, 1.0).mix(&[1]).put(190, 0.0);
            match name {
                "FM Envelope" => {
                    s.put(14, 4.0).put(190, 0.25);
                    s.mm(1, "ENV2", "Osc1Wave", "Off", 0.5, 0.0);
                    s.put(103, 0.45).put(105, 0.1);
                }
                "FM Drive" => {
                    s.put(14, 4.0).put(190, 0.35).put(28, 0.6).put(36, 0.4);
                    s.mm(1, "ENV2", "Cutoff 2", "Off", 0.5, 0.0);
                }
                "FM Digiwave" => {
                    // a Digiwave modulates; polyphonic Unison; LFO 2 morphs oscillator 2's
                    // Digiwaves at a tempo-synced two-bar rate
                    s.put(14, 4.0).put(190, 0.4).put(20, 24.0);
                    s.put(9, 1.0).put(8, 0.0).put(3, 0.3);
                    s.mm(2, "LFO2", "Osc2Wave", "Off", 0.3, 0.0);
                    s.put(94, 0.0).put(95, lfo2_sync("2 bars"));
                }
                "FM Megafat" => {
                    s.put(14, 4.0).put(190, 0.7).put(24, 2.0).put(21, 1.0).put(22, -12.0);
                    s.put(26, 1.0).put(27, 0.6).put(9, 1.0).put(3, 0.6);
                }
                "FM Out of Tune" => {
                    s.put(14, 4.0).put(190, 0.3).put(12, 30.0).put(17, 0.0);
                }
                _ => {}
            }
        }
        "Sync Start" => {
            // only oscillator 2 (synced sawtooth); oscillator 3 off; the mod wheel on its pitch,
            // router channel 2: Env 1 → Pitch 2
            s.put(19, 5.0).put(16, 1.0).put(21, 0.0).mix(&[2]);
            s.mm(1, "ModWhl", "Pitch 2", "Off", 0.6, 0.0);
            s.mm(2, "ENV1", "Pitch 2", "Off", 0.35, 0.0);
            s.put(100, 0.35);
        }
        "PWM Start" | "PWM Slow" | "PWM Scaled" | "PWM 2 Osc" | "PWM Soft Strings" => {
            s.put(19, 3.0).put(16, 1.0).mix(&[2]).put(191, 0.0);
            if name != "PWM Start" {
                // LFO 1 drives the pulse width (rate 0.230); router channel 2: Kybd → LFO1 Rate
                s.put(91, 0.0).put(92, 0.23);
                s.mm(1, "LFO1", "Osc2Wave", "Off", 0.45, 0.0);
                s.mm(2, "Kybd", "LFO1Rate", "Off", 0.3, 0.0);
            }
            if name == "PWM Scaled" {
                // the pulse width modulation scaled across the keyboard
                s.mm(1, "LFO1", "Osc2Wave", "Kybd", 0.15, 0.6);
            }
            if name == "PWM 2 Osc" || name == "PWM Soft Strings" {
                s.put(24, 3.0).put(21, 1.0).put(23, 6.0).mix(&[2, 3]);
                s.mm(3, "LFO2", "Osc3Wave", "Off", 0.4, 0.0);
                s.put(95, 0.3);
            }
            if name == "PWM Soft Strings" {
                s.put(108, 0.55).put(109, 0.45).put(113, 0.6).put(36, 0.55);
                s.put(119, 0.0).put(121, 0.5);
            }
        }
        "Ringmod Start" => {
            s.put(14, 0.0).put(19, 6.0).put(16, 1.0).put(17, 7.0).mix(&[2]);
        }
        "Crescendo Brass" => {
            // saw + near-square pulse + a Digiwave
            s.put(14, 2.0).put(19, 3.0).put(16, 1.0).put(191, 0.08).put(24, 0.0).put(25, 30.0).put(21, 1.0).mix(&[1, 2, 3]);
            s.put(36, 0.45).put(32, 0.8);
            s.put(100, 0.25);
            s.mm(1, "ENV2", "Cut 1+2", "Off", 0.4, 0.0);
            // 3: Env 1's decay moves oscillator 3's wave (a short growl)
            s.mm(3, "ENV1", "Osc3Wave", "Off", 0.35, 0.0);
            // 4: LFO 1 → oscillator 2 pulse width, scaled by the keyboard
            s.mm(4, "LFO1", "Osc2Wave", "Kybd", 0.05, 0.4);
            // 5–6: Env 1 shifts oscillator 2's pitch against oscillator 3 in the attack
            s.mm(5, "ENV1", "Pitch 2", "Off", 0.12, 0.0);
            s.mm(6, "ENV1", "Pitch 3", "Off", -0.12, 0.0);
            // 7–8: the mod wheel: a crescendo, plus LFO 2 pitch modulation
            s.mm(7, "ModWhl", "Amp", "Off", 0.6, 0.0);
            s.mm(8, "LFO2", "Pitch123", "ModWhl", 0.0, 0.12);
            // 9: aftertouch closes the filters (a decrescendo)
            s.mm(9, "Touch", "Cut 1+2", "Off", -0.45, 0.0);
            s.put(108, 0.3).put(109, 0.2);
        }
        "MW-Pad-Creator" => {
            // oscillators 1 and 3 start on Digiwaves, oscillator 2 PWM for an ensemble effect
            s.put(14, 0.0).put(15, 10.0).put(19, 3.0).put(16, 1.0).put(24, 0.0).put(25, 46.0).put(21, 1.0).mix(&[1, 2, 3]);
            s.mm(2, "LFO1", "Osc2Wave", "Off", 0.35, 0.0);
            s.put(92, 0.6);
            // 3: the mod wheel scans oscillators 1 and 3's wavetables and moves 2's pulse width
            s.mm(3, "ModWhl", "OscWaves", "Off", 0.6, 0.0);
            // 4: Lowpass Filter FM on Filter 2, increasing with the mod wheel
            s.mm(4, "ModWhl", "LPF FM", "Off", 0.5, 0.0);
            // 9: aftertouch opens the cutoff slightly; 10: aftertouch vibrato
            s.mm(9, "Touch", "Cut 1+2", "Off", 0.12, 0.0);
            s.mm(10, "LFO2", "Pitch123", "Touch", 0.0, 0.15);
            s.put(108, 0.6).put(109, 0.5).put(113, 0.65).put(36, 0.5);
        }
        "Slapped StratENV" => {
            // Digiwaves on 1 and 3, a synced wave on 2
            s.put(14, 0.0).put(15, 33.0).put(19, 5.0).put(16, 1.0).put(24, 0.0).put(25, 61.0).put(21, 1.0).mix(&[1, 2, 3]);
            // Env 1's decay pushes the wave selectors briefly (a punchy attack)
            s.put(100, 0.15);
            s.mm(1, "ENV1", "OscWaves", "Off", 0.3, 0.0);
            // Env 2 on the filter with a slight attack, velocity very responsive
            s.mm(2, "ENV2", "Cutoff 2", "Off", 0.45, 0.0);
            s.put(101, 0.08).put(102, 0.05).put(103, 0.4).put(105, 0.2).put(107, 0.9).put(36, 0.4);
            s.mm(4, "Velo", "Cutoff 2", "Off", 0.6, 0.0);
            // LFO 2 vibrato on the mod wheel and aftertouch
            s.mm(3, "LFO2", "Pitch123", "Whl+To", 0.0, 0.15);
        }
        "Vector Start" | "Vector Envelope" | "Vector Loop" | "Vector Kick" | "Vector Perc Synth" => {
            s.put(19, 2.0).put(16, 1.0).put(24, 3.0).put(21, 1.0);
            s.put(142, 1.0).put(134, 0.0);
            // three points: start, sustain, release target
            let mut pts = vec![[0.0, 1.0, 1.0, 0.0, 0.0], [0.498, 0.0, 0.0, 0.0, 0.0], [0.5, 1.0, 0.0, 0.0, 0.0]];
            let (mut sus, mut lp) = (1usize, 0usize);
            match name {
                "Vector Envelope" => {
                    // the Planar Pad: X → Filter 2 cutoff, Y → Pan, both at 0.50
                    s.put(142, 3.0).put(124, target("Cutoff 2")).put(125, 0.5).put(126, target("Pan")).put(127, 0.5);
                    pts[1][3] = -1.0;
                    pts[2][4] = 1.0;
                }
                "Vector Loop" => {
                    // oscillator 1 FM from oscillator 2's wavetable, 2 crossfading Digiwaves
                    // (LFO 2), 3 PWM (LFO 1); Unison and Analog; a slow forward loop over five
                    // points, each segment 25 %, Loop Rate 0.09
                    s.put(14, 4.0).put(190, 0.3).put(19, 0.0).put(20, 12.0).put(24, 3.0);
                    s.mm(2, "LFO2", "Osc2Wave", "Off", 0.4, 0.0);
                    s.mm(3, "LFO1", "Osc3Wave", "Off", 0.4, 0.0);
                    s.put(9, 1.0).put(3, 0.5).put(132, 0.09).put(138, 1.0);
                    pts = vec![
                        [0.0, 1.0, 1.0, 0.0, 0.0],
                        [0.25, 0.0, 0.0, 0.0, 0.0],
                        [0.25, 1.0, 0.0, 0.0, 0.0],
                        [0.25, 0.5, 0.5, 0.0, 0.0],
                        [0.25, 1.0, 1.0, 0.0, 0.0],
                        [0.5, 0.5, 0.34, 0.0, 0.0],
                    ];
                    sus = 4;
                    lp = 0;
                }
                "Vector Kick" => {
                    // a short pitch / filter sweep: Time 1>2 9.0 ms, 2>3 303 ms
                    s.put(4, 1.0).put(37, 0.85).put(36, 0.3).put(110, 0.5).put(112, 0.0);
                    pts[1][0] = 0.009;
                    pts[2][0] = 0.303;
                }
                "Vector Perc Synth" => {
                    pts[1][0] = 0.02;
                    pts[2][0] = 0.25;
                    s.put(110, 0.45).put(112, 0.0);
                }
                _ => {}
            }
            for (id, v) in crate::es2::vec_store(&pts, sus, lp, 0) {
                s.put(id, v);
            }
        }
        "Wheelrocker" => {
            // an organ from three oscillators; the mod wheel speeds up the "rotor"
            s.put(14, 0.0).put(19, 0.0).put(16, 1.0).put(17, 12.0).put(24, 0.0).put(21, 1.0).put(22, 19.0).mix(&[1, 2, 3]);
            s.put(112, 1.0).put(113, 0.05).put(92, 1.0);
            s.mm(1, "ENV2", "Cutoff 1", "Kybd", 0.2, 0.4);
            s.put(103, 0.08).put(105, 0.0);
            s.mm(2, "LFO1", "Pitch 2", "Off", 0.05, 0.0);
            s.mm(3, "LFO1", "Pitch 3", "Off", -0.05, 0.0);
            s.mm(4, "ENV1", "OscWaves", "Off", 0.2, 0.0);
            s.put(100, 0.1);
            s.mm(5, "ModWhl", "Amp", "Off", -0.2, 0.0);
            s.mm(6, "ModWhl", "Pitch 2", "Off", 0.04, 0.0);
            s.mm(7, "ModWhl", "Pitch 3", "Off", -0.04, 0.0);
            s.mm(8, "LFO1", "Pan", "Off", 0.6, 0.0);
            s.mm(9, "ModWhl", "LFO1Rate", "Off", 0.7, 0.0);
            s.mm(10, "ModWhl", "Cutoff 1", "Off", 0.25, 0.0);
        }
        "Wheelsyncer" => {
            // a sync lead: only oscillator 2 (synced, 3 semitones down); oscillator 1 off
            s.put(11, 0.0).put(19, 5.0).put(16, 1.0).put(17, -3.0).put(21, 0.0).mix(&[2]);
            s.mm(6, "ModWhl", "Cut 1+2", "Off", 0.4, 0.0);
            s.mm(7, "ModWhl", "Pitch 2", "Off", 1.0, 0.0);
            s.mm(8, "LFO1", "Pan", "ModWhl", 0.0, 0.7);
            s.mm(9, "ModWhl", "LFO1Rate", "Off", 0.6, 0.0);
            s.put(32, 0.7).put(36, 0.7);
        }
        _ => {}
    }
    Some(s.0)
}
