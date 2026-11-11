//! Every effect, every parameter at its extremes and every menu choice: no NaN, no infinity, no
//! runaway on noise bursts and impulses; and with defaults, silence in gives (near) silence out
//! once the tail has passed.

mod common;
use common::*;
use lg_dsp::{Effect, Unit};

fn settings(fx: &dyn Effect) -> Vec<(u32, f32)> {
    let mut v = Vec::new();
    for p in fx.params() {
        if p.unit == Unit::Menu {
            for c in 0..p.choices.len().max(1) {
                v.push((p.id, c as f32));
            }
        } else {
            v.push((p.id, p.min));
            v.push((p.id, p.max));
        }
    }
    v
}

#[test]
fn every_effect_every_setting_is_stable() {
    for (logic, ours) in lg_fx::list() {
        let probe = lg_fx::by_logic_name(logic, RATE).unwrap_or_else(|| panic!("{logic} not built"));
        let all = settings(probe.as_ref());
        for (id, v) in all {
            let mut fx = lg_fx::by_logic_name(logic, RATE).unwrap();
            fx.set(id, v);
            assert_stable(fx.as_mut(), 1e4, &format!("{ours} param {id} = {v}"));
        }
    }
}

#[test]
fn all_params_extreme_together() {
    for (logic, ours) in lg_fx::list() {
        for pick_max in [false, true] {
            let mut fx = lg_fx::by_logic_name(logic, RATE).unwrap();
            let ps: Vec<_> = fx.params().to_vec();
            for p in ps {
                fx.set(p.id, if pick_max { p.max } else { p.min });
            }
            // (all gains at maximum can be enormous by design: this checks for runaway only)
            assert_stable(fx.as_mut(), 1e15, &format!("{ours} all {}", if pick_max { "max" } else { "min" }));
        }
    }
}

#[test]
fn defaults_fall_silent() {
    for (logic, ours) in lg_fx::list() {
        // the Test Oscillator is a generator: it sounds by design whatever comes in
        if logic == "Test Oscillator" {
            continue;
        }
        let mut fx = lg_fx::by_logic_name(logic, RATE).unwrap();
        fx.set_tempo(120.0);
        let tail = fx.tail().min(30.0);
        let n = ((tail + 1.0) * RATE) as usize + 48000;
        let mut x = vec![0.0f32; n];
        for (i, v) in x[..4800].iter_mut().enumerate() {
            *v = ((i as f32) * 0.37).sin() * 0.5;
        }
        let (l, r) = run(fx.as_mut(), &x);
        let end = l[n - 4800..].iter().chain(r[n - 4800..].iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(end < 1e-3, "{ours} still sounds at {end} after its tail ({tail} s)");
    }
}

#[test]
fn every_effect_handles_odd_block_sizes_and_rates() {
    for rate in [44100.0, 96000.0] {
        for (logic, ours) in lg_fx::list() {
            let mut fx = lg_fx::by_logic_name(logic, rate).unwrap();
            let mut s: u32 = 5;
            let mut l: Vec<f32> = (0..20000)
                .map(|_| {
                    s ^= s << 13;
                    s ^= s >> 17;
                    s ^= s << 5;
                    (s as f32 / u32::MAX as f32 - 0.5) * 0.8
                })
                .collect();
            let mut r = l.clone();
            let mut at = 0;
            for size in [1usize, 7, 64, 333, 1024, 4096].iter().cycle() {
                if at >= l.len() {
                    break;
                }
                let e = (at + size).min(l.len());
                fx.process(&mut l[at..e], &mut r[at..e]);
                at = e;
            }
            assert!(l.iter().chain(r.iter()).all(|v| v.is_finite() && v.abs() < 1e3), "{ours} at {rate}");
        }
    }
}

#[test]
fn names_and_list_agree() {
    let names = lg_fx::list();
    assert!(names.len() >= 20);
    for (logic, _) in names {
        assert!(lg_fx::by_logic_name(logic, 48000.0).is_some(), "{logic}");
    }
    assert!(lg_fx::by_logic_name("Not A Plug-in", 48000.0).is_none());
}

/// A real project block (the research's compressor render settings) loads by index.
#[test]
fn compressor_loads_project_floats() {
    let Some(floats) = settings_array("Compressor/VintageFET", "project_GAMETSPP_floats") else { return };
    let mut fx = lg_fx::by_logic_name("Compressor", RATE).unwrap();
    fx.load_floats(&floats);
    assert_eq!(fx.get(lg_fx::compressor::CIRCUIT), 5.0);
    assert_eq!(fx.get(lg_fx::compressor::THRESHOLD), -20.0);
    assert_eq!(fx.get(lg_fx::compressor::MIX), 100.0);
}

/// Real-time cost of each effect (run with --release --ignored --nocapture).
#[test]
#[ignore]
fn cpu_cost() {
    for (logic, ours) in lg_fx::list() {
        let mut fx = lg_fx::by_logic_name(logic, 48000.0).unwrap();
        let mut l: Vec<f32> = (0..48000).map(|i| ((i as f32) * 0.01).sin() * 0.3).collect();
        let mut r = l.clone();
        let t = std::time::Instant::now();
        for (a, b) in l.chunks_mut(256).zip(r.chunks_mut(256)) {
            fx.process(a, b);
        }
        let secs = t.elapsed().as_secs_f64();
        eprintln!("{ours:>18}: {:5.2} % of one core (1 s of stereo at 48 kHz)", secs * 100.0);
    }
}
