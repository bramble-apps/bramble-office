//! Sculpture's Tutorial Settings (the Settings pop-up's Tutorial Settings folder): Studio's own
//! settings for the guide's tutorial examples — a fingered electric bass with Body EQ, a
//! roundwound fretless bass with effects, and a raw bowed pad.

use crate::sculpture::*;

pub const NAMES: &[&str] = &["E-Bass Fingered Basic EQ1", "Fretless Roundwound#1", "0001 raw pad"];

fn type_index(o: usize, n: &str) -> f32 {
    types_of(o).iter().position(|t| *t == n).unwrap_or_else(|| panic!("Sculpture object type {n}")) as f32
}

fn model(n: &str) -> f32 {
    BODY_MODELS.iter().position(|t| *t == n).unwrap_or_else(|| panic!("Body EQ model {n}")) as f32
}

/// A tutorial setting's full parameter list, or None.
pub fn setting(name: &str) -> Option<Vec<(u32, f32)>> {
    if !NAMES.contains(&name) {
        return None;
    }
    let mut v: Vec<(u32, f32)> = params_table().iter().map(|p| (p.id, p.default)).collect();
    let mut put = |id: u32, x: f32| {
        if let Some(e) = v.iter_mut().find(|e| e.0 == id) {
            e.1 = x;
        }
    };
    match name {
        "E-Bass Fingered Basic EQ1" => {
            // a plucked (fingered) string two octaves down, a short decay, the Upright Bass body
            put(TRANSPOSE, -2.0);
            put(obj(0, O_ON), 1.0);
            put(obj(0, O_TYPE), type_index(0, "Pick"));
            put(obj(0, O_STR), 0.45);
            put(obj(0, O_POS), 0.18);
            put(INNER_LOSS, 0.25);
            put(MEDIA_LOSS, 0.35);
            put(STIFFNESS, 0.3);
            put(PICK_A, 0.12);
            put(PICK_B, 0.6);
            put(CUTOFF, 0.55);
            put(A_DECAY, 2.5);
            put(A_SUSTAIN, 0.0);
            put(A_RELEASE, 0.15);
            put(B_ON, 1.0);
            put(B_MODEL, model("Upright Bass"));
            put(B_P1, 0.4);
            put(KBMODE, 1.0);
            put(POLY, 1.0);
        }
        "Fretless Roundwound#1" => {
            // the fingered bass without frets (glide, low media loss) and with effects: Body EQ,
            // a little waveshaping and the delay
            put(TRANSPOSE, -2.0);
            put(obj(0, O_ON), 1.0);
            put(obj(0, O_TYPE), type_index(0, "Pick"));
            put(obj(0, O_STR), 0.5);
            put(obj(0, O_POS), 0.2);
            put(INNER_LOSS, 0.18);
            put(MEDIA_LOSS, 0.2);
            put(STIFFNESS, 0.35);
            put(PICK_A, 0.15);
            put(PICK_B, 0.62);
            put(CUTOFF, 0.6);
            put(GLIDE, 0.08);
            put(A_DECAY, 4.0);
            put(A_SUSTAIN, 0.0);
            put(A_RELEASE, 0.25);
            put(B_ON, 1.0);
            put(B_MODEL, model("Upright Bass"));
            put(WS_ON, 1.0);
            put(WS_INPUT, -0.4);
            put(D_ON, 1.0);
            put(D_WET, 0.12);
            put(D_FB, 0.2);
            put(KBMODE, 2.0);
            put(POLY, 1.0);
        }
        _ => {
            // 0001 raw pad: a bowed string, slow attack and release, spread pickups
            put(obj(0, O_ON), 1.0);
            put(obj(0, O_TYPE), type_index(0, "Bow"));
            put(obj(0, O_STR), 0.55);
            put(INNER_LOSS, 0.15);
            put(MEDIA_LOSS, 0.15);
            put(STIFFNESS, 0.1);
            put(A_SOFT, 0.6);
            put(A_SUSTAIN, 1.0);
            put(A_RELEASE, 1.5);
            put(SPREAD_PICK, 0.6);
            put(CUTOFF, 0.7);
            put(POLY, 8.0);
        }
    }
    Some(v)
}
