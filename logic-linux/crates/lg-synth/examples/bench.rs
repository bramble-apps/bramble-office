//! CPU check: 32 simultaneous voices per instrument at 48 kHz, 128-sample blocks.
use lg_dsp::Instrument;
fn main() {
    let rate = 48000.0;
    let mut list: Vec<(String, Box<dyn Instrument>)> = Vec::new();
    for n in [
        "Vintage Electric Piano",
        "Vintage B3 Organ",
        "Vintage Clav",
        "Retro Synth",
        "Piano",
        "Drum Kit Designer",
        "Bass",
        "Strings",
        "Guitar",
    ] {
        list.push((n.into(), lg_synth::by_logic_name(n, rate).unwrap()));
    }
    let mut r4 = lg_synth::by_logic_name("Retro Synth", rate).unwrap();
    r4.set(1, 4.0);
    r4.set(19, 2.0);
    list.push(("Retro Synth (table, unison 4)".into(), r4));
    let dir = std::env::temp_dir().join("lg-synth-bench");
    let sfz = lg_synth::sampler::write_test_instrument(&dir).unwrap();
    list.push((
        "Sampler (SFZ)".into(),
        Box::new(lg_synth::sampler::Sampler::from_sfz(&sfz, rate).unwrap()),
    ));
    let mut b = [0.0f32; 128];
    let mut c = [0.0f32; 128];
    for (name, inst) in list.iter_mut() {
        inst.controller(64, 127);
        for i in 0..32u8 {
            inst.note_on(36 + i * 2 % 60, 100);
        }
        let blocks = (2.0 * rate / 128.0) as usize;
        let t = std::time::Instant::now();
        for _ in 0..blocks {
            inst.render(&mut b, &mut c);
        }
        let used = t.elapsed().as_secs_f64() / 2.0;
        println!(
            "{name:<32} 32 voices: {:>5.1}% of one core in real time",
            used * 100.0
        );
    }
}
