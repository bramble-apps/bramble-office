//! Notes' look: light or dark, the colour theme, liquid glass, translucent panes, the classic
//! look, the window's title bar and the toolbar's background (Letters' look.rs). Each app of the
//! suite keeps its own (Notes' in ~/.config/bramble-notes/look.txt); the first time, Notes
//! starts from Keydeck's, but light, as Apple Notes opens.
//!
//! Also the Apple Notes 4.13 colours and fonts measured on the Mac screenshots (light mode,
//! screenshots/ui-*.png): the interface is SF Pro, here its metric match Sfera (kn-render's
//! bundled fonts).

use std::path::PathBuf;

use egui::Color32;

/// Notes' yellow (settings checkboxes, the slider, the Done button: #ECB649).
pub const NOTES_YELLOW: u32 = 0xecb649;

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub color_theme: String,
    pub liquid_glass: bool,
    pub translucent_panes: bool,
    pub classic_look: bool,
    pub system_title_bar: bool,
    /// The title bar shows one menu button instead of File, Edit, … (Alt or F10 shows them).
    pub hide_menus: bool,
    pub toolbar_background: kn_ui::glass::ToolbarBackground,
}

impl Default for Look {
    fn default() -> Self {
        Look { dark: false, color_theme: "default".into(), liquid_glass: false, translucent_panes: false, classic_look: false, system_title_bar: cfg!(target_os = "macos"), toolbar_background: kn_ui::glass::ToolbarBackground::Tinted, hide_menus: false }
    }
}

fn config(app: &str, file: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app).join(file))
}

/// Notes' own look file.
pub fn path() -> Option<PathBuf> {
    config("bramble-notes", "look.txt")
}

impl Look {
    /// Notes' look; before it has one of its own, Keydeck's glass and title bar settings (but
    /// light and Notes' yellow, Notes' defaults).
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
                "translucent_panes" => l.translucent_panes = on,
                "classic_look" => l.classic_look = on,
                "system_title_bar" => l.system_title_bar = on,
                "hide_menus" if !seeded => l.hide_menus = on,
                "toolbar_background" if !seeded => {
                    if let Some(b) = kn_ui::glass::ToolbarBackground::from_key(v) {
                        l.toolbar_background = b;
                    }
                }
                _ => {}
            }
        }
        l
    }

    pub fn save(&self) {
        let Some(path) = path() else { return };
        let text = format!(
            "dark={}\ncolor_theme={}\nliquid_glass={}\ntranslucent_panes={}\nclassic_look={}\nsystem_title_bar={}\ntoolbar_background={}\nhide_menus={}\n",
            self.dark, self.color_theme, self.liquid_glass, self.translucent_panes, self.classic_look, self.system_title_bar, self.toolbar_background.key(), self.hide_menus
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }

    /// Style the interface: colour theme, appearance, glass and Notes' fonts.
    pub fn apply(&self, ctx: &egui::Context) {
        kn_ui::theme::set_default_accent(NOTES_YELLOW);
        kn_ui::theme::set_color_theme(&self.color_theme);
        kn_ui::theme::apply_appearance(ctx, self.dark);
        use_sfera(ctx);
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let dark = self.dark;
        ctx.all_styles_mut(|s| {
            s.visuals.window_shadow = egui::Shadow { offset: [0, 10], blur: 36, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 50 }) };
            s.visuals.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if dark { 90 } else { 36 }) };
            // Notes' text is 13 pt SF Pro
            for (style, font) in s.text_styles.iter_mut() {
                if *style == egui::TextStyle::Body || *style == egui::TextStyle::Button {
                    font.size = 13.0;
                }
            }
            // selection in text fields: the accent, lighter
            s.visuals.selection.bg_fill = if dark { Color32::from_rgb(0x6b, 0x55, 0x22) } else { Color32::from_rgb(0xf7, 0xe0, 0xa8) };
            s.visuals.selection.stroke.color = if dark { Color32::WHITE } else { Color32::BLACK };
            // scroll bars only while scrolling (macOS's overlay scrollers)
            s.spacing.scroll = egui::style::ScrollStyle::floating();
            s.spacing.scroll.dormant_background_opacity = 0.0;
            s.spacing.scroll.dormant_handle_opacity = 0.0;
            s.spacing.scroll.active_background_opacity = 0.0;
            s.spacing.scroll.active_handle_opacity = 0.0;
            // menus: the hovered item in the accent with dark text, as Notes' (ctx-*.png: #F2BB4B)
            s.visuals.widgets.hovered.weak_bg_fill = kn_ui::theme::ACCENT();
            s.visuals.widgets.hovered.bg_fill = kn_ui::theme::ACCENT();
            s.visuals.widgets.hovered.fg_stroke.color = on_accent();
            s.visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
        });
        kn_ui::glass::set_enabled(self.liquid_glass);
        kn_ui::glass::set_translucent_panes(self.translucent_panes);
        kn_ui::glass::set_classic(self.classic_look);
        kn_ui::glass::set_toolbar_background(self.toolbar_background);
    }
}

/// SF Pro's metric match (Sfera, bundled with kn-render) first in the interface's families:
/// "bold" (Bold) for titles and headings, "semibold" for the sidebar's and list's names.
fn use_sfera(ctx: &egui::Context) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    static REGULAR: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Regular.ttf");
    static BOLD: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Bold.ttf");
    static SEMIBOLD: &[u8] = include_bytes!("../../../../keynote-linux/crates/kn-render/fonts/matched/sfera/Sfera-Semibold.ttf");
    let at = |family: egui::FontFamily| InsertFontFamily { family, priority: FontPriority::Highest };
    // A new name each time: switching light / dark makes kn-ui queue a fresh font set, which
    // egui applies before these; under the same names add_font would skip them as already
    // installed, and the interface would fall back to the suite's font (other metrics, other
    // spacing) after the switch.
    static GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let g = GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = |n: &str| format!("{n}-{g}");
    ctx.add_font(FontInsert::new(&name("sfera"), egui::FontData::from_static(REGULAR), vec![at(egui::FontFamily::Proportional)]));
    ctx.add_font(FontInsert::new(&name("sfera-bold"), egui::FontData::from_static(BOLD), vec![at(egui::FontFamily::Name("bold".into()))]));
    ctx.add_font(FontInsert::new(&name("sfera-semibold"), egui::FontData::from_static(SEMIBOLD), vec![at(egui::FontFamily::Name("semibold".into()))]));
    // colour emoji: kn-ui puts them right after the text font in the regular and bold families
    // (ahead of the monochrome symbol fonts); "semibold" is Notes' own, so here, before its
    // symbol fallback below. Music symbols (Notes' "22 Emoji": 𝄞) at the end of all three.
    if let Some(bytes) = ["/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf", "/usr/share/fonts/noto/NotoColorEmoji.ttf", "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf", "/usr/share/fonts/noto-color-emoji/NotoColorEmoji.ttf"].iter().find_map(|p| std::fs::read(p).ok()) {
        ctx.add_font(FontInsert::new(&name("ui-emoji"), egui::FontData::from_owned(bytes), vec![InsertFontFamily { family: egui::FontFamily::Name("semibold".into()), priority: FontPriority::Lowest }]));
    }
    let fams = || ["bold", "semibold"].iter().map(|f| InsertFontFamily { family: egui::FontFamily::Name((*f).into()), priority: FontPriority::Lowest }).chain(std::iter::once(InsertFontFamily { family: egui::FontFamily::Proportional, priority: FontPriority::Lowest })).collect::<Vec<_>>();
    if let Some(bytes) = ["/usr/share/fonts/truetype/noto/NotoMusic-Regular.ttf", "/usr/share/fonts/noto/NotoMusic-Regular.ttf", "/usr/share/fonts/google-noto/NotoMusic-Regular.ttf"].iter().find_map(|p| std::fs::read(p).ok()) {
        ctx.add_font(FontInsert::new(&name("ui-music"), egui::FontData::from_owned(bytes), fams()));
    }
    // (semibold falls back like bold: symbols from the proportional list)
    ctx.add_font(FontInsert::new(&name("sfera-semibold-sym"), egui::FontData::from_static(REGULAR), vec![InsertFontFamily { family: egui::FontFamily::Name("semibold".into()), priority: FontPriority::Lowest }]));
}

pub fn bold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("bold".into()))
}

pub fn semibold(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("semibold".into()))
}

pub fn regular(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

fn pick(light: u32, dark: u32) -> Color32 {
    kn_ui::theme::rgb(if kn_ui::theme::is_light() { light } else { dark })
}

/// The note list's and editor's background (white; Notes dark: #1e1e1e).
pub fn content_bg() -> Color32 {
    pick(0xffffff, 0x1e1e1e)
}

/// The sidebar's background (Notes 4.13: a floating, slightly translucent grey pane).
pub fn sidebar_bg() -> Color32 {
    kn_ui::theme::tint(pick(0xededec, 0x2a2a2a))
}

/// The title bar row and the toolbar's band: Letters' header colour (the suite's), so the
/// toolbar's capsules read as they do there.
pub fn header_bg() -> Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::TOOLBAR_BG() } else { kn_ui::theme::tint(Color32::from_gray(0x1e)) }
}

/// Main text (#272727 measured; dark: #e6e6e6).
pub fn text() -> Color32 {
    pick(0x272727, 0xe6e6e6)
}

/// Secondary text: snippets, counts (#808080; dark: #8c8c8c).
pub fn text_dim() -> Color32 {
    pick(0x808080, 0x8c8c8c)
}

/// The editor's date line (#999999).
pub fn text_faint() -> Color32 {
    pick(0x999999, 0x7a7a7a)
}

/// Folder name in the list header (#4c4c4c).
pub fn header_text() -> Color32 {
    pick(0x4c4c4c, 0xd8d8d8)
}

/// Hairlines: between rows, between the list and the editor (#e6e6e6).
pub fn line() -> Color32 {
    pick(0xe6e6e6, 0x383838)
}

/// The accent: the colour theme's (Notes' yellow by default).
pub fn accent() -> Color32 {
    kn_ui::theme::ACCENT()
}

/// A selected note in the list while the list has focus (#FAE49B), else grey.
pub fn selection(focused: bool) -> Color32 {
    if focused && kn_ui::theme::is_light() && accent() == kn_ui::theme::rgb(NOTES_YELLOW) {
        // (measured: #FAE49B)
        kn_ui::theme::rgb(0xfae49b)
    } else if focused {
        let a = accent();
        if kn_ui::theme::is_light() { mix(a, Color32::WHITE, 0.45) } else { mix(a, Color32::from_gray(0x1e), 0.55) }
    } else {
        pick(0xdcdcdc, 0x3a3a3a)
    }
}

/// The sidebar's folder icons (#F4B122) and chevrons: the theme's colour when one is picked.
pub fn folder_icon() -> Color32 {
    if kn_ui::theme::default_accent() == NOTES_YELLOW && kn_ui::theme::ACCENT() == kn_ui::theme::rgb(NOTES_YELLOW) { kn_ui::theme::rgb(0xf4b122) } else { accent() }
}

/// The sidebar's counts (#A19F9B) and section headers (#878786).
pub fn count_text() -> Color32 {
    pick(0xa19f9b, 0x8a8a8a)
}

pub fn section_text() -> Color32 {
    pick(0x878786, 0x8e8e8e)
}

/// The sidebar's selected row.
pub fn sidebar_selection(focused: bool) -> Color32 {
    if focused { pick(0xd7d7d6, 0x454545) } else { pick(0xdcdcdc, 0x3a3a3a) }
}

/// A toolbar capsule's fill (Letters' look::capsule_fill: the suite's).
pub fn capsule_fill() -> Color32 {
    kn_ui::theme::tint(pick(0xf4f4f4, 0x3c3c3c))
}

pub fn capsule_rim() -> Color32 {
    if kn_ui::theme::is_light() { Color32::from_black_alpha(30) } else { Color32::from_white_alpha(28) }
}

/// Toolbar glyphs (black; greyed #ADADAD, measured).
pub fn icon(enabled: bool) -> Color32 {
    match (kn_ui::theme::is_light(), enabled) {
        (true, true) => Color32::BLACK,
        (true, false) => Color32::from_gray(0xad),
        (false, true) => Color32::from_gray(0xe8),
        (false, false) => Color32::from_gray(0x6a),
    }
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Text on an accent-coloured button (Notes' yellow takes black text).
pub fn on_accent() -> Color32 {
    let a = accent();
    let lum = 0.2126 * a.r() as f32 + 0.7152 * a.g() as f32 + 0.0722 * a.b() as f32;
    if lum > 150.0 { Color32::BLACK } else { Color32::WHITE }
}
