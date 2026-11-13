//! Alchemy's Sampler element playing sample zones (alchemy_zones): pitch from the root key,
//! Release groups after note off, group Poly, note keyswitches and the zone audition.

use std::sync::Arc;

use lg_dsp::{Instrument, Plugin};
use lg_synth::alchemy::Alchemy;
use lg_synth::alchemy_zones::{self as z, ImportFile, Mapping, ZAudio, ZoneDoc};

const RATE: f32 = 48000.0;

fn render(a: &mut Alchemy, secs: f32) -> Vec<f32> {
    let n = (secs * RATE) as usize;
    let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
    for c in (0..n).step_by(256) {
        let e = (c + 256).min(n);
        a.render(&mut l[c..e], &mut r[c..e]);
    }
    l
}

fn sine(hz: f32, secs: f32) -> Arc<ZAudio> {
    let n = (secs * RATE) as usize;
    Arc::new(ZAudio { ch: vec![(0..n).map(|i| (i as f32 * hz / RATE * std::f32::consts::TAU).sin() * 0.5).collect()], rate: RATE })
}

fn file(hz: f32, root: u8) -> ImportFile {
    ImportFile { name: format!("{hz}"), path: None, audio: sine(hz, 2.0), root: Some(root) }
}

/// Source A plays only its Sampler element; B is off.
fn sampler(doc: ZoneDoc) -> Alchemy {
    let mut a = Alchemy::new(RATE);
    a.set(277, 1.0);
    a.set(297, 0.0);
    a.set(278, 0.0);
    a.set(z::SAMP_ON, 1.0);
    a.set(85, 1.0);
    a.set_zones(Arc::new(doc));
    a
}

fn freq(x: &[f32]) -> f32 {
    let c = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    c as f32 / (x.len() as f32 / RATE)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

#[test]
fn zones_play_at_root_and_transpose() {
    let mut d = ZoneDoc::default();
    d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
    let mut a = sampler(d);
    a.note_on(69, 100);
    let x = render(&mut a, 0.5);
    let f = freq(&x[4800..]);
    assert!((f - 440.0).abs() < 8.0, "{f}");
    a.note_off(69);
    render(&mut a, 3.0);
    // an octave up plays twice as fast
    a.note_on(81, 100);
    let x = render(&mut a, 0.5);
    let f = freq(&x[4800..]);
    assert!((f - 880.0).abs() < 12.0, "{f}");
    // the Sampler element off: silence
    a.note_off(81);
    render(&mut a, 3.0);
    a.set(z::SAMP_ON, 0.0);
    a.note_on(69, 100);
    assert!(rms(&render(&mut a, 0.3)) < 1e-4);
}

#[test]
fn release_group_sounds_after_note_off() {
    let mk = |with_release: bool| {
        let mut d = ZoneDoc::default();
        d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
        if with_release {
            let g = d.src[0].import(vec![file(1000.0, 69)], Mapping::Pitch, false);
            d.src[0].groups[g].trigger = 1;
        }
        let mut a = sampler(d);
        a.set(16, 0.0); // shortest Perform release
        a.set(136, 0.01); // and AHDSR 1's (it shapes Master Vol)
        a.set(5, 0.0); // no Delay+Reverb tail
        a
    };
    let mut plain = mk(false);
    let mut rel = mk(true);
    for a in [&mut plain, &mut rel] {
        a.note_on(69, 100);
        render(a, 0.3);
        a.note_off(69);
    }
    let tail_plain = render(&mut plain, 0.5);
    let tail_rel = render(&mut rel, 0.5);
    assert!(rms(&tail_plain[12000..]) < 1e-3);
    assert!(rms(&tail_rel[12000..]) > 0.01, "{}", rms(&tail_rel[12000..]));
    assert!((freq(&tail_rel[12000..]) - 1000.0).abs() < 15.0);
}

#[test]
fn group_poly_limits_zones() {
    let mut d = ZoneDoc::default();
    let g = d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
    d.src[0].groups[g].poly = 1;
    let mut a = sampler(d);
    a.note_on(60, 100);
    a.note_on(64, 100);
    render(&mut a, 0.05);
    assert_eq!(a.zones_sounding(0), 1);
    let mut d = ZoneDoc::default();
    d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
    let mut a = sampler(d);
    a.note_on(60, 100);
    a.note_on(64, 100);
    render(&mut a, 0.05);
    assert_eq!(a.zones_sounding(0), 2);
}

#[test]
fn note_keyswitch_switches_without_playing() {
    let mut d = ZoneDoc::default();
    let x = d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
    let y = d.src[0].import(vec![file(1000.0, 69)], Mapping::Pitch, false);
    d.src[0].groups[x].rules[0].rule = z::Rule::Keyswitch(z::Ks::Note(24), z::Ks::Note(24));
    d.src[0].groups[y].rules[0].rule = z::Rule::Keyswitch(z::Ks::Note(25), z::Ks::Note(25));
    let mut a = sampler(d);
    a.note_on(25, 100);
    assert!(a.voice_states().is_empty());
    a.note_off(25);
    a.note_on(69, 100);
    let f = freq(&render(&mut a, 0.5)[4800..]);
    assert!((f - 1000.0).abs() < 15.0, "{f}");
}

#[test]
fn audition_goes_to_the_outputs() {
    let mut d = ZoneDoc::default();
    d.src[0].import(vec![file(440.0, 69)], Mapping::Pitch, false);
    let mut a = sampler(d);
    a.audition_zone(0, 0, 0);
    assert!(a.auditioning());
    let x = render(&mut a, 0.3);
    assert!((freq(&x) - 440.0).abs() < 8.0);
    assert!(a.zone_position(0).is_some());
    a.audition_stop();
    render(&mut a, 0.1);
    assert!(!a.auditioning());
    // a keymap key: the zone for that note, pitched
    a.audition_key(0, 81, 100);
    let x = render(&mut a, 0.3);
    assert!((freq(&x) - 880.0).abs() < 12.0);
}

/// The period (in samples) with the strongest autocorrelation between lo and hi.
fn period(x: &[f32], lo: usize, hi: usize) -> usize {
    (lo..hi).max_by(|&a, &b| {
        let c = |l: usize| x.iter().zip(&x[l..]).map(|(p, q)| p * q).sum::<f32>();
        c(a).total_cmp(&c(b))
    }).unwrap()
}

#[test]
fn granular_sync_follows_the_note_and_taps_add_streams() {
    use lg_synth::alchemy_gran::{self as gr, id};
    let mk = || {
        let mut a = Alchemy::new(RATE);
        a.set(277, 1.0);
        a.set(297, 0.0);
        a.set(278, 0.0);
        a.set(313, 1.0);
        a.set(5, 0.0);
        a
    };
    // Sync: a grain every period of the note
    let mut a = mk();
    a.set(id(0, gr::SYNC), 1.0);
    a.set(id(0, gr::RTIME), 0.0);
    a.note_on(69, 100);
    let x = render(&mut a, 0.4);
    let per = period(&x[9600..14400], 60, 200);
    assert!((per as f32 - RATE / 440.0).abs() < 3.0, "{per}");
    // more taps: more grain streams, more level
    let mut one = mk();
    let mut four = mk();
    four.set(id(0, gr::TAPS), 4.0);
    four.set(id(0, gr::SPACING), 20.0);
    for a in [&mut one, &mut four] {
        a.note_on(60, 100);
    }
    let (r1, r4) = (rms(&render(&mut one, 0.5)[4800..]), rms(&render(&mut four, 0.5)[4800..]));
    assert!(r4 > r1 * 1.3, "{r1} {r4}");
}
