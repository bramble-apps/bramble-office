//! Attachment blocks: pictures and drawings (their file or fallback image), PDFs (the first page,
//! from Notes' preview or drawn with pdftoppm as the suite's equation tool does), file and link
//! cards ("Text Document · 31 bytes" with a page thumbnail, as on the gallery's "30 File").
//! Files are read off the UI thread; until then a grey placeholder of the right size shows.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, mpsc};

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Rect, Stroke, TextureHandle, Vec2, pos2, vec2};

use super::Editor;
use super::doc::{Att, AttKind};
use super::layout;

enum Tex {
    Loading,
    Ready(TextureHandle, Vec2),
    Failed,
}

/// What's read once about a file for its card.
#[derive(Clone, Debug)]
enum Fact {
    Length(Option<f64>),
    FirstLine(Option<String>),
}

/// Audio, by its type (Notes' recordings are m4a).
pub fn is_audio(uti: &str) -> bool {
    uti.contains("audio") || matches!(uti, "public.mp3" | "public.mpeg-4-audio" | "org.xiph.flac" | "public.aiff-audio" | "public.aifc-audio")
}

/// A picture read off the UI thread: at most MAX_TEX pixels across, with its own size.
type Read = (PathBuf, Option<(egui::ColorImage, Vec2)>);

/// The longest side a picture's texture gets (the column is at most 784 points, 2 pixels a point):
/// bigger photos are scaled down when read.
const MAX_TEX: u32 = 2048;
/// Pixels of new textures a frame takes in (the rest the next frames).
const UPLOAD_PX: usize = 2_000_000;

/// Two low-priority workers for reading pictures, PDF pages and PDFs' page sizes, so a note
/// full of photos doesn't take the cores the UI draws with. The newest job goes first: the note
/// open now, not one left behind.
pub(super) fn spawn_job(job: Box<dyn FnOnce() + Send>) {
    type Jobs = (Mutex<Vec<Box<dyn FnOnce() + Send>>>, std::sync::Condvar);
    static POOL: OnceLock<std::sync::Arc<Jobs>> = OnceLock::new();
    let pool = POOL.get_or_init(|| {
        let pool: std::sync::Arc<Jobs> = Default::default();
        for i in 0..2 {
            let pool = pool.clone();
            let _ = std::thread::Builder::new().name(format!("nt-media-{i}")).spawn(move || {
                // (Linux: a thread's own nice value)
                #[cfg(target_os = "linux")]
                unsafe {
                    libc::setpriority(libc::PRIO_PROCESS as _, 0, 10);
                }
                loop {
                    let job = {
                        let Ok(mut jobs) = pool.0.lock() else { return };
                        loop {
                            if let Some(j) = jobs.pop() {
                                break j;
                            }
                            jobs = match pool.1.wait(jobs) {
                                Ok(j) => j,
                                Err(_) => return,
                            };
                        }
                    };
                    job();
                }
            });
        }
        pool
    });
    if let Ok(mut jobs) = pool.0.lock() {
        jobs.push(job);
        pool.1.notify_one();
    }
}

#[derive(Default)]
pub struct Media {
    tex: HashMap<PathBuf, Tex>,
    chan: Option<(mpsc::Sender<Read>, mpsc::Receiver<Read>)>,
    /// pictures read, waiting for their turn to become textures
    pending: VecDeque<Read>,
    /// PDFs' page sizes (pdfinfo, off the UI thread): asked for, and those come back
    pdf_info: HashMap<PathBuf, bool>,
    /// The PDFs' page sizes as read from the files (kept: a note loaded again gets them too).
    pdf_sizes: HashMap<PathBuf, Vec<[f32; 2]>>,
    pdf_chan: Option<(mpsc::Sender<(PathBuf, Vec<[f32; 2]>)>, mpsc::Receiver<(PathBuf, Vec<[f32; 2]>)>)>,
    /// files' facts read on the workers: an audio file's length, a text file's first line
    facts: HashMap<PathBuf, Option<Fact>>,
    facts_chan: Option<(mpsc::Sender<(PathBuf, Fact)>, mpsc::Receiver<(PathBuf, Fact)>)>,
    /// this frame's audio cards: (attachment, play button, bar, file, length)
    pub audio_hits: Vec<(String, Rect, Rect, PathBuf, Option<f64>)>,
    /// picture sizes in pixels by attachment id
    sizes: HashMap<String, Vec2>,
    /// each PDF's viewer: where it is scrolled to, the page shown
    pub pdf: HashMap<String, super::pdf::PdfView>,
    /// this frame's PDF controls (for the next press)
    pub pdf_hits: Vec<(String, super::pdf::Hit, Rect)>,
    /// the PDF header menu open (attachment, where), Rename's field, Quick Look
    pub pdf_menu: Option<(String, egui::Pos2)>,
    pub pdf_rename: Option<(String, String)>,
    pub quick_look: Option<String>,
}

impl Media {
    /// Takes in pictures read since the last frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some((_, rx)) = &self.chan {
            while let Ok(r) = rx.try_recv() {
                self.pending.push_back(r);
            }
        }
        // (a few megapixels a frame: uploading a dozen photos at once would stall one)
        let mut px = 0;
        while px < UPLOAD_PX {
            let Some((path, img)) = self.pending.pop_front() else { break };
            let t = match img {
                Some((img, size)) => {
                    px += img.size[0] * img.size[1];
                    Tex::Ready(ctx.load_texture(path.to_string_lossy(), img, egui::TextureOptions::LINEAR), size)
                }
                None => Tex::Failed,
            };
            self.tex.insert(path, t);
            ctx.request_repaint();
        }
        if !self.pending.is_empty() {
            ctx.request_repaint();
        }
    }

    /// A file's length (audio) or first line (text), read off the UI thread; None until then.
    fn fact(&mut self, ctx: &egui::Context, path: &Path, audio: bool) -> Option<Fact> {
        if let Some(rx) = self.facts_chan.as_ref().map(|c| &c.1) {
            for (p, f) in rx.try_iter() {
                self.facts.insert(p, Some(f));
            }
        }
        match self.facts.get(path) {
            Some(f) => return f.clone(),
            None => {
                self.facts.insert(path.to_path_buf(), None);
            }
        }
        let tx = self.facts_chan.get_or_insert_with(mpsc::channel).0.clone();
        let (p, ctx) = (path.to_path_buf(), ctx.clone());
        spawn_job(Box::new(move || {
            let f = if audio {
                Fact::Length(crate::audio::info(&p).ok().map(|i| i.2))
            } else {
                // (the first line of what reads as text, from the first few KB)
                use std::io::Read;
                let mut b = vec![0u8; 4096];
                let n = std::fs::File::open(&p).and_then(|mut f| f.read(&mut b)).unwrap_or(0);
                b.truncate(n);
                Fact::FirstLine(String::from_utf8(b).ok().and_then(|t| t.lines().next().map(|l| l.chars().take(18).collect())))
            };
            let _ = tx.send((p, f));
            ctx.request_repaint();
        }));
        None
    }

    /// Whether pictures are still being read or waiting to become textures.
    pub fn busy(&self) -> bool {
        !self.pending.is_empty() || self.tex.values().any(|t| matches!(t, Tex::Loading))
    }

    /// A PDF's page sizes, read off the UI thread the first time (see [`Media::pdf_pages_read`]).
    pub fn want_pdf_pages(&mut self, ctx: &egui::Context, path: &Path) {
        if self.pdf_info.contains_key(path) {
            return;
        }
        self.pdf_info.insert(path.to_path_buf(), false);
        let tx = self.pdf_chan.get_or_insert_with(mpsc::channel).0.clone();
        let (p, ctx) = (path.to_path_buf(), ctx.clone());
        spawn_job(Box::new(move || {
            let pages = pdf_pages(&p);
            let _ = tx.send((p, pages));
            ctx.request_repaint();
        }));
    }

    /// The PDFs' page sizes read from their files so far (the ones read since the last frame
    /// added).
    pub fn pdf_pages_read(&mut self) -> &HashMap<PathBuf, Vec<[f32; 2]>> {
        if let Some((_, rx)) = &self.pdf_chan {
            for (p, pages) in rx.try_iter() {
                if !pages.is_empty() {
                    self.pdf_sizes.insert(p, pages);
                }
            }
        }
        &self.pdf_sizes
    }

    pub fn sizes(&self) -> HashMap<String, Vec2> {
        self.sizes.clone()
    }

    fn get(&mut self, ctx: &egui::Context, path: &Path, pdf: bool) -> Option<(egui::TextureId, Vec2)> {
        let p = path.to_path_buf();
        self.load(ctx, path.to_path_buf(), move || if pdf { pdf_page(&p, 0, None) } else { std::fs::read(&p).ok() })
    }

    /// A drawing's picture for a dark background: black and grey ink turned light (lightness
    /// flipped), coloured ink kept, as Notes shows handwriting in dark mode.
    pub fn dark_ink(&mut self, ctx: &egui::Context, path: &Path) -> Option<(egui::TextureId, Vec2)> {
        let key = PathBuf::from(format!("{}#dark-ink", path.display()));
        let p = path.to_path_buf();
        self.load(ctx, key, move || {
            let mut img = image::load_from_memory(&std::fs::read(&p).ok()?).ok()?.to_rgba8();
            for px in img.pixels_mut() {
                let [r, g, b, a] = px.0;
                let (hi, lo) = (r.max(g).max(b), r.min(g).min(b));
                if a > 0 && hi - lo < 40 {
                    px.0 = [255 - r, 255 - g, 255 - b, a];
                }
            }
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).ok()?;
            Some(out.into_inner())
        })
    }

    /// A PDF's page `page` (0-based), drawn `px` pixels tall, kept for the next time.
    pub fn page(&mut self, ctx: &egui::Context, path: &Path, page: usize, px: u32) -> Option<(egui::TextureId, Vec2)> {
        let key = PathBuf::from(format!("{}#page{page}@{px}", path.display()));
        let p = path.to_path_buf();
        self.load(ctx, key, move || pdf_page(&p, page, Some(px)))
    }

    /// A texture by its key, read off the UI thread the first time.
    fn load(&mut self, ctx: &egui::Context, key: PathBuf, read: impl FnOnce() -> Option<Vec<u8>> + Send + 'static) -> Option<(egui::TextureId, Vec2)> {
        match self.tex.get(&key) {
            Some(Tex::Ready(t, s)) => return Some((t.id(), *s)),
            Some(_) => return None,
            None => {}
        }
        self.tex.insert(key.clone(), Tex::Loading);
        let tx = self.chan.get_or_insert_with(mpsc::channel).0.clone();
        let p = key;
        let ctx = ctx.clone();
        spawn_job(Box::new(move || {
            let img = read().and_then(|b| image::load_from_memory(&b).ok()).map(|i| {
                let size = vec2(i.width() as f32, i.height() as f32);
                let i = if i.width().max(i.height()) > MAX_TEX { i.resize(MAX_TEX, MAX_TEX, image::imageops::FilterType::Triangle) } else { i };
                let i = i.to_rgba8();
                (egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], &i), size)
            });
            let _ = tx.send((p, img));
            ctx.request_repaint();
        }));
        None
    }
}

/// A page of a PDF as a PNG (pdftoppm): 2 pixels per point, or `px` pixels tall.
fn pdf_page(path: &Path, page: usize, px: Option<u32>) -> Option<Vec<u8>> {
    let dir = std::env::temp_dir().join(format!("nt-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    // (a file of its own per render: the same page asked for twice at once, by an editor whose
    // media was renewed meanwhile, must not read or delete the other's picture)
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = dir.join(format!("p{:x}-{page}-{}-{seq}", fxhash(path.to_string_lossy().as_bytes()), px.unwrap_or(0)));
    let n = (page + 1).to_string();
    let mut cmd = std::process::Command::new("pdftoppm");
    cmd.args(["-png", "-f", &n, "-l", &n, "-singlefile"]);
    match px {
        Some(px) => {
            cmd.args(["-scale-to-y", &px.to_string(), "-scale-to-x", "-1"]);
        }
        None => {
            cmd.args(["-r", "144"]);
        }
    }
    let ok = cmd.arg(path).arg(&out).status().ok()?.success();
    if !ok {
        return None;
    }
    let png = out.with_extension("png");
    let b = std::fs::read(&png).ok();
    let _ = std::fs::remove_file(&png);
    b
}

fn fxhash(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100000001b3))
}

/// A picture's kind as Finder names it ("JPEG image").
fn image_kind(uti: &str) -> &'static str {
    match uti {
        "public.jpeg" => "JPEG image",
        "public.png" => "PNG image",
        "public.heic" | "public.heif" => "HEIF image",
        "com.compuserve.gif" => "GIF image",
        "org.webmproject.webp" => "WebP image",
        "public.tiff" => "TIFF image",
        "public.avif" => "AVIF image",
        "com.microsoft.bmp" => "Windows BMP image",
        "public.svg-image" => "SVG image",
        "com.apple.notes.gallery" => "Scanned Document",
        _ => "Image",
    }
}

/// "31 bytes", "1.2 KB", "3.4 MB" (Finder's decimal units).
pub fn size_text(n: u64) -> String {
    match n {
        0..=999 => format!("{n} bytes"),
        1_000..=999_999 => format!("{:.0} KB", n as f64 / 1e3),
        1_000_000..=999_999_999 => format!("{:.1} MB", n as f64 / 1e6),
        _ => format!("{:.1} GB", n as f64 / 1e9),
    }
}

/// Draws attachment `id` in `r`.
pub fn paint(ed: &mut Editor, painter: &Painter, id: &str, r: Rect) {
    let Some(att) = ed.doc.att(id).cloned() else { return };
    let ctx = painter.ctx().clone();
    let marked_up = ed.draw.as_ref().is_some_and(|d| d.att == att.id && d.markup.is_some());
    match &att.kind {
        AttKind::Drawing { ink: Some(_), .. } => super::draw::paint_drawing(ed, painter, &att.id, r, None),
        AttKind::Image { path, view: nt_core::model::ViewSize::Small, name, .. } => {
            // the card (as a PDF's Small, NO3 2b): name, "JPEG image · 120 KB", the picture
            let s = ed.settings.text_scale;
            painter.rect_filled(r, CornerRadius::same((11.0 * s) as u8), if layout::dark() { Color32::from_gray(52) } else { Color32::from_rgb(229, 230, 233) });
            let shown = if name.is_empty() { path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Image".into()) } else { name.clone() };
            let bytes = path.as_deref().and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len());
            let th = Rect::from_min_size(pos2(r.max.x - 12.0 * s - 40.0 * s, r.min.y + 13.0 * s), vec2(40.0 * s, 40.0 * s));
            let tw = th.min.x - r.min.x - 24.0 * s;
            let g = painter.layout(shown, FontId::new(13.0 * s, layout::family("Sfera-Bold")), layout::text_color(), tw.max(20.0));
            let g = if g.rows.len() > 1 { painter.layout_no_wrap(g.text().chars().take(28).collect::<String>() + "\u{2026}", FontId::new(13.0 * s, layout::family("Sfera-Bold")), layout::text_color()) } else { g };
            painter.galley(pos2(r.min.x + 12.0 * s, r.min.y + 13.0 * s), g, layout::text_color());
            painter.text(pos2(r.min.x + 12.0 * s, r.min.y + 33.0 * s), Align2::LEFT_TOP, format!("{} \u{00b7} {}", image_kind(&att.uti), size_text(bytes)), FontId::new(13.0 * s, layout::family("Sfera-Regular")), super::pdf::dim());
            if let Some((t, sz)) = path.as_deref().and_then(|p| ed.media.get(&ctx, p, false)) {
                // (fitted in the square, its own shape)
                let k = (th.width() / sz.x).min(th.height() / sz.y);
                let ir = Rect::from_center_size(th.center(), vec2(sz.x * k, sz.y * k));
                painter.image(t, ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
        }
        AttKind::Image { path, .. } | AttKind::Drawing { path, .. } => {
            // (a drawing's ink adapts to dark backgrounds, as Notes does)
            let ink = matches!(att.kind, AttKind::Drawing { .. }) && layout::dark();
            let tex = path.as_deref().and_then(|p| if ink { ed.media.dark_ink(&ctx, p) } else { ed.media.get(&ctx, p, false) });
            let under = tex.as_ref().map(|t| t.0);
            match tex {
                Some((t, s)) => {
                    ed.media.sizes.insert(att.id.clone(), s);
                    painter.image(t, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None if matches!(att.kind, AttKind::Drawing { .. }) => {
                    painter.rect_stroke(r, CornerRadius::same(6), Stroke::new(1.0, layout::line_color().gamma_multiply(0.4)), egui::StrokeKind::Inside);
                }
                None => {
                    painter.rect_filled(r, 0.0, layout::card_color());
                }
            }
            if marked_up {
                super::draw::paint_drawing(ed, painter, &att.id, r, under);
            }
        }
        AttKind::Pdf { .. } => super::pdf::paint(ed, painter, &att, r),
        AttKind::File { name, path: Some(path), .. } if is_audio(&att.uti) => {
            // an audio card: Play / Pause, the name, its length (playing: the bar and the time)
            painter.rect_filled(r, CornerRadius::same(11), layout::card_color());
            let s = ed.settings.text_scale;
            let len = match ed.media.fact(&ctx, path, true) {
                Some(Fact::Length(l)) => l,
                _ => None,
            };
            let st = super::player::state();
            let mine = st.path.as_deref() == Some(path.as_path());
            let (button, bar) = super::player::card_parts(r, s);
            painter.circle_filled(button.center(), button.width() / 2.0, layout::accent());
            let c = button.center();
            let k = button.width() / 34.0;
            if mine && st.playing {
                for dx in [-4.0, 4.0] {
                    painter.rect_filled(Rect::from_center_size(c + vec2(dx * k, 0.0), vec2(4.0 * k, 13.0 * k)), CornerRadius::same(1), Color32::WHITE);
                }
            } else {
                painter.add(egui::Shape::convex_polygon(vec![c + vec2(-4.0 * k, -7.0 * k), c + vec2(8.0 * k, 0.0), c + vec2(-4.0 * k, 7.0 * k)], Color32::WHITE, Stroke::NONE));
            }
            let shown = if name.is_empty() { "Audio Recording".to_string() } else { name.clone() };
            let g = painter.layout_no_wrap(shown, FontId::new(13.0 * s, layout::family("Sfera-Semibold")), layout::text_color());
            painter.galley(pos2(bar.min.x, r.min.y + 13.0 * s), g, layout::text_color());
            let cap = if layout::dark() { Color32::from_gray(170) } else { Color32::from_rgb(110, 110, 115) };
            let font = FontId::new(11.0 * s, layout::family("Sfera-Regular"));
            if mine {
                // the bar, filled to where it is
                painter.rect_filled(bar, CornerRadius::same(2), cap.gamma_multiply(0.35));
                if let Some(l) = len.filter(|l| *l > 0.0) {
                    let f = (st.pos / l).clamp(0.0, 1.0) as f32;
                    painter.rect_filled(Rect::from_min_max(bar.min, pos2(bar.min.x + bar.width() * f, bar.max.y)), CornerRadius::same(2), layout::accent());
                }
                let t = format!("{} / {}", super::player::clock(st.pos), len.map_or("–".into(), super::player::clock));
                painter.text(pos2(r.max.x - 14.0 * s, bar.center().y), Align2::RIGHT_CENTER, t, font, cap);
                ed.request_repaint = true;
            } else {
                let t = match (&st.error, len) {
                    (Some(e), _) if st.path.is_none() => format!("Audio \u{00b7} can\u{2019}t play ({e})"),
                    (_, Some(l)) => format!("Audio \u{00b7} {}", super::player::clock(l)),
                    _ => "Audio".to_string(),
                };
                painter.text(pos2(bar.min.x, r.min.y + 33.0 * s), Align2::LEFT_TOP, t, font, cap);
            }
            ed.media.audio_hits.push((att.id.clone(), button, bar, path.clone(), len));
        }
        AttKind::File { kind, size, path, .. } => {
            painter.rect_filled(r, CornerRadius::same(11), layout::card_color());
            let s = ed.settings.text_scale;
            // the gallery's "30 File": a 10.5 pt caption, darker than the date line
            let cap = if layout::dark() { Color32::from_gray(170) } else { Color32::from_rgb(110, 110, 115) };
            painter.text(pos2(r.min.x + 11.7 * s, r.min.y + 9.5 * s), Align2::LEFT_TOP, format!("{kind} \u{00b7} {}", size_text(*size)), FontId::new(10.5 * s, layout::family("Sfera-Regular")), cap);
            // the file's first page as a small white sheet on the right
            let th = Rect::from_min_size(pos2(r.max.x - 11.0 * s - 40.0 * s, r.min.y + 10.0 * s), vec2(40.0 * s, 40.0 * s));
            painter.rect_filled(th, CornerRadius::same(2), Color32::WHITE);
            // (its first line, read once on the workers: not the whole file every frame)
            let first = path.as_deref().and_then(|p| match ed.media.fact(&ctx, p, false) {
                Some(Fact::FirstLine(l)) => l,
                _ => None,
            });
            if let Some(first) = first {
                painter.text(pos2(th.min.x + 2.0 * s, th.min.y + 3.0 * s), Align2::LEFT_TOP, first.chars().take(18).collect::<String>(), FontId::new(2.5 * s, layout::family("Sfera-Regular")), Color32::from_gray(90));
            }
        }
        AttKind::Missing { what, name, .. } => {
            // a card with the kind's icon over "Not Downloaded" (and the file's name when known)
            let s = ed.settings.text_scale;
            painter.rect_filled(r, CornerRadius::same(11), layout::card_color());
            let dim = layout::dim_color();
            let line = if name.is_empty() { None } else { Some(name.clone()) };
            let rows = 1.0 + line.is_some() as i32 as f32;
            let icon = 22.0 * s;
            // (side by side in a short card, stacked in a tall one)
            let tall = r.height() >= 62.0 * s + 40.0 * s;
            let (ic, text_at, align) = if tall {
                let top = r.center().y - (icon + 6.0 * s + rows * 15.0 * s) / 2.0;
                (pos2(r.center().x, top + icon / 2.0), pos2(r.center().x, top + icon + 6.0 * s), Align2::CENTER_TOP)
            } else {
                let x = r.min.x + 14.0 * s;
                (pos2(x + icon / 2.0, r.center().y), pos2(x + icon + 10.0 * s, r.center().y - rows * 15.0 * s / 2.0), Align2::LEFT_TOP)
            };
            let ir = Rect::from_center_size(ic, vec2(icon, icon));
            use super::doc::Missing as M;
            match what {
                M::Scan => scan_glyph(painter, ir, dim),
                M::Table => crate::icons::draw(painter, ir, crate::icons::Glyph::Table, dim),
                M::Drawing => crate::icons::draw(painter, ir, crate::icons::Glyph::Markup, dim),
                M::File => kn_ui::icons::draw(painter, ir, kn_ui::icons::Icon::File, dim),
            }
            let mut y = text_at.y;
            if let Some(n) = line {
                painter.text(pos2(text_at.x, y), align, n, FontId::new(13.0 * s, layout::family("Sfera-Semibold")), layout::text_color());
                y += 15.0 * s;
            }
            painter.text(pos2(text_at.x, y), align, "Not Downloaded", FontId::new(11.5 * s, layout::family("Sfera-Regular")), dim);
        }
        AttKind::Url { url, title } => {
            painter.rect_filled(r, CornerRadius::same(11), layout::card_color());
            let s = ed.settings.text_scale;
            let t = if title.is_empty() { url.clone() } else { title.clone() };
            painter.text(pos2(r.min.x + 11.7 * s, r.min.y + 10.5 * s), Align2::LEFT_TOP, t, FontId::new(13.0 * s, layout::family("Sfera-Bold")), layout::text_color());
            let host = url.split("//").nth(1).unwrap_or(url).split('/').next().unwrap_or("").to_string();
            painter.text(pos2(r.min.x + 11.7 * s, r.min.y + 30.0 * s), Align2::LEFT_TOP, host, FontId::new(12.0 * s, layout::family("Sfera-Regular")), layout::dim_color());
        }
        _ => {}
    }
}

/// A scan: a page inside viewfinder corners (kn-ui's 24-unit grid and 1.6-unit stroke).
fn scan_glyph(painter: &egui::Painter, r: Rect, col: Color32) {
    let q = |x: f32, y: f32| pos2(r.min.x + x / 24.0 * r.width(), r.min.y + y / 24.0 * r.height());
    let st = Stroke::new(1.6 * r.width() / 24.0, col);
    for (x, y, dx, dy) in [(2.0, 2.0, 1.0, 1.0), (22.0, 2.0, -1.0, 1.0), (22.0, 22.0, -1.0, -1.0), (2.0, 22.0, 1.0, -1.0)] {
        painter.add(egui::Shape::line(vec![q(x, y + dy * 5.0), q(x, y), q(x + dx * 5.0, y)], st));
    }
    painter.rect_stroke(Rect::from_min_max(q(7.0, 5.5), q(17.0, 18.5)), CornerRadius::same((1.5 * r.width() / 24.0) as u8), st, egui::StrokeKind::Middle);
    for y in [9.5, 12.0, 14.5] {
        painter.line_segment([q(9.5, y), q(14.5, y)], Stroke::new(1.2 * r.width() / 24.0, col));
    }
}

impl Editor {
    /// Edit > Attach File…: a picture, PDF or other file as a block at the caret.
    pub fn attach_file(&mut self, path: &Path) {
        self.insert_block(att_for_file(path));
    }
}

/// A file as an attachment: a picture, a PDF (its pages) or a file card.
pub fn att_for_file(path: &Path) -> Att {
    {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (uti, kind) = match ext.as_str() {
            "png" => ("public.png", None),
            "jpg" | "jpeg" => ("public.jpeg", None),
            "gif" => ("com.compuserve.gif", None),
            "webp" => ("org.webmproject.webp", None),
            // (Notes 4.13 keeps an attached PDF as a PaperKit document of its pages)
            "pdf" => (nt_core::model::UTI_PAPER_PDF, Some(())),
            "txt" | "text" => ("public.plain-text", Some(())),
            // (audio plays in its card)
            "m4a" => ("com.apple.m4a-audio", Some(())),
            "mp3" => ("public.mp3", Some(())),
            "wav" => ("com.microsoft.waveform-audio", Some(())),
            "aac" => ("public.aac-audio", Some(())),
            "flac" => ("org.xiph.flac", Some(())),
            "ogg" | "oga" => ("org.xiph.ogg-audio", Some(())),
            "md" => ("net.daringfireball.markdown", Some(())),
            _ => ("public.data", Some(())),
        };
        let kind = match (uti, kind) {
            (nt_core::model::UTI_PAPER_PDF, _) => {
                let pages = pdf_pages(path);
                let (w, h) = pages.first().map_or((612.0, 792.0), |p| (p[0], p[1]));
                // its name without the extension, as Notes titles it
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or(name);
                AttKind::Pdf { path: Some(path.into()), preview: None, width: w, height: h, name, pages, view: nt_core::model::ViewSize::Large, thumbs: false }
            }
            (_, None) => {
                let (w, h) = image::image_dimensions(path).map(|(w, h)| (w as f32, h as f32)).unwrap_or((320.0, 200.0));
                AttKind::Image { path: Some(path.into()), width: w, height: h, view: nt_core::model::ViewSize::Large, name }
            }
            _ => {
                let size = std::fs::metadata(path).map_or(0, |m| m.len());
                AttKind::File { name, kind: super::doc::kind_name(uti), size, path: Some(path.into()) }
            }
        };
        Att::new(uti, kind)
    }
}

/// A PDF's pages' sizes in points (pdfinfo; empty when it can't be read).
pub fn pdf_pages(path: &Path) -> Vec<[f32; 2]> {
    let Ok(out) = std::process::Command::new("pdfinfo").args(["-f", "1", "-l", "100000"]).arg(path).output() else { return Vec::new() };
    let s = String::from_utf8_lossy(&out.stdout);
    // "Page    3 size: 612 x 792 pts (letter)"
    s.lines()
        .filter(|l| l.starts_with("Page") && l.contains(" size:"))
        .filter_map(|l| {
            let mut it = l.split("size:").nth(1)?.split_whitespace().filter_map(|w| w.parse::<f32>().ok());
            Some([it.next()?, it.next()?])
        })
        .collect()
}

