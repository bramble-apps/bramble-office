//! Prints Smart Tempo's analysis and the transients of an audio file: `analyze FILE [variable]`.
fn main() {
    let path = std::env::args().nth(1).expect("a WAV or AIFF file");
    let variable = std::env::args().nth(2).is_some();
    let a = lg_audio::AudioFile::read(std::path::Path::new(&path)).expect("readable");
    println!("{} Hz, {} ch, {} bits, {:.2} s", a.rate, a.channels, a.bits, a.seconds());
    let mono = a.mono();
    let t = lg_audio::transients::detect_mono(&mono, a.rate, 0.5);
    println!("{} transients: {:?}", t.len(), t.iter().take(12).map(|f| *f as f64 / a.rate as f64).collect::<Vec<_>>());
    match lg_audio::tempo::analyze(&mono, a.rate, variable) {
        Some(m) => println!("tempo {:.1}, {}/{}, downbeat {} ({:.3} s), first beat {:?}", m.tempo(), m.beats_per_bar, m.beat_unit, m.downbeat, m.beats[m.downbeat], m.bar_beat(0)),
        None => println!("no tempo"),
    }
}
