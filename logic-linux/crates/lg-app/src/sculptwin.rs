//! Sculpture's window (stub: laid out by its window work).

use egui::vec2;

use crate::app::App;
use crate::synthui::{self, Pane, Skin};

const SKIN: Skin = Skin::new(0x3a3d42, 0x2e3135, 0x7fb0e0);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    synthui::window(app, ctx, t, "Sculpture", vec2(900.0, 470.0), SKIN, &[], body);
}

fn body(_p: &mut Pane) {}
