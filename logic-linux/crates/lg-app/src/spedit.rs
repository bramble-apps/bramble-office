//! The Session Player Editor for Bass and Keyboard Players (and their Synth Player styles), laid
//! out as Logic 13's: the editor's bar (Chords pop-up, the Session Player and Preset pop-ups),
//! the ruler with the preview Play button, the region chords lane and the region; then, scrolled,
//! the Session Player button (type and style), Complexity and Intensity sliders, Regenerate, the
//! Main / Details / Manual tabs and the dark panel with the player's controls: patterns, Fill
//! Amount and Swing with their locks, the hands, Melody / Octaves / Phrasing, the voicings, the
//! style's own Details, Feel / Dynamics / Tempo / Humanize, and the Synth Players' XY pad, note
//! range, LFO, Slides and Envelope views.

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke};
use lg_players::bass::{BassStyle, Extra, OCTAVE_OPTIONS, PHRASING_OPTIONS};
use lg_players::drummer::{SwingType, TempoFeel};
use lg_players::keys::{KeysStyle, LeftHand, ARP_MODES, LH_STYLES, PHRASING, RH_MOVEMENT};
use lg_players::synth::{self, ChordResponse, EnvLength, Lfo, PatternKind, SlideMode, SlideStart, Slides, Sync, Wave};

use crate::app::App;
use crate::glyphs;
use crate::look;
use crate::players::Player;

pub fn yellow() -> Color32 {
    Color32::from_rgb(0xff, 0xd6, 0x0a)
}
pub(crate) fn panel_bg() -> Color32 {
    Color32::from_gray(0x12)
}
pub(crate) fn strip_bg() -> Color32 {
    Color32::from_gray(0x18)
}
pub(crate) fn button_bg() -> Color32 {
    Color32::from_gray(0x36)
}
pub(crate) fn label_col() -> Color32 {
    Color32::from_gray(0xd8)
}

/// What the editor shows at the top (in points).
const TITLE_H: f32 = 30.0;
const BAR_H: f32 = 30.0;
const RULER_H: f32 = 20.0;
const CHORDS_H: f32 = 16.0;
const REGION_H: f32 = 58.0;
/// The panel's width (Logic: 459 points) and corner radius.
pub(crate) const PANEL_W: f32 = 459.0;
pub(crate) const PANEL_R: f32 = 21.0;

/// The ids and painter the controls share.
pub(crate) struct Cx {
    pub p: egui::Painter,
    pub base: egui::Id,
}

impl Cx {
    pub(crate) fn id(&self, s: &str) -> egui::Id {
        self.base.with(s)
    }

    /// A label (grey-white, 13 pt) centred at `c`.
    pub(crate) fn label(&self, c: Pos2, s: &str) {
        self.p.text(c, Align2::CENTER_CENTER, s, look::regular(13.0), label_col());
    }

    /// Logic's knob: the label above, a 44-point body in a ring lit yellow from the start (from
    /// the middle when bipolar); drag up or down, Option-click / double-click for the default.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn knob(&self, ui: &mut egui::Ui, c: Pos2, key: &str, label: &str, v: &mut f32, min: f32, max: f32, default: f32, shown: &dyn Fn(f32) -> String) -> bool {
        self.label(c, label);
        let kc = c + vec2(0.0, 46.0);
        let d = 58.0;
        let resp = ui.interact(Rect::from_center_size(kc, vec2(d, d)), self.id(key), Sense::click_and_drag()).on_hover_text(format!("{label}: {}", shown(*v)));
        let mut changed = false;
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let nv = (*v - resp.drag_delta().y / if fine { 600.0 } else { 150.0 } * (max - min)).clamp(min, max);
            changed = nv != *v;
            *v = nv;
        }
        if resp.double_clicked() || (resp.clicked() && ui.input(|i| i.modifiers.alt)) {
            changed = *v != default;
            *v = default;
        }
        let start = std::f32::consts::PI * (0.5 + 1.0 / 6.0);
        let sweep = std::f32::consts::PI * 5.0 / 3.0;
        let ring_r = d / 2.0 - 3.0;
        let arc = |a0: f32, a1: f32, col: Color32| {
            let pts: Vec<Pos2> = (0..=32).map(|k| {
                let a = a0 + (a1 - a0) * k as f32 / 32.0;
                kc + vec2(a.cos(), a.sin()) * ring_r
            }).collect();
            self.p.add(Shape::line(pts, Stroke::new(5.0, col)));
        };
        arc(start, start + sweep, Color32::from_gray(0x2a));
        let frac = ((*v - min) / (max - min).max(1e-6)).clamp(0.0, 1.0);
        // lit from the middle when the default is the middle (Feel, Dynamics)
        let zero = if (default - (min + max) / 2.0).abs() < 1e-4 && (min < 0.0 || key == "dyn") { 0.5 } else if min < 0.0 && max > 0.0 { -min / (max - min) } else { 0.0 };
        let (a0, a1) = (start + sweep * zero.min(frac), start + sweep * zero.max(frac));
        if a1 > a0 + 0.01 {
            arc(a0, a1, yellow());
        }
        self.p.circle_filled(kc, 22.0, Color32::from_gray(if resp.hovered() || resp.dragged() { 0x44 } else { 0x3a }));
        let a = start + sweep * frac;
        self.p.line_segment([kc + vec2(a.cos(), a.sin()) * 10.0, kc + vec2(a.cos(), a.sin()) * 19.0], Stroke::new(2.0, yellow()));
        if resp.dragged() || resp.hovered() {
            self.p.text(kc + vec2(0.0, d / 2.0 + 8.0), Align2::CENTER_CENTER, shown(*v), look::regular(11.0), yellow());
        }
        changed
    }

    /// A switch button: yellow with black text when on, grey when off.
    pub(crate) fn toggle(&self, ui: &mut egui::Ui, c: Pos2, key: &str, label: &str, on: &mut bool) -> bool {
        let w = ui.fonts_mut(|f| f.layout_no_wrap(label.into(), look::regular(13.0), Color32::WHITE).size().x) + 20.0;
        let r = Rect::from_center_size(c, vec2(w, 24.0));
        let resp = ui.interact(r, self.id(key), Sense::click());
        self.p.rect_filled(r, 6.0, if *on { yellow() } else { button_bg() });
        self.p.text(r.center(), Align2::CENTER_CENTER, label, look::regular(13.0), if *on { Color32::BLACK } else { label_col() });
        if resp.clicked() {
            *on = !*on;
            return true;
        }
        false
    }

    /// A pop-up: the label above, the value in yellow with the up-down chevrons.
    pub(crate) fn popup(&self, ui: &mut egui::Ui, c: Pos2, key: &str, label: &str, options: &[&str], cur: &mut usize) -> bool {
        self.label(c, label);
        let val = options.get(*cur).copied().unwrap_or("");
        let w = ui.fonts_mut(|f| f.layout_no_wrap(val.into(), look::regular(13.0), Color32::WHITE).size().x);
        let vr = Rect::from_center_size(c + vec2(0.0, 17.0), vec2(w + 18.0, 18.0));
        self.p.text(pos2(vr.min.x, vr.center().y), Align2::LEFT_CENTER, val, look::regular(13.0), yellow());
        glyphs::paint(&self.p, Rect::from_center_size(pos2(vr.max.x - 6.0, vr.center().y), vec2(12.0, 12.0)), glyphs::ph::CARET_UP_DOWN, 11.0, yellow());
        let resp = ui.interact(vr.expand(2.0), self.id(key), Sense::click());
        let mut changed = false;
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(180.0);
            for (i, o) in options.iter().enumerate() {
                if ui.add(egui::Button::selectable(i == *cur, *o)).clicked() {
                    changed = i != *cur;
                    *cur = i;
                    ui.close();
                }
            }
        });
        changed
    }

    /// A value: the label above, the number in yellow; drag up or down to change it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn value(&self, ui: &mut egui::Ui, c: Pos2, key: &str, label: &str, v: &mut f32, min: f32, max: f32, default: f32, shown: &dyn Fn(f32) -> String) -> bool {
        self.label(c, label);
        let vr = Rect::from_center_size(c + vec2(0.0, 17.0), vec2(70.0, 18.0));
        self.p.text(vr.center(), Align2::CENTER_CENTER, shown(*v), look::regular(13.0), yellow());
        let resp = ui.interact(vr, self.id(key), Sense::click_and_drag()).on_hover_text(format!("{label}: drag up or down"));
        let mut changed = false;
        if resp.dragged() {
            let nv = (*v - resp.drag_delta().y / 150.0 * (max - min)).clamp(min, max);
            changed = nv != *v;
            *v = nv;
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        if resp.double_clicked() {
            changed = *v != default;
            *v = default;
        }
        changed
    }

    /// Logic's vertical slider: a thin track, yellow below the thumb (a grey pill with a yellow
    /// line).
    pub(crate) fn vslider(&self, ui: &mut egui::Ui, track: Rect, key: &str, v: &mut f32) -> bool {
        let resp = ui.interact(track.expand2(vec2(14.0, 6.0)), self.id(key), Sense::click_and_drag());
        let mut changed = false;
        if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
            let nv = ((track.max.y - pp.y) / track.height()).clamp(0.0, 1.0);
            changed = nv != *v;
            *v = nv;
        }
        let x = track.center().x;
        self.p.rect_filled(Rect::from_center_size(track.center(), vec2(4.0, track.height())), 2.0, Color32::from_gray(0x35));
        let y = track.max.y - *v * track.height();
        self.p.rect_filled(Rect::from_min_max(pos2(x - 2.0, y), pos2(x + 2.0, track.max.y)), 2.0, yellow());
        let thumb = Rect::from_center_size(pos2(x, y), vec2(28.0, 11.0));
        self.p.rect_filled(thumb, 5.5, Color32::from_gray(0x50));
        self.p.hline(thumb.x_range().shrink(8.0), y, Stroke::new(1.5, yellow()));
        changed
    }

    /// A horizontal slider (LFO / Slides Amount).
    pub(crate) fn hslider(&self, ui: &mut egui::Ui, track: Rect, key: &str, label: &str, v: &mut f32) -> bool {
        self.p.text(pos2(track.min.x, track.min.y - 10.0), Align2::LEFT_CENTER, label, look::regular(13.0), label_col());
        self.p.text(pos2(track.max.x, track.min.y - 10.0), Align2::RIGHT_CENTER, format!("{:.0}", *v * 100.0), look::regular(13.0), yellow());
        let resp = ui.interact(track.expand2(vec2(6.0, 8.0)), self.id(key), Sense::click_and_drag());
        let mut changed = false;
        if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
            let nv = ((pp.x - track.min.x) / track.width()).clamp(0.0, 1.0);
            changed = nv != *v;
            *v = nv;
        }
        let y = track.center().y;
        self.p.rect_filled(Rect::from_center_size(track.center(), vec2(track.width(), 4.0)), 2.0, Color32::from_gray(0x35));
        let x = track.min.x + *v * track.width();
        self.p.rect_filled(Rect::from_min_max(pos2(track.min.x, y - 2.0), pos2(x, y + 2.0)), 2.0, yellow());
        self.p.circle_filled(pos2(x, y), 7.0, Color32::from_gray(0x50));
        self.p.circle_stroke(pos2(x, y), 7.0, Stroke::new(1.0, yellow()));
        changed
    }

    /// A segmented control (yellow selected segment).
    pub(crate) fn seg(&self, ui: &mut egui::Ui, r: Rect, key: &str, items: &[&str], cur: &mut usize) -> bool {
        self.p.rect_filled(r, r.height() / 2.0, panel_bg());
        let w = r.width() / items.len() as f32;
        let mut changed = false;
        for (i, n) in items.iter().enumerate() {
            let sr = Rect::from_min_size(pos2(r.min.x + i as f32 * w, r.min.y), vec2(w, r.height())).shrink(1.0);
            let on = *cur == i;
            if on {
                self.p.rect_filled(sr, sr.height() / 2.0, yellow());
            }
            self.p.text(sr.center(), Align2::CENTER_CENTER, *n, look::regular(13.0), if on { Color32::BLACK } else { label_col() });
            // a thin divider between two unselected segments
            if i > 0 && !on && *cur != i - 1 {
                self.p.vline(sr.min.x - 1.0, sr.y_range().shrink(5.0), Stroke::new(1.0, Color32::from_gray(0x44)));
            }
            if ui.interact(sr, self.id(key).with(i), Sense::click()).clicked() && !on {
                *cur = i;
                changed = true;
            }
        }
        changed
    }

    /// The More button (an ellipsis in a circle) at `c`; returns its response.
    pub(crate) fn more(&self, ui: &mut egui::Ui, c: Pos2, key: &str) -> egui::Response {
        let r = Rect::from_center_size(c, vec2(20.0, 20.0));
        let resp = ui.interact(r, self.id(key), Sense::click()).on_hover_text("More");
        glyphs::paint(&self.p, r, glyphs::ph::DOTS_THREE_CIRCLE, 19.0, if resp.hovered() { Color32::WHITE } else { label_col() });
        resp
    }

    /// A row of 16 pattern dots (bright: main accents, dim: probable ones).
    pub(crate) fn dots(&self, at: Pos2, dots: &synth::Dots, spacing: f32) {
        for (i, d) in dots.iter().enumerate() {
            let c = at + vec2(i as f32 * spacing + (i / 4) as f32 * spacing * 0.6, 0.0);
            let (rad, col) = match d {
                2 => (4.5, yellow()),
                1 => (3.5, yellow().gamma_multiply(0.45)),
                _ => (2.0, Color32::from_gray(0x55)),
            };
            self.p.circle_filled(c, rad, col);
        }
    }
}

/// The editor.
/// The editor's top: the title, the bar (Chords pop-up), the ruler with the Play button, the
/// region chords and the region (a drummer's hits by kit piece). Returns the controls' area.
pub(crate) fn top(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize, kit: Option<lg_players::kit::KitMap>, regen: &mut bool) -> Option<Rect> {
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.hline(area.x_range(), area.min.y, Stroke::new(1.0, look::line()));
    // ---- the title and the editor's bar
    // (Logic shows the title alone: ui-session-player-keys-details.png)
    p.text(pos2(area.center().x, area.min.y + TITLE_H / 2.0), Align2::CENTER_CENTER, "Session Player", look::semibold(13.0), look::text());
    let bar = Rect::from_min_size(area.min + vec2(0.0, TITLE_H), vec2(area.width(), BAR_H));
    p.hline(area.x_range(), bar.min.y, Stroke::new(1.0, look::line()));
    chords_popup(app, ui, &p, Rect::from_min_size(bar.min + vec2(10.0, 3.0), vec2(84.0, 24.0)), t, r, regen);
    // ---- the ruler with the Play button, the chords lane, the region
    let ruler = Rect::from_min_size(pos2(area.min.x, bar.max.y), vec2(area.width(), RULER_H));
    let region = app.project.as_ref().and_then(|pr| pr.tracks.get(t)?.regions.get(r)).cloned()?;
    let lane = Rect::from_min_max(pos2(area.min.x + 30.0, ruler.min.y), pos2(area.max.x - 10.0, ruler.max.y + CHORDS_H + REGION_H));
    p.rect_filled(Rect::from_min_max(ruler.min, pos2(ruler.max.x, ruler.max.y)), 0.0, look::header_bg());
    let bar_t = app.bar_ticks().max(1);
    let x_of = |tick: u32| lane.min.x + (tick.saturating_sub(region.start)) as f32 / region.length.max(1) as f32 * lane.width();
    let mut b = region.start - region.start % bar_t;
    while b < region.start + region.length {
        if b >= region.start {
            let x = x_of(b);
            p.vline(x, ruler.y_range(), Stroke::new(1.0, look::text_dim()));
            p.text(pos2(x + 3.0, ruler.center().y), Align2::LEFT_CENTER, format!("{}", b / bar_t + 1), look::regular(12.5), look::text());
        }
        b += bar_t;
    }
    // the preview Play button: the cycle set to the region, the region soloed
    let play = Rect::from_center_size(pos2(area.min.x + 15.0, ruler.center().y + 8.0), vec2(22.0, 22.0));
    let previewing = app.spx.preview.is_some();
    let presp = ui.interact(play, ui.scope_id().with(("sp-play", t, r)), Sense::click()).on_hover_text("Play the region (cycled, soloed)");
    glyphs::paint(&p, play, if previewing { glyphs::ph_fill::STOP } else { glyphs::ph_fill::PLAY }, 14.0, if previewing { yellow() } else { look::text() });
    if presp.clicked() {
        preview(app, t, r);
    }
    // region chords: shown when the region follows them
    let chords_lane = Rect::from_min_size(pos2(lane.min.x, ruler.max.y), vec2(lane.width(), CHORDS_H));
    if let Some((true, _)) = app.spx.region_chords.get(&(t, r)) {
        for c in crate::spx::region_chords(app, t, r) {
            if c.pos >= region.start && c.pos < region.start + region.length {
                let x = x_of(c.pos);
                p.vline(x, chords_lane.y_range(), Stroke::new(1.0, look::text_dim()));
                p.text(pos2(x + 3.0, chords_lane.center().y), Align2::LEFT_CENTER, c.chord.name(), look::regular(11.0), look::text());
            }
        }
    }
    let reg = Rect::from_min_size(pos2(lane.min.x, chords_lane.max.y), vec2(lane.width(), REGION_H - 4.0));
    let col = Color32::from_rgb(0xc3, 0xa6, 0x3a);
    p.rect_filled(reg, 4.0, col);
    p.rect_filled(Rect::from_min_size(reg.min, vec2(reg.width(), 14.0)), egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, Color32::from_rgb(0xfd, 0xf2, 0xc4));
    p.text(pos2(reg.min.x + 6.0, reg.min.y + 7.0), Align2::LEFT_CENTER, &region.name, look::regular(11.0), Color32::BLACK);
    let (lo, hi) = region.notes.iter().fold((127u8, 0u8), |a, n| (a.0.min(n.pitch), a.1.max(n.pitch)));
    let ink = look::mix(col, Color32::BLACK, 0.55);
    for n in &region.notes {
        let x0 = x_of(n.pos);
        let x1 = x_of(n.pos + n.len).max(x0 + 2.0);
        let y = match kit {
            // a drummer's hits by kit piece: cymbals high, kick low
            Some(k) => {
                use lg_players::kit::Family;
                let row = match k.piece(n.pitch).map(|pc| pc.family()) {
                    Some(Family::Kick) => 0.95,
                    Some(Family::Snare) => 0.7,
                    Some(Family::Tom) => 0.55,
                    Some(Family::HiHat) => 0.3,
                    Some(Family::Cymbal) => 0.1,
                    _ => 0.45,
                };
                reg.min.y + 16.0 + row * (reg.height() - 20.0)
            }
            None => reg.max.y - 4.0 - (n.pitch.saturating_sub(lo)) as f32 / (hi.saturating_sub(lo).max(1)) as f32 * (reg.height() - 22.0),
        };
        if kit.is_some() {
            // a drum hit: a triangle, taller when harder
            let hh = 2.0 + n.velocity as f32 / 127.0 * 5.0;
            p.add(Shape::convex_polygon(vec![pos2(x0, y - hh), pos2(x0 + 3.5, y), pos2(x0, y + hh)], ink, Stroke::NONE));
        } else {
            p.hline(x0..=x1, y, Stroke::new(1.5, ink));
        }
    }
    let body = Rect::from_min_max(pos2(area.min.x, reg.max.y + 6.0), area.max);
    p.hline(body.x_range(), body.min.y - 2.0, Stroke::new(1.0, look::line()));
    Some(body)
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect, t: usize, r: usize) {
    let Some(mut player) = app.players.get(&(t, r)).cloned() else { return };
    let before = player.clone();
    let mut regen = false;
    let Some(body) = top(app, ui, area, t, r, None, &mut regen) else { return };
    // the Session Player type and the Preset pop-ups (spx.rs), at the bar's right
    let head = Rect::from_min_max(pos2(area.max.x - 600.0, area.min.y + TITLE_H - 2.0), pos2(area.max.x, area.min.y + TITLE_H + BAR_H));
    let style_before = player.clone();
    crate::spx::header(app, ui, head, t, &mut player);
    // ---- the scrolled controls
    let mut pattern_changed = false;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
    egui::ScrollArea::vertical().id_salt(("sp-scroll", t, r)).auto_shrink([false, false]).show(&mut child, |ui| {
        let w = body.width();
        let top = ui.cursor().min;
        let cx = top.x + w / 2.0;
        let cxs = Cx { p: ui.painter().clone(), base: ui.scope_id().with(("spedit", t, r)) };
        // the Session Player button (type over style), the sliders, Regenerate
        let y0 = top.y + 22.0;
        let type_name = match &player {
            Player::Bass(b) if b.style.synth().is_some() => "Synth Bass",
            Player::Bass(_) => "Bass Player",
            Player::Keys(k) if k.style.synth().is_some() => "Synth Keyboard",
            Player::Keys(_) => "Keyboard Player",
        };
        style_button(ui, &cxs, pos2(cx - 83.0, y0), type_name, &mut player);
        let (cpx, ipy) = match &mut player {
            Player::Bass(b) => (&mut b.complexity, &mut b.intensity),
            Player::Keys(k) => (&mut k.complexity, &mut k.intensity),
        };
        cxs.label(pos2(cx + 21.0, y0), "Complexity");
        cxs.label(pos2(cx + 105.0, y0), "Intensity");
        let st = y0 + 24.0;
        cxs.vslider(ui, Rect::from_min_size(pos2(cx + 21.0 - 2.0, st), vec2(4.0, 83.0)), "cx", cpx);
        cxs.vslider(ui, Rect::from_min_size(pos2(cx + 105.0 - 2.0, st), vec2(4.0, 83.0)), "iy", ipy);
        let rg = Rect::from_center_size(pos2(cx - 83.0, st + 83.0), vec2(83.0, 24.0));
        let rresp = ui.interact(rg, cxs.id("regen"), Sense::click());
        cxs.p.rect_filled(rg, 6.0, Color32::from_gray(if rresp.hovered() { 0x4c } else { 0x41 }));
        cxs.p.text(rg.center(), Align2::CENTER_CENTER, "Regenerate", look::regular(13.0), look::text());
        if rresp.clicked() {
            player = match &player {
                Player::Bass(b) => Player::Bass(b.regenerated()),
                Player::Keys(k) => Player::Keys(k.regenerated()),
            };
        }
        // the tabs
        let tabs_c = st + 83.0 + 46.0;
        let mut tab = app.drum_tab.min(2);
        if cxs.seg(ui, Rect::from_center_size(pos2(cx, tabs_c), vec2(171.0, 24.0)), "tabs", &["Main", "Details", "Manual"], &mut tab) {
            app.drum_tab = tab;
        }
        // the panel
        let pw = PANEL_W.min(w - 16.0);
        let panel_top = tabs_c + 23.0;
        let bg = cxs.p.add(Shape::Noop);
        let px = cx - pw / 2.0;
        let h = match tab {
            0 => main_panel(app, ui, &cxs, pos2(px, panel_top), pw, t, r, &mut player, &mut pattern_changed),
            1 => details_panel(ui, &cxs, pos2(px, panel_top), pw, &mut player),
            _ => {
                let mr = Rect::from_min_size(pos2(px + 16.0, panel_top + 16.0), vec2(pw - 32.0, 170.0));
                pattern_changed |= crate::spx::manual(app, ui, mr, t, r);
                200.0
            }
        };
        cxs.p.set(bg, Shape::rect_filled(Rect::from_min_size(pos2(px, panel_top), vec2(pw, h)), PANEL_R, panel_bg()));
        ui.allocate_exact_size(vec2(w, panel_top + h + 24.0 - top.y), Sense::hover());
    });
    if regen {
        crate::players::regenerate(app, t, r);
    }
    if player.style_name() != style_before.style_name() && std::mem::discriminant(&player) == std::mem::discriminant(&style_before) {
        player = crate::spx::style_changed(app, &style_before, &player);
        if let Some(rg) = app.project.as_mut().and_then(|pr| pr.tracks.get_mut(t)?.regions.get_mut(r)) {
            rg.name = player.region_name();
        }
    }
    if pattern_changed && player == before {
        app.checkpoint("Session Player Pattern");
        crate::players::regenerate(app, t, r);
        app.dirty = true;
    }
    if player != before {
        app.checkpoint("Change Session Player");
        app.players.insert((t, r), player);
        crate::players::regenerate(app, t, r);
        app.dirty = true;
    }
    patterns_dialog(app, ui, t, r);
    chord_editor(app, ui, t, r);
}

/// The Session Player button: the type above the style (yellow); a click opens the Session
/// Player dialog's choice: every style by group (Synth styles under Synth).
fn style_button(ui: &mut egui::Ui, cx: &Cx, c: Pos2, type_name: &str, player: &mut Player) {
    cx.label(c, type_name);
    let style = match player {
        Player::Bass(b) => b.style.name().to_string(),
        Player::Keys(k) => k.style.name().to_string(),
    };
    let r = Rect::from_center_size(c + vec2(0.0, 9.0), vec2(150.0, 38.0));
    let resp = ui.interact(r, cx.id("style"), Sense::click()).on_hover_text("Session Player: choose the style");
    cx.p.text(c + vec2(0.0, 18.0), Align2::CENTER_CENTER, style, look::regular(13.0), if resp.hovered() { Color32::WHITE } else { yellow() });
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(220.0);
        match player {
            Player::Bass(b) => {
                for g in ["Electric Bass", "Upright Bass", "Synth Bass"] {
                    ui.label(egui::RichText::new(if g == "Synth Bass" { "Synth" } else { g }).small().color(look::text_dim()));
                    for s in BassStyle::ALL.into_iter().filter(|s| s.group() == g) {
                        if ui.add(egui::Button::selectable(b.style == s, s.name())).clicked() {
                            b.style = s;
                            if s.synth().is_some() {
                                b.slide_mode = SlideMode::PitchBend;
                            }
                            ui.close();
                        }
                    }
                }
            }
            Player::Keys(k) => {
                for g in ["Acoustic Piano", "Keyboard", "Synth Keyboard"] {
                    ui.label(egui::RichText::new(if g == "Synth Keyboard" { "Synth" } else { g }).small().color(look::text_dim()));
                    for s in KeysStyle::ALL.into_iter().filter(|s| s.group() == g) {
                        if ui.add(egui::Button::selectable(k.style == s, s.name())).clicked() {
                            k.style = s;
                            ui.close();
                        }
                    }
                }
            }
        }
    });
}

/// The editor bar's Chords pop-up: which chords the region follows, the region chords.
fn chords_popup(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, t: usize, reg: usize, regen: &mut bool) {
    let resp = ui.interact(r, ui.scope_id().with(("sp-chords", t, reg)), Sense::click());
    p.rect_filled(r, 6.0, if resp.hovered() { look::button_on() } else { look::group_bg() });
    p.text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, "Chords", look::regular(13.0), look::text());
    glyphs::paint(p, Rect::from_center_size(pos2(r.max.x - 12.0, r.center().y), vec2(12.0, 12.0)), glyphs::ph::CARET_DOWN, 11.0, look::text());
    let follows = matches!(app.spx.region_chords.get(&(t, reg)), Some((true, _)));
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_min_width(240.0);
        if ui.add(egui::Button::selectable(!follows, "Follow Chord Track")).clicked() {
            if let Some(e) = app.spx.region_chords.get_mut(&(t, reg)) {
                e.0 = false;
            }
            *regen = true;
            ui.close();
        }
        if ui.add(egui::Button::selectable(follows, "Follow Region Chords")).clicked() {
            let text = app.spx.region_chords.get(&(t, reg)).map(|x| x.1.clone()).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| if app.chord_text.trim().is_empty() { crate::spx::default_progression(app) } else { app.chord_text.clone() });
            app.spx.region_chords.insert((t, reg), (true, text));
            *regen = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Edit Region Chords…").clicked() {
            app.spx.chord_edit = Some(app.spx.region_chords.get(&(t, reg)).map(|x| x.1.clone()).unwrap_or_default());
            ui.close();
        }
        ui.menu_button("Chord Progressions", |ui| {
            let (key, minor) = crate::spx::project_key(app);
            let major = if minor { (key + 3) % 12 } else { key };
            for pr in lg_players::chords::Progression::ALL {
                if ui.button(pr.name()).clicked() {
                    let text = pr.chords(major, 0, 1, pr.degrees().len() as u32).iter().map(|c| c.chord.name()).collect::<Vec<_>>().join(" ");
                    app.spx.region_chords.insert((t, reg), (true, text));
                    *regen = true;
                    ui.close();
                }
            }
        });
        if ui.add_enabled(!app.chord_text.trim().is_empty(), egui::Button::new("Copy Chord Track to Region Chords")).clicked() {
            app.spx.region_chords.insert((t, reg), (true, app.chord_text.clone()));
            *regen = true;
            ui.close();
        }
        if ui.add_enabled(follows, egui::Button::new("Copy Region Chords to Chord Track")).clicked() {
            if let Some((_, s)) = app.spx.region_chords.get(&(t, reg)) {
                app.chord_text = s.clone();
            }
            crate::players::regenerate_all(app);
            ui.close();
        }
        if ui.add_enabled(app.spx.region_chords.contains_key(&(t, reg)), egui::Button::new("Clear Region Chords")).clicked() {
            app.spx.region_chords.remove(&(t, reg));
            *regen = true;
            ui.close();
        }
    });
    if *regen {
        app.checkpoint("Region Chords");
    }
}

/// Edit Region Chords: one chord name per bar.
fn chord_editor(app: &mut App, ui: &mut egui::Ui, t: usize, r: usize) {
    let Some(mut text) = app.spx.chord_edit.clone() else { return };
    let mut done = None;
    egui::Modal::new(egui::Id::unique(format!("sp-chord-edit-{t}-{r}"))).show(ui.ctx(), |ui| {
        ui.label(egui::RichText::new("Region Chords").strong());
        ui.label(egui::RichText::new("One chord a bar, from the region's start (C Am F G, F#m7, Bb/D …)").small().color(look::text_dim()));
        ui.text_edit_singleline(&mut text);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                done = Some(false);
            }
            if ui.button("OK").clicked() {
                done = Some(true);
            }
        });
    });
    app.spx.chord_edit = Some(text.clone());
    if let Some(ok) = done {
        app.spx.chord_edit = None;
        if ok {
            app.checkpoint("Region Chords");
            app.spx.region_chords.insert((t, r), (true, text.trim().to_string()));
            crate::players::regenerate(app, t, r);
            app.dirty = true;
        }
    }
}

/// The preview Play button: plays the region cycled and soloed; again (or Stop) ends it.
pub fn preview(app: &mut App, t: usize, r: usize) {
    if let Some((solo, soloed, on, cyc)) = app.spx.preview.take() {
        app.halt();
        app.rp.region_solo = solo;
        app.rp.soloed = soloed;
        app.set_cycle(cyc);
        if app.cycle_on() != on {
            app.toggle_cycle();
        }
        app.load_engine();
        return;
    }
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)) else { return };
    let (a, b) = (rg.start, rg.start + rg.length);
    app.spx.preview = Some((app.rp.region_solo, app.rp.soloed.clone(), app.cycle_on(), app.cycle));
    app.rp.region_solo = true;
    app.rp.soloed = vec![(t, r)];
    app.set_cycle((a, b));
    if !app.cycle_on() {
        app.toggle_cycle();
    }
    app.load_engine();
    app.locate(a as f64);
    app.play_stop();
}

/// The Main panel; returns its height.
#[allow(clippy::too_many_arguments)]
fn main_panel(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, t: usize, r: usize, player: &mut Player, pattern_changed: &mut bool) -> f32 {
    let mid = o.x + w / 2.0;
    let synth_views: Option<&[&str]> = match player {
        Player::Bass(b) if b.style.synth().is_some() => Some(&["Main", "LFO", "Slides"]),
        Player::Keys(k) if k.style == KeysStyle::SynthModulatedPad => Some(&["Main", "LFO", "Envelope"]),
        Player::Keys(k) if k.style == KeysStyle::SynthRhythmic => Some(&["Main", "LFO"]),
        _ => None,
    };
    let mut y = o.y + 18.0;
    if let Some(views) = synth_views {
        let mut v = (app.spx.view as usize).min(views.len() - 1);
        if cx.seg(ui, Rect::from_center_size(pos2(mid, y + 4.0), vec2(70.0 * views.len() as f32, 22.0)), "views", views, &mut v) {
            app.spx.view = v as u8;
        }
        y += 30.0;
        if v == 1 {
            let pads = matches!(player, Player::Keys(k) if k.style == KeysStyle::SynthModulatedPad);
            let lfo = match player {
                Player::Bass(b) => &mut b.synth.lfo,
                Player::Keys(k) => &mut k.synth.lfo,
            };
            return lfo_view(ui, cx, pos2(o.x, y), w, lfo, pads) + (y - o.y);
        }
        if v == 2 {
            return match player {
                Player::Bass(b) => slides_view(ui, cx, pos2(o.x, y), w, &mut b.synth.slides),
                Player::Keys(k) => envelope_view(app, ui, cx, pos2(o.x, y), w, &mut k.synth.envelope, &mut k.synth.pattern),
            } + (y - o.y);
        }
    }
    // the pattern (click: the Patterns dialog)
    let pads = matches!(player, Player::Keys(k) if matches!(k.style, KeysStyle::SynthModulatedPad | KeysStyle::SynthSimplePad));
    if !pads {
        let (kind, sel) = pattern_of(player);
        let custom = app.spx.patterns.get(&(t, r)).is_some_and(|p| p.steps.iter().any(|s| *s));
        let list = synth::patterns(kind);
        // (the style's own pattern when none is chosen)
        let own = match &*player {
            Player::Bass(b) if b.style == BassStyle::Synth808 => 5,
            Player::Bass(b) if b.style == BassStyle::SynthPump => 3,
            Player::Keys(k) if k.style == KeysStyle::SynthRhythmic => 3,
            _ => 0,
        };
        let dots = sel.and_then(|i| list.get(i)).map_or(list[own.min(list.len() - 1)].1, |p| p.1);
        let name = if custom { "Custom".to_string() } else { sel.and_then(|i| list.get(i)).map_or("Style Default".to_string(), |p| p.0.to_string()) };
        let pr = Rect::from_min_size(pos2(o.x + 22.0, y - 10.0), vec2(w - 44.0, 28.0));
        let resp = ui.interact(pr, cx.id("patterns-btn"), Sense::click()).on_hover_text("Patterns");
        if resp.hovered() {
            cx.p.rect_filled(pr, 6.0, Color32::from_gray(0x22));
        }
        cx.p.text(pos2(pr.min.x + 8.0, pr.center().y), Align2::LEFT_CENTER, "Patterns", look::regular(13.0), label_col());
        cx.dots(pos2(pr.min.x + 92.0, pr.center().y), &dots, 13.0);
        cx.p.text(pos2(pr.max.x - 8.0, pr.center().y), Align2::RIGHT_CENTER, name, look::regular(12.0), yellow());
        if resp.clicked() {
            app.spx.patterns_open = true;
        }
        y += 34.0;
    }
    let pct = |v: f32| format!("{:.0}", v * 100.0);
    let cols = [o.x + w * 0.18, o.x + w * 0.39, o.x + w * 0.61, o.x + w * 0.82];
    match player {
        Player::Bass(b) if b.style.synth().is_some() => {
            // the XY pad: right adds notes beyond the root, up more octave moves
            let pad = Rect::from_min_size(pos2(o.x + 24.0, y + 4.0), vec2(190.0, 120.0));
            cx.p.rect_filled(pad, 8.0, Color32::from_gray(0x1e));
            for k in 1..4 {
                cx.p.vline(pad.min.x + pad.width() * k as f32 / 4.0, pad.y_range(), Stroke::new(1.0, Color32::from_white_alpha(10)));
                cx.p.hline(pad.x_range(), pad.min.y + pad.height() * k as f32 / 4.0, Stroke::new(1.0, Color32::from_white_alpha(10)));
            }
            cx.p.text(pad.left_bottom() + vec2(6.0, -6.0), Align2::LEFT_BOTTOM, "Root", look::regular(10.5), look::text_dim());
            cx.p.text(pad.right_bottom() + vec2(-6.0, -6.0), Align2::RIGHT_BOTTOM, "More Notes", look::regular(10.5), look::text_dim());
            cx.p.text(pad.left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, "Octaves", look::regular(10.5), look::text_dim());
            let resp = ui.interact(pad, cx.id("xy"), Sense::click_and_drag());
            if let (true, Some(pp)) = (resp.dragged() || resp.clicked(), resp.interact_pointer_pos()) {
                b.synth.xy_notes = ((pp.x - pad.min.x) / pad.width()).clamp(0.0, 1.0);
                b.synth.xy_octaves = ((pad.max.y - pp.y) / pad.height()).clamp(0.0, 1.0);
            }
            let puck = pos2(pad.min.x + b.synth.xy_notes * pad.width(), pad.max.y - b.synth.xy_octaves * pad.height());
            cx.p.circle_filled(puck, 8.0, yellow());
            cx.p.circle_stroke(puck, 8.0, Stroke::new(2.0, Color32::WHITE));
            // the note range: two handles; drag between them to move both
            let rr = Rect::from_min_size(pos2(pad.min.x, pad.max.y + 30.0), vec2(pad.width(), 6.0));
            range_slider(ui, cx, rr, "range", &mut b.synth.range_low, &mut b.synth.range_high, 16, 72, "Note Range");
            let kx = pad.max.x + (o.x + w - pad.max.x) / 2.0;
            cx.knob(ui, pos2(kx - 60.0, y + 10.0), "fills", "Fill Amount", &mut b.fills, 0.0, 1.0, 0.0, &pct);
            cx.knob(ui, pos2(kx + 60.0, y + 10.0), "swing", "Swing", &mut b.swing, 0.0, 1.0, 0.0, &pct);
            swing_type(ui, cx, pos2(kx + 60.0, y + 100.0), &mut b.swing_type);
            locks(app, ui, cx, pos2(kx - 60.0, y + 100.0));
            y + 186.0 - o.y
        }
        Player::Keys(k) if matches!(k.style, KeysStyle::SynthModulatedPad | KeysStyle::SynthSimplePad) => {
            // Tie Notes, Note Start, Note Length
            let mut tie = k.synth.tie_notes as usize;
            if cx.popup(ui, pos2(cols[0], y + 4.0), "tie", "Tie Notes", &["Off", "On"], &mut tie) {
                k.synth.tie_notes = tie == 1;
            }
            cx.knob(ui, pos2(cols[1] + 20.0, y + 4.0), "nstart", "Note Start", &mut k.synth.note_start, -1.0, 1.0, 0.0, &|v| if v.abs() < 0.02 { "On Beat".into() } else if v < 0.0 { format!("Ahead {:.0}", -v * 100.0) } else { format!("Behind {:.0}", v * 100.0) });
            cx.knob(ui, pos2(cols[2] + 20.0, y + 4.0), "nlen", "Note Length", &mut k.synth.note_length, 0.1, 1.0, 1.0, &pct);
            *pattern_changed |= crate::spx::hand_buttons(app, ui, pos2(mid, y + 130.0), t, r);
            y + 160.0 - o.y
        }
        Player::Bass(b) => {
            let (c1, c2) = (o.x + w * 0.3, o.x + w * 0.7);
            cx.knob(ui, pos2(c1, y + 6.0), "fills", "Fill Amount", &mut b.fills, 0.0, 1.0, 0.0, &pct);
            locks(app, ui, cx, pos2(c1, y + 96.0));
            cx.knob(ui, pos2(c2, y + 6.0), "swing", "Swing", &mut b.swing, 0.0, 1.0, 0.0, &pct);
            swing_type(ui, cx, pos2(c2, y + 96.0), &mut b.swing_type);
            let row = y + 130.0;
            let opts = b.style.melody_options();
            let mut m = b.melody as usize;
            if cx.popup(ui, pos2(o.x + w * 0.2, row), "melody", "Melody", opts, &mut m) {
                b.melody = m as u8;
            }
            let mut oc = b.octaves as usize;
            if cx.popup(ui, pos2(o.x + w * 0.5, row), "octaves", "Octaves", &OCTAVE_OPTIONS, &mut oc) {
                b.octaves = oc as u8;
            }
            let mut ph = b.phrasing as usize;
            if cx.popup(ui, pos2(o.x + w * 0.8, row), "phrasing", "Phrasing", &PHRASING_OPTIONS, &mut ph) {
                b.phrasing = ph as u8;
            }
            row + 56.0 - o.y
        }
        Player::Keys(k) => {
            cx.knob(ui, pos2(cols[0], y + 6.0), "fills", "Fill Amount", &mut k.fills, 0.0, 1.0, 0.0, &pct);
            locks(app, ui, cx, pos2(cols[0], y + 96.0));
            cx.knob(ui, pos2(cols[1], y + 6.0), "swing", "Swing", &mut k.swing, 0.0, 1.0, 0.0, &pct);
            swing_type(ui, cx, pos2(cols[1], y + 96.0), &mut k.swing_type);
            // the hands: Left / Right Hand buttons and the hand-position range (low keys left)
            *pattern_changed |= crate::spx::hand_buttons(app, ui, pos2(o.x + w * 0.73, y + 14.0), t, r);
            let rr = Rect::from_min_size(pos2(o.x + w * 0.52, y + 70.0), vec2(w * 0.42, 6.0));
            let (mut lo, mut hi) = (k.lower_position, k.upper_position);
            if range_slider(ui, cx, rr, "hands", &mut lo, &mut hi, 41, 84, "Left Hand · Right Hand") {
                k.lower_position = lo;
                k.upper_position = hi;
            }
            y + 136.0 - o.y
        }
    }
}

/// The pattern kind and chosen pattern of a player.
fn pattern_of(player: &Player) -> (PatternKind, Option<usize>) {
    match player {
        Player::Bass(b) if b.style == BassStyle::SynthSequenced => (PatternKind::Sequence, b.synth.pattern),
        Player::Bass(b) if b.style.synth().is_some() => (PatternKind::Bass, b.synth.pattern),
        Player::Bass(b) => (PatternKind::Bass, b.pattern),
        Player::Keys(k) if k.style.synth().is_some() => (PatternKind::Keys, k.synth.pattern),
        Player::Keys(k) => (PatternKind::Keys, k.pattern),
    }
}

fn set_pattern(player: &mut Player, v: Option<usize>) {
    match player {
        Player::Bass(b) if b.style.synth().is_some() => b.synth.pattern = v,
        Player::Bass(b) => b.pattern = v,
        Player::Keys(k) if k.style.synth().is_some() => k.synth.pattern = v,
        Player::Keys(k) => k.pattern = v,
    }
}

pub(crate) fn swing_type(ui: &mut egui::Ui, cx: &Cx, c: Pos2, st: &mut SwingType) {
    let mut i = if *st == SwingType::Eighths { 0 } else { 1 };
    if cx.seg(ui, Rect::from_center_size(c, vec2(84.0, 20.0)), "swingtype", &["8th", "16th"], &mut i) {
        *st = if i == 0 { SwingType::Eighths } else { SwingType::Sixteenths };
    }
}

/// The Fill Amount / Swing locks (kept when presets or styles change).
pub(crate) fn locks(app: &mut App, ui: &mut egui::Ui, cx: &Cx, c: Pos2) {
    for (i, (name, on)) in [("Fills", app.spx.lock_fills), ("Swing", app.spx.lock_swing)].into_iter().enumerate() {
        let r = Rect::from_center_size(c + vec2(if i == 0 { -16.0 } else { 16.0 }, 0.0), vec2(26.0, 20.0));
        let resp = ui.interact(r, cx.id("lock").with(i), Sense::click()).on_hover_text(format!("Lock {name}: kept when switching presets or styles"));
        glyphs::paint(&cx.p, r, if on { glyphs::ph::LOCK } else { glyphs::ph::LOCK_OPEN }, 14.0, if on { yellow() } else { look::text_dim() });
        if resp.clicked() {
            if i == 0 {
                app.spx.lock_fills = !app.spx.lock_fills;
            } else {
                app.spx.lock_swing = !app.spx.lock_swing;
            }
        }
    }
}

/// Two handles on one track (a low and a high MIDI note); drag the space between them to move
/// both.
#[allow(clippy::too_many_arguments)]
fn range_slider(ui: &mut egui::Ui, cx: &Cx, r: Rect, key: &str, lo: &mut u8, hi: &mut u8, min: u8, max: u8, label: &str) -> bool {
    let span = (max - min) as f32;
    let x_of = |n: u8| r.min.x + (n.clamp(min, max) - min) as f32 / span * r.width();
    let n_of = |x: f32| (min as f32 + ((x - r.min.x) / r.width()).clamp(0.0, 1.0) * span).round() as u8;
    cx.p.text(pos2(r.min.x, r.min.y - 12.0), Align2::LEFT_CENTER, label, look::regular(12.0), label_col());
    cx.p.text(pos2(r.max.x, r.min.y - 12.0), Align2::RIGHT_CENTER, format!("{} – {}", crate::app::note_name(*lo), crate::app::note_name(*hi)), look::regular(12.0), yellow());
    cx.p.rect_filled(Rect::from_center_size(r.center(), vec2(r.width(), 4.0)), 2.0, Color32::from_gray(0x35));
    let (xl, xh) = (x_of(*lo), x_of(*hi));
    cx.p.rect_filled(Rect::from_min_max(pos2(xl, r.center().y - 2.0), pos2(xh, r.center().y + 2.0)), 2.0, yellow());
    let mut changed = false;
    let hl = Rect::from_center_size(pos2(xl, r.center().y), vec2(14.0, 18.0));
    let hh = Rect::from_center_size(pos2(xh, r.center().y), vec2(14.0, 18.0));
    let mid = Rect::from_min_max(pos2(xl + 7.0, r.center().y - 9.0), pos2((xh - 7.0).max(xl + 8.0), r.center().y + 9.0));
    let rl = ui.interact(hl, cx.id(key).with("lo"), Sense::drag());
    let rh = ui.interact(hh, cx.id(key).with("hi"), Sense::drag());
    let rm = ui.interact(mid, cx.id(key).with("mid"), Sense::drag());
    if let (true, Some(pp)) = (rl.dragged(), rl.interact_pointer_pos()) {
        let n = n_of(pp.x).min(hi.saturating_sub(5));
        changed |= n != *lo;
        *lo = n;
    }
    if let (true, Some(pp)) = (rh.dragged(), rh.interact_pointer_pos()) {
        let n = n_of(pp.x).max(*lo + 5);
        changed |= n != *hi;
        *hi = n;
    }
    if rm.dragged() {
        let d = (rm.drag_delta().x / r.width() * span).round() as i32;
        let acc = ui.ctx().data_mut(|m| {
            let a: &mut f32 = m.get_temp_mut_or_default(cx.id(key).with("acc"));
            *a += rm.drag_delta().x / r.width() * span - d as f32;
            *a
        });
        let d = d + if acc.abs() >= 1.0 { acc.signum() as i32 } else { 0 };
        if acc.abs() >= 1.0 {
            ui.ctx().data_mut(|m| m.insert_temp(cx.id(key).with("acc"), acc - acc.signum()));
        }
        let d = d.clamp(min as i32 - *lo as i32, max as i32 - *hi as i32);
        if d != 0 {
            *lo = (*lo as i32 + d) as u8;
            *hi = (*hi as i32 + d) as u8;
            changed = true;
        }
    }
    for h in [hl, hh] {
        cx.p.rect_filled(h, 4.0, Color32::from_gray(0x50));
        cx.p.vline(h.center().x, h.y_range().shrink(4.0), Stroke::new(1.5, yellow()));
    }
    changed
}

/// The LFO view; returns its height.
fn lfo_view(ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, lfo: &mut Lfo, pads: bool) -> f32 {
    let y = o.y + 6.0;
    cx.hslider(ui, Rect::from_min_size(pos2(o.x + 24.0, y + 22.0), vec2(190.0, 6.0)), "lfo-amt", "Amount", &mut lfo.amount);
    // the waveform buttons
    for (i, wv) in Wave::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(o.x + 24.0 + i as f32 * 48.0, y + 44.0), vec2(44.0, 26.0));
        let on = lfo.wave == wv;
        let resp = ui.interact(r, cx.id("lfo-wave").with(i), Sense::click()).on_hover_text(wv.name());
        cx.p.rect_filled(r, 6.0, if on { yellow() } else { button_bg() });
        let icon = match wv {
            Wave::Triangle => glyphs::ph::WAVE_TRIANGLE,
            Wave::Sine => glyphs::ph::WAVE_SINE,
            Wave::Square => glyphs::ph::WAVE_SQUARE,
            Wave::Random => glyphs::ph::SHUFFLE,
        };
        glyphs::paint(&cx.p, r, icon, 16.0, if on { Color32::BLACK } else { label_col() });
        if resp.clicked() {
            lfo.wave = wv;
        }
    }
    // the drawn waveform
    let g = Rect::from_min_size(pos2(o.x + 24.0, y + 82.0), vec2(190.0, 64.0));
    cx.p.rect_filled(g, 6.0, Color32::from_gray(0x1e));
    let pts: Vec<Pos2> = (0..=96).map(|k| {
        let x = k as f32 / 96.0;
        let v = lfo.wave_at(x * 2.0 + lfo.phase, ((x * 2.0 + lfo.phase).floor() * 0.37).fract());
        let v = (0.5 + 0.5 * v * lfo.amount.max(0.05) + lfo.offset * 0.5).clamp(0.0, 1.0);
        pos2(g.min.x + x * g.width(), g.max.y - 4.0 - v * (g.height() - 8.0))
    }).collect();
    cx.p.add(Shape::line(pts, Stroke::new(1.5, yellow())));
    // Phase, Symmetry, Rate, Sync
    let rx = o.x + w * 0.62;
    cx.value(ui, pos2(rx, y + 18.0), "lfo-phase", "Phase", &mut lfo.phase, 0.0, 1.0, 0.0, &|v| format!("{:.0}°", v * 360.0));
    cx.value(ui, pos2(rx + 100.0, y + 18.0), "lfo-sym", "Symmetry", &mut lfo.symmetry, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
    let unit = lfo.sync.unit();
    let rate_max = if lfo.sync == Sync::Free { 20.0 } else { 16.0 };
    cx.knob(ui, pos2(rx, y + 70.0), "lfo-rate", "Rate", &mut lfo.rate, 0.05, rate_max, 1.0, &|v| format!("{v:.2} {unit}"));
    let mut si = Sync::ALL.iter().position(|s| *s == lfo.sync).unwrap_or(0);
    if cx.popup(ui, pos2(rx + 100.0, y + 70.0), "lfo-sync", "Sync", &Sync::ALL.map(|s| s.name()), &mut si) {
        lfo.sync = Sync::ALL[si];
    }
    let more = cx.more(ui, pos2(o.x + w - 22.0, o.y + 4.0), "lfo-more");
    egui::Popup::from_toggle_button_response(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(230.0);
        ui.add(egui::Slider::new(&mut lfo.curve, -1.0..=1.0).text("Curve"));
        ui.add_enabled(lfo.curve.abs() > 0.001, egui::Checkbox::new(&mut lfo.s_curve, "S Curve"));
        ui.add(egui::Slider::new(&mut lfo.offset, -1.0..=1.0).text("Offset"));
        if !pads {
            ui.add(egui::Slider::new(&mut lfo.to_velocity, -1.0..=1.0).text("To Velocity"));
            ui.add(egui::Slider::new(&mut lfo.to_length, -1.0..=1.0).text("To Note Length"));
        }
        cc_target(ui, "CC Target", &mut lfo.cc);
    });
    170.0
}

/// A MIDI CC pop-up.
pub(crate) fn cc_target(ui: &mut egui::Ui, label: &str, cc: &mut u8) {
    let names = |n: u8| match n {
        1 => "1 Modulation".to_string(),
        7 => "7 Volume".to_string(),
        10 => "10 Pan".to_string(),
        11 => "11 Expression".to_string(),
        71 => "71 Resonance".to_string(),
        74 => "74 Cutoff".to_string(),
        n => format!("{n}"),
    };
    egui::ComboBox::from_label(label).selected_text(names(*cc)).show_ui(ui, |ui| {
        for n in 0..=119u8 {
            ui.selectable_value(cc, n, names(n));
        }
    });
}

/// The Slides view (Synth Bass); returns its height.
fn slides_view(ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, s: &mut Slides) -> f32 {
    let y = o.y + 6.0;
    cx.hslider(ui, Rect::from_min_size(pos2(o.x + 24.0, y + 22.0), vec2(190.0, 6.0)), "sl-amt", "Amount", &mut s.amount);
    // the slide curve: drag it up or down to bend it
    let g = Rect::from_min_size(pos2(o.x + 24.0, y + 46.0), vec2(190.0, 90.0));
    cx.p.rect_filled(g, 6.0, Color32::from_gray(0x1e));
    let (c, sc) = if s.mode == SlideMode::StudioBass { (0.0, false) } else { (s.curve, s.s_curve) };
    let pts: Vec<Pos2> = (0..=48).map(|k| {
        let x = k as f32 / 48.0;
        pos2(g.min.x + 8.0 + x * (g.width() - 16.0), g.max.y - 8.0 - synth::shape(x, c, sc) * (g.height() - 16.0))
    }).collect();
    cx.p.add(Shape::line(pts, Stroke::new(2.0, yellow())));
    let resp = ui.interact(g, cx.id("sl-curve"), Sense::drag()).on_hover_text("Drag up or down to curve the slide");
    if resp.dragged() && s.mode != SlideMode::Overlap {
        s.curve = (s.curve + resp.drag_delta().y / 120.0).clamp(-1.0, 1.0);
    }
    let rx = o.x + w * 0.62;
    let mut time = s.time as f32;
    if cx.knob(ui, pos2(rx, y + 10.0), "sl-time", "Slide Time", &mut time, 10.0, 960.0, 120.0, &|v| if v >= 480.0 { format!("{:.2} beats", v / 960.0) } else { format!("{:.0} ticks", v) }) {
        s.time = time.round() as u32;
    }
    cx.toggle(ui, pos2(rx, y + 100.0), "sl-auto", "Auto", &mut s.auto);
    let mut leg = s.legato as f32;
    if cx.value(ui, pos2(rx + 100.0, y + 10.0), "sl-legato", "Legato", &mut leg, 0.0, 100.0, 100.0, &|v| format!("{v:.0}")) {
        s.legato = leg.round() as u8;
    }
    let mut st = SlideStart::ALL.iter().position(|x| *x == s.start).unwrap_or(0);
    if cx.popup(ui, pos2(rx + 100.0, y + 56.0), "sl-start", "Start", &SlideStart::ALL.map(|x| x.name()), &mut st) {
        s.start = SlideStart::ALL[st];
    }
    cx.value(ui, pos2(rx + 100.0, y + 100.0), "sl-cv", "Curve", &mut s.curve, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
    let mut sc = s.s_curve;
    if cx.toggle(ui, pos2(rx + 100.0, y + 146.0), "sl-s", "S Curve", &mut sc) {
        s.s_curve = sc;
    }
    let more = cx.more(ui, pos2(o.x + w - 22.0, o.y + 4.0), "sl-more");
    egui::Popup::from_toggle_button_response(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(240.0);
        egui::ComboBox::from_label("Slide Mode").selected_text(s.mode.name()).show_ui(ui, |ui| {
            for m in SlideMode::ALL {
                ui.selectable_value(&mut s.mode, m, m.name());
            }
        });
        let mono = ["Off", "On (Base Channel 1)", "On (Base Channel 16)"];
        egui::ComboBox::from_label("MIDI Mono Mode").selected_text(mono[s.mono_mode.min(2) as usize]).show_ui(ui, |ui| {
            for (i, n) in mono.iter().enumerate() {
                ui.selectable_value(&mut s.mono_mode, i as u8, *n);
            }
        });
        ui.add(egui::Slider::new(&mut s.bend_range, 0..=96).text("Pitch Bend Range"));
    });
    186.0
}

/// The Modulated Pad's Envelope view; returns its height.
fn envelope_view(app: &mut App, ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, e: &mut synth::Envelope, pattern: &mut Option<usize>) -> f32 {
    let y = o.y + 6.0;
    // the drawn envelope over one gap: yellow nodes (attack peak, hold end, decay end) drag
    // across; the hollow nodes on the attack and decay lines drag up or down for curvature
    let g = Rect::from_min_size(pos2(o.x + 24.0, y + 8.0), vec2(w - 48.0, 110.0));
    cx.p.rect_filled(g, 6.0, Color32::from_gray(0x1e));
    let gap = 960u32 * 2;
    let total = if e.length == EnvLength::Relative { 1.0 } else { gap as f32 / 960.0 };
    let xs = |v: f32| g.min.x + 8.0 + (v / total).clamp(0.0, 1.0) * (g.width() - 16.0);
    let (a, h, d) = e.segments(gap);
    let to_x = |ticks: u32| g.min.x + 8.0 + ticks as f32 / gap as f32 * (g.width() - 16.0);
    let pts: Vec<Pos2> = (0..=120).map(|k| {
        let x = k as u32 * gap / 120;
        pos2(to_x(x), g.max.y - 8.0 - e.level(x, gap) * (g.height() - 16.0))
    }).collect();
    cx.p.add(Shape::line(pts, Stroke::new(2.0, yellow())));
    let rel = e.length == EnvLength::Relative;
    let scale = if rel { (e.attack + e.hold + e.decay).max(1e-4) } else { 1.0 };
    let env = *e;
    let node_y = |x: u32| g.max.y - 8.0 - env.level(x, gap) * (g.height() - 16.0);
    let nodes = [(a, 0usize), (a + h, 1), (a + h + d, 2)];
    for (x, k) in nodes {
        let c = pos2(to_x(x), node_y(x.saturating_sub(1)));
        let resp = ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), cx.id("env-node").with(k), Sense::drag());
        if resp.dragged() {
            let dv = resp.drag_delta().x / (g.width() - 16.0) * if rel { scale } else { total };
            match k {
                0 => e.attack = (e.attack + dv).max(0.0),
                1 => e.hold = (e.hold + dv).max(0.0),
                _ => e.decay = (e.decay + dv).max(0.0),
            }
        }
        cx.p.circle_filled(c, 5.0, yellow());
    }
    let _ = xs;
    for (k, x) in [(0usize, a / 2), (1, a + h + d / 2)] {
        let c = pos2(to_x(x), node_y(x));
        let resp = ui.interact(Rect::from_center_size(c, vec2(14.0, 14.0)), cx.id("env-curve").with(k), Sense::drag());
        if resp.dragged() {
            let cv = if k == 0 { &mut e.attack_curve } else { &mut e.decay_curve };
            *cv = (*cv + resp.drag_delta().y / 80.0).clamp(-1.0, 1.0);
        }
        cx.p.circle_filled(c, 5.0, panel_bg());
        cx.p.circle_stroke(c, 5.0, Stroke::new(1.5, yellow()));
    }
    // Shift Start, Align, Invert, Length, Patterns
    let ry = g.max.y + 22.0;
    cx.label(pos2(o.x + w * 0.14, ry), "Shift Start");
    for (i, (s, dv)) in [("‹", -1i8), ("›", 1)].into_iter().enumerate() {
        let r = Rect::from_center_size(pos2(o.x + w * 0.14 - 14.0 + i as f32 * 28.0, ry + 20.0), vec2(24.0, 20.0));
        cx.p.rect_filled(r, 5.0, button_bg());
        cx.p.text(r.center(), Align2::CENTER_CENTER, s, look::regular(14.0), label_col());
        if ui.interact(r, cx.id("env-shift").with(i), Sense::click()).clicked() {
            e.shift = (e.shift + dv).clamp(-8, 8);
        }
    }
    let mut al = e.peak_on_step as usize;
    if cx.popup(ui, pos2(o.x + w * 0.36, ry), "env-align", "Align", &["Start on Step", "Peak on Step"], &mut al) {
        e.peak_on_step = al == 1;
    }
    cx.toggle(ui, pos2(o.x + w * 0.55, ry + 18.0), "env-inv", "Invert", &mut e.invert);
    let mut li = EnvLength::ALL.iter().position(|x| *x == e.length).unwrap_or(0);
    if cx.popup(ui, pos2(o.x + w * 0.78, ry), "env-len", "Length", &EnvLength::ALL.map(|x| x.name()), &mut li) {
        e.length = EnvLength::ALL[li];
    }
    // the envelope's Patterns button
    let pr = Rect::from_center_size(pos2(o.x + w / 2.0, ry + 58.0), vec2(120.0, 24.0));
    let presp = ui.interact(pr, cx.id("env-patterns"), Sense::click());
    cx.p.rect_filled(pr, 6.0, button_bg());
    let pname = pattern.and_then(|i| synth::patterns(PatternKind::Envelope).get(i)).map_or("Quarters", |p| p.0);
    cx.p.text(pr.center(), Align2::CENTER_CENTER, format!("Patterns: {pname}"), look::regular(12.0), label_col());
    egui::Popup::menu(&presp).show(|ui| {
        for (i, (n, _)) in synth::patterns(PatternKind::Envelope).iter().enumerate() {
            if ui.add(egui::Button::selectable(*pattern == Some(i), *n)).clicked() {
                *pattern = Some(i);
                ui.close();
            }
        }
    });
    let _ = app;
    let more = cx.more(ui, pos2(o.x + w - 22.0, o.y + 4.0), "env-more");
    egui::Popup::from_toggle_button_response(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(230.0);
        cc_target(ui, "CC Target", &mut e.cc);
        ui.add(egui::Slider::new(&mut e.offset, -1.0..=1.0).text("Offset"));
    });
    ry + 84.0 - o.y
}

/// The Details panel; returns its height.
fn details_panel(ui: &mut egui::Ui, cx: &Cx, o: Pos2, w: f32, player: &mut Player) -> f32 {
    let pct = |v: f32| format!("{:.0}", v * 100.0);
    let at = |fx: f32, dy: f32| pos2(o.x + fx / PANEL_W * w, o.y + dy);
    let top_h;
    match player {
        Player::Bass(b) if b.style.synth().is_some() => {
            let s = &mut b.synth;
            let mut pl = s.phrase_length as usize;
            if cx.popup(ui, at(90.0, 25.0), "phrase", "Phrase Length", &["Chord", "1 Bar", "2 Bars", "4 Bars", "8 Bars"], &mut pl) {
                s.phrase_length = pl as u8;
            }
            cx.value(ui, at(229.0, 25.0), "rhythm", "Rhythm", &mut s.rhythm, 0.0, 1.0, 0.3, &pct);
            cx.value(ui, at(370.0, 25.0), "octave", "Octave", &mut s.octave, 0.0, 1.0, 0.3, &pct);
            cx.value(ui, at(90.0, 75.0), "melody", "Melody", &mut s.melody, 0.0, 1.0, 0.3, &pct);
            match b.style {
                BassStyle::Synth808 => {
                    cx.knob(ui, at(229.0, 75.0), "repeats", "Repeats", &mut s.repeats, 0.0, 1.0, 0.2, &pct);
                    cx.knob(ui, at(370.0, 75.0), "triplets", "Triplets", &mut s.triplets, 0.0, 1.0, 0.0, &pct);
                }
                BassStyle::SynthPump => {
                    cx.knob(ui, at(229.0, 75.0), "pump-amt", "Amount", &mut s.pump_amount, 0.0, 1.0, 0.6, &pct);
                    cx.knob(ui, at(370.0, 75.0), "pump-shape", "Shape", &mut s.pump_shape, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
                }
                _ => {
                    cx.knob(ui, at(229.0, 75.0), "acc-vel", "Velocity", &mut s.acc_velocity, 0.0, 1.0, 0.5, &pct);
                    cx.knob(ui, at(370.0, 75.0), "acc-len", "Note Length", &mut s.acc_length, 0.0, 1.0, 0.3, &pct);
                    cx.knob(ui, at(90.0, 140.0), "acc-cc", "MIDI CC", &mut s.acc_cc, 0.0, 1.0, 0.0, &pct);
                    let mut cr = ChordResponse::ALL.iter().position(|x| *x == s.chord_response).unwrap_or(0);
                    if cx.popup(ui, at(300.0, 160.0), "chord-resp", "Chord Response", &ChordResponse::ALL.map(|x| x.name()), &mut cr) {
                        s.chord_response = ChordResponse::ALL[cr];
                    }
                }
            }
            top_h = if b.style == BassStyle::SynthSequenced { 236.0 } else { 176.0 };
            let more = cx.more(ui, at(432.0, 26.0), "more");
            egui::Popup::from_toggle_button_response(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(220.0);
                cc_target(ui, "CC Target", &mut b.synth.acc_cc_target);
            });
        }
        Player::Bass(b) => {
            cx.knob(ui, at(156.0, 25.0), "dead", "Dead Notes", &mut b.dead_notes, 0.0, 1.0, 0.0, &pct);
            cx.knob(ui, at(304.0, 25.0), "slides", "Slides", &mut b.slides, 0.0, 1.0, 0.5, &pct);
            cx.toggle(ui, at(90.0, 131.0), "pickup", "Pickup Hits", &mut b.pickup_hits);
            cx.toggle(ui, at(229.0, 131.0), "dynmute", "Dynamic Mute", &mut b.dynamic_mute);
            cx.toggle(ui, at(370.0, 131.0), "alignroot", "Align Root", &mut b.align_root);
            // the style's own: Double Stops, Buzz Trills, Laid Back, Blue Notes, Slaps
            let extras = b.style.extras();
            let mut slots = vec![at(90.0, 172.0), at(229.0, 172.0), at(370.0, 172.0)];
            if extras.contains(&Extra::DoubleStops) {
                cx.value(ui, slots.remove(0), "double", "Double Stops", &mut b.double_stops, 0.0, 1.0, 0.0, &pct);
            }
            cx.value(ui, slots.remove(0), "muteoff", "Mute Offset", &mut b.mute_offset, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
            let mut extra_slot = |key: &str, label: &str, v: &mut f32| {
                if let Some(c) = (!slots.is_empty()).then(|| slots.remove(0)) {
                    cx.value(ui, c, key, label, v, 0.0, 1.0, 0.0, &pct);
                }
            };
            for e in extras {
                match e {
                    Extra::BuzzTrills => extra_slot("buzz", "Buzz Trills", &mut b.buzz_trills),
                    Extra::LaidBack => extra_slot("laid", "Laid Back", &mut b.laid_back),
                    Extra::BlueNotes => extra_slot("blue", "Blue Notes", &mut b.blue_notes),
                    _ => {}
                }
            }
            if extras.contains(&Extra::Slaps) {
                cx.toggle(ui, at(229.0, 218.0), "slaps", "Slaps", &mut b.slaps);
            }
            top_h = if extras.contains(&Extra::Slaps) { 250.0 } else { 219.0 };
            let more = cx.more(ui, at(432.0, 26.0), "more");
            egui::Popup::from_toggle_button_response(&more).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(220.0);
                egui::ComboBox::from_label("Slide Mode").selected_text(b.slide_mode.name()).show_ui(ui, |ui| {
                    for m in SlideMode::ALL {
                        ui.selectable_value(&mut b.slide_mode, m, m.name());
                    }
                });
                let mut low = b.lowest_note as f32;
                if ui.add(egui::Slider::new(&mut low, 23.0..=52.0).text("Lowest Note").custom_formatter(|v, _| crate::app::note_name(v.round() as u8))).changed() {
                    b.lowest_note = low.round() as u8;
                }
            });
        }
        Player::Keys(k) => {
            let simple = k.style == KeysStyle::SynthSimplePad;
            let mut y = 25.0;
            if !simple {
                // (Simple Pad has no Grace Notes)
                cx.knob(ui, at(229.5, y), "grace", "Grace Notes", &mut k.grace_notes, 0.0, 1.0, 0.33, &pct);
                y += 116.0;
            }
            // the hands' voicings and styles
            let voicings = k.style.voicings();
            let mut rv = voicings.iter().position(|v| *v == k.rh_voicing).unwrap_or(0);
            let names: Vec<&str> = voicings.iter().map(|v| v.name()).collect();
            if cx.popup(ui, at(115.0, y), "rhv", "Right-Hand Voicing", &names, &mut rv) {
                k.rh_voicing = voicings[rv];
            }
            let mut mv = k.rh_movement as usize;
            if cx.popup(ui, at(344.0, y), "rhm", "Right-Hand Movement", &RH_MOVEMENT, &mut mv) {
                k.rh_movement = mv as u8;
            }
            y += 50.0;
            let lv_opts: Vec<(LeftHand, &str)> = LeftHand::VOICINGS.iter().copied().filter(|(l, _)| *l != LeftHand::Broken || k.style == KeysStyle::SynthRhythmic).collect();
            let mut lv = lv_opts.iter().position(|x| x.0 == k.left_hand).unwrap_or(0);
            if cx.popup(ui, at(115.0, y), "lhv", "Left-Hand Voicing", &lv_opts.iter().map(|x| x.1).collect::<Vec<_>>(), &mut lv) {
                k.left_hand = lv_opts[lv].0;
            }
            let n_styles = if k.style == KeysStyle::SynthRhythmic { 7 } else { 6 };
            let mut ls = (k.lh_style as usize).min(n_styles - 1);
            if cx.popup(ui, at(344.0, y), "lhs", "Left-Hand Style", &LH_STYLES[..n_styles], &mut ls) {
                k.lh_style = ls as u8;
            }
            y += 50.0;
            // the style's own
            match k.style {
                KeysStyle::FullChords => {
                    let mut ph = (k.phrasing as usize).min(2);
                    if cx.popup(ui, at(115.0, y), "phrasing", "Phrasing", &PHRASING, &mut ph) {
                        k.phrasing = ph as u8;
                    }
                    y += 50.0;
                }
                KeysStyle::Arpeggiated => {
                    let mut am = (k.arp_direction.max(1) - 1) as usize;
                    if cx.popup(ui, at(115.0, y), "arp", "Arpeggio Mode", &ARP_MODES, &mut am) {
                        k.arp_direction = am as u8 + 1;
                    }
                    y += 50.0;
                }
                KeysStyle::SynthRhythmic => {
                    let s = &mut k.synth;
                    cx.value(ui, at(90.0, y), "rhythm", "Rhythm", &mut s.rhythm, 0.0, 1.0, 0.3, &pct);
                    cx.value(ui, at(229.0, y), "sfills", "Fills", &mut s.fills, 0.0, 1.0, 0.2, &pct);
                    let mut ph = (s.phrasing as usize).min(2);
                    if cx.popup(ui, at(370.0, y), "sphr", "Phrasing", &["Staccato", "Mixed", "Long"], &mut ph) {
                        s.phrasing = ph as u8;
                    }
                    y += 50.0;
                    cx.knob(ui, at(156.0, y), "strum", "Strum", &mut s.strum, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
                    cx.knob(ui, at(304.0, y), "slen", "Note Length", &mut s.length_fine, -1.0, 1.0, 0.0, &|v| format!("{:+.0}", v * 100.0));
                    y += 112.0;
                }
                KeysStyle::SynthModulatedPad => {
                    let s = &mut k.synth;
                    cx.value(ui, at(90.0, y), "pvar", "Phrase Variation", &mut s.phrase_variation, 0.0, 1.0, 0.2, &pct);
                    y += 46.0;
                    cx.knob(ui, at(156.0, y), "strum", "Strum", &mut s.strum, -1.0, 1.0, 0.0, &|v| format!("{:.0}", v * 100.0));
                    cx.knob(ui, at(304.0, y), "svel", "Velocity", &mut s.velocity, 0.0, 1.0, 0.7, &pct);
                    y += 112.0;
                }
                _ => {}
            }
            top_h = y + 4.0;
        }
    }
    // the strip: Feel, Dynamics, Tempo, Humanize
    let sy = o.y + top_h;
    let strip = Rect::from_min_size(pos2(o.x, sy), vec2(w, 130.0));
    cx.p.rect_filled(strip, egui::CornerRadius { nw: 0, ne: 0, sw: PANEL_R as u8, se: PANEL_R as u8 }, strip_bg());
    let (feel, dynamics, tempo, humanize) = match player {
        Player::Bass(b) => (&mut b.feel, &mut b.dynamics, &mut b.tempo, &mut b.humanize),
        Player::Keys(k) => (&mut k.feel, &mut k.dynamics, &mut k.tempo, &mut k.humanize),
    };
    let sat = |fx: f32| pos2(o.x + fx / PANEL_W * w, sy + 21.0);
    cx.knob(ui, sat(80.0), "feel", "Feel", feel, -1.0, 1.0, 0.0, &|v| if v.abs() < 0.02 { "Center".into() } else if v > 0.0 { format!("Push {:.0}", v * 100.0) } else { format!("Pull {:.0}", -v * 100.0) });
    cx.p.text(sat(80.0) + vec2(-20.0, 87.0), Align2::CENTER_CENTER, "Pull", look::regular(12.0), look::text_dim());
    cx.p.text(sat(80.0) + vec2(20.0, 87.0), Align2::CENTER_CENTER, "Push", look::regular(12.0), look::text_dim());
    cx.knob(ui, sat(180.0), "dyn", "Dynamics", dynamics, 0.0, 2.0, 1.0, &|v| format!("{:.0} %", v * 100.0));
    let mut ti = TempoFeel::ALL.iter().position(|x| *x == *tempo).unwrap_or(1);
    if cx.popup(ui, sat(280.0), "tempo", "Tempo", &TempoFeel::ALL.map(|x| x.name()), &mut ti) {
        *tempo = TempoFeel::ALL[ti];
    }
    cx.knob(ui, sat(380.0), "human", "Humanize", humanize, 0.0, 1.0, 0.25, &pct);
    top_h + 130.0
}

/// The Patterns dialog: Studio's patterns as dot rows (bright: main accents, dim: probable
/// ones), choosing one turns a custom pattern off; Follow Rhythm Of (a track, the chords); the
/// style's extras (Add Notes, Steady 8ths).
fn patterns_dialog(app: &mut App, ui: &mut egui::Ui, t: usize, r: usize) {
    if !app.spx.patterns_open {
        return;
    }
    let Some(mut player) = app.players.get(&(t, r)).cloned() else { return };
    let before = player.clone();
    let (kind, sel) = pattern_of(&player);
    let tracks: Vec<(usize, String)> = app.project.as_ref().map(|p| p.tracks.iter().enumerate().filter(|(i, tr)| *i != t && tr.regions.iter().any(|x| !x.notes.is_empty())).map(|(i, tr)| (i, format!("{} {}", i + 1, tr.name))).collect()).unwrap_or_default();
    let mut follow = app.spx.follow_track.get(&(t, r)).copied();
    let follow_before = follow;
    let mut close = false;
    let mut clear_custom = false;
    egui::Modal::new(egui::Id::unique(format!("sp-patterns-{t}-{r}"))).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.label(egui::RichText::new("Patterns").strong());
        let list = synth::patterns(kind);
        let row = |ui: &mut egui::Ui, name: &str, dots: Option<&synth::Dots>, on: bool| -> bool {
            let (rect, resp) = ui.allocate_exact_size(vec2(400.0, 26.0), Sense::click());
            let p = ui.painter();
            if on || resp.hovered() {
                p.rect_filled(rect, 6.0, if on { Color32::from_gray(0x3a) } else { Color32::from_gray(0x2a) });
            }
            p.text(pos2(rect.min.x + 8.0, rect.center().y), Align2::LEFT_CENTER, name, look::regular(13.0), if on { yellow() } else { look::text() });
            if let Some(d) = dots {
                Cx { p: p.clone(), base: egui::Id::NULL }.dots(pos2(rect.min.x + 130.0, rect.center().y), d, 15.0);
            }
            resp.clicked()
        };
        if row(ui, "Style Default", None, sel.is_none()) {
            set_pattern(&mut player, None);
            clear_custom = true;
        }
        for (i, (n, d)) in list.iter().enumerate() {
            if row(ui, n, Some(d), sel == Some(i)) {
                set_pattern(&mut player, Some(i));
                clear_custom = true;
            }
        }
        ui.separator();
        ui.label(egui::RichText::new("Follow Rhythm Of").strong());
        egui::ComboBox::from_label("Track").selected_text(follow.and_then(|f| tracks.iter().find(|x| x.0 == f)).map_or("None".to_string(), |x| x.1.clone())).show_ui(ui, |ui| {
            ui.selectable_value(&mut follow, None, "None");
            for (i, n) in &tracks {
                ui.selectable_value(&mut follow, Some(*i), n);
            }
        });
        match &mut player {
            Player::Bass(b) => {
                ui.checkbox(&mut b.follow_chords, "Chords");
                if b.style == BassStyle::SynthSequenced {
                    let opts = ["Off", "8th Notes", "16th Notes"];
                    egui::ComboBox::from_label("Add Notes").selected_text(opts[b.synth.add_notes.min(2) as usize]).show_ui(ui, |ui| {
                        for (i, o) in opts.iter().enumerate() {
                            ui.selectable_value(&mut b.synth.add_notes, i as u8, *o);
                        }
                    });
                }
            }
            Player::Keys(k) => {
                ui.checkbox(&mut k.follow_chords, "Chords");
                if k.style == KeysStyle::FullChords {
                    ui.checkbox(&mut k.steady_8ths, "Steady 8ths");
                }
            }
        }
        ui.add_space(6.0);
        if ui.button("Done").clicked() {
            close = true;
        }
    });
    if close {
        app.spx.patterns_open = false;
    }
    if clear_custom {
        app.spx.patterns.remove(&(t, r));
    }
    if follow != follow_before {
        match follow {
            Some(f) => app.spx.follow_track.insert((t, r), f),
            None => app.spx.follow_track.remove(&(t, r)),
        };
    }
    if player != before || follow != follow_before || clear_custom {
        app.checkpoint("Session Player Pattern");
        app.players.insert((t, r), player);
        crate::players::regenerate(app, t, r);
        app.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_choice_is_per_kind() {
        let mut p = Player::Bass(lg_players::bass::BassParams::default());
        set_pattern(&mut p, Some(2));
        assert_eq!(pattern_of(&p), (PatternKind::Bass, Some(2)));
        if let Player::Bass(b) = &mut p {
            b.style = BassStyle::SynthSequenced;
        }
        assert_eq!(pattern_of(&p).0, PatternKind::Sequence);
        assert_eq!(pattern_of(&p).1, None, "the synth's own choice");
    }
}
