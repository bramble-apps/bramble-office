//! The legacy instruments' window (Analog Basic … Sound Effects), as Logic's: a plain column of
//! the instrument's few parameters — a name at the left, a slider (or pop-up menu) and its
//! value — under the plug-in header. Church Organ's Bass buttons are off for registrations that
//! have no bass pipes.

use egui::{vec2, Align2};
use lg_synth::legacy::{self as lg, Legacy};

use crate::app::App;
use crate::synthui::{Pane, Skin};

const SKIN: Skin = Skin::new(0x2b2b2d, 0x353538, 0x8fb6e8).texts(0xe6e6e8, 0xa8a8ae);

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some(v) = crate::instwin::with_inst(app, t, |l: &mut Legacy| l.variant) else { return };
    let name = lg::VARIANTS.iter().find(|x| x.1 == v).map(|x| x.0).unwrap_or("Legacy");
    let rows = lg::params_of(v).len() as f32;
    crate::synthui::window(app, ctx, t, name, vec2(440.0, 20.0 + rows * 34.0), SKIN, &[], move |pn| body(pn, v));
}

fn body(pn: &mut Pane, v: lg::Variant) {
    for (k, p) in lg::params_of(v).iter().enumerate() {
        let y = 12.0 + k as f32 * 34.0;
        pn.text(16.0, y + 11.0, Align2::LEFT_CENTER, p.name, 11.5, SKIN.text);
        if p.unit == lg_dsp::Unit::Menu {
            let off = p.id == lg::BASS && lg::bass_off(pn.geti(lg::REGISTRATION));
            if off {
                pn.text(150.0, y + 11.0, Align2::LEFT_CENTER, "(no bass pipes in this registration)", 10.5, SKIN.dim);
            } else if p.id == lg::BASS {
                pn.seg(150.0, y, 200.0, 22.0, p.id, lg::BASS_CHOICES, false);
            } else {
                pn.menu(150.0, y, 200.0, 22.0, p.id);
            }
        } else {
            pn.hfader(150.0, y, 200.0, 22.0, p.id);
            let val = pn.get(p.id);
            let s = pn.shown(p.id, val);
            pn.text(424.0, y + 11.0, Align2::RIGHT_CENTER, &s, 10.5, SKIN.dim);
        }
    }
}
