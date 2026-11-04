//! Swipe actions on the note list's rows (Notes on the iPhone and iPad, and on the Mac with a
//! two-finger swipe): a horizontal scroll over a row slides it aside and its buttons grow out from
//! under it. Right: Pin / Unpin. Left: Delete, Move, Share (in Recently Deleted: Delete, Move).
//! The row follows the fingers 1:1; let go past half the buttons, or flick, and it stays open on a
//! spring that carries the fingers' speed; swipe far enough and the outermost button takes the
//! whole row, and letting go runs it. As notes_mobile's SwipeRow (same spring and thresholds),
//! sized for the Mac's 56 pt rows.
//!
//! Trackpads (wheel events with phases), wheels without phases and fingers (kn_ui::touch turns a
//! finger's pan into the same wheel events) all come through `input`, which runs before the list's
//! scroller and takes the events of a horizontal swipe away from it: the first 6 pt decide the
//! axis, so scrolling the list never opens a row. A click elsewhere, Escape or scrolling closes it.

use crate::app::App;
use crate::icons::{draw, Glyph};
use crate::look;
use egui::{pos2, vec2, Align2, Color32, Rect, Vec2};

/// A button's slot and circle (the Mac's rows are 56 pt: the iPhone's 74 / 50 scaled to fit with
/// the name under the circle).
const SLOT: f32 = 62.0;
const CIRCLE: f32 = 34.0;
/// How far the fingers go before the swipe picks an axis (points).
const DECIDE: f32 = 6.0;
/// iOS's snap (notes_mobile): mass 1, stiffness 260, just short of critical damping.
const STIFFNESS: f32 = 260.0;
const DAMPING: f32 = 0.92;
/// A trackpad's scroll runs ahead of the fingers (its deltas are bigger and quicker than a
/// finger's on glass): scaled down. A finger on a touch screen moves the row 1:1.
const TRACKPAD_GAIN: f32 = 0.7;
/// Past fully open the row pulls back: each point of travel moves it this much at first, less
/// and less towards the row's edge (a rubber band that never reaches it).
const RUBBER: f32 = 0.3;
/// The full swipe: the fingers' travel past fully open, this share of the row's width (at least
/// FULL_MIN pt); let go of it again below FULL_KEEP of that.
const FULL_SHARE: f32 = 0.65;
const FULL_MIN: f32 = 120.0;
const FULL_KEEP: f32 = 0.75;
/// Open means slid more than this (points): a spring that ends a hair off 0 never counts.
const OPEN_MIN: f32 = 0.5;
/// After a swipe is let go, wheel events that don't start a new gesture are a coasting tail: left
/// alone this long, so they never take a row.
const COOLDOWN: f64 = 0.3;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Axis {
    Across,
    Along,
}

#[derive(Default)]
struct Gesture {
    acc: Vec2,
    axis: Option<Axis>,
    row: Option<String>,
    last_t: f64,
    /// when it last moved (not just an event)
    moved_t: f64,
    /// the device says when the fingers lift (a trackpad's phases; a finger on the screen doesn't),
    /// so stillness isn't a release
    phases: bool,
    touch: bool,
}

/// What a swipe button does.
pub struct Act {
    pub label: &'static str,
    pub glyph: Glyph,
    pub color: Color32,
    pub action: String,
}

#[derive(Default)]
pub struct Swipe {
    /// the rows as drawn last frame (unmoved), and this frame's as the list draws them
    rows: Vec<(String, Rect)>,
    next_rows: Vec<(String, Rect)>,
    /// the row slid aside, how far (+ right: its leading buttons), and how fast
    row: Option<String>,
    x: f32,
    /// the fingers' travel (after the trackpad's gain): x is this, rubber-banded past open
    raw: f32,
    vel: f32,
    /// when the last swipe was let go (its coasting is ignored)
    released_at: f64,
    /// the fingers are on it (a swipe under way), so it follows them
    dragging: bool,
    /// where the spring takes it once let go (None: following the fingers)
    target: Option<f32>,
    gesture: Option<Gesture>,
    /// a full swipe: the outermost button takes the row (0..1, animated), and runs when it lands
    committed: bool,
    full: f32,
    run_on_landing: Option<String>,
    width: f32,
    /// how many buttons the open row has on each side (leading, trailing)
    slots: (usize, usize),
    /// Move / Share's menu, open at a button: (what, note, where)
    pub menu: Option<(&'static str, String, egui::Pos2)>,
}

impl Swipe {
    /// How far `id`'s row is slid (0 for every other row).
    pub fn offset(&self, id: &str) -> f32 {
        if self.row.as_deref() == Some(id) { self.x } else { 0.0 }
    }

    /// (a row on its way shut isn't open: a click on it is the note's)
    pub fn is_open(&self, id: &str) -> bool {
        self.row.as_deref() == Some(id) && self.x.abs() > OPEN_MIN && self.target != Some(0.0)
    }

    /// The list drew `id`'s row at `r` (unmoved).
    pub fn drawn(&mut self, id: &str, r: Rect) {
        self.next_rows.push((id.to_string(), r));
    }

    pub fn close(&mut self) {
        if self.row.is_some() {
            self.target = Some(0.0);
            self.dragging = false;
            self.committed = false;
            self.run_on_landing = None;
        }
    }

    /// For NT_DUMP: the open row and its offset.
    pub fn describe(&self) -> String {
        let at = self.rows.iter().find(|(id, _)| Some(id) == self.row.as_ref()).map(|(_, r)| (r.min.x, r.min.y, r.max.x));
        format!("row={:?} x={:.1} open={} full={:.2} width={:.0} at={:?} menu={}", self.row, self.x, self.row.as_ref().is_some_and(|id| self.is_open(id)), self.full, self.width, at, self.menu.is_some())
    }
}

/// The row's buttons: (leading = right swipe, trailing = left swipe), outermost first.
pub fn actions(app: &App, id: &str) -> (Vec<Act>, Vec<Act>) {
    let Some(n) = app.note_by_id(id) else { return (Vec::new(), Vec::new()) };
    let red = Color32::from_rgb(0xeb, 0x44, 0x3e);
    let purple = Color32::from_rgb(0x7d, 0x7c, 0xec);
    let blue = Color32::from_rgb(0x34, 0x8e, 0xf7);
    let delete = Act { label: "Delete", glyph: Glyph::Trash, color: red, action: format!("delete:{id}") };
    let mv = Act { label: "Move", glyph: Glyph::Folder, color: purple, action: format!("swipe-move:{id}") };
    if n.deleted {
        return (Vec::new(), vec![delete, mv]);
    }
    let pin = Act { label: if n.pinned { "Unpin" } else { "Pin" }, glyph: Glyph::Pin, color: look::accent(), action: format!("pin:{id}") };
    let mut trailing = vec![delete, mv];
    if !n.locked {
        trailing.push(Act { label: "Share", glyph: Glyph::Share, color: blue, action: format!("swipe-share:{id}") });
    }
    (vec![pin], trailing)
}

/// Each frame, before the list scrolls: the wheel's (and fingers') horizontal swipes taken, the
/// spring moved on. Returns an action to run (a full swipe that landed).
pub fn input(app: &mut App, ctx: &egui::Context, area: Rect) -> Option<String> {
    let (now, dt, pointer, touching, pressed, escape) = ctx.input(|i| (i.time, i.stable_dt.clamp(0.001, 0.05), i.pointer.latest_pos(), i.any_touches(), i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten(), i.key_pressed(egui::Key::Escape)));
    let s = &mut app.swipe;
    s.rows = std::mem::take(&mut s.next_rows);
    s.width = area.width().max(1.0);
    let rows = s.rows.clone();
    // (the slid row's note no longer in the list, deleted or moved away: nothing left open)
    if !s.dragging && s.row.as_ref().is_some_and(|id| !rows.is_empty() && !rows.iter().any(|(r, _)| r == id)) {
        *s = Swipe { rows: std::mem::take(&mut s.rows), width: s.width, released_at: s.released_at, ..Default::default() };
    }
    let row_at = |p: egui::Pos2| rows.iter().find(|(_, r)| r.contains(p)).map(|(id, _)| id.clone());
    // the wheel's events, in order: which to take from the list's scroller
    let wheel: Vec<(usize, Vec2, egui::TouchPhase, egui::MouseWheelSource, egui::MouseWheelUnit, bool)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .enumerate()
            .filter_map(|(k, e)| match e {
                egui::Event::MouseWheel { unit, delta, phase, source, modifiers } => Some((k, *delta, *phase, *source, *unit, modifiers.command || modifiers.ctrl || modifiers.shift)),
                _ => None,
            })
            .collect()
    });
    let mut take: Vec<usize> = Vec::new();
    let mut release = false;
    let mut close = false;
    let mut ended = false;
    for (k, delta, phase, source, unit, modded) in wheel {
        // (a mouse wheel's notches and modified scrolls are the list's; only smooth, unmodified
        // scrolls over the list can swipe)
        if unit != egui::MouseWheelUnit::Point || modded || !pointer.is_some_and(|p| area.contains(p)) {
            continue;
        }
        if source == egui::MouseWheelSource::Momentum {
            // the fingers have lifted: a swipe settles on its own spring, its coasting dropped
            if s.gesture.as_ref().is_some_and(|g| g.axis == Some(Axis::Across)) {
                take.push(k);
                release = true;
            }
            continue;
        }
        let fresh = phase == egui::TouchPhase::Start || s.gesture.as_ref().is_none_or(|g| now - g.last_t > 0.15);
        // (what coasts on after a swipe's fingers lift, without a new start: not a swipe)
        if fresh && phase != egui::TouchPhase::Start && !touching && now - s.released_at < COOLDOWN {
            // (and the cooldown lasts as long as the tail does)
            s.released_at = now;
            continue;
        }
        if fresh {
            if s.gesture.as_ref().is_some_and(|g| g.axis == Some(Axis::Across)) {
                release = true;
            }
            s.gesture = Some(Gesture { row: pointer.and_then(row_at), last_t: now, phases: phase == egui::TouchPhase::Start && !touching, touch: touching, ..Default::default() });
        }
        let Some(g) = s.gesture.as_mut() else { continue };
        g.last_t = now;
        if delta != Vec2::ZERO {
            g.moved_t = now;
        }
        g.touch |= touching;
        match g.axis {
            None => {
                g.acc += delta;
                if g.acc.length() > DECIDE {
                    let across = g.acc.x.abs() > g.acc.y.abs() && g.row.is_some();
                    g.axis = Some(if across { Axis::Across } else { Axis::Along });
                    if across {
                        take.push(k);
                        // (another row open: it shuts as this one opens)
                        if s.row != g.row {
                            s.x = 0.0;
                            s.full = 0.0;
                            s.committed = false;
                            s.run_on_landing = None;
                        }
                        s.row = g.row.clone();
                        s.target = None;
                        s.vel = 0.0;
                        // (taking an open row again: from where it is)
                        s.raw = s.x;
                        s.dragging = true;
                        let gain = if g.touch { 1.0 } else { TRACKPAD_GAIN };
                        let dx = g.acc.x * gain;
                        follow(s, dx, dt);
                    } else {
                        close = true;
                    }
                }
            }
            Some(Axis::Across) => {
                take.push(k);
                let gain = if g.touch { 1.0 } else { TRACKPAD_GAIN };
                follow(s, delta.x * gain, dt);
            }
            Some(Axis::Along) => {}
        }
        if matches!(phase, egui::TouchPhase::End | egui::TouchPhase::Cancel) {
            if s.gesture.as_ref().is_some_and(|g| g.axis == Some(Axis::Across)) {
                release = true;
            }
            ended = true;
        }
    }
    // (no word of the fingers lifting: a short stillness ends it, unless a finger is still on the
    // glass or the trackpad will say when they lift)
    if let Some(g) = &s.gesture {
        let quiet = now - g.last_t;
        if g.axis == Some(Axis::Across) && ((!g.phases && !touching && quiet > 0.12) || quiet > 1.5) {
            release = true;
        }
        if quiet > 1.5 {
            s.gesture = None;
        }
    }
    if let Some(id) = app.swipe.row.clone() {
        let (l, t) = actions(app, &id);
        app.swipe.slots = (l.len(), t.len());
    }
    if ended && !release {
        app.swipe.gesture = None;
    }
    if release {
        // (fingers that stopped before lifting carry no speed)
        if app.swipe.gesture.as_ref().is_some_and(|g| now - g.moved_t > 0.08) {
            app.swipe.vel = 0.0;
        }
        app.swipe.gesture = None;
        app.swipe.released_at = now;
        app.swipe.dragging = false;
        let_go(app);
    }
    let s = &mut app.swipe;
    if !take.is_empty() {
        ctx.input_mut(|i| {
            let mut k = 0;
            i.raw.events.retain(|_| {
                let keep = !take.contains(&k);
                k += 1;
                keep
            });
        });
    }
    // a click outside the open row, Escape, or a scroll of the list: it shuts
    if s.row.is_some() && s.x.abs() > OPEN_MIN {
        let inside = |p: egui::Pos2| s.rows.iter().any(|(id, r)| Some(id) == s.row.as_ref() && r.contains(p));
        let menu_open = s.menu.is_some();
        if close || escape || pressed.is_some_and(|p| !inside(p) && !menu_open) {
            s.close();
        }
    }
    step(s, dt, ctx)
}

/// The fingers moved the row by `dx`.
fn follow(s: &mut Swipe, dx: f32, dt: f32) {
    // (a lift's event says nothing of speed)
    if s.row.is_none() || dx == 0.0 {
        return;
    }
    s.raw = (s.raw + dx).clamp(-s.width * 4.0, s.width * 4.0);
    // (speed: smoothed over the last few events)
    s.vel = s.vel * 0.6 + (dx / dt.max(0.004)) * 0.4;
}

/// Let go: open, closed, or the full swipe's action.
fn let_go(app: &mut App) {
    let Some(id) = app.swipe.row.clone() else { return };
    let (lead, trail) = actions(app, &id);
    let s = &mut app.swipe;
    let acts = if s.x > 0.0 { &lead } else { &trail };
    if acts.is_empty() {
        s.target = Some(0.0);
        return;
    }
    let open = acts.len() as f32 * SLOT;
    if s.committed {
        s.target = Some(s.x.signum() * s.width * 1.05);
        s.run_on_landing = Some(acts[0].action.clone());
        return;
    }
    // a flick decides by its direction; a slow release by how far it's open
    let keep = if s.vel.abs() > 300.0 { s.vel.signum() == s.x.signum() } else { s.x.abs() > open / 2.0 };
    s.target = Some(if keep { s.x.signum() * open } else { 0.0 });
}

/// The spring and the full-swipe fold, one frame on; an action when a full swipe lands.
fn step(s: &mut Swipe, dt: f32, ctx: &egui::Context) -> Option<String> {
    let mut out = None;
    let following = s.dragging && s.row.is_some();
    if following {
        // the buttons of the side it's slid to: the resting stop, fully open
        let n = if s.raw > 0.0 { s.slots.0 } else { s.slots.1 };
        let open = SLOT * n as f32;
        let a = s.raw.abs();
        // the full swipe, from the fingers' own travel only (a coasting tail never gets here):
        // well past open, and back off it below FULL_KEEP of the way
        let need = (s.width * FULL_SHARE).max(FULL_MIN);
        s.committed = n > 0 && if s.committed { a > open + need * FULL_KEEP } else { a > open + need };
        // 1:1 up to open; past it, a rubber band towards the row's edge
        let room = (s.width - open).max(20.0);
        let shown = if a <= open { a } else { open + room * (1.0 - 1.0 / (1.0 + (a - open) * RUBBER / room)) };
        // (a full swipe snaps the row most of the rest of the way, the outermost button with it)
        let shown = shown + (s.width - shown).max(0.0) * 0.85 * s.full;
        s.x = s.raw.signum() * shown.min(s.width);
    }
    let want = if s.committed { 1.0 } else { 0.0 };
    if (s.full - want).abs() > 0.001 {
        // (a quick snap, ~150 ms, easing out: the moment it takes the row is felt)
        s.full += (want - s.full) * (1.0 - (-dt * 22.0).exp());
        if (s.full - want).abs() < 0.01 {
            s.full = want;
        }
        ctx.request_repaint();
    }
    if let Some(to) = s.target {
        // semi-implicit Euler in small steps: smooth at 120 Hz and at 30
        let c = 2.0 * DAMPING * STIFFNESS.sqrt();
        let n = (dt / 0.002).ceil().max(1.0) as usize;
        let h = dt / n as f32;
        for _ in 0..n {
            let a = -STIFFNESS * (s.x - to) - c * s.vel;
            s.vel += a * h;
            s.x += s.vel * h;
        }
        let landed = (s.x - to).abs() < 0.5 && s.vel.abs() < 8.0;
        // (a full swipe runs once the row is off the edge; the spring needn't come to rest)
        if let Some(a) = s.run_on_landing.as_ref().filter(|_| s.x.abs() >= s.width * 0.98 || landed) {
            out = Some(a.clone());
            *s = Swipe { rows: std::mem::take(&mut s.rows), next_rows: std::mem::take(&mut s.next_rows), width: s.width, ..Default::default() };
            return out;
        }
        if landed {
            // (exactly at its stop: a closed row is at 0, not a hair off it)
            s.x = to;
            s.raw = to;
            s.vel = 0.0;
            s.target = None;
            if to == 0.0 {
                s.row = None;
                s.committed = false;
                s.full = 0.0;
            }
        } else {
            ctx.request_repaint();
        }
    }
    // (not held and not on its way anywhere, but a hair off 0: shut)
    if !s.dragging && s.target.is_none() && s.row.is_some() && s.x.abs() <= OPEN_MIN {
        s.x = 0.0;
        s.raw = 0.0;
        s.vel = 0.0;
        s.row = None;
        s.committed = false;
        s.full = 0.0;
    }
    out
}

/// The buttons in the space the row has left (`r`: the row unmoved). Returns a button's action
/// when one is clicked.
pub fn paint(app: &mut App, ui: &egui::Ui, id: &str, r: Rect) -> Option<String> {
    let x = app.swipe.offset(id);
    if x == 0.0 {
        return None;
    }
    let (lead, trail) = actions(app, id);
    let acts = if x > 0.0 { lead } else { trail };
    let n = acts.len();
    if n == 0 {
        return None;
    }
    let f = app.swipe.full;
    let v = x.abs();
    // each button's share of the space; the outermost takes it all as a full swipe lands
    let slot = |k: usize| if k == 0 { v / n as f32 + (v - v / n as f32) * f } else { v / n as f32 * (1.0 - f) };
    // (inside the row's highlight: 9 pt from the column's left, 10 pt from its divider)
    let r = Rect::from_min_max(pos2(r.min.x + 9.0, r.min.y), pos2(r.max.x - 10.0, r.max.y));
    let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let mut out = None;
    // laid out from the row's edge inwards: the outermost against the edge
    let mut edge = if x > 0.0 { r.min.x } else { r.max.x };
    for (k, a) in acts.iter().enumerate() {
        let w = slot(k);
        let cell = if x > 0.0 { Rect::from_min_max(pos2(edge, r.min.y), pos2(edge + w, r.max.y)) } else { Rect::from_min_max(pos2(edge - w, r.min.y), pos2(edge, r.max.y)) };
        edge = if x > 0.0 { cell.max.x } else { cell.min.x };
        if w < 1.0 {
            continue;
        }
        // the circle grows with its space, then the name fades in under it
        let grow = ((w - 10.0) / CIRCLE).clamp(0.0, 1.0);
        let name = ((w - 44.0) / 14.0).clamp(0.0, 1.0);
        // (the full swipe's button keeps by the row's edge)
        let align = if k == 0 { f } else { 0.0 };
        let cx = if x > 0.0 { cell.center().x + (cell.min.x + SLOT / 2.0 - cell.center().x) * align } else { cell.center().x + (cell.max.x - SLOT / 2.0 - cell.center().x) * align };
        let cy = r.center().y - 7.0;
        let ease = 1.0 - (1.0 - grow) * (1.0 - grow);
        let rad = CIRCLE / 2.0 * ease;
        p.circle_filled(pos2(cx, cy), rad, a.color.gamma_multiply(grow));
        if grow > 0.3 {
            draw(&p, Rect::from_center_size(pos2(cx, cy), vec2(17.0, 17.0) * ease), a.glyph, Color32::WHITE.gamma_multiply(grow));
        }
        if name > 0.0 {
            p.text(pos2(cx, cy + CIRCLE / 2.0 + 3.0 + 2.0 * (1.0 - name)), Align2::CENTER_TOP, a.label, look::regular(10.5), look::text_dim().gamma_multiply(name));
        }
        // (on its way shut: the buttons take no clicks, the row under them does)
        if !app.swipe.is_open(id) {
            continue;
        }
        let hit = Rect::from_center_size(pos2(cx, r.center().y), vec2(w.min(SLOT), r.height()));
        let resp = ui.interact(hit, ui.id().with(("swipe-button", id, k)), egui::Sense::click());
        if resp.clicked() {
            out = Some(a.action.clone());
            if a.action.starts_with("swipe-") {
                app.swipe.menu = Some((if a.action.starts_with("swipe-move") { "move" } else { "share" }, id.to_string(), pos2(cx, r.max.y)));
                out = None;
            }
        }
    }
    if out.is_some() {
        app.swipe.close();
    }
    out
}

/// Move's and Share's menus, open under their button. Returns the action picked.
pub fn menus(app: &mut App, ctx: &egui::Context) -> Option<String> {
    let (what, id, at) = app.swipe.menu.clone()?;
    let items = match what {
        "move" => match crate::menus::move_to(app, &id) {
            crate::menus::MItem::Sub { kids, .. } => kids,
            other => vec![other],
        },
        _ => vec![crate::menus::MItem::act("Export as PDF…", format!("swipe-export:{id}|Export To > PDF"), true), crate::menus::MItem::act("Export as Markdown…", format!("swipe-export:{id}|Export To > Markdown"), true)],
    };
    let mut out = None;
    let shown = egui::Area::new(egui::Id::new("swipe-menu")).order(egui::Order::Foreground).fixed_pos(at - vec2(80.0, 0.0)).show(ctx, |ui| {
        // (styled as the context menus are)
        egui::containers::menu::menu_style(ui.style_mut());
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(160.0);
            out = crate::menus::show_items(ui, &items);
        })
    });
    let clicked_out = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !shown.response.rect.contains(p)));
    if out.is_some() || clicked_out || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.swipe.menu = None;
        app.swipe.close();
    }
    out
}
