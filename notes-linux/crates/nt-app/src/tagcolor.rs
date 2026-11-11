//! Tag colours: a tag can wear one of the Mac's tag colours (Finder's: Red, Orange, Yellow, Green,
//! Blue, Purple, Gray), picked from its chip's menu in the sidebar (Tag Color). Shown as a filled
//! dot by the tag wherever it is listed (the Tags browser, a note's Tags menu, the editor's tag
//! suggestions), never as a colour code. A tag whose name is itself a colour code ("#ff8800")
//! shows that colour's dot too. Kept in settings.txt (`tag_colors=work:Red,home:Blue`).

use egui::Color32;
use std::collections::HashMap;
use std::sync::RwLock;

/// The colours offered, by name, in light and dark (Apple's system colours).
pub const PALETTE: &[(&str, [u8; 3], [u8; 3])] = &[
    ("Red", [255, 59, 48], [255, 69, 58]),
    ("Orange", [255, 149, 0], [255, 159, 10]),
    ("Yellow", [255, 204, 0], [255, 214, 10]),
    ("Green", [52, 199, 89], [48, 209, 88]),
    ("Blue", [0, 122, 255], [10, 132, 255]),
    ("Purple", [175, 82, 222], [191, 90, 242]),
    ("Gray", [142, 142, 147], [152, 152, 157]),
];

/// tag (lower case) → colour name, as settings has them (the editor reads it too).
static COLORS: RwLock<Option<HashMap<String, String>>> = RwLock::new(None);

/// "work:Red,home:Blue" → the map (unknown colour names left out).
pub fn parse(s: &str) -> HashMap<String, String> {
    s.split(',')
        .filter_map(|kv| kv.rsplit_once(':'))
        .filter_map(|(t, c)| {
            let name = PALETTE.iter().find(|p| p.0.eq_ignore_ascii_case(c.trim()))?.0;
            let t = t.trim().trim_start_matches('#').to_lowercase();
            (!t.is_empty()).then(|| (t, name.to_string()))
        })
        .collect()
}

/// The map back as settings.txt keeps it (sorted, so the file doesn't churn).
pub fn format(m: &HashMap<String, String>) -> String {
    let mut v: Vec<String> = m.iter().map(|(t, c)| format!("{t}:{c}")).collect();
    v.sort();
    v.join(",")
}

/// The settings' map made the one every part of the window reads.
pub fn publish(s: &str) {
    if let Ok(mut g) = COLORS.write() {
        *g = Some(parse(s));
    }
}

/// The colour name picked for `tag`, if any.
pub fn name_of(tag: &str) -> Option<String> {
    let key = tag.trim_start_matches('#').to_lowercase();
    COLORS.read().ok()?.as_ref()?.get(&key).cloned()
}

/// A palette colour by name, for the current appearance.
pub fn named(name: &str) -> Option<Color32> {
    let p = PALETTE.iter().find(|p| p.0 == name)?;
    let c = if kn_ui::theme::is_light() { p.1 } else { p.2 };
    Some(Color32::from_rgb(c[0], c[1], c[2]))
}

/// A tag that is a colour code ("ff8800", "f80"): that colour.
pub fn code(tag: &str) -> Option<Color32> {
    let h = tag.trim_start_matches('#');
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = |s: &str| u8::from_str_radix(s, 16).ok();
    match h.len() {
        6 => Some(Color32::from_rgb(v(&h[0..2])?, v(&h[2..4])?, v(&h[4..6])?)),
        3 => {
            let d = |i: usize| v(&h[i..i + 1]).map(|x| x * 17);
            Some(Color32::from_rgb(d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

/// The dot drawn by `tag`: its picked colour, else the colour its name spells.
pub fn of(tag: &str) -> Option<Color32> {
    name_of(tag).and_then(|n| named(&n)).or_else(|| code(tag))
}

/// The filled dot (with a hairline so pale colours show on any background).
pub fn dot(p: &egui::Painter, c: egui::Pos2, r: f32, col: Color32) {
    p.circle_filled(c, r, col);
    p.circle_stroke(c, r, egui::Stroke::new(0.5, Color32::from_black_alpha(50)));
}

/// A menu row with a dot and a name (a colour's), ticked when `on`; clicked or not.
pub fn swatch_button(ui: &mut egui::Ui, col: Option<Color32>, name: &str, on: bool) -> egui::Response {
    let r = ui.add(egui::Button::new(format!("       {name}")).selected(on));
    let c = egui::pos2(r.rect.min.x + 14.0, r.rect.center().y);
    match col {
        Some(col) => dot(ui.painter(), c, 5.5, col),
        // (None: an empty ring)
        None => {
            ui.painter().circle_stroke(c, 5.0, egui::Stroke::new(1.0, kn_ui::theme::TEXT_DIM()));
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let m = parse("work:Red, #Home:blue,odd:Chartreuse,:Red");
        assert_eq!(m.get("work").map(String::as_str), Some("Red"));
        assert_eq!(m.get("home").map(String::as_str), Some("Blue"));
        assert_eq!(m.len(), 2);
        assert_eq!(format(&m), "home:Blue,work:Red");
        assert_eq!(parse(&format(&m)), m);
    }

    #[test]
    fn colour_codes_are_colours() {
        assert_eq!(code("#ff8800"), Some(Color32::from_rgb(255, 136, 0)));
        assert_eq!(code("f80"), Some(Color32::from_rgb(255, 136, 0)));
        assert_eq!(code("work"), None);
        assert_eq!(code("ff88"), None);
        // every colour has a name and a colour
        assert!(PALETTE.iter().all(|p| named(p.0).is_some()));
    }
}
