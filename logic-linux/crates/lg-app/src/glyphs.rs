//! Studio's icon set: Bramble Icons, one font made from Phosphor (regular and fill) and Tabler
//! (outline, drawn at Phosphor's line weight so the two match), MIT licences in
//! data/icons. Rebuilt by tool/make_icon_font.py.
//!
//! Each icon is a character: `glyphs::ph::PIANO_KEYS`, `glyphs::ph_fill::PLAY`,
//! `glyphs::tb::METRONOME`. Draw one with [`paint`] (centred in a rectangle, any colour) or put
//! it in text with `RichText::new(ph::GEAR).family(glyphs::family())`.

include!("glyphs_gen.rs");

/// The font's bytes (bundled).
pub static FONT: &[u8] = include_bytes!("../data/icons/BrambleIcons.ttf");

const KEY: &str = "bramble-icons";

/// The egui family holding only the icons.
pub fn family() -> egui::FontFamily {
    egui::FontFamily::Name(KEY.into())
}

/// Adds the icon font (look.rs, with Studio's other fonts): as its own family and last in the
/// proportional fallbacks (so icons also draw inside ordinary text). `g` keeps the name unique
/// each time the fonts are set again.
pub fn add(ctx: &egui::Context, g: u32) {
    use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
    let fams = vec![
        InsertFontFamily { family: family(), priority: FontPriority::Highest },
        InsertFontFamily { family: egui::FontFamily::Proportional, priority: FontPriority::Lowest },
    ];
    ctx.add_font(FontInsert::new(&format!("{KEY}-{g}"), egui::FontData::from_static(FONT), fams));
}

/// Draws `icon` centred in `r`, `size` points tall, in `color`. (Draws nothing until the app's
/// fonts include the icons: never panics on a missing family.)
pub fn paint(p: &egui::Painter, r: egui::Rect, icon: &str, size: f32, color: egui::Color32) {
    let fam = family();
    if !p.ctx().fonts(|f| f.families().contains(&fam)) {
        return;
    }
    p.text(r.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::new(size, fam), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_and_names_line_up() {
        // a TrueType file, and every library's icons listed with their characters
        assert_eq!(&FONT[..4], &[0, 1, 0, 0]);
        assert!(ph::ALL.len() > 1000 && ph_fill::ALL.len() > 1000 && tb::ALL.len() > 4000);
        assert_eq!(ph::ALL.iter().find(|(n, _)| *n == "piano-keys").map(|x| x.1), Some(ph::PIANO_KEYS));
        assert_eq!(tb::ALL.iter().find(|(n, _)| *n == "metronome").map(|x| x.1), Some(tb::METRONOME));
    }
}
