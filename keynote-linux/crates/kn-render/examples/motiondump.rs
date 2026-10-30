//! motiondump <file.key> <outdir> <w> <h> <t0> <dt> <n> [time multiplier] [preset]: for every
//! dynamic background preset (or just one), n frames from t0 every dt seconds as 8-bit gray,
//! written to <outdir>/<name>.gray (no grain). The multiplier speeds the clock up (calibration).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let d = kn_model::Document::open(&a[1]).unwrap();
    let (w, h): (u32, u32) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let (t0, dt, n): (f32, f32, usize) = (a[5].parse().unwrap(), a[6].parse().unwrap(), a[7].parse().unwrap());
    let mult: f32 = a.get(8).and_then(|m| m.parse().ok()).unwrap_or(1.0);
    for (id, name) in d.motion_presets() {
        if a.get(9).is_some_and(|only| *only != name) {
            continue;
        }
        let Some(mut mb) = kn_model::motionbg::read(&d.file, id) else { continue };
        mb.nums.insert("grain_intensity", 0.0);
        let mut out = Vec::new();
        for i in 0..n {
            let px = kn_render::motion::render(&mb, (t0 + dt * i as f32) * mult, w, h);
            out.extend(px.chunks(4).map(|p| ((p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) / 256) as u8));
        }
        std::fs::write(format!("{}/{}.gray", a[2], name.replace(' ', "-")), out).unwrap();
    }
}
