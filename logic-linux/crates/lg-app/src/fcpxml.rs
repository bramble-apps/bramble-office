//! Final Cut Pro XML (xmeml), as Logic's:
//! - File ▸ Export ▸ Project to Final Cut Pro XML…: a dialog with Save As, Export as Final Cut
//!   Compound Clip and Include video in export. Audio and software instrument tracks are bounced
//!   to audio files (external MIDI tracks are left out; Studio has no I/O or External
//!   Instrument plug-ins, so the bounce never needs realtime) and the XML places them, with each
//!   track's volume and stereo pan automation as keyframes.
//! - File ▸ Import ▸ Final Cut Pro XML…: the sequence's audio clips on new audio tracks (where
//!   they sit, their in points), with their volume and pan keyframes as automation. Clips at a
//!   sample rate other than the project's are said to play converted.

use std::path::{Path, PathBuf};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Project to Final Cut Pro XML…", "Final Cut Pro XML…"];

#[derive(Clone, Debug, Default)]
pub struct State {
    pub dialog: Option<Dialog>,
}

#[derive(Clone, Debug, Default)]
pub struct Dialog {
    pub name: String,
    pub compound: bool,
    pub video: bool,
}

/// A placed clip: file, position and in point (seconds), length (seconds), volume and pan
/// keyframes (seconds from the clip's start; dB, −1…1).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Clip {
    pub track: String,
    pub file: PathBuf,
    pub start: f64,
    pub in_point: f64,
    pub length: f64,
    pub volume: Vec<(f64, f32)>,
    pub pan: Vec<(f64, f32)>,
    pub rate: u32,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn url(p: &Path) -> String {
    let s = p.display().to_string();
    let enc: String = s.chars().map(|c| match c {
        ' ' => "%20".to_string(),
        '#' => "%23".to_string(),
        '%' => "%25".to_string(),
        c => c.to_string(),
    }).collect();
    format!("file://localhost{enc}")
}

fn unurl(s: &str) -> PathBuf {
    let s = s.trim().trim_start_matches("file://localhost").trim_start_matches("file://");
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).to_string())
}

/// FCP's audio level: a gain factor (0 dB = 1).
fn db_to_level(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn level_to_db(l: f32) -> f32 {
    if l <= 0.0 { -96.0 } else { 20.0 * l.log10() }
}

/// The xmeml for clips over `seconds` at `fps` (a compound clip: the sequence wrapped in a clip;
/// `video`: the movie on a video track).
pub fn write(name: &str, clips: &[Clip], seconds: f64, fps: u32, rate: u32, compound: bool, video: Option<&Path>) -> String {
    let fr = |s: f64| (s * fps as f64).round() as i64;
    let rate_xml = format!("<rate><timebase>{fps}</timebase><ntsc>FALSE</ntsc></rate>");
    let mut tracks: Vec<String> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for c in clips {
        if !names.contains(&c.track.as_str()) {
            names.push(&c.track);
        }
    }
    for (ti, tn) in names.iter().enumerate() {
        let mut items = String::new();
        for (ci, c) in clips.iter().filter(|c| c.track == *tn).enumerate() {
            let id = format!("clip-{}-{}", ti + 1, ci + 1);
            let mut filters = String::new();
            let keys = |v: &[(f64, f32)], f: &dyn Fn(f32) -> f32| -> String {
                v.iter().map(|(t, x)| format!("<keyframe><when>{}</when><value>{:.4}</value></keyframe>", fr(*t), f(*x))).collect()
            };
            let vol0 = c.volume.first().map_or(1.0, |v| db_to_level(v.1));
            filters.push_str(&format!("<filter><effect><name>Audio Levels</name><effectid>audiolevels</effectid><effectcategory>audiolevels</effectcategory><effecttype>audiolevels</effecttype><mediatype>audio</mediatype><parameter><parameterid>level</parameterid><name>Level</name><valuemin>0</valuemin><valuemax>3.98109</valuemax><value>{vol0:.4}</value>{}</parameter></effect></filter>", keys(&c.volume, &db_to_level)));
            let pan0 = c.pan.first().map_or(0.0, |p| p.1);
            filters.push_str(&format!("<filter><effect><name>Audio Pan</name><effectid>audiopan</effectid><effectcategory>audiopan</effectcategory><effecttype>audiopan</effecttype><mediatype>audio</mediatype><parameter><parameterid>pan</parameterid><name>Pan</name><valuemin>-1</valuemin><valuemax>1</valuemax><value>{pan0:.4}</value>{}</parameter></effect></filter>", keys(&c.pan, &|x| x)));
            let file_name = c.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            items.push_str(&format!(
                "<clipitem id=\"{id}\"><name>{}</name><duration>{}</duration>{rate_xml}<start>{}</start><end>{}</end><in>{}</in><out>{}</out><file id=\"file-{id}\"><name>{}</name><pathurl>{}</pathurl>{rate_xml}<duration>{}</duration><media><audio><samplecharacteristics><depth>24</depth><samplerate>{}</samplerate></samplecharacteristics><channelcount>2</channelcount></audio></media></file><sourcetrack><mediatype>audio</mediatype><trackindex>1</trackindex></sourcetrack>{filters}</clipitem>",
                esc(&file_name), fr(c.length), fr(c.start), fr(c.start + c.length), fr(c.in_point), fr(c.in_point + c.length), esc(&file_name), esc(&url(&c.file)), fr(c.length), if c.rate == 0 { rate } else { c.rate }
            ));
        }
        tracks.push(format!("<track>{items}</track>"));
    }
    let video_xml = video.map(|v| format!("<video><track><clipitem id=\"video-1\"><name>{0}</name>{rate_xml}<start>0</start><end>{1}</end><in>0</in><out>{1}</out><file id=\"file-video-1\"><name>{0}</name><pathurl>{2}</pathurl>{rate_xml}</file></clipitem></track></video>", esc(&v.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()), fr(seconds), esc(&url(v)))).unwrap_or_default();
    let seq = format!("<sequence id=\"sequence-1\"><name>{}</name><duration>{}</duration>{rate_xml}<media>{video_xml}<audio><format><samplecharacteristics><depth>24</depth><samplerate>{rate}</samplerate></samplecharacteristics></format>{}</audio></media></sequence>", esc(name), fr(seconds), tracks.join(""));
    let body = if compound { format!("<clip id=\"compound-1\"><name>{}</name><duration>{}</duration>{rate_xml}<media><video><track><clipitem id=\"nest-1\"><name>{0}</name>{seq}</clipitem></track></video></media></clip>", esc(name), fr(seconds)) } else { seq };
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE xmeml>\n<xmeml version=\"5\">{body}</xmeml>\n")
}

// ---- reading ---------------------------------------------------------------------------------

/// The text inside the first <tag>…</tag> of `s` (after `from`).
fn inner<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let a = s.find(&open)?;
    let gt = a + s[a..].find('>')?;
    if s[a..=gt].ends_with("/>") {
        return Some("");
    }
    let close = format!("</{tag}>");
    let b = gt + 1 + s[gt + 1..].find(&close)?;
    Some(&s[gt + 1..b])
}

/// Every <tag>…</tag> block (not nested) in `s`.
fn blocks<'a>(s: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut rest = s;
    while let Some(a) = rest.find(&open) {
        let after = &rest[a + open.len()..];
        if !after.starts_with('>') && !after.starts_with(' ') {
            rest = &rest[a + open.len()..];
            continue;
        }
        let Some(b) = rest[a..].find(&close) else { break };
        out.push(&rest[a..a + b + close.len()]);
        rest = &rest[a + b + close.len()..];
    }
    out
}

fn num(s: Option<&str>) -> f64 {
    s.and_then(|x| x.trim().parse().ok()).unwrap_or(0.0)
}

fn unesc(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&")
}

/// The audio clips of an xmeml sequence (its first), per track ("Audio 1" …).
pub fn read(xml: &str) -> Vec<Clip> {
    let seq = inner(xml, "sequence").unwrap_or(xml);
    let fps = num(inner(seq, "timebase")).max(1.0);
    // (the sequence's own <audio> comes before any clip's file media)
    let audio = seq.find("<audio>").map_or("", |i| &seq[i..]);
    let mut out = Vec::new();
    // (files given once by id, then referred to by id alone)
    let mut files: Vec<(String, PathBuf, u32)> = Vec::new();
    for (ti, tr) in blocks(audio, "track").into_iter().enumerate() {
        for c in blocks(tr, "clipitem") {
            let file_block = inner(c, "file");
            let fid = c.find("<file id=\"").map(|i| c[i + 10..].split('"').next().unwrap_or("").to_string()).unwrap_or_default();
            let (path, rate) = match file_block.and_then(|f| inner(f, "pathurl")) {
                Some(u) => {
                    let p = unurl(&unesc(u));
                    let r = file_block.map(|f| num(inner(f, "samplerate")) as u32).unwrap_or(0);
                    files.push((fid.clone(), p.clone(), r));
                    (p, r)
                }
                None => match files.iter().find(|f| f.0 == fid) {
                    Some(f) => (f.1.clone(), f.2),
                    None => continue,
                },
            };
            let start = num(inner(c, "start"));
            let end = num(inner(c, "end"));
            let inp = num(inner(c, "in"));
            let out_p = num(inner(c, "out"));
            let len = if end > start { end - start } else { out_p - inp };
            let mut volume = Vec::new();
            let mut pan = Vec::new();
            for f in blocks(c, "filter") {
                let is_level = f.contains("audiolevels");
                let is_pan = f.contains("audiopan");
                let p = inner(f, "parameter").unwrap_or("");
                let keys: Vec<(f64, f32)> = blocks(p, "keyframe").into_iter().map(|k| (num(inner(k, "when")) / fps, num(inner(k, "value")) as f32)).collect();
                let base = num(inner(p, "value")) as f32;
                let keys = if keys.is_empty() { vec![(0.0, base)] } else { keys };
                if is_level {
                    volume = keys.into_iter().map(|(t, v)| (t, level_to_db(v))).collect();
                } else if is_pan {
                    pan = keys;
                }
            }
            out.push(Clip { track: format!("Audio {}", ti + 1), file: path, start: start / fps, in_point: inp / fps, length: len.max(0.0) / fps, volume, pan, rate });
        }
    }
    out
}

// ---- the commands ----------------------------------------------------------------------------

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Project to Final Cut Pro XML…" => {
            let n = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
            app.fcp.dialog = Some(Dialog { name: n, compound: false, video: app.hub.movie.path.is_some() });
            true
        }
        "Final Cut Pro XML…" => {
            let path = if app.automated { std::env::var_os("LG_FCP_XML").map(PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Import Final Cut Pro XML").add_filter("Final Cut Pro XML", &["xml", "fcpxml"]).pick_file() };
            if let Some(p) = path {
                import(app, &p);
            }
            true
        }
        _ => false,
    }
}

fn import(app: &mut App, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        app.tell("Couldn't read the XML file");
        return;
    };
    let clips = read(&text);
    if clips.is_empty() {
        app.tell("No audio clips in this XML");
        return;
    }
    app.checkpoint("Import Final Cut Pro XML");
    let rate = crate::audio::RATE as u32;
    let mismatched = clips.iter().filter(|c| c.rate != 0 && c.rate != rate).count();
    let mut names: Vec<String> = Vec::new();
    for c in &clips {
        if !names.contains(&c.track) {
            names.push(c.track.clone());
        }
    }
    for tn in names {
        let mut t = lg_core::Track::new(&tn, lg_core::TrackKind::Audio);
        let mut vol_pts: Vec<(u32, u8)> = Vec::new();
        let mut pan_pts: Vec<(u32, u8)> = Vec::new();
        for c in clips.iter().filter(|c| c.track == tn) {
            let start = crate::smarttempo::tick_at(app, c.start).max(0.0) as u32;
            let end = crate::smarttempo::tick_at(app, c.start + c.length).max(0.0) as u32;
            let seconds = crate::soundlib::audio_seconds(&c.file).unwrap_or(c.in_point + c.length);
            let mut props: lg_core::project::RegionProps = Default::default();
            props.audio_offset = (c.in_point * rate as f64) as u32;
            t.regions.push(lg_core::Region { name: c.file.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), start, length: end.saturating_sub(start).max(1), notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: c.file.clone(), seconds }), muted: false, source: None, origin: None, props, ext: Default::default() });
            for (s, db) in &c.volume {
                vol_pts.push((crate::smarttempo::tick_at(app, c.start + s) as u32, crate::automation::db_to_logic(*db).round().clamp(0.0, 127.0) as u8));
            }
            for (s, p) in &c.pan {
                pan_pts.push((crate::smarttempo::tick_at(app, c.start + s) as u32, (64.0 + p * 63.0).round().clamp(0.0, 127.0) as u8));
            }
        }
        if vol_pts.len() > 1 {
            t.automation.push(lg_core::project::AutoLane { param: 7, points: vol_pts, ..Default::default() });
        } else if let Some(v) = vol_pts.first() {
            t.volume_db = crate::automation::logic_to_db(v.1 as f32);
        }
        if pan_pts.len() > 1 {
            t.automation.push(lg_core::project::AutoLane { param: 10, points: pan_pts, ..Default::default() });
        } else if let Some(p) = pan_pts.first() {
            t.pan = (p.1 as f32 - 64.0) / 63.0;
        }
        if let Some(p) = app.project.as_mut() {
            p.tracks.push(t);
        }
        app.programs.push(0);
    }
    crate::trackarea::sync(app);
    app.dirty = true;
    app.load_engine();
    if mismatched > 0 {
        app.tell(format!("{mismatched} clip(s) aren't at the project's sample rate: they play converted in real time"));
    } else {
        app.tell("Imported the Final Cut Pro XML");
    }
}

/// The export dialog.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.fcp.dialog.clone() else { return };
    let mut close = false;
    let mut go = false;
    egui::Window::new("Export Project to Final Cut Pro XML").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Save As:");
            ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(240.0));
        });
        ui.checkbox(&mut d.compound, "Export as Final Cut Compound Clip");
        ui.add_enabled(app.hub.movie.path.is_some(), egui::Checkbox::new(&mut d.video, "Include video in export"));
        ui.label(egui::RichText::new("Audio and software instrument tracks are bounced to audio files; MIDI tracks are left out.").color(crate::look::text_dim()).font(crate::look::regular(11.0)));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.button("Export").clicked() {
                go = true;
            }
        });
    });
    app.fcp.dialog = if close || go { None } else { Some(d.clone()) };
    if go {
        let dir = if app.automated { std::env::var_os("LG_FCP_DIR").map(PathBuf::from) } else { kn_ui::files::FileDialog::new().set_title("Export Final Cut Pro XML to").pick_folder() };
        if let Some(dir) = dir {
            export(app, &dir, &d);
        }
    }
}

/// Bounces the tracks into "<name> Audio" beside the XML and writes it.
pub fn export(app: &mut App, dir: &Path, d: &Dialog) {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let audio_dir = dir.join(format!("{} Audio", d.name));
    let _ = std::fs::create_dir_all(&audio_dir);
    let o = crate::freeze::ExportOpts { range: crate::freeze::Range::default(), tail: true, ..Default::default() };
    let mut clips = Vec::new();
    for t in 0..n {
        let files = crate::freeze::export_tracks(app, &[t], &o, &audio_dir);
        let Some(f) = files.into_iter().next() else { continue };
        let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)).cloned() else { continue };
        let length = crate::soundlib::audio_seconds(&f).unwrap_or(0.0);
        let lane = |param: u8| tr.automation.iter().find(|a| a.param == param && a.plugin.is_none() && !a.muted).map(|a| a.points.clone()).unwrap_or_default();
        let volume: Vec<(f64, f32)> = {
            let pts = lane(7);
            if pts.is_empty() { vec![(0.0, tr.volume_db)] } else { pts.iter().map(|(tk, v)| (app.seconds_at(*tk as f64), crate::automation::logic_to_db(*v as f32))).collect() }
        };
        let pan: Vec<(f64, f32)> = {
            let pts = lane(10);
            if pts.is_empty() { vec![(0.0, tr.pan)] } else { pts.iter().map(|(tk, v)| (app.seconds_at(*tk as f64), (*v as f32 - 64.0) / 63.0)).collect() }
        };
        clips.push(Clip { track: tr.name.clone(), file: f, start: 0.0, in_point: 0.0, length, volume, pan, rate: crate::audio::RATE as u32 });
    }
    let secs = clips.iter().map(|c| c.length).fold(0.0, f64::max);
    let fps = crate::projsettings::smpte(&app.proj).1.round().max(1.0) as u32;
    let video = if d.video { app.hub.movie.path.clone() } else { None };
    let xml = write(&d.name, &clips, secs, fps, crate::audio::RATE as u32, d.compound, video.as_deref());
    let path = dir.join(format!("{}.xml", d.name));
    match std::fs::write(&path, xml) {
        Ok(()) => app.tell(format!("Exported {} with {} tracks", path.file_name().unwrap_or_default().to_string_lossy(), clips.len())),
        Err(e) => app.tell(format!("Couldn't write the XML: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back() {
        let clips = vec![
            Clip { track: "Bass".into(), file: PathBuf::from("/m/My Song Audio/Bass.wav"), start: 0.0, in_point: 0.0, length: 10.0, volume: vec![(0.0, 0.0), (5.0, -6.0)], pan: vec![(0.0, -0.5)], rate: 48_000 },
            Clip { track: "Keys".into(), file: PathBuf::from("/m/Keys #2.wav"), start: 2.0, in_point: 1.0, length: 4.0, volume: vec![(0.0, -3.0)], pan: vec![(0.0, 0.25)], rate: 44_100 },
        ];
        let x = write("My Song", &clips, 10.0, 25, 48_000, false, None);
        assert!(x.contains("<xmeml version=\"5\">") && x.contains("Audio Levels") && x.contains("Audio Pan"));
        assert!(x.contains("file://localhost/m/My%20Song%20Audio/Bass.wav"));
        let back = read(&x);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].track, "Audio 1");
        assert_eq!(back[1].track, "Audio 2");
        assert_eq!(back[1].file, PathBuf::from("/m/Keys #2.wav"));
        assert!((back[1].start - 2.0).abs() < 1e-6 && (back[1].in_point - 1.0).abs() < 1e-6 && (back[1].length - 4.0).abs() < 1e-6);
        assert_eq!(back[0].volume.len(), 2);
        assert!((back[0].volume[1].1 + 6.0).abs() < 0.05);
        assert!((back[0].pan[0].1 + 0.5).abs() < 1e-3);
        assert_eq!(back[1].rate, 44_100);
        // compound clip and video
        let c = write("My Song", &clips, 10.0, 25, 48_000, true, Some(Path::new("/m/film.mov")));
        assert!(c.contains("<clip id=\"compound-1\">") && c.contains("film.mov"));
        assert_eq!(read(&c).len(), 2);
    }
}
