//! The Browser (Logic Pro 13's left pane, ui-tracks-with-pianoroll.png): "Browser" with its two
//! blue buttons; the search field and its filter button; two columns, the sidebar (All, Loops &
//! One-Shots, Instruments, Effects | Plug-in Presets | Sound Library, and Bramble's SoundFonts)
//! and the list (green keyboard marks, Logic's patch names); the footer ("Sorted by:
//! Relevance", the count, …) and + at the bottom left.
//!
//! The patch names are Logic's library (data/patches.txt, 1,700 or so instruments): a click
//! gives the selected track that patch, played by our stand-in for it (units.rs) or, until there
//! is one, the SoundFont's closest General MIDI sound.

use std::sync::OnceLock;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};

use crate::app::{App, SoundFontMode};
use crate::look;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cat {
    All,
    Loops,
    Instruments,
    Effects,
    Presets,
    SoundLibrary,
    SoundFonts,
}

const SIDEBAR: [(Cat, &str); 7] = [(Cat::All, "All"), (Cat::Loops, "Loops & One-Shots"), (Cat::Instruments, "Instruments"), (Cat::Effects, "Effects"), (Cat::Presets, "Plug-in Presets"), (Cat::SoundLibrary, "Sound Library"), (Cat::SoundFonts, "SoundFonts")];

/// Logic's instrument patches: (category, subcategory, name).
pub fn patches() -> &'static [(String, String, String)] {
    static P: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    P.get_or_init(|| {
        include_str!("../data/patches.txt").lines().filter(|l| !l.starts_with('#') && !l.is_empty()).filter_map(|l| {
            let mut it = l.split('\t');
            Some((it.next()?.to_string(), it.next()?.to_string(), it.next()?.to_string()))
        }).collect()
    })
}

/// Logic's order for the list ("Relevance"): the everyday instruments first, as Logic lists them
/// (keyboards, then bass, guitar, strings, brass, mallets, synths, drums).
fn relevance(cat: &str) -> u8 {
    match cat {
        "Keyboard" => 0,
        "Bass" => 1,
        "Guitar" => 2,
        "Strings" => 3,
        "Brass & Woodwind" => 4,
        "Mallet" => 5,
        "Synthesizer" => 6,
        _ => 7,
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    // ---- the header: "Browser" and the two blue buttons at the right
    let head = Rect::from_min_size(area.min, vec2(area.width(), 34.0));
    p.text(pos2(head.min.x + (head.width() - 74.0) / 2.0, head.center().y), Align2::CENTER_CENTER, "Browser", look::semibold(13.0), look::text());
    for (i, tip) in ["Link to the selected track", "Replace the patch (off: add a track)"].into_iter().enumerate() {
        let b = Rect::from_center_size(pos2(head.max.x - 50.0 + i as f32 * 26.0, head.center().y), vec2(22.0, 20.0));
        let on = if i == 0 { app.browser_link } else { app.browser_replace };
        let resp = ui.interact(b, ui.scope_id().with(("bhead", i)), Sense::click()).on_hover_text(tip);
        p.rect_filled(b, 5.0, if on { look::accent() } else { look::group_bg() });
        let c = b.center();
        let col = Color32::WHITE;
        if i == 0 {
            p.circle_stroke(c + vec2(-2.5, 2.5), 3.0, Stroke::new(1.4, col));
            p.circle_stroke(c + vec2(2.5, -2.5), 3.0, Stroke::new(1.4, col));
        } else {
            p.rect_stroke(Rect::from_center_size(c, vec2(12.0, 9.0)), 2.0, Stroke::new(1.3, col), egui::StrokeKind::Inside);
            p.line_segment([c + vec2(-3.0, 0.0), c + vec2(3.0, 0.0)], Stroke::new(1.3, col));
        }
        if resp.clicked() {
            if i == 0 {
                app.browser_link = !app.browser_link;
            } else {
                app.browser_replace = !app.browser_replace;
            }
        }
    }
    // ---- search and the filter button
    let sr = Rect::from_min_size(pos2(area.min.x + 10.0, head.max.y + 6.0), vec2(area.width() - 52.0, 26.0));
    p.rect_filled(sr, 7.0, look::group_bg());
    p.circle_stroke(pos2(sr.min.x + 13.0, sr.center().y - 1.0), 4.5, Stroke::new(1.4, look::text_dim()));
    p.line_segment([pos2(sr.min.x + 16.5, sr.center().y + 2.5), pos2(sr.min.x + 19.5, sr.center().y + 5.5)], Stroke::new(1.4, look::text_dim()));
    let field = Rect::from_min_max(pos2(sr.min.x + 26.0, sr.min.y + 3.0), pos2(sr.max.x - 6.0, sr.max.y - 3.0));
    ui.put(field, egui::TextEdit::singleline(&mut app.browser_search).hint_text("Search").frame(egui::Frame::NONE).font(look::regular(13.0)));
    let fb = Rect::from_min_size(pos2(sr.max.x + 8.0, sr.min.y), vec2(26.0, 26.0));
    let fresp = ui.interact(fb, ui.scope_id().with("bfilter"), Sense::click()).on_hover_text("Filter");
    p.rect_filled(fb, 6.0, if app.browser_filter.is_some() { look::accent() } else { look::group_bg() });
    for (k, w) in [(-4.0f32, 12.0f32), (0.0, 8.0), (4.0, 4.0)] {
        p.hline(fb.center().x - w / 2.0..=fb.center().x + w / 2.0, fb.center().y + k, Stroke::new(1.4, look::text()));
    }
    egui::Popup::menu(&fresp).show(|ui| {
        if ui.add(egui::Button::selectable(app.browser_filter.is_none(), "All Categories")).clicked() {
            app.browser_filter = None;
            ui.close();
        }
        ui.separator();
        let mut cats: Vec<&str> = patches().iter().map(|x| x.0.as_str()).collect();
        cats.sort_by_key(|c| (relevance(c), *c));
        cats.dedup();
        for c in cats {
            if ui.add(egui::Button::selectable(app.browser_filter.as_deref() == Some(c), c)).clicked() {
                app.browser_filter = Some(c.to_string());
                app.browser_tab = 2;
                ui.close();
            }
        }
    });
    let top = sr.max.y + 8.0;
    // ---- the sidebar
    let side_w = (area.width() * 0.385).clamp(120.0, 200.0);
    let side = Rect::from_min_max(pos2(area.min.x, top), pos2(area.min.x + side_w, area.max.y));
    let list = Rect::from_min_max(pos2(side.max.x + 2.0, top), pos2(area.max.x - 1.0, area.max.y - 42.0));
    p.rect_filled(list, 0.0, look::lane_bg());
    let mut y = side.min.y + 4.0;
    for (i, (cat, name)) in SIDEBAR.iter().enumerate() {
        if matches!(cat, Cat::Presets | Cat::SoundLibrary) {
            y += 6.0;
            p.hline(side.min.x + 10.0..=side.max.x - 10.0, y, Stroke::new(1.0, look::line()));
            y += 6.0;
        }
        let r = Rect::from_min_size(pos2(side.min.x + 8.0, y), vec2(side.width() - 16.0, 26.0));
        let resp = ui.interact(r, ui.scope_id().with(("bcat", i)), Sense::click());
        if app.browser_tab == i {
            p.rect_filled(r, 6.0, look::group_bg());
        } else if resp.hovered() {
            p.rect_filled(r, 6.0, look::group_bg().gamma_multiply(0.5));
        }
        sidebar_icon(&p, Rect::from_center_size(pos2(r.min.x + 14.0, r.center().y), vec2(16.0, 16.0)), *cat, look::text());
        p.with_clip_rect(r).text(pos2(r.min.x + 30.0, r.center().y), Align2::LEFT_CENTER, *name, look::regular(13.0), look::text());
        if resp.clicked() {
            app.browser_tab = i;
        }
        y += 28.0;
    }
    // + at the bottom left: add a SoundFont or a folder of sounds
    let plus = Rect::from_min_size(pos2(side.min.x + 10.0, area.max.y - 34.0), vec2(26.0, 24.0));
    let presp = ui.interact(plus, ui.scope_id().with("bplus"), Sense::click()).on_hover_text("Add SoundFont…");
    p.rect_filled(plus, 6.0, if presp.hovered() { look::button_on() } else { look::group_bg() });
    p.text(plus.center(), Align2::CENTER_CENTER, "+", look::regular(16.0), look::text());
    if presp.clicked() {
        add_soundfont(app);
    }
    // ---- the list and its footer
    let cat = SIDEBAR.get(app.browser_tab).map_or(Cat::Instruments, |c| c.0);
    let count = match cat {
        Cat::All | Cat::Instruments => patch_list(app, ui, list),
        Cat::SoundFonts => soundfont_list(app, ui, list),
        Cat::Effects => effect_list(app, ui, list),
        _ => {
            p.text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, match cat {
                Cat::Loops => "No loops installed",
                Cat::Presets => "Select a plug-in to see its presets",
                _ => "Logic's Sound Library isn't installed",
            }, look::regular(12.0), look::text_dim());
            0
        }
    };
    let foot = Rect::from_min_max(pos2(list.min.x, list.max.y), pos2(list.max.x, area.max.y));
    p.hline(foot.x_range(), foot.min.y, Stroke::new(1.0, look::line()));
    p.text(pos2(foot.min.x + 10.0, foot.min.y + 13.0), Align2::LEFT_CENTER, "Sorted by:", look::regular(11.0), look::text_dim());
    let sw = p.layout_no_wrap("Sorted by: ".into(), look::regular(11.0), look::text_dim()).size().x;
    p.text(pos2(foot.min.x + 10.0 + sw, foot.min.y + 13.0), Align2::LEFT_CENTER, if app.browser_sort_name { "Name" } else { "Relevance" }, look::semibold(11.0), look::text());
    p.text(pos2(foot.min.x + 10.0, foot.min.y + 29.0), Align2::LEFT_CENTER, format!("{} Items", thousands(count)), look::regular(11.0), look::text_dim());
    let more = Rect::from_min_size(pos2(foot.max.x - 40.0, foot.min.y + 8.0), vec2(30.0, 24.0));
    let mresp = ui.interact(more, ui.scope_id().with("bmore"), Sense::click());
    p.rect_filled(more, 6.0, if mresp.hovered() { look::button_on() } else { look::group_bg() });
    p.text(more.center(), Align2::CENTER_CENTER, "···", look::bold(12.0), look::text());
    egui::Popup::menu(&mresp).show(|ui| {
        ui.label(egui::RichText::new("Sort by").color(look::text_dim()));
        if ui.add(egui::Button::selectable(!app.browser_sort_name, "Relevance")).clicked() {
            app.browser_sort_name = false;
            ui.close();
        }
        if ui.add(egui::Button::selectable(app.browser_sort_name, "Name")).clicked() {
            app.browser_sort_name = true;
            ui.close();
        }
    });
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn row(ui: &mut egui::Ui, w: f32, selected: bool, name: &str, mark: Color32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::accent().gamma_multiply(0.4));
    } else if resp.hovered() {
        p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::group_bg().gamma_multiply(0.6));
    }
    if selected {
        p.circle_filled(pos2(r.min.x + 8.0, r.center().y), 2.5, look::text());
    }
    // Logic's green keyboard mark
    let m = Rect::from_center_size(pos2(r.min.x + 24.0, r.center().y), vec2(17.0, 13.0));
    p.rect_filled(m, 3.0, mark);
    for k in 0..4 {
        let x = m.min.x + 3.5 + k as f32 * 3.4;
        p.vline(x, m.min.y + 3.0..=m.max.y - 3.0, Stroke::new(1.0, Color32::from_black_alpha(140)));
    }
    p.with_clip_rect(r).text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(13.5), look::text());
    resp
}

/// Logic's instrument patches (the Instruments and All lists); returns how many are listed.
fn patch_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let q = app.browser_search.to_lowercase();
    let mut items: Vec<&(String, String, String)> = patches().iter().filter(|(c, s, n)| app.browser_filter.as_ref().is_none_or(|f| f == c) && (q.is_empty() || n.to_lowercase().contains(&q) || s.to_lowercase().contains(&q) || c.to_lowercase().contains(&q))).collect();
    if app.browser_sort_name {
        items.sort_by_key(|x| x.2.to_lowercase());
    } else {
        // (the file is in Logic's order: its list as Logic showed it, then by category)
        let pos = |x: &(String, String, String)| patches().iter().position(|y| std::ptr::eq(y, x)).unwrap_or(0);
        items.sort_by_key(|x| if pos(x) < 30 { (0, pos(x), false, String::new()) } else { (1 + relevance(&x.0) as usize, 0, x.1.contains("Processed"), x.1.clone()) });
    }
    let current = app.selected_track.and_then(|t| app.instrument_choice.get(&t).cloned().or_else(|| app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone())));
    let n = items.len();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt("browser-patches").auto_shrink([false, false]).show_rows(&mut child, 24.0, n, |ui, range| {
        for k in range {
            let (cat, _, name) = items[k];
            let mark = if cat == "Drums & Percussion" { Color32::from_rgb(0xe3, 0xb3, 0x2b) } else { Color32::from_rgb(0x52, 0xc0, 0x3c) };
            if row(ui, list.width(), current.as_deref() == Some(name.as_str()), name, mark).clicked() {
                match app.selected_track {
                    Some(t) => app.choose_patch(t, cat, name),
                    None => app.tell("Select a track first"),
                }
            }
        }
    });
    n
}

/// Bramble's extra: the SoundFont's own sounds, played natively.
fn soundfont_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let q = app.browser_search.to_lowercase();
    let presets: Vec<(u16, u8, String)> = app.presets.iter().filter(|(_, _, n)| q.is_empty() || n.to_lowercase().contains(&q)).cloned().collect();
    if presets.is_empty() {
        ui.painter_at(list).text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, "No SoundFont: add one with +", look::regular(12.0), look::text_dim());
        return 0;
    }
    let current = app.selected_track.and_then(|t| app.programs.get(t).copied());
    let n = presets.len();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt("browser-sf").auto_shrink([false, false]).show_rows(&mut child, 24.0, n, |ui, range| {
        for k in range {
            let (bank, prog, name) = &presets[k];
            let mark = if *bank == 128 { Color32::from_rgb(0xe3, 0xb3, 0x2b) } else { Color32::from_rgb(0x9b, 0x6c, 0xe6) };
            if row(ui, list.width(), current == Some(*prog) && *bank == 0, name.trim(), mark).clicked() {
                match app.selected_track {
                    Some(t) => {
                        app.instrument_choice.remove(&t);
                        app.set_program(t, *prog, name.trim());
                        app.load_engine();
                    }
                    None => app.tell("Select a track first"),
                }
            }
        }
    });
    n
}

/// The effects Studio has (lg-fx through units.rs).
fn effect_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let names = crate::units::effect_names();
    let q = app.browser_search.to_lowercase();
    let items: Vec<&str> = names.iter().copied().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).collect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    egui::ScrollArea::vertical().id_salt("browser-fx").auto_shrink([false, false]).show(&mut child, |ui| {
        for n in &items {
            row(ui, list.width(), false, n, Color32::from_rgb(0x3d, 0x8f, 0xe0));
        }
    });
    items.len()
}

fn sidebar_icon(p: &egui::Painter, r: Rect, cat: Cat, c: Color32) {
    let s = Stroke::new(1.4, c);
    let m = r.center();
    match cat {
        Cat::All => {
            for (dx, dy) in [(-3.5f32, -3.5f32), (3.5, -3.5), (-3.5, 3.5), (3.5, 3.5)] {
                p.rect_stroke(Rect::from_center_size(m + vec2(dx, dy), vec2(5.5, 5.5)), 1.0, s, egui::StrokeKind::Inside);
            }
        }
        Cat::Loops => {
            for (k, h) in [3.0f32, 7.0, 11.0, 6.0, 9.0, 4.0].iter().enumerate() {
                let x = m.x - 6.0 + k as f32 * 2.4;
                p.vline(x, m.y - h / 2.0..=m.y + h / 2.0, Stroke::new(1.3, c));
            }
        }
        Cat::Instruments => {
            let k = Rect::from_center_size(m, vec2(15.0, 11.0));
            p.rect_filled(k, 2.0, c);
            for i in 1..4 {
                p.vline(k.min.x + i as f32 * 3.75, k.min.y + 1.0..=k.max.y - 1.0, Stroke::new(1.0, look::pane_bg()));
            }
        }
        Cat::Effects => {
            p.circle_stroke(m, 6.5, s);
            p.add(egui::Shape::convex_polygon(vec![m + vec2(0.0, -6.5), m + vec2(4.6, -4.6), m + vec2(6.5, 0.0), m + vec2(4.6, 4.6), m + vec2(0.0, 6.5)], c, Stroke::NONE));
        }
        Cat::Presets => {
            for (dy, k) in [(-4.0f32, -2.0f32), (0.0, 3.0), (4.0, -1.0)] {
                p.hline(m.x - 7.0..=m.x + 7.0, m.y + dy, s);
                p.circle_filled(m + vec2(k, dy), 2.0, c);
            }
        }
        Cat::SoundLibrary => {
            let b = Rect::from_center_size(m + vec2(0.0, 1.0), vec2(15.0, 11.0));
            p.rect_stroke(b, 2.0, s, egui::StrokeKind::Inside);
            p.hline(b.x_range(), b.min.y + 3.5, s);
        }
        Cat::SoundFonts => {
            p.circle_stroke(m + vec2(-2.0, 3.0), 3.0, s);
            p.vline(m.x + 1.0, m.y - 6.0..=m.y + 3.0, s);
            p.line_segment([m + vec2(1.0, -6.0), m + vec2(6.0, -4.0)], s);
        }
    }
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
                app.browser_tab = SIDEBAR.iter().position(|c| c.0 == Cat::SoundFonts).unwrap_or(2);
                app.tell(format!("{} added ({} sounds)", path.file_stem().unwrap_or_default().to_string_lossy(), app.presets.len()));
            } else {
                app.tell("That SoundFont couldn't be read");
            }
        }
        SoundFontMode::Sampler => app.tell("Converting SoundFonts into Sampler instruments isn't available yet: switch to “Play natively” in Settings"),
    }
}
