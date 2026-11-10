//! Renders a short phrase on each instrument to /tmp/claude-1000/lg-synth-demo/*.wav and prints
//! peak/RMS per file. `cargo run --release -p lg-synth --example demo [filter]`.

use lg_dsp::Instrument;
use std::path::Path;

const RATE: f32 = 48000.0;
const OUT: &str = "/tmp/claude-1000/lg-synth-demo";

#[derive(Clone, Copy)]
enum Ev {
    On(u8, u8),
    Off(u8),
    Cc(u8, u8),
    Bend(i16),
}

struct Seq {
    ev: Vec<(f64, Ev)>,
}

impl Seq {
    fn new() -> Seq {
        Seq { ev: Vec::new() }
    }
    fn note(&mut self, t: f64, n: u8, v: u8, d: f64) {
        self.ev.push((t, Ev::On(n, v)));
        self.ev.push((t + d, Ev::Off(n)));
    }
    fn chord(&mut self, t: f64, ns: &[u8], v: u8, d: f64) {
        for (i, &n) in ns.iter().enumerate() {
            self.note(t + i as f64 * 0.008, n, v.saturating_sub(i as u8 * 3), d);
        }
    }
    fn cc(&mut self, t: f64, c: u8, v: u8) {
        self.ev.push((t, Ev::Cc(c, v)));
    }
    fn bend(&mut self, t: f64, v: i16) {
        self.ev.push((t, Ev::Bend(v)));
    }
}

fn render(inst: &mut dyn Instrument, seq: &Seq, secs: f64) -> (Vec<f32>, Vec<f32>, f64) {
    let total = (secs * RATE as f64) as usize;
    let mut l = vec![0.0f32; total];
    let mut r = vec![0.0f32; total];
    let mut ev = seq.ev.clone();
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut k = 0;
    let mut pos = 0;
    let t0 = std::time::Instant::now();
    while pos < total {
        while k < ev.len() && (ev[k].0 * RATE as f64) as usize <= pos {
            match ev[k].1 {
                Ev::On(n, v) => inst.note_on(n, v),
                Ev::Off(n) => inst.note_off(n),
                Ev::Cc(c, v) => inst.controller(c, v),
                Ev::Bend(b) => inst.pitch_bend(b),
            }
            k += 1;
        }
        let next = if k < ev.len() {
            ((ev[k].0 * RATE as f64) as usize).max(pos + 1)
        } else {
            total
        };
        let n = (next - pos).min(256).min(total - pos);
        inst.render(&mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    let cpu = t0.elapsed().as_secs_f64() / secs;
    (l, r, cpu)
}

fn save(name: &str, inst: &mut dyn Instrument, seq: &Seq, secs: f64) {
    let (l, r, cpu) = render(inst, seq, secs);
    let peak = l
        .iter()
        .chain(r.iter())
        .fold(0.0f32, |a, &x| a.max(x.abs()));
    let rms = (l
        .iter()
        .chain(r.iter())
        .map(|&x| (x as f64) * (x as f64))
        .sum::<f64>()
        / (2 * l.len()) as f64)
        .sqrt();
    let nan = l.iter().chain(r.iter()).any(|x| !x.is_finite());
    let path = Path::new(OUT).join(format!("{name}.wav"));
    lg_synth::wav::write_wav_24(&path, RATE as u32, &l, &r).unwrap();
    println!(
        "{:<34} peak {:>6.1} dBFS  rms {:>6.1} dBFS  cpu {:>5.1}% of real time{}",
        name,
        20.0 * (peak.max(1e-9)).log10(),
        20.0 * (rms.max(1e-12)).log10(),
        cpu * 100.0,
        if nan { "  NaN!" } else { "" }
    );
}

fn keys_phrase() -> Seq {
    let mut s = Seq::new();
    let chords: [&[u8]; 4] = [
        &[48, 55, 59, 64, 67],
        &[45, 55, 60, 64, 71],
        &[50, 57, 60, 65, 69],
        &[43, 53, 57, 59, 64],
    ];
    for (i, c) in chords.iter().enumerate() {
        let t = i as f64 * 2.0;
        s.cc(t, 64, 127);
        s.chord(t, c, 70 + (i as u8 % 2) * 30, 1.6);
        s.cc(t + 1.95, 64, 0);
        // melody on top
        for (j, n) in [76u8, 79, 77, 74].iter().enumerate() {
            s.note(
                t + 0.5 + j as f64 * 0.35,
                n - (i as u8 % 3),
                60 + j as u8 * 15,
                0.3,
            );
        }
    }
    // a few soft/loud single notes, low to high
    for (j, n) in [36u8, 48, 60, 72, 84, 96].iter().enumerate() {
        s.note(
            8.5 + j as f64 * 0.45,
            *n,
            if j % 2 == 0 { 40 } else { 115 },
            0.4,
        );
    }
    s
}

fn organ_phrase() -> Seq {
    let mut s = keys_phrase();
    s.cc(4.0, 1, 127); // rotor fast
    s.cc(7.5, 1, 0);
    s.bend(9.0, 0);
    s
}

fn clav_phrase() -> Seq {
    let mut s = Seq::new();
    let riff = [48u8, 0, 51, 48, 0, 53, 0, 51, 48, 0, 55, 53, 51, 0, 48, 46];
    for bar in 0..4 {
        for (i, &n) in riff.iter().enumerate() {
            if n > 0 {
                let t = bar as f64 * 2.0 + i as f64 * 0.125;
                s.note(t, n + 12, if i % 4 == 0 { 120 } else { 70 }, 0.08);
            }
        }
        s.chord(bar as f64 * 2.0 + 0.5, &[63, 67, 70], 90, 0.1);
        s.chord(bar as f64 * 2.0 + 1.5, &[63, 67, 70], 100, 0.1);
    }
    s
}

fn synth_phrase() -> Seq {
    let mut s = Seq::new();
    // arpeggio then pad chord with mod wheel vibrato and a bend
    for i in 0..16 {
        let n = [48u8, 55, 60, 63, 67, 63, 60, 55][i % 8] + if i >= 8 { 5 } else { 0 };
        s.note(i as f64 * 0.18, n, 60 + (i as u8 % 4) * 18, 0.15);
    }
    s.chord(3.2, &[48, 55, 60, 63, 67], 100, 2.6);
    s.cc(4.2, 1, 100);
    s.bend(5.0, 4096);
    s.bend(5.5, 0);
    s.cc(5.8, 1, 0);
    s.note(6.4, 36, 127, 1.0);
    s.note(6.4, 72, 127, 1.0);
    s
}

fn bass_phrase() -> Seq {
    let mut s = Seq::new();
    let line = [
        (28u8, 0.0),
        (28, 0.5),
        (31, 1.0),
        (33, 1.5),
        (35, 1.75),
        (36, 2.0),
        (36, 2.5),
        (35, 3.0),
        (33, 3.25),
        (31, 3.5),
    ];
    for rep in 0..2 {
        for (n, t) in line.iter() {
            s.note(
                rep as f64 * 4.0 + t,
                n + 12,
                70 + ((t * 4.0) as u8 % 3) * 25,
                0.22,
            );
        }
    }
    s.note(8.0, 40, 110, 1.5);
    s
}

fn pad_phrase() -> Seq {
    let mut s = Seq::new();
    s.chord(0.0, &[48, 55, 60, 64, 67], 90, 3.0);
    s.chord(3.0, &[45, 52, 57, 60, 64], 70, 3.0);
    s.cc(1.5, 1, 90);
    s.note(6.2, 72, 110, 0.6);
    s.note(6.9, 76, 80, 0.6);
    s.note(7.6, 79, 60, 1.0);
    s
}

fn drum_groove() -> Seq {
    let mut s = Seq::new();
    let step = 0.125; // 16ths at 120 bpm
    for bar in 0..4 {
        let b = bar as f64 * 2.0;
        for i in 0..16 {
            let t = b + i as f64 * step;
            // hats: closed 8ths with accents, open hat on the "and" of 4 in bar 2/4
            if i % 2 == 0 {
                if bar % 2 == 1 && i == 14 {
                    s.note(t, 46, 100, 0.2);
                } else {
                    s.note(t, 42, if i % 4 == 0 { 105 } else { 70 }, 0.05);
                }
            } else if bar >= 2 {
                s.note(t, 42, 45, 0.05); // ghost 16ths
            }
            if [0, 7, 10].contains(&i) {
                s.note(t, 36, if i == 0 { 120 } else { 95 }, 0.1);
            }
            if i == 4 || i == 12 {
                s.note(t, 38, 115, 0.1);
            }
            if bar >= 2 && (i == 6 || i == 15) {
                s.note(t, 38, 35, 0.1); // ghost notes
            }
        }
        if bar == 0 {
            s.note(b, 49, 110, 0.5);
        }
    }
    // fill and crash/ride
    let f = 8.0;
    for (i, n) in [48u8, 48, 45, 45, 43, 43, 41, 41].iter().enumerate() {
        s.note(f + i as f64 * 0.125, *n, 90 + (i as u8 % 2) * 20, 0.1);
    }
    s.note(9.0, 36, 120, 0.1);
    s.note(9.0, 57, 120, 1.0);
    for i in 0..8 {
        s.note(9.5 + i as f64 * 0.25, if i == 4 { 53 } else { 51 }, 90, 0.1);
        s.note(9.5 + i as f64 * 0.25, 44, 60, 0.1);
    }
    s.note(10.0, 37, 90, 0.1);
    s.note(10.5, 39, 110, 0.1);
    s.note(11.0, 56, 100, 0.1);
    s.note(11.25, 54, 100, 0.1);
    s
}

fn main() {
    std::fs::create_dir_all(OUT).unwrap();
    let filter = std::env::args().nth(1).unwrap_or_default();
    let want = |n: &str| filter.is_empty() || n.contains(&filter);

    let mut jobs: Vec<(String, Box<dyn Instrument>, Seq, f64)> = Vec::new();
    let mut add = |name: &str, inst: Option<Box<dyn Instrument>>, seq: Seq, secs: f64| {
        if want(name) {
            if let Some(i) = inst {
                jobs.push((name.to_string(), i, seq, secs));
            } else {
                println!("{name}: not available");
            }
        }
    };
    add(
        "ep-classic-tines",
        lg_synth::by_logic_name("Vintage Electric Piano", RATE),
        keys_phrase(),
        12.0,
    );
    for (patch, file) in [
        ("Wurlitzer Classic", "ep-reed"),
        ("Bright Suitcase", "ep-suitcase-bright"),
        ("Phasing Bells", "ep-phasing-bells"),
    ] {
        add(file, lg_synth::for_patch(patch, RATE), keys_phrase(), 12.0);
    }
    add(
        "b3-organ",
        lg_synth::by_logic_name("Vintage B3 Organ", RATE),
        organ_phrase(),
        12.0,
    );
    add(
        "b3-jazz-organ",
        lg_synth::for_patch("Jazz Organ", RATE),
        organ_phrase(),
        12.0,
    );
    add(
        "clav",
        lg_synth::by_logic_name("Vintage Clav", RATE),
        clav_phrase(),
        8.5,
    );
    add(
        "clav-wah",
        lg_synth::for_patch("Wah-Wah Clav", RATE),
        clav_phrase(),
        8.5,
    );
    add(
        "retro-analog",
        lg_synth::by_logic_name("Retro Synth", RATE),
        synth_phrase(),
        8.0,
    );
    for (patch, file) in [
        ("Sync Lead", "retro-sync-lead"),
        ("Glass Table", "retro-table"),
        ("FM Bells", "retro-fm-bells"),
        ("Analog Bass", "retro-bass"),
    ] {
        add(file, lg_synth::for_patch(patch, RATE), synth_phrase(), 8.0);
    }
    add(
        "piano-grand",
        lg_synth::by_logic_name("Piano", RATE),
        keys_phrase(),
        12.0,
    );
    add(
        "piano-upright",
        lg_synth::for_patch("Vintage Upright", RATE),
        keys_phrase(),
        12.0,
    );
    for (kit, name) in lg_synth::drums::KITS.iter().enumerate() {
        let file = format!("drums-{}", name.to_lowercase().replace(' ', "-"));
        add(
            &file,
            Some(Box::new(lg_synth::drums::StudioDrumKit::with_kit(
                RATE, kit as u8,
            ))),
            drum_groove(),
            12.5,
        );
    }
    for (patch, file) in [
        ("Solid Soul Electric Bass", "bass-finger"),
        ("Rock Growl", "bass-pick"),
        ("Slap & Pop", "bass-slap"),
        ("Roots Upright", "bass-upright"),
    ] {
        add(file, lg_synth::for_patch(patch, RATE), bass_phrase(), 10.0);
    }
    for (patch, file) in [
        ("Studio Strings", "strings-ensemble"),
        ("Full Brass", "brass-section"),
        ("Mellotron Choir", "choir-tape"),
        ("Warm Analog Pad", "pad-warm"),
        ("Flute Solo", "flute"),
        ("Vibraphone", "mallet-vibes"),
        ("Marimba", "mallet-marimba"),
        ("Acoustic Guitar", "guitar-acoustic"),
        ("Classical Acoustic Guitar", "guitar-nylon"),
        ("Harpsichord", "harpsichord"),
        ("Harp", "harp"),
        ("Tubular Bells", "tubular-bells"),
    ] {
        add(file, lg_synth::for_patch(patch, RATE), pad_phrase(), 8.5);
    }
    for (name, file, phrase) in [
        ("ES2", "es2-default", 0),
        ("ES1", "es1", 1),
        ("ES M", "es-m", 2),
        ("ES P", "es-p", 0),
        ("ES E", "es-e", 3),
        ("EFM1", "efm1", 0),
        ("Sculpture", "sculpture", 3),
        ("Alchemy", "alchemy-default", 0),
        ("Drum Machine Designer", "dmd-drum-synth-kit", 4),
    ] {
        let seq = match phrase {
            1 => synth_phrase(),
            2 => bass_phrase(),
            3 => pad_phrase(),
            4 => drum_groove(),
            _ => synth_phrase(),
        };
        add(
            file,
            lg_synth::by_logic_name(name, RATE),
            seq,
            if phrase == 4 { 12.5 } else { 8.5 },
        );
    }
    for (patch, file) in [
        ("Shimmering Pad", "alchemy-pad"),
        ("Vocal Atmosphere Pad", "alchemy-vocal-pad"),
        ("Cosmic Soundscape", "alchemy-soundscape"),
        ("Pulsing Arp", "alchemy-arp"),
    ] {
        let spec = lg_synth::patches::alchemy(match file {
            "alchemy-pad" => "pad",
            "alchemy-vocal-pad" => "vocal pad",
            "alchemy-soundscape" => "soundscape",
            _ => "arp",
        });
        let _ = patch;
        add(
            file,
            Some(lg_synth::build(&spec, RATE)),
            if file == "alchemy-arp" {
                synth_phrase()
            } else {
                pad_phrase()
            },
            8.5,
        );
    }
    for (i, sound) in lg_synth::mellotron::SOUNDS.iter().enumerate() {
        let mut m = lg_synth::mellotron::VintageMellotron::new(RATE);
        lg_dsp::Plugin::set(&mut m, 0, i as f32);
        add(
            &format!(
                "mellotron-{}",
                sound.to_lowercase().replace([' ', '&'], "-")
            ),
            Some(Box::new(m)),
            pad_phrase(),
            8.5,
        );
    }
    {
        // Studio Strings: keyswitch through sustain, staccato, pizzicato, tremolo, trill
        let mut s = Seq::new();
        for (k, art) in [0u8, 1, 3, 4, 5].iter().enumerate() {
            let t = k as f64 * 1.6;
            s.note(t, 24 + art, 100, 0.05);
            s.chord(t + 0.05, &[55, 62, 67], 95, 1.2);
        }
        add(
            "studio-strings-articulations",
            lg_synth::by_logic_name("Studio Strings", RATE),
            s,
            9.0,
        );
        let mut h = Seq::new();
        for (k, art) in [0u8, 8, 10, 12, 13, 14].iter().enumerate() {
            let t = k as f64 * 1.6;
            h.note(t, 24 + art, 100, 0.05);
            h.chord(t + 0.05, &[58, 62, 65], 100, 0.9);
        }
        add(
            "studio-horns-articulations",
            lg_synth::by_logic_name("Studio Horns", RATE),
            h,
            10.5,
        );
    }
    // the Sampler on content made here (a sine-based multi-sample SFZ)
    if want("sampler") {
        let dir = Path::new(OUT).join("sfz-test");
        let sfz = lg_synth::sampler::write_test_instrument(&dir).unwrap();
        let s = lg_synth::sampler::Sampler::from_sfz(&sfz, RATE).unwrap();
        jobs.push(("sampler-sfz".into(), Box::new(s), keys_phrase(), 12.0));
    }
    for (name, mut inst, seq, secs) in jobs {
        save(&name, inst.as_mut(), &seq, secs);
    }
}
