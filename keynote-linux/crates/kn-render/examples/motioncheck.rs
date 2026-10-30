//! motioncheck <file.key>: for every dynamic background preset, how much consecutive frames
//! (1/30 s apart, small size) differ over 60 s: mean change and the largest jump.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let secs: f32 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    for (id, name) in d.motion_presets() {
        let Some(mut mb) = kn_model::motionbg::read(&d.file, id) else { continue };
        if std::env::var_os("NOGRAIN").is_some() {
            mb.nums.insert("grain_intensity", 0.0);
        }
        let (w, h) = (96u32, 54u32);
        let mut prev: Option<Vec<u8>> = None;
        let (mut sum, mut n, mut worst, mut at) = (0.0f64, 0, 0.0f64, 0.0f32);
        let mut t = 0.0f32;
        while t < secs {
            let f = kn_render::motion::render(&mb, t, w, h);
            if let Some(p) = &prev {
                let diff: f64 = f.iter().zip(p).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / f.len() as f64;
                sum += diff;
                n += 1;
                if diff > worst {
                    worst = diff;
                    at = t;
                }
            }
            prev = Some(f);
            t += 1.0 / 30.0;
        }
        let mean = sum / n.max(1) as f64;
        println!("{:<22} {:<26} mean {:7.3} worst {:7.2} at {:5.1}s ratio {:6.1}", name, mb.family, mean, worst, at, worst / mean.max(1e-6));
    }
}
