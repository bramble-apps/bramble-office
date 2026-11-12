//! Studio's look: light or dark (Logic is dark), the colour theme, liquid glass, the window's
//! title bar. Each app of the suite keeps its own (Studio's in ~/.config/bramble-studio/look.txt);
//! the first time, it starts from Keydeck's glass and title-bar settings, but dark.
//! Colours measured on Logic Pro 13's screenshots (ui-tracks-with-pianoroll.png, 2x).

use std::path::PathBuf;

use egui::{Color32, FontFamily, FontId};

/// Logic's blue (selected tab, Browser buttons): #0A64D2-ish.
pub const STUDIO_BLUE: u32 = 0x1f6fdc;

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub color_theme: String,
    pub liquid_glass: bool,
    pub system_title_bar: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look { dark: true, color_theme: "default".into(), liquid_glass: false, system_title_bar: cfg!(target_os = "macos") }
    }
}

fn config(app: &str, file: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app).join(file))
}

pub fn path() -> Option<PathBuf> {
    config("bramble-studio", "look.txt")
}

impl Look {
    pub fn load() -> Look {
        let mut l = Look::default();
        let own = path().and_then(|p| std::fs::read_to_string(p).ok());
        let seeded = own.is_none();
        let Some(text) = own.or_else(|| config("keydeck", "settings.txt").and_then(|p| std::fs::read_to_string(p).ok())) else { return l };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let on = v == "true";
            match k {
                "dark" if !seeded => l.dark = on,
                "color_theme" if !seeded => l.color_theme = v.to_string(),
                "liquid_glass" => l.liquid_glass = on,
                "system_title_bar" => l.system_title_bar = on,
                _ => {}
            }
        }
        l
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let text = format!("dark={}\ncolor_theme={}\nliquid_glass={}\nsystem_title_bar={}\n", self.dark, self.color_theme, self.liquid_glass, self.system_title_bar);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }

    pub fn apply(&self, ctx: &egui::Context) {
        kn_ui::theme::set_default_accent(STUDIO_BLUE);
        kn_ui::theme::set_color_theme(&self.color_theme);
        kn_ui::theme::apply_appearance(ctx, self.dark);
        use_sfera(ctx);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let dark = self.dark;
        ctx.all_styles_mut(|s| {
            s.visuals.window_shadow = egui::Shadow { offset: [0, 10], blur: 36, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 50 }) };
            s.visuals.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if dark { 90 } else { 36 }) };
            for (style, font) in s.text_styles.iter_mut() {
                if *style == egui::TextStyle::Body || *style == egui::TextStyle::Button {
                    font.size = 13.0;
                }
            }
            s.spacing.scroll = egui::style::ScrollStyle::floating();
        });
        kn_ui::glass::set_enabled(self.liquid_glass);
    }
}

/// The interface's text fonts and the Score's music font (also outlined into printed PDFs).
pub static REGULAR: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Regular.ttf");
pub static BOLD: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Bold.ttf");
pub static BRAVURA: &[u8] = include_bytes!("../fonts/Bravura.otf");

/// SF Pro's metric match (Sfera, bundled with kn-render) first in the interface's families.
fn use_sfera(ctx: &egui::Context) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    static SEMIBOLD: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Semibold.ttf");
    let at = |family: FontFamily| InsertFontFamily { family, priority: FontPriority::Highest };
    static GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let g = GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ctx.add_font(FontInsert::new(&format!("lg-sfera-{g}"), egui::FontData::from_static(REGULAR), vec![at(FontFamily::Proportional)]));
    ctx.add_font(FontInsert::new(&format!("lg-sfera-bold-{g}"), egui::FontData::from_static(BOLD), vec![at(FontFamily::Name("bold".into()))]));
    ctx.add_font(FontInsert::new(&format!("lg-sfera-semibold-{g}"), egui::FontData::from_static(SEMIBOLD), vec![at(FontFamily::Name("semibold".into()))]));
    // Bravura (SIL OFL 1.1, Steinberg): the Score's music symbols, by their SMuFL code points
    ctx.add_font(FontInsert::new(&format!("lg-bravura-{g}"), egui::FontData::from_static(BRAVURA), vec![at(FontFamily::Name("bravura".into()))]));
    // the icons (Phosphor + Tabler, MIT): glyphs.rs
    crate::glyphs::add(ctx, g);
}

/// The music font (Bravura) at `size`: SMuFL's em is the staff's height, four staff spaces.
pub fn music(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bravura".into()))
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

fn dark() -> bool {
    !kn_ui::theme::is_light()
}

fn pick(d: u32, l: u32) -> Color32 {
    kn_ui::theme::rgb(if dark() { d } else { l })
}

/// Logic's track-header pills (M|S, R|I) and the volume groove: a tint over the row, so they
/// show on selected and plain rows alike.
pub fn header_pill() -> Color32 {
    if dark() { Color32::from_white_alpha(20) } else { Color32::from_black_alpha(18) }
}
pub fn header_groove() -> Color32 {
    if dark() { Color32::from_black_alpha(95) } else { Color32::from_black_alpha(30) }
}

/// The window's background (behind the panes).
pub fn window_bg() -> Color32 {
    pick(0x1c1c1e, 0xe9e9eb)
}
/// The control bar's band.
pub fn bar_bg() -> Color32 {
    pick(0x2a2a2c, 0xdedee0)
}
/// A group of control bar buttons (rounded capsule behind them).
pub fn group_bg() -> Color32 {
    pick(0x3a3a3c, 0xc8c8cc)
}
pub fn button_on() -> Color32 {
    pick(0x58585c, 0xb0b0b6)
}
/// The LCD's black display.
pub fn lcd_bg() -> Color32 {
    pick(0x111112, 0x1a1a1c)
}
pub fn lcd_text() -> Color32 {
    Color32::from_rgb(0xe4, 0xe4, 0xe6)
}
pub fn lcd_dim() -> Color32 {
    Color32::from_rgb(0x8a, 0x8a, 0x8e)
}
/// Panes (Browser, Inspector, editors).
pub fn pane_bg() -> Color32 {
    pick(0x232325, 0xf2f2f4)
}
pub fn header_bg() -> Color32 {
    pick(0x2c2c2e, 0xe2e2e4)
}
/// The tracks area's lanes.
pub fn lane_bg() -> Color32 {
    pick(0x1e1e20, 0xf4f4f6)
}
pub fn lane_alt() -> Color32 {
    pick(0x242426, 0xeaeaec)
}
pub fn track_header_bg() -> Color32 {
    pick(0x2a2a2c, 0xe4e4e6)
}
pub fn track_selected() -> Color32 {
    pick(0x3d3d40, 0xcfd6e6)
}
pub fn line() -> Color32 {
    pick(0x3a3a3c, 0xc4c4c8)
}
pub fn grid_line() -> Color32 {
    pick(0x303033, 0xd8d8dc)
}
pub fn text() -> Color32 {
    kn_ui::theme::TEXT()
}
pub fn text_dim() -> Color32 {
    kn_ui::theme::TEXT_DIM()
}
pub fn accent() -> Color32 {
    kn_ui::theme::ACCENT()
}
pub fn playhead() -> Color32 {
    pick(0xe8e8ea, 0x202022)
}
/// Cycle area (Logic's yellow).
pub fn cycle() -> Color32 {
    Color32::from_rgb(0xd6, 0xb1, 0x2a)
}
pub fn record_red() -> Color32 {
    Color32::from_rgb(0xe8, 0x3b, 0x34)
}

/// A track's region colour by kind (Logic: software instruments green, audio blue, Drummer
/// yellow), and the darker / lighter tones regions are drawn in.
pub fn region_color(kind: lg_core::TrackKind) -> Color32 {
    match kind {
        lg_core::TrackKind::Instrument => Color32::from_rgb(0x52, 0xc0, 0x3c),
        lg_core::TrackKind::Audio => Color32::from_rgb(0x3d, 0x8f, 0xe0),
        lg_core::TrackKind::Drummer => Color32::from_rgb(0xe3, 0xb3, 0x2b),
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// A control bar capsule's fill in dark mode (the suite's dark rimmed capsules).
pub fn capsule_fill() -> Color32 {
    kn_ui::theme::tint(pick(0x3a3a3c, 0xf4f4f4))
}
