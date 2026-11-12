//! The Side Chain pop-up of an instrument's plug-in header (Logic's Side Chain / Analysis
//! Input menu): None, another track's sound, a bus, or the live input. The choice is kept per
//! track (by its stable id) and handed to the engine, which feeds the block to the instrument
//! before it renders (`Instrument::side_chain`): EVOC 20 PolySynth's analysis, ES1 / ES2's EXT
//! sources, Sculpture's External object, Ultrabeat's side-chain oscillator.

use std::collections::HashMap;

use crate::app::App;
use crate::audio::SideSrc;

/// A side-chain choice, kept by track id so it survives reordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Track(u64),
    Bus(usize),
    Input,
}

/// The buses the menu offers.
pub const BUSES: usize = 32;

/// The choices by the listening track's id.
#[derive(Default)]
pub struct State {
    pub by_track: HashMap<u64, Choice>,
    /// Effects' choices by (strip, insert slot).
    pub by_slot: HashMap<(usize, usize), Choice>,
}

fn label(app: &App, c: Option<Choice>) -> String {
    match c {
        None => "None".into(),
        Some(Choice::Input) => "Input 1-2".into(),
        Some(Choice::Bus(b)) => format!("Bus {}", b + 1),
        Some(Choice::Track(id)) => app.project.as_ref().and_then(|p| p.tracks.iter().find(|t| t.extra.id == id)).map(|t| t.name.clone()).unwrap_or_else(|| "None".into()),
    }
}

/// A choice's name (a track, Bus N, Input 1-2).
pub fn label_of(app: &App, c: Choice) -> String {
    label(app, Some(c))
}

/// The current choice for track `t`.
pub fn choice(app: &App, t: usize) -> Option<Choice> {
    let id = crate::instwin::track_id(app, t)?;
    app.side.by_track.get(&id).copied()
}

pub fn set(app: &mut App, t: usize, c: Option<Choice>) {
    let Some(id) = crate::instwin::track_id(app, t) else { return };
    match c {
        Some(c) => app.side.by_track.insert(id, c),
        None => app.side.by_track.remove(&id),
    };
    sync(app);
}

/// Hands the choices to the engine (track ids to today's indices).
pub fn sync(app: &mut App) {
    let Some(p) = app.project.as_ref() else { return };
    let idx: HashMap<u64, usize> = p.tracks.iter().enumerate().map(|(i, t)| (t.extra.id, i)).collect();
    let src: Vec<Option<SideSrc>> = p.tracks.iter().map(|t| {
        app.side.by_track.get(&t.extra.id).and_then(|c| match *c {
            Choice::Track(id) => idx.get(&id).map(|i| SideSrc::Track(*i)),
            Choice::Bus(b) => Some(SideSrc::Bus(b)),
            Choice::Input => Some(SideSrc::Input),
        })
    }).collect();
    let to_src = |c: &Choice| match *c {
        Choice::Track(id) => idx.get(&id).map(|i| SideSrc::Track(*i)),
        Choice::Bus(b) => Some(SideSrc::Bus(b)),
        Choice::Input => Some(SideSrc::Input),
    };
    let fx: HashMap<(crate::audio::FxStrip, usize), SideSrc> = app.side.by_slot.iter().filter_map(|((s, k), c)| to_src(c).map(|src| ((crate::mixer::fx_strip(*s), *k), src))).collect();
    if let Ok(mut e) = app.engine.lock() {
        if e.side_src != src {
            e.side_src = src;
        }
        if e.fx_side != fx {
            e.fx_side = fx;
        }
    }
}

/// The pop-up in the header at `r`.
pub fn popup(app: &mut App, ui: &mut egui::Ui, r: egui::Rect, t: usize) {
    let cur = choice(app, t);
    if let Some(c) = menu(app, ui, r, egui::Id::unique(("side-chain", t)), cur, Some(t), "None") {
        set(app, t, c);
    }
}

/// An effect's Side Chain pop-up (strip `s`, insert slot `k`); `none` is what no choice is
/// called ("Internal" for plug-ins that otherwise listen to their own input, as Compressor).
pub fn fx_popup(app: &mut App, ui: &mut egui::Ui, r: egui::Rect, s: usize, k: usize, none: &str) {
    let cur = app.side.by_slot.get(&(s, k)).copied();
    let own = (s < crate::mixer::AUX).then_some(s);
    if let Some(c) = menu(app, ui, r, egui::Id::unique(("fx-side-chain", s, k)), cur, own, none) {
        match c {
            Some(c) => app.side.by_slot.insert((s, k), c),
            None => app.side.by_slot.remove(&(s, k)),
        };
        sync(app);
    }
}

/// The menu: None (or Internal), Input, Bus ▸, the other tracks, with a search field.
fn menu(app: &App, ui: &mut egui::Ui, r: egui::Rect, id: egui::Id, cur: Option<Choice>, own: Option<usize>, none: &str) -> Option<Option<Choice>> {
    let shown = if cur.is_none() { none.to_string() } else { label(app, cur) };
    // (the header draws the "Side Chain:" label beside it)
    let resp = crate::plughead::side_chain_button(ui, r, id, &shown).on_hover_text("Side Chain");
    let tracks: Vec<(u64, String)> = app.project.as_ref().map(|p| p.tracks.iter().enumerate().filter(|(i, _)| Some(*i) != own).map(|(_, tr)| (tr.extra.id, tr.name.clone())).collect()).unwrap_or_default();
    let mut pick: Option<Option<Choice>> = None;
    let search_id = id.with("search");
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(180.0);
        // the search field (finds a source by name)
        let mut find: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut find).hint_text("Search").desired_width(170.0));
        ui.data_mut(|d| d.insert_temp(search_id, find.clone()));
        let hit = |n: &str| find.is_empty() || n.to_lowercase().contains(&find.to_lowercase());
        if ui.add(egui::Button::selectable(cur.is_none(), none)).clicked() {
            pick = Some(None);
            ui.close();
        }
        ui.separator();
        if hit("Input 1-2") {
            ui.menu_button("Input", |ui| {
                if ui.add(egui::Button::selectable(cur == Some(Choice::Input), "Input 1-2")).clicked() {
                    pick = Some(Some(Choice::Input));
                    ui.close();
                }
            });
        }
        ui.menu_button("Bus", |ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for b in 0..BUSES {
                    let n = format!("Bus {}", b + 1);
                    if hit(&n) && ui.add(egui::Button::selectable(cur == Some(Choice::Bus(b)), n)).clicked() {
                        pick = Some(Some(Choice::Bus(b)));
                        ui.close();
                    }
                }
            });
        });
        if !tracks.is_empty() {
            ui.separator();
        }
        for (tid, name) in &tracks {
            if hit(name) && ui.add(egui::Button::selectable(cur == Some(Choice::Track(*tid)), name.as_str())).clicked() {
                pick = Some(Some(Choice::Track(*tid)));
                ui.close();
            }
        }
    });
    pick
}
