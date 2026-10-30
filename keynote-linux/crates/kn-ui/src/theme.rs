//! Keynote's dark appearance (colours sampled from the reference screenshots).

use egui::{Color32, CornerRadius, Stroke, Visuals};

static LIGHT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Light appearance (Keynote's light mode) instead of dark.
pub fn is_light() -> bool {
    LIGHT.load(std::sync::atomic::Ordering::Relaxed)
}

fn pick(dark: Color32, light: Color32) -> Color32 {
    tint(if is_light() { light } else { dark })
}

/// The colour theme's tint of a neutral grey (as ChordPro Presenter's Material themes derive
/// every surface from the theme colour): the same lightness in the theme's hue. Coloured
/// things and the Default theme are left as they are.
pub fn tint(c: Color32) -> Color32 {
    let theme = ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed);
    if theme == DEFAULT_RGB.load(std::sync::atomic::Ordering::Relaxed) || c.a() == 0 {
        return c;
    }
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if r.max(g).max(b) - r.min(g).min(b) > 10 {
        return c;
    }
    let (h, ts, _) = hsl(rgb(theme));
    // (white surfaces take a faint tint too, as Material's lightest surface does)
    let l = ((f32::from(r) + f32::from(g) + f32::from(b)) / 765.0).min(0.975);
    // light surfaces a gentle wash, dark ones a deeper tint; pure white / black barely move
    let s = ts.min(1.0) * if l > 0.5 { 0.42 } else { 0.24 };
    let out = from_hsl(h, s, l);
    Color32::from_rgba_unmultiplied(out.r(), out.g(), out.b(), a)
}

fn hsl(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (f32::from(c.r()) / 255.0, f32::from(c.g()) / 255.0, f32::from(c.b()) / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let l = (mx + mn) / 2.0;
    if mx - mn < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = mx - mn;
    let s = if l > 0.5 { d / (2.0 - mx - mn) } else { d / (mx + mn) };
    let h = if mx == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } * 60.0;
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Color32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(to(r), to(g), to(b))
}

// The interface palette (Keynote 15.4 dark and light appearances).
#[allow(non_snake_case)]
pub fn CANVAS_BG() -> Color32 {
    pick(Color32::from_rgb(0x15, 0x15, 0x15), Color32::from_rgb(0xeb, 0xeb, 0xeb))
}
#[allow(non_snake_case)]
pub fn PANEL_BG() -> Color32 {
    pick(Color32::from_rgb(0x23, 0x23, 0x23), Color32::from_rgb(0xe8, 0xe8, 0xe8))
}
#[allow(non_snake_case)]
pub fn SIDEBAR_BG() -> Color32 {
    pick(Color32::from_rgb(0x22, 0x22, 0x22), Color32::from_rgb(0xe8, 0xe8, 0xe8))
}
#[allow(non_snake_case)]
pub fn TOOLBAR_BG() -> Color32 {
    pick(Color32::from_rgb(0x2b, 0x2b, 0x2b), Color32::from_rgb(0xf0, 0xf0, 0xf0))
}
#[allow(non_snake_case)]
pub fn CONTROL_BG() -> Color32 {
    pick(Color32::from_rgb(0x3a, 0x3a, 0x3c), Color32::WHITE)
}
/// Colour themes (the same as ChordPro Presenter's): id, name, main colour, accent. Keydeck's
/// highlight colour (selections, active buttons, switches) is the main colour; "Default" is
/// Keynote's blue.
pub const COLOR_THEMES: [(&str, &str, u32, u32); 13] = [
    ("default", "Default", 0x3a82f7, 0x3a82f7),
    ("indigo", "Indigo", 0x3F51B5, 0x00BFA5),
    ("ocean", "Ocean", 0x0277BD, 0x26C6DA),
    ("forest", "Forest", 0x2E7D32, 0xFFB300),
    ("sunset", "Sunset", 0xEF6C00, 0xEC407A),
    ("rose", "Rose", 0xC2185B, 0x7E57C2),
    ("lavender", "Lavender", 0x7E57C2, 0x26A69A),
    ("crimson", "Crimson", 0xC62828, 0xFFC107),
    ("teal", "Teal", 0x00897B, 0xFF7043),
    ("sand", "Sand", 0x8D6E63, 0x26A69A),
    ("slate", "Slate", 0x546E7A, 0xFF8F00),
    ("midnight", "Midnight", 0x1A237E, 0x00E5FF),
    ("graphite", "Graphite", 0x616161, 0x9E9E9E),
];

static ACCENT_RGB: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3a82f7);
/// The "Default" theme's colour: Keydeck's blue unless the app picks its own.
static DEFAULT_RGB: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3a82f7);

/// Give the Default theme the app's own colour (call before set_color_theme).
pub fn set_default_accent(c: u32) {
    DEFAULT_RGB.store(c, std::sync::atomic::Ordering::Relaxed);
}

/// The Default theme's colour.
pub fn default_accent() -> u32 {
    DEFAULT_RGB.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn rgb(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// Use colour theme `id` (unknown ids: Default).
pub fn set_color_theme(id: &str) {
    let c = COLOR_THEMES.iter().skip(1).find(|t| t.0 == id).map(|t| t.2).unwrap_or_else(default_accent);
    ACCENT_RGB.store(c, std::sync::atomic::Ordering::Relaxed);
}

/// The highlight colour: the theme's main colour, lifted in dark mode when it's too deep to
/// stand out on the dark panels (as ChordPro Presenter's dark themes do).
#[allow(non_snake_case)]
pub fn ACCENT() -> Color32 {
    let c = rgb(ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed));
    if is_light() {
        return c;
    }
    let lum = 0.2126 * f32::from(c.r()) + 0.7152 * f32::from(c.g()) + 0.0722 * f32::from(c.b());
    if lum >= 90.0 {
        return c;
    }
    let t = ((90.0 - lum) / 90.0 * 0.55).clamp(0.0, 0.5);
    let lift = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * t) as u8;
    Color32::from_rgb(lift(c.r()), lift(c.g()), lift(c.b()))
}
#[allow(non_snake_case)]
pub fn TEXT() -> Color32 {
    pick(Color32::from_rgb(0xe8, 0xe8, 0xea), Color32::from_rgb(0x1d, 0x1d, 0x1f))
}
#[allow(non_snake_case)]
pub fn TEXT_DIM() -> Color32 {
    pick(Color32::from_rgb(0x9a, 0x9a, 0x9e), Color32::from_rgb(0x6e, 0x6e, 0x73))
}
#[allow(non_snake_case)]
pub fn HANDLE() -> Color32 {
    Color32::WHITE
}
#[allow(non_snake_case)]
pub fn SEPARATOR() -> Color32 {
    pick(Color32::from_rgb(0x3a, 0x3a, 0x3a), Color32::from_rgb(0xd4, 0xd4, 0xd4))
}

/// A grey of the dark design mapped for the current appearance: control fills and hovers
/// (0x2a–0x60) become light greys, text greys (0x80+) become dark ones.
pub fn gray(v: u8) -> Color32 {
    if !is_light() {
        return tint(Color32::from_gray(v));
    }
    let v = v as f32;
    let l = if v < 128.0 { 0xf4 as f32 - (v - 0x2a as f32).max(0.0) * 0.9 } else { 255.0 - v + 0x20 as f32 };
    tint(Color32::from_gray(l.clamp(0.0, 255.0) as u8))
}

/// Add installed fonts as fallbacks so toolbar symbols (▶ ＋ ⬠ ◔ ▦ …) render.
fn load_fallback_fonts(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        // macOS: broad symbol coverage (geometric shapes, arrows, technical symbols)
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "/System/Library/Fonts/Apple Symbols.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    // the interface font: Nimbus Sans (a Helvetica design, closest to macOS's look among
    // fonts with static regular + bold files), bold as its own family for headings
    let ui_font = |fonts: &mut egui::FontDefinitions, name: &str, paths: &[&str]| -> bool {
        for p in paths {
            if let Ok(bytes) = std::fs::read(p) {
                // Nimbus / Liberation Sans sit high in their line box: move them down so text is
                // centred in controls (macOS's and Windows' fonts don't need it)
                let y = if p.starts_with("/System/") || p.contains("\\Fonts\\") { 0.0 } else { 0.215 };
                let tweak = egui::FontTweak { y_offset_factor: y, ..Default::default() };
                fonts.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_owned(bytes).tweak(tweak)));
                return true;
            }
        }
        false
    };
    // Windows: Segoe UI (its interface font), else Arial, from the system's Fonts folder
    let win = std::env::var("SystemRoot").or_else(|_| std::env::var("WINDIR")).unwrap_or_else(|_| "C:\\Windows".into());
    let wf = |f: &str| format!("{win}\\Fonts\\{f}");
    let (win_regular, win_bold) = ([wf("segoeui.ttf"), wf("arial.ttf")], [wf("segoeuib.ttf"), wf("arialbd.ttf")]);
    let mut regular: Vec<&str> = vec!["/usr/share/fonts/opentype/urw-base35/NimbusSans-Regular.otf", "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf"];
    let mut bold_paths: Vec<&str> = vec!["/usr/share/fonts/opentype/urw-base35/NimbusSans-Bold.otf", "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf"];
    if cfg!(windows) {
        regular.splice(0..0, win_regular.iter().map(String::as_str));
        bold_paths.splice(0..0, win_bold.iter().map(String::as_str));
    }
    // none of them installed (a bare Windows under Wine, say): the bundled Carlito (SIL Open
    // Font License, kn-render/fonts/carlito), so the interface never falls back to egui's
    // light default
    static CARLITO: &[u8] = include_bytes!("../../kn-render/fonts/carlito/Carlito-Regular.ttf");
    static CARLITO_BOLD: &[u8] = include_bytes!("../../kn-render/fonts/carlito/Carlito-Bold.ttf");
    let bundled = |fonts: &mut egui::FontDefinitions, name: &str, bytes: &'static [u8]| {
        let tweak = egui::FontTweak { y_offset_factor: 0.1, ..Default::default() };
        fonts.font_data.insert(name.to_string(), std::sync::Arc::new(egui::FontData::from_static(bytes).tweak(tweak)));
    };
    if !ui_font(&mut fonts, "ui-regular", &regular) {
        bundled(&mut fonts, "ui-regular", CARLITO);
    }
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "ui-regular".into());
    let mut bold = Vec::new();
    if !ui_font(&mut fonts, "ui-bold", &bold_paths) {
        bundled(&mut fonts, "ui-bold", CARLITO_BOLD);
    }
    bold.push("ui-bold".to_string());
    bold.extend(fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(egui::FontFamily::Name("bold".into()), bold);
    let mut have_dejavu = false;
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        have_dejavu |= name == "DejaVuSans.ttf";
        fonts.font_data.insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace, egui::FontFamily::Name("bold".into())] {
            fonts.families.entry(fam).or_default().push(name.clone());
        }
    }
    // systems without DejaVu (macOS, Windows): the bundled copy, so every interface symbol
    // (⚙ ⌄ ▦ ◷ …) draws everywhere (Bitstream Vera licence: data/DejaVuSans-LICENSE.txt)
    if !have_dejavu {
        static BUNDLED: &[u8] = include_bytes!("../data/DejaVuSans.ttf");
        fonts.font_data.insert("DejaVuSans-bundled".into(), std::sync::Arc::new(egui::FontData::from_static(BUNDLED)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace, egui::FontFamily::Name("bold".into())] {
            fonts.families.entry(fam).or_default().push("DejaVuSans-bundled".into());
        }
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context) {
    apply_appearance(ctx, true);
}

/// Set the appearance (dark or light) and style the widgets for it.
pub fn apply_appearance(ctx: &egui::Context, dark: bool) {
    LIGHT.store(!dark, std::sync::atomic::Ordering::Relaxed);
    load_fallback_fonts(ctx);
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = PANEL_BG();
    v.window_fill = PANEL_BG();
    v.extreme_bg_color = pick(Color32::from_rgb(0x1a, 0x1a, 0x1a), Color32::WHITE);
    v.faint_bg_color = pick(Color32::from_rgb(0x2c, 0x2c, 0x2e), Color32::from_rgb(0xee, 0xee, 0xee));
    v.selection.bg_fill = ACCENT();
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT();
    v.override_text_color = Some(TEXT());
    let r = CornerRadius::same(6);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.inactive.weak_bg_fill = CONTROL_BG();
    v.widgets.inactive.bg_fill = CONTROL_BG();
    v.widgets.hovered.weak_bg_fill = pick(Color32::from_rgb(0x48, 0x48, 0x4a), Color32::from_rgb(0xe4, 0xe4, 0xe6));
    v.widgets.hovered.bg_fill = v.widgets.hovered.weak_bg_fill;
    v.widgets.noninteractive.bg_fill = PANEL_BG();
    v.widgets.noninteractive.weak_bg_fill = PANEL_BG();
    if !dark {
        // light controls: white with a hairline border, like macOS
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, tint(Color32::from_rgb(0xcc, 0xcc, 0xcc)));
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT());
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT());
    }
    v.widgets.active.weak_bg_fill = ACCENT();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR());
    v.window_corner_radius = CornerRadius::same(10);
    // every grey egui draws (window title bars, open menus, buttons, frames) in the theme's tint
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.bg_fill = tint(w.bg_fill);
        w.weak_bg_fill = tint(w.weak_bg_fill);
        w.bg_stroke.color = tint(w.bg_stroke.color);
    }
    v.window_stroke.color = tint(v.window_stroke.color);
    v.extreme_bg_color = tint(v.extreme_bg_color);
    v.faint_bg_color = tint(v.faint_bg_color);
    v.code_bg_color = tint(v.code_bg_color);
    // the app's own light / dark choice, not the system's: where the system reports a theme
    // (Windows, macOS), egui would otherwise switch to its default style for that one, which
    // has none of these colours (labels and menus came out dim on Windows)
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.set_visuals(v);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        // no fade at scroll edges (Keynote has none, and it squared off the rounded window corners)
        s.spacing.scroll.fade.strength = 0.0;
        // sliders: blue track up to a round white knob (macOS)
        s.visuals.slider_trailing_fill = true;
        s.visuals.handle_shape = egui::style::HandleShape::Circle;
        // pane slide animations: Keynote's are 200–320 ms
        s.animation_time = 0.25;
        // toolbar tooltips (Numbers, Mac N21): the first after ~1.6 s; moving on to the next
        // button soon after shows its tip at once (egui's nearest to the Mac's ~0.45 s)
        s.interaction.tooltip_delay = TOOLTIP_DELAY_S;
        s.interaction.tooltip_grace_time = TOOLTIP_NEXT_S;
    });
}

/// A side pane's slide in / out (Numbers, Mac N21: ~233–250 ms, ease-in-out, the canvas
/// pushed aside as it goes).
pub const PANE_SLIDE_S: f32 = 0.25;
/// Before the first tooltip shows (Mac N21: ~1.6 s).
pub const TOOLTIP_DELAY_S: f32 = 1.6;
/// Moving on to the next button: its tip comes sooner (Mac N21: ~0.45 s).
pub const TOOLTIP_NEXT_S: f32 = 0.45;

/// How far a pane sliding open (`open`) or shut has got: 0 shut .. 1 open, eased over
/// [`PANE_SLIDE_S`].
pub fn slide(ctx: &egui::Context, id: egui::Id, open: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(id, open, PANE_SLIDE_S, egui::emath::easing::cubic_in_out)
}

/// Bold interface text (section headings, names) — egui's `strong()` only brightens.
pub fn bold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).family(egui::FontFamily::Name("bold".into()))
}

/// macOS pop-up button: a combo box with up/down chevrons on the right.
pub fn combo<'a>(id: impl std::hash::Hash + std::fmt::Debug) -> egui::ComboBox<'a> {
    egui::ComboBox::from_id_salt(id).icon(|ui, rect, visuals, _open| {
        let c = rect.center();
        let s = egui::Stroke::new(1.4, visuals.fg_stroke.color);
        let (w, h) = (3.5, 2.5);
        ui.painter().line(vec![egui::pos2(c.x - w, c.y - 1.5), egui::pos2(c.x, c.y - 1.5 - h), egui::pos2(c.x + w, c.y - 1.5)], s);
        ui.painter().line(vec![egui::pos2(c.x - w, c.y + 1.5), egui::pos2(c.x, c.y + 1.5 + h), egui::pos2(c.x + w, c.y + 1.5)], s);
    })
}

/// macOS checkbox: blue rounded square with a white tick when on.
pub fn checkbox(ui: &mut egui::Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let text: egui::WidgetText = text.into();
    let galley = text.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Body);
    let size = egui::vec2(18.0 + 8.0 + galley.size().x, galley.size().y.max(20.0));
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    let b = egui::Rect::from_center_size(egui::pos2(rect.min.x + 9.0, rect.center().y), egui::vec2(17.0, 17.0));
    if *on {
        ui.painter().rect_filled(b, 4.0, if enabled { ACCENT() } else { Color32::from_gray(90) });
        ui.painter().line(vec![egui::pos2(b.min.x + 4.0, b.center().y), egui::pos2(b.min.x + 7.5, b.max.y - 4.5), egui::pos2(b.max.x - 4.0, b.min.y + 4.0)], Stroke::new(2.0, Color32::WHITE));
    } else {
        ui.painter().rect_filled(b, 4.0, gray(0x3a));
        ui.painter().rect_stroke(b, 4.0, Stroke::new(1.0, gray(0x5a)), egui::StrokeKind::Inside);
    }
    let col = if enabled { TEXT() } else { TEXT_DIM() };
    ui.painter().galley(egui::pos2(b.max.x + 8.0, rect.center().y - galley.size().y / 2.0), galley, col);
    resp
}


/// macOS number field with a ⌃⌄ stepper on its right. Returns true when the value changed.
pub fn stepper(ui: &mut egui::Ui, v: &mut f64, step: f64, range: std::ops::RangeInclusive<f64>, suffix: &str, decimals: usize) -> bool {
    let mut changed = false;
    // always field-then-chevrons, even inside a right-to-left row
    ui.allocate_ui_with_layout(egui::vec2(82.0, 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let suffix = suffix.to_string();
        let r = ui.add_sized(
            [64.0, 24.0],
            egui::DragValue::new(v).range(range.clone()).speed(step / 4.0).custom_formatter(move |x, _| {
                let s = if decimals == 0 || x.fract().abs() < 1e-9 { format!("{x:.0}") } else { format!("{x:.decimals$}") };
                format!("{s}{suffix}")
            }),
        );
        changed |= r.changed();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 24.0), egui::Sense::hover());
        let up = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.center().y));
        let down = egui::Rect::from_min_max(egui::pos2(rect.min.x, rect.center().y), rect.max);
        for (half, dir) in [(up, 1.0), (down, -1.0)] {
            // ids from this stepper's own field (the enclosing scope is shared by the steppers
            // of a panel, so ids from it collided and clicks went nowhere)
            let resp = ui.interact(half, r.id.with(("step", dir as i32)), egui::Sense::click());
            let c = half.center();
            let col = if resp.hovered() { Color32::WHITE } else { TEXT_DIM() };
            let dy = 2.0 * dir as f32;
            ui.painter().line(vec![egui::pos2(c.x - 4.0, c.y + dy), egui::pos2(c.x, c.y - dy), egui::pos2(c.x + 4.0, c.y + dy)], Stroke::new(1.3, col));
            if resp.clicked() {
                *v = (*v + step * dir).clamp(*range.start(), *range.end());
                changed = true;
            }
        }
    });
    changed
}

/// [`stepper`] for an f32 value.
pub fn stepper32(ui: &mut egui::Ui, v: &mut f32, step: f64, range: std::ops::RangeInclusive<f64>, suffix: &str, decimals: usize) -> bool {
    let mut x = *v as f64;
    let changed = stepper(ui, &mut x, step, range, suffix, decimals);
    if changed {
        *v = x as f32;
    }
    changed
}

/// What a section header's swatch shows (Keynote's Fill / Border / Shadow headers).
pub enum Swatch {
    Color(Color32),
    /// "none": white with a red diagonal
    None,
}

/// Keynote's collapsible inspector section: "⌄ Title" in bold, an optional swatch at the right.
/// Returns whether it is open (remembered per `id`).
pub fn disclosure(ui: &mut egui::Ui, id: &str, title: &str, swatch: Option<Swatch>) -> bool {
    let key = ui.scope_id().with(("disclosure", id));
    let mut open = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(true);
    ui.add_space(4.0);
    ui.separator();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    let p = ui.painter();
    let c = egui::pos2(rect.min.x + 6.0, rect.center().y);
    let s = Stroke::new(1.4, TEXT_DIM());
    if open {
        p.line(vec![egui::pos2(c.x - 3.5, c.y - 2.0), egui::pos2(c.x, c.y + 2.0), egui::pos2(c.x + 3.5, c.y - 2.0)], s);
    } else {
        p.line(vec![egui::pos2(c.x - 2.0, c.y - 3.5), egui::pos2(c.x + 2.0, c.y), egui::pos2(c.x - 2.0, c.y + 3.5)], s);
    }
    p.text(egui::pos2(rect.min.x + 20.0, rect.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::new(14.0, egui::FontFamily::Name("bold".into())), TEXT());
    if let Some(sw) = swatch {
        let r = egui::Rect::from_min_max(egui::pos2(rect.max.x - 92.0, rect.min.y + 3.0), egui::pos2(rect.max.x, rect.max.y - 3.0));
        match sw {
            Swatch::Color(col) => {
                p.rect_filled(r, 6.0, col);
            }
            Swatch::None => {
                p.rect_filled(r, 6.0, Color32::WHITE);
                p.line_segment([egui::pos2(r.min.x + 4.0, r.max.y - 3.0), egui::pos2(r.max.x - 4.0, r.min.y + 3.0)], Stroke::new(1.5, Color32::from_rgb(0xe0, 0x30, 0x30)));
            }
        }
    }
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(key, open));
    }
    open
}

/// macOS switch (Preview Motion and the like): a pill with a sliding knob, blue when on.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (r, mut resp) = ui.allocate_exact_size(egui::vec2(38.0, 22.0), egui::Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    // its own time: the apps make popups instant (animation_time 0) but macOS switches glide
    let k = ui.ctx().animate_bool_with_time(resp.id, *on, 0.2);
    let p = ui.painter();
    let track = if *on { ACCENT() } else { gray(0x55) };
    p.rect_filled(r, 11.0, track);
    let x = egui::lerp(r.min.x + 11.0..=r.max.x - 11.0, k);
    p.circle_filled(egui::pos2(x, r.center().y), 9.0, egui::Color32::WHITE);
    resp
}

#[cfg(test)]
mod stepper_tests {
    use super::*;

    /// Run frames with the stepper; returns the value and the arrows' rect.
    fn frame(ctx: &egui::Context, input: egui::RawInput, v: &mut f64) -> egui::Rect {
        let mut arrows = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            ui.horizontal(|ui| {
                let start = ui.cursor().min;
                stepper(ui, v, 1.0, 0.0..=100.0, "", 0);
                arrows = egui::Rect::from_min_size(egui::pos2(start.x + 66.0, start.y), egui::vec2(14.0, 24.0));
            });
        });
        out.textures_delta.clear();
        arrows
    }

    #[test]
    fn arrows_step_the_value() {
        let ctx = egui::Context::default();
        let mut v = 19.0;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let raw = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let arrows = frame(&ctx, raw(vec![]), &mut v);
        let up = egui::pos2(arrows.center().x, arrows.min.y + 5.0);
        let down = egui::pos2(arrows.center().x, arrows.max.y - 5.0);
        let click = |p: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ]
        };
        frame(&ctx, raw(vec![egui::Event::PointerMoved(up)]), &mut v);
        frame(&ctx, raw(click(up)), &mut v);
        frame(&ctx, raw(vec![]), &mut v);
        assert_eq!(v, 20.0, "up arrow");
        frame(&ctx, raw(click(down)), &mut v);
        frame(&ctx, raw(vec![]), &mut v);
        assert_eq!(v, 19.0, "down arrow");
    }

    /// Several steppers in a column, as in the inspector: each one's arrows change only its value.
    #[test]
    fn arrows_in_a_column_of_steppers() {
        let ctx = egui::Context::default();
        let mut vals = [10.0, 20.0, 30.0];
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        let raw = |events: Vec<egui::Event>| egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut rects = vec![egui::Rect::NOTHING; 3];
        let run = |events: Vec<egui::Event>, vals: &mut [f64; 3], rects: &mut Vec<egui::Rect>| {
            let mut out = ctx.run_ui(raw(events), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, v) in vals.iter_mut().enumerate() {
                        ui.label("Speed");
                        ui.horizontal(|ui| {
                            let mut x = *v;
                            let _ = ui.add_sized([ui.available_width() - 90.0, 20.0], egui::Slider::new(&mut x, 0.0..=100.0).show_value(false));
                            let r = ui.scope(|ui| stepper(ui, v, 1.0, 0.0..=100.0, "", 0)).response.rect;
                            rects[i] = egui::Rect::from_min_max(egui::pos2(r.max.x - 15.0, r.min.y), r.max);
                        });
                    }
                });
            });
            out.textures_delta.clear();
        };
        run(vec![], &mut vals, &mut rects);
        let up = egui::pos2(rects[1].center().x, rects[1].min.y + 5.0);
        let click = |p: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ]
        };
        run(vec![egui::Event::PointerMoved(up)], &mut vals, &mut rects);
        run(click(up), &mut vals, &mut rects);
        run(vec![], &mut vals, &mut rects);
        assert_eq!(vals, [10.0, 21.0, 30.0]);
    }
}

/// Text centred on `pos` by its capitals (painter.text centres the whole line box, which leaves
/// this font's letters about an eighth of the size too high in controls).
pub fn centered(p: &egui::Painter, pos: egui::Pos2, text: impl ToString, font: egui::FontId, color: egui::Color32) -> egui::Rect {
    let shift = font.size * 0.03;
    p.text(pos + egui::vec2(0.0, shift), egui::Align2::CENTER_CENTER, text, font, color)
}

#[cfg(test)]
mod accent_tests {
    use super::*;

    #[test]
    fn app_default_accent() {
        let green = 0x2daa4f;
        set_default_accent(green);
        set_color_theme("default");
        assert_eq!(ACCENT(), rgb(green));
        let grey = Color32::from_rgb(0x38, 0x38, 0x38);
        assert_eq!(tint(grey), grey, "the Default theme leaves greys alone");
        // a named theme is still its own colour
        set_color_theme("forest");
        assert_eq!(ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed), 0x2E7D32);
        // (process-wide: put Keydeck's back for the other tests)
        set_default_accent(0x3a82f7);
        set_color_theme("default");
    }
}

#[cfg(test)]
mod slide_tests {
    use super::*;

    /// Run 60 fps frames from `t` for `secs` with the pane `open`; returns (time, slide()'s value).
    fn frames(ctx: &egui::Context, mut t: f64, secs: f64, open: bool) -> (f64, f32) {
        let mut v = 0.0;
        let end = t + secs;
        while t < end - 1e-9 {
            t += 1.0 / 60.0;
            let raw = egui::RawInput { time: Some(t), predicted_dt: 1.0 / 60.0, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| v = slide(ui.ctx(), egui::Id::unique("pane"), open));
            out.textures_delta.clear();
        }
        (t, v)
    }

    #[test]
    fn slides_over_pane_slide_s() {
        let ctx = egui::Context::default();
        let (t, v) = frames(&ctx, 0.0, 0.1, false);
        assert_eq!(v, 0.0);
        // opening: part-way at half the time, all the way after it
        let (t, mid) = frames(&ctx, t, f64::from(PANE_SLIDE_S) / 2.0, true);
        assert!(mid > 0.2 && mid < 0.8, "mid-slide {mid}");
        let (_, end) = frames(&ctx, t, f64::from(PANE_SLIDE_S) / 2.0 + 0.05, true);
        assert_eq!(end, 1.0);
    }
}
