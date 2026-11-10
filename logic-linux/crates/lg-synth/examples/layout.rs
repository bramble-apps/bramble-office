//! Renders Logic's reference MIDI layout (C2/C3/C4 at v40 then v100, 1 s on, 1 s off) through
//! one instrument for comparison with the reference bounces.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let name = args
        .get(1)
        .cloned()
        .unwrap_or("Vintage Electric Piano".into());
    let out = args
        .get(2)
        .cloned()
        .unwrap_or("/tmp/claude-1000/lg-synth-demo/layout.wav".into());
    let rate = 48000.0;
    let mut inst = lg_synth::by_logic_name(&name, rate).expect("unknown instrument");
    for kv in args.iter().skip(3) {
        if let Some((k, v)) = kv.split_once('=') {
            inst.set(k.parse().unwrap(), v.parse().unwrap());
        }
    }
    let total = 16 * 48000;
    let mut l = vec![0.0f32; total];
    let mut r = vec![0.0f32; total];
    let notes = [
        (48u8, 40u8),
        (48, 100),
        (60, 40),
        (60, 100),
        (72, 40),
        (72, 100),
    ];
    let block = 256;
    let mut pos = 0;
    while pos < total {
        let t = pos / 48000;
        if pos % 48000 == 0 {
            let k = t / 2;
            if k < 6 {
                if t % 2 == 0 {
                    inst.note_on(notes[k].0, notes[k].1)
                } else {
                    inst.note_off(notes[k].0)
                }
            }
        }
        let n = block.min(48000 - pos % 48000).min(total - pos);
        inst.render(&mut l[pos..pos + n], &mut r[pos..pos + n]);
        pos += n;
    }
    std::fs::create_dir_all(std::path::Path::new(&out).parent().unwrap()).ok();
    lg_synth::wav::write_wav_24(std::path::Path::new(&out), 48000, &l, &r).unwrap();
}
