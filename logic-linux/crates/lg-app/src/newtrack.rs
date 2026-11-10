//! Logic's Create New Track sheet (⌥⌘N, the + over the track headers): choose a kind —
//! Software Instrument, Pattern, Session Player (Drummer, Bass Player, Keyboard Player), Audio
//! (Mic or Line, Guitar or Bass) — then Details and how many to make; Create.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::App;
use crate::look;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Instrument,
    Pattern,
    Drummer,
    Bass,
    Keys,
    Audio,
    Guitar,
}

const CARDS: [(Kind, &str, &str); 7] = [
    (Kind::Instrument, "Software Instrument", "MIDI"),
    (Kind::Pattern, "Pattern", "MIDI"),
    (Kind::Drummer, "Drummer", "Session Player"),
    (Kind::Bass, "Bass Player", "Session Player"),
    (Kind::Keys, "Keyboard Player", "Session Player"),
    (Kind::Audio, "Mic or Line", "Audio"),
    (Kind::Guitar, "Guitar or Bass", "Audio"),
];

pub fn sheet(app: &mut App, ctx: &egui::Context) {
    if !app.new_track_open {
        return;
    }
    let mut create = false;
    let mut cancel = false;
    let screen = ctx.content_rect();
    egui::Window::new("New Track").id(egui::Id::unique("new-track")).title_bar(false).resizable(false).collapsible(false).fixed_size(vec2(640.0, 330.0)).default_pos(pos2(screen.center().x - 320.0, screen.min.y + 100.0)).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(vec2(640.0, 330.0), Sense::hover());
        let p = ui.painter().clone();
        p.text(pos2(r.center().x, r.min.y + 18.0), Align2::CENTER_CENTER, "Choose a track type", look::semibold(14.0), look::text());
        // the sections: MIDI, Session Player, Audio, each with its cards
        let mut x = r.min.x + 12.0;
        let mut last_group = "";
        let cw = 82.0;
        for (kind, name, group) in CARDS {
            if group != last_group {
                if !last_group.is_empty() {
                    x += 14.0;
                }
                p.text(pos2(x, r.min.y + 48.0), Align2::LEFT_CENTER, group, look::semibold(12.0), look::text_dim());
                last_group = group;
            }
            let card = Rect::from_min_size(pos2(x, r.min.y + 62.0), vec2(cw - 6.0, 96.0));
            x += cw;
            let on = app.new_track_kind == kind;
            let resp = ui.interact(card, ui.scope_id().with(("nt", name)), Sense::click());
            p.rect_filled(card, 10.0, if on { look::accent().gamma_multiply(0.35) } else if resp.hovered() { look::button_on() } else { look::group_bg() });
            if on {
                p.rect_stroke(card, 10.0, Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
            }
            card_icon(&p, Rect::from_center_size(card.center() - vec2(0.0, 14.0), vec2(36.0, 36.0)), kind);
            let g = p.layout(name.to_string(), look::regular(11.0), look::text(), card.width() - 8.0);
            p.galley(pos2(card.center().x - g.size().x / 2.0, card.max.y - 30.0), g, look::text());
            if resp.clicked() {
                app.new_track_kind = kind;
            }
            if resp.double_clicked() {
                create = true;
            }
        }
        // Details and the count
        let y = r.min.y + 186.0;
        p.hline(r.min.x + 12.0..=r.max.x - 12.0, y - 10.0, Stroke::new(1.0, look::line()));
        p.text(pos2(r.min.x + 16.0, y + 10.0), Align2::LEFT_CENTER, "Details:", look::semibold(12.0), look::text());
        let detail = match app.new_track_kind {
            Kind::Instrument => "Instrument: Empty Patch, Output: Stereo Out",
            Kind::Pattern => "A Step Sequencer pattern region on a Drum Machine kit",
            Kind::Drummer => "Drummer: Kyle — Pop Rock, kit SoCal; an 8-bar region at the playhead",
            Kind::Bass => "Bass Player: Pop Songwriter over the chord track; an 8-bar region",
            Kind::Keys => "Keyboard Player: Strummed piano over the chord track; an 8-bar region",
            Kind::Audio => "Input: Input 1, Output: Stereo Out",
            Kind::Guitar => "Input: Input 1 with a guitar channel strip",
        };
        p.text(pos2(r.min.x + 90.0, y + 10.0), Align2::LEFT_CENTER, detail, look::regular(12.0), look::text_dim());
        p.text(pos2(r.min.x + 16.0, y + 46.0), Align2::LEFT_CENTER, "Number of tracks:", look::regular(12.0), look::text());
        let mut n = app.new_track_count as f32;
        if crate::widgets::value_slot(ui, Rect::from_min_size(pos2(r.min.x + 130.0, y + 36.0), vec2(60.0, 20.0)), ui.scope_id().with("nt-count"), &mut n, 1.0, 64.0, 1.0, 1.0, &|v| format!("{v:.0}")) {
            app.new_track_count = n as u32;
        }
        let bc = Rect::from_min_size(pos2(r.max.x - 196.0, r.max.y - 38.0), vec2(88.0, 28.0));
        let bk = Rect::from_min_size(pos2(r.max.x - 100.0, r.max.y - 38.0), vec2(88.0, 28.0));
        if ui.put(bc, egui::Button::new("Cancel")).clicked() {
            cancel = true;
        }
        if ui.put(bk, egui::Button::new(egui::RichText::new("Create").color(Color32::WHITE)).fill(look::accent())).clicked() {
            create = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            create = true;
        }
    });
    if create {
        app.new_track_open = false;
        let n = app.new_track_count.max(1);
        for _ in 0..n {
            make(app, app.new_track_kind);
        }
    } else if cancel {
        app.new_track_open = false;
    }
}

fn make(app: &mut App, kind: Kind) {
    match kind {
        Kind::Drummer => app.new_drummer(),
        Kind::Bass => crate::players::new_track(app, false),
        Kind::Keys => crate::players::new_track(app, true),
        Kind::Audio | Kind::Guitar => {
            let ctx = egui::Context::default();
            app.edit_command(&ctx, "New Audio Track");
            if kind == Kind::Guitar {
                if let Some(t) = app.project.as_mut().and_then(|p| p.tracks.last_mut()) {
                    t.name = "Guitar".into();
                }
            }
        }
        Kind::Instrument | Kind::Pattern => {
            app.checkpoint("New Track");
            if let Some(p) = app.project.as_mut() {
                let row = p.tracks.len() as u32 + 1;
                let name = if kind == Kind::Pattern { "Pattern" } else { "Inst 1" };
                p.tracks.push(lg_core::Track { name: name.into(), kind: TrackKind::Instrument, channel: 0, row, regions: Vec::new(), volume_db: 0.0, pan: 0.0, mute: false, solo: false, plugins: Vec::new() });
                app.programs.push(0);
                app.selected_track = Some(p.tracks.len() - 1);
            }
            app.after_edit();
        }
    }
}

fn card_icon(p: &egui::Painter, r: Rect, kind: Kind) {
    let c = r.center();
    let col = look::text();
    let s = Stroke::new(1.6, col);
    match kind {
        Kind::Instrument => {
            let k = Rect::from_center_size(c, vec2(32.0, 20.0));
            p.rect_filled(k, 3.0, Color32::from_rgb(0x52, 0xc0, 0x3c));
            for i in 1..6 {
                p.vline(k.min.x + i as f32 * 5.3, k.min.y + 2.0..=k.max.y - 2.0, Stroke::new(1.0, Color32::from_black_alpha(150)));
            }
        }
        Kind::Pattern => {
            for (i, on) in [true, false, true, true, false, true, false, true].iter().enumerate() {
                let x = c.x - 16.0 + (i % 4) as f32 * 8.5;
                let y = c.y - 8.0 + (i / 4) as f32 * 9.0;
                p.rect_filled(Rect::from_min_size(pos2(x, y), vec2(7.0, 7.0)), 1.5, if *on { Color32::from_rgb(0x52, 0xc0, 0x3c) } else { look::line() });
            }
        }
        Kind::Drummer => {
            p.circle_filled(c, 14.0, Color32::from_rgb(0xe3, 0xb3, 0x2b));
            p.circle_stroke(c, 9.0, Stroke::new(1.5, Color32::from_black_alpha(120)));
        }
        Kind::Bass | Kind::Guitar => {
            p.line_segment([c + vec2(-12.0, 12.0), c + vec2(10.0, -10.0)], Stroke::new(3.0, col));
            p.circle_filled(c + vec2(-10.0, 10.0), 7.0, if kind == Kind::Bass { Color32::from_rgb(0x52, 0xc0, 0x3c) } else { Color32::from_rgb(0x3d, 0x8f, 0xe0) });
        }
        Kind::Keys => {
            let k = Rect::from_center_size(c, vec2(34.0, 18.0));
            p.rect_stroke(k, 2.0, s, egui::StrokeKind::Inside);
            for i in 1..7 {
                p.vline(k.min.x + i as f32 * 4.85, k.min.y..=k.max.y, Stroke::new(1.0, col));
            }
        }
        Kind::Audio => {
            p.rect_stroke(Rect::from_center_size(c - vec2(0.0, 4.0), vec2(10.0, 18.0)), 5.0, s, egui::StrokeKind::Inside);
            p.vline(c.x, c.y + 6.0..=c.y + 13.0, s);
            p.hline(c.x - 5.0..=c.x + 5.0, c.y + 13.0, s);
        }
    }
}
