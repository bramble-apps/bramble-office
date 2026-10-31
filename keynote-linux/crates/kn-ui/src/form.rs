//! The phone or tablet form of the window, as Apple's iPhone and iPad versions of Keynote,
//! Numbers and Pages: on a phone (compact) the menus sit under one button, the inspector is a
//! sheet from the bottom and the sidebar is closed until asked for. On Android the form follows
//! the window's width (so a foldable switches as it opens) unless Settings > Layout picks one;
//! desktops always have the full (tablet) form.

use std::sync::atomic::{AtomicU8, Ordering};

/// Settings > Layout: 0 automatic, 1 phone, 2 tablet.
static PREF: AtomicU8 = AtomicU8::new(0);

/// Narrower than this (points), Automatic is the phone form.
pub const PHONE_WIDTH: f32 = 700.0;

/// The choices for Settings > Layout, as stored.
pub const CHOICES: [(u8, &str); 3] = [(0, "Automatic"), (1, "Phone"), (2, "Tablet")];

/// Settings > Layout (each frame, from the app's settings).
pub fn set_pref(p: u8) {
    PREF.store(p.min(2), Ordering::Relaxed);
}

/// The form the last frame had (for code without the context, such as opening a document).
static LAST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The window is in the phone form. (KN_UI_FORM=phone|tablet for automated checks.)
pub fn compact(ctx: &egui::Context) -> bool {
    let c = match std::env::var("KN_UI_FORM").ok().as_deref() {
        Some("phone") => true,
        Some("tablet") => false,
        _ => match PREF.load(Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => cfg!(target_os = "android") && ctx.content_rect().width() < PHONE_WIDTH,
        },
    };
    LAST.store(c, Ordering::Relaxed);
    c
}

/// The phone form, as of the last frame: a document opened then starts with its inspector and
/// sidebar closed, as on the iPhone.
pub fn was_compact() -> bool {
    LAST.load(Ordering::Relaxed)
}

/// Settings > Layout's drop-down (shown on Android).
pub fn combo(ui: &mut egui::Ui, id: &str, pref: &mut u8) {
    let cur = CHOICES.iter().find(|c| c.0 == *pref).map(|c| c.1).unwrap_or("Automatic");
    egui::ComboBox::from_id_salt(id).selected_text(cur).show_ui(ui, |ui| {
        for (v, name) in CHOICES {
            ui.selectable_value(pref, v, name);
        }
    });
}

/// The window just switched form: Some(true) into the phone form (apps close the sidebar and
/// inspector then), Some(false) out of it. None the rest of the time (and on the first frame
/// in the tablet form).
pub fn switched(ctx: &egui::Context) -> Option<bool> {
    thread_local!(static WAS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) });
    let now = compact(ctx);
    let was = WAS.with(|w| w.replace(Some(now)));
    match was {
        Some(w) if w != now => Some(now),
        None if now => Some(true),
        _ => None,
    }
}

/// The inspector in the phone form: a sheet from the bottom of the window, as on the iPhone
/// (rounded top, a grabber; about half the window, and it can be dragged taller or shorter).
pub fn sheet(ui: &mut egui::Ui, id: &str, fill: egui::Color32, add: impl FnOnce(&mut egui::Ui)) {
    let h = ui.max_rect().height();
    egui::Panel::bottom(id)
        .resizable(true)
        .show_separator_line(false)
        .default_size(h * 0.45)
        .size_range(160.0..=h * 0.85)
        .frame(egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }).inner_margin(egui::Margin { left: 12, right: 12, top: 18, bottom: 12 }))
        .show(ui, |ui| {
            let top = ui.max_rect().center_top() - egui::vec2(0.0, 10.0);
            ui.painter().rect_filled(egui::Rect::from_center_size(top, egui::vec2(36.0, 5.0)), 2.5, ui.visuals().weak_text_color());
            add(ui);
        });
}
