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
    /// The size a waiting video starts at (a front end's ▶).
    waiting_px: [u32; 2],
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
        // (the Shape popover's Browse Content Hub)
        "Content Hub Shapes" => app.hub.show_tab(kn_ui::contenthub::Tab::Shapes),
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
    add_gallery_paths(app, &files);
}

/// An image gallery of these pictures: the first inserted, the others added to it.
pub fn add_gallery_paths(app: &mut App, files: &[std::path::PathBuf]) {
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
        record_start(app);
    }
    if stop || (!open && recording.is_some()) {
        record_stop(app, stop);
        if stop {
            open = false;
        }
    }
    with(|s| s.record_open = open);
}

/// Record Audio is open, and how long it has been recording (seconds).
pub fn record_state() -> Option<Option<u64>> {
    with(|s| s.record_open.then(|| s.recorder.as_ref().map(|r| r.2.elapsed().as_secs())))
}

pub fn record_close() {
    with(|s| s.record_open = false);
}

/// ● Record: the default microphone into a temporary file.
pub fn record_start(app: &mut App) {
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

/// The recording stopped; with `insert`, it goes in as a sound (AAC when ffmpeg is there).
pub fn record_stop(app: &mut App, insert: bool) {
    if let Some((mut child, path, _)) = with(|s| s.recorder.take()) {
        // stop politely so the file is finished properly
        let _ = std::process::Command::new("kill").args(["-INT", &child.id().to_string()]).status();
        let _ = child.wait();
        if insert {
            // AAC as Pages records, when ffmpeg is there
            let m4a = path.with_extension("m4a");
            let ok = std::process::Command::new("ffmpeg").args(["-v", "error", "-y", "-i"]).arg(&path).args(["-c:a", "aac", "-b:a", "128k"]).arg(&m4a).status().is_ok_and(|s| s.success());
            let named = std::env::temp_dir().join(if ok { "Recording.m4a" } else { "Recording.wav" });
            let _ = std::fs::rename(if ok { &m4a } else { &path }, &named);
            insert_path(app, &named);
        }
    }
}

fn web_window(app: &mut App, ctx: &egui::Context) {
    web_poll(app);
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
        let ctx2 = ctx.clone();
        web_go(&url, move || ctx2.request_repaint());
        open = false;
    }
    with(|s| s.web_sheet = if open { Some(url) } else { None });
    if !open {
        web_close();
    }
}

/// Add Web Video is open: (the link typed, a lookup running).
pub fn web_state() -> (Option<String>, bool) {
    with(|s| (s.web_sheet.clone(), s.web_fetch.is_some()))
}

/// The link typed so far (for Insert's state).
pub fn web_edit(url: &str) {
    with(|s| s.web_sheet = Some(url.to_string()));
}

/// Add Web Video closed: without a lookup, not replacing after all.
pub fn web_close() {
    with(|s| {
        s.web_sheet = None;
        if s.web_fetch.is_none() {
            s.replacing = None;
        }
    });
}

/// Insert: look the link up in the background (`done` when it's back); the window closes.
pub fn web_go(url: &str, done: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    let u = url.trim().to_string();
    std::thread::spawn({
        let u = u.clone();
        move || {
            let _ = tx.send(media::web::fetch(u));
            done();
        }
    });
    with(|s| {
        s.web_fetch = Some((rx, u));
        s.web_sheet = None;
    });
}

/// A lookup that finished: the poster inserted as a picture that plays the video.
pub fn web_poll(app: &mut App) {
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
}

/// Start playing drawable `id` (a web video's page link, or a movie) at `px` pixels.
fn start_player(app: &App, ctx: &egui::Context, id: ObjId, web: Option<&str>, movie: Option<&pg_core::kn_model::MovieData>, px: [u32; 2]) -> bool {
    if let Some(url) = web {
        let Some(stream) = media::web_stream(url) else { return false };
        let spec = media::Spec { path: PathBuf::from(stream), len: 0, start: 0.0, end: 0.0, volume: 1.0, looping: false, video: Some(px) };
        with(|s| s.player = Some(Player::start(ctx, id, 1, spec)));
        return true;
    }
    let Some(m) = movie else { return false };
    let Some(data) = m.movie_data else { return false };
    let Some((path, len)) = app.doc().and_then(|d| media::cache_file(d, data)) else { return false };
    let spec = media::Spec::new(path, len, m, Some(px));
    with(|s| s.player = Some(Player::start(ctx, id, data, spec)));
    true
}

/// The movies, sounds and web videos on the pages for a front end's ▶ buttons (canvas_controls):
/// [{id, page, frame (page points), selected, playing}].
pub fn api_state(app: &App) -> serde_json::Value {
    use serde_json::json;
    let sel = app.selected_objs();
    let playing = with(|s| s.player.as_ref().filter(|p| !p.finished()).map(|p| p.drawable));
    let v: Vec<serde_json::Value> = app
        .drawables
        .iter()
        .filter(|d| matches!(&d.kind, DrawableKind::Movie(_)) || matches!(&d.kind, DrawableKind::Image(im) if im.web_video.is_some()))
        .filter_map(|d| {
            let pi = *app.obj_page.get(&d.id)?;
            let g = d.geom;
            Some(json!({ "id": d.id, "page": pi, "frame": [g.x, g.y, g.x + g.w, g.y + g.h], "selected": sel.contains(&d.id), "playing": playing == Some(d.id), "waiting": with(|s| s.waiting) == Some(d.id) }))
        })
        .collect();
    serde_json::Value::Array(v)
}

/// A front end's ▶ on drawable `id`: play it in place at `px` pixels, or stop it when it plays.
/// Its frames come through `frame`.
pub fn play(app: &mut App, ctx: &egui::Context, id: ObjId, px: [u32; 2]) {
    if with(|s| s.player.as_ref().is_some_and(|p| p.drawable == id && !p.finished())) {
        with(|s| s.player = None);
        return;
    }
    let Some(d) = app.drawables.iter().find(|d| d.id == id) else { return };
    let (web, movie) = match &d.kind {
        DrawableKind::Movie(m) => (None, Some(m.clone())),
        DrawableKind::Image(im) => (im.web_video.as_ref().map(|w| w.url.clone()), None),
        _ => return,
    };
    let px = [(px[0].max(2) + 1) & !1, (px[1].max(2) + 1) & !1];
    if !start_player(app, ctx, id, web.as_deref(), movie.as_ref(), px) {
        if web.is_some() {
            with(|s| s.waiting = Some(id));
            with(|s| s.waiting_px = px);
            app.notify("Loading the video…");
        } else {
            app.notify("This movie can't be played (no ffmpeg?)");
        }
    }
}

/// The newest frame of what plays (for a front end): (drawable, [w, h], RGBA), None when no new
/// frame came; a web video waiting for its stream starts once it's there.
pub fn frame(app: &mut App, ctx: &egui::Context) -> Option<(ObjId, [usize; 2], Vec<u8>)> {
    if let Some(id) = with(|s| s.waiting) {
        let px = with(|s| s.waiting_px);
        let url = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
            DrawableKind::Image(im) => im.web_video.as_ref().map(|w| w.url.clone()),
            _ => None,
        });
        match url {
            Some(u) if start_player(app, ctx, id, Some(&u), None, px) => with(|s| s.waiting = None),
            Some(_) => {}
            None => with(|s| s.waiting = None),
        }
    }
    let id = with(|s| s.player.as_mut().filter(|p| !p.finished()).map(|p| {
        // the frame goes into a texture of the (headless) context; its pixels are read back
        // from the texture's upload
        let _ = p.texture(ctx);
        p.drawable
    }))?;
    let mut delta = ctx.tex_manager().write().take_delta();
    std::mem::take(&mut delta.set).into_values().flatten().filter_map(|d| match d.image {
        egui::ImageData::Color(c) => Some((id, c.size, c.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect())),
        #[allow(unreachable_patterns)]
        _ => None,
    }).last()
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
            start_player(app, ctx, id, web.as_deref(), movie.as_ref(), px)
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
