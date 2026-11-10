//! Plays single notes: `notes <plugin> <out.wav> note:vel:dur ...` (dur in s; 1 s gap after each).
use lg_dsp::Instrument;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rate = 48000.0;
    let mut inst: Box<dyn Instrument> = if let Some(k) = a[1].strip_prefix("kit") {
        Box::new(lg_synth::drums::StudioDrumKit::with_kit(
            rate,
            k.parse().unwrap(),
        ))
    } else {
        lg_synth::by_logic_name(&a[1], rate).expect("unknown")
    };
    let mut l = Vec::new();
    let mut r = Vec::new();
    for spec in a[3..].iter().filter(|s| s.contains('=')) {
        let (k, v) = spec.split_once('=').unwrap();
        inst.set(k.parse().unwrap(), v.parse().unwrap());
    }
    for spec in a[3..].iter().filter(|s| !s.contains('=')) {
        let p: Vec<f32> = spec.split(':').map(|x| x.parse().unwrap()).collect();
        inst.note_on(p[0] as u8, p[1] as u8);
        let on = (p[2] * rate) as usize;
        let total = on + rate as usize;
        let mut bl = vec![0.0f32; total];
        let mut br = vec![0.0f32; total];
        inst.render(&mut bl[..on], &mut br[..on]);
        inst.note_off(p[0] as u8);
        inst.render(&mut bl[on..], &mut br[on..]);
        l.extend(bl);
        r.extend(br);
    }
    lg_synth::wav::write_wav_24(std::path::Path::new(&a[2]), 48000, &l, &r).unwrap();
}
