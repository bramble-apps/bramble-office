//! Keynote's Dynamic Backgrounds, drawn on the CPU. Seven renderer families (the stored
//! `background_name`), driven by the stored parameters and the "Colors" gradient. The output is
//! soft by nature (Keynote blurs these heavily), so it is rendered small and scaled up.

use kn_model::motionbg::MotionBg;
use kn_model::Color;

/// Render `bg` at time `t` (seconds) into an RGBA buffer of `w` x `h`.
pub fn render(bg: &MotionBg, t: f32, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w.max(2), h.max(2));
    let mut out = vec![0u8; (w * h * 4) as usize];
    let pal = Palette::new(bg);
    let p = Params::new(bg, t, w, h);
    // per-column data for the column-based families (terrain, waves)
    let cols = Columns::new(&p, &pal, w);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16);
    let rows_per = (h as usize).div_ceil(threads);
    std::thread::scope(|s| {
        for (ti, chunk) in out.chunks_mut(rows_per * w as usize * 4).enumerate() {
            let (p, pal, cols) = (&p, &pal, &cols);
            s.spawn(move || {
                let y0 = ti * rows_per;
                for (ri, row) in chunk.chunks_mut(w as usize * 4).enumerate() {
                    let y = y0 + ri;
                    for x in 0..w as usize {
                        let c = pixel(p, pal, cols, x, y);
                        let c = post(p, c, x, y);
                        let o = x * 4;
                        row[o] = (c[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        row[o + 1] = (c[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        row[o + 2] = (c[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        row[o + 3] = 255;
                    }
                }
            });
        }
    });
    out
}

/// The still Keynote stores with a dynamic background (the "poster frame"): a JPEG at the
/// slide's size, used by readers that can't animate it.
pub fn poster_jpeg(bg: &MotionBg, w: u32, h: u32) -> Option<Vec<u8>> {
    let (sw, sh) = ((w / 2).max(16), (h / 2).max(16));
    let px = render(bg, 0.0, sw, sh);
    let small = image::RgbaImage::from_raw(sw, sh, px)?;
    let big = image::imageops::resize(&small, w.max(16), h.max(16), image::imageops::FilterType::Triangle);
    let rgb = image::DynamicImage::ImageRgba8(big).to_rgb8();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
    enc.encode_image(&rgb).ok()?;
    Some(out)
}

#[derive(Clone, Copy, PartialEq)]
enum Family {
    Linear,
    Terrain,
    Flyover,
    Diffusion,
    Waves,
    WavesSym,
    Spheres,
}

struct Params {
    fam: Family,
    t: f32,
    /// Real time (film grain changes at its own rate, whatever the motion speed).
    t_real: f32,
    w: f32,
    h: f32,
    aspect: f32,
    // shared
    speed: f32,
    scale: f32,
    angle: f32,
    contrast: f32,
    brightness: f32,
    saturation: f32,
    gamma: f32,
    grain: f32,
    grain_speed: f32,
    palette_offset: f32,
    // noise
    amp: f32,
    octaves: f32,
    noise_scale: f32,
    noise_speed: f32,
    movement: f32,
    fog_distance: f32,
    fog_power: f32,
    zoom: f32,
    // waves
    density: f32,
    wave_amp: f32,
    envelope: f32,
    y_offset: f32,
    blur: f32,
    opacity: f32,
    offset: f32,
    multiply: bool,
    background: [f32; 3],
}

/// How fast each built-in background moves compared with our renderer, fitted to Keynote 15.4's
/// playback of the White theme presets (Mac round 25: frames 2 s apart, motion compared at 0.5,
/// 1 and 2 s). Other backgrounds use their family's middle value.
const CALIBRATION: &[(&str, f32)] = &[
    ("Rainbow", 2.833),
    ("Ocean Floor", 1.06),
    ("Neptune", 0.263),
    ("Snow Day", 0.224),
    ("Neon Splash", 0.432),
    ("Jade Cliffs", 0.843),
    ("Stormy Seas", 0.63),
    ("Poolside", 0.269),
    ("Nebula", 0.75),
    ("Black Smoke", 0.89),
    ("Lime Juice", 0.213),
    ("Solar Flare", 0.084),
    ("Sunset Dunes", 1.82),
    ("Morning Surf", 0.12),
    ("Lava Lamp", 0.563),
    ("Flamingo", 0.449),
    ("Cool Breeze", 0.09),
    ("Purple Vista", 0.561),
];

fn time_factor(bg: &MotionBg) -> f32 {
    if let Some((_, f)) = CALIBRATION.iter().find(|(n, _)| *n == bg.name) {
        return *f;
    }
    match bg.family.as_str() {
        "KNGradientTerrainBase" => 0.97,
        "KNGradientTerrainFlyover" => 0.63,
        "KNGradientDiffusionBase" => 0.3,
        "KNWavesBase" | "KNWavesSymmetrical" => 0.11,
        "KNGraphicSpheresBase" => 0.43,
        _ => 0.28,
    }
}

fn rgb(c: Color) -> [f32; 3] {
    [c.r, c.g, c.b]
}

impl Params {
    fn new(bg: &MotionBg, t: f32, w: u32, h: u32) -> Params {
        let n = |k: &str, d: f64| bg.num(k, d) as f32;
        let fam = match bg.family.as_str() {
            "KNGradientTerrainBase" => Family::Terrain,
            "KNGradientTerrainFlyover" => Family::Flyover,
            "KNGradientDiffusionBase" => Family::Diffusion,
            "KNWavesBase" => Family::Waves,
            "KNWavesSymmetrical" => Family::WavesSym,
            "KNGraphicSpheresBase" => Family::Spheres,
            _ => Family::Linear,
        };
        Params {
            fam,
            t: t * time_factor(bg),
            t_real: t,
            w: w as f32,
            h: h as f32,
            aspect: w as f32 / h as f32,
            speed: n("speed", 50.0),
            scale: n("scale", 1.0),
            angle: n("angle", 45.0),
            contrast: n("contrast", 1.0),
            brightness: n("brightness", 0.0),
            saturation: n("saturation", 1.0) * n("color_saturation", 1.0).max(0.0).sqrt(),
            gamma: n("gamma", 1.0) * n("color_gamma", 1.0),
            grain: n("grain_intensity", 0.3),
            grain_speed: n("grain_speed", 1.0),
            palette_offset: n("color_palette_offset", 0.0),
            amp: n("noise_amplitude", 2.0),
            octaves: n("noise_octaves", 3.0).clamp(1.0, 6.0),
            noise_scale: n("noise_scale_horizontal", n("noise_scale", 1.0) as f64),
            noise_speed: n("noise_speed", 1.0),
            movement: n("noise_movement", 0.0),
            fog_distance: n("fog_distance", 1.0),
            fog_power: n("fog_power", 6.0),
            zoom: n("zoom", 50.0),
            density: n("density", 3.0),
            wave_amp: n("amplitude", 1.0),
            envelope: n("envelope", 0.5),
            y_offset: n("y_offset", 0.0),
            blur: n("blur", 0.5),
            opacity: n("motion_background_opacity", 0.3),
            offset: n("offset", 1.0),
            multiply: bg.blend_mode.as_deref() == Some("Multiply"),
            background: bg.background_color.map(rgb).unwrap_or([0.0, 0.0, 0.0]),
        }
    }
}

struct Palette {
    stops: Vec<(f32, [f32; 3])>,
}

impl Palette {
    fn new(bg: &MotionBg) -> Palette {
        let mut stops: Vec<(f32, [f32; 3])> = bg.colors.iter().map(|(f, c)| (*f, rgb(*c))).collect();
        if stops.is_empty() {
            stops = vec![(0.0, [0.2, 0.3, 0.8]), (1.0, [0.9, 0.3, 0.6])];
        }
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        Palette { stops }
    }
    fn at(&self, x: f32) -> [f32; 3] {
        let s = &self.stops;
        if x <= s[0].0 {
            return s[0].1;
        }
        for w in s.windows(2) {
            if x <= w[1].0 {
                let k = ((x - w[0].0) / (w[1].0 - w[0].0).max(1e-5)).clamp(0.0, 1.0);
                let k = k * k * (3.0 - 2.0 * k);
                return mix3(w[0].1, w[1].1, k);
            }
        }
        s[s.len() - 1].1
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}

// ---------------------------------------------------------------------------------- noise

fn hash(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xi as i32, yi as i32);
    let a = hash(xi, yi);
    let b = hash(xi + 1, yi);
    let c = hash(xi, yi + 1);
    let d = hash(xi + 1, yi + 1);
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

fn fbm(x: f32, y: f32, octaves: f32) -> f32 {
    let mut v = 0.0;
    let mut a = 0.5;
    let (mut x, mut y) = (x, y);
    let n = octaves.ceil() as i32;
    let mut norm = 0.0;
    for i in 0..n {
        let w = if (i as f32) < octaves.floor() { 1.0 } else { octaves.fract().max(0.001) };
        v += a * w * noise(x, y);
        norm += a * w;
        a *= 0.5;
        // rotate and grow so octaves don't line up
        let (nx, ny) = (x * 1.6 + y * 1.2, -x * 1.2 + y * 1.6);
        x = nx + 17.3;
        y = ny + 9.1;
    }
    v / norm.max(1e-4)
}

// ---------------------------------------------------------------------------------- columns

/// Terrain ridges / wave curves evaluated once per column.
struct Columns {
    /// For each layer (back to front): screen-space ridge height (0 top..1 bottom) per column,
    /// the layer's colour and opacity.
    layers: Vec<(Vec<f32>, [f32; 3], f32, f32)>,
    /// Terrain: light per layer and column (slope facing the light = brighter).
    light: Vec<Vec<f32>>,
    sky_top: [f32; 3],
    sky_bottom: [f32; 3],
}

impl Columns {
    fn new(p: &Params, pal: &Palette, w: u32) -> Columns {
        let mut layers = Vec::new();
        let mut light = Vec::new();
        let (mut sky_top, mut sky_bottom) = (pal.at(0.0), pal.at(0.0));
        let t = p.t;
        match p.fam {
            Family::Terrain if p.noise_scale < 0.5 => {
                // very broad hills (Nebula): one smooth crest, coloured by depth below it
                let drift = t * 0.12 * (p.movement / 100.0);
                let morph = t * 0.05 * p.noise_speed.max(0.5).sqrt();
                let amp = (p.amp / 4.0).clamp(0.15, 2.0);
                let mut ys = Vec::with_capacity(w as usize);
                for x in 0..w {
                    let u = x as f32 / w as f32 - 0.5;
                    let h = fbm(u * p.aspect * 0.55 + drift + 3.1, 0.7 + morph, 2.0) - 0.5;
                    ys.push(0.56 - u * 0.08 - h * amp * 1.2);
                }
                sky_top = mix3(pal.at(0.0), pal.at(0.25), 0.1);
                sky_bottom = mix3(pal.at(0.0), pal.at(0.25), 0.16);
                layers.push((ys, pal.at(0.25), 1.0, 0.03 + p.blur * 0.02));
            }
            Family::Terrain | Family::Flyover => {
                let n = 28;
                // flyover: layers move towards the viewer; terrain: the ridges drift sideways
                let fly = if p.fam == Family::Flyover { t * 0.04 * (p.movement.abs().max(20.0) / 100.0) * p.movement.signum().max(0.0).max(0.5) } else { 0.0 };
                // terrain: the land travels sideways (movement) and its shape slowly changes
                // (noise speed)
                let drift = t * 0.12 * (p.movement / 100.0);
                let morph = t * 0.05 * p.noise_speed.max(0.5).sqrt();
                sky_top = pal.at(0.0);
                sky_bottom = mix3(pal.at(0.0), pal.at(0.2), 0.5);
                // zoom: the camera's field of view (larger = wider view = finer hills)
                let freq = (p.noise_scale / 4.0).clamp(1.1, 3.0) * (p.zoom / 50.0).clamp(0.5, 2.0);
                let amp = (p.amp / 4.0).clamp(0.15, 2.0);
                for i in 0..n {
                    // depth 1 = far, 0 = near; flyover layers loop forward
                    let mut d = 1.0 - (i as f32 + 0.5) / n as f32;
                    // each flyover layer keeps its own hills for a whole pass (from far to near);
                    // a new pass starts far away in the fog, where the change can't be seen
                    let mut pass = 0.0;
                    if p.fam == Family::Flyover {
                        let z = d + fly;
                        pass = z.floor();
                        d = z - pass;
                    }
                    let persp = 1.0 / (0.35 + d * 2.2);
                    // world x under screen x grows with depth: far ridges look finer
                    let horizon = if p.fam == Family::Flyover { 0.30 } else { 0.42 };
                    let base = horizon + (1.0 - d) * (1.0 - horizon) * 0.95;
                    let seed = (i as f32) * 7.31 + pass * 3.7;
                    let mut ys = Vec::with_capacity(w as usize);
                    for x in 0..w {
                        let u = x as f32 / w as f32 - 0.5;
                        let nx = u * p.aspect * freq * (0.25 + d * 1.6) * 3.0 + seed + drift;
                        let hgt = fbm(nx, seed * 0.37 + morph, (p.octaves * 0.5).max(1.0));
                        let ridge = (hgt - 0.3).max(0.0) * amp * persp * 0.32;
                        ys.push(base - ridge);
                    }
                    // colour by nearness, lit tops lighter; fog towards the sky in the distance
                    let near = 1.0 - d;
                    let c = if p.fam == Family::Flyover { 0.28 + 0.42 * near } else { 0.15 + 0.85 * near };
                    let mut col = pal.at(c);
                    let fog = (d / p.fog_distance.max(0.2)).clamp(0.0, 1.0).powf(p.fog_power.max(1.0) / 4.0);
                    col = mix3(col, sky_bottom, fog * 0.85);
                    let fade = if p.fam == Family::Flyover { ((1.0 - d) * 6.0).min(d * 12.0).clamp(0.0, 1.0) } else { 1.0 };
                    let lt: Vec<f32> = (0..ys.len()).map(|x| {
                        let a = ys[x.saturating_sub(2)];
                        let b = ys[(x + 2).min(ys.len() - 1)];
                        // rising to the right faces the light (from the upper left)
                        (1.0 + (a - b) * w as f32 * 0.02).clamp(0.6, 1.35)
                    }).collect();
                    light.push(lt);
                    layers.push((ys, col, fade, 0.004));
                }
            }
            Family::Waves | Family::WavesSym => {
                let n = (p.density * 2.0).round().clamp(3.0, 12.0) as usize;
                let soft = 0.004 + p.blur * 0.05;
                for i in 0..n {
                    let fi = i as f32;
                    let col = pal.at((fi + 0.5) / n as f32);
                    let k1 = 0.6 + (fi * 0.37).fract() * p.scale * 0.2;
                    let k2 = 1.3 + (fi * 0.61).fract() * p.scale * 0.3;
                    let ph = fi * 1.7 + p.offset + t * p.speed * 0.02 * (1.0 + fi * 0.13);
                    let mut ys = Vec::with_capacity(w as usize);
                    for x in 0..w {
                        let u = x as f32 / w as f32;
                        let env = if p.envelope > 0.0 { (1.0 - p.envelope * 0.5 * (2.0 * u - 1.0).powi(2)).max(0.2) } else { 1.0 };
                        let a = (0.04 + 0.045 * p.wave_amp.min(10.0)) * env;
                        let v = (u * k1 * 3.0 + ph).sin() * 0.6 + (u * k2 * 3.0 - ph * 1.3).sin() * 0.4;
                        ys.push(v * a);
                    }
                    layers.push((ys, col, p.opacity.max(0.15).min(0.6), soft));
                }
            }
            _ => {}
        }
        Columns { layers, light, sky_top, sky_bottom }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let k = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

fn pixel(p: &Params, pal: &Palette, cols: &Columns, x: usize, y: usize) -> [f32; 3] {
    let u = (x as f32 + 0.5) / p.w;
    let v = (y as f32 + 0.5) / p.h;
    let t = p.t;
    match p.fam {
        Family::Linear => {
            // a soft gradient along `angle` whose colours slowly flow, warped by large noise
            let a = p.angle.to_radians();
            let (dx, dy) = (a.cos(), -a.sin());
            let (px, py) = ((u - 0.5) * p.aspect, v - 0.5);
            let s = 1.0 / p.scale.max(0.2);
            let warp = fbm(px * 0.9 + t * 0.09 * p.speed / 40.0, py * 0.9 - t * 0.07 * p.speed / 40.0, 2.0) - 0.5;
            let along = (px * dx + py * dy) / p.aspect.max(1.0) * s * 1.1 + 0.55 + warp * 0.3;
            let flow = (t * p.speed * 0.006).sin() * (0.15 + 0.1 * p.scale.min(3.0));
            let x = (along + flow + p.palette_offset).clamp(-0.5, 1.5);
            // mirror outside 0..1 so the ends don't clamp hard
            let x = if x < 0.0 { -x } else if x > 1.0 { 2.0 - x } else { x };
            pal.at(x)
        }
        Family::Diffusion => {
            let (px, py) = ((u - 0.5) * p.aspect * p.scale * 0.55, (v - 0.5) * p.scale * 0.55);
            let ts = t * 0.09 * (p.noise_speed + p.speed * 0.01);
            let q = (fbm(px + ts, py - ts * 0.7, 2.0), fbm(px - ts * 0.6 + 5.2, py + ts + 1.3, 2.0));
            let n = fbm(px * 0.7 + q.0 * p.amp * 0.45, py * 0.7 + q.1 * p.amp * 0.45, 2.0);
            pal.at(((n - 0.2) * 1.7).clamp(0.0, 1.0))
        }
        Family::Terrain if p.noise_scale < 0.5 => {
            let r = cols.layers.first().map(|l| l.0[x]).unwrap_or(0.5);
            let soft = cols.layers.first().map(|l| l.3).unwrap_or(0.03);
            // sky: dark, with the crest's glow rising into it
            let glow = (-((r - v).max(0.0)) / 0.09).exp() * 0.35;
            let sky = mix3(mix3(cols.sky_top, cols.sky_bottom, v), pal.at(0.25), glow);
            // below the crest: bright at the crest, through the palette's middle to its depth
            let d = ((v - r) / (1.0 - r).max(0.05)).clamp(0.0, 1.0);
            let ground = pal.at(0.25 + d.powf(1.5) * 0.5);
            mix3(sky, ground, smoothstep(r - soft, r + soft, v))
        }
        Family::Terrain | Family::Flyover => {
            let mut c = mix3(cols.sky_top, cols.sky_bottom, v);
            for (li, (ys, col, alpha, soft)) in cols.layers.iter().enumerate() {
                let r = ys[x];
                let cover = smoothstep(r - soft, r + soft, v) * alpha;
                if cover > 0.0 {
                    // lit slopes lighter, a little darker further below the ridge line
                    let l = cols.light.get(li).map(|l| l[x]).unwrap_or(1.0);
                    let shade = l * (1.0 - ((v - r).max(0.0) * 0.6).min(0.15));
                    let lit = [col[0] * shade, col[1] * shade, col[2] * shade];
                    c = mix3(c, lit, cover);
                }
            }
            c
        }
        Family::Waves | Family::WavesSym => {
            let mut c = p.background;
            let center = if p.fam == Family::WavesSym { 0.5 - p.y_offset * 0.25 } else { 0.62 - p.y_offset * 0.25 };
            for (ys, col, alpha, soft) in &cols.layers {
                let r = ys[x];
                let cover = if p.fam == Family::WavesSym {
                    // a band symmetric about the centre line
                    let half = r.abs() + 0.02;
                    1.0 - smoothstep(half - soft, half + soft, (v - center).abs())
                } else {
                    smoothstep(center + r - soft, center + r + soft, v)
                };
                if cover > 0.0 {
                    let k = cover * alpha;
                    c = if p.multiply { mix3(c, [c[0] * col[0], c[1] * col[1], c[2] * col[2]], k) } else { mix3(c, *col, k) };
                }
            }
            c
        }
        Family::Spheres => {
            // flat, overlapping blobs drifting slowly (metaballs), in palette colours
            let (px, py) = (u * p.aspect, v);
            let mut c = pal.at(0.5);
            let layers = 3;
            let picks = [1.0, 0.75, 0.25];
            let ts = t * (p.speed * 0.004).max(0.03) + p.offset;
            for l in 0..layers {
                let fl = l as f32;
                let mut field = 0.0;
                // each layer: a cluster of balls orbiting a slowly moving centre, so they merge
                // into one organic shape
                let lx = p.aspect * (0.5 + 0.4 * (ts * 0.31 + fl * 2.4).sin());
                let ly = 0.5 + 0.4 * (ts * 0.23 + fl * 1.7).cos();
                for b in 0..4 {
                    let fb = b as f32 + fl * 4.0;
                    let cx = lx + 0.35 * (ts * (0.7 + 0.13 * fb) + fb * 2.1).sin();
                    let cy = ly + 0.3 * (ts * (0.5 + 0.11 * fb) + fb * 1.3).cos();
                    let r = 0.13 + 0.1 * (fb * 0.77).fract() * (p.noise_scale / 5.0).clamp(0.5, 1.5);
                    let d2 = (px - cx).powi(2) + (py - cy).powi(2);
                    field += r * r / d2.max(1e-4);
                }
                let edge = 2.0 / p.h;
                let cover = smoothstep(1.0 - edge * 3.0, 1.0 + edge * 3.0, field);
                if cover > 0.0 {
                    c = mix3(c, pal.at(picks[l]), cover * (0.75 + 0.25 * p.opacity.min(1.0)));
                }
            }
            c
        }
    }
}

/// Colour grading and film grain (contrast, brightness, saturation, gamma, grain).
fn post(p: &Params, c: [f32; 3], x: usize, y: usize) -> [f32; 3] {
    let mut c = c;
    let l = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let sat = p.saturation.clamp(0.0, 2.0);
    for v in c.iter_mut() {
        *v = l + (*v - l) * (1.0 + (sat - 1.0) * 0.7);
        *v = (*v - 0.5) * (1.0 + (p.contrast.clamp(0.3, 2.0) - 1.0) * 0.6) + 0.5 + p.brightness;
        *v = v.max(0.0).powf(p.gamma.clamp(0.3, 3.0).powf(0.5));
    }
    if p.grain > 0.0 {
        let frame = (p.t_real * (6.0 + p.grain_speed.clamp(0.0, 100.0) * 0.18)).floor() as i32;
        // fine and faint, like Keynote (the frame is scaled up, so strong grain turns blotchy)
        let g = (hash(x as i32 + frame * 7919, y as i32 - frame * 104729) - 0.5) * p.grain * 0.01;
        for v in c.iter_mut() {
            *v += g;
        }
    }
    c
}
