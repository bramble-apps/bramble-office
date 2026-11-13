//! Score project settings ▸ Colors: the Pitch Colors (Type Diatonic, 7 colours, or Chromatic,
//! 12, laid out like piano keys), the eight Velocity Colors (softest at the left), whether
//! accidentals, dots, ties and rests take their note's colour (else black), and the User
//! Palette (16 colours, each with a name the staff style colour menus show). Defaults resets
//! the pane. Values live with the project's settings ("Score / Colors / …", hex colours).

use egui::Color32;

use crate::projsettings::{set, Values};

const SUB: &str = "Score / Colors / ";
pub const TYPES: [&str; 2] = ["Diatonic", "Chromatic"];

/// The pane's colours and switches, read once per frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub chromatic: bool,
    pub pitch: [Color32; 12],
    pub diatonic: [Color32; 7],
    pub velocity: [Color32; 8],
    pub user: [Color32; 16],
    pub names: [String; 16],
    pub accidentals: bool,
    pub dots: bool,
    pub ties: bool,
    pub rests: bool,
}

fn hsv(h: f32, s: f32, v: f32) -> Color32 {
    egui::ecolor::Hsva::new(h, s, v, 1.0).into()
}

pub fn default_pitch(k: usize) -> Color32 {
    hsv(k as f32 / 12.0, 0.8, 0.7)
}

pub fn default_diatonic(k: usize) -> Color32 {
    hsv(k as f32 / 7.0, 0.8, 0.7)
}

/// Blue (soft) through green to red (loud).
pub fn default_velocity(k: usize) -> Color32 {
    hsv((1.0 - k as f32 / 7.0) * 0.66, 0.85, 0.75)
}

pub const DEFAULT_USER: [u32; 16] = [
    0x101010, 0xd03030, 0xe08020, 0xc0a010, 0x50a030, 0x209090, 0x3060d0, 0x8040c0, 0xc04090, 0x705030, 0x606060, 0x902020, 0x207040, 0x204090, 0xa07000, 0x502070,
];

fn rgb(x: u32) -> Color32 {
    Color32::from_rgb((x >> 16) as u8, (x >> 8) as u8, x as u8)
}

fn hex(c: Color32) -> String {
    format!("{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn color(v: &Values, key: &str, default: Color32) -> Color32 {
    let s = v.get(&format!("{SUB}{key}"));
    u32::from_str_radix(s.trim_start_matches('#'), 16).ok().filter(|_| s.len() >= 6).map(rgb).unwrap_or(default)
}

fn flag(v: &Values, key: &str, default: bool) -> bool {
    match v.get(&format!("{SUB}{key}")).as_str() {
        "1" => true,
        "0" => false,
        _ => default,
    }
}

impl Palette {
    pub fn from(v: &Values) -> Palette {
        let names = std::array::from_fn(|k| {
            let n = v.get(&format!("{SUB}Name {k}"));
            if n.is_empty() { format!("Color {k}") } else { n }
        });
        Palette {
            chromatic: v.get(&format!("{SUB}Type:")) != "Diatonic",
            pitch: std::array::from_fn(|k| color(v, &format!("Pitch {k}"), default_pitch(k))),
            diatonic: std::array::from_fn(|k| color(v, &format!("Diatonic {k}"), default_diatonic(k))),
            velocity: std::array::from_fn(|k| color(v, &format!("Velocity {k}"), default_velocity(k))),
            user: std::array::from_fn(|k| color(v, &format!("User {k}"), rgb(DEFAULT_USER[k]))),
            names,
            accidentals: flag(v, "Accidentals", true),
            dots: flag(v, "Dots", true),
            ties: flag(v, "Ties", true),
            rests: flag(v, "Rests", false),
        }
    }

    /// A note's pitch colour (Diatonic: the letter's, sharps taking the note below).
    pub fn of_pitch(&self, pitch: u8) -> Color32 {
        const LETTER: [usize; 12] = [0, 0, 1, 1, 2, 3, 3, 4, 4, 5, 5, 6];
        let pc = (pitch % 12) as usize;
        if self.chromatic { self.pitch[pc] } else { self.diatonic[LETTER[pc]] }
    }

    /// A velocity's colour: eight steps from the softest to the loudest.
    pub fn of_velocity(&self, vel: u8) -> Color32 {
        self.velocity[(vel as usize * 8 / 128).min(7)]
    }
}

/// Defaults: every Colors value back to Logic's.
pub fn reset(v: &mut Values) {
    v.0.retain(|k, _| !k.starts_with(SUB));
}

fn well(ui: &mut egui::Ui, v: &mut Values, key: &str, cur: Color32, size: f32) {
    let mut c = cur;
    let r = ui.scope(|ui| {
        ui.spacing_mut().interact_size = egui::vec2(size, 18.0);
        egui::widgets::color_picker::color_edit_button_srgba(ui, &mut c, egui::widgets::color_picker::Alpha::Opaque)
    });
    if r.inner.changed() && c != cur {
        set(v, &format!("{SUB}{key}"), &hex(c));
    }
}

fn check(ui: &mut egui::Ui, v: &mut Values, key: &str, cur: bool) {
    let mut on = cur;
    if ui.checkbox(&mut on, key).changed() {
        set(v, &format!("{SUB}{key}"), if on { "1" } else { "0" });
    }
}

/// The Colors pane.
pub fn pane(ui: &mut egui::Ui, v: &mut Values) {
    let pal = Palette::from(v);
    ui.label(egui::RichText::new("Pitch Colors").strong());
    ui.horizontal(|ui| {
        ui.label("Type:");
        let cur = if pal.chromatic { 1 } else { 0 };
        egui::ComboBox::from_id_salt("sc-colors-type").selected_text(TYPES[cur]).show_ui(ui, |ui| {
            for (k, t) in TYPES.iter().enumerate() {
                if ui.selectable_label(k == cur, *t).clicked() {
                    set(v, &format!("{SUB}Type:"), t);
                }
            }
        });
    });
    // (laid out like piano keys: the black keys' colours above the white keys')
    if pal.chromatic {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            for k in [1usize, 3, 6, 8, 10] {
                well(ui, v, &format!("Pitch {k}"), pal.pitch[k], 26.0);
                if k == 3 {
                    ui.add_space(30.0);
                }
            }
        });
        ui.horizontal(|ui| {
            for k in [0usize, 2, 4, 5, 7, 9, 11] {
                well(ui, v, &format!("Pitch {k}"), pal.pitch[k], 26.0);
            }
        });
    } else {
        ui.horizontal(|ui| {
            for k in 0..7 {
                well(ui, v, &format!("Diatonic {k}"), pal.diatonic[k], 26.0);
            }
        });
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Velocity Colors").strong());
    ui.horizontal(|ui| {
        ui.label("min");
        for k in 0..8 {
            well(ui, v, &format!("Velocity {k}"), pal.velocity[k], 22.0);
        }
        ui.label("max");
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label("Colored:");
        check(ui, v, "Accidentals", pal.accidentals);
        check(ui, v, "Dots", pal.dots);
        check(ui, v, "Ties", pal.ties);
        check(ui, v, "Rests", pal.rests);
    });
    ui.add_space(6.0);
    ui.label(egui::RichText::new("User Palette").strong());
    egui::Grid::new("sc-colors-user").num_columns(8).spacing([6.0, 4.0]).show(ui, |ui| {
        for k in 0..16 {
            well(ui, v, &format!("User {k}"), pal.user[k], 22.0);
            let mut name = pal.names[k].clone();
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(78.0)).changed() {
                set(v, &format!("{SUB}Name {k}"), &name);
            }
            if k % 4 == 3 {
                ui.end_row();
            }
        }
    });
    ui.add_space(6.0);
    if ui.button("Defaults").clicked() {
        reset(v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_pane_values() {
        let mut v = Values::default();
        let p = Palette::from(&v);
        assert!(p.chromatic && p.accidentals && p.dots && p.ties && !p.rests);
        assert_eq!(p.names[3], "Color 3");
        // chromatic: C and C# differ; diatonic: C# takes C's colour
        assert_ne!(p.of_pitch(60), p.of_pitch(61));
        set(&mut v, "Score / Colors / Type:", "Diatonic");
        set(&mut v, "Score / Colors / Velocity 7", "ff0000");
        set(&mut v, "Score / Colors / User 2", "00ff00");
        set(&mut v, "Score / Colors / Name 2", "Brass");
        set(&mut v, "Score / Colors / Ties", "0");
        let p = Palette::from(&v);
        assert_eq!(p.of_pitch(60), p.of_pitch(61));
        assert_eq!(p.of_pitch(60), p.of_pitch(72));
        assert_eq!(p.of_velocity(127), Color32::from_rgb(255, 0, 0));
        assert_eq!(p.of_velocity(0), default_velocity(0));
        assert_eq!(p.user[2], Color32::from_rgb(0, 255, 0));
        assert_eq!(p.names[2], "Brass");
        assert!(!p.ties);
        reset(&mut v);
        assert_eq!(Palette::from(&v), Palette::from(&Values::default()));
    }
}
