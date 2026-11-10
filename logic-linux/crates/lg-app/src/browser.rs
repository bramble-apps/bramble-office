//! The Browser (Logic Pro 13's left pane): a search field, the categories (All, Loops &
//! One-Shots, Instruments, Effects, Plug-in Presets) and the list. Instruments lists the
//! SoundFont's sounds (Bramble plays SoundFonts natively); a click gives the selected track that
//! sound. "Add SoundFont…" loads another .sf2 / .sf3, played natively or (Settings) converted to a
//! Sampler instrument.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::{App, SoundFontMode};
use crate::look;

const CATEGORIES: [&str; 5] = ["All", "Loops & One-Shots", "Instruments", "Effects", "Plug-in Presets"];

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    p.text(head.center(), Align2::CENTER_CENTER, "Browser", look::semibold(13.0), look::text());
    p.hline(area.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    let search = Rect::from_min_size(pos2(area.min.x + 10.0, head.max.y + 8.0), vec2(area.width() - 20.0, 24.0));
    ui.put(search, egui::TextEdit::singleline(&mut app.browser_search).hint_text("Search"));
    let mut y = search.max.y + 12.0;
    for (i, c) in CATEGORIES.iter().enumerate() {
        let r = Rect::from_min_size(pos2(area.min.x + 8.0, y), vec2(area.width() - 16.0, 24.0));
        let resp = ui.interact(r, ui.scope_id().with(("cat", i)), Sense::click());
        if app.browser_tab == i {
            p.rect_filled(r, 6.0, look::group_bg());
        } else if resp.hovered() {
            p.rect_filled(r, 6.0, look::group_bg().gamma_multiply(0.5));
        }
        p.text(pos2(r.min.x + 10.0, r.center().y), Align2::LEFT_CENTER, *c, look::regular(13.0), look::text());
        if resp.clicked() {
            app.browser_tab = i;
        }
        y += 26.0;
    }
    y += 6.0;
    p.hline(area.min.x + 10.0..=area.max.x - 10.0, y, Stroke::new(1.0, look::line()));
    y += 6.0;
    let list = Rect::from_min_max(pos2(area.min.x, y), pos2(area.max.x, area.max.y - 40.0));
    match app.browser_tab {
        0 | 2 => instruments(app, ui, list),
        _ => {
            p.text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, "Coming with Bramble's own sounds", look::regular(12.0), look::text_dim());
        }
    }
    // Add SoundFont…
    let add = Rect::from_min_size(pos2(area.min.x + 10.0, area.max.y - 34.0), vec2(area.width() - 20.0, 26.0));
    if ui.put(add, egui::Button::new("Add SoundFont…").corner_radius(6.0)).clicked() {
        add_soundfont(app);
    }
}

fn instruments(app: &mut App, ui: &mut egui::Ui, list: Rect) {
    let q = app.browser_search.to_lowercase();
    let presets: Vec<(u16, u8, String)> = app.presets.iter().filter(|(b, _, n)| *b == 0 && (q.is_empty() || n.to_lowercase().contains(&q))).cloned().collect();
    if presets.is_empty() {
        ui.painter_at(list).text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, "No SoundFont found: Add SoundFont…", look::regular(12.0), look::text_dim());
        return;
    }
    let current = app.selected_track.and_then(|t| app.programs.get(t).copied());
    ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
        egui::ScrollArea::vertical().id_salt("browser-list").show(ui, |ui| {
            ui.set_width(list.width());
            for (_, prog, name) in presets {
                let (r, resp) = ui.allocate_exact_size(vec2(list.width(), 26.0), Sense::click());
                let p = ui.painter();
                if current == Some(prog) {
                    p.rect_filled(r.shrink2(vec2(6.0, 1.0)), 5.0, look::accent().gamma_multiply(0.35));
                } else if resp.hovered() {
                    p.rect_filled(r.shrink2(vec2(6.0, 1.0)), 5.0, look::group_bg().gamma_multiply(0.6));
                }
                // Logic's green keyboard mark
                let m = Rect::from_center_size(pos2(r.min.x + 22.0, r.center().y), vec2(16.0, 12.0));
                p.rect_filled(m, 2.0, Color32::from_rgb(0x52, 0xc0, 0x3c));
                p.text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, name.trim(), look::regular(13.0), look::text());
                if resp.clicked() {
                    match app.selected_track {
                        Some(t) => app.set_program(t, prog, name.trim()),
                        None => app.tell("Select a track first"),
                    }
                }
            }
        });
    });
}

fn add_soundfont(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(path) = kn_ui::files::FileDialog::new().set_title("Add a SoundFont").add_filter("SoundFont", &["sf2", "sf3", "SF2", "SF3"]).pick_file() else { return };
    match app.soundfont_mode {
        SoundFontMode::Native => {
            let ok = {
                let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
                e.load_soundfont(Some(&path));
                e.has_soundfont()
            };
            if ok {
                app.presets = crate::app::soundfont_presets(&path);
                crate::app::save_soundfont_setting(&path);
                app.load_engine();
                app.browser_tab = 2;
                app.tell(format!("{} added ({} sounds)", path.file_stem().unwrap_or_default().to_string_lossy(), app.presets.len()));
            } else {
                app.tell("That SoundFont couldn't be read");
            }
        }
        SoundFontMode::Sampler => app.tell("Converting SoundFonts into Sampler instruments isn't available yet: switch to “Play natively” in Settings"),
    }
}
