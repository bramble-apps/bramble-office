//! Liquid glass buttons (Settings > General, off by default), after the Bible XML Viewer's
//! port of BitChord's recipe: what's behind seen through a lens that bends it near the rim,
//! a light surface tint (40%), a white rim highlight lit at 45° (50%), a hairline edge and a
//! soft shadow. egui has no shaders here, so the lens is a mesh whose rim samples the
//! (already blurred) backdrop from further in, and the rim light is per-vertex.

use egui::{pos2, vec2, Color32, Mesh, Pos2, Rect, Vec2};

static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_enabled(on: bool) {
    ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// What lies behind the floating toolbar: the blurred slide texture, where it is drawn, and
/// the area it shows in (set by the canvas each frame).
#[derive(Clone, Copy)]
pub struct Backdrop {
    pub tex: egui::TextureId,
    pub slide: Rect,
    pub clip: Rect,
}

pub fn set_backdrop(ctx: &egui::Context, b: Option<Backdrop>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("glass-backdrop"), b.map(|b| (b.tex, b.slide, b.clip))));
}

pub fn backdrop(ctx: &egui::Context) -> Option<Backdrop> {
    ctx.data(|d| d.get_temp::<Option<(egui::TextureId, Rect, Rect)>>(egui::Id::unique("glass-backdrop"))).flatten().map(|(tex, slide, clip)| Backdrop { tex, slide, clip })
}

/// Half-width of the soft edge of the glass's own meshes (logical points).
const FEATHER: f32 = 0.6;

/// Points round a rounded rectangle (clockwise from the top-left corner's start) with their
/// outward normals.
fn outline(r: Rect, radius: f32) -> Vec<(Pos2, Vec2)> {
    let rad = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.5);
    let corners = [
        (pos2(r.min.x + rad, r.min.y + rad), std::f32::consts::PI),
        (pos2(r.max.x - rad, r.min.y + rad), 1.5 * std::f32::consts::PI),
        (pos2(r.max.x - rad, r.max.y - rad), 0.0),
        (pos2(r.min.x + rad, r.max.y - rad), 0.5 * std::f32::consts::PI),
    ];
    let steps = 16;
    let mut out: Vec<(Pos2, Vec2)> = Vec::new();
    for (i, (c, a0)) in corners.iter().enumerate() {
        for k in 0..=steps {
            let a = a0 + k as f32 / steps as f32 * std::f32::consts::FRAC_PI_2;
            let n = vec2(a.cos(), a.sin());
            out.push((*c + n * rad, n));
        }
        // points along the straight side to the next corner (so the lens can fade across it)
        let (next, na) = corners[(i + 1) % 4];
        let end = next + vec2(na.cos(), na.sin()) * rad;
        let (start, n) = *out.last().unwrap();
        let len = (end - start).length();
        let parts = (len / 5.0).floor() as usize;
        for j in 1..parts {
            out.push((start + (end - start) * (j as f32 / parts as f32), n));
        }
    }
    out
}

/// A glass capsule (or rounded rectangle), as shapes to draw under its contents. With
/// `lens`, what's behind it is refracted from the toolbar backdrop (when one is under it).
pub fn shapes(ctx: &egui::Context, r: Rect, radius: f32, lens: bool) -> Vec<egui::Shape> {
    let mut out = Vec::new();
    let light = crate::theme::is_light();
    // macOS 26 toolbar glass: a tight, faint contact shadow (the user found the old soft
    // 18 pt one too strong next to Apple's apps)
    let shadow = egui::epaint::Shadow { offset: [0, 1], blur: 8, spread: 0, color: Color32::from_black_alpha(if light { 14 } else { 40 }) };
    out.push(shadow.as_shape(r, radius).into());
    let pts = outline(r, radius);
    let center = r.center();
    let mut refracted = false;
    // the lens: rim samples come from further inside, so what's behind bends at the edge
    if let Some(b) = backdrop(ctx).filter(|_| lens) {
        let clip = b.clip.intersect(b.slide);
        if clip.expand(12.0).intersects(r) {
            // inside the backdrop's area the picture shows fully; across its edge (a panel
            // beside the canvas) it fades out over a few points, so no line runs through
            let fade = |q: Pos2| {
                let d = (q.x - clip.min.x).min(clip.max.x - q.x).min(q.y - clip.min.y).min(clip.max.y - q.y);
                let t = ((d + 10.0) / 20.0).clamp(0.0, 1.0);
                let t = t * t * (3.0 - 2.0 * t);
                Color32::from_white_alpha((t * 255.0) as u8)
            };
            let uv = |q: Pos2| pos2((q.x - b.slide.min.x) / b.slide.width(), (q.y - b.slide.min.y) / b.slide.height());
            // the bend stays near the rim (on a thin bar a deeper one met in the middle as a streak)
            let depth = (radius * 0.9).min(r.height() * 0.3).max(2.0);
            let pull = depth * 0.85;
            let mut m = Mesh::with_texture(b.tex);
            m.colored_vertex(center, fade(center));
            m.vertices[0].uv = uv(center);
            let n = pts.len() as u32;
            for (q, nrm) in &pts {
                // inner ring: undistorted; rim: pulled inward; then a hair outside it fading out
                // (meshes aren't anti-aliased: this feather smooths the edge)
                let inner = *q - *nrm * depth;
                let i = m.vertices.len();
                m.colored_vertex(inner, fade(inner));
                m.vertices[i].uv = uv(inner);
                m.colored_vertex(*q - *nrm * FEATHER, fade(*q));
                m.vertices[i + 1].uv = uv(*q - *nrm * pull);
                m.colored_vertex(*q + *nrm * FEATHER, Color32::TRANSPARENT);
                m.vertices[i + 2].uv = uv(*q - *nrm * pull);
            }
            for k in 0..n {
                let (a, b2) = (1 + 3 * k, 1 + 3 * ((k + 1) % n));
                m.add_triangle(0, a, b2);
                m.add_triangle(a, a + 1, b2);
                m.add_triangle(a + 1, b2 + 1, b2);
                m.add_triangle(a + 1, a + 2, b2 + 1);
                m.add_triangle(a + 2, b2 + 2, b2 + 1);
            }
            out.push(egui::Shape::mesh(m));
            refracted = true;
        }
    }
    // surface tint (40% over the backdrop; more where nothing is refracted)
    let tint = crate::theme::tint(if light { Color32::from_rgb(0xfa, 0xfa, 0xfa) } else { Color32::from_rgb(0x2a, 0x2a, 0x2c) });
    out.push(egui::Shape::rect_filled(r, radius, tint.gamma_multiply(if refracted { 0.4 } else { 0.72 })));
    // rim light at 45° (both lit corners), white at up to 50%, on a thin band inside the edge
    let dir = vec2(std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let mut m = Mesh::default();
    let n = pts.len() as u32;
    for (q, nrm) in &pts {
        let k = nrm.dot(dir).abs();
        // a fine rim (BitChord's is half a point): soft outside, brightest on the edge, gone
        // a point inside
        m.colored_vertex(*q + *nrm * 0.4, Color32::TRANSPARENT);
        m.colored_vertex(*q - *nrm * 0.2, Color32::from_white_alpha((k * 0.42 * 255.0) as u8));
        m.colored_vertex(*q - *nrm * 1.0, Color32::TRANSPARENT);
    }
    for k in 0..n {
        let (a, b) = (3 * k, 3 * ((k + 1) % n));
        for j in 0..2 {
            m.add_triangle(a + j, a + j + 1, b + j);
            m.add_triangle(a + j + 1, b + j + 1, b + j);
        }
    }
    out.push(egui::Shape::mesh(m));
    // hairline edge
    out.push(egui::Shape::rect_stroke(r, radius, egui::Stroke::new(0.5, Color32::from_white_alpha(if light { 28 } else { 14 })), egui::StrokeKind::Inside));
    out
}

/// What shows under a floating toolbar where the document runs on beneath it (Settings).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolbarBackground {
    /// the blurred document at full strength, then an 18 pt band fading back to sharp
    Feathered,
    /// as Pages 15.4: the blurred document under a 60 % tint, with a hard bottom edge
    Tinted,
    /// no see-through: the band is the pane colour
    Solid,
}

impl ToolbarBackground {
    pub const ALL: [(ToolbarBackground, &'static str); 3] = [(ToolbarBackground::Tinted, "Pages (tinted)"), (ToolbarBackground::Feathered, "Keydeck (feathered)"), (ToolbarBackground::Solid, "Solid")];

    /// The settings-file value.
    pub fn key(self) -> &'static str {
        match self {
            ToolbarBackground::Feathered => "feathered",
            ToolbarBackground::Tinted => "tinted",
            ToolbarBackground::Solid => "solid",
        }
    }

    pub fn from_key(k: &str) -> Option<ToolbarBackground> {
        ToolbarBackground::ALL.iter().map(|a| a.0).find(|b| b.key() == k)
    }

    /// Pages' toolbar tint over the blurred document (dark mode measured from Pages 15.4:
    /// #272727 at 60 %; light mode not measured, its mirror image for now).
    pub fn tint() -> Color32 {
        if crate::theme::is_light() { Color32::from_rgba_unmultiplied(0xf6, 0xf6, 0xf6, 153) } else { Color32::from_rgba_unmultiplied(0x27, 0x27, 0x27, 153) }
    }
}

static TOOLBAR_BG: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set_toolbar_background(b: ToolbarBackground) {
    TOOLBAR_BG.store(b as u8, std::sync::atomic::Ordering::Relaxed);
}

pub fn toolbar_background() -> ToolbarBackground {
    match TOOLBAR_BG.load(std::sync::atomic::Ordering::Relaxed) {
        1 => ToolbarBackground::Tinted,
        2 => ToolbarBackground::Solid,
        _ => ToolbarBackground::Feathered,
    }
}

static PANES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Translucent sidebars (Settings): the navigator and Format pane show the slide behind them
/// (blurred) and the desktop through them, under the theme's tint.
pub fn set_translucent_panes(on: bool) {
    PANES.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn translucent_panes() -> bool {
    PANES.load(std::sync::atomic::Ordering::Relaxed)
}

/// The blurred slide and where it lies on screen (set by the canvas each frame), for the panes.
pub fn set_slide_blur(ctx: &egui::Context, b: Option<(egui::TextureId, Rect)>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("pane-slide-blur"), b));
}

/// The desktop wallpaper, small and heavily blurred (the frosted backdrop of translucent panes
/// where the system can't blur behind windows: GNOME and KDE on Wayland). Read on a thread.
static WALLPAPER: std::sync::Mutex<Option<Option<egui::ColorImage>>> = std::sync::Mutex::new(None);

fn wallpaper_path() -> Option<std::path::PathBuf> {
    let gs = |schema: &str, key: &str| {
        std::process::Command::new("gsettings").args(["get", schema, key]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_matches('\'').to_string())
    };
    // GNOME (and Ubuntu, Budgie, Cinnamon…): the dark wallpaper in dark mode
    let dark = gs("org.gnome.desktop.interface", "color-scheme").is_some_and(|v| v.contains("dark"));
    let uri = if dark { gs("org.gnome.desktop.background", "picture-uri-dark").filter(|u| !u.is_empty()) } else { None }.or_else(|| gs("org.gnome.desktop.background", "picture-uri"));
    if let Some(u) = uri.filter(|u| !u.is_empty()) {
        let path = std::path::PathBuf::from(u.trim_start_matches("file://").replace("%20", " "));
        if path.is_file() {
            return Some(path);
        }
    }
    // KDE Plasma: the desktop's Image= in its applets config
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let cfg = std::fs::read_to_string(home.join(".config/plasma-org.kde.plasma.desktop-appletsrc")).ok()?;
    cfg.lines().find_map(|l| l.strip_prefix("Image=")).map(|v| std::path::PathBuf::from(v.trim_start_matches("file://"))).filter(|p| p.is_file())
}

fn wallpaper(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique("frosted-wallpaper");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let mut g = WALLPAPER.lock().ok()?;
    match &*g {
        None => {
            *g = Some(None);
            let ctx2 = ctx.clone();
            std::thread::spawn(move || {
                let img = wallpaper_path().and_then(|p| image::open(p).ok()).map(|i| {
                    let i = i.thumbnail(480, 480).to_rgba8();
                    let ci = egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw());
                    box_blur(&ci, 6, 3)
                });
                if let (Some(img), Ok(mut g)) = (img, WALLPAPER.lock()) {
                    *g = Some(Some(img));
                    ctx2.request_repaint();
                }
            });
            None
        }
        Some(None) => None,
        Some(Some(img)) => {
            let t = ctx.load_texture("frosted-wallpaper", img.clone(), egui::TextureOptions::LINEAR);
            ctx.data_mut(|d| d.insert_temp(id, t.clone()));
            Some(t)
        }
    }
}

/// Whether the system blurs what's behind the window itself (macOS, Windows: see native_blur).
fn system_blurs() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Paint a pane's translucent background: the frosted wallpaper (where the system can't blur
/// behind the window), the blurred slide where it reaches under the pane, then the pane's
/// colour at part strength.
pub fn pane_background(ui: &egui::Ui, color: Color32) {
    pane_background_rounded(ui, color, egui::CornerRadius::ZERO);
}

/// pane_background with rounded corners (a pane at the window's rounded corner).
pub fn pane_background_rounded(ui: &egui::Ui, color: Color32, corner: egui::CornerRadius) {
    let r = ui.clip_rect();
    let p = ui.painter().with_clip_rect(r);
    // a texture drawn over `full`, of which the pane shows its own part
    let show = |tex: egui::TextureId, full: Rect| {
        let uv = Rect::from_min_max(pos2((r.min.x - full.min.x) / full.width(), (r.min.y - full.min.y) / full.height()), pos2((r.max.x - full.min.x) / full.width(), (r.max.y - full.min.y) / full.height()));
        p.add(egui::epaint::RectShape::filled(r, corner, Color32::WHITE).with_texture(tex, uv));
    };
    if !system_blurs() {
        if let Some(w) = wallpaper(ui.ctx()) {
            // the wallpaper as the desktop shows it ("zoom": cover the screen), seen through the
            // window where it is on the screen (Wayland doesn't say: then as if at the top left)
            let (outer, monitor) = ui.ctx().input(|i| (i.viewport().outer_rect, i.viewport().monitor_size));
            let screen = monitor.unwrap_or(egui::vec2(1920.0, 1080.0));
            let origin = outer.map(|o| o.min).unwrap_or(egui::Pos2::ZERO);
            let [iw, ih] = w.size();
            let k = (screen.x / iw as f32).max(screen.y / ih as f32);
            let size = egui::vec2(iw as f32 * k, ih as f32 * k);
            let at = egui::pos2((screen.x - size.x) / 2.0, (screen.y - size.y) / 2.0) - origin.to_vec2();
            show(w.id(), Rect::from_min_size(at, size));
        }
    }
    if let Some((tex, full)) = ui.ctx().data(|d| d.get_temp::<Option<(egui::TextureId, Rect)>>(egui::Id::unique("pane-slide-blur"))).flatten() {
        show(tex, full);
    }
    // light panes frost more (Mac's light sidebars are near-white, with a faint tint)
    let frost = if crate::theme::is_light() { 0.86 } else { 0.64 };
    p.rect_filled(r, corner, color.gamma_multiply(frost));
}

static CLASSIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Settings > Classic look: the layout from before the macOS 26 styling.
pub fn set_classic(on: bool) {
    CLASSIC.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn classic() -> bool {
    CLASSIC.load(std::sync::atomic::Ordering::Relaxed)
}

/// The system's own blur behind the window (macOS vibrancy, Windows mica / acrylic), switched
/// with Settings > Translucent sidebars. Elsewhere nothing: the panes draw a frosted wallpaper.
pub fn native_blur(frame: &eframe::Frame, on: bool) {
    static APPLIED: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(2);
    let want = u8::from(on);
    if APPLIED.swap(want, std::sync::atomic::Ordering::Relaxed) == want {
        return;
    }
    #[cfg(target_os = "macos")]
    mac_vibrancy::set(frame, on);
    #[cfg(target_os = "windows")]
    {
        use window_vibrancy::{apply_acrylic, apply_mica, clear_acrylic, clear_mica};
        if on {
            if apply_mica(frame, None).is_err() {
                let _ = apply_acrylic(frame, Some((24, 24, 24, 110)));
            }
        } else {
            let _ = clear_mica(frame);
            let _ = clear_acrylic(frame);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = frame;
}

/// The macOS blur behind the window. window-vibrancy's apply_vibrancy adds its
/// NSVisualEffectView as a subview of winit's content view; that view draws through its own
/// (Metal) layer, and subviews always composite above their superview's layer, so the blur
/// covered the whole window (a plain grey window with Translucent sidebars on). Here the effect
/// view is a sibling placed BELOW the content view in the window's frame view, so it shows only
/// where the content is transparent.
#[cfg(target_os = "macos")]
mod mac_vibrancy {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSColor, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindowOrderingMode};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    thread_local! {
        static FX: std::cell::RefCell<Option<Retained<NSVisualEffectView>>> = const { std::cell::RefCell::new(None) };
    }

    pub fn set(frame: &eframe::Frame, on: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        FX.with_borrow_mut(|fx| {
            if let Some(v) = fx.take() {
                v.removeFromSuperview();
            }
            let Ok(h) = frame.window_handle() else { return };
            let RawWindowHandle::AppKit(h) = h.as_raw() else { return };
            // SAFETY: winit's NSView, alive as long as the window; we're on the main thread
            let view: &NSView = unsafe { h.ns_view.cast().as_ref() };
            // the window is transparent on macOS (so the blur can show); with the system title
            // bar and the blur off, give it the normal window background again, or the title
            // bar strip shows what's behind the window (N27)
            if crate::winframe::system_title_bar() {
                if let Some(w) = view.window() {
                    w.setBackgroundColor(Some(&*if on { NSColor::clearColor() } else { NSColor::windowBackgroundColor() }));
                }
            }
            if !on {
                return;
            }
            let Some(frame_view) = (unsafe { view.superview() }) else { return };
            let v = NSVisualEffectView::initWithFrame(mtm.alloc(), frame_view.bounds());
            v.setMaterial(NSVisualEffectMaterial::Sidebar);
            v.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            v.setState(NSVisualEffectState::FollowsWindowActiveState);
            v.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
            frame_view.addSubview_positioned_relativeTo(&v, NSWindowOrderingMode::Below, Some(view));
            *fx = Some(v);
        });
    }
}

/// `passes` box blurs of `radius` pixels, across then down (close to a Gaussian).
pub fn box_blur(img: &egui::ColorImage, radius: usize, passes: usize) -> egui::ColorImage {
    let [w, h] = img.size;
    let mut px: Vec<[f32; 4]> = img.pixels.iter().map(|c| c.to_array().map(f32::from)).collect();
    let mut tmp = px.clone();
    for _ in 0..passes {
        for (horizontal, len, lines) in [(true, w, h), (false, h, w)] {
            for line in 0..lines {
                let at = |i: usize| if horizontal { line * w + i } else { i * w + line };
                for i in 0..len {
                    let (a, b) = (i.saturating_sub(radius), (i + radius).min(len - 1));
                    let mut sum = [0.0f32; 4];
                    for j in a..=b {
                        let p = px[at(j)];
                        for k in 0..4 {
                            sum[k] += p[k];
                        }
                    }
                    let n = (b - a + 1) as f32;
                    tmp[at(i)] = sum.map(|v| v / n);
                }
            }
            std::mem::swap(&mut px, &mut tmp);
        }
    }
    egui::ColorImage::new([w, h], px.iter().map(|p| egui::Color32::from_rgba_premultiplied(p[0] as u8, p[1] as u8, p[2] as u8, p[3] as u8)).collect())
}
