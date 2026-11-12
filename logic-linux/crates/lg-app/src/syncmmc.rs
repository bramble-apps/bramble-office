//! MMC tape tracks: tracks with the MMC icon (Option in the Icon palette shows it, under Other)
//! at the top of the track list control an MMC tape machine's tracks, one each (up to 64).
//! Selecting one record-enables that tape track and disables the others (Shift adds,
//! Control-Shift toggles, Control takes the others out); Record Toggle toggles the selected
//! ones. While recording, the autopunch locators punch the machine in and out (MMC Record
//! Strobe / Exit), and stopping makes an empty MIDI region on each record-enabled tape track
//! (none where one starts at the same place already). syncmidi.rs sends MMC Play, Stop and
//! Locate (Transmit MMC).

use lg_core::sync::{self, Mmc};

use crate::app::App;

pub const MMC_ICON: &str = "MMC";
pub const COMMANDS: &[&str] = &[];

#[derive(Default)]
pub struct State {
    /// The tape tracks record-enabled (bit n: tape track n + 1).
    pub armed: u64,
    last_sel: Option<usize>,
    rec_from: Option<f64>,
    punched: bool,
    last_pos: f64,
}

/// The tape tracks: the leading tracks with the MMC icon (a folder at the top holding them is
/// the top track), at most 64.
pub fn tape_tracks(app: &App) -> Vec<usize> {
    let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let mut v = Vec::new();
    for t in 0..n {
        let mmc = crate::tm::set(app, t).and_then(|s| s.icon.as_deref()) == Some(MMC_ICON);
        if !mmc {
            break;
        }
        v.push(t);
    }
    v.truncate(64);
    v
}

/// A selection with modifiers (`shift`, `ctrl`): the new record-enable mask for tape track `k`.
pub fn select_mask(armed: u64, k: usize, shift: bool, ctrl: bool) -> u64 {
    let bit = 1u64 << k.min(63);
    match (shift, ctrl) {
        (true, true) => armed ^ bit,
        (true, false) => armed | bit,
        _ => bit,
    }
}

fn set_armed(app: &mut App, mask: u64) {
    if app.hub.mmc.armed != mask {
        app.hub.mmc.armed = mask;
        crate::syncmidi::send_arm(app, mask);
    }
}

/// The MMC icon in the Icon palette's Other category (shown while Option is held, or when the
/// track has it). Some(name) when clicked.
pub fn icon_row(ui: &mut egui::Ui, current: Option<&str>) -> Option<String> {
    let alt = ui.input(|i| i.modifiers.alt);
    if !alt && current != Some(MMC_ICON) {
        return None;
    }
    let mut pick = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Other").color(crate::look::text_dim()));
        let on = current == Some(MMC_ICON);
        if ui.add(egui::Button::selectable(on, "MMC")).on_hover_text("MMC: the track controls a tape machine's track (MIDI Machine Control)").clicked() {
            pick = Some(MMC_ICON.to_string());
        }
    });
    pick
}

/// Record Toggle with a tape track selected: toggles the selected tape tracks. True when taken.
pub fn command(app: &mut App, name: &str) -> bool {
    if !matches!(name, "Record Toggle" | "Record / Record Toggle") {
        return false;
    }
    let tapes = tape_tracks(app);
    let Some(k) = app.selected_track.and_then(|t| tapes.iter().position(|x| *x == t)) else { return false };
    let mut mask = app.hub.mmc.armed;
    for t in app.ta.selected.iter().copied().chain(std::iter::once(tapes[k])) {
        if let Some(j) = tapes.iter().position(|x| *x == t) {
            mask ^= 1 << j;
        }
    }
    // (the selected track may be in both lists: toggled once)
    if app.ta.selected.contains(&tapes[k]) {
        mask ^= 1 << k;
    }
    set_armed(app, mask);
    true
}

/// Each frame: selection arms tape tracks, punching, the take regions when recording stops.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let tapes = tape_tracks(app);
    if tapes.is_empty() {
        app.hub.mmc.last_sel = app.selected_track;
        return;
    }
    let sel = app.selected_track;
    if sel != app.hub.mmc.last_sel {
        app.hub.mmc.last_sel = sel;
        if let Some(k) = sel.and_then(|t| tapes.iter().position(|x| *x == t)) {
            let m = ctx.input(|i| i.modifiers);
            let mask = select_mask(app.hub.mmc.armed, k, m.shift, m.ctrl);
            set_armed(app, mask);
        }
    }
    let recording = app.recording();
    let pos = app.position();
    let r = crate::projsettings::smpte(&app.proj);
    let rate = sync::Rate { fps: r.1, drop: r.2 };
    let id = crate::syncmidi::mmc_id(app);
    let ports: Vec<String> = crate::syncmidi::mmc_ports(app);
    if recording && app.hub.mmc.armed != 0 {
        if app.hub.mmc.rec_from.is_none() {
            app.hub.mmc.rec_from = Some(pos);
            app.hub.mmc.punched = false;
        }
        // autopunch: the machine punches in and out at the punch locators
        if app.autopunch {
            let (a, b) = app.rec.punch;
            let last = app.hub.mmc.last_pos;
            let send = |app: &App, m: Mmc| {
                for p in &ports {
                    app.hub.outs.send(p, &sync::mmc(id, m, rate));
                }
            };
            if last < a as f64 && pos >= a as f64 {
                send(app, Mmc::RecordStrobe);
                app.hub.mmc.punched = true;
            }
            if last < b as f64 && pos >= b as f64 && app.hub.mmc.punched {
                send(app, Mmc::RecordExit);
            }
        }
    } else if let Some(from) = app.hub.mmc.rec_from.take() {
        // recording stopped: an empty MIDI region on each record-enabled tape track
        let (start, end) = if app.autopunch { (app.rec.punch.0 as f64, app.rec.punch.1 as f64) } else { (from, app.hub.mmc.last_pos) };
        let armed = app.hub.mmc.armed;
        let tracks: Vec<usize> = tapes.iter().enumerate().filter(|(k, _)| armed & (1 << k) != 0).map(|x| *x.1).collect();
        if end > start {
            make_takes(app, &tracks, start as u32, (end - start) as u32);
        }
    }
    app.hub.mmc.last_pos = pos;
}

/// Empty MIDI regions for an MMC take (not where a region starts at the same place).
pub fn make_takes(app: &mut App, tracks: &[usize], start: u32, length: u32) -> usize {
    let Some(p) = app.project.as_mut() else { return 0 };
    let mut n = 0;
    for &t in tracks {
        let Some(tr) = p.tracks.get_mut(t) else { continue };
        if tr.regions.iter().any(|r| r.start == start) {
            continue;
        }
        tr.regions.push(lg_core::Region { name: tr.name.clone(), start, length: length.max(1), notes: Vec::new(), audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        n += 1;
    }
    if n > 0 {
        app.after_edit();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_masks_and_takes() {
        assert_eq!(select_mask(0b101, 1, false, false), 0b010);
        assert_eq!(select_mask(0b101, 1, true, false), 0b111);
        assert_eq!(select_mask(0b111, 1, true, true), 0b101);
        assert_eq!(select_mask(0b111, 2, false, true), 0b100);
        let mut app = crate::tm::test_app();
        let n = app.project.as_ref().map_or(0, |p| p.tracks.len());
        assert!(n > 0);
        assert!(tape_tracks(&app).is_empty());
        crate::tm::set_mut(&mut app, 0).unwrap().icon = Some(MMC_ICON.into());
        assert_eq!(tape_tracks(&app), vec![0]);
        assert_eq!(make_takes(&mut app, &[0], 3840, 7680), 1);
        assert_eq!(make_takes(&mut app, &[0], 3840, 7680), 0, "a region starts there already");
    }
}
