//! The Bounce window's Burn: CD/DVD destination, as Logic's: Mode (CDDA: an audio CD, DVDA:
//! DVD-Audio), Media Status, Simulate write only, Write as multi-session (CDDA only), Device,
//! Speed (the burner is asked for its speeds the first time the menu opens) and Dithering.
//! Choosing it turns Split off (and Split turns it off); OK becomes Bounce and Burn. CDDA burns a
//! 44.1 kHz 16-bit copy of the bounce with the system's disc tool (wodim, cdrecord or cdrskin),
//! titled after the output strip. DVD-Audio needs authoring Linux has no tool for: Studio says so.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crate::app::App;
use crate::look;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Cdda,
    Dvda,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Burn {
    pub on: bool,
    pub mode: Mode,
    pub simulate: bool,
    pub multi: bool,
    pub device: String,
    /// None: the burner's maximum.
    pub speed: Option<u32>,
    /// The speeds the burner gave (asked once).
    pub speeds: Option<Vec<u32>>,
    /// Dithering (share::Dither's names).
    pub dither: usize,
    pub media: Option<String>,
}

pub const DITHERS: [&str; 5] = ["None", "POWr #1 (Dithering)", "POWr #2 (Noise Shaping)", "POWr #3 (Noise Shaping)", "UV22HR"];
/// Red Book audio's sample rate.
pub const CD_RATE: u32 = 44_100;

thread_local! {
    static ARMED: RefCell<Option<(Burn, String)>> = const { RefCell::new(None) };
}

/// The disc burners (/dev/sr0 …).
pub fn devices() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir("/dev").map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("sr") && n[2..].chars().all(|c| c.is_ascii_digit())).map(|n| format!("/dev/{n}")).collect()).unwrap_or_default();
    v.sort();
    v
}

/// The burning tool found on the PATH.
pub fn tool() -> Option<&'static str> {
    ["wodim", "cdrecord", "cdrskin"].into_iter().find(|t| std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file())))
}

/// The write speeds in a `wodim -prcap` listing ("Write speed # 0: 7056 kB/s CD-RW 40x …").
pub fn parse_speeds(text: &str) -> Vec<u32> {
    let mut v: Vec<u32> = text
        .lines()
        .filter(|l| l.contains("Write speed #"))
        .filter_map(|l| {
            let i = l.find("CD")?;
            l[i..].split_whitespace().find_map(|w| w.strip_suffix('x').and_then(|n| n.parse().ok()))
        })
        .collect();
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

/// Asks the burner for its speeds (the Speed menu's first opening).
pub fn query_speeds(dev: &str) -> Vec<u32> {
    let out = tool().and_then(|t| std::process::Command::new(t).arg("-prcap").arg(format!("dev={dev}")).stdin(std::process::Stdio::null()).output().ok());
    let v = out.map(|o| parse_speeds(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default();
    if v.is_empty() { vec![48, 40, 32, 24, 16, 8, 4] } else { v }
}

/// Whether a disc is in the drive (udev's ID_CDROM_MEDIA).
pub fn media_status(dev: &str) -> String {
    if dev.is_empty() {
        return "No burner found".into();
    }
    let out = std::process::Command::new("udevadm").args(["info", "--query=property", &format!("--name={dev}")]).stdin(std::process::Stdio::null()).output().ok();
    let props = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
    if props.lines().any(|l| l == "ID_CDROM_MEDIA_BLANK=1") {
        "Blank disc inserted".into()
    } else if props.lines().any(|l| l == "ID_CDROM_MEDIA=1") {
        "Disc inserted".into()
    } else {
        "No disc".into()
    }
}

/// The disc tool's arguments for an audio CD of `wav`.
pub fn args(b: &Burn, wav: &Path) -> Vec<String> {
    let mut a = vec![format!("dev={}", if b.device.is_empty() { "/dev/sr0" } else { &b.device })];
    if let Some(s) = b.speed {
        a.push(format!("speed={s}"));
    }
    if b.simulate {
        a.push("-dummy".into());
    }
    // (multi-session: track at once; otherwise disc at once)
    if b.multi && b.mode == Mode::Cdda {
        a.push("-multi".into());
        a.push("-tao".into());
    } else {
        a.push("-dao".into());
    }
    a.push("-audio".into());
    a.push("-pad".into());
    a.push(wav.display().to_string());
    a
}

/// The Burn options (the Bounce window's right side). `bits` is the uncompressed resolution.
pub fn options(ui: &mut egui::Ui, b: &mut Burn, bits: u16) {
    if b.device.is_empty() {
        b.device = devices().into_iter().next().unwrap_or_default();
    }
    if b.media.is_none() {
        b.media = Some(media_status(&b.device));
    }
    egui::Grid::new("share-burn").num_columns(2).spacing(egui::vec2(8.0, 6.0)).show(ui, |ui| {
        ui.label("Mode:");
        egui::ComboBox::from_id_salt("burn-mode").selected_text(if b.mode == Mode::Cdda { "CDDA" } else { "DVDA" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut b.mode, Mode::Cdda, "CDDA");
            ui.selectable_value(&mut b.mode, Mode::Dvda, "DVDA");
        });
        ui.end_row();
        ui.label("Media Status:");
        ui.horizontal(|ui| {
            ui.label(b.media.clone().unwrap_or_default());
            if ui.small_button("↻").on_hover_text("Check again").clicked() {
                b.media = Some(media_status(&b.device));
            }
        });
        ui.end_row();
        ui.label("");
        ui.vertical(|ui| {
            ui.checkbox(&mut b.simulate, "Simulate write only");
            ui.add_enabled(b.mode == Mode::Cdda, egui::Checkbox::new(&mut b.multi, "Write as multi-session"));
        });
        ui.end_row();
        ui.label("Device:");
        let devs = devices();
        egui::ComboBox::from_id_salt("burn-dev").selected_text(if b.device.is_empty() { "None".to_string() } else { b.device.clone() }).show_ui(ui, |ui| {
            for d in &devs {
                if ui.selectable_label(b.device == *d, d).clicked() {
                    b.device = d.clone();
                    b.speeds = None;
                    b.media = Some(media_status(d));
                }
            }
        });
        ui.end_row();
        ui.label("Speed:");
        let cur = b.speed.map_or("Maximum".to_string(), |s| format!("{s}x"));
        let resp = egui::ComboBox::from_id_salt("burn-speed").selected_text(cur).show_ui(ui, |ui| {
            // (the burner is asked the first time the menu opens)
            if b.speeds.is_none() {
                b.speeds = Some(query_speeds(&b.device));
            }
            if ui.selectable_label(b.speed.is_none(), "Maximum").clicked() {
                b.speed = None;
            }
            for s in b.speeds.clone().unwrap_or_default() {
                if ui.selectable_label(b.speed == Some(s), format!("{s}x")).clicked() {
                    b.speed = Some(s);
                }
            }
        });
        let _ = resp;
        ui.end_row();
        ui.label("Dithering:");
        egui::ComboBox::from_id_salt("burn-dither").selected_text(DITHERS[b.dither.min(4)]).show_ui(ui, |ui| {
            for (i, d) in DITHERS.iter().enumerate() {
                ui.selectable_value(&mut b.dither, i, *d);
            }
        });
        ui.end_row();
    });
    if b.mode == Mode::Dvda && bits == 16 {
        ui.label(egui::RichText::new("With a 16-bit resolution DVD burning is no longer available.").color(look::record_red()));
    }
    if b.mode == Mode::Dvda {
        ui.label(egui::RichText::new("DVD-Audio needs disc authoring Linux has no tool for.").color(look::text_dim()));
    } else if tool().is_none() {
        ui.label(egui::RichText::new("Install wodim (or cdrecord / cdrskin) to burn CDs.").color(look::text_dim()));
    }
}

/// OK in the Bounce window with Burn on: the burn waits for the bounce (`title`: the disc's name,
/// the output strip's).
pub fn arm(b: &Burn, title: &str) {
    ARMED.with(|a| *a.borrow_mut() = b.on.then(|| (b.clone(), title.to_string())));
}

/// A Red Book copy: 44.1 kHz, 16-bit stereo (a higher rate converted in the copy).
pub fn cd_copy(src: &Path, dest: &Path) -> Result<(), String> {
    let a = lg_synth::wav::read_audio(src).map_err(|e| e.to_string())?;
    let l = a.channels.first().cloned().unwrap_or_default();
    let r = a.channels.get(1).cloned().unwrap_or_else(|| l.clone());
    let n_out = (l.len() as f64 * CD_RATE as f64 / a.rate.max(1) as f64).round() as usize;
    let step = a.rate as f64 / CD_RATE as f64;
    let at = |v: &[f32], x: f64| -> f32 {
        let i = x.floor() as usize;
        let f = (x - i as f64) as f32;
        let p = v.get(i).copied().unwrap_or(0.0);
        let q = v.get(i + 1).copied().unwrap_or(p);
        p + (q - p) * f
    };
    let mut data: Vec<u8> = Vec::with_capacity(n_out * 4);
    for k in 0..n_out {
        for ch in [&l, &r] {
            let s = (at(ch, k as f64 * step).clamp(-1.0, 1.0) * 32767.0).round() as i16;
            data.extend(s.to_le_bytes());
        }
    }
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend(b"RIFF");
    out.extend(((36 + data.len()) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(CD_RATE.to_le_bytes());
    out.extend((CD_RATE * 4).to_le_bytes());
    out.extend(4u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    std::fs::write(dest, out).map_err(|e| e.to_string())
}

/// After the bounce: burns its first file.
pub fn after_bounce(app: &mut App, files: &[PathBuf]) {
    let Some((b, title)) = ARMED.with(|a| a.borrow_mut().take()) else { return };
    let Some(src) = files.iter().find(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav") || e.eq_ignore_ascii_case("aif") || e.eq_ignore_ascii_case("aiff"))) else {
        app.tell("Burn: the bounce made no uncompressed file to burn");
        return;
    };
    if b.mode == Mode::Dvda {
        app.tell("DVD-Audio needs disc authoring Linux has no tool for: the bounce is saved, burn it with a DVD authoring tool");
        return;
    }
    let Some(t) = tool() else {
        app.tell("Burn: install wodim (or cdrecord / cdrskin) to burn CDs");
        return;
    };
    let cd = std::env::temp_dir().join(format!("{title}-cd.wav"));
    if let Err(e) = cd_copy(src, &cd) {
        app.tell(format!("Burn: couldn't make the CD copy: {e}"));
        return;
    }
    match std::process::Command::new(t).args(args(&b, &cd)).stdin(std::process::Stdio::null()).spawn() {
        Ok(_) => app.tell(format!("Burning “{title}”{}", if b.simulate { " (simulated)" } else { "" })),
        Err(e) => app.tell(format!("Couldn't start {t}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_and_arguments() {
        let t = "  Write speed # 0:  7056 kB/s CD-RW   40x (CD  40x, DVD  5x)\n  Write speed # 1:  5645 kB/s CD-RW   32x (CD  32x, DVD  4x)\n  Write speed # 2: 4234 kB/s CD-R 24x\n";
        assert_eq!(parse_speeds(t), vec![40, 32, 24]);
        let mut b = Burn { on: true, device: "/dev/sr1".into(), speed: Some(16), simulate: true, ..Default::default() };
        let a = args(&b, Path::new("/tmp/x.wav"));
        assert_eq!(a, ["dev=/dev/sr1", "speed=16", "-dummy", "-dao", "-audio", "-pad", "/tmp/x.wav"]);
        b.multi = true;
        assert!(args(&b, Path::new("/x.wav")).contains(&"-multi".to_string()));
        b.mode = Mode::Dvda;
        assert!(!args(&b, Path::new("/x.wav")).contains(&"-multi".to_string()));
    }

    #[test]
    fn cd_copy_is_red_book() {
        let dir = std::env::temp_dir().join(format!("lg-cd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("in.wav");
        std::fs::write(&src, crate::audio::wav_bytes(&vec![0.25; crate::audio::RATE as usize * 2])).unwrap();
        let dst = dir.join("cd.wav");
        cd_copy(&src, &dst).unwrap();
        let a = lg_synth::wav::read_audio(&dst).unwrap();
        assert_eq!(a.rate, CD_RATE);
        assert_eq!(a.channels.len(), 2);
        assert!((a.channels[0].len() as i64 - CD_RATE as i64).abs() <= 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
