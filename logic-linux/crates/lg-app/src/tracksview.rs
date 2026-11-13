//! Settings ▸ View ▸ Tracks, wired to the Tracks area: the lanes' Background (Dark, Light or
//! Custom with its grayscale slider), Grid Lines Placement (Foreground: the bar lines over the
//! regions; Background: under them), Grid Lines Color (Automatic, or the Horizontal and
//! Vertical sliders, each with its Black checkbox) and Shaded loops.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::Color32;

use crate::settings::Values;

pub const BACKGROUND: &str = "View / Tracks / Background:";
pub const BG_LEVEL: &str = "View / Tracks / Background Level";
pub const PLACEMENT: &str = "View / Tracks / Placement:";
pub const AUTO_COLOR: &str = "View / Tracks / Automatic";
pub const H_LEVEL: &str = "View / Tracks / Horizontal:";
pub const H_BLACK: &str = "View / Tracks / Black";
pub const V_LEVEL: &str = "View / Tracks / Vertical:";
pub const V_BLACK: &str = "View / Tracks / Black #2";
pub const SHADED: &str = "View / Tracks / Shaded loops";

/// The lanes' grey (0 = the theme's), packed as 0x1_00_00_00 | grey.
static BG: AtomicU32 = AtomicU32::new(0);
/// The horizontal (lane) and vertical (bar) lines: 0 = the theme's, else 0x1_00_00_00 |
/// black << 8 | alpha.
static HLINE: AtomicU32 = AtomicU32::new(0);
static VLINE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// The lanes' grey level (None: the theme's dark lanes).
    pub bg: Option<u8>,
    /// Bar lines over the regions.
    pub foreground: bool,
    /// (alpha, black) of the horizontal and vertical lines (None: Automatic).
    pub h: Option<(u8, bool)>,
    pub v: Option<(u8, bool)>,
    pub shaded: bool,
}

/// The grey of the Light background, and of the Custom slider's ends (0 = dark … 100 = light).
const LIGHT: u8 = 0xa8;
const DARKEST: u8 = 0x14;

fn level(v: &Values, key: &str, default: f32) -> f32 {
    v.get(key).trim().parse::<f32>().unwrap_or(default).clamp(0.0, 100.0)
}

pub fn look(v: &Values) -> Look {
    let bg = match v.get(BACKGROUND).as_str() {
        "Light" => Some(LIGHT),
        "Custom" => Some((DARKEST as f32 + (0xe0 - DARKEST) as f32 * level(v, BG_LEVEL, 30.0) / 100.0).round() as u8),
        _ => None,
    };
    let auto = v.get(AUTO_COLOR) != "0";
    let line = |lk: &str, bk: &str| (!auto).then(|| ((level(v, lk, 30.0) * 2.55).round() as u8, v.on(bk)));
    Look { bg, foreground: v.get(PLACEMENT) != "Background", h: line(H_LEVEL, H_BLACK), v: line(V_LEVEL, V_BLACK), shaded: v.on(SHADED) }
}

/// Called each frame before the Tracks area draws: look.rs's lane colours follow the settings.
pub fn apply(v: &Values) -> Look {
    let l = look(v);
    let pack = |x: Option<(u8, bool)>| x.map_or(0, |(a, b)| 0x100_0000 | (b as u32) << 8 | a as u32);
    BG.store(l.bg.map_or(0, |g| 0x100_0000 | g as u32), Ordering::Relaxed);
    HLINE.store(pack(l.h), Ordering::Relaxed);
    VLINE.store(pack(l.v), Ordering::Relaxed);
    l
}

/// The lanes' colour when a Light or Custom background is set (`selected`: a little lighter).
pub fn lane(selected: bool) -> Option<Color32> {
    let b = BG.load(Ordering::Relaxed);
    if b == 0 {
        return None;
    }
    let g = (b & 0xff) as u8;
    let g = if selected { g.saturating_add(10) } else { g };
    Some(Color32::from_gray(g))
}

fn line(packed: u32) -> Option<Color32> {
    if packed == 0 {
        return None;
    }
    let a = (packed & 0xff) as u8;
    Some(if packed & 0x100 != 0 { Color32::from_black_alpha(a) } else { Color32::from_white_alpha(a) })
}

/// The horizontal grid lines (under each lane) when Color isn't Automatic.
pub fn h_line() -> Option<Color32> {
    line(HLINE.load(Ordering::Relaxed))
}

/// The vertical grid lines (the bar lines) when Color isn't Automatic.
pub fn v_line() -> Option<Color32> {
    line(VLINE.load(Ordering::Relaxed))
}

/// Settings rows shown only in some states: the Custom background's slider, and the line
/// sliders while Color isn't Automatic.
pub fn row_hidden(v: &Values, key: &str) -> bool {
    match key {
        BG_LEVEL => v.get(BACKGROUND) != "Custom",
        H_LEVEL | H_BLACK | V_LEVEL | V_BLACK => v.get(AUTO_COLOR) != "0",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(kv: &[(&str, &str)]) -> Values {
        Values(kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
    }

    #[test]
    fn defaults_are_logics() {
        let l = look(&Values::default());
        assert_eq!(l, Look { bg: None, foreground: true, h: None, v: None, shaded: false });
        assert!(row_hidden(&Values::default(), BG_LEVEL));
        assert!(row_hidden(&Values::default(), H_LEVEL));
    }

    #[test]
    fn background_light_and_custom() {
        assert_eq!(look(&vals(&[(BACKGROUND, "Light")])).bg, Some(LIGHT));
        let dark = look(&vals(&[(BACKGROUND, "Custom"), (BG_LEVEL, "0")])).bg.unwrap();
        let light = look(&vals(&[(BACKGROUND, "Custom"), (BG_LEVEL, "100")])).bg.unwrap();
        assert!(dark < light);
        assert!(!row_hidden(&vals(&[(BACKGROUND, "Custom")]), BG_LEVEL));
    }

    #[test]
    fn grid_lines_color_and_placement() {
        let v = vals(&[(AUTO_COLOR, "0"), (H_LEVEL, "100"), (V_BLACK, "1"), (V_LEVEL, "0"), (PLACEMENT, "Background")]);
        let l = look(&v);
        assert!(!l.foreground);
        assert_eq!(l.h, Some((255, false)));
        assert_eq!(l.v, Some((0, true)));
        assert!(!row_hidden(&v, V_BLACK));
        apply(&v);
        assert_eq!(h_line(), Some(Color32::from_white_alpha(255)));
        assert_eq!(v_line(), Some(Color32::from_black_alpha(0)));
        apply(&vals(&[(BACKGROUND, "Light")]));
        assert_eq!(lane(false), Some(Color32::from_gray(LIGHT)));
        apply(&Values::default());
        assert_eq!(h_line(), None);
        assert_eq!(lane(false), None);
    }
}
