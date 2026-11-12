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
}

fn label(app: &App, c: Option<Choice>) -> String {
    match c {
        None => "None".into(),
        Some(Choice::Input) => "Input 1-2".into(),
        Some(Choice::Bus(b)) => format!("Bus {}", b + 1),
        Some(Choice::Track(id)) => app.project.as_ref().and_then(|p| p.tracks.iter().find(|t| t.extra.id == id)).map(|t| t.name.clone()).unwrap_or_else(|| "None".into()),
    }
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
    if let Ok(mut e) = app.engine.lock() {
        if e.side_src != src {
            e.side_src = src;
        }
    }
}

/// The pop-up in the header at `r`.
pub fn popup(app: &mut App, ui: &mut egui::Ui, r: egui::Rect, t: usize) {
    let cur = choice(app, t);
    let resp = ui.interact(r, egui::Id::unique(("side-chain", t)), egui::Sense::click()).on_hover_text("Side Chain");
    let p = ui.painter();
    p.rect_filled(r, 5.0, if resp.hovered() { crate::look::button_on() } else { crate::look::group_bg() });
    let text = format!("Side Chain: {}", label(app, cur));
    p.with_clip_rect(r.shrink(2.0)).text(egui::pos2(r.min.x + 8.0, r.center().y), egui::Align2::LEFT_CENTER, text, crate::look::regular(12.0), crate::look::text());
    let tracks: Vec<(u64, String)> = app.project.as_ref().map(|p| p.tracks.iter().enumerate().filter(|(i, _)| *i != t).map(|(_, tr)| (tr.extra.id, tr.name.clone())).collect()).unwrap_or_default();
    let mut pick: Option<Option<Choice>> = None;
    egui::Popup::menu(&resp).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(180.0);
        if ui.add(egui::Button::selectable(cur.is_none(), "None")).clicked() {
            pick = Some(None);
            ui.close();
        }
        ui.separator();
        ui.menu_button("Input", |ui| {
            if ui.add(egui::Button::selectable(cur == Some(Choice::Input), "Input 1-2")).clicked() {
                pick = Some(Some(Choice::Input));
                ui.close();
            }
        });
        ui.menu_button("Bus", |ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for b in 0..BUSES {
                    if ui.add(egui::Button::selectable(cur == Some(Choice::Bus(b)), format!("Bus {}", b + 1))).clicked() {
                        pick = Some(Some(Choice::Bus(b)));
                        ui.close();
                    }
                }
            });
        });
        if !tracks.is_empty() {
            ui.separator();
        }
        for (id, name) in &tracks {
            if ui.add(egui::Button::selectable(cur == Some(Choice::Track(*id)), name.as_str())).clicked() {
                pick = Some(Some(Choice::Track(*id)));
                ui.close();
            }
        }
    });
    if let Some(c) = pick {
        set(app, t, c);
    }
}
