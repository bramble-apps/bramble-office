//! "Studio Drum Kit": synthesized drum kits on the General MIDI / Drum Kit Designer note map
//! (the Drummer plays through this). Acoustic kits are modelled (membrane modes with pitch glide,
//! beater/stick transients, snare wires, inharmonic cymbal partials and wash); electronic kits
//! follow the classic analog drum-machine circuits (bridged-T kick, two-tone snare, six square
//! oscillators for the metal sounds). Every hit has velocity layers (timbre changes, not only
//! level) and round-robin variation so repeated notes never sound machine-identical.
//!
//! Note map (MIDI): 35/36 kick, 37 side stick, 38 snare, 39 clap, 40 rimshot/snare 2,
//! 41/43/45/47/48/50 toms, 42 closed hat, 44 pedal hat, 46 open hat, 49/57 crash, 51/59 ride,
//! 52 china, 53 ride bell, 54 tambourine, 55 splash, 56 cowbell, 60-86 GM percussion; below 35
//! the Drum Kit Designer hi-hat articulations (33/34 semi-open, others closed/edge).

use crate::dsp::{self, CHUNK, Fade, Mode, Rng, Room, Svf, SvfMode};
use crate::poly::{Channel, Ctl, Params, Poly, Voice};
use lg_dsp::{Instrument, Param, Plugin, Unit};

pub const KITS: &[&str] = &[
    "Studio Kit",
    "Vintage Kit",
    "Heavy Kit",
    "Brush Kit",
    "Analog 808",
    "Analog 909",
    "Electronic Pop",
];

pub static PARAMS: &[Param] = &[
    Param::menu(0, "Kit", KITS, 0.0),
    Param::new(1, "Volume", -40.0, 12.0, 0.0, Unit::Db),
    Param::new(2, "Kick Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(3, "Kick Tune", -12.0, 12.0, 0.0, Unit::Semitones),
    Param::new(4, "Kick Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(5, "Snare Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(6, "Snare Tune", -12.0, 12.0, 0.0, Unit::Semitones),
    Param::new(7, "Snare Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(8, "Snare Wires", 0.0, 2.0, 1.0, Unit::Ratio),
    Param::new(9, "Toms Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(10, "Toms Tune", -12.0, 12.0, 0.0, Unit::Semitones),
    Param::new(11, "Toms Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(12, "Hi-Hat Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(13, "Hi-Hat Tone", -1.0, 1.0, 0.0, Unit::Ratio),
    Param::new(14, "Hi-Hat Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(15, "Cymbals Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(16, "Cymbals Decay", 0.2, 3.0, 1.0, Unit::Ratio),
    Param::new(17, "Percussion Level", -24.0, 12.0, 0.0, Unit::Db),
    Param::new(18, "Room", 0.0, 1.0, 0.25, Unit::Ratio),
    Param::new(19, "Velocity Sensitivity", 0.0, 1.0, 0.85, Unit::Ratio),
    Param::new(20, "Variation", 0.0, 1.0, 0.5, Unit::Ratio),
    Param::new(21, "Stereo Width", 0.0, 1.0, 0.6, Unit::Ratio),
    Param::new(22, "Tune", -12.0, 12.0, 0.0, Unit::Semitones),
    Param::new(23, "Punch", 0.0, 1.0, 0.3, Unit::Ratio),
];

/// Which mixer group a piece belongs to (for the level/tune/decay knobs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Group {
    #[default]
    Kick,
    Snare,
    Tom,
    HiHat,
    Cymbal,
    Perc,
}

const MAXT: usize = 4;
const MAXM: usize = 24;

/// A hit, fully described: built at note-on, rendered by a generic drum voice.
#[derive(Clone, Copy, Debug, Default)]
pub struct Hit {
    pub group: Group,
    pub choke: u8,
    pub pan: f32,
    pub gain: f32,
    /// Tonal components: (start Hz, end Hz, glide time constant s, amplitude, T60 s).
    pub tones: [(f32, f32, f32, f32, f32); MAXT],
    pub ntones: usize,
    /// Noise: amplitude, T60, filter mode, centre/cutoff Hz, Q, attack s.
    pub noise: (f32, f32, u8, f32, f32, f32),
    /// Second noise layer (e.g. wash / tail) with the same layout.
    pub noise2: (f32, f32, u8, f32, f32, f32),
    /// Transient click: amplitude, T60, band-pass centre Hz.
    pub click: (f32, f32, f32),
    /// Metal (six square oscillators): amplitude, T60, base multiplier, band-pass centre Hz, high-pass Hz.
    pub metal: (f32, f32, f32, f32, f32),
    /// Modal partials (cymbals, bells, wood): count, lowest Hz, highest Hz, amplitude, T60, spread seed.
    pub modes: (usize, f32, f32, f32, f32, u32),
    /// Repeats of the noise burst (claps): count, spacing s.
    pub bursts: (u8, f32),
    pub drive: f32,
    /// Extra tuning in semitones and decay multiplier (Drum Machine Designer pads; 0 = 1).
    pub tune: f32,
    pub decay: f32,
    /// Extra low-pass on the noise layer, Hz (0 = none): darkens snare wires.
    pub noise_lp: f32,
    /// The kit the hit came from (output trim).
    pub kit: u8,
}

/// A Drum Machine Designer pad: which kit and source note it plays, and its own tuning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pad {
    pub kit: u8,
    pub src: u8,
    pub tune: f32,
    pub gain: f32,
    pub pan: f32,
    pub decay: f32,
    pub choke: u8,
}

impl Default for Pad {
    fn default() -> Pad {
        Pad {
            kit: 6,
            src: 0,
            tune: 0.0,
            gain: 1.0,
            pan: 0.0,
            decay: 1.0,
            choke: 0,
        }
    }
}

/// The analog machine's six metal oscillator frequencies (Hz).
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

fn noise(a: f32, t: f32, mode: u8, f: f32, q: f32) -> (f32, f32, u8, f32, f32, f32) {
    (a, t, mode, f, q, 0.0)
}

/// Builds the hit for a note on a kit. `v` = velocity 0..1, `rr` = round-robin random −1..1 values.
pub fn build_hit(kit: u8, note: u8, v: f32, rr: [f32; 4]) -> Option<Hit> {
    let mut h = Hit {
        gain: 1.0,
        ..Default::default()
    };
    let elec = kit >= 4;
    let brush = kit == 3;
    let heavy = kit == 2;
    let vintage = kit == 1;
    let var = |x: f32, amt: f32| x * (1.0 + amt * rr[0]);
    let push = |h: &mut Hit, t: (f32, f32, f32, f32, f32)| {
        if h.ntones < MAXT {
            h.tones[h.ntones] = t;
            h.ntones += 1;
        }
    };
    // DKD hi-hat articulations below 35
    let note = if note < 35 {
        match note {
            33 | 34 => 200, // semi-open (internal code)
            26 | 30 => 46,
            _ => 42,
        }
    } else {
        note
    };
    match note {
        // ------------------------------------------------------------------ kicks
        35 | 36 => {
            h.group = Group::Kick;
            let alt = note == 35;
            match kit {
                4 => {
                    // 808: a decaying bridged-T resonance, slight pitch drop
                    push(
                        &mut h,
                        (var(58.0, 0.01) * 1.6, var(52.0, 0.01), 0.012, 1.0, 1.1),
                    );
                    h.click = (0.15 + 0.15 * v, 0.006, 1500.0);
                    h.gain = 0.9;
                }
                5 => {
                    // 909: fast sweep down, noisy click
                    push(
                        &mut h,
                        (var(220.0, 0.02), var(54.0, 0.01), 0.022, 1.0, 0.55),
                    );
                    h.click = (0.35 + 0.3 * v, 0.008, 3500.0);
                    h.noise = noise(0.12 * v, 0.02, 1, 6000.0, 0.5);
                    h.drive = 0.4;
                    h.gain = 0.85;
                }
                6 => {
                    push(&mut h, (var(160.0, 0.02), var(50.0, 0.01), 0.03, 1.0, 0.7));
                    h.click = (0.3 + 0.2 * v, 0.01, 2500.0);
                    h.drive = 0.5;
                    h.gain = 0.85;
                }
                _ => {
                    // acoustic: shell fundamental, head overtone, beater click, port air
                    let f = if alt {
                        52.0
                    } else if heavy {
                        50.0
                    } else if vintage {
                        62.0
                    } else {
                        56.0
                    };
                    let f = var(f, 0.015);
                    let snap = 0.3 + 0.7 * v;
                    push(
                        &mut h,
                        (f * 1.9, f, 0.012, 1.0, if vintage { 0.45 } else { 0.32 }),
                    );
                    push(&mut h, (f * 1.6 * 1.3, f * 1.6, 0.01, 0.35, 0.12));
                    push(&mut h, (f * 2.3, f * 2.25, 0.02, 0.12, 0.08));
                    h.click = (
                        if brush { 0.2 } else { 0.5 + 0.6 * v * v } * snap,
                        0.012,
                        if heavy { 4500.0 } else { 3200.0 },
                    );
                    h.noise = noise(0.05 + 0.08 * v, 0.06, 0, 700.0, 0.7);
                    h.drive = if heavy { 0.6 } else { 0.25 };
                    h.gain = 1.3;
                }
            }
        }
        // ------------------------------------------------------------------ snares
        38 | 40 => {
            h.group = Group::Snare;
            let rim = note == 40;
            match kit {
                4 => {
                    push(&mut h, (var(238.0, 0.01), 238.0, 0.01, 0.6, 0.12));
                    push(&mut h, (var(476.0, 0.01), 476.0, 0.01, 0.35, 0.07));
                    h.noise = noise(0.55 + 0.25 * v, 0.17, 2, 1800.0, 0.6);
                    h.gain = 0.6;
                }
                5 => {
                    push(&mut h, (var(260.0, 0.01), 190.0, 0.015, 0.6, 0.11));
                    push(&mut h, (var(400.0, 0.01), 340.0, 0.015, 0.3, 0.06));
                    h.noise = noise(0.75 + 0.2 * v, 0.2, 2, 2500.0, 0.6);
                    h.click = (0.2 * v, 0.004, 4000.0);
                    h.gain = 0.6;
                }
                6 => {
                    push(&mut h, (var(250.0, 0.01), 200.0, 0.02, 0.5, 0.1));
                    h.noise = noise(0.7, 0.18, 1, 3500.0, 0.8);
                    h.noise2 = noise(0.25, 0.4, 2, 1200.0, 0.5);
                    h.drive = 0.4;
                    h.gain = 0.6;
                }
                _ => {
                    // drum head modes (Bessel ratios), wires, stick crack
                    let f = var(
                        if heavy {
                            170.0
                        } else if vintage {
                            205.0
                        } else {
                            190.0
                        },
                        0.015,
                    );
                    let crack = if rim { 1.0 } else { 0.3 + 0.7 * v * v };
                    let decay = if brush { 0.6 } else { 1.0 };
                    push(&mut h, (f * 1.12, f, 0.012, 0.55, 0.18 * decay));
                    push(
                        &mut h,
                        (f * 1.59 * 1.05, f * 1.59, 0.01, 0.35, 0.12 * decay),
                    );
                    push(&mut h, (f * 2.14, f * 2.14, 0.01, 0.2, 0.08 * decay));
                    push(&mut h, (f * 2.65, f * 2.65, 0.01, 0.12, 0.06 * decay));
                    // wires: broadband, longer at high velocity
                    let wires = if brush { 0.9 } else { 0.55 + 0.45 * v };
                    h.noise = noise(wires * 0.9, 0.22 + 0.12 * v, 0, 3000.0 + 2500.0 * v, 0.8);
                    h.noise2 = noise(0.05 * wires, 0.08, 1, 6500.0, 0.7);
                    h.noise_lp = 6000.0 + 3000.0 * v;
                    h.click = (
                        if brush {
                            0.05
                        } else {
                            0.6 * crack + if rim { 0.5 } else { 0.0 }
                        },
                        if rim { 0.012 } else { 0.007 },
                        if rim { 1800.0 } else { 2600.0 },
                    );
                    if brush {
                        h.noise.5 = 0.012; // the brush sweep softens the attack
                    }
                    h.gain = if rim { 0.8 } else { 0.7 };
                }
            }
        }
        37 => {
            h.group = Group::Snare;
            if elec {
                h.click = (0.8, 0.01, 2200.0);
                push(&mut h, (var(1700.0, 0.01), 1700.0, 0.01, 0.4, 0.025));
                push(&mut h, (480.0, 480.0, 0.01, 0.3, 0.03));
                h.gain = 0.5;
            } else {
                // side stick: stick across the rim on the head — woody knock with a short body
                h.click = (0.8, 0.008, 1900.0);
                push(&mut h, (var(520.0, 0.02), 500.0, 0.005, 0.4, 0.05));
                push(&mut h, (var(1650.0, 0.02), 1600.0, 0.005, 0.25, 0.035));
                push(&mut h, (var(205.0, 0.01), 190.0, 0.01, 0.2, 0.06));
                h.noise = noise(0.1, 0.05, 2, 3000.0, 1.0);
                h.gain = 0.55;
            }
        }
        39 => {
            h.group = Group::Snare;
            h.noise = noise(0.9, 0.012, 2, var(1150.0, 0.03), 2.2);
            h.bursts = (4, 0.0095);
            h.noise2 = noise(0.4, 0.18, 2, 1250.0, 1.6);
            h.gain = 0.6;
        }
        // ------------------------------------------------------------------ toms
        41 | 43 | 45 | 47 | 48 | 50 => {
            h.group = Group::Tom;
            let idx = [41, 43, 45, 47, 48, 50]
                .iter()
                .position(|&n| n == note)
                .unwrap_or(0) as f32;
            let f = if elec {
                65.0 * 2f32.powf(idx * 0.42)
            } else {
                78.0 * 2f32.powf(idx * 0.33)
            };
            let f = var(f, 0.012);
            if elec {
                push(
                    &mut h,
                    (f * 1.5, f, 0.05, 1.0, if kit == 4 { 0.5 } else { 0.35 }),
                );
                h.noise = noise(0.1, 0.03, 2, 3000.0, 0.7);
                h.gain = 0.65;
            } else {
                let d = 0.55 - idx * 0.05;
                push(&mut h, (f * 1.18, f, 0.06, 1.0, d));
                push(&mut h, (f * 1.59 * 1.1, f * 1.59, 0.05, 0.4, d * 0.5));
                push(&mut h, (f * 2.14, f * 2.14, 0.05, 0.18, d * 0.3));
                h.click = (if brush { 0.1 } else { 0.25 + 0.4 * v }, 0.008, 2200.0);
                h.noise = noise(0.08 + 0.06 * v, 0.08, 2, 900.0, 0.6);
                h.gain = 0.75;
            }
            h.pan = (idx - 2.5) / 2.5 * -0.6;
        }
        // ------------------------------------------------------------------ hi-hats
        42 | 44 | 46 | 200 => {
            h.group = Group::HiHat;
            h.choke = 1;
            h.pan = 0.35;
            let t = match note {
                42 => 0.07,
                44 => 0.05,
                200 => 0.28,
                _ => 0.75,
            } * (0.9 + 0.2 * v);
            if elec {
                let (bp, hp) = if kit == 4 {
                    (10_000.0, 7000.0)
                } else {
                    (11_000.0, 8000.0)
                };
                h.metal = (0.7, t, 1.0 + 0.002 * rr[1], bp, hp);
                if kit != 4 {
                    h.noise = noise(0.3, t, 1, 9000.0, 0.7);
                }
                h.gain = if kit == 4 { 1.1 } else { 0.7 };
            } else {
                // two cymbals: a metal cluster high up plus "sizzle" noise; tip vs shank by velocity
                h.metal = (0.45, t, 1.9 + 0.02 * rr[1], 8500.0 + 2500.0 * v, 6000.0);
                h.noise = noise(0.35 + 0.25 * v, t * 0.9, 2, 7000.0 + 1500.0 * v, 0.7);
                h.modes = (10, 3200.0, 12000.0, 0.10 * v, t * 1.1, 11);
                if note == 44 {
                    // foot chick: the cymbals clap shut
                    h.noise2 = noise(0.4, 0.025, 2, 1500.0, 1.0);
                    h.click = (0.25, 0.006, 4000.0);
                }
                if brush {
                    h.noise.5 = 0.008;
                }
                h.gain = if note == 44 { 0.5 } else { 0.6 };
            }
        }
        // ------------------------------------------------------------------ cymbals
        49 | 57 | 52 | 55 => {
            h.group = Group::Cymbal;
            let (t, lo, amp) = match note {
                55 => (1.0, 900.0, 0.8),
                52 => (2.0, 450.0, 1.0),
                57 => (2.6, 550.0, 1.0),
                _ => (2.4, 500.0, 1.0),
            };
            h.pan = if note == 57 { 0.5 } else { -0.45 };
            if elec {
                h.metal = (0.5 * amp, t * 0.7, 1.4, 7000.0, 4500.0);
                h.noise = noise(0.35, t * 0.6, 1, 6000.0, 0.6);
                h.gain = 0.4;
            } else {
                h.modes = (
                    MAXM,
                    lo,
                    11000.0,
                    0.22 * amp * (0.5 + 0.5 * v),
                    t,
                    note as u32 * 7 + (rr[2] * 3.0) as u32,
                );
                h.noise = noise(
                    0.3 * amp * (0.4 + 0.6 * v),
                    t * 0.8,
                    1,
                    4500.0 - 1200.0 * (1.0 - v),
                    0.6,
                );
                h.noise2 = noise(0.5 * v, 0.05, 1, 3000.0, 0.7); // stick attack
                if note == 52 {
                    h.drive = 0.6;
                }
                h.gain = 0.42;
            }
        }
        51 | 59 | 53 => {
            h.group = Group::Cymbal;
            h.pan = 0.55;
            let bell = note == 53;
            if elec {
                h.metal = (
                    0.45,
                    if bell { 0.9 } else { 1.2 },
                    if bell { 2.6 } else { 1.7 },
                    if bell { 3500.0 } else { 8000.0 },
                    if bell { 1500.0 } else { 4000.0 },
                );
                h.gain = 0.9;
            } else if bell {
                h.modes = (12, 700.0, 6000.0, 0.45, 2.6, 53);
                h.click = (0.3, 0.004, 5000.0);
                h.gain = 0.4;
            } else {
                // stick ping on the bow plus a soft wash
                h.modes = (
                    16,
                    1800.0,
                    9500.0,
                    0.25 + 0.1 * v,
                    2.2,
                    51 + (rr[2] * 2.0) as u32,
                );
                h.noise = noise(0.09 + 0.08 * v, 1.6, 1, 5500.0, 0.6);
                h.click = (0.35 * v, 0.003, 6000.0);
                h.gain = 0.38;
            }
        }
        // ------------------------------------------------------------------ percussion
        54 => {
            h.group = Group::Perc;
            h.modes = (10, 5500.0, 11000.0, 0.3, 0.3, 54);
            h.noise = noise(0.5, 0.18, 2, 8000.0, 1.0);
            h.gain = 0.4;
            h.pan = -0.3;
        }
        56 => {
            h.group = Group::Perc;
            if elec {
                h.metal = (0.0, 0.0, 1.0, 0.0, 0.0);
                push(&mut h, (540.0, 540.0, 0.01, 0.5, 0.3));
                push(&mut h, (800.0, 800.0, 0.01, 0.5, 0.25));
            } else {
                push(&mut h, (var(565.0, 0.01), 560.0, 0.005, 0.6, 0.35));
                push(&mut h, (var(845.0, 0.01), 845.0, 0.005, 0.4, 0.25));
                push(&mut h, (1360.0, 1360.0, 0.005, 0.25, 0.15));
                h.click = (0.3, 0.004, 3000.0);
            }
            h.drive = 0.5;
            h.gain = 0.4;
            h.pan = 0.3;
        }
        58 => {
            h.group = Group::Perc;
            h.noise = noise(0.4, 0.6, 2, 3000.0, 3.0);
            h.bursts = (8, 0.035);
            h.gain = 0.35;
        }
        60 | 61 | 62 | 63 | 64 | 65 | 66 | 86 | 87 => {
            // bongos, congas, timbales, surdo: membrane with a slap
            h.group = Group::Perc;
            let (f, t, slap) = match note {
                60 => (420.0, 0.18, 0.4),
                61 => (290.0, 0.22, 0.4),
                62 => (340.0, 0.12, 0.9), // mute conga
                63 => (330.0, 0.3, 0.5),
                64 => (230.0, 0.35, 0.4),
                65 => (520.0, 0.45, 0.6),
                66 => (390.0, 0.5, 0.6),
                86 => (82.0, 0.25, 0.2),
                _ => (82.0, 0.6, 0.2),
            };
            let f = var(f, 0.01);
            push(&mut h, (f * 1.1, f, 0.015, 1.0, t));
            push(&mut h, (f * 1.59, f * 1.59, 0.01, 0.3, t * 0.5));
            if note == 65 || note == 66 {
                h.modes = (6, f * 2.2, f * 9.0, 0.2, t, 65); // timbale shell ring
            }
            h.click = (slap * (0.4 + 0.6 * v), 0.006, 2500.0);
            h.gain = 0.5;
            h.pan = if note % 2 == 0 { 0.4 } else { -0.4 };
        }
        67 | 68 => {
            h.group = Group::Perc;
            let f = if note == 67 { 900.0 } else { 660.0 };
            push(&mut h, (f, f, 0.01, 0.5, 0.35));
            push(&mut h, (f * 2.76, f * 2.76, 0.01, 0.25, 0.2));
            h.click = (0.2, 0.003, 4000.0);
            h.gain = 0.35;
        }
        69 | 70 | 82 => {
            h.group = Group::Perc;
            let t = if note == 70 { 0.06 } else { 0.09 };
            h.noise = noise(0.7, t, 1, 6000.0, 0.8);
            h.noise.5 = 0.01 + 0.01 * rr[1].abs();
            h.gain = 0.35;
            h.pan = -0.5;
        }
        71 | 72 => {
            h.group = Group::Perc;
            let f = if note == 71 { 2400.0 } else { 2000.0 };
            push(
                &mut h,
                (f, f, 0.01, 0.3, if note == 71 { 0.12 } else { 0.4 }),
            );
            h.noise = noise(0.05, 0.2, 2, f, 4.0);
            h.gain = 0.3;
        }
        73 | 74 => {
            h.group = Group::Perc;
            h.noise = noise(0.5, 0.012, 2, 3500.0, 3.0);
            h.bursts = (if note == 73 { 4 } else { 10 }, 0.016);
            h.gain = 0.35;
        }
        75 | 76 | 77 | 85 => {
            h.group = Group::Perc;
            let (f, t) = match note {
                75 => (2500.0, 0.06),
                76 => (1250.0, 0.07),
                77 => (850.0, 0.08),
                _ => (2100.0, 0.03),
            };
            push(&mut h, (var(f, 0.01), f, 0.002, 0.7, t));
            push(&mut h, (f * 2.3, f * 2.3, 0.002, 0.2, t * 0.5));
            h.click = (0.4, 0.002, f);
            h.gain = 0.4;
            h.pan = 0.2;
        }
        78 | 79 => {
            h.group = Group::Perc;
            let f = if note == 78 { 700.0 } else { 450.0 };
            push(&mut h, (f * 0.7, f, 0.06, 0.5, 0.25));
            h.gain = 0.35;
        }
        80 | 81 | 83 | 84 => {
            h.group = Group::Perc;
            let t = match note {
                80 => 0.12,
                81 => 1.6,
                83 => 0.5,
                _ => 2.0,
            };
            h.modes = (8, 3000.0, 11000.0, 0.35, t, note as u32);
            h.click = (0.15, 0.002, 7000.0);
            h.gain = 0.3;
            h.pan = 0.45;
        }
        _ => return None,
    }
    Some(h)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DrumShared {
    rate: f32,
    kit: u8,
    gains: [f32; 6],
    tunes: [f32; 6],
    decays: [f32; 6],
    wires: f32,
    hat_tone: f32,
    vel_sens: f32,
    variation: f32,
    width: f32,
    punch: f32,
    tune_all: f32,
    use_pads: bool,
    pads: [Pad; 128],
}

impl DrumShared {
    fn new(rate: f32) -> DrumShared {
        DrumShared {
            rate,
            kit: 0,
            gains: [1.0; 6],
            tunes: [0.0; 6],
            decays: [1.0; 6],
            wires: 1.0,
            hat_tone: 0.0,
            vel_sens: 0.85,
            variation: 0.5,
            width: 0.6,
            punch: 0.3,
            tune_all: 0.0,
            use_pads: false,
            pads: [Pad::default(); 128],
        }
    }
    /// Kit, source note and pad for a played note.
    fn route(&self, note: u8) -> (u8, u8, Option<Pad>) {
        if self.use_pads {
            let p = self.pads[note as usize & 127];
            (p.kit, if p.src == 0 { note } else { p.src }, Some(p))
        } else {
            (self.kit, note, None)
        }
    }
}

fn gi(g: Group) -> usize {
    match g {
        Group::Kick => 0,
        Group::Snare => 1,
        Group::Tom => 2,
        Group::HiHat => 3,
        Group::Cymbal => 4,
        Group::Perc => 5,
    }
}

pub(crate) struct DrumVoice {
    on: bool,
    pub note: u8,
    hit: Hit,
    t: u32,
    ph: [f32; MAXT],
    tamp: [f32; MAXT],
    tdec: [f32; MAXT],
    glide: [f32; MAXT],
    tones_gt: [f32; MAXT],
    tf: [f32; MAXT],
    natk_step: f32,
    nlp: dsp::OnePole,
    nlp_on: bool,
    metal_inc: [f32; 6],
    nf: Svf,
    nf2: Svf,
    namp: f32,
    ndec: f32,
    natk: f32,
    n2amp: f32,
    n2dec: f32,
    cf: Svf,
    camp: f32,
    cdec: f32,
    mph: [f32; 6],
    mamp: f32,
    mdec: f32,
    mbp: Svf,
    mhp: Svf,
    modes: [Mode; MAXM],
    nmodes: usize,
    rng: Rng,
    gain: f32,
    pan_l: f32,
    pan_r: f32,
    drive: f32,
    fade: Fade,
    killing: bool,
    level: f32,
    burst_left: u8,
    burst_next: u32,
    burst_gap: u32,
    burst_amp: f32,
    pub choke: u8,
}

impl DrumVoice {
    fn new(seed: u32) -> DrumVoice {
        DrumVoice {
            on: false,
            note: 0,
            hit: Hit::default(),
            t: 0,
            ph: [0.0; MAXT],
            tamp: [0.0; MAXT],
            tdec: [0.0; MAXT],
            glide: [0.0; MAXT],
            tones_gt: [0.0; MAXT],
            tf: [0.0; MAXT],
            natk_step: 1.0,
            nlp: dsp::OnePole::default(),
            nlp_on: false,
            metal_inc: [0.0; 6],
            nf: Svf::default(),
            nf2: Svf::default(),
            namp: 0.0,
            ndec: 0.0,
            natk: 1.0,
            n2amp: 0.0,
            n2dec: 0.0,
            cf: Svf::default(),
            camp: 0.0,
            cdec: 0.0,
            mph: [0.0; 6],
            mamp: 0.0,
            mdec: 0.0,
            mbp: Svf::default(),
            mhp: Svf::default(),
            modes: [Mode::default(); MAXM],
            nmodes: 0,
            rng: Rng::new(seed),
            gain: 0.0,
            pan_l: 1.0,
            pan_r: 1.0,
            drive: 0.0,
            fade: Fade::default(),
            killing: false,
            level: 0.0,
            burst_left: 0,
            burst_next: 0,
            burst_gap: 0,
            burst_amp: 0.0,
            choke: 0,
        }
    }

    fn setup(&mut self, note: u8, h: Hit, v: f32, sh: &DrumShared) {
        let rate = sh.rate;
        let g = gi(h.group);
        let tune = dsp::semis(sh.tunes[g] + sh.tune_all + h.tune);
        let dec = sh.decays[g] * if h.decay > 0.0 { h.decay } else { 1.0 };
        self.hit = h;
        self.note = note;
        self.t = 0;
        for i in 0..h.ntones {
            let (f0, f1, gt, a, t60) = h.tones[i];
            self.tf[i] = f1 * tune;
            self.glide[i] = (f0 - f1) * tune;
            self.ph[i] = 0.0;
            self.tamp[i] = a;
            self.tdec[i] = dsp::t60_coef(t60 * dec, rate);
            self.tones_gt[i] = dsp::tau_coef(gt, rate);
        }
        let (na, nt, nm, nfq, nq, natk) = h.noise;
        self.namp = na * if g == 1 { sh.wires } else { 1.0 };
        self.ndec = dsp::t60_coef(nt * dec, rate);
        self.natk = if natk > 0.0 { 0.0 } else { 1.0 };
        self.natk_step = if natk > 0.0 { 1.0 / (natk * rate) } else { 1.0 };
        let hat = if g == 3 {
            dsp::semis(sh.hat_tone * 8.0)
        } else {
            1.0
        };
        self.nlp_on = h.noise_lp > 0.0;
        if self.nlp_on {
            self.nlp.lowpass_hz(h.noise_lp, rate);
            self.nlp.z = 0.0;
        }
        self.nf = Svf::new(mode_of(nm), (nfq * hat).min(rate * 0.45), nq, rate);
        let (n2a, n2t, n2m, n2f, n2q, _) = h.noise2;
        self.n2amp = n2a * if g == 1 { sh.wires } else { 1.0 };
        self.n2dec = dsp::t60_coef(n2t * dec, rate);
        self.nf2 = Svf::new(mode_of(n2m), (n2f * hat).min(rate * 0.45), n2q, rate);
        let (ca, ct, cfq) = h.click;
        self.camp = ca;
        self.cdec = dsp::t60_coef(ct, rate);
        self.cf = Svf::new(SvfMode::Band, cfq.min(rate * 0.45), 0.8, rate);
        let (ma, mt, mmul, mbp, mhp) = h.metal;
        self.mamp = ma;
        self.mdec = dsp::t60_coef(mt * dec, rate);
        for k in 0..6 {
            self.mph[k] = self.rng.unit();
        }
        self.metal_inc = [0.0; 6];
        for k in 0..6 {
            self.metal_inc[k] = METAL[k] * mmul * tune * hat / rate;
        }
        self.mbp = Svf::new(
            SvfMode::Band,
            (mbp * hat).clamp(100.0, rate * 0.45),
            1.2,
            rate,
        );
        self.mhp = Svf::new(
            SvfMode::High,
            (mhp * hat).clamp(100.0, rate * 0.45),
            0.7,
            rate,
        );
        let (mc, lo, hi, amp, t60, seed) = h.modes;
        self.nmodes = mc.min(MAXM);
        let mut r = Rng::new(seed.wrapping_add(12345));
        for k in 0..self.nmodes {
            // log-spaced, randomly scattered inharmonic partials; the highest die fastest
            let x = (k as f32 + 0.5 * r.unit()) / self.nmodes as f32;
            let f = lo * (hi / lo).powf(x) * tune * hat;
            let f = f.min(rate * 0.45);
            let t = t60 * dec * (1.0 - 0.6 * x) * (0.7 + 0.6 * r.unit());
            self.modes[k].tune(f, t, rate);
            let a = amp * (0.5 + r.unit()) / (self.nmodes as f32).sqrt();
            self.modes[k].strike(f, a * (if r.unit() > 0.5 { 1.0 } else { -1.0 }), rate);
        }
        let (bn, bs) = h.bursts;
        self.burst_left = bn.saturating_sub(1);
        self.burst_gap = (bs * rate) as u32;
        self.burst_next = self.burst_gap;
        self.burst_amp = self.namp;
        let vel_gain = (1.0 - sh.vel_sens) + sh.vel_sens * v.powf(1.6);
        // output trim per kit (levels pass: about −13 dBFS RMS on a groove, peaks under −1 dBFS)
        let kit_trim = match h.kit {
            4 => 1.85,
            5 | 6 => 1.95,
            _ => 1.33,
        };
        self.gain = h.gain * vel_gain * sh.gains[g] * 0.55 * kit_trim;
        let p = (h.pan * sh.width).clamp(-1.0, 1.0);
        self.pan_l = (1.0 - p).sqrt();
        self.pan_r = (1.0 + p).sqrt();
        self.drive = h.drive + sh.punch * if g <= 2 { 0.6 } else { 0.1 };
        self.choke = h.choke;
        self.killing = false;
        self.on = true;
        self.level = 1.0;
    }
}

fn mode_of(m: u8) -> SvfMode {
    match m {
        0 => SvfMode::Low,
        1 => SvfMode::High,
        _ => SvfMode::Band,
    }
}

impl Voice<DrumShared> for DrumVoice {
    fn start(&mut self, note: u8, velocity: u8, sh: &DrumShared, _ctl: &Ctl) {
        let v = velocity.max(1) as f32 / 127.0;
        let var = sh.variation;
        let rr = [
            self.rng.bipolar() * var,
            self.rng.bipolar() * var,
            self.rng.unit() * var * 3.0,
            self.rng.bipolar() * var,
        ];
        // round-robin also nudges the effective velocity a little
        let vv = (v * (1.0 + 0.06 * rr[3])).clamp(0.0, 1.0);
        let (kit, src, pad) = sh.route(note);
        if let Some(mut h) = build_hit(kit, src, vv, rr) {
            if let Some(p) = pad {
                h.tune += p.tune;
                h.decay = p.decay;
                h.gain *= p.gain;
                if p.pan != 0.0 {
                    h.pan = p.pan / sh.width.max(0.01);
                }
                if p.choke != 0 {
                    h.choke = p.choke;
                }
            }
            self.setup(note, h, vv, sh);
        } else {
            self.on = false;
        }
    }
    fn release(&mut self, _sh: &DrumShared, _ctl: &Ctl) {}
    fn kill(&mut self, rate: f32) {
        if self.on {
            self.killing = true;
            self.fade.start(0.008, rate);
        }
    }
    fn render(&mut self, l: &mut [f32], r: &mut [f32], sh: &DrumShared, _ctl: &Ctl) {
        let rate = sh.rate;
        let nt = self.hit.ntones;
        let nm = self.nmodes;
        let mut peak = 0.0f32;
        let has_metal = self.mamp > 0.0;
        for i in 0..l.len() {
            let mut x = 0.0;
            for k in 0..nt {
                let f = self.tf[k] + self.glide[k];
                self.glide[k] *= self.tones_gt[k];
                self.ph[k] += f / rate;
                if self.ph[k] >= 1.0 {
                    self.ph[k] -= 1.0;
                }
                x += dsp::sin_cycles(self.ph[k]) * self.tamp[k];
                self.tamp[k] *= self.tdec[k];
            }
            if self.namp > 1e-6 || self.burst_left > 0 {
                if self.natk < 1.0 {
                    self.natk = (self.natk + self.natk_step).min(1.0);
                }
                let nz = self.nf.run(self.rng.bipolar());
                let nz = if self.nlp_on {
                    self.nlp.lp(nz) * 1.6
                } else {
                    nz
                };
                x += nz * self.namp * self.natk;
                self.namp *= self.ndec;
                if self.burst_left > 0 {
                    if self.burst_next == 0 {
                        self.namp = self.burst_amp * (0.8 + 0.2 * self.rng.unit());
                        self.burst_left -= 1;
                        self.burst_next =
                            (self.burst_gap as f32 * (0.85 + 0.3 * self.rng.unit())) as u32;
                    } else {
                        self.burst_next -= 1;
                    }
                }
            }
            if self.n2amp > 1e-6 {
                x += self.nf2.run(self.rng.bipolar()) * self.n2amp;
                self.n2amp *= self.n2dec;
            }
            if self.camp > 1e-6 {
                let e = if self.t < 2 { 3.0 } else { 0.0 };
                x += self.cf.run(self.rng.bipolar() + e) * self.camp;
                self.camp *= self.cdec;
            }
            if has_metal && self.mamp > 1e-6 {
                let mut m = 0.0;
                for k in 0..6 {
                    self.mph[k] += self.metal_inc[k];
                    if self.mph[k] >= 1.0 {
                        self.mph[k] -= 1.0;
                    }
                    m += if self.mph[k] < 0.5 { 1.0 } else { -1.0 };
                }
                x += self.mhp.run(self.mbp.run(m * 0.25)) * self.mamp;
                self.mamp *= self.mdec;
            }
            if nm > 0 {
                let mut s = 0.0;
                for md in &mut self.modes[..nm] {
                    s += md.next();
                }
                x += s;
            }
            if self.drive > 0.0 {
                let d = 1.0 + 3.0 * self.drive;
                x = dsp::soft_clip(x * d) / d.sqrt();
            }
            let mut g = self.gain;
            if self.killing {
                g *= self.fade.next();
            }
            let y = x * g;
            peak = peak.max(y.abs());
            l[i] += y * self.pan_l;
            r[i] += y * self.pan_r;
            self.t += 1;
        }
        self.level = peak;
        let done = self.t > (rate * 0.05) as u32 && peak < 1e-5 && self.burst_left == 0;
        if done || (self.killing && self.fade.gain <= 0.0) || self.t > (rate * 12.0) as u32 {
            self.on = false;
        }
    }
    fn active(&self) -> bool {
        self.on
    }
    fn level(&self) -> f32 {
        self.level
    }
}

pub struct StudioDrumKit {
    params: Params,
    poly: Poly<DrumVoice>,
    ch: Channel,
    sh: DrumShared,
    room: Room,
    rate: f32,
}

impl StudioDrumKit {
    pub fn new(rate: f32) -> StudioDrumKit {
        dsp::warm_tables();
        let mut seed = 1000;
        let mut k = StudioDrumKit {
            params: Params::new(PARAMS),
            poly: Poly::new(48, || {
                seed += 101;
                DrumVoice::new(seed)
            }),
            ch: Channel::new(rate),
            sh: DrumShared::new(rate),
            room: Room::new(rate, 0.45),
            rate,
        };
        k.poly.retrigger_kills = false;
        k.room.feedback = 0.72;
        k.room.damp = 0.35;
        k.update();
        k
    }
    /// Drum Machine Designer mode: every note plays its pad's kit/source/tuning.
    pub fn set_pads(&mut self, pads: &[Pad; 128]) {
        self.sh.pads = *pads;
        self.sh.use_pads = true;
    }
    /// Selects a kit by index (see `KITS`).
    pub fn with_kit(rate: f32, kit: u8) -> StudioDrumKit {
        let mut k = StudioDrumKit::new(rate);
        k.set(0, kit as f32);
        k
    }
    fn p(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn update(&mut self) {
        self.sh.kit = self.p(0).round() as u8;
        let db = lg_dsp::db_to_gain;
        self.sh.gains = [
            db(self.p(2)),
            db(self.p(5)),
            db(self.p(9)),
            db(self.p(12)),
            db(self.p(15)),
            db(self.p(17)),
        ];
        self.sh.tunes = [self.p(3), self.p(6), self.p(10), 0.0, 0.0, 0.0];
        self.sh.decays = [
            self.p(4),
            self.p(7),
            self.p(11),
            self.p(14),
            self.p(16),
            1.0,
        ];
        self.sh.wires = self.p(8);
        self.sh.hat_tone = self.p(13);
        self.sh.vel_sens = self.p(19);
        self.sh.variation = self.p(20);
        self.sh.width = self.p(21);
        self.sh.tune_all = self.p(22);
        self.sh.punch = self.p(23);
    }
}

impl Plugin for StudioDrumKit {
    fn params(&self) -> &[Param] {
        PARAMS
    }
    fn get(&self, id: u32) -> f32 {
        self.params.get(id)
    }
    fn set(&mut self, id: u32, v: f32) {
        if self.params.set(id, v) {
            self.update();
        }
    }
    fn reset(&mut self) {
        for v in &mut self.poly.voices {
            v.on = false;
        }
        self.room.reset();
        self.ch.reset();
    }
    fn tail(&self) -> f32 {
        3.0
    }
}

impl Instrument for StudioDrumKit {
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            return;
        }
        let rate = self.rate;
        let (kit, src, pad) = self.sh.route(pitch);
        let probe = build_hit(kit, src, 0.5, [0.0; 4]);
        let Some(mut probe) = probe else { return };
        if let Some(p) = pad {
            if p.choke != 0 {
                probe.choke = p.choke;
            }
        }
        // choke group (closed/pedal hat cuts the open hat) and at most 3 overlapping hits per note
        let mut same = 0;
        let mut oldest: Option<usize> = None;
        for (i, v) in self.poly.voices.iter_mut().enumerate() {
            if !v.on || v.killing {
                continue;
            }
            if probe.choke != 0 && v.choke == probe.choke && v.note != pitch {
                v.kill(rate);
            } else if v.note == pitch {
                same += 1;
                oldest = Some(i);
                let _ = i;
            }
        }
        if same >= 3 {
            if let Some(i) = oldest {
                self.poly.voices[i].kill(rate);
            }
        }
        self.poly.note_on(pitch, velocity, &self.sh, &self.ch.ctl);
    }
    fn note_off(&mut self, pitch: u8) {
        self.poly.note_off(pitch, &self.sh, &self.ch.ctl);
    }
    fn controller(&mut self, cc: u8, value: u8) {
        // CC4 (hi-hat pedal) opens/closes: below 64 counts as a closed hat for choking purposes
        if cc == 120 {
            for v in &mut self.poly.voices {
                v.kill(self.rate);
            }
            return;
        }
        let _ = self.ch.cc(cc, value);
    }
    fn pitch_bend(&mut self, value: i16) {
        self.ch.set_bend(value);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut sl = [0.0f32; CHUNK];
        let mut sr = [0.0f32; CHUNK];
        let (gl, gr) = self.ch.out_gains();
        let vol = lg_dsp::db_to_gain(self.p(1));
        let room = self.p(18);
        let mut pos = 0;
        while pos < n {
            let m = (n - pos).min(CHUNK);
            sl[..m].fill(0.0);
            sr[..m].fill(0.0);
            self.poly
                .render(&mut sl[..m], &mut sr[..m], &self.sh, &self.ch.ctl);
            self.room.process(&mut sl[..m], &mut sr[..m], room);
            for i in 0..m {
                left[pos + i] += sl[i] * gl * vol;
                right[pos + i] += sr[i] * gr * vol;
            }
            pos += m;
        }
    }
}
