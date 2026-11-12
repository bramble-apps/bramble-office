//! Region Solo as Logic's: the control bar's Solo button (Toggle Region Solo) solos the
//! selected regions; while it is on, selecting regions (Shift-click adds, also during playback)
//! changes what plays. Option-clicking the button (Set Solo Lock Mode) locks the soloed
//! regions: selecting others no longer changes them; Reselect Solo-Locked Regions selects them.
//! The Solo tool: press and hold a region to hear it alone from the pointer (with the other
//! selected regions when it is one of them; Option: from the region's, or the leftmost selected
//! region's, start) until the button is released.

use crate::app::App;

pub const COMMANDS: &[&str] = &["Set Solo Lock Mode", "Reselect Solo-Locked Regions"];

#[derive(Clone, Debug, Default)]
struct St {
    locked: bool,
    /// the Solo button as last seen
    mode: bool,
    last_sel: Vec<(usize, usize)>,
    /// the Solo tool held: what to restore (region solo on, soloed, was playing)
    hold: Option<(bool, Vec<(usize, usize)>, bool)>,
}

fn id() -> egui::Id {
    egui::Id::unique("lg-regsolo")
}

fn st(ctx: &egui::Context) -> St {
    ctx.data(|d| d.get_temp::<St>(id())).unwrap_or_default()
}

fn put(ctx: &egui::Context, s: St) {
    ctx.data_mut(|d| d.insert_temp(id(), s));
}

pub fn locked(app: &App) -> bool {
    st(&app.ctx).locked
}

/// The control bar's Solo button: a click toggles Region Solo, Option-click the lock.
pub fn button(app: &mut App, alt: bool) {
    if alt {
        command(app, "Set Solo Lock Mode");
    } else {
        app.solo_mode = !app.solo_mode;
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    let mut s = st(&app.ctx);
    match name {
        "Set Solo Lock Mode" => {
            if !app.solo_mode {
                app.solo_mode = true;
            }
            s.locked = !s.locked;
            app.tell(if s.locked { "Solo locked: selecting regions doesn't change what is soloed" } else { "Solo lock off" });
        }
        "Reselect Solo-Locked Regions" => {
            if !app.rp.soloed.is_empty() {
                app.selected_regions = app.rp.soloed.clone();
                app.selected_region = app.selected_regions.last().copied();
            }
        }
        _ => return false,
    }
    put(&app.ctx, s);
    true
}

/// Every frame: Region Solo follows the Solo button and, unless locked, the selection; the
/// Solo tool's hold ends on release.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let mut s = st(ctx);
    let down = ctx.input(|i| i.pointer.primary_down());
    if let Some((on, soloed, was_playing)) = s.hold.clone() {
        if !down {
            s.hold = None;
            if app.playing() && !was_playing {
                app.play_stop();
            }
            app.rp.region_solo = on;
            app.rp.soloed = soloed;
            app.load_engine();
        }
        put(ctx, s);
        return;
    }
    if app.solo_mode != s.mode {
        s.mode = app.solo_mode;
        app.rp.region_solo = app.solo_mode;
        app.rp.soloed = if app.solo_mode { app.region_selection() } else { Vec::new() };
        if !app.solo_mode {
            s.locked = false;
        }
        s.last_sel = app.region_selection();
        app.load_engine();
    } else if app.solo_mode && app.rp.region_solo && !s.locked {
        let sel = app.region_selection();
        if sel != s.last_sel {
            app.rp.soloed = sel.clone();
            s.last_sel = sel;
            app.load_engine();
        }
    }
    put(ctx, s);
}

/// The Solo tool pressed on region (t, r) at `tick`.
pub fn hold_start(app: &mut App, t: usize, r: usize, tick: Option<u32>, alt: bool) {
    let mut s = st(&app.ctx);
    if s.hold.is_some() {
        return;
    }
    let sel = app.region_selection();
    let regions = if sel.contains(&(t, r)) { sel } else { vec![(t, r)] };
    let from = if alt {
        regions.iter().filter_map(|&(tt, rr)| app.project.as_ref()?.tracks.get(tt)?.regions.get(rr).map(|g| g.start)).min()
    } else {
        tick
    };
    s.hold = Some((app.rp.region_solo, app.rp.soloed.clone(), app.playing()));
    put(&app.ctx, s);
    app.rp.region_solo = true;
    app.rp.soloed = regions;
    app.load_engine();
    if let Some(at) = from {
        app.locate(at as f64);
    }
    if !app.playing() {
        app.play_stop();
    }
}

pub fn holding(app: &App) -> bool {
    st(&app.ctx).hold.is_some()
}
