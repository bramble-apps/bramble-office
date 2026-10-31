//! File > Export To: PDF, Images (PNG / JPEG), Movie and Animated GIF, like Keynote's Export
//! dialog. Movies and GIFs show each slide for a set time (made with ffmpeg); builds and
//! transitions are not in them yet. File > Print makes a PDF and opens it to print.
//!
//! Exports render one slide per frame (with a progress bar) so the UI never freezes.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use egui::RichText;

use crate::app::App;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Pdf,
    Png,
    Jpeg,
    Movie,
    Gif,
    /// A web page that plays the slides (keys, clicks, full screen).
    Html,
    /// PowerPoint: editable shapes, text, pictures and tables (pptx.rs), or with
    /// `pptx_pictures` each slide as a full-slide picture with text boxes over it.
    Pptx,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Good,
    Better,
    Best,
}

impl Quality {
    fn width(self) -> u32 {
        match self {
            Quality::Good => 1280,
            Quality::Better => 1920,
            Quality::Best => 3840,
        }
    }
}

pub struct ExportState {
    pub open: bool,
    pub format: Format,
    pub quality: Quality,
    pub include_skipped: bool,
    picking: Option<Receiver<Option<PathBuf>>>,
    job: Option<Job>,
    /// Movie / GIF: seconds each slide is shown.
    pub seconds: f32,
    /// Movie / GIF: play the show with its builds and transitions (else one still per slide).
    pub animated: bool,
    /// ffmpeg running in the background: (output, result).
    encoding: Option<(PathBuf, Receiver<Result<(), String>>)>,
    /// File > Print: export a PDF to a temporary file, then open it.
    print: bool,
    /// PowerPoint: slides as pictures (exactly as they look) instead of editable objects.
    pub pptx_pictures: bool,
}

impl Default for ExportState {
    fn default() -> Self {
        ExportState {
            open: false,
            format: Format::Pdf,
            quality: Quality::Better,
            include_skipped: false,
            picking: None,
            job: None,
            seconds: 5.0,
            animated: true,
            encoding: None,
            print: false,
            pptx_pictures: false,
        }
    }
}

struct Job {
    format: Format,
    path: PathBuf,
    indices: Vec<usize>,
    next: usize,
    width: u32,
    /// PDF pages: (pixel w, pixel h, JPEG bytes)
    pages: Vec<(u32, u32, Vec<u8>)>,
    page_size: (f32, f32),
    stem: String,
    /// Movie / GIF frames are written here before encoding.
    frames: Option<PathBuf>,
    seconds: f32,
    /// PowerPoint: each slide's text boxes as PowerPoint text (over its picture).
    texts: Vec<String>,
}

impl ExportState {
    pub fn show(&mut self, format: Format) {
        self.open = true;
        self.format = format;
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    dialog(app, ctx);
    poll_pick(app);
    tick(app, ctx);
    poll_encoding(app, ctx);
}

/// File > Print…: a PDF of the slides opened in the PDF viewer (print from there).
pub fn print(app: &mut App) {
    let Some(dir) = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/keydeck/print")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    let stem = app.doc.as_ref().and_then(|d| d.path.as_ref()).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    app.export.format = Format::Pdf;
    app.export.print = true;
    start_job(app, dir.join(format!("{stem}.pdf")));
}

fn poll_encoding(app: &mut App, ctx: &egui::Context) {
    let Some((out, rx)) = &app.export.encoding else { return };
    match rx.try_recv() {
        Ok(r) => {
            let msg = match &r {
                Ok(()) => format!("Exported to {}", out.display()),
                Err(e) => format!("Export failed: {e}"),
            };
            app.export.encoding = None;
            app.notify(msg.clone());
            // KN_RECORD (automated checks): quit when done
            if std::env::var_os("KN_RECORD").is_some() {
                eprintln!("{msg}");
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            egui::Window::new("Exporting…").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Encoding the video…");
                });
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        Err(_) => app.export.encoding = None,
    }
}

fn dialog(app: &mut App, ctx: &egui::Context) {
    if !app.export.open {
        return;
    }
    let mut open = true;
    let mut go = false;
    egui::Window::new("Export Your Presentation").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        let e = &mut app.export;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut e.format, Format::Pdf, "PDF");
            if ui.selectable_label(matches!(e.format, Format::Png | Format::Jpeg), "Images").clicked() && !matches!(e.format, Format::Png | Format::Jpeg) {
                e.format = Format::Png;
            }
            ui.selectable_value(&mut e.format, Format::Movie, "Movie");
            ui.selectable_value(&mut e.format, Format::Gif, "Animated GIF");
            ui.selectable_value(&mut e.format, Format::Html, "HTML");
            ui.selectable_value(&mut e.format, Format::Pptx, "PowerPoint");
        });
        ui.separator();
        if matches!(e.format, Format::Movie | Format::Gif) {
            ui.horizontal(|ui| {
                ui.label("Go to next slide after:");
                crate::theme::stepper32(ui, &mut e.seconds, 0.5, 0.5..=60.0, " s", 1);
            });
            ui.checkbox(&mut e.animated, "Include builds and transitions");
            if e.animated {
                ui.label(RichText::new("The show plays in the window while it is recorded; clicks come by themselves.").small().color(crate::theme::TEXT_DIM()));
            }
        }
        if e.format == Format::Pptx {
            ui.checkbox(&mut e.pptx_pictures, "Slides as pictures");
            let note = if e.pptx_pictures {
                "Each slide as a picture (exactly as it looks here) with its text editable over it, and the presenter notes."
            } else {
                "Shapes, text, pictures, tables, backgrounds and presenter notes stay editable; charts and movies go as pictures. Builds and transitions are left out."
            };
            ui.label(RichText::new(note).small().color(crate::theme::TEXT_DIM()));
        }
        if matches!(e.format, Format::Png | Format::Jpeg) {
            ui.horizontal(|ui| {
                ui.label("Format:");
                ui.radio_value(&mut e.format, Format::Png, "PNG");
                ui.radio_value(&mut e.format, Format::Jpeg, "JPEG");
            });
        }
        ui.horizontal(|ui| {
            ui.label(match e.format {
                Format::Pdf => "Image Quality:",
                Format::Movie | Format::Gif => "Resolution:",
                _ => "Size:",
            });
            ui.radio_value(&mut e.quality, Quality::Good, "Good");
            ui.radio_value(&mut e.quality, Quality::Better, "Better");
            ui.radio_value(&mut e.quality, Quality::Best, "Best");
        });
        ui.checkbox(&mut e.include_skipped, "Include skipped slides");
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Next…").strong()).clicked() {
                go = true;
            }
        });
    });
    // scripted runs press Next themselves
    if std::env::var_os("KN_EXPORT_TO").is_some() {
        go = true;
    }
    if go {
        open = false;
        start_pick(app);
    }
    app.export.open = open;
}

fn start_pick(app: &mut App) {
    // scripted runs: KN_EXPORT_TO names the file (no native dialog)
    if let Some(p) = std::env::var_os("KN_EXPORT_TO") {
        app.export.print = false;
        start_job(app, PathBuf::from(p));
        return;
    }
    let format = app.export.format;
    let name = app
        .doc
        .as_ref()
        .and_then(|d| d.path.as_ref())
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".into());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let r = match format {
            Format::Pdf => kn_ui::files::FileDialog::new().add_filter("PDF", &["pdf"]).set_file_name(format!("{name}.pdf")).save_file(),
            Format::Movie => kn_ui::files::FileDialog::new().add_filter("MPEG-4 Movie", &["mp4"]).set_file_name(format!("{name}.mp4")).save_file(),
            Format::Gif => kn_ui::files::FileDialog::new().add_filter("Animated GIF", &["gif"]).set_file_name(format!("{name}.gif")).save_file(),
            Format::Pptx => kn_ui::files::FileDialog::new().add_filter("PowerPoint", &["pptx"]).set_file_name(format!("{name}.pptx")).save_file(),
            Format::Html => kn_ui::files::FileDialog::new().set_title("Name of the web page folder").set_file_name(name.clone()).save_file(),
            _ => kn_ui::files::FileDialog::new().set_title("Choose a folder for the images").pick_folder(),
        };
        let _ = tx.send(r);
    });
    app.export.picking = Some(rx);
}

fn poll_pick(app: &mut App) {
    let Some(rx) = &app.export.picking else { return };
    let Ok(res) = rx.try_recv() else { return };
    app.export.picking = None;
    let Some(path) = res else { return };
    app.export.print = false;
    start_job(app, path);
}

/// Start an animated movie / GIF export to `path` with the current settings (tests).
pub fn record_to(app: &mut App, path: PathBuf) {
    app.export.animated = true;
    start_job(app, path);
}

/// The file name with the format's extension (a name typed without one would otherwise save as a
/// file nothing opens).
fn with_extension(path: PathBuf, format: Format) -> PathBuf {
    let ext = match format {
        Format::Pdf => "pdf",
        Format::Movie => "mp4",
        Format::Gif => "gif",
        Format::Pptx => "pptx",
        Format::Html => "html",
        Format::Png | Format::Jpeg => return path,
    };
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)) {
        return path;
    }
    let mut s = path.into_os_string();
    s.push(".");
    s.push(ext);
    PathBuf::from(s)
}

fn start_job(app: &mut App, path: PathBuf) {
    let path = with_extension(path, app.export.format);
    // images go in a folder: make it when it isn't there yet
    if matches!(app.export.format, Format::Png | Format::Jpeg) {
        if let Err(e) = std::fs::create_dir_all(&path) {
            app.notify(format!("Export failed: {e}"));
            return;
        }
    }
    // PowerPoint with editable objects: written in one go
    if app.export.format == Format::Pptx && !app.export.pptx_pictures {
        let Some(doc) = app.doc.as_ref() else { return };
        let r = &mut app.renderer;
        let res = crate::textures::guarded("export", || crate::pptx::write(doc, r, &path, None));
        match res {
            Some(Ok(())) => app.notify(format!("Exported to {}", path.display())),
            Some(Err(e)) => app.notify(format!("Export failed: {e}")),
            None => app.notify("Export failed: internal error"),
        }
        return;
    }
    let Some(doc) = app.doc.as_ref() else { return };
    let size = doc.slide_size();
    let indices: Vec<usize> = (0..app.slides.len()).filter(|i| app.export.include_skipped || !app.slides[*i].skipped).collect();
    let stem = doc
        .path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Slide".into());
    let format = app.export.format;
    let frames = matches!(format, Format::Movie | Format::Gif).then(|| std::env::temp_dir().join(format!("kn-export-{}", std::process::id())));
    // with builds and transitions: record the show itself
    if let (Some(dir), true) = (frames.clone(), app.export.animated) {
        let _ = std::fs::remove_dir_all(&dir);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            app.notify(format!("Export failed: {e}"));
            return;
        }
        let width = match format {
            Format::Gif => app.export.quality.width().min(960) / 2 * 2,
            _ => app.export.quality.width(),
        };
        let fps = if format == Format::Gif { 15.0 } else { 30.0 };
        let first = indices.first().copied().unwrap_or(0);
        app.renderer.set_async_images(false);
        let mut play = crate::play::Play::new(first, true, false, false);
        play.recording = Some(crate::play::Recording::new(dir, path, format, fps, app.export.seconds, width));
        app.play = Some(play);
        return;
    }
    if let Some(f) = &frames {
        let _ = std::fs::remove_dir_all(f);
        if let Err(e) = std::fs::create_dir_all(f) {
            app.notify(format!("Export failed: {e}"));
            return;
        }
    }
    let width = match format {
        // GIFs get big quickly; Keynote's GIF sizes are small too
        Format::Gif => app.export.quality.width().min(960) / 2 * 2,
        _ => app.export.quality.width(),
    };
    app.export.job = Some(Job { format, path, indices, next: 0, width, pages: Vec::new(), page_size: (size.w, size.h), stem, frames, seconds: app.export.seconds, texts: Vec::new() });
}

fn tick(app: &mut App, ctx: &egui::Context) {
    let Some(mut job) = app.export.job.take() else { return };
    // progress window
    let total = job.indices.len().max(1);
    let mut cancel = false;
    egui::Window::new("Exporting…").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.add(egui::ProgressBar::new(job.next as f32 / total as f32).show_percentage().desired_width(280.0));
        ui.label(format!("Slide {} of {}", (job.next + 1).min(total), total));
        if ui.button("Cancel").clicked() {
            cancel = true;
        }
    });
    if cancel {
        app.notify("Export cancelled");
        return;
    }
    if job.next < job.indices.len() {
        let i = job.indices[job.next];
        let pptx = job.format == Format::Pptx;
        let mut text = String::new();
        let rendered = app.scene(i).and_then(|sc| {
            let doc = app.doc.as_ref()?;
            let r = &mut app.renderer;
            crate::textures::guarded("export", || {
                if pptx {
                    let (ids, xml) = crate::pptx_text::text_boxes(r, &sc);
                    text = xml;
                    crate::pptx_text::render_without(r, doc, &sc, job.width, &ids)
                } else {
                    r.render_scene(doc, &sc, job.width)
                }
            })
            .flatten()
        });
        if pptx {
            job.texts.push(text);
        }
        if let Some(pm) = rendered {
            if let Err(e) = write_slide(&mut job, &pm) {
                app.notify(format!("Export failed: {e}"));
                return;
            }
        }
        job.next += 1;
        ctx.request_repaint();
        app.export.job = Some(job);
        return;
    }
    // finished
    if let Some(dir) = job.frames.clone() {
        let (tx, rx) = channel();
        let (format, out, secs) = (job.format, job.path.clone(), job.seconds);
        std::thread::spawn(move || {
            let r = encode(format, &dir, &out, secs);
            let _ = std::fs::remove_dir_all(&dir);
            let _ = tx.send(r);
        });
        app.export.encoding = Some((job.path, rx));
        return;
    }
    let notes: Vec<String> = job.indices.iter().map(|&i| app.slides.get(i).and_then(|s| app.doc.as_ref().map(|d| d.notes(s.slide))).unwrap_or_default()).collect();
    let result = match job.format {
        Format::Pdf => write_pdf(&job),
        Format::Html => write_html(&job, &notes),
        Format::Pptx => write_pptx(&job, &notes),
        _ => Ok(()),
    };
    match result {
        Ok(()) if app.export.print => {
            app.export.print = false;
            match kn_ui::open_command(&job.path).stdin(std::process::Stdio::null()).spawn() {
                Ok(_) => app.notify("Opened the slides as a PDF: print from the viewer"),
                Err(e) => app.notify(format!("Couldn't open the PDF ({e}); it is at {}", job.path.display())),
            }
        }
        Ok(()) => app.notify(format!("Exported to {}", job.path.display())),
        Err(e) => app.notify(format!("Export failed: {e}")),
    }
}

/// A recorded show (play::Recording) is over: encode its frames in the background.
pub fn finish_recording(app: &mut App, dir: PathBuf, out: PathBuf, format: Format, fps: f32, frames: u32) {
    if frames == 0 {
        let _ = std::fs::remove_dir_all(&dir);
        app.notify("Export cancelled");
        return;
    }
    let (tx, rx) = channel();
    let o = out.clone();
    std::thread::spawn(move || {
        let r = encode_frames(format, &dir, &o, fps);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = tx.send(r);
    });
    app.export.encoding = Some((out, rx));
    app.notify("Encoding the movie…");
}

/// Movie or GIF from the recorded frames f00000.png… at `fps`.
fn encode_frames(format: Format, frames: &std::path::Path, out: &std::path::Path, fps: f32) -> Result<(), String> {
    let input = frames.join("f%05d.png");
    let rate = format!("{fps}");
    let mut cmd = std::process::Command::new("timeout");
    cmd.arg("3600").arg("ffmpeg").args(["-y", "-loglevel", "error", "-framerate", &rate, "-i"]).arg(&input);
    match format {
        Format::Gif => {
            cmd.args(["-vf", "split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"]);
        }
        _ => {
            cmd.args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-movflags", "+faststart"]);
        }
    }
    let o = cmd.arg(out).stdin(std::process::Stdio::null()).output().map_err(|e| format!("ffmpeg is needed for this export ({e})"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("ffmpeg failed").to_string())
    }
}

/// Movie (H.264 MP4) or animated GIF from numbered PNG frames, one per slide, with ffmpeg.
fn encode(format: Format, frames: &std::path::Path, out: &std::path::Path, secs: f32) -> Result<(), String> {
    let input = frames.join("f%04d.png");
    let rate = format!("1/{secs}");
    let mut cmd = std::process::Command::new("timeout");
    cmd.arg("1800").arg("ffmpeg").args(["-y", "-loglevel", "error", "-framerate", &rate, "-i"]).arg(&input);
    match format {
        Format::Gif => {
            cmd.args(["-vf", "split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"]);
        }
        _ => {
            cmd.args(["-c:v", "libx264", "-r", "30", "-pix_fmt", "yuv420p", "-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2", "-movflags", "+faststart"]);
        }
    }
    let o = cmd.arg(out).stdin(std::process::Stdio::null()).output().map_err(|e| format!("ffmpeg is needed for this export ({e})"))?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("ffmpeg failed").to_string())
    }
}

/// Un-premultiplied RGBA from a tiny-skia pixmap.
fn rgba(pm: &kn_render::tiny_skia::Pixmap) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(pm.width(), pm.height());
    for (dst, px) in img.pixels_mut().zip(pm.pixels()) {
        let c = px.demultiply();
        *dst = image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    img
}

fn jpeg(img: &image::RgbaImage, quality: u8) -> Result<Vec<u8>, String> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out)
}

fn write_slide(job: &mut Job, pm: &kn_render::tiny_skia::Pixmap) -> Result<(), String> {
    let img = rgba(pm);
    let n = job.next + 1;
    match job.format {
        Format::Pdf | Format::Html | Format::Pptx => {
            let bytes = jpeg(&img, 92)?;
            job.pages.push((img.width(), img.height(), bytes));
        }
        Format::Png => {
            let p = job.path.join(format!("{}.{:03}.png", job.stem, n));
            img.save(&p).map_err(|e| e.to_string())?;
        }
        Format::Jpeg => {
            let p = job.path.join(format!("{}.{:03}.jpeg", job.stem, n));
            std::fs::write(&p, jpeg(&img, 92)?).map_err(|e| e.to_string())?;
        }
        Format::Movie | Format::Gif => {
            let dir = job.frames.as_ref().ok_or("no frame folder")?;
            // movies have no transparency: flatten on black like the slideshow
            let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
            rgb.save(dir.join(format!("f{n:04}.png"))).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// File > Export To > HTML: one self-contained page with the slides; arrows / space / clicks
/// move through them, F goes full screen, N shows the presenter notes.
/// Bytes as base64 (the slides embedded in the HTML page).
fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn write_html(job: &Job, notes: &[String]) -> Result<(), String> {
    // one file, the slides inside it: it opens anywhere (moved, mailed, a sandboxed browser)
    let mut slides = String::new();
    for (i, (_, _, bytes)) in job.pages.iter().enumerate() {
        let note = notes.get(i).map(|n| esc(n).replace('\n', "<br>")).unwrap_or_default();
        slides.push_str(&format!("<figure class=\"s\"><img src=\"data:image/jpeg;base64,{}\" alt=\"Slide {}\"><figcaption>{note}</figcaption></figure>\n", base64(bytes), i + 1));
    }
    let (w, h) = job.page_size;
    let html = format!(r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
html,body{{margin:0;height:100%;background:#000;overflow:hidden;font-family:system-ui,sans-serif}}
.s{{position:absolute;inset:0;margin:0;display:flex;align-items:center;justify-content:center;opacity:0;transition:opacity .5s}}
.s.on{{opacity:1}}
.s img{{max-width:100vw;max-height:100vh;aspect-ratio:{w}/{h}}}
figcaption{{display:none;position:fixed;left:0;right:0;bottom:0;max-height:30vh;overflow:auto;background:rgba(0,0,0,.8);color:#eee;padding:12px 18px;font-size:16px}}
body.notes .s.on figcaption{{display:block}}
#n{{position:fixed;right:12px;bottom:10px;color:#888;font-size:13px}}
</style></head><body>
{slides}<div id="n"></div>
<script>
const s=[...document.querySelectorAll('.s')];let i=0;
function go(k){{i=Math.max(0,Math.min(s.length-1,k));s.forEach((e,j)=>e.classList.toggle('on',j==i));document.getElementById('n').textContent=(i+1)+' / '+s.length;location.hash=i+1}}
document.addEventListener('keydown',e=>{{if(['ArrowRight','ArrowDown',' ','PageDown','Enter'].includes(e.key))go(i+1);else if(['ArrowLeft','ArrowUp','PageUp','Backspace'].includes(e.key))go(i-1);else if(e.key=='Home')go(0);else if(e.key=='End')go(s.length-1);else if(e.key=='f')document.documentElement.requestFullscreen();else if(e.key=='n')document.body.classList.toggle('notes')}});
document.addEventListener('click',e=>go(e.clientX<innerWidth/4?i-1:i+1));
go((parseInt(location.hash.slice(1))||1)-1);
</script></body></html>"#, title = esc(&job.stem));
    std::fs::write(&job.path, html).map_err(|e| e.to_string())
}

/// File > Export To > PowerPoint: one picture per slide (Office Open XML), notes included.
fn write_pptx(job: &Job, notes: &[String]) -> Result<(), String> {
    use std::io::Write;
    let emu = |pt: f32| (pt * 12700.0).round() as i64;
    let (cx, cy) = (emu(job.page_size.0), emu(job.page_size.1));
    let n = job.pages.len();
    let file = std::fs::File::create(&job.path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipWriter::new(file);
    let o = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, body: &[u8]| -> Result<(), String> {
        z.start_file(name, o).map_err(|e| e.to_string())?;
        z.write_all(body).map_err(|e| e.to_string())
    };
    let ns = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
    let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"/><Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#);
    for i in 1..=n {
        ct.push_str(&format!(r#"<Override PartName="/ppt/slides/slide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/><Override PartName="/ppt/notesSlides/notesSlide{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml"/>"#));
    }
    ct.push_str("</Types>");
    put("[Content_Types].xml", ct.as_bytes())?;
    put("_rels/.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#)?;
    put("docProps/core.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title><dc:creator>Keydeck</dc:creator></cp:coreProperties>"#, esc(&job.stem)).as_bytes())?;
    put("docProps/app.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Keydeck</Application><Slides>{n}</Slides></Properties>"#).as_bytes())?;
    let mut sld_ids = String::new();
    let mut pres_rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="notesMasters/notesMaster1.xml"/>"#);
    for i in 1..=n {
        sld_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{}"/>"#, 255 + i, 10 + i));
        pres_rels.push_str(&format!(r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{i}.xml"/>"#, 10 + i));
    }
    pres_rels.push_str("</Relationships>");
    put("ppt/presentation.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:presentation {ns}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst><p:sldIdLst>{sld_ids}</p:sldIdLst><p:sldSz cx="{cx}" cy="{cy}"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#).as_bytes())?;
    put("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes())?;
    let theme = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Keydeck"><a:themeElements><a:clrScheme name="Keydeck"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="1F1F1F"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="0A84FF"/></a:accent1><a:accent2><a:srgbClr val="30D158"/></a:accent2><a:accent3><a:srgbClr val="FFD60A"/></a:accent3><a:accent4><a:srgbClr val="FF9F0A"/></a:accent4><a:accent5><a:srgbClr val="FF453A"/></a:accent5><a:accent6><a:srgbClr val="BF5AF2"/></a:accent6><a:hlink><a:srgbClr val="0A84FF"/></a:hlink><a:folHlink><a:srgbClr val="BF5AF2"/></a:folHlink></a:clrScheme><a:fontScheme name="Keydeck"><a:majorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Helvetica Neue"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Keydeck"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"#;
    put("ppt/theme/theme1.xml", theme.as_bytes())?;
    put("ppt/theme/theme2.xml", theme.as_bytes())?;
    let tree = r#"<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>"#;
    put("ppt/slideMasters/slideMaster1.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldMaster {ns}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="000000"/></a:solidFill><a:effectLst/></p:bgPr></p:bg><p:spTree>{tree}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#).as_bytes())?;
    put("ppt/slideMasters/_rels/slideMaster1.xml.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#)?;
    put("ppt/slideLayouts/slideLayout1.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout {ns} type="blank"><p:cSld name="Blank"><p:spTree>{tree}</p:spTree></p:cSld></p:sldLayout>"#).as_bytes())?;
    put("ppt/slideLayouts/_rels/slideLayout1.xml.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#)?;
    put("ppt/notesMasters/notesMaster1.xml", format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:notesMaster {ns}><p:cSld><p:spTree>{tree}</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/></p:notesMaster>"#).as_bytes())?;
    put("ppt/notesMasters/_rels/notesMaster1.xml.rels", br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme2.xml"/></Relationships>"#)?;
    for (i, (_, _, bytes)) in job.pages.iter().enumerate() {
        let k = i + 1;
        put(&format!("ppt/media/image{k}.jpeg"), bytes)?;
        put(&format!("ppt/slides/slide{k}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sld {ns}><p:cSld><p:spTree>{tree}<p:pic><p:nvPicPr><p:cNvPr id="2" name="Slide {k}"/><p:cNvPicPr><a:picLocks noChangeAspect="1"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>{}</p:spTree></p:cSld></p:sld>"#, job.texts.get(i).map(String::as_str).unwrap_or("")).as_bytes())?;
        put(&format!("ppt/slides/_rels/slide{k}.xml.rels"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image{k}.jpeg"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/notesSlide{k}.xml"/></Relationships>"#).as_bytes())?;
        let paras: String = notes.get(i).map(|t| t.split('\n').map(|l| format!("<a:p><a:r><a:rPr lang=\"en-US\"/><a:t>{}</a:t></a:r></a:p>", esc(l))).collect()).unwrap_or_default();
        let paras = if paras.is_empty() { "<a:p/>".to_string() } else { paras };
        put(&format!("ppt/notesSlides/notesSlide{k}.xml"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:notes {ns}><p:cSld><p:spTree>{tree}<p:sp><p:nvSpPr><p:cNvPr id="2" name="Notes"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="685800" y="4343400"/><a:ext cx="5486400" cy="4114800"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/>{paras}</p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#).as_bytes())?;
        put(&format!("ppt/notesSlides/_rels/notesSlide{k}.xml.rels"), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesMaster" Target="../notesMasters/notesMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide{k}.xml"/></Relationships>"#).as_bytes())?;
    }
    z.finish().map_err(|e| e.to_string())?;
    Ok(())
}

/// Minimal PDF: one page per slide (page size in points = slide size), each a full-page JPEG.
fn write_pdf(job: &Job) -> Result<(), String> {
    let (pw, ph) = job.page_size;
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    let n = job.pages.len();
    // object numbering: 1 catalog, 2 pages, then per page: page, content, image
    let page_obj = |i: usize| 3 + i * 3;
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let id = offsets.len();
        out.extend(format!("{id} 0 obj\n").as_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    obj(&mut out, &mut offsets, format!("<< /Type /Pages /Count {n} /Kids [{}] >>", kids.join(" ")).as_bytes());
    for (i, (w, h, jpg)) in job.pages.iter().enumerate() {
        let content_id = page_obj(i) + 1;
        let image_id = page_obj(i) + 2;
        obj(
            &mut out,
            &mut offsets,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 {image_id} 0 R >> >> /Contents {content_id} 0 R >>"
            )
            .as_bytes(),
        );
        let content = format!("q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q");
        obj(&mut out, &mut offsets, format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()).as_bytes());
        let mut body = format!(
            "<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            jpg.len()
        )
        .into_bytes();
        body.extend(jpg);
        body.extend(b"\nendstream");
        obj(&mut out, &mut offsets, &body);
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes());
    for o in &offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len() + 1).as_bytes());
    std::fs::write(&job.path, out).map_err(|e| e.to_string())
}

/// Headless export for tests and the command line: `kn-app --export-pdf in.key out.pdf`.
pub fn export_pdf_headless(input: &std::path::Path, output: &std::path::Path) -> Result<usize, String> {
    export_headless(input, output, Format::Pdf, 1920, 0.0)
}

/// `kn-app --export in.key out.{pdf,mp4,gif} [seconds per slide]`.
pub fn export_any_headless(input: &std::path::Path, output: &std::path::Path, secs: f32) -> Result<usize, String> {
    match output.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("pdf") => export_headless(input, output, Format::Pdf, 1920, 0.0),
        Some("mp4") => export_headless(input, output, Format::Movie, 1920, secs),
        Some("gif") => export_headless(input, output, Format::Gif, 960, secs),
        Some("pptx") => crate::pptx::convert(input, output, false).map(|_| kn_model::Document::open(input).map(|d| d.slides().len()).unwrap_or(0)),
        Some("html") => export_headless(input, output, Format::Html, 1920, 0.0),
        _ => Err("the output must end in .pdf, .mp4, .gif, .pptx or .html".into()),
    }
}

fn export_headless(input: &std::path::Path, output: &std::path::Path, format: Format, width: u32, secs: f32) -> Result<usize, String> {
    let doc = kn_model::Document::open(input).map_err(|e| e.to_string())?;
    let slides = doc.slides();
    let size = doc.slide_size();
    let mut r = kn_render::Renderer::new();
    let frames = matches!(format, Format::Movie | Format::Gif).then(|| std::env::temp_dir().join(format!("kn-export-{}", std::process::id())));
    if let Some(f) = &frames {
        std::fs::create_dir_all(f).map_err(|e| e.to_string())?;
    }
    let mut job = Job {
        format,
        path: with_extension(output.to_path_buf(), format),
        indices: (0..slides.len()).filter(|i| !slides[*i].skipped).collect(),
        next: 0,
        width,
        pages: Vec::new(),
        page_size: (size.w, size.h),
        stem: input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        frames: frames.clone(),
        seconds: secs,
        texts: Vec::new(),
    };
    for k in 0..job.indices.len() {
        let i = job.indices[k];
        let sc = doc.scene(&slides, i).ok_or("scene")?;
        let pm = if job.format == Format::Pptx {
            let (ids, xml) = crate::pptx_text::text_boxes(&mut r, &sc);
            job.texts.push(xml);
            crate::pptx_text::render_without(&mut r, &doc, &sc, job.width, &ids).ok_or("render")?
        } else {
            r.render_scene(&doc, &sc, job.width).ok_or("render")?
        };
        job.next = k;
        write_slide(&mut job, &pm)?;
    }
    if let Some(f) = frames {
        let r = encode(format, &f, output, secs);
        let _ = std::fs::remove_dir_all(&f);
        r?;
        return Ok(job.indices.len());
    }
    let notes: Vec<String> = job.indices.iter().map(|&i| doc.notes(slides[i].slide)).collect();
    match format {
        Format::Pptx => write_pptx(&job, &notes)?,
        Format::Html => write_html(&job, &notes)?,
        _ => write_pdf(&job)?,
    }
    Ok(job.pages.len())
}
