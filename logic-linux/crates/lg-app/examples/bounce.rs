//! bounce <project.logicx | demo> <out.wav>: renders a project as Studio plays it.
use lg_app::{app, audio, demo};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (src, out) = (args.first().expect("project"), args.get(1).expect("out.wav"));
    let p = if src == "demo" { demo::song() } else { lg_core::open(std::path::Path::new(src)).expect("open") };
    let mut next = 0u8;
    let tracks: Vec<audio::PlayTrack> = p
        .tracks
        .iter()
        .map(|t| {
            let channel = if t.kind == lg_core::TrackKind::Drummer {
                9
            } else {
                let c = next;
                next = if next + 1 == 9 { 10 } else { (next + 1) % 16 };
                c
            };
            let mut notes: Vec<(u32, u32, u8, u8)> = t.regions.iter().flat_map(|r| r.notes.iter().map(|n| (n.pos, n.pos + n.len.max(1), n.pitch, n.velocity))).collect();
            notes.sort_by_key(|n| n.0);
            let audio = t.regions.iter().filter_map(|r| r.audio.as_ref().and_then(|a| audio::load_audio(&a.file)).map(|s| (r.start, std::sync::Arc::new(s)))).collect();
            audio::PlayTrack { channel, program: app::program_for(&t.name, t.kind), notes, audio, volume: 1.0, pan: 0.0, mute: false, solo: false, eq: None }
        })
        .collect();
    let end = p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(3840);
    let samples = audio::bounce(tracks, p.tempo, p.tempo_map.clone(), None, 0, end);
    std::fs::write(out, audio::wav_bytes(&samples)).expect("write");
    println!("{}: {} tracks, {:.1} s → {out}", p.name, p.tracks.len(), samples.len() as f64 / 2.0 / audio::RATE as f64);
}
