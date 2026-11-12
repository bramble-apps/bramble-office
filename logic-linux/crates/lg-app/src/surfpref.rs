//! Settings ▸ Control Surfaces ▸ General, as the surfaces use it: Pickup mode (an absolute
//! control moves its parameter only after reaching the parameter's value), Maximum MIDI
//! Bandwidth (feedback to a surface is paced to that share of a MIDI cable's 3125 bytes/s),
//! Control surface follows track selection (the fader bank moves to show the selected track),
//! Control surface follows focused view, Open or close plug-in window following change to
//! control surface, Jog resolution depends on horizontal zoom, Multiple Controls per Parameter
//! with Only when all parameters fit on one page (which V-Pot edits which plug-in parameter in
//! the Mackie plug-in and instrument modes), and Show Value Units For (dB, Hz, ms … on the LCD).

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use lg_dsp::Unit;

pub const COMMANDS: &[&str] = &[];

pub const PICKUP: &str = "Control Surfaces / General / Pickup mode";
pub const BANDWIDTH: &str = "Control Surfaces / General / Maximum MIDI Bandwidth:";
pub const FOLLOW_SELECTION: &str = "Control Surfaces / General / Control surface follows track selection";
pub const OPEN_PLUGIN: &str = "Control Surfaces / General / Open or close plug-in window following change to control surface";
pub const FOLLOW_FOCUS: &str = "Control Surfaces / General / Control surface follows focused view";
pub const JOG_ZOOM: &str = "Control Surfaces / General / Jog resolution depends on horizontal zoom";
pub const MULTIPLE: &str = "Control Surfaces / General / Multiple Controls per Parameter:";
pub const FIT_ONLY: &str = "Control Surfaces / General / Only when all parameters fit on one page";
pub const UNITS_PLUGIN: &str = "Control Surfaces / General / Instrument/plug-in parameters";
pub const UNITS_STRIP: &str = "Control Surfaces / General / Volume and other parameters";

/// A MIDI cable's bytes per second (31250 baud, 10 bits a byte).
pub const MIDI_BYTES_PER_SEC: f32 = 3125.0;

// ---- Multiple Controls per Parameter ----------------------------------------------------------

/// How many encoders each parameter gets: the setting (1, 2, 4, 8), halved while the parameters
/// would not all fit on the encoders when Only when all parameters fit on one page is on.
pub fn per_param(setting: usize, encoders: usize, params: usize, fit_only: bool) -> usize {
    let mut per = setting.clamp(1, 8);
    if fit_only {
        while per > 1 && params * per > encoders {
            per /= 2;
        }
    }
    per
}

/// The parameter encoder `enc` (0-based across the group's surfaces) edits: in each group of
/// `per` encoders only the first is active; `first` is the parameter on the leftmost encoder
/// (the group's Instrument / Plug-in Parameter Page).
pub fn encoder_param(enc: usize, encoders: usize, params: usize, setting: usize, fit_only: bool, first: usize) -> Option<usize> {
    let per = per_param(setting, encoders, params, fit_only);
    if enc >= encoders || enc % per != 0 {
        return None;
    }
    let i = first + enc / per;
    (i < params).then_some(i)
}

/// The group parameter holding the parameter on the leftmost encoder: instrument and plug-in
/// pages are kept apart, so switching between them doesn't lose the place.
pub fn page_key(mode: u8) -> &'static str {
    if mode == MODE_INSTRUMENT { "Instrument Parameter Page" } else { "Plug-in Parameter Page" }
}

// ---- Show Value Units For ----------------------------------------------------------------------

/// A value as the surface's display shows it (six characters), with its unit when `units`.
pub fn value_text(v: f32, unit: Unit, units: bool) -> String {
    let u = |s: &'static str| if units { s } else { "" };
    match unit {
        Unit::Db if v.abs() >= 10.0 => format!("{v:.0}{}", u("dB")),
        Unit::Db => format!("{v:.1}{}", u("dB")),
        Unit::Hz if v >= 10000.0 => format!("{:.0}{}", v / 1000.0, u("kHz")),
        Unit::Hz if v >= 1000.0 => format!("{:.1}{}", v / 1000.0, u("kHz")),
        Unit::Hz => format!("{v:.0}{}", u("Hz")),
        Unit::Ms => format!("{v:.0}{}", u("ms")),
        Unit::Seconds => format!("{v:.2}{}", u("s")),
        Unit::Percent => format!("{v:.0}{}", u("%")),
        Unit::Semitones => format!("{v:+.0}{}", u("st")),
        Unit::Cents => format!("{v:+.0}{}", u("ct")),
        Unit::Octaves => format!("{v:+.0}{}", u("oct")),
        Unit::Pan => format!("{:+.0}", v * 64.0),
        Unit::OnOff => if v >= 0.5 { "On".into() } else { "Off".into() },
        Unit::Ratio => format!("{v:.2}"),
        Unit::None | Unit::Menu => format!("{v:.0}"),
    }
}

// ---- Pickup mode -------------------------------------------------------------------------------

/// Whether an absolute control moving to `incoming` (0–1, from `prev`) reaches the parameter's
/// value `cur`: it is there (within a step of 127) or passed it on the way.
pub fn reaches(prev: Option<f32>, incoming: f32, cur: f32) -> bool {
    (incoming - cur).abs() <= 1.5 / 127.0 || prev.is_some_and(|p| (p - cur) * (incoming - cur) <= 0.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct Grip {
    last_in: Option<f32>,
    /// The value this control set last (while it stays there, the control holds the parameter).
    held: Option<f32>,
}

thread_local! {
    static GRIPS: RefCell<HashMap<String, Grip>> = RefCell::new(HashMap::new());
}

/// An absolute control `key` moved to `incoming` with the parameter at `cur`: the value to set,
/// or None while it hasn't picked the parameter up (Pickup mode on). With Pickup mode off the
/// control sets the value at once (the parameter may jump).
pub fn pickup(key: &str, pickup_on: bool, incoming: f32, cur: f32) -> Option<f32> {
    GRIPS.with(|g| {
        let mut g = g.borrow_mut();
        let grip = g.entry(key.to_string()).or_default();
        let holding = grip.held.is_some_and(|h| (h - cur).abs() < 1e-3);
        let ok = !pickup_on || holding || reaches(grip.last_in, incoming, cur);
        grip.last_in = Some(incoming);
        grip.held = if ok { Some(incoming) } else { None };
        ok.then_some(incoming)
    })
}

// ---- Maximum MIDI Bandwidth --------------------------------------------------------------------

/// A token bucket: how many bytes a port may take now at `pct` percent of a MIDI cable.
#[derive(Clone, Debug)]
pub struct Budget {
    tokens: f32,
    at: Option<Instant>,
    pub queue: VecDeque<Vec<u8>>,
}

impl Default for Budget {
    fn default() -> Budget {
        Budget { tokens: f32::MAX, at: None, queue: VecDeque::new() }
    }
}

impl Budget {
    fn cap(pct: f32) -> f32 {
        // a quarter second's worth, and room for one LCD message at least
        (MIDI_BYTES_PER_SEC * pct.clamp(1.0, 100.0) / 100.0 * 0.25).max(160.0)
    }

    /// Refills for the time passed, then queues `msgs` and returns those that fit now (in order).
    pub fn pace(&mut self, now: Instant, pct: f32, msgs: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        let rate = MIDI_BYTES_PER_SEC * pct.clamp(1.0, 100.0) / 100.0;
        let cap = Budget::cap(pct);
        let dt = self.at.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        self.at = Some(now);
        self.tokens = (self.tokens.min(cap) + dt * rate).min(cap);
        self.queue.extend(msgs);
        // (a stuck port can't grow it without end: the oldest go)
        while self.queue.len() > 4096 {
            self.queue.pop_front();
        }
        let mut out = Vec::new();
        while let Some(m) = self.queue.front() {
            let n = m.len() as f32;
            if n > self.tokens {
                break;
            }
            self.tokens -= n;
            out.push(self.queue.pop_front().unwrap());
        }
        out
    }
}

thread_local! {
    static BUDGETS: RefCell<HashMap<String, Budget>> = RefCell::new(HashMap::new());
}

/// Feedback for a surface's output port, paced to Maximum MIDI Bandwidth; true while some wait.
pub fn send_paced(outs: &crate::midiout::Outs, port: &str, pct: f32, msgs: Vec<Vec<u8>>) -> bool {
    BUDGETS.with(|b| {
        let mut b = b.borrow_mut();
        let budget = b.entry(port.to_string()).or_default();
        for m in budget.pace(Instant::now(), pct, msgs) {
            outs.send(port, &m);
        }
        !budget.queue.is_empty()
    })
}

// ---- Jog resolution ----------------------------------------------------------------------------

/// Ticks one jog-wheel step moves: a 16th note, or with Jog resolution depends on horizontal
/// zoom, four pixels at the Tracks area's zoom (`bar_w` points a bar of `bar_ticks`).
pub fn jog_ticks(by_zoom: bool, bar_w: f32, bar_ticks: f64, ppq: f64) -> f64 {
    if by_zoom && bar_w > 0.0 {
        (4.0 / bar_w as f64 * bar_ticks).max(1.0)
    } else {
        ppq / 4.0
    }
}

// ---- following -------------------------------------------------------------------------------

/// The fader bank nearest `cur` (0..=max) for which `shows(bank)` holds, if any.
pub fn bank_showing(cur: i32, max: i32, shows: impl Fn(i32) -> bool) -> Option<i32> {
    if shows(cur) {
        return Some(cur);
    }
    (1..=max.max(0) + 1).find_map(|d| [cur - d, cur + d].into_iter().find(|b| (0..=max).contains(b) && shows(*b)))
}

/// Control surface follows focused view: the Channel Strip View Mode for the area with key
/// focus (the Mixer's All, the Tracks area's Arrange); None leaves it.
pub fn view_for_focus(area: crate::focus::Area) -> Option<u8> {
    match area {
        crate::focus::Area::Mixer => Some(1),
        crate::focus::Area::Tracks | crate::focus::Area::Editor => Some(0),
        _ => None,
    }
}

// ---- Help Tags -------------------------------------------------------------------------------

pub const HT: &str = "Control Surfaces / Help Tags / ";

/// A long name and value shown across the LCD while (and a while after) it is edited.
#[derive(Clone, Debug)]
struct Tag {
    name: String,
    value: String,
    at: Instant,
}

thread_local! {
    static TAGS: RefCell<Vec<Tag>> = const { RefCell::new(Vec::new()) };
}

/// Help Tags ▸ Show Value Units For: the unit after a help tag's value (plug-in parameters, or
/// volume and the other channel strip parameters).
pub fn tag_units(v: &crate::settings::Values, plugin: bool) -> bool {
    v.on(&format!("{HT}{}", if plugin { "Instrument/plug-in parameters" } else { "Volume and other parameters" }))
}

/// A parameter edited on a surface: its long name and value become a help tag (Show info for
/// multiple parameters keeps the others until they time out; off, only the latest shows).
pub fn help_tag(v: &crate::settings::Values, name: &str, value: &str, now: Instant) {
    let multiple = v.on(&format!("{HT}Show info for multiple parameters"));
    TAGS.with(|t| {
        let mut t = t.borrow_mut();
        if !multiple {
            t.clear();
        }
        t.retain(|x| x.name != name);
        t.push(Tag { name: name.to_string(), value: value.to_string(), at: now });
    });
}

/// The track selection's help tag (Show info when selecting tracks): Selected over its name.
pub fn tag_selected(v: &crate::settings::Values, track_name: &str, now: Instant) {
    if v.on(&format!("{HT}Show info when selecting tracks")) {
        help_tag(v, "Selected", track_name, now);
    }
}

/// A track volume edited (Show info when editing volume): Volume over its value.
pub fn tag_volume(v: &crate::settings::Values, db: f32, now: Instant) {
    if v.on(&format!("{HT}Show info when editing volume")) {
        let units = tag_units(v, false);
        help_tag(v, "Volume", &format!("{db:.1}{}", if units { " dB" } else { "" }), now);
    }
}

fn line(s: &str) -> String {
    let t: String = s.chars().filter(|c| c.is_ascii()).take(56).collect();
    format!("{t:<56}")
}

/// The LCD's rows while help tags show (each None: that row as usual): the long names on the
/// upper row (Parameter name), the values on the lower (Parameter value), for Display Duration
/// seconds after the last edit. Nothing when both checkboxes are off.
pub fn help_rows(v: &crate::settings::Values, now: Instant) -> (Option<String>, Option<String>) {
    let (names, values) = (v.on(&format!("{HT}Parameter name")), v.on(&format!("{HT}Parameter value")));
    if !names && !values {
        return (None, None);
    }
    let secs = v.get(&format!("{HT}Display Duration:")).parse::<f32>().unwrap_or(2.0).clamp(0.1, 10.0);
    TAGS.with(|t| {
        let mut t = t.borrow_mut();
        t.retain(|x| now.saturating_duration_since(x.at).as_secs_f32() < secs);
        if t.is_empty() {
            return (None, None);
        }
        let top = t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>().join(" / ");
        let bottom = t.iter().map(|x| x.value.as_str()).collect::<Vec<_>>().join(" / ");
        (names.then(|| line(&top)), values.then(|| line(&bottom)))
    })
}

// ---- on the app ------------------------------------------------------------------------------

use crate::app::App;
use crate::smartcontrols::{ParamInfo, Target, View};

/// Mackie V-Pot modes past Track / Pan / Send (Group::strip_param).
pub const MODE_PLUGIN: u8 = 3;
pub const MODE_INSTRUMENT: u8 = 4;

/// The parameters the plug-in mode (the group's Insert Slot) or the instrument mode edits: the
/// selected (or locked) track's.
pub fn plugin_params(app: &App, row: usize, mode: u8) -> Option<(usize, Vec<(Target, ParamInfo)>)> {
    let t = app.hub.surf.groups.get(row).and_then(|g| g.track_lock).or(app.selected_track)?;
    let slot = (crate::surfext::gx(app, row, "Insert Slot").max(1) - 1) as usize;
    let list = crate::smartcontrols::parameters(app, View::Track(t))
        .into_iter()
        .filter(|(tg, _, _)| match tg {
            Target::Instrument(_) => mode == MODE_INSTRUMENT,
            Target::Effect(k, _) => mode == MODE_PLUGIN && *k == slot,
            _ => false,
        })
        .map(|(tg, _, i)| (tg, i))
        .collect();
    Some((t, list))
}

/// What V-Pot `k` of device `d` edits in a plug-in or instrument mode.
pub fn plugin_slot(app: &App, d: usize, k: u8) -> Option<(usize, Target, ParamInfo)> {
    let s = &app.hub.surf;
    let row = s.devices.get(d)?.row;
    let mode = s.groups.get(row)?.strip_param;
    let (enc, n_enc) = crate::surfaces::encoders(app, d);
    let (t, list) = plugin_params(app, row, mode)?;
    let setting = app.logic_settings.get(MULTIPLE).parse().unwrap_or(2);
    let first = crate::surfext::gx(app, row, page_key(mode)).max(0) as usize;
    let i = encoder_param(enc + k as usize, n_enc, list.len(), setting, app.logic_settings.on(FIT_ONLY), first)?;
    let (tg, info) = list.into_iter().nth(i)?;
    Some((t, tg, info))
}

/// Bank ◀ / ▶ in a plug-in or instrument mode: the parameters a page further (the leftmost
/// encoder's parameter moves by the parameters shown).
pub fn plugin_bank(app: &mut App, d: usize, dir: i32) {
    let row = app.hub.surf.devices[d].row;
    let mode = app.hub.surf.groups[row].strip_param;
    let Some((_, list)) = plugin_params(app, row, mode) else { return };
    let (_, n_enc) = crate::surfaces::encoders(app, d);
    let per = per_param(app.logic_settings.get(MULTIPLE).parse().unwrap_or(2), n_enc, list.len(), app.logic_settings.on(FIT_ONLY));
    let shown = (n_enc / per).max(1) as i32;
    let key = page_key(mode);
    let cur = crate::surfext::gx(app, row, key);
    let nv = (cur + dir * shown).clamp(0, (list.len() as i32 - 1).max(0));
    crate::surfext::set_gx(app, row, key, nv);
}

/// A V-Pot turned `n` steps in a plug-in or instrument mode (Resolution of Relative Controls
/// steps across the range; switches and menus a step a click).
pub fn plugin_vpot(app: &mut App, d: usize, k: u8, n: i32) {
    let Some((t, tg, info)) = plugin_slot(app, d, k) else { return };
    let res = app.logic_settings.get("Control Surfaces / General / Resolution of Relative Controls:").parse::<f32>().unwrap_or(128.0).max(1.0);
    let step = if matches!(info.unit, Unit::OnOff | Unit::Menu) { 1.0 } else { (info.max - info.min) / res };
    let nv = (info.value + n as f32 * step).clamp(info.min, info.max);
    crate::smartcontrols::set_target(app, View::Track(t), tg, nv);
    let units = tag_units(&app.logic_settings, true);
    help_tag(&app.logic_settings, &info.name, &value_text(nv, info.unit, units), Instant::now());
}

/// The LCD's two rows in a plug-in or instrument mode: parameter names, then values (with
/// units when Show Value Units For ▸ Instrument/plug-in parameters is on).
pub fn plugin_lcd(app: &App, d: usize) -> (String, String) {
    let units = app.logic_settings.on(UNITS_PLUGIN);
    let (mut names, mut values) = (String::new(), String::new());
    for k in 0..8u8 {
        let p = plugin_slot(app, d, k);
        names += &lg_core::ctrlsurf::lcd_cell(&p.as_ref().map_or(String::new(), |(_, _, i)| short_name(&i.name)));
        values += &lg_core::ctrlsurf::lcd_cell(&p.as_ref().map_or(String::new(), |(_, _, i)| value_text(i.value, i.unit, units)));
    }
    (names, values)
}

/// A parameter's name in six characters: words shortened (vowels after the first letter go).
pub fn short_name(s: &str) -> String {
    if s.chars().count() <= 6 {
        return s.to_string();
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    let squeeze = |w: &str| -> String { w.chars().enumerate().filter(|(i, c)| *i == 0 || !"aeiouAEIOU".contains(*c)).map(|x| x.1).collect() };
    let joined: String = words.iter().map(|w| squeeze(w)).collect();
    joined.chars().take(6).collect()
}

/// A V-Pot mode button: Plug-in / Instrument open the edited plug-in's window and other modes
/// close it, when Open or close plug-in window following change to control surface is on (or
/// the group's own Open/Close Plug-in Window).
pub fn mode_changed(app: &mut App, row: usize, old: u8, new: u8) {
    if old == new || !(app.logic_settings.on(OPEN_PLUGIN) || crate::surfext::gx(app, row, "Open/Close Plug-in Window") == 1) {
        return;
    }
    let Some(t) = app.hub.surf.groups.get(row).and_then(|g| g.track_lock).or(app.selected_track) else { return };
    let slot = (crate::surfext::gx(app, row, "Insert Slot").max(1) - 1) as usize;
    if old == MODE_INSTRUMENT && app.plugin_window == Some(t) {
        app.plugin_window = None;
    }
    if old == MODE_PLUGIN && app.mix.fx_window.is_some_and(|w| w.0 == View::Track(t).strip()) {
        app.mix.fx_window = None;
    }
    match new {
        MODE_INSTRUMENT => app.plugin_window = Some(t),
        MODE_PLUGIN if slot < crate::mixstate::inserts(app, View::Track(t).strip()).len() => app.mix.fx_window = Some((View::Track(t).strip(), slot)),
        _ => {}
    }
}

#[derive(Default)]
struct Seen {
    selected: Option<usize>,
    area: Option<crate::focus::Area>,
}

thread_local! {
    static SEEN: RefCell<Seen> = RefCell::new(Seen::default());
}

/// Each frame: the bank follows a new track selection, the view follows a new key focus.
pub fn follow(app: &mut App) {
    let (sel_changed, area_changed) = SEEN.with(|s| {
        let mut s = s.borrow_mut();
        let a = (s.selected != app.selected_track, s.area != Some(app.kf.area));
        s.selected = app.selected_track;
        s.area = Some(app.kf.area);
        a
    });
    if area_changed && app.logic_settings.on(FOLLOW_FOCUS) {
        if let Some(v) = view_for_focus(app.kf.area) {
            for row in 0..app.hub.surf.groups.len() {
                app.hub.surf.groups[row].view = v;
            }
        }
    }
    if sel_changed && app.logic_settings.on(FOLLOW_SELECTION) {
        if let Some(sel) = app.selected_track {
            crate::surfaces::bank_to_show(app, sel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn encoders_per_parameter() {
        // 2 per parameter: 1-2 the first, 3-4 the second; only the first of each pair is live
        assert_eq!(encoder_param(0, 8, 3, 2, false, 0), Some(0));
        assert_eq!(encoder_param(1, 8, 3, 2, false, 0), None);
        assert_eq!(encoder_param(2, 8, 3, 2, false, 0), Some(1));
        // 4: 1-4, 5-8
        assert_eq!(encoder_param(4, 8, 3, 4, false, 0), Some(1));
        assert_eq!(encoder_param(3, 8, 3, 4, false, 0), None);
        // 8: 1-8, 9-16
        assert_eq!(encoder_param(8, 16, 3, 8, false, 0), Some(1));
        assert_eq!(encoder_param(7, 16, 3, 8, false, 0), None);
        // the parameter page: the leftmost encoder's parameter
        assert_eq!(encoder_param(0, 8, 9, 2, false, 4), Some(4));
        assert_eq!(encoder_param(2, 8, 9, 2, false, 4), Some(5));
        assert_eq!(encoder_param(0, 8, 9, 2, false, 12), None);
        // instrument and plug-in pages apart
        assert_ne!(page_key(MODE_INSTRUMENT), page_key(MODE_PLUGIN));
    }

    #[test]
    fn only_when_all_fit() {
        // the guide's example: 24 encoders, 13 parameters one each, 11 parameters two each
        assert_eq!(per_param(2, 24, 13, true), 1);
        assert_eq!(per_param(2, 24, 11, true), 2);
        assert_eq!(encoder_param(12, 24, 13, 2, true, 0), Some(12));
        assert_eq!(encoder_param(21, 24, 11, 2, true, 0), None);
        assert_eq!(encoder_param(20, 24, 11, 2, true, 0), Some(10));
        // off: always the setting, paging instead
        assert_eq!(per_param(2, 24, 13, false), 2);
        assert_eq!(per_param(1, 24, 3, true), 1);
    }

    #[test]
    fn units_on_the_display() {
        assert_eq!(value_text(-6.0, Unit::Db, true), "-6.0dB");
        assert_eq!(value_text(-6.0, Unit::Db, false), "-6.0");
        assert_eq!(value_text(1200.0, Unit::Hz, true), "1.2kHz");
        assert_eq!(value_text(440.0, Unit::Hz, true), "440Hz");
        assert_eq!(value_text(50.0, Unit::Percent, true), "50%");
        assert_eq!(value_text(50.0, Unit::Percent, false), "50");
        assert_eq!(value_text(1.0, Unit::OnOff, true), "On");
    }

    #[test]
    fn pickup_waits_for_the_value() {
        let k = "test-pickup-1";
        // parameter at 0.5, fader at 0.1: nothing until it gets there
        assert_eq!(pickup(k, true, 0.1, 0.5), None);
        assert_eq!(pickup(k, true, 0.3, 0.5), None);
        // passing the value picks it up
        assert_eq!(pickup(k, true, 0.6, 0.5), Some(0.6));
        // from then on it follows (the parameter is where the control left it)
        assert_eq!(pickup(k, true, 0.2, 0.6), Some(0.2));
        // moved elsewhere (automation, the mouse): it has to reach it again
        assert_eq!(pickup(k, true, 0.25, 0.9), None);
        // Pickup mode off: the parameter jumps to the control
        assert_eq!(pickup("test-pickup-2", false, 0.1, 0.9), Some(0.1));
    }

    #[test]
    fn bandwidth_paces_feedback() {
        let t0 = Instant::now();
        let mut b = Budget::default();
        let msg = vec![0u8; 100];
        // 50%: 1562.5 B/s, a burst of 390 bytes: three 100-byte messages now, the rest wait
        let first = b.pace(t0, 50.0, vec![msg.clone(); 10]);
        assert_eq!(first.len(), 3);
        assert_eq!(b.queue.len(), 7);
        // a fifth of a second later: 312 more bytes
        let next = b.pace(t0 + Duration::from_millis(200), 50.0, Vec::new());
        assert_eq!(next.len(), 3);
        // at 100% twice as fast
        let mut c = Budget::default();
        c.pace(t0, 100.0, vec![msg.clone(); 20]);
        let n = c.pace(t0 + Duration::from_millis(200), 100.0, Vec::new()).len();
        assert_eq!(n, 7);
    }

    #[test]
    fn jog_by_zoom() {
        assert_eq!(jog_ticks(false, 100.0, 3840.0, 960.0), 240.0);
        // 4 px of a 100 px bar
        assert!((jog_ticks(true, 100.0, 3840.0, 960.0) - 153.6).abs() < 1e-6);
        // zoomed in further: finer
        assert!(jog_ticks(true, 400.0, 3840.0, 960.0) < jog_ticks(true, 100.0, 3840.0, 960.0));
    }

    fn vals(kv: &[(&str, &str)]) -> crate::settings::Values {
        let mut v = crate::settings::Values::default();
        for (k, x) in kv {
            v.0.insert(format!("{HT}{k}"), x.to_string());
        }
        v
    }

    #[test]
    fn help_tags_on_the_display() {
        let t0 = Instant::now();
        let v = vals(&[]);
        help_tag(&v, "Cutoff Frequency", "1.2kHz", t0);
        let (top, bottom) = help_rows(&v, t0 + Duration::from_millis(500));
        assert!(top.as_deref().unwrap().starts_with("Cutoff Frequency "));
        assert_eq!(top.unwrap().len(), 56);
        assert!(bottom.unwrap().starts_with("1.2kHz"));
        // multiple parameters: both until each times out (Display Duration 2 s)
        help_tag(&v, "Resonance", "40%", t0 + Duration::from_secs(1));
        assert!(help_rows(&v, t0 + Duration::from_millis(1500)).0.unwrap().starts_with("Cutoff Frequency / Resonance"));
        assert!(help_rows(&v, t0 + Duration::from_millis(2500)).0.unwrap().starts_with("Resonance "));
        assert_eq!(help_rows(&v, t0 + Duration::from_secs(4)), (None, None));
        // off: only the latest
        let one = vals(&[("Show info for multiple parameters", "0")]);
        help_tag(&one, "A", "1", t0);
        help_tag(&one, "B", "2", t0);
        assert!(help_rows(&one, t0).0.unwrap().starts_with("B "));
        // name off: the upper row stays as it is; both off: no help tags at all
        let novalue = vals(&[("Parameter name", "0")]);
        help_tag(&novalue, "C", "3", t0);
        let (top, bottom) = help_rows(&novalue, t0);
        assert!(top.is_none() && bottom.is_some());
        let none = vals(&[("Parameter name", "0"), ("Parameter value", "0")]);
        tag_volume(&none, -6.0, t0);
        assert_eq!(help_rows(&none, t0), (None, None));
        // volume and selection tags, units from Help Tags' Volume and other parameters
        let v2 = vals(&[("Show info for multiple parameters", "0")]);
        tag_volume(&v2, -6.0, t0);
        assert!(help_rows(&v2, t0).1.unwrap().starts_with("-6.0 dB"));
        let v3 = vals(&[("Show info for multiple parameters", "0"), ("Volume and other parameters", "0")]);
        tag_volume(&v3, -6.0, t0);
        assert!(help_rows(&v3, t0).1.unwrap().starts_with("-6.0 "));
        tag_selected(&v2, "Bass", t0);
        let (top, bottom) = help_rows(&v2, t0);
        assert!(top.unwrap().starts_with("Selected") && bottom.unwrap().starts_with("Bass"));
        let quiet = vals(&[("Show info for multiple parameters", "0"), ("Show info when selecting tracks", "0")]);
        tag_selected(&quiet, "Keys", t0);
        assert!(!help_rows(&quiet, t0).1.unwrap().contains("Keys"));
    }

    #[test]
    fn bank_follows_selection() {
        // 8 strips a bank, track 12 selected: the nearest bank showing it
        let shows = |b: i32| (b..b + 8).contains(&12);
        assert_eq!(bank_showing(0, 20, shows), Some(5));
        assert_eq!(bank_showing(8, 20, shows), Some(8));
        assert_eq!(bank_showing(16, 20, shows), Some(12));
        assert_eq!(bank_showing(0, 3, |_| false), None);
    }
}
