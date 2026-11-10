//! Renders a demo phrase on every bundled sampled instrument (needs the sample library):
//! `cargo run --release -p lg-synth --example sampled_demo -- [out dir] [id…]`. Prints peak and RMS.

use lg_dsp::Instrument;

const RATE: f32 = 48000.0;

fn phrase(id: &str) -> Vec<(f32, f32, u8, u8)> {
    // (start s, length s, note, velocity)
    let mut v = Vec::new();
    if id.starts_with("drums") {
        let bar = 2.0;
        for b in 0..2 {
            let t0 = b as f32 * bar;
            for i in 0..8 {
                let t = t0 + i as f32 * bar / 8.0;
                v.push((t, 0.1, 42, if i % 2 == 0 { 100 } else { 70 }));
            }
            v.push((t0, 0.1, 36, 110));
            v.push((t0 + 0.75, 0.1, 36, 90));
            v.push((t0 + 0.5, 0.1, 38, 115));
            v.push((t0 + 1.5, 0.1, 38, 112));
            v.push((t0 + 1.25, 0.1, 38, 35));
        }
        v.push((4.0, 0.1, 49, 115));
        v.push((4.0, 0.1, 36, 115));
        for (i, n) in [48u8, 47, 43, 41].iter().enumerate() {
            v.push((4.5 + i as f32 * 0.2, 0.1, *n, 100));
        }
        for i in 0..4 {
            v.push((5.5 + i as f32 * 0.25, 0.1, 51, 90));
        }
        v.push((6.5, 0.1, 46, 100));
        v.push((7.0, 0.1, 44, 90));
        return v;
    }
    let (lo, hi) = match id {
        "bass-electric" | "bass-upright" | "basses" | "tuba" | "bassoon" | "timpani" => (28u8, 52u8),
        "cellos" | "trombone" | "french-horn" => (40, 64),
        "violins" | "violin-solo" | "flute" | "oboe" | "trumpet" | "glockenspiel" => (60, 84),
        _ => (48, 72),
    };
    let scale = [0u8, 2, 4, 5, 7, 9, 11, 12];
    let mut t = 0.0;
    for (i, s) in scale.iter().enumerate() {
        let n = lo + s;
        if n > hi + 12 {
            break;
        }
        v.push((t, 0.45, n, 40 + (i as u8) * 12));
        t += 0.5;
    }
    // a chord, held
    for n in [lo + 12, lo + 16, lo + 19] {
        v.push((t, 2.0, n, 90));
    }
    v
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::path::PathBuf::from(args.first().cloned().unwrap_or_else(|| "/tmp/claude-1000/lg-sampled-demo".into()));
    std::fs::create_dir_all(&out).unwrap();
    let only: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    println!("{:<20} {:>8} {:>8} {:>7}", "instrument", "peak dB", "rms dB", "load s");
    for s in lg_sounds::index() {
        if !only.is_empty() && !only.contains(&s.id) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let Some(mut inst) = lg_synth::sampled::SampledInstrument::load(s.id, RATE) else {
            println!("{:<20} not installed", s.id);
            continue;
        };
        let load = t0.elapsed().as_secs_f32();
        let ev = phrase(s.id);
        let end = ev.iter().map(|e| e.0 + e.1).fold(0.0, f32::max) + 2.5;
        let n = (end * RATE) as usize;
        let (mut l, mut r) = (vec![0.0f32; n], vec![0.0f32; n]);
        let block = 256;
        let mut pos = 0;
        // events: (sample, on?, note, vel)
        let mut evs: Vec<(usize, bool, u8, u8)> = Vec::new();
        for &(t, len, note, vel) in &ev {
            evs.push(((t * RATE) as usize, true, note, vel));
            evs.push((((t + len) * RATE) as usize, false, note, 0));
        }
        evs.sort_by_key(|e| (e.0, e.1));
        let mut k = 0;
        while pos < n {
            let m = block.min(n - pos);
            while k < evs.len() && evs[k].0 < pos + m {
                let (_, on, note, vel) = evs[k];
                if on { inst.note_on(note, vel) } else { inst.note_off(note) }
                k += 1;
            }
            inst.render(&mut l[pos..pos + m], &mut r[pos..pos + m]);
            pos += m;
        }
        let peak = l.iter().chain(r.iter()).fold(0.0f32, |a, &b| a.max(b.abs()));
        let rms = (l.iter().chain(r.iter()).map(|x| x * x).sum::<f32>() / (2 * n) as f32).sqrt();
        let db = |x: f32| 20.0 * x.max(1e-9).log10();
        println!("{:<20} {:>8.1} {:>8.1} {:>7.2}", s.id, db(peak), db(rms), load);
        lg_synth::wav::write_wav_24(&out.join(format!("{}.wav", s.id)), RATE as u32, &l, &r).unwrap();
    }
}
