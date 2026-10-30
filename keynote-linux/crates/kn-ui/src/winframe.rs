//! The window's own frame when its menus sit in the title bar: on desktops that leave window
//! frames to the app (GNOME on Wayland) the window gets a soft shadow, rounded corners and a
//! hairline outline the way GNOME's own windows have them, and the window buttons follow the
//! desktop's button layout. Elsewhere "System title bar" (Settings > Window) gives the
//! system's own frame.

use egui::{Color32, CornerRadius, Rect, Stroke, StrokeKind};

/// Transparent room around the window for its shadow (logical pixels), as GTK windows have.
pub const SHADOW_MARGIN: u32 = 32;

/// Corner radius of GNOME (libadwaita) windows.
pub const RADIUS: f32 = 12.0;

/// The frame chosen when the window was made (Settings > Window takes effect at the next start).
pub static SYSTEM_TITLE_BAR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Whether this window uses the system's own title bar.
pub fn system_title_bar() -> bool {
    SYSTEM_TITLE_BAR.get().copied().unwrap_or(false)
}

/// Desktops that leave window frames to the app: their "system" title bar is only a basic one.
pub fn desktop_draws_no_frames() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("GNOME"))
}

/// Ask for the shadow margin (before the window is made). Only Wayland windows can have one.
pub fn request_margin() {
    #[cfg(target_os = "linux")]
    winit::platform::wayland::set_client_shadow_margin(SHADOW_MARGIN);
}

/// The margin in effect now: 0 when maximized, tiled, fullscreen or on another system.
pub fn margin() -> f32 {
    #[cfg(target_os = "linux")]
    return winit::platform::wayland::client_shadow_margin_active() as f32;
    #[cfg(not(target_os = "linux"))]
    0.0
}

/// One CSS box-shadow layer: y offset, blur radius, spread (logical px) and opacity.
type Layer = (f32, f32, f32, f32);

/// libadwaita's window shadows (GNOME's own windows), focused and in the background.
const FOCUSED: &[Layer] = &[(2.0, 8.0, 2.0, 0.13), (3.0, 20.0, 10.0, 0.09)];
const BACKDROP: &[Layer] = &[(2.0, 6.0, 2.0, 0.10), (2.0, 14.0, 4.0, 0.05)];

/// Complementary error function (Abramowitz & Stegun 7.1.26, plenty for a shadow).
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let r = t * (-z * z - 1.265_512_2 + t * (1.000_023_7 + t * (0.374_091_96 + t * (0.096_784_18 + t * (-0.186_288_06 + t * (0.278_868_07 + t * (-1.135_203_9 + t * (1.488_515_9 + t * (-0.822_152_23 + t * 0.170_872_77))))))))).exp();
    if x >= 0.0 { r } else { 2.0 - r }
}

/// Signed distance from `p` to a rounded rectangle (centre, half size, radius).
fn sd_round_rect(p: (f32, f32), c: (f32, f32), half: f32, r: f32) -> f32 {
    let (qx, qy) = ((p.0 - c.0).abs() - half + r, (p.1 - c.1).abs() - half + r);
    (qx.max(0.0).hypot(qy.max(0.0))) + qx.max(qy).min(0.0) - r
}

/// The shadow as a nine-slice picture: a window (rounded square) with its shadow layers
/// around it, blurred like CSS box-shadows and cut away under the window itself (as GTK
/// draws them), in device pixels.
fn shadow_image(layers: &[Layer], k: f32, ppp: f32) -> (egui::ColorImage, usize) {
    let (m, r) = (SHADOW_MARGIN as f32 * ppp, RADIUS * ppp);
    let tile = (m + r).ceil() as usize;
    let size = tile * 2 + 2;
    let c = (size as f32 / 2.0, size as f32 / 2.0);
    let half = r + 1.0;
    let mut px = vec![Color32::TRANSPARENT; size * size];
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let cover = sd_round_rect(p, c, half, r).clamp(-0.5, 0.5) + 0.5;
            if cover <= 0.0 {
                continue;
            }
            let mut keep = 1.0;
            for &(dy, blur, spread, a) in layers {
                let sigma = (blur * ppp / 2.0).max(0.5);
                let d = sd_round_rect((p.0, p.1 - dy * ppp), c, half + spread * ppp, r + spread * ppp);
                keep *= 1.0 - (a * k).min(1.0) * 0.5 * erfc(d / (sigma * std::f32::consts::SQRT_2));
            }
            px[y * size + x] = Color32::from_black_alpha(((1.0 - keep) * cover * 255.0).round() as u8);
        }
    }
    (egui::ColorImage { size: [size, size], pixels: px, source_size: egui::vec2(size as f32, size as f32) }, tile)
}

/// GNOME's window shadow around `inner`: lighter when the window is in the background,
/// stronger on a dark desktop.
pub fn paint_shadow(ctx: &egui::Context, inner: Rect, _radius: f32, focused: bool, dark: bool) {
    thread_local!(static CACHE: std::cell::RefCell<Vec<((bool, bool, u32), egui::TextureHandle, usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) });
    let ppp = ctx.pixels_per_point();
    let key = (focused, dark, (ppp * 100.0) as u32);
    let (tex, tile, size) = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(e) = c.iter().find(|e| e.0 == key) {
            return (e.1.clone(), e.2, e.3);
        }
        let (img, tile) = shadow_image(if focused { FOCUSED } else { BACKDROP }, if dark { 1.8 } else { 1.0 }, ppp);
        let size = img.size[0];
        let t = ctx.load_texture("window-shadow", img, egui::TextureOptions::LINEAR);
        c.push((key, t.clone(), tile, size));
        (t, tile, size)
    });
    // nine slices (the middle stays empty): corners as drawn, edges stretched
    let t = tile as f32 / ppp;
    let outer = inner.expand(SHADOW_MARGIN as f32);
    let xs = [outer.min.x, outer.min.x + t, outer.max.x - t, outer.max.x];
    let ys = [outer.min.y, outer.min.y + t, outer.max.y - t, outer.max.y];
    let u = [0.0, tile as f32 / size as f32, (size - tile) as f32 / size as f32, 1.0];
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for j in 0..3 {
        for i in 0..3 {
            if i == 1 && j == 1 {
                continue;
            }
            let r = Rect::from_min_max(egui::pos2(xs[i], ys[j]), egui::pos2(xs[i + 1], ys[j + 1]));
            let uv = Rect::from_min_max(egui::pos2(u[i], u[j]), egui::pos2(u[i + 1], u[j + 1]));
            mesh.add_rect_with_uv(r, uv, Color32::WHITE);
        }
    }
    ctx.layer_painter(egui::LayerId::background()).add(mesh);
}

/// The hairline around the window (and, on dark windows, a faint light inner edge).
pub fn paint_outline(ctx: &egui::Context, inner: Rect, radius: f32, dark: bool) {
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("window-outline")));
    let cr = CornerRadius::same(radius.round() as u8);
    p.rect_stroke(inner, cr, Stroke::new(1.0, Color32::from_black_alpha(if dark { 190 } else { 60 })), StrokeKind::Outside);
    if dark {
        p.rect_stroke(inner, cr, Stroke::new(1.0, Color32::from_white_alpha(14)), StrokeKind::Inside);
    }
}

/// The window buttons the desktop shows, on each side ("close", "minimize", "maximize").
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonLayout {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl Default for ButtonLayout {
    fn default() -> Self {
        #[cfg(target_os = "macos")]
        return ButtonLayout { left: vec!["close".into(), "minimize".into(), "maximize".into()], right: vec![] };
        #[cfg(not(target_os = "macos"))]
        ButtonLayout { left: vec![], right: vec!["minimize".into(), "maximize".into(), "close".into()] }
    }
}

impl ButtonLayout {
    /// GNOME's (and most GTK desktops') setting, e.g. "appmenu:minimize,maximize,close".
    pub fn parse(s: &str) -> Option<ButtonLayout> {
        let s = s.trim().trim_matches('\'');
        let (l, r) = s.split_once(':')?;
        let keep = |part: &str| part.split(',').map(str::trim).filter(|b| matches!(*b, "close" | "minimize" | "maximize")).map(String::from).collect::<Vec<_>>();
        Some(ButtonLayout { left: keep(l), right: keep(r) })
    }

    /// Read from the desktop (falls back to minimize, maximize, close on the right).
    pub fn from_desktop() -> ButtonLayout {
        #[cfg(target_os = "linux")]
        {
            let out = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
                .stdin(std::process::Stdio::null())
                .output();
            if let Some(b) = out.ok().filter(|o| o.status.success()).and_then(|o| ButtonLayout::parse(&String::from_utf8_lossy(&o.stdout))) {
                return b;
            }
        }
        ButtonLayout::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gnome_button_layouts() {
        let b = ButtonLayout::parse("'appmenu:minimize,maximize,close'").unwrap();
        assert_eq!((b.left.len(), b.right), (0, vec!["minimize".to_string(), "maximize".into(), "close".into()]));
        let b = ButtonLayout::parse("close,minimize,maximize:").unwrap();
        assert_eq!((b.left.len(), b.right.len()), (3, 0));
        let b = ButtonLayout::parse("icon:close").unwrap();
        assert_eq!(b.right, vec!["close".to_string()]);
        assert!(ButtonLayout::parse("nonsense").is_none());
    }
}
