//! Media (Keydeck's, on a sheet): View > Show Photo and Video Browser / Show Music Browser
//! (files from ~/Pictures, ~/Videos, ~/Music), Insert > Choose… for pictures, movies and
//! sounds, Record Audio, Web Video… and Image Gallery; movies and web videos play in place.

use std::path::PathBuf;
use std::time::Instant;

use std::sync::mpsc::{channel, Receiver};

use egui::RichText;
use kn_ui::media::web::{fetch, video_link, Fetched};
use kn_ui::media::{AUDIO_EXT, IMAGE_EXT, MOVIE_EXT};
use kn_ui::theme;
use nm_core::kn_format::ObjId;

use crate::app::App;

#[derive(Default)]
pub struct MediaBrowser {
    /// Open: true = music, false = photos and videos.
    pub open: Option<bool>,
    files: Vec<PathBuf>,
    listed: Option<bool>,
    query: String,
    pub recorder: Option<Recorder>,
    pub record_open: bool,
    /// Insert > Web Video: the address typed so far.
    pub url_sheet: Option<String>,
    fetching: Option<Receiver<Fetched>>,
    picking: Option<Receiver<Vec<(String, Vec<u8>)>>>,
    /// The gallery the pictures being picked are added to.
    adding_to: Option<ObjId>,
    /// A movie / sound read in the background (Insert > Choose…).
    preparing: Option<Receiver<Result<kn_ui::media::Picked, String>>>,
    /// The movie or web video playing in place on the sheet.
    pub player: Option<kn_ui::media::Player>,
    /// A web video whose stream is being looked up (it plays when known).
    pub web_waiting: Option<ObjId>,
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
                scan(&[home().join("Music")], AUDIO_EXT)
            } else {
                let mut ex: Vec<&str> = IMAGE_EXT.to_vec();
                ex.extend(MOVIE_EXT);
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
            insert_path(app, p);
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
            let path = std::env::temp_dir().join(format!("values-recording-{}.wav", std::process::id()));
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
                    insert_path(app, named);
                }
            }
            if stop {
                open = false;
            }
        }
        app.media.record_open = open;
    }
}

/// Insert > Choose…: a picture, movie or sound file.
pub fn choose(app: &mut App) {
    let mut exts: Vec<&str> = IMAGE_EXT.to_vec();
    exts.extend(MOVIE_EXT);
    exts.extend(AUDIO_EXT);
    if let Some(p) = kn_ui::files::FileDialog::new().add_filter("Pictures, Movies and Sounds", &exts).pick_file() {
        insert_path(app, p);
    }
}

/// A picture goes in at once; a movie or sound is read (length, poster still) in the
/// background, then inserted.
pub fn insert_path(app: &mut App, p: PathBuf) {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if IMAGE_EXT.contains(&ext.as_str()) {
        app.insert_image_file(&p);
        return;
    }
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _ = tx.send(kn_ui::media::prepare(&p));
    });
    app.media.preparing = Some(rx);
    app.notify("Adding the media…");
}

/// A picture's bytes on the sheet where there's room; its id.
pub fn insert_picture(app: &mut App, bytes: Vec<u8>, name: &str) -> Option<ObjId> {
    let sheet = app.sheets().get(app.sheet).map(|s| s.id)?;
    let (x, y) = app.free_spot_pub();
    let name = name.to_string();
    match app.edit("Insert Image", |d, h| nm_core::sheets::insert_image(d, h, sheet, bytes, &name, x, y)) {
        Some(Ok(id)) => {
            app.sel = None;
            app.canvas.obj = Some(id);
            Some(id)
        }
        Some(Err(e)) => {
            app.notify(format!("Couldn't insert the picture: {e}"));
            None
        }
        None => None,
    }
}

/// Insert > Image Gallery: several pictures in one frame.
pub fn pick_gallery(app: &mut App, adding_to: Option<ObjId>) {
    app.media.adding_to = adding_to;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let files = kn_ui::files::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg"]).set_title("Choose Pictures for the Gallery").pick_files().unwrap_or_default();
        let pics: Vec<(String, Vec<u8>)> = files.iter().filter_map(|p| Some((p.file_name()?.to_string_lossy().to_string(), std::fs::read(p).ok()?))).collect();
        let _ = tx.send(pics);
    });
    app.media.picking = Some(rx);
}

/// The Web Video sheet and the background work coming back.
pub fn web_and_jobs(app: &mut App, ctx: &egui::Context) {
    if let Some(mut url) = app.media.url_sheet.take() {
        let mut keep = true;
        let mut go = false;
        let valid = video_link(url.trim());
        egui::Window::new("Add Web Video").title_bar(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                let bad = !url.trim().is_empty() && !valid;
                if bad {
                    ui.label(RichText::new("You haven't entered a valid YouTube or Vimeo video URL.").strong());
                } else {
                    ui.label(RichText::new("Add videos from YouTube and Vimeo.").strong());
                }
                ui.label(RichText::new("Third party content and services may be subject to additional terms and conditions.").small().color(theme::TEXT_DIM()));
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(&mut url).hint_text("Enter URL").desired_width(f32::INFINITY));
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(valid, egui::Button::new("Insert").fill(theme::ACCENT())).clicked() || (enter && valid) {
                            go = true;
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            keep = false;
                        }
                    });
                });
            });
        });
        if go {
            let (tx, rx) = channel();
            let u = url.trim().to_string();
            std::thread::spawn(move || {
                let _ = tx.send(fetch(u));
            });
            app.media.fetching = Some(rx);
            app.notify("Getting the video's details…");
        } else if keep {
            app.media.url_sheet = Some(url);
        }
    }
    // a web video's details: its poster as a picture that is the video
    if let Some(rx) = app.media.fetching.take() {
        match rx.try_recv() {
            Ok(Ok((url, title, author, poster))) => {
                let aspect = nm_core::kn_format::objects::image_size(&poster).map(|(w, h)| h as f32 / w.max(1) as f32).filter(|a| (0.2..5.0).contains(a)).unwrap_or(0.75);
                if let Some(id) = insert_picture(app, poster, "hqdefault.jpeg") {
                    app.merge_next_edit = true;
                    app.edit("Insert Image", |d, h| {
                        if let Some(mut g) = d.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) {
                            g.w = 480.0;
                            g.h = (480.0 * aspect).round();
                            d.set_frame(h, id, g);
                        }
                        d.make_web_video(h, id, &url, &title, &author);
                    });
                    app.merge_next_edit = false;
                }
            }
            Ok(Err(e)) => app.notify(e),
            Err(std::sync::mpsc::TryRecvError::Empty) => app.media.fetching = Some(rx),
            Err(_) => {}
        }
    }
    // gallery pictures picked
    if let Some(rx) = app.media.picking.take() {
        match rx.try_recv() {
            Ok(mut pics) if !pics.is_empty() => {
                let target = match app.media.adding_to.take() {
                    Some(id) => Some(id),
                    None => {
                        let (name, first) = pics.remove(0);
                        insert_picture(app, first, &name)
                    }
                };
                if let Some(id) = target {
                    if !pics.is_empty() {
                        app.merge_next_edit = true;
                        let r = app.edit("Insert Image", |d, h| d.make_gallery(h, id, pics.into_iter().map(|(n, b)| (b, n)).collect()));
                        app.merge_next_edit = false;
                        if let Some(Err(e)) = r {
                            app.notify(format!("Couldn't make the gallery: {e}"));
                        }
                    }
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => app.media.picking = Some(rx),
            Err(_) => {}
        }
    }
    // a movie / sound read: on the sheet
    if let Some(rx) = app.media.preparing.take() {
        match rx.try_recv() {
            Ok(Ok(kn_ui::media::Picked::Image(name, bytes))) => {
                insert_picture(app, bytes, &name);
            }
            Ok(Ok(kn_ui::media::Picked::Movie(m))) => {
                if let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) {
                    let (x, y) = app.free_spot_pub();
                    let audio = m.audio_only;
                    let natural = m.natural;
                    match app.edit(if audio { "Insert Audio" } else { "Insert Movie" }, |d, h| -> Result<ObjId, nm_core::kn_format::Error> {
                        let id = d.insert_movie(h, sheet, m)?;
                        if let Some(mut g) = d.file.obj(id).and_then(|o| o.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) {
                            // a movie at most 480 wide where there's room; a sound as Numbers' speaker
                            let (w, hh) = natural.map(|(w, hh)| { let k = (480.0 / w).min(360.0 / hh).min(1.0); (w * k, hh * k) }).unwrap_or((g.w, g.h));
                            g.x = x;
                            g.y = y;
                            g.w = w.round();
                            g.h = hh.round();
                            d.set_frame(h, id, g);
                        }
                        Ok(id)
                    }) {
                        Some(Ok(id)) => {
                            app.sel = None;
                            app.canvas.obj = Some(id);
                        }
                        Some(Err(e)) => app.notify(format!("Couldn't insert it: {e}")),
                        None => {}
                    }
                }
            }
            Ok(Err(e)) => app.notify(e),
            Err(std::sync::mpsc::TryRecvError::Empty) => app.media.preparing = Some(rx),
            Err(_) => {}
        }
    }
    if app.media.fetching.is_some() || app.media.picking.is_some() || app.media.preparing.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// Movies, sounds and web videos play in place on the sheet (a ▶ on them; a click stops),
/// and a selected gallery shows its arrows and dots. `items`: each such object's screen rect.
pub fn controls(app: &mut App, ui: &mut egui::Ui, items: &[(ObjId, egui::Rect)]) {
    use egui::{vec2, Color32, Rect, Stroke};
    use nm_core::kn_model::DrawableKind;
    let ctx = ui.ctx().clone();
    let play_mark = |ui: &mut egui::Ui, r: Rect, key: (&str, ObjId)| -> bool {
        let rad = (r.width().min(r.height()) * 0.12).clamp(14.0, 40.0);
        let c = r.center();
        let p = ui.painter();
        p.circle_filled(c, rad, Color32::from_black_alpha(140));
        p.circle_stroke(c, rad, Stroke::new(2.0, Color32::WHITE));
        let s = rad * 0.45;
        p.add(egui::Shape::convex_polygon(vec![c + vec2(-s * 0.55, -s), c + vec2(s, 0.0), c + vec2(-s * 0.55, s)], Color32::WHITE, Stroke::NONE));
        ui.interact(Rect::from_center_size(c, vec2(rad, rad) * 2.0), ui.scope_id().with(key), egui::Sense::click()).clicked()
    };
    for &(id, r) in items {
        let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { continue };
        let playing = app.media.player.as_ref().is_some_and(|p| p.drawable == id && !p.finished());
        if playing {
            if let Some(t) = app.media.player.as_mut().and_then(|p| p.texture(&ctx)).map(|t| t.id()) {
                ui.painter().image(t, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            if ui.interact(r, ui.scope_id().with(("media-playing", id)), egui::Sense::click()).clicked() {
                app.media.player = None;
            }
            ctx.request_repaint();
            continue;
        }
        match &d.kind {
            DrawableKind::Movie(m) => {
                if play_mark(ui, r, ("movie-play", id)) {
                    let file = m.movie_data.and_then(|data| app.doc().and_then(|doc| kn_ui::media::cache_file(doc, data)).map(|f| (data, f)));
                    match file {
                        Some((data, (path, len))) => {
                            let px = [(r.width() * ctx.pixels_per_point()) as u32, (r.height() * ctx.pixels_per_point()) as u32];
                            let spec = kn_ui::media::Spec::new(path, len, m, Some(px));
                            app.media.player = Some(kn_ui::media::Player::start(&ctx, id, data, spec));
                        }
                        None => app.notify("The movie's file isn't in this document"),
                    }
                }
            }
            DrawableKind::Image(im) => {
                if let Some(w) = &im.web_video {
                    if play_mark(ui, r, ("web-play", id)) || app.media.web_waiting == Some(id) {
                        match kn_ui::media::web_stream(&w.url) {
                            Some(stream) => {
                                app.media.web_waiting = None;
                                let px = [(r.width() * ctx.pixels_per_point()) as u32 & !1, (r.height() * ctx.pixels_per_point()) as u32 & !1];
                                let spec = kn_ui::media::Spec { path: PathBuf::from(stream), len: 0, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: Some(px) };
                                app.media.player = Some(kn_ui::media::Player::start(&ctx, id, 1, spec));
                            }
                            None => {
                                if app.media.web_waiting != Some(id) {
                                    app.notify("Loading the video…");
                                }
                                if let Some(e) = kn_ui::media::take_web_error() {
                                    app.notify(e);
                                    app.media.web_waiting = None;
                                } else {
                                    app.media.web_waiting = Some(id);
                                    ctx.request_repaint_after(std::time::Duration::from_millis(250));
                                }
                            }
                        }
                    }
                }
                if im.gallery.len() > 1 && app.canvas.obj == Some(id) {
                    let n = im.gallery.len();
                    let pos = app.gallery_pos.get(&id).copied().unwrap_or(im.gallery_shown).min(n - 1);
                    let mut go = None;
                    let rad = 16.0;
                    for (dir, show) in [(-1i32, pos > 0), (1, pos + 1 < n)] {
                        if !show {
                            continue;
                        }
                        let c = egui::pos2(if dir < 0 { r.min.x + 14.0 + rad } else { r.max.x - 14.0 - rad }, r.center().y);
                        let resp = ui.interact(Rect::from_center_size(c, vec2(rad * 2.0, rad * 2.0)), ui.scope_id().with(("gallery-arrow", id, dir)), egui::Sense::click());
                        ui.painter().circle_filled(c, rad, Color32::from_black_alpha(if resp.hovered() { 150 } else { 110 }));
                        let k = 5.0 * dir as f32;
                        ui.painter().add(egui::Shape::line(vec![c + vec2(-k * 0.6, -7.0), c + vec2(k * 0.6, 0.0), c + vec2(-k * 0.6, 7.0)], Stroke::new(2.4, Color32::WHITE)));
                        if resp.clicked() {
                            go = Some(if dir < 0 { pos - 1 } else { pos + 1 });
                        }
                    }
                    for k in 0..n {
                        let c = egui::pos2(r.center().x + (k as f32 - (n - 1) as f32 / 2.0) * 16.0, r.max.y + 12.0);
                        let resp = ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), ui.scope_id().with(("gallery-dot", id, k)), egui::Sense::click());
                        ui.painter().circle_filled(c, 4.0, if k == pos { theme::gray(0x70) } else { theme::gray(0xc8) });
                        if resp.clicked() {
                            go = Some(k);
                        }
                    }
                    if let Some(p) = go {
                        app.gallery_pos.insert(id, p);
                        app.version += 1;
                    }
                }
            }
            _ => {}
        }
    }
}
