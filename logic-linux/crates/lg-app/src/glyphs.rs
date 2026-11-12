//! Studio's icon set: Bramble Icons, one font made from Phosphor (regular and fill) and Tabler
//! (outline, drawn at Phosphor's line weight so the two match), MIT licences in
//! data/icons, plus Studio's own few that neither has (`br`: tuning fork, drum, MIDI port, the
//! Flex and Fade tools; tool/icons). Rebuilt by tool/make_icon_font.py.
//!
//! Each icon is a character: `glyphs::ph::PIANO_KEYS`, `glyphs::ph_fill::PLAY`,
//! `glyphs::tb::METRONOME`. Draw one with [`paint`] (centred in a rectangle, any colour) or put
//! it in text with `RichText::new(ph::GEAR).family(glyphs::family())`.

include!("glyphs_gen.rs");

/// The font's bytes (bundled).
pub static FONT: &[u8] = include_bytes!("../data/icons/BrambleIcons.ttf");

const KEY: &str = "bramble-icons";

/// The icons' vertical correction, as a fraction of their size (negative: up).
const ICON_LIFT: f32 = 0.0;

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
    // (no lift: paint() centres each icon by its drawn shape)
    let tweak = egui::FontTweak { y_offset_factor: ICON_LIFT, ..Default::default() };
    ctx.add_font(FontInsert::new(&format!("{KEY}-{g}"), egui::FontData::from_static(FONT).tweak(tweak), fams));
}

/// Draws `icon` centred in `r`, `size` points tall, in `color`. (Draws nothing until the app's
/// fonts include the icons: never panics on a missing family.)
pub fn paint(p: &egui::Painter, r: egui::Rect, icon: &str, size: f32, color: egui::Color32) {
    let fam = family();
    if !p.ctx().fonts(|f| f.families().contains(&fam)) {
        return;
    }
    let galley = p.layout_no_wrap(icon.to_string(), egui::FontId::new(size, fam), color);
    // centred by the shape actually drawn (the glyph's rendered bounds), not by the font's line
    // box: icons' outlines don't fill their em square the same way, and the line box is rounded
    // to pixels differently at each scale
    let ink = galley.rows.iter().flat_map(|row| row.row.glyphs.iter().map(move |g| (row.pos, g))).fold(egui::Rect::NOTHING, |acc, (rp, g)| {
        if g.uv_rect.is_nothing() {
            acc
        } else {
            acc.union(egui::Rect::from_min_size(rp + g.pos.to_vec2() + g.uv_rect.offset, g.uv_rect.size))
        }
    });
    let ink = if ink.is_positive() { ink } else { egui::Rect::from_min_size(egui::Pos2::ZERO, galley.size()) };
    // (snapped to the physical pixel grid so the edges stay crisp)
    let ppp = p.ctx().pixels_per_point();
    let at = r.center() - ink.center().to_vec2();
    let at = egui::pos2((at.x * ppp).round() / ppp, (at.y * ppp).round() / ppp);
    p.galley(at, galley, color);
}

/// A Logic tool's icon (the tool menus and pop-ups).
pub fn tool(t: crate::edit::Tool) -> &'static str {
    use crate::edit::Tool;
    match t {
        Tool::Pointer => ph::CURSOR,
        Tool::Pencil => ph::PENCIL_SIMPLE,
        Tool::Eraser => ph::ERASER,
        Tool::Text => ph::TEXT_T,
        Tool::Scissors => ph::SCISSORS,
        Tool::Glue => tb::DROPLET,
        Tool::Solo => tb::LETTER_S,
        Tool::Mute => tb::LETTER_M,
        Tool::Zoom => ph::MAGNIFYING_GLASS,
        Tool::Fade => br::FADE,
        Tool::Automation => ph::LINE_SEGMENTS,
        Tool::Marquee => ph::CROSSHAIR_SIMPLE,
        Tool::Flex => br::FLEX,
        Tool::Velocity => ph::CHART_BAR,
        Tool::Quantize => ph::MAGNET,
        Tool::Brush => ph::PAINT_BRUSH,
        Tool::Finger => ph::HAND_POINTING,
        Tool::Gain => ph::SPEAKER_HIGH,
    }
}

/// What a track plays, as its header icon shows it (Logic picks the icon from the patch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackIcon {
    Keys,
    Drums,
    Guitar,
    Bass,
    Synth,
    Strings,
    Voice,
    Wave,
}

impl TrackIcon {
    /// From the track's kind, its name and its instrument's name and patch.
    pub fn of(t: &lg_core::Track) -> TrackIcon {
        use lg_core::TrackKind;
        let mut s = format!(" {} ", t.name.to_lowercase());
        for p in &t.plugins {
            s.push_str(&format!(" {} {} ", p.name.to_lowercase(), p.preset.to_lowercase()));
        }
        let has = |words: &[&str]| words.iter().any(|w| s.contains(w));
        if t.kind == TrackKind::Drummer || has(&["drum", " kit", "beat", "percussion", "808", "909", "conga", "bongo", "cajon", "snare", "hi-hat", "cymbal"]) {
            return TrackIcon::Drums;
        }
        if has(&["vocal", "voice", " vox", "choir", "singer", " mic "]) {
            return TrackIcon::Voice;
        }
        if t.kind == TrackKind::Audio {
            return if has(&["bass"]) {
                TrackIcon::Bass
            } else if has(&["guitar", "gtr", "acoustic"]) {
                TrackIcon::Guitar
            } else {
                TrackIcon::Wave
            };
        }
        if has(&["synth", " pad", "lead", "alchemy", "retro", " es2", " es1", "sculpture", "pluck", "analog", "sweep", " saw"]) {
            TrackIcon::Synth
        } else if has(&["bass"]) {
            TrackIcon::Bass
        } else if has(&["guitar", "gtr", "strum", "steel string", "nylon"]) {
            TrackIcon::Guitar
        } else if has(&["string", "violin", "viola", "cello", "orchestra", "ensemble", "harp"]) {
            TrackIcon::Strings
        } else {
            TrackIcon::Keys
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            TrackIcon::Keys => ph::PIANO_KEYS,
            TrackIcon::Drums => br::DRUM,
            // (neither library has a bass guitar: the guitar; the track colour tells them apart)
            TrackIcon::Guitar | TrackIcon::Bass => ph::GUITAR,
            TrackIcon::Synth => ph::WAVE_SAWTOOTH,
            TrackIcon::Strings => ph::MUSIC_NOTES,
            TrackIcon::Voice => ph::MICROPHONE,
            TrackIcon::Wave => ph::WAVEFORM,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_icons_follow_the_instrument() {
        use lg_core::{Track, TrackKind};
        let icon = |n: &str, k| TrackIcon::of(&Track::new(n, k));
        assert_eq!(icon("SoCal", TrackKind::Drummer), TrackIcon::Drums);
        assert_eq!(icon("Fingerstyle Bass", TrackKind::Instrument), TrackIcon::Bass);
        assert_eq!(icon("Deluxe Classic", TrackKind::Instrument), TrackIcon::Keys);
        assert_eq!(icon("Synth Bass", TrackKind::Instrument), TrackIcon::Synth);
        assert_eq!(icon("Steel String Acoustic", TrackKind::Instrument), TrackIcon::Guitar);
        assert_eq!(icon("Lead Vocal", TrackKind::Audio), TrackIcon::Voice);
        assert_eq!(icon("Audio 3", TrackKind::Audio), TrackIcon::Wave);
        assert_eq!(icon("Studio Strings", TrackKind::Instrument), TrackIcon::Strings);
        assert_eq!(icon("Trap Kit", TrackKind::Instrument), TrackIcon::Drums);
    }

    #[test]
    fn font_and_names_line_up() {
        // a TrueType file, and every library's icons listed with their characters
        assert_eq!(&FONT[..4], &[0, 1, 0, 0]);
        assert!(ph::ALL.len() > 1000 && ph_fill::ALL.len() > 1000 && tb::ALL.len() > 4000);
        assert_eq!(ph::ALL.iter().find(|(n, _)| *n == "piano-keys").map(|x| x.1), Some(ph::PIANO_KEYS));
        assert_eq!(tb::ALL.iter().find(|(n, _)| *n == "metronome").map(|x| x.1), Some(tb::METRONOME));
    }
}
