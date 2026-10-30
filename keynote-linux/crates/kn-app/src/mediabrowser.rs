//! View > Show Photo and Video Browser / Show Music Browser (files from ~/Pictures, ~/Videos,
//! ~/Music), and Insert > Record Audio.

use std::path::PathBuf;
use std::time::Instant;

use egui::RichText;

use crate::app::App;
use crate::theme;

#[derive(Default)]
pub struct MediaBrowser {
    /// Open: true = music, false = photos and videos.
    pub open: Option<bool>,
    files: Vec<PathBuf>,
    listed: Option<bool>,
    query: String,
    pub recorder: Option<Recorder>,
    pub record_open: bool,
}

pub struct Recorder {
    child: std::process::Child,
    path: PathBuf,
    started: Instant,
}

fn scan(dirs: &[PathBuf], exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                continue;
            }
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().is_some_and(|x| exts.contains(&x.to_string_lossy().to_lowercase().as_str())) {
                out.push(p);
            }
            if out.len() > 2000 {
                return out;
            }
        }
    }
    out.sort();
    out
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if let Some(music) = app.media.open {
        if app.media.listed != Some(music) {
            app.media.files = if music {
                scan(&[home().join("Music")], crate::media::AUDIO_EXT)
            } else {
                let mut ex: Vec<&str> = crate::media::IMAGE_EXT.to_vec();
                ex.extend(crate::media::MOVIE_EXT);
                scan(&[home().join("Pictures"), home().join("Videos")], &ex)
            };
            app.media.listed = Some(music);
        }
        let mut open = true;
        let mut pick = None;
        egui::Window::new(if music { "Music" } else { "Photos and Videos" }).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).default_size([360.0, 460.0]).open(&mut open).show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut app.media.query).hint_text("Search").desired_width(f32::INFINITY));
            let q = app.media.query.to_lowercase();
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut n = 0;
                for p in &app.media.files {
                    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    if !q.is_empty() && !name.to_lowercase().contains(&q) {
                        continue;
                    }
                    n += 1;
                    if ui.selectable_label(false, &name).on_hover_text(p.display().to_string()).clicked() {
                        pick = Some(p.clone());
                    }
                }
                if n == 0 {
                    ui.label(RichText::new(if music { "No songs in ~/Music" } else { "No pictures or movies in ~/Pictures or ~/Videos" }).color(theme::TEXT_DIM()));
                }
            });
        });
        if !open {
            app.media.open = None;
        }
        if let Some(p) = pick {
            app.insert_media_path(p);
        }
    }
    if app.media.record_open {
        let mut open = true;
        let mut start = false;
        let mut stop = false;
        egui::Window::new("Record Audio").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            match &app.media.recorder {
                Some(r) => {
                    let t = r.started.elapsed().as_secs();
                    ui.label(RichText::new(format!("\u{25cf} Recording  {}:{:02}", t / 60, t % 60)).color(egui::Color32::from_rgb(0xff, 0x45, 0x3a)).size(16.0));
                    stop = ui.button("Stop and Insert").clicked();
                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                }
                None => {
                    ui.label("Records from the default microphone.");
                    start = ui.button("\u{25cf}  Record").clicked();
                }
            }
        });
        if start {
            let path = std::env::temp_dir().join(format!("keynote-recording-{}.wav", std::process::id()));
            let child = std::process::Command::new("pw-record")
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .or_else(|_| std::process::Command::new("arecord").args(["-f", "cd"]).arg(&path).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn());
            match child {
                Ok(child) => app.media.recorder = Some(Recorder { child, path, started: Instant::now() }),
                Err(e) => app.notify(format!("Couldn't start recording: {e}")),
            }
        }
        if stop || (!open && app.media.recorder.is_some()) {
            if let Some(mut r) = app.media.recorder.take() {
                // stop politely so the file is finished properly
                let _ = std::process::Command::new("kill").args(["-INT", &r.child.id().to_string()]).status();
                let _ = r.child.wait();
                if stop {
                    // a compressed copy (Keynote records AAC) when ffmpeg is there
                    let m4a = r.path.with_extension("m4a");
                    let ok = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&r.path).args(["-c:a", "aac", "-b:a", "128k"]).arg(&m4a).status().is_ok_and(|s| s.success());
                    let named = std::env::temp_dir().join(if ok { "Recording.m4a" } else { "Recording.wav" });
                    let _ = std::fs::rename(if ok { &m4a } else { &r.path }, &named);
                    app.insert_media_path(named);
                }
            }
            if stop {
                open = false;
            }
        }
        app.media.record_open = open;
    }
}
