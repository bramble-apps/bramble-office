//! LG_SNAP=/path.png (checks and screenshots): draw a few frames, save the window as a PNG,
//! quit. LG_SELECT="track region" selects a region first; LG_MIXER=1 shows the Mixer.

use crate::app;

pub struct Snap {
    app: app::App,
    frames: u32,
    /// LG_SCRIPT's steps, one batch of events per frame.
    script: std::collections::VecDeque<(egui::Modifiers, Vec<egui::Event>)>,
    shot: bool,
}

impl Snap {
    pub fn new(app: app::App) -> Snap {
        let script = std::env::var("LG_SCRIPT").map(|s| parse_script(&s)).unwrap_or_default();
        Snap { app, frames: 0, script, shot: false }
    }
}

/// LG_SCRIPT="click x y; dclick x y; altdrag x y x2 y2; drag x y x2 y2; key Space; key cmd+Z;
/// wait" (points in the window): each step becomes a few frames of events.
fn parse_script(s: &str) -> std::collections::VecDeque<(egui::Modifiers, Vec<egui::Event>)> {
    use egui::{Event, Key, Modifiers, PointerButton, Pos2};
    let mut out: std::collections::VecDeque<(Modifiers, Vec<Event>)> = std::collections::VecDeque::new();
    let button = |pos: Pos2, pressed: bool, m: Modifiers| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: m };
    for step in s.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let mut it = step.splitn(2, ' ');
        let cmd = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("").trim();
        let n: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        match (cmd, n.as_slice()) {
            ("click", [x, y]) | ("dclick", [x, y]) | ("shiftclick", [x, y]) => {
                let p = Pos2::new(*x, *y);
                let m = if cmd == "shiftclick" { Modifiers::SHIFT } else { Modifiers::NONE };
                out.push_back((m, vec![Event::PointerMoved(p)]));
                for _ in 0..if cmd == "dclick" { 2 } else { 1 } {
                    out.push_back((m, vec![button(p, true, m)]));
                    out.push_back((m, vec![button(p, false, m)]));
                }
                out.push_back((Modifiers::NONE, vec![]));
            }
            ("drag", [x, y, x2, y2]) | ("altdrag", [x, y, x2, y2]) => {
                let m = if cmd == "altdrag" { Modifiers::ALT } else { Modifiers::NONE };
                let (a, b) = (Pos2::new(*x, *y), Pos2::new(*x2, *y2));
                out.push_back((m, vec![Event::PointerMoved(a)]));
                out.push_back((m, vec![button(a, true, m)]));
                for k in 1..=8 {
                    out.push_back((m, vec![Event::PointerMoved(a + (b - a) * (k as f32 / 8.0))]));
                }
                out.push_back((m, vec![button(b, false, m)]));
                out.push_back((Modifiers::NONE, vec![]));
            }
            ("key", _) => {
                let (mods, name) = match rest.rsplit_once('+') {
                    Some((m, k)) => (if m.contains("cmd") || m.contains("ctrl") { Modifiers::COMMAND | Modifiers::CTRL } else { Modifiers::NONE } | if m.contains("shift") { Modifiers::SHIFT } else { Modifiers::NONE }, k),
                    None => (Modifiers::NONE, rest),
                };
                if let Some(key) = Key::from_name(name) {
                    out.push_back((mods, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }]));
                    out.push_back((mods, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods }]));
                }
            }
            _ => out.push_back((Modifiers::NONE, vec![])),
        }
    }
    out
}

impl eframe::App for Snap {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.frames >= 3 {
            if let Some((m, ev)) = self.script.pop_front() {
                raw.events.push(egui::Event::ModifiersChanged(m));
                raw.events.extend(ev);
            }
        }
    }

    fn clear_color(&self, v: &egui::Visuals) -> [f32; 4] {
        eframe::App::clear_color(&self.app, v)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.app, ui, frame);
        let Some(out) = std::env::var_os("LG_SNAP") else { return };
        self.frames += 1;
        let ctx = ui.ctx().clone();
        if self.frames == 2 {
            if let Ok(s) = std::env::var("LG_SELECT") {
                let v: Vec<usize> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if let [t, r] = v[..] {
                    self.app.selected_track = Some(t);
                    self.app.selected_region = Some((t, r));
                    self.app.center_roll(t, r);
                }
            }
            if std::env::var_os("LG_MIXER").is_some() {
                self.app.bottom = app::Bottom::Mixer;
            }
        }
        if self.frames >= 8 && self.script.is_empty() && !self.shot {
            self.shot = true;
            if std::env::var_os("LG_DEBUG").is_some() {
                if let Some(p) = &self.app.project {
                    for t in &p.tracks {
                        eprintln!("track {:?}: {:?}", t.name, t.regions.iter().map(|r| (r.start, r.length, r.notes.len())).collect::<Vec<_>>());
                    }
                }
                eprintln!("selected {:?} undo {:?}", self.app.selected_region, self.app.undo.iter().map(|u| u.0.clone()).collect::<Vec<_>>());
            }
            let ctx2 = ctx.clone();
            let path = std::path::PathBuf::from(out);
            let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                let _ = save_png(&path, w as u32, h as u32, &bytes);
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        }
        ctx.request_repaint();
    }
}

/// A minimal PNG writer (stored deflate blocks, no compression library needed).
fn save_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn adler(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &x in data {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    let mut z = vec![0x78, 0x01];
    let n_chunks = raw.chunks(65535).count();
    for (i, chunk) in raw.chunks(65535).enumerate() {
        z.push((i + 1 == n_chunks) as u8);
        let n = chunk.len() as u16;
        z.extend_from_slice(&n.to_le_bytes());
        z.extend_from_slice(&(!n).to_le_bytes());
        z.extend_from_slice(chunk);
    }
    z.extend_from_slice(&adler(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut put = |tag: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = tag.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc(&c).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    put(b"IHDR", &ihdr);
    put(b"IDAT", &z);
    put(b"IEND", &[]);
    std::fs::write(path, out)
}
