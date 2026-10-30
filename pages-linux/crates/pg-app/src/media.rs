//! Media, as Pages' toolbar Media ▾: Choose Photo or Video…, Choose Music…, Add Image Gallery…,
//! Add Web Video…, Record Audio, Choose File…; movies and sounds play in place in the document
//! (a ▶ on the poster), web videos stream (yt-dlp). The playing and probing are the suite's
//! shared kn_ui::media (Keydeck's).

use egui::{pos2, vec2, Color32, Rect, RichText, Stroke};
use kn_ui::media::{self, Picked, Player};
use pg_core::kn_format::ObjId;
use pg_core::kn_model::DrawableKind;
use pg_core::objects::Placement;
use std::path::PathBuf;

use crate::app::App;

#[derive(Default)]
struct State {
    record_open: bool,
    recorder: Option<(std::process::Child, PathBuf, std::time::Instant)>,
    /// Add Web Video: the link typed, and a lookup in flight.
    web_sheet: Option<String>,
    web_fetch: Option<(std::sync::mpsc::Receiver<media::web::Fetched>, String)>,
    /// The web video whose link is being replaced (the Web Video pane's Replace).
    replacing: Option<ObjId>,
    /// The movie / web video playing in place (and one waiting for its stream).
    player: Option<Player>,
    waiting: Option<ObjId>,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Where new media goes: in the text at the caret (word processing), else on the page.
fn place(app: &App, w: f32, h: f32) -> Placement {
    match crate::insert::body_caret(app) {
        Some(at) => Placement::Inline { at },
        None => crate::insert::floating_spot(app, w, h),
    }
}

/// Insert a picture, movie or sound from a file.
pub fn insert_path(app: &mut App, path: &std::path::Path) {
    match media::prepare(path) {
        Ok(Picked::Image(name, bytes)) => crate::insert::insert_image_bytes(app, bytes, &name),
        Ok(Picked::Movie(m)) => {
            let (w, h) = m.natural.unwrap_or((128.0, 128.0));
            let where_ = place(app, w, h);
            let max_w = app.layout.as_ref().map(|l| l.setup.text_width()).unwrap_or(468.0);
            match app.edit_pages(if m.audio_only { "Insert Audio" } else { "Insert Movie" }, |p, hist| p.insert_movie(hist, m, where_, max_w)) {
                Some(Ok(id)) => {
                    app.refresh();
                    crate::canvas::select_object(app, id);
                }
                Some(Err(e)) => app.notify(format!("Couldn't insert it: {e}")),
                None => {}
            }
        }
        Err(e) => app.notify(e),
    }
}

fn pick(filters: &[(&str, &[&str])], many: bool) -> Vec<PathBuf> {
    crate::app::file_dialog(|mut d| {
        for (n, exts) in filters {
            d = d.add_filter(*n, exts);
        }
        if many { d.pick_files() } else { d.pick_file().map(|p| vec![p]) }
    })
    .unwrap_or_default()
}

/// The Media menu's commands (toolbar Media ▾ and Insert): false when `name` isn't one.
pub fn command(app: &mut App, name: &str) -> bool {
    if app.word.as_ref().is_some_and(|w| !w.hybrid) && crate::app::word_blocks(name) {
        app.notify("Word can't keep this. Choose Word + Letters as the format (click the document's name) to use it.");
        return true;
    }
    match name {
        "Content Hub" => app.hub.show_hub(),
        "Choose Photo or Video…" => {
            let all: Vec<&str> = media::IMAGE_EXT.iter().chain(media::MOVIE_EXT).copied().collect();
            for p in pick(&[("Photos and videos", &all)], true) {
                insert_path(app, &p);
            }
        }
        "Choose Music…" => {
            for p in pick(&[("Music and sounds", media::AUDIO_EXT)], true) {
                insert_path(app, &p);
            }
        }
        "Choose File…" => {
            let all: Vec<&str> = media::IMAGE_EXT.iter().chain(media::MOVIE_EXT).chain(media::AUDIO_EXT).copied().collect();
            for p in pick(&[("Pictures, movies and sounds", &all)], true) {
                insert_path(app, &p);
            }
        }
        "Add Image Gallery…" => add_gallery(app),
        "Add Web Video…" => with(|s| s.web_sheet = Some(String::new())),
        "Record Audio" => with(|s| s.record_open = true),
        _ => return false,
    }
    true
}

/// Add Image Gallery: the first picture inserted, the others added to it.
fn add_gallery(app: &mut App) {
    let files = pick(&[("Pictures", media::IMAGE_EXT)], true);
    let mut imgs: Vec<(Vec<u8>, String)> = files.iter().filter_map(|p| Some((std::fs::read(p).ok()?, p.file_name()?.to_string_lossy().to_string()))).collect();
    if imgs.is_empty() {
        return;
    }
    let (first, name) = imgs.remove(0);
    crate::insert::insert_image_bytes(app, first, &name);
    let [id] = app.selected_objs()[..] else { return };
    if !imgs.is_empty() {
        if let Some(Err(e)) = app.edit("Image Gallery", |d, h| d.make_gallery(h, id, imgs)) {
            app.notify(format!("Couldn't make the gallery: {e}"));
        }
    }
}

/// The Web Video pane's Replace: the link sheet, filled in, for that video.
pub fn replace_web_video(id: ObjId, url: &str) {
    with(|s| {
        s.replacing = Some(id);
        s.web_sheet = Some(url.to_string());
    });
}

/// The Record Audio and Add Web Video windows (each frame).
pub fn ui(app: &mut App, ctx: &egui::Context) {
    record_window(app, ctx);
    web_window(app, ctx);
}

fn record_window(app: &mut App, ctx: &egui::Context) {
    if !with(|s| s.record_open) {
        return;
    }
    let mut open = true;
    let (mut start, mut stop) = (false, false);
    let recording = with(|s| s.recorder.as_ref().map(|r| r.2.elapsed().as_secs()));
    egui::Window::new("Record Audio").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| match recording {
        Some(t) => {
            ui.label(RichText::new(format!("● Recording  {}:{:02}", t / 60, t % 60)).color(Color32::from_rgb(0xff, 0x45, 0x3a)).size(16.0));
            stop = ui.button("Stop and Insert").clicked();
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        None => {
            ui.label("Records from the default microphone.");
            start = ui.button("●  Record").clicked();
        }
    });
    if start {
        let path = std::env::temp_dir().join(format!("letters-recording-{}.wav", std::process::id()));
        let child = std::process::Command::new("pw-record")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .or_else(|_| std::process::Command::new("arecord").args(["-f", "cd"]).arg(&path).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn());
        match child {
            Ok(c) => with(|s| s.recorder = Some((c, path, std::time::Instant::now()))),
            Err(e) => app.notify(format!("Couldn't start recording: {e}")),
        }
    }
    if stop || (!open && recording.is_some()) {
        if let Some((mut child, path, _)) = with(|s| s.recorder.take()) {
            // stop politely so the file is finished properly
            let _ = std::process::Command::new("kill").args(["-INT", &child.id().to_string()]).status();
            let _ = child.wait();
            if stop {
                // AAC as Pages records, when ffmpeg is there
                let m4a = path.with_extension("m4a");
                let ok = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&path).args(["-c:a", "aac", "-b:a", "128k"]).arg(&m4a).status().is_ok_and(|s| s.success());
                let named = std::env::temp_dir().join(if ok { "Recording.m4a" } else { "Recording.wav" });
                let _ = std::fs::rename(if ok { &m4a } else { &path }, &named);
                insert_path(app, &named);
            }
        }
        if stop {
            open = false;
        }
    }
    with(|s| s.record_open = open);
}

fn web_window(app: &mut App, ctx: &egui::Context) {
    // a lookup that finished: the poster inserted as a picture that plays the video
    let done = with(|s| s.web_fetch.as_ref().and_then(|(rx, url)| rx.try_recv().ok().map(|r| (r, url.clone()))));
    if let Some((res, url)) = done {
        with(|s| s.web_fetch = None);
        match res {
            Ok((title, author, _thumb, poster)) if with(|s| s.replacing.is_some()) => {
                let id = with(|s| s.replacing.take()).unwrap();
                let size = pg_core::kn_format::objects::image_size(&poster);
                let r = app.edit("Replace Web Video", |d, h| -> Result<(), pg_core::kn_format::Error> {
                    if let Some((w, hh)) = size {
                        d.structural(h, &[id], |k| -> Result<(), pg_core::kn_format::Error> {
                            let data = k.add_image_data(poster, "web-video.jpeg")?;
                            k.set_image_data(id, data, w as f32, hh as f32);
                            Ok(())
                        })?;
                    }
                    d.make_web_video(h, id, &url, &title, &author);
                    Ok(())
                });
                if let Some(Err(e)) = r {
                    app.notify(format!("Couldn't replace the video: {e}"));
                }
            }
            Ok((title, author, _thumb, poster)) => {
                crate::insert::insert_image_bytes(app, poster, "web-video.jpeg");
                if let [id] = app.selected_objs()[..] {
                    app.edit("Add Web Video", |d, h| d.make_web_video(h, id, &url, &title, &author));
                }
            }
            Err(e) => {
                with(|s| s.replacing = None);
                app.notify(format!("Couldn't add the video: {e}"))
            }
        }
    }
    let Some(mut url) = with(|s| s.web_sheet.clone()) else { return };
    let busy = with(|s| s.web_fetch.is_some());
    let mut open = true;
    let mut go = false;
    egui::Window::new("Add Web Video").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        ui.label("Paste a YouTube or Vimeo link.");
        let r = ui.add(egui::TextEdit::singleline(&mut url).hint_text("https://www.youtube.com/watch?v=…").desired_width(360.0));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            let ok = media::web::video_link(url.trim());
            if busy {
                ui.spinner();
                ui.label("Looking it up…");
            } else if ui.add_enabled(ok, egui::Button::new("Insert")).clicked() || (ok && enter) {
                go = true;
            }
        });
    });
    if go {
        let (tx, rx) = std::sync::mpsc::channel();
        let u = url.trim().to_string();
        let ctx2 = ctx.clone();
        std::thread::spawn({
            let u = u.clone();
            move || {
                let _ = tx.send(media::web::fetch(u));
                ctx2.request_repaint();
            }
        });
        with(|s| s.web_fetch = Some((rx, u)));
        open = false;
    }
    with(|s| {
        s.web_sheet = if open { Some(url) } else { None };
        // closed without looking anything up: not replacing after all
        if !open && s.web_fetch.is_none() {
            s.replacing = None;
        }
    });
}

/// Movies, sounds and web videos on the visible pages: a ▶ on each (selected or hovered) to play
/// it in place; while playing, the video over its frame (a click stops it). True while the
/// pointer is on one of the buttons.
pub fn canvas_controls(app: &mut App, ui: &mut egui::Ui) -> bool {
    let Some(l) = app.layout.as_ref() else { return false };
    let view = app.canvas.view_rect();
    let mut items: Vec<(ObjId, Rect, Option<String>, Option<pg_core::kn_model::MovieData>)> = Vec::new();
    for d in &app.drawables {
        let Some(&pi) = app.obj_page.get(&d.id) else { continue };
        let g = d.geom;
        let r = Rect::from_min_max(app.canvas.page_to_screen(l, pi, pos2(g.x, g.y)), app.canvas.page_to_screen(l, pi, pos2(g.x + g.w, g.y + g.h)));
        if !r.intersects(view) {
            continue;
        }
        match &d.kind {
            DrawableKind::Movie(m) => items.push((d.id, r, None, Some(m.clone()))),
            DrawableKind::Image(im) => {
                if let Some(w) = &im.web_video {
                    items.push((d.id, r, Some(w.url.clone()), None));
                }
            }
            _ => {}
        }
    }
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut hot = false;
    for (id, r, web, movie) in items {
        let start = |app: &mut App, ctx: &egui::Context| -> bool {
            let px = [((r.width() * ctx.pixels_per_point()) as u32 + 1) & !1, ((r.height() * ctx.pixels_per_point()) as u32 + 1) & !1];
            if let Some(url) = &web {
                let Some(stream) = media::web_stream(url) else { return false };
                let spec = media::Spec { path: PathBuf::from(stream), len: 0, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: Some(px) };
                with(|s| s.player = Some(Player::start(ctx, id, 1, spec)));
                return true;
            }
            let Some(m) = &movie else { return false };
            let Some(data) = m.movie_data else { return false };
            let Some((path, len)) = app.doc().and_then(|d| media::cache_file(d, data)) else { return false };
            let spec = media::Spec::new(path, len, m, Some(px));
            with(|s| s.player = Some(Player::start(ctx, id, data, spec)));
            true
        };
        if with(|s| s.waiting) == Some(id) && start(app, ui.ctx()) {
            with(|s| s.waiting = None);
        }
        let playing = with(|s| s.player.as_ref().is_some_and(|p| p.drawable == id && !p.finished()));
        if playing {
            let tex = with(|s| s.player.as_mut().and_then(|p| p.texture(ui.ctx())).map(|t| t.id()));
            if let Some(t) = tex {
                ui.painter().image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            let hit = ui.interact(r, ui.scope_id().with(("media-playing", id)), egui::Sense::click());
            if hit.clicked() {
                with(|s| s.player = None);
            }
            hot |= hit.hovered();
            ui.ctx().request_repaint();
            continue;
        }
        // Pages: a ▶ in a dark circle on the poster, shown on the selected or pointed-at one
        let selected = app.selected_objs().contains(&id);
        if !(selected || pointer.is_some_and(|q| r.contains(q))) {
            continue;
        }
        let rad = (r.width().min(r.height()) * 0.12).clamp(14.0, 40.0);
        let c = r.center();
        let p = ui.painter();
        p.circle_filled(c, rad, Color32::from_black_alpha(140));
        p.circle_stroke(c, rad, Stroke::new(2.0, Color32::WHITE));
        let s = rad * 0.45;
        p.add(egui::Shape::convex_polygon(vec![c + vec2(-s * 0.55, -s), c + vec2(s, 0.0), c + vec2(-s * 0.55, s)], Color32::WHITE, Stroke::NONE));
        let hit = ui.interact(Rect::from_center_size(c, vec2(rad, rad) * 2.0), ui.scope_id().with(("media-play", id)), egui::Sense::click());
        hot |= hit.hovered();
        if hit.clicked() && !start(app, ui.ctx()) {
            if web.is_some() {
                with(|s| s.waiting = Some(id));
                app.notify("Loading the video…");
            } else {
                app.notify("This movie can't be played (no ffmpeg?)");
            }
        }
        if with(|s| s.waiting) == Some(id) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
    hot
}
