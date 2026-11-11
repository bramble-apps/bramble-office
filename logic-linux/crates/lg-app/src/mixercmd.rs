//! The Mixer's commands by their Logic names: its Edit / Options / View menus, the Mix menu's
//! groups, the plug-in window and channel strip key commands. (build.rs and tool/checklist.py
//! read every name in this file as handled, so it holds only command names.)

use crate::app::{App, MixerView};
use crate::audio::Out;
use crate::mixer::AUX;
use crate::mixstate::{self, Group, Kind, MASTER, VCA};

fn kind_filter(name: &str) -> Option<Kind> {
    Some(match name {
        "Show/Hide Audio Channel Strips" | "Scroll to Audio" => Kind::Audio,
        "Inst" | "Show/Hide Instrument Channel Strips" | "Scroll to Instruments" => Kind::Inst,
        "Show/Hide Auxiliary Channel Strips" | "Scroll to Auxiliaries" => Kind::Aux,
        "Bus" | "Show/Hide Bus Channel Strips" | "Scroll to Buses" => Kind::Bus,
        "Input" | "Show/Hide Input Channel Strips" | "Scroll to Inputs" => Kind::Input,
        "Output" | "Show/Hide Output Channel Strips" | "Scroll to Outputs" => Kind::Output,
        "Master/VCA" | "Scroll to Master/VCA" => Kind::MasterVca,
        "Show/Hide MIDI Instruments" | "Scroll to MIDI Instruments" => Kind::Midi,
        _ => return None,
    })
}

/// Channel strip components (View ▸ Channel Strip Components and its key commands).
fn component(name: &str) -> Option<&'static str> {
    Some(match name {
        "Setting Menu" | "Show/Hide Setting Menu" => "Setting Menu",
        "Gain Reduction Meter" | "Show/Hide Gain Reduction Meter" => "Gain Reduction Meter",
        "EQ Thumbnail" | "Show/Hide EQ Thumbnails" => "EQ Thumbnail",
        "MIDI Effects" | "Show/Hide MIDI Effects" => "MIDI Effects",
        "Input / Instrument" | "Show/Hide Input / Instrument" => "Input / Instrument",
        "Audio Effects" | "Show/Hide Audio Effects" => "Audio Effects",
        "Sends" | "Show/Hide Sends" => "Sends",
        "Show/Hide Output" => "Output",
        "Group" | "Show/Hide Grouping" => "Group",
        "Show/Hide Automation Mode Menu" => "part:Automation",
        "Pan Knob" | "Show/Hide Pan Knob" => "Pan Knob",
        "VCA" | "Show/Hide VCA" => "VCA",
        "Show/Hide Track Name" => "part:Track Name",
        _ => return None,
    })
}

/// The strips the selection-based commands act on: the Mixer's selection, else the selected
/// track.
fn targets(app: &App) -> Vec<usize> {
    if !app.mix.selected.is_empty() {
        return app.mix.selected.iter().copied().collect();
    }
    app.selected_track.into_iter().collect()
}

/// The filter buttons (Audio, Inst, Aux … MIDI): show or hide that kind of strip. (Their
/// labels are also other menus' items, so they aren't matched by name.)
pub fn toggle_kind(app: &mut App, k: Kind) {
    if !app.mix.hidden.remove(&k) {
        app.mix.hidden.insert(k);
    }
}

/// Runs a Mixer command; false when it isn't one.
pub fn run(app: &mut App, ctx: &egui::Context, name: &str) -> bool {
    let _ = ctx;
    let scroll_to = match name {
        "Instruments" => Some(Kind::Inst),
        "Auxiliaries" => Some(Kind::Aux),
        "Buses" => Some(Kind::Bus),
        "Inputs" => Some(Kind::Input),
        "Outputs" => Some(Kind::Output),
        "MIDI Instruments" => Some(Kind::Midi),
        _ => None,
    };
    if let Some(k) = scroll_to.or_else(|| kind_filter(name)) {
        if scroll_to.is_some() || name.starts_with("Scroll to") {
            let first = mixstate::all_strips(app).into_iter().position(|s| mixstate::kind_of(app, s) == k);
            if let Some(i) = first {
                app.mix.scroll = i as f32;
            }
            return true;
        }
        if !app.mix.hidden.remove(&k) {
            app.mix.hidden.insert(k);
        }
        return true;
    }
    if let Some(c) = component(name) {
        if !app.mix.hidden_parts.remove(c) {
            app.mix.hidden_parts.insert(c.to_string());
        }
        return true;
    }
    if let Some(n) = name.strip_prefix("Select Members of Group ").and_then(|n| n.parse::<usize>().ok()) {
        let members: Vec<usize> = app.mix.group_of.iter().filter(|(_, g)| **g + 1 == n).map(|(s, _)| *s).collect();
        if members.is_empty() {
            return true;
        }
        app.mix.selected = members.into_iter().collect();
        return true;
    }
    match name {
        "Mixer Undo" => {
            mixstate::undo(app);
        }
        "Mixer Redo" => {
            mixstate::redo(app);
        }
        "Mixer Undo Deluxe Classic: Solo" => {
            mixstate::undo(app);
        }
        "Delete Mixer Undo History" => {
            app.mix.undo.clear();
            app.mix.redo.clear();
        }
        "Include Mixer Undo Steps in Project Undo History" => app.mix.include_in_project_undo = !app.mix.include_in_project_undo,
        "Cycle Through Mixer Modes (Single, Tracks, All)" => {
            app.mixer_view = match app.mixer_view {
                MixerView::Single => MixerView::Tracks,
                MixerView::Tracks => MixerView::All,
                MixerView::All => MixerView::Single,
            }
        }
        "Single" => app.mixer_view = MixerView::Single,
        "All" => app.mixer_view = MixerView::All,
        "Narrow Channel Strips" => app.mix.narrow = true,
        "Wide Channel Strips" => app.mix.narrow = false,
        "Toggle Narrow/Wide Channel Strips" => app.mix.narrow = !app.mix.narrow,
        "Select Audio Channel Strips" | "Select Instrument Channel Strips" | "Select Auxiliary Channel Strips" | "Select Output Channel Strips" => {
            let k = match name {
                "Select Audio Channel Strips" => Kind::Audio,
                "Select Instrument Channel Strips" => Kind::Inst,
                "Select Auxiliary Channel Strips" => Kind::Aux,
                _ => Kind::MasterVca,
            };
            app.mix.selected = mixstate::all_strips(app).into_iter().filter(|s| mixstate::kind_of(app, *s) == k && (k != Kind::MasterVca || *s == MASTER)).collect();
        }
        "Select Muted Channel Strips" => {
            app.mix.selected = mixstate::all_strips(app).into_iter().filter(|s| mixstate::values(app, *s).is_some_and(|v| v.2)).collect();
        }
        "Select Same-Colored Channel Strips" => {
            let Some(&first) = targets(app).first() else { return true };
            let k = mixstate::kind_of(app, first);
            let tk = app.project.as_ref().and_then(|p| p.tracks.get(first)).map(|t| t.kind);
            app.mix.selected = mixstate::all_strips(app).into_iter().filter(|s| mixstate::kind_of(app, *s) == k && (*s >= AUX || app.project.as_ref().and_then(|p| p.tracks.get(*s)).map(|t| t.kind) == tk)).collect();
        }
        "Create New Auxiliary Channel Strip" => {
            mixstate::checkpoint(app, "Create Aux");
            let a = mixstate::new_aux(app, None);
            app.mix.selected = [AUX + a].into_iter().collect();
            app.load_engine();
        }
        "Create New VCA for Selected Channel Strips" | "Create New VCA Fader" | "Create New VCA" => {
            let members = if name == "Create New VCA Fader" { Vec::new() } else { targets(app) };
            mixstate::new_vca(app, &members);
            app.load_engine();
        }
        "Enable Groups" | "Enable/Disable Groups" | "Groups Active" => app.mix.groups_enabled = !app.mix.groups_enabled,
        "Open Group Settings…" | "Group Settings" | "Show/Hide Groups Window" => {
            let g = targets(app).first().and_then(|s| app.mix.group_of.get(s).copied()).or((!app.mix.groups.is_empty()).then_some(0));
            app.mix.group_window = if app.mix.group_window.is_some() && name == "Show/Hide Groups Window" { None } else { g };
        }
        "Create Group for Selected Channel Strips / Tracks" | "Create Group" => {
            mixstate::checkpoint(app, "Create Group");
            app.mix.groups.push(Group::new());
            let g = app.mix.groups.len() - 1;
            for s in targets(app) {
                app.mix.group_of.insert(s, g);
            }
            app.mix.group_window = Some(g);
        }
        "Delete Selected Groups" => {
            let gs: Vec<usize> = targets(app).iter().filter_map(|s| app.mix.group_of.get(s).copied()).collect();
            mixstate::checkpoint(app, "Delete Group");
            app.mix.group_of.retain(|_, g| !gs.contains(g));
        }
        "Delete Unused Groups" => {
            mixstate::checkpoint(app, "Delete Unused Groups");
            while app.mix.groups.len() > 0 && !app.mix.group_of.values().any(|g| *g == app.mix.groups.len() - 1) {
                app.mix.groups.pop();
            }
        }
        "Select Members of Selected Groups" => {
            let gs: Vec<usize> = targets(app).iter().filter_map(|s| app.mix.group_of.get(s).copied()).collect();
            app.mix.selected = app.mix.group_of.iter().filter(|(_, g)| gs.contains(g)).map(|(s, _)| *s).collect();
        }
        "Add Selected Channel Strips to Selected Groups" => {
            let Some(g) = app.mix.group_window else { return true };
            mixstate::checkpoint(app, "Group");
            for s in targets(app) {
                app.mix.group_of.insert(s, g);
            }
        }
        "Remove Selected Channel Strips from Selected Groups" => {
            mixstate::checkpoint(app, "Group");
            for s in targets(app) {
                app.mix.group_of.remove(&s);
            }
        }
        "Copy Group Settings" => {
            let g = app.mix.group_window.or_else(|| targets(app).first().and_then(|s| app.mix.group_of.get(s).copied()));
            app.mix.group_clipboard = g.and_then(|g| app.mix.groups.get(g)).map(|g| g.mask);
        }
        "Paste Group Settings" => {
            let g = app.mix.group_window.or_else(|| targets(app).first().and_then(|s| app.mix.group_of.get(s).copied()));
            if let (Some(g), Some(m)) = (g, app.mix.group_clipboard) {
                mixstate::checkpoint(app, "Group Settings");
                if let Some(x) = app.mix.groups.get_mut(g) {
                    x.mask = m;
                }
            }
        }
        "Bypass All Effect Plug-ins" => {
            mixstate::checkpoint(app, "Bypass");
            for s in mixstate::all_strips(app) {
                mixstate::edit_inserts(app, s, |v| v.iter_mut().for_each(|u| u.bypass = true));
            }
            app.load_engine();
        }
        "Remove Bypassed Plug-ins" => {
            mixstate::checkpoint(app, "Remove Bypassed Plug-ins");
            for s in targets(app) {
                mixstate::edit_inserts(app, s, |v| v.retain(|u| !u.bypass));
            }
            app.load_engine();
        }
        "Copy Channel Strip Setting" => {
            let Some(&s) = targets(app).first() else { return true };
            app.mix.strip_clipboard = Some((mixstate::inserts(app, s), app.sends.get(&s).cloned().unwrap_or_default(), (s < AUX).then(|| app.eqs.get(&s).copied()).flatten()));
        }
        "Paste Channel Strip Setting" | "Paste Channel Strip Setting Plug-ins Only" | "Paste Channel Strip Setting Sends Only" => {
            let Some((fx, sends, eq)) = app.mix.strip_clipboard.clone() else { return true };
            mixstate::checkpoint(app, "Paste Channel Strip Setting");
            for s in targets(app) {
                if name != "Paste Channel Strip Setting Sends Only" {
                    let fx = fx.clone();
                    mixstate::edit_inserts(app, s, |v| *v = fx);
                    if let (true, Some(e)) = (s < AUX, eq) {
                        app.eqs.insert(s, e);
                    }
                }
                if name != "Paste Channel Strip Setting Plug-ins Only" && s != MASTER && s < VCA {
                    app.sends.insert(s, sends.clone());
                }
            }
            app.load_engine();
        }
        "Reset Channel Strip Setting" => {
            mixstate::checkpoint(app, "Reset Channel Strip Setting");
            for s in targets(app) {
                mixstate::edit_inserts(app, s, |v| v.clear());
                app.sends.remove(&s);
                app.eqs.remove(&s);
                mixstate::set_values(app, s, (0.0, 0.0, false, false));
            }
            app.load_engine();
        }
        "Show/Hide All Plug-in Windows" | "Hide All Plug-in Windows" => {
            if app.mix.fx_window.is_some() || app.plugin_window.is_some() || app.eq_window.is_some() {
                app.mix.hidden_windows = (app.mix.fx_window.take(), app.plugin_window.take(), app.eq_window.take());
            } else {
                (app.mix.fx_window, app.plugin_window, app.eq_window) = std::mem::take(&mut app.mix.hidden_windows);
            }
        }
        "Next Plug-in Setting for topmost Plug-in Window" | "Previous Plug-in Setting for topmost Plug-in Window" => {
            if let Some((s, k)) = app.mix.fx_window {
                crate::pluginwin::step_setting(app, s, k, if name.starts_with("Next") { 1 } else { -1 });
            }
        }
        "Mastering Assistant…" => {
            mixstate::checkpoint(app, "Insert Mastering Assistant");
            if !app.mix.master.iter().any(|u| u.name == "Mastering Assistant") {
                app.mix.master.push(mixstate::new_plugin("Mastering Assistant"));
            }
            let k = app.mix.master.iter().position(|u| u.name == "Mastering Assistant").unwrap_or(0);
            app.mix.fx_window = Some((MASTER, k));
            app.load_engine();
        }
        "Sends on Faders - On/Off" => {
            app.sends_on_faders = match app.sends_on_faders {
                Some(_) => None,
                None => app.auxes.first().map(|a| a.bus),
            }
        }
        "Sends on Faders - Next Send" | "Sends on Faders - Previous Send" | "Sends on Faders - Cycle Through Sends" => {
            let mut buses: Vec<usize> = app.auxes.iter().map(|a| a.bus).collect();
            buses.sort();
            buses.dedup();
            if buses.is_empty() {
                return true;
            }
            let at = app.sends_on_faders.and_then(|b| buses.iter().position(|x| *x == b));
            app.sends_on_faders = match (name, at) {
                ("Sends on Faders - Previous Send", Some(0)) => None,
                ("Sends on Faders - Previous Send", Some(i)) => Some(buses[i - 1]),
                ("Sends on Faders - Previous Send", None) => buses.last().copied(),
                (_, None) => Some(buses[0]),
                (_, Some(i)) if i + 1 < buses.len() => Some(buses[i + 1]),
                ("Sends on Faders - Cycle Through Sends", _) => Some(buses[0]),
                _ => None,
            };
        }
        "No Output" | "Stereo Output" => {
            for s in targets(app) {
                mixstate::set_output(app, s, if name == "No Output" { Out::Off } else { Out::Stereo });
            }
        }
        "Pre-Fader Metering" => app.mix.pre_fader_meters = !app.mix.pre_fader_meters,
        _ => return false,
    }
    let _ = VCA;
    true
}

/// A checkmark for the toggles (None: not a Mixer item).
pub fn checked(app: &App, label: &str) -> Option<bool> {
    if let Some(k) = kind_filter(label).filter(|_| !label.starts_with("Scroll to")) {
        return Some(!app.mix.hidden.contains(&k));
    }
    if let Some(c) = component(label) {
        return Some(!app.mix.hidden_parts.contains(c));
    }
    Some(match label {
        "Single" => app.mixer_view == MixerView::Single,
        "All" => app.mixer_view == MixerView::All,
        "Narrow Channel Strips" => app.mix.narrow,
        "Wide Channel Strips" => !app.mix.narrow,
        "Enable Groups" | "Groups Active" => app.mix.groups_enabled,
        "Include Mixer Undo Steps in Project Undo History" => app.mix.include_in_project_undo,
        "Pre-Fader Metering" => app.mix.pre_fader_meters,
        _ => return None,
    })
}
