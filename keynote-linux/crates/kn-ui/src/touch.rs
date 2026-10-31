//! Touch, as on the iPad and iPhone versions of Keynote, Numbers and Pages: on a document, one
//! finger scrolls it (and coasts on when let go), a tap clicks, a double tap double-clicks and
//! touching and holding opens the context menu; a selected object and its handles still take a
//! drag (to move or resize them). Elsewhere (panels, inspector, popovers) touches work as they
//! always did: egui's scroll areas scroll with a finger, sliders drag.
//!
//! Selected text shows round handles at both ends (selection_handles); dragging one moves that
//! end, as on the iPad.
//!
//! Each app's canvas says where its document is (pan_area), where a drag belongs to the
//! selection (grab) and where its text selection runs every frame; the app's raw_input_hook
//! calls raw_input, which rewrites the touches of the next frame from that.

use egui::{Event, Pos2, Rect, TouchPhase, Vec2};
use std::sync::Mutex;

/// How far a finger moves before a touch is a scroll, not a tap (points).
const SLOP: f32 = 8.0;
/// How long a finger holds still before it's a touch and hold (seconds).
const HOLD: f64 = 0.45;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// down, not yet a tap, scroll or hold
    Pending,
    Pan,
    /// held: the context menu opened; the rest of this touch is ignored
    Held,
    /// not on a document (or on the selection): the touch works as a mouse
    Pass,
    /// a second finger came: pinch zoom (egui's), no scrolling or clicks
    Ignore,
    /// dragging a selection handle: a shift-drag where the handle's end of the text is
    /// (`offset` from the finger); `press` is still to come (after a click at the other end,
    /// for the start handle, so that end becomes the anchor)
    Handle { offset: Vec2, press: bool },
}

/// A text selection's two ends: (top, bottom) of the start and of the end, on screen.
type Ends = ([Pos2; 2], [Pos2; 2]);

/// Where a handle's knob is drawn: above the start, below the end.
fn knobs((s, e): Ends) -> (Pos2, Pos2) {
    (s[0] - Vec2::new(0.0, 7.0), e[1] + Vec2::new(0.0, 7.0))
}

struct Finger {
    id: u64,
    start: Pos2,
    last: Pos2,
    t0: f64,
    mode: Mode,
}

#[derive(Default)]
struct State {
    finger: Option<Finger>,
    fingers: usize,
    /// what the canvases registered last frame, and this frame
    pan: Vec<Rect>,
    grab: Vec<Rect>,
    next_pan: Vec<Rect>,
    next_grab: Vec<Rect>,
    handles: Option<Ends>,
    next_handles: Option<Ends>,
    vel: Vec2,
    coasting: bool,
    /// where the finger left the glass (the coasting scroll goes there)
    coast_at: Pos2,
    last_t: Option<f64>,
    /// the last input was a finger (tooltips stay hidden)
    touch: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(s.get_or_insert_with(State::default))
}

/// A canvas: one finger scrolls here (this frame's area, used for the next frame's touches).
pub fn pan_area(rect: Rect) {
    with(|s| s.next_pan.push(rect));
}

/// The selection (or one of its handles): a finger here drags it, as a mouse would.
pub fn grab(rect: Rect) {
    with(|s| s.next_grab.push(rect));
}

/// A text selection from `start` to `end` (each the caret line's top and bottom on screen): with
/// a finger, its two handles are drawn, and dragging them moves that end.
pub fn selection_handles(painter: &egui::Painter, start: [Pos2; 2], end: [Pos2; 2], color: egui::Color32) {
    with(|s| s.next_handles = Some((start, end)));
    if !active() {
        return;
    }
    let (ks, ke) = knobs((start, end));
    for ([a, b], k) in [(start, ks), (end, ke)] {
        painter.line_segment([a, b], egui::Stroke::new(2.0, color));
        painter.circle_filled(k, 5.5, color);
    }
}

/// The last input came from a finger.
pub fn active() -> bool {
    with(|s| s.touch)
}

/// In the app's raw_input_hook, before anything else reads the events.
pub fn raw_input(ctx: &egui::Context, raw: &mut egui::RawInput) {
    let now = raw.time.unwrap_or(0.0);
    with(|s| {
        s.pan = std::mem::take(&mut s.next_pan);
        s.grab = std::mem::take(&mut s.next_grab);
        s.handles = s.next_handles.take();
        let dt = s.last_t.map(|t| (now - t).clamp(0.001, 0.1) as f32).unwrap_or(1.0 / 60.0);
        s.last_t = Some(now);
        let mut out = Vec::with_capacity(raw.events.len());
        for e in raw.events.drain(..) {
            match e {
                Event::Touch { id, phase, pos, .. } => {
                    let id = id.0;
                    s.touch = true;
                    match phase {
                        TouchPhase::Start => {
                            s.fingers += 1;
                            s.coasting = false;
                            match &mut s.finger {
                                None if s.fingers == 1 => {
                                    // on a selection handle: drag that end of the selection
                                    let handle = s.handles.and_then(|h| {
                                        let (ks, ke) = knobs(h);
                                        let mid = |[a, b]: [Pos2; 2]| a + (b - a) * 0.5;
                                        if pos.distance(ke) < 24.0 {
                                            Some((mid(h.1) - pos, None))
                                        } else if pos.distance(ks) < 24.0 {
                                            Some((mid(h.0) - pos, Some(mid(h.1))))
                                        } else {
                                            None
                                        }
                                    });
                                    if let Some((offset, other)) = handle {
                                        // (the start handle: a click at the end first, so the end stays put)
                                        if let Some(o) = other {
                                            out.push(Event::PointerMoved(o));
                                            out.push(button(o, egui::PointerButton::Primary, true));
                                            out.push(button(o, egui::PointerButton::Primary, false));
                                        }
                                        s.finger = Some(Finger { id, start: pos, last: pos, t0: now, mode: Mode::Handle { offset, press: true } });
                                        out.push(e);
                                        continue;
                                    }
                                    let on_doc = s.pan.iter().any(|r| r.contains(pos)) && !s.grab.iter().any(|r| r.contains(pos)) && ctx.layer_id_at(pos).is_none_or(|l| l.order == egui::Order::Background);
                                    let mode = if on_doc { Mode::Pending } else { Mode::Pass };
                                    s.finger = Some(Finger { id, start: pos, last: pos, t0: now, mode });
                                    s.vel = Vec2::ZERO;
                                }
                                Some(f) if f.mode != Mode::Pass => f.mode = Mode::Ignore,
                                _ => {}
                            }
                        }
                        TouchPhase::Move => {
                            if let Some(f) = s.finger.as_mut().filter(|f| f.id == id) {
                                if let Mode::Handle { offset, press: false } = f.mode {
                                    out.push(Event::PointerMoved(pos + offset));
                                    f.last = pos;
                                } else if f.mode == Mode::Pending && pos.distance(f.start) > SLOP {
                                    f.mode = Mode::Pan;
                                    out.push(Event::PointerMoved(pos));
                                    out.push(wheel(pos - f.start, TouchPhase::Start, egui::MouseWheelSource::Trackpad));
                                    f.last = pos;
                                } else if f.mode == Mode::Pan {
                                    let d = pos - f.last;
                                    s.vel = s.vel * 0.5 + (d / dt) * 0.5;
                                    out.push(Event::PointerMoved(pos));
                                    out.push(wheel(d, TouchPhase::Move, egui::MouseWheelSource::Trackpad));
                                    f.last = pos;
                                }
                            }
                        }
                        TouchPhase::End | TouchPhase::Cancel => {
                            s.fingers = s.fingers.saturating_sub(1);
                            if let Some(f) = s.finger.as_ref().filter(|f| f.id == id) {
                                match f.mode {
                                    // a tap: the click, at the place touched
                                    Mode::Pending if phase == TouchPhase::End => {
                                        let p = f.start;
                                        out.push(Event::PointerMoved(p));
                                        out.push(button(p, egui::PointerButton::Primary, true));
                                        out.push(button(p, egui::PointerButton::Primary, false));
                                        out.push(Event::PointerGone);
                                    }
                                    Mode::Pan => {
                                        s.coasting = s.vel.length() > 120.0;
                                        s.coast_at = f.last;
                                    }
                                    Mode::Handle { offset, press: false } => {
                                        out.push(button(f.last + offset, egui::PointerButton::Primary, false));
                                        out.push(Event::ModifiersChanged(egui::Modifiers::NONE));
                                        out.push(Event::PointerGone);
                                    }
                                    _ => {}
                                }
                            }
                            if s.fingers == 0 {
                                s.finger = None;
                            }
                        }
                    }
                    out.push(e);
                }
                // the pointer egui makes from the first finger: kept only where touches work
                // as a mouse
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone if s.finger.as_ref().is_some_and(|f| f.mode != Mode::Pass) => {}
                Event::PointerMoved(_) if s.finger.is_none() => {
                    s.touch = false;
                    out.push(e);
                }
                e => out.push(e),
            }
        }
        // a handle's drag starts (a frame after the click at the other end, if there was one)
        if let Some(f) = s.finger.as_mut() {
            if let Mode::Handle { offset, press: true } = f.mode {
                let at_once = !out.iter().any(|e| matches!(e, Event::PointerButton { .. }));
                if at_once {
                    // (held shift through the drag: it extends the selection)
                    out.push(Event::ModifiersChanged(egui::Modifiers::SHIFT));
                    out.push(Event::PointerMoved(f.last + offset));
                    out.push(Event::PointerButton { pos: f.last + offset, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::SHIFT });
                    f.mode = Mode::Handle { offset, press: false };
                } else {
                    ctx.request_repaint();
                }
            }
        }
        // touch and hold: what's there is selected, then its context menu opens (a click and a
        // right click where the finger is)
        if let Some(f) = s.finger.as_mut().filter(|f| f.mode == Mode::Pending) {
            if now - f.t0 >= HOLD {
                f.mode = Mode::Held;
                out.push(Event::PointerMoved(f.start));
                out.push(button(f.start, egui::PointerButton::Primary, true));
                out.push(button(f.start, egui::PointerButton::Primary, false));
                out.push(button(f.start, egui::PointerButton::Secondary, true));
                out.push(button(f.start, egui::PointerButton::Secondary, false));
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
        }
        // let go while scrolling: it coasts on, slowing down
        if s.coasting && s.finger.is_none() {
            s.vel *= (-dt * 3.2).exp();
            if s.vel.length() < 15.0 {
                s.coasting = false;
                s.vel = Vec2::ZERO;
            } else {
                out.push(Event::PointerMoved(s.coast_at));
                out.push(wheel(s.vel * dt, TouchPhase::Move, egui::MouseWheelSource::Momentum));
                ctx.request_repaint();
            }
        }
        raw.events = out;
    });
}

/// Once per frame (from begin_frame): no hover tooltips while using a finger.
pub(crate) fn frame(ctx: &egui::Context) {
    thread_local!(static WAS: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) });
    let now = active();
    if WAS.with(|w| w.replace(Some(now))) != Some(now) {
        ctx.all_styles_mut(|s| s.interaction.tooltip_delay = if now { f32::INFINITY } else { egui::Style::default().interaction.tooltip_delay });
    }
}

fn wheel(delta: Vec2, phase: TouchPhase, source: egui::MouseWheelSource) -> Event {
    Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, phase, modifiers: egui::Modifiers::NONE, source }
}

fn button(pos: Pos2, button: egui::PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f32, y: f32) -> Event {
        Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: Pos2::new(x, y), force: None }
    }

    /// One frame: the canvas registers, then the next frame's touches are rewritten.
    fn frame(ctx: &egui::Context, t: f64, events: Vec<Event>) -> Vec<Event> {
        pan_area(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));
        grab(Rect::from_min_size(Pos2::new(500.0, 400.0), Vec2::new(100.0, 100.0)));
        let mut raw = egui::RawInput { time: Some(t), events, ..Default::default() };
        raw_input(ctx, &mut raw);
        raw.events
    }

    fn reset() {
        *STATE.lock().unwrap() = None;
    }

    fn clicks(evs: &[Event], b: egui::PointerButton) -> usize {
        evs.iter().filter(|e| matches!(e, Event::PointerButton { button, pressed: true, .. } if *button == b)).count()
    }

    fn scrolled(evs: &[Event]) -> Vec2 {
        evs.iter().fold(Vec2::ZERO, |a, e| if let Event::MouseWheel { delta, source: egui::MouseWheelSource::Trackpad, .. } = e { a + *delta } else { a })
    }

    // (one test: the state is shared)
    #[test]
    fn taps_scrolls_holds_and_grabs() {
        let ctx = egui::Context::default();
        let p = |x: f32, y: f32| Event::PointerButton { pos: Pos2::new(x, y), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };

        // a tap: one click, when the finger lifts, where it touched
        reset();
        let down = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 100.0, 100.0), p(100.0, 100.0)]);
        assert_eq!(clicks(&down, egui::PointerButton::Primary), 0);
        let up = frame(&ctx, 0.1, vec![touch(1, TouchPhase::End, 101.0, 101.0)]);
        assert_eq!(clicks(&up, egui::PointerButton::Primary), 1);

        // a drag scrolls (the content follows the finger) and doesn't click
        reset();
        let mut all = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 100.0, 300.0), p(100.0, 300.0)]);
        all.extend(frame(&ctx, 0.02, vec![touch(1, TouchPhase::Move, 100.0, 250.0)]));
        all.extend(frame(&ctx, 0.04, vec![touch(1, TouchPhase::Move, 100.0, 200.0)]));
        all.extend(frame(&ctx, 0.06, vec![touch(1, TouchPhase::End, 100.0, 200.0)]));
        assert_eq!(clicks(&all, egui::PointerButton::Primary), 0);
        assert!((scrolled(&all).y + 100.0).abs() < 0.01, "{:?}", scrolled(&all));
        // ... and coasts on after
        let coast = frame(&ctx, 0.08, vec![]);
        assert!(coast.iter().any(|e| matches!(e, Event::MouseWheel { delta, source: egui::MouseWheelSource::Momentum, .. } if delta.y < 0.0)));

        // touch and hold: a right click, once
        reset();
        frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 100.0, 100.0)]);
        let held = frame(&ctx, 0.5, vec![]);
        assert_eq!((clicks(&held, egui::PointerButton::Primary), clicks(&held, egui::PointerButton::Secondary)), (1, 1));
        let up = frame(&ctx, 0.6, vec![touch(1, TouchPhase::End, 100.0, 100.0)]);
        assert_eq!(clicks(&up, egui::PointerButton::Primary) + clicks(&up, egui::PointerButton::Secondary), 0);

        // on the selection: the press goes through at once (dragging moves it)
        reset();
        let down = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 550.0, 450.0), p(550.0, 450.0)]);
        assert_eq!(clicks(&down, egui::PointerButton::Primary), 1);

        // two fingers (pinch): no click, no scroll
        reset();
        let mut all = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 100.0, 100.0), touch(2, TouchPhase::Start, 300.0, 300.0)]);
        all.extend(frame(&ctx, 0.05, vec![touch(1, TouchPhase::Move, 80.0, 80.0), touch(2, TouchPhase::Move, 320.0, 320.0)]));
        all.extend(frame(&ctx, 0.1, vec![touch(1, TouchPhase::End, 80.0, 80.0), touch(2, TouchPhase::End, 320.0, 320.0)]));
        assert_eq!(clicks(&all, egui::PointerButton::Primary), 0);
        assert_eq!(scrolled(&all), Vec2::ZERO);

        // a selection handle: the end handle's drag is a shift-drag from that end
        reset();
        with(|s| s.next_handles = Some(([Pos2::new(100.0, 200.0), Pos2::new(100.0, 220.0)], [Pos2::new(300.0, 200.0), Pos2::new(300.0, 220.0)])));
        let mut all = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 300.0, 228.0)]);
        all.extend(frame(&ctx, 0.02, vec![touch(1, TouchPhase::Move, 340.0, 228.0)]));
        all.extend(frame(&ctx, 0.04, vec![touch(1, TouchPhase::End, 340.0, 228.0)]));
        assert!(all.iter().any(|e| matches!(e, Event::PointerButton { pressed: true, modifiers, pos, .. } if modifiers.shift && (pos.x - 300.0).abs() < 0.1 && (pos.y - 210.0).abs() < 0.1)), "{all:?}");
        assert!(all.iter().any(|e| matches!(e, Event::PointerMoved(p) if (p.x - 340.0).abs() < 0.1)));
        assert_eq!(scrolled(&all), Vec2::ZERO);
        // the start handle: first a click at the end (so it stays put), then the shift-drag
        reset();
        with(|s| s.next_handles = Some(([Pos2::new(100.0, 200.0), Pos2::new(100.0, 220.0)], [Pos2::new(300.0, 200.0), Pos2::new(300.0, 220.0)])));
        let first = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 100.0, 193.0)]);
        assert!(matches!(first.iter().find(|e| matches!(e, Event::PointerButton { .. })), Some(Event::PointerButton { pos, modifiers, .. }) if pos.x == 300.0 && !modifiers.shift));
        let second = frame(&ctx, 0.02, vec![]);
        assert!(second.iter().any(|e| matches!(e, Event::PointerButton { pressed: true, modifiers, pos, .. } if modifiers.shift && pos.x == 100.0)));

        // outside the document (a panel): touches pass as they are
        reset();
        let down = frame(&ctx, 0.0, vec![touch(1, TouchPhase::Start, 900.0, 100.0), p(900.0, 100.0)]);
        assert_eq!(clicks(&down, egui::PointerButton::Primary), 1);
    }
}
