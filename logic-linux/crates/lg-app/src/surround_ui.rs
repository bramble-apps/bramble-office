//! Surround and Spatial Audio in the interface: the channel strip's Surround Panner and 3D
//! Object Panner (with the Elevation slider), their windows (Link, Settings, Planar /
//! Spherical, speakers on and off, Angle / Diversity / Elevation / Spread / Center / LFE,
//! Separation with its frame; the object window's two grids), the Output slot's Surround and
//! 3D Object Panner, the surround master's meters and Bounce button and its plug-in menu,
//! the Dolby Atmos plug-in window (Monitoring Format, 3D Object Viewer, Surround Bed and 3D
//! Objects lists, Binaural Renderer, Downmix & Trim), Project Settings ▸ Audio ▸ General's
//! Spatial Audio and Surround Format, Settings ▸ Audio ▸ I/O Assignments (Output, Bounce
//! Extensions, Input), the Bounce window's Surround Bounce (Split / Interleaved), File ▸ Export
//! ▸ Project as Spatial Audio (ADM BWF) and File ▸ Import ▸ Create Project from ADM BWF.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{pos2, vec2, Align2, Color32, Pos2, Rect, Sense, Stroke};
use lg_dsp::surround::{self as sur, BinMode, Ch, Format, Monitoring, Object, Pan};

use crate::app::App;
use crate::audio::Out;
use crate::look;
use crate::mixer::AUX;
use crate::surround::{self as s, ATMOS};

pub const COMMANDS: &[&str] = &["Open Surround Settings…", "Project as Spatial Audio…", "Create Project from ADM BWF…", "Dolby Atmos…"];

const GREEN: Color32 = Color32::from_rgb(0x4c, 0xd9, 0x64);
const BLUE: Color32 = Color32::from_rgb(0x4f, 0xa8, 0xe8);
const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x9a, 0x30);

/// An open Surround Panner or 3D Object Panner window.
#[derive(Clone, Debug, PartialEq)]
pub struct PannerWin {
    pub id: u64,
    pub strip: usize,
    /// Link: follows the selected channel strip.
    pub link: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AtmosPane {
    #[default]
    List,
    Binaural,
    Trim,
}

/// The Bounce window's choices.
#[derive(Clone, Debug)]
pub struct BounceDlg {
    pub pcm: bool,
    pub mp3: bool,
    pub m4a: bool,
    pub burn: bool,
    pub file_format: usize,
    pub bits: usize,
    pub surround: bool,
    pub split: bool,
    pub music_library: bool,
    pub add_to_project: bool,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug)]
pub struct ExportDlg {
    pub selection: bool,
    pub format: usize,
    pub also_adm: bool,
}

#[derive(Default)]
pub struct Ui {
    pub panners: Vec<PannerWin>,
    next_id: u64,
    pub atmos_open: bool,
    pub atmos_list: bool,
    pub atmos_pane: AtmosPane,
    pub trim_layout: usize,
    /// The 3D Object Viewer: yaw / pitch (degrees), the preset chosen (6 = Custom), and the
    /// manually dragged angle Custom recalls.
    pub view: (f32, f32),
    pub view_preset: usize,
    pub custom_view: (f32, f32),
    pub selected_objects: BTreeSet<usize>,
    pub bounce: Option<BounceDlg>,
    pub export: Option<ExportDlg>,
    /// A bounce or export running: its report when done.
    pub job: Option<Arc<Mutex<Option<String>>>>,
    pub notice: Option<String>,
}

// ---- commands ---------------------------------------------------------------------------------------

pub fn command(app: &mut App, _ctx: &egui::Context, name: &str) -> bool {
    match name {
        "Open Surround Settings…" => {
            app.settings_tab = "Audio".into();
            app.settings_sub = "I/O Assignments".into();
            app.show_settings = true;
        }
        "Project as Spatial Audio…" => open_export(app),
        "Create Project from ADM BWF…" => import_adm_dialog(app),
        "Dolby Atmos…" => {
            if !app.sur.atmos {
                s::set_atmos(app, true);
            }
            app.sur_ui.atmos_open = true;
        }
        _ => return false,
    }
    true
}

fn open_export(app: &mut App) {
    if !app.sur.atmos {
        app.tell("Project as Spatial Audio needs Spatial Audio: Dolby Atmos (File ▸ Project Settings ▸ Audio)");
        return;
    }
    app.sur_ui.export = Some(ExportDlg { selection: false, format: 0, also_adm: false });
}

// ---- the Output slot ----------------------------------------------------------------------------

/// The Output slot's Spatial Audio entries: 3D Object Panner (object track) and, on an object
/// track, Pan (back to a bed track).
pub fn output_items(app: &App, ui: &mut egui::Ui, o: Out, pick: &mut Option<Out>) {
    if !app.sur.atmos {
        return;
    }
    ui.separator();
    if ui.add(egui::Button::selectable(o == Out::Object, "3D Object Panner")).clicked() {
        *pick = Some(Out::Object);
    }
    if o == Out::Object && ui.button("Pan").clicked() {
        *pick = Some(Out::Surround);
    }
}

/// The engine's copy of one strip's panning, updated without rebuilding the engine.
fn push(app: &mut App, sid: usize) {
    let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
    let node = if sid >= AUX { n_tracks + sid - AUX } else { sid };
    let pan = app.sur.pan(sid);
    let obj = app.sur.object(sid);
    if let Ok(mut e) = app.engine.lock() {
        if let Some(Some(sp)) = e.surround.as_mut().and_then(|x| x.strips.get_mut(node)) {
            match sp {
                s::StripPan::Panner { pan: p, .. } | s::StripPan::Balancer { pan: p } => *p = pan,
                s::StripPan::Object { obj: o, .. } => *o = obj,
            }
        }
    }
    app.dirty = true;
}

fn field_xy(c: Pos2, rad: f32, x: f32, y: f32) -> Pos2 {
    pos2(c.x + x * rad, c.y - y * rad)
}

fn speaker_xy(f: Format, ch: Ch) -> (f32, f32) {
    let (az, _) = sur::position(f, ch);
    let a = az.to_radians();
    (a.sin(), a.cos())
}

/// The channel strip's pan control on a surround strip: the circular Surround Panner (speaker
/// dots, green puck; a stereo strip's spread shaded) or the square 3D Object Panner, and the
/// Elevation slider beside it when there is height.
pub fn pan_control(app: &mut App, ui: &mut egui::Ui, p: &egui::Painter, r: Rect, y: f32, i: usize) {
    let object = s::output_of_is_object(app, i);
    let fmt = app.sur.format;
    let height = object || fmt.has_height();
    let cx = r.center().x - if height { 7.0 } else { 0.0 };
    let c = pos2(cx, y + 18.0);
    let id = ui.scope_id().with(("surpan", i));
    let mods = ui.input(|x| x.modifiers);
    if object {
        let o = app.sur.object(i);
        let sq = Rect::from_center_size(c, vec2(32.0, 32.0));
        p.rect_filled(sq, 3.0, Color32::from_gray(0x2a));
        p.rect_stroke(sq, 3.0, Stroke::new(1.0, Color32::from_gray(0x55)), egui::StrokeKind::Inside);
        p.line_segment([pos2(sq.center().x, sq.min.y + 2.0), pos2(sq.center().x, sq.max.y - 2.0)], Stroke::new(0.5, Color32::from_gray(0x50)));
        p.line_segment([pos2(sq.min.x + 2.0, sq.center().y), pos2(sq.max.x - 2.0, sq.center().y)], Stroke::new(0.5, Color32::from_gray(0x50)));
        let pk = field_xy(c, 14.0, o.x, o.y);
        if o.size > 0.01 {
            p.circle_filled(pk, 3.0 + o.size * 8.0, GREEN.gamma_multiply(0.25));
        }
        p.circle_filled(pk, 3.0, GREEN);
        let resp = ui.interact(sq, id, Sense::click_and_drag()).on_hover_text(format!("3D Object Panner: L/R {:+.2}, B/F {:+.2}, Elev {:.2}", o.x, o.y, o.z));
        if resp.drag_started() {
            crate::mixstate::checkpoint(app, "3D Object Panner");
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let mut o = app.sur.object(i);
                o.x = ((pp.x - c.x) / 14.0).clamp(-1.0, 1.0);
                o.y = (-(pp.y - c.y) / 14.0).clamp(-1.0, 1.0);
                app.sur.objects.insert(i, o);
                push(app, i);
                s::record(app, i, &[s::P_X, s::P_Y]);
            }
        }
        if resp.clicked() && mods.alt {
            crate::mixstate::checkpoint(app, "3D Object Panner");
            let mut o = app.sur.object(i);
            o.x = 0.0;
            o.y = 1.0;
            app.sur.objects.insert(i, o);
            push(app, i);
        }
        if resp.double_clicked() {
            open_panner(app, i);
        }
        resp.context_menu(|ui| {
            if ui.button("Pan").clicked() {
                for x in s::chosen(app, i) {
                    crate::mixstate::set_output(app, x, Out::Surround);
                }
                ui.close();
            }
            if ui.button("Save As Default").clicked() {
                app.sur.default_object = Some(app.sur.object(i));
                ui.close();
            }
        });
    } else {
        let pan = app.sur.pan(i);
        let balancer = s::surround_input(app, i);
        let stereo = s::strip_stereo(app, i) && !balancer;
        p.circle_filled(c, 16.0, Color32::from_gray(0x2a));
        p.circle_stroke(c, 16.0, Stroke::new(1.0, Color32::from_gray(0x55)));
        for ch in fmt.channels().iter().filter(|x| **x != Ch::Lfe && !x.height()) {
            let (x, y) = speaker_xy(fmt, *ch);
            let col = if pan.is_off(*ch) { Color32::from_gray(0x60) } else { Color32::from_gray(0xc0) };
            p.circle_filled(field_xy(c, 15.0, x, y), 1.5, col);
        }
        let (px, py) = pan.puck();
        if stereo {
            let half = pan.spread / 2.0;
            for (side, col) in [(-1.0, BLUE), (1.0, GREEN)] {
                let a = (pan.angle + side * half).to_radians();
                let rr = 1.0 - pan.diversity;
                p.line_segment([c, field_xy(c, 13.0, a.sin() * rr, a.cos() * rr)], Stroke::new(1.0, col.gamma_multiply(0.6)));
            }
        }
        p.circle_filled(field_xy(c, 13.0, px, py), 3.0, GREEN);
        let rect = Rect::from_center_size(c, vec2(34.0, 34.0));
        let tip = if balancer { format!("Surround Balancer: Angle {:.0}°, Amount {:.2}", pan.angle, pan.amount) } else { format!("Surround Panner: Angle {:.0}°, Diversity {:.2}", pan.angle, pan.diversity) };
        let resp = ui.interact(rect, id, Sense::click_and_drag()).on_hover_text(tip);
        if resp.drag_started() {
            crate::mixstate::checkpoint(app, "Surround Panner");
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let (x, y) = ((pp.x - c.x) / 13.0, -(pp.y - c.y) / 13.0);
                let mut pan = app.sur.pan(i);
                drag_pan(&mut pan, x, y, lock_of(mods), fmt, balancer);
                app.sur.pans.insert(i, pan);
                push(app, i);
                s::record(app, i, &[s::P_ANGLE, if balancer { s::P_AMOUNT } else { s::P_DIVERSITY }]);
            }
        }
        if resp.clicked() && mods.alt {
            crate::mixstate::checkpoint(app, "Surround Panner");
            let mut pan = app.sur.pan(i);
            pan.angle = 0.0;
            pan.diversity = 0.0;
            pan.amount = 0.0;
            app.sur.pans.insert(i, pan);
            push(app, i);
        }
        if resp.double_clicked() {
            open_panner(app, i);
        }
        resp.context_menu(|ui| {
            if app.sur.atmos && i < crate::mixstate::VCA && ui.button("3D Object Panner").clicked() {
                for x in s::chosen(app, i) {
                    crate::mixstate::set_output(app, x, Out::Object);
                }
                ui.close();
            }
            if ui.button("Save As Default").clicked() {
                app.sur.default_pan = Some(app.sur.pan(i));
                ui.close();
            }
        });
    }
    // the Elevation slider
    if height {
        let sr = Rect::from_min_size(pos2(c.x + 20.0, y + 2.0), vec2(6.0, 32.0));
        p.rect_filled(sr, 3.0, look::lcd_bg());
        let v = if object { app.sur.object(i).z } else { app.sur.pan(i).elevation / 90.0 };
        let fy = sr.max.y - v * sr.height();
        p.rect_filled(Rect::from_min_max(pos2(sr.min.x, fy), sr.max), 3.0, GREEN.gamma_multiply(0.7));
        let resp = ui.interact(sr.expand(3.0), id.with("elev"), Sense::click_and_drag()).on_hover_text("Elevation");
        if resp.drag_started() {
            crate::mixstate::checkpoint(app, "Elevation");
        }
        if resp.dragged() {
            if let Some(pp) = resp.interact_pointer_pos() {
                let f = ((sr.max.y - pp.y) / sr.height()).clamp(0.0, 1.0);
                if object {
                    let mut o = app.sur.object(i);
                    o.z = f;
                    app.sur.objects.insert(i, o);
                    s::record(app, i, &[s::P_Z]);
                } else {
                    let mut pan = app.sur.pan(i);
                    pan.elevation = f * 90.0;
                    app.sur.pans.insert(i, pan);
                    s::record(app, i, &[s::P_ELEVATION]);
                }
                push(app, i);
            }
        }
    }
}

/// Lock from the modifiers while dragging: Command or Option keeps the diversity (1),
/// Control-Command or Option-Shift keeps the angle (2).
fn lock_of(m: egui::Modifiers) -> u8 {
    if (m.ctrl && m.command && !cfg!(target_os = "macos")) || (m.ctrl && m.mac_cmd) || (m.alt && m.shift) {
        2
    } else if m.command || m.alt {
        1
    } else {
        0
    }
}

fn drag_pan(pan: &mut Pan, x: f32, y: f32, lock: u8, fmt: Format, balancer: bool) {
    if balancer {
        // (the Surround Balancer: distance from the centre is the Amount)
        let r = (x * x + y * y).sqrt().min(1.0);
        if lock != 2 && r > 1e-4 {
            pan.angle = x.atan2(y).to_degrees();
        }
        if lock != 1 {
            pan.amount = r;
        }
    } else {
        pan.set_puck(x, y, lock, fmt.has_height());
    }
}

fn open_panner(app: &mut App, strip: usize) {
    if app.sur_ui.panners.iter().any(|w| w.strip == strip && !w.link) {
        return;
    }
    app.sur_ui.next_id += 1;
    let id = app.sur_ui.next_id;
    app.sur_ui.panners.push(PannerWin { id, strip, link: false });
}

// ---- the surround master -----------------------------------------------------------------------------

/// The surround master's meters: a bar per output channel in Settings ▸ View ▸ Mixer's channel
/// order, height channels in a section of their own; no labels (as Logic's).
pub fn master_meters(app: &App, ui: &egui::Ui, p: &egui::Painter, area: Rect) {
    let (layout, levels) = app.engine.lock().map(|e| e.surround.as_ref().map(|s| (s.layout.clone(), s.meters.clone())).unwrap_or_default()).unwrap_or_default();
    let layout = if layout.is_empty() { app.sur.format.channels().to_vec() } else { layout };
    let order = sur::MeterOrder::from_setting(&app.logic_settings.get("View / Mixer / Channel Order:"));
    let ordered: Vec<Ch> = match Format::of_channels(&layout) {
        Some(f) => sur::meter_order(f, order),
        None => layout.clone(),
    };
    let ear: Vec<Ch> = ordered.iter().copied().filter(|c| !c.height()).collect();
    let top: Vec<Ch> = ordered.iter().copied().filter(|c| c.height()).collect();
    let gap = if top.is_empty() { 0.0 } else { 4.0 };
    let n = (ear.len() + top.len()) as f32;
    let w = ((area.width() - gap) / n.max(1.0)).clamp(2.0, 7.0);
    let mut x = area.min.x;
    for (k, c) in ear.iter().chain(top.iter()).enumerate() {
        if k == ear.len() && !top.is_empty() {
            x += gap;
        }
        let mr = Rect::from_min_size(pos2(x, area.min.y), vec2(w - 1.0, area.height()));
        p.rect_filled(mr, 1.0, look::lcd_bg());
        let v = layout.iter().position(|x| x == c).and_then(|i| levels.get(i)).copied().unwrap_or(0.0);
        let lv = v.clamp(0.0, 1.0).sqrt();
        if lv > 0.01 {
            p.rect_filled(Rect::from_min_max(pos2(mr.min.x, mr.max.y - lv * mr.height()), mr.max), 1.0, if v > 0.98 { look::record_red() } else { GREEN });
        }
        let _ = ui.interact(mr, ui.scope_id().with(("surmeter", k)), Sense::hover()).on_hover_text(format!("{}: {:.1} dB", c.name(), if v > 1e-5 { 20.0 * v.log10() } else { -90.0 }));
        x += w;
    }
}

/// The surround master's Bounce button (bottom right).
pub fn bounce_button(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let b = Rect::from_min_size(pos2(r.max.x - 26.0, r.max.y - 50.0), vec2(20.0, 18.0));
    let p = ui.painter();
    p.rect_filled(b, 4.0, look::group_bg());
    crate::glyphs::paint(p, b, crate::glyphs::ph::EXPORT, 12.0, look::text());
    if ui.interact(b, ui.scope_id().with("sur-bounce"), Sense::click()).on_hover_text("Bounce").clicked() {
        open_bounce(app);
    }
}

/// The surround master's plug-in menu: the surround plug-ins in the project's format, the rest
/// Multi Mono. (The Dolby Atmos plug-in isn't offered: Spatial Audio inserts it.)
pub fn master_plugin_menu(app: &App, ui: &mut egui::Ui, current: Option<&str>) -> Option<Option<String>> {
    let mut picked = None;
    ui.set_min_width(220.0);
    if current.is_some() && ui.button("No Plug-in").clicked() {
        picked = Some(None);
        ui.close();
    }
    let fmt = app.sur.format.short();
    let native = |ui: &mut egui::Ui, cat: &str, names: &[&str], picked: &mut Option<Option<String>>| {
        ui.menu_button(cat, |ui| {
            for n in names {
                let label = if current == Some(*n) { format!("✓ {n}") } else { n.to_string() };
                // (Down Mixer: not for formats with overhead channels)
                let ok = !(*n == "Down Mixer" && app.sur.format.has_height() && !app.sur.atmos);
                ui.add_enabled_ui(ok, |ui| {
                    ui.menu_button(label, |ui| {
                        if ui.button(fmt).clicked() {
                            *picked = Some(Some(n.to_string()));
                            ui.close();
                        }
                    });
                });
            }
        });
    };
    native(ui, "Dynamics", &["Surround Compressor"], &mut picked);
    native(ui, "Imaging", &["Spatial Audio Monitoring"], &mut picked);
    native(ui, "Reverb", &["Quantec Room Simulator"], &mut picked);
    native(ui, "Metering", &["Level Meter", "Loudness Meter", "MultiMeter"], &mut picked);
    native(ui, "Utility", &["Multichannel Gain", "Down Mixer"], &mut picked);
    ui.separator();
    ui.label(egui::RichText::new("Multi Mono").color(look::text_dim()));
    let have: BTreeSet<&str> = crate::units::effect_names().into_iter().collect();
    fn walk(ui: &mut egui::Ui, items: &[crate::menus::Item], have: &BTreeSet<&str>, current: Option<&str>, picked: &mut Option<Option<String>>) {
        for it in items {
            if it.separator {
                continue;
            }
            let is_plugin = it.children.iter().any(|c| c.label == "Stereo" || c.label == "Dual Mono" || c.label == "Mono");
            if is_plugin {
                let ok = have.contains(it.label.as_str()) && !lg_fx::surround::NATIVE.contains(&it.label.as_str());
                let label = if current == Some(it.label.as_str()) { format!("✓ {}", it.label) } else { it.label.clone() };
                ui.add_enabled_ui(ok, |ui| {
                    ui.menu_button(label, |ui| {
                        if ui.button("Multi Mono").clicked() {
                            *picked = Some(Some(it.label.clone()));
                            ui.close();
                        }
                    });
                });
            } else if !it.children.is_empty() && it.label != "Audio Units" {
                ui.menu_button(&it.label, |ui| walk(ui, &it.children, have, current, picked));
            }
        }
    }
    walk(ui, crate::menus::context("mixer-audiofx-slot"), &have, current, &mut picked);
    picked
}

// ---- Project Settings ▸ Audio ▸ General ---------------------------------------------------------------

pub fn project_settings(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    let mut atmos = app.sur.atmos;
    let mut fmt = app.sur.format;
    egui::Grid::new("sur-proj").num_columns(2).show(ui, |ui| {
        ui.label("Spatial Audio:");
        egui::ComboBox::from_id_salt("sur-spatial").selected_text(if atmos { "Dolby Atmos" } else { "Off" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut atmos, false, "Off");
            ui.selectable_value(&mut atmos, true, "Dolby Atmos");
        });
        ui.end_row();
        ui.label("Surround Format:");
        egui::ComboBox::from_id_salt("sur-format").selected_text(fmt.name()).show_ui(ui, |ui| {
            for f in Format::ALL {
                let ok = !atmos || Format::ATMOS_BEDS.contains(&f);
                ui.add_enabled_ui(ok, |ui| {
                    ui.selectable_value(&mut fmt, f, f.name());
                });
            }
        });
        ui.end_row();
    });
    ui.label(egui::RichText::new("A project is surround once a channel strip's output is Surround; Dolby Atmos adds the Dolby Atmos plug-in to the surround master, a 7.1.2 bed and 24 fps.").small().color(look::text_dim()));
    if atmos != app.sur.atmos {
        s::set_atmos(app, atmos);
    } else if fmt != app.sur.format {
        s::set_format(app, fmt);
    }
}

// ---- Settings ▸ Audio ▸ I/O Assignments --------------------------------------------------------------

const IO_PANE: &str = "Audio / I/O Assignments / Output | Bounce Extensions | Input";
const IO_SHOW: &str = "Audio / I/O Assignments / Show as";

/// Rows Settings leaves to us: all of I/O Assignments' but the Output / Bounce Extensions /
/// Input switch.
pub fn skip_setting_row(tab: &str, sub: &str, kind: &str) -> bool {
    tab == "Audio" && sub == "I/O Assignments" && kind != "radio"
}

/// The speakers' interface outputs (or inputs) for a format and an Initialize layout:
/// Default (5.1: L R Ls Rs C LFE), ITU (L R C LFE Ls Rs), WG-4 (DVD-Audio: L R C LFE Ls Rs,
/// the wider formats' extra channels in WG-4's order).
pub fn initialize(f: Format, how: &str) -> Vec<(Ch, usize)> {
    use Ch::*;
    let order: &[Ch] = match how {
        "ITU" => &[L, R, C, Lfe, Ls, Rs, Lm, Rm, Lt, Rt, Lts, Rts, Ltm, Rtm, Lc, Rc, S],
        "WG-4" => &[L, R, C, Lfe, Ls, Rs, S, Lm, Rm, Lc, Rc, Lt, Rt, Lts, Rts, Ltm, Rtm],
        _ => &[L, R, Ls, Rs, C, Lfe, Lm, Rm, Lt, Rt, Lts, Rts, Ltm, Rtm, Lc, Rc, S],
    };
    order.iter().filter(|c| f.index(**c).is_some()).enumerate().map(|(k, c)| (*c, k + 1)).collect()
}

pub fn ext_key(c: Ch) -> String {
    format!("Audio / I/O Assignments / Ext {}", c.label())
}

/// A channel's split-bounce extension (Bounce Extensions; also read when importing split files).
pub fn extension(app: &App, c: Ch) -> String {
    let v = app.logic_settings.get(&ext_key(c));
    if v.is_empty() { c.ext() } else { v }
}

pub fn io_assignments(app: &mut App, ui: &mut egui::Ui, left: f32, label_right: f32, y: f32) {
    let pane = app.logic_settings.get(IO_PANE);
    let pane = if pane.is_empty() { "Output".to_string() } else { pane };
    let area = Rect::from_min_size(pos2(left, y), vec2(660.0, 600.0));
    let _ = label_right;
    let mut changed = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        let vals = &mut app.logic_settings;
        let show = Format::from_name(&vals.get(IO_SHOW)).unwrap_or_default();
        match pane.as_str() {
            "Bounce Extensions" => {
                ui.label("Channel identifiers added to split bounce files' names (and read when importing split files):");
                egui::Grid::new("io-ext").num_columns(4).show(ui, |ui| {
                    let all: Vec<Ch> = Ch::ALL.to_vec();
                    for (k, c) in all.iter().enumerate() {
                        ui.label(c.name());
                        let key = ext_key(*c);
                        let mut t = vals.0.get(&key).cloned().unwrap_or_else(|| c.ext());
                        if ui.add(egui::TextEdit::singleline(&mut t).desired_width(60.0)).changed() {
                            vals.0.insert(key, t);
                            changed = true;
                        }
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
                if ui.button("Reset Extensions").clicked() {
                    vals.0.retain(|k, _| !k.starts_with("Audio / I/O Assignments / Ext "));
                    changed = true;
                }
            }
            which => {
                let dir = if which == "Input" { "In" } else { "Out" };
                let mut show_f = show;
                ui.horizontal(|ui| {
                    ui.label("Show as:");
                    egui::ComboBox::from_id_salt("io-show").selected_text(show_f.name()).show_ui(ui, |ui| {
                        for f in Format::ALL {
                            ui.selectable_value(&mut show_f, f, f.name());
                        }
                    });
                });
                if show_f != show {
                    vals.0.insert(IO_SHOW.into(), show_f.name().into());
                    changed = true;
                }
                let mut init: Option<&str> = None;
                ui.horizontal(|ui| {
                    ui.label("Initialize:");
                    for b in ["Default", "ITU", "WG-4"] {
                        if ui.button(b).clicked() {
                            init = Some(b);
                        }
                    }
                });
                if let Some(how) = init {
                    for (c, n) in initialize(show_f, how) {
                        for d in ["In", "Out"] {
                            vals.0.insert(format!("Audio / I/O Assignments / {d} {}", c.label()), format!("{} {n}", if d == "In" { "Input" } else { "Output" }));
                        }
                    }
                    changed = true;
                }
                let defaults = initialize(show_f, "Default");
                egui::Grid::new(("io-ch", dir)).num_columns(2).show(ui, |ui| {
                    for c in show_f.channels() {
                        ui.label(c.name());
                        let key = format!("Audio / I/O Assignments / {dir} {}", c.label());
                        let word = if dir == "In" { "Input" } else { "Output" };
                        let def = defaults.iter().find(|x| x.0 == *c).map_or(1, |x| x.1);
                        let cur = vals.0.get(&key).cloned().unwrap_or_else(|| format!("{word} {def}"));
                        let mut pick = cur.clone();
                        egui::ComboBox::from_id_salt(("io", dir, c.label())).selected_text(&cur).show_ui(ui, |ui| {
                            for n in 1..=16 {
                                let t = format!("{word} {n}");
                                ui.selectable_value(&mut pick, t.clone(), t);
                            }
                        });
                        if pick != cur {
                            vals.0.insert(key, pick);
                            changed = true;
                        }
                        ui.end_row();
                    }
                });
                if dir == "Out" {
                    ui.separator();
                    ui.label(egui::RichText::new("Bramble").strong());
                    let cur = vals.get(s::MONITOR_KEY);
                    let mut bin = cur == "Binaural";
                    ui.horizontal(|ui| {
                        ui.label("Two-channel device plays a surround mix:");
                        ui.selectable_value(&mut bin, false, "Folded down (Lo/Ro)");
                        ui.selectable_value(&mut bin, true, "Binaural (headphones)");
                    });
                    if bin != (cur == "Binaural") {
                        vals.0.insert(s::MONITOR_KEY.into(), if bin { "Binaural".into() } else { "Downmix".into() });
                        changed = true;
                    }
                    ui.label(egui::RichText::new("Studio's sound card output is two channels: the speaker assignments are kept for when the mix goes to a multichannel interface.").small().color(look::text_dim()));
                }
            }
        }
    });
    if changed {
        app.logic_settings.save();
        app.load_engine();
    }
}

// ---- windows ------------------------------------------------------------------------------------------

pub fn windows(app: &mut App, ctx: &egui::Context) {
    // (a click on the Dolby Atmos plug-in's slot opens its own window)
    if let Some((st, k)) = app.mix.fx_window {
        if st == crate::mixstate::MASTER && s::active(app) && app.sur.master.get(k).is_some_and(|u| u.name == ATMOS) {
            app.mix.fx_window = None;
            app.sur_ui.atmos_open = true;
        }
    }
    panner_windows(app, ctx);
    if app.sur_ui.atmos_open {
        atmos_window(app, ctx);
    }
    if app.sur_ui.bounce.is_some() {
        bounce_window(app, ctx);
    }
    if app.sur_ui.export.is_some() {
        export_window(app, ctx);
    }
    if let Some(j) = app.sur_ui.job.clone() {
        let done = j.lock().ok().and_then(|mut x| x.take());
        if let Some(msg) = done {
            app.sur_ui.job = None;
            app.tell(msg);
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
    if let Some(msg) = app.sur_ui.notice.clone() {
        let mut open = true;
        egui::Window::new("Bounce").id(egui::Id::unique("sur-notice")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label(&msg);
            if ui.button("OK").clicked() {
                app.sur_ui.notice = None;
            }
        });
        if !open {
            app.sur_ui.notice = None;
        }
    }
}

fn panner_windows(app: &mut App, ctx: &egui::Context) {
    let wins = app.sur_ui.panners.clone();
    let mut close = Vec::new();
    for w in wins {
        let strip = if w.link { app.selected_track.unwrap_or(w.strip) } else { w.strip };
        let object = s::output_of_is_object(app, strip);
        let kind = if object { "3D Object Panner" } else if s::surround_input(app, strip) { "Surround Balancer" } else { "Surround Panner" };
        let mut open = true;
        let title = format!("{}: {kind}", crate::mixstate::strip_name(app, strip));
        egui::Window::new(title).id(egui::Id::unique(("sur-panner", w.id))).open(&mut open).resizable(false).default_width(300.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut link = w.link;
                if ui.add(egui::Button::selectable(link, crate::glyphs::ph::LINK)).on_hover_text("Link: follow the selected channel strip").clicked() {
                    link = !link;
                    if let Some(x) = app.sur_ui.panners.iter_mut().find(|x| x.id == w.id) {
                        x.link = link;
                        x.strip = strip;
                    }
                }
                panner_settings_menu(app, ui, strip, object);
                if !object && app.sur.format.has_height() {
                    let mut pan = app.sur.pan(strip);
                    let mut sph = pan.spherical;
                    ui.selectable_value(&mut sph, false, "Planar");
                    ui.selectable_value(&mut sph, true, "Spherical");
                    if sph != pan.spherical {
                        pan.spherical = sph;
                        app.sur.pans.insert(strip, pan);
                    }
                }
            });
            ui.separator();
            if object {
                object_panner(app, ui, strip);
            } else {
                surround_panner(app, ui, strip);
            }
        });
        if !open {
            close.push(w.id);
        }
    }
    app.sur_ui.panners.retain(|w| !close.contains(&w.id));
}

fn presets_dir(object: bool) -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Music/Audio Music Apps/Plug-In Settings").join(if object { "3D Object Panner" } else { "Surround Panner" })
}

/// The panner window's Settings pop-up: Save As Default, Reset, presets in Logic's folder.
fn panner_settings_menu(app: &mut App, ui: &mut egui::Ui, strip: usize, object: bool) {
    ui.menu_button("Settings ⌄", |ui| {
        if ui.button("Save As Default").clicked() {
            if object {
                app.sur.default_object = Some(app.sur.object(strip));
            } else {
                app.sur.default_pan = Some(app.sur.pan(strip));
            }
            ui.close();
        }
        if ui.button("Reset").clicked() {
            crate::mixstate::checkpoint(app, "Reset Panner");
            if object {
                app.sur.objects.remove(&strip);
            } else {
                app.sur.pans.remove(&strip);
            }
            push(app, strip);
            ui.close();
        }
        ui.separator();
        let dir = presets_dir(object);
        if ui.button("Save Setting As…").clicked() {
            let _ = std::fs::create_dir_all(&dir);
            if let Some(f) = (!app.automated).then(|| kn_ui::files::FileDialog::new().set_directory(&dir).set_file_name("Pan.txt").save_file()).flatten() {
                let text = if object { let o = app.sur.object(strip); format!("object {},{},{},{},{}", o.x, o.y, o.z, o.size, o.spread) } else { let p = app.sur.pan(strip); format!("pan {},{},{},{},{},{},{}", p.angle, p.diversity, p.elevation, p.spread, p.center_db, p.lfe_db, p.amount) };
                let _ = std::fs::write(f, text);
            }
            ui.close();
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            let name = f.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            if ui.button(&name).clicked() {
                if let Ok(t) = std::fs::read_to_string(&f) {
                    crate::mixstate::checkpoint(app, "Load Panner Setting");
                    let v: Vec<f32> = t.split_whitespace().nth(1).unwrap_or("").split(',').filter_map(|x| x.parse().ok()).collect();
                    if object && v.len() >= 5 {
                        app.sur.objects.insert(strip, Object { x: v[0], y: v[1], z: v[2], size: v[3], spread: v[4] });
                    } else if !object && v.len() >= 7 {
                        let mut p = app.sur.pan(strip);
                        (p.angle, p.diversity, p.elevation, p.spread, p.center_db, p.lfe_db, p.amount) = (v[0], v[1], v[2], v[3], v[4], v[5], v[6]);
                        app.sur.pans.insert(strip, p);
                    }
                    push(app, strip);
                }
                ui.close();
            }
        }
    });
}

fn db_field(ui: &mut egui::Ui, label: &str, v: &mut f32) -> egui::Response {
    ui.label(label);
    let mut x = v.max(sur::LEVEL_OFF);
    let r = ui.add(egui::DragValue::new(&mut x).range(sur::LEVEL_OFF..=6.0).speed(if ui.input(|i| i.modifiers.shift) { 0.02 } else { 0.2 }).custom_formatter(|n, _| if n <= sur::LEVEL_OFF as f64 { "-∞".into() } else { format!("{n:.1} dB") }));
    if r.changed() {
        *v = if x <= sur::LEVEL_OFF { f32::NEG_INFINITY } else { x };
    }
    r
}

fn surround_panner(app: &mut App, ui: &mut egui::Ui, strip: usize) {
    let fmt = app.sur.format;
    let balancer = s::surround_input(app, strip);
    let stereo = s::strip_stereo(app, strip) && !balancer;
    let mut pan = app.sur.pan(strip);
    let before = pan;
    let size = 240.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    let p = ui.painter_at(rect.expand(4.0));
    let c = rect.center();
    let rad = size / 2.0 - 22.0;
    // the field: four circles, eight 45° segments, listener in the middle facing forward
    p.circle_filled(c, rad, Color32::from_gray(0x24));
    for k in 1..=4 {
        p.circle_stroke(c, rad * k as f32 / 4.0, Stroke::new(0.6, Color32::from_gray(0x48)));
    }
    for k in 0..8 {
        let a = (k as f32 * 45.0).to_radians();
        p.line_segment([c, c + vec2(a.sin(), -a.cos()) * rad], Stroke::new(0.5, Color32::from_gray(0x40)));
    }
    p.circle_filled(c, 4.0, Color32::from_gray(0x80));
    // the outer ring: which speakers carry each side (blue left, green right)
    let ring = |p: &egui::Painter, gains: &[f32], col: Color32, r: f32| {
        for (k, ch) in fmt.channels().iter().enumerate() {
            if *ch == Ch::Lfe || ch.height() || gains[k] < 0.05 {
                continue;
            }
            let (az, _) = sur::position(fmt, *ch);
            let w = 10.0 + gains[k] * 30.0;
            let pts: Vec<Pos2> = (0..=8).map(|j| {
                let a = (az - w / 2.0 + w * j as f32 / 8.0).to_radians();
                c + vec2(a.sin(), -a.cos()) * r
            }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(3.0, col.gamma_multiply(0.4 + 0.6 * gains[k]))));
        }
    };
    if balancer {
        ring(&p, &sur::balancer_gains(fmt, &pan), GREEN, rad + 6.0);
    } else if stereo {
        let (l, r) = sur::stereo_gains(fmt, &pan);
        ring(&p, &l, BLUE, rad + 5.0);
        ring(&p, &r, GREEN, rad + 9.0);
    } else {
        ring(&p, &sur::mono_gains(fmt, &pan), GREEN, rad + 6.0);
    }
    // speakers: blue ear level, green overhead; solid on, outline off (no icon for the LFE)
    let mut speaker_hit: Option<Ch> = None;
    for ch in fmt.channels().iter().filter(|x| **x != Ch::Lfe) {
        let (az, el) = sur::position(fmt, *ch);
        let a = az.to_radians();
        let rr = if ch.height() { rad * (1.0 - el / 90.0).max(0.35) } else { rad + 15.0 };
        let sp = c + vec2(a.sin(), -a.cos()) * rr;
        let col = if ch.height() { GREEN } else { BLUE };
        let sr = Rect::from_center_size(sp, vec2(12.0, 12.0));
        if pan.is_off(*ch) {
            p.rect_stroke(sr, 2.0, Stroke::new(1.2, col), egui::StrokeKind::Inside);
        } else {
            p.rect_filled(sr, 2.0, col);
        }
        p.text(sp + vec2(0.0, 11.0), Align2::CENTER_CENTER, ch.label(), look::regular(8.5), look::text_dim());
        if resp.clicked() && resp.interact_pointer_pos().is_some_and(|pp| sr.expand(2.0).contains(pp)) {
            speaker_hit = Some(*ch);
        }
    }
    if let Some(ch) = speaker_hit {
        crate::mixstate::checkpoint(app, "Speaker On/Off");
        pan.toggle(ch);
    }
    // separation frame (orange stereo, green mono)
    let sep_ok = !balancer && !fmt.has_height();
    if sep_ok && pan.separation {
        let col = if stereo { ORANGE } else { GREEN };
        let fy = rad * (0.3 + 0.7 * pan.sep_y);
        let tl = c + vec2(-rad * 0.3 - rad * 0.6 * pan.sep_xf, -fy);
        let tr = c + vec2(rad * 0.3 + rad * 0.6 * pan.sep_xf, -fy);
        let bl = c + vec2(-rad * 0.3 - rad * 0.6 * pan.sep_xr, fy);
        let br = c + vec2(rad * 0.3 + rad * 0.6 * pan.sep_xr, fy);
        p.add(egui::Shape::closed_line(vec![tl, tr, br, bl], Stroke::new(1.5, col)));
        // corners and edges drag: top corners XF, bottom corners XR, top / bottom edges Y
        for (k, pt) in [tr, br, pos2(c.x, tl.y)].into_iter().enumerate() {
            let hr = Rect::from_center_size(pt, vec2(10.0, 10.0));
            p.circle_filled(pt, 3.0, col);
            let h = ui.interact(hr, ui.scope_id().with(("sep", strip, k)), Sense::drag());
            if h.drag_started() {
                crate::mixstate::checkpoint(app, "Separation");
            }
            if h.dragged() {
                if let Some(pp) = h.interact_pointer_pos() {
                    match k {
                        0 => pan.sep_xf = (((pp.x - c.x).abs() / rad - 0.3) / 0.6).clamp(0.0, 1.0),
                        1 => pan.sep_xr = (((pp.x - c.x).abs() / rad - 0.3) / 0.6).clamp(0.0, 1.0),
                        _ => pan.sep_y = (((c.y - pp.y).abs() / rad - 0.3) / 0.7).clamp(0.0, 1.0),
                    }
                }
            }
        }
    }
    // pucks: one (mono, balancer), or L, R and the dot that moves both (stereo)
    let mods = ui.input(|x| x.modifiers);
    let (px, py) = pan.puck();
    let main = field_xy(c, rad, px, py);
    if stereo {
        for (side, col, t) in [(-1.0f32, BLUE, "L"), (1.0, GREEN, "R")] {
            let a = (pan.angle + side * pan.spread / 2.0).to_radians();
            let rr = 1.0 - pan.diversity;
            let pt = field_xy(c, rad, a.sin() * rr, a.cos() * rr);
            p.circle_filled(pt, 7.0, col);
            p.text(pt, Align2::CENTER_CENTER, t, look::bold(9.0), Color32::BLACK);
            let h = ui.interact(Rect::from_center_size(pt, vec2(16.0, 16.0)), ui.scope_id().with(("lr", strip, t)), Sense::click_and_drag());
            if h.drag_started() {
                crate::mixstate::checkpoint(app, "Spread");
            }
            if h.dragged() {
                if let Some(pp) = h.interact_pointer_pos() {
                    // dragging one side moves the other symmetrically (the spread)
                    let ang = (pp.x - c.x).atan2(c.y - pp.y).to_degrees();
                    pan.spread = (2.0 * sur::angle_diff(ang, pan.angle) * side).clamp(-180.0, 180.0);
                }
            }
        }
        p.circle_filled(main, 4.0, Color32::WHITE);
    } else {
        p.circle_filled(main, 7.0, GREEN);
    }
    if resp.drag_started() {
        crate::mixstate::checkpoint(app, "Surround Panner");
    }
    if resp.dragged() && !ui.ctx().is_being_dragged(ui.scope_id().with(("lr", strip, "L"))) && !ui.ctx().is_being_dragged(ui.scope_id().with(("lr", strip, "R"))) {
        if let Some(pp) = resp.interact_pointer_pos() {
            drag_pan(&mut pan, (pp.x - c.x) / rad, -(pp.y - c.y) / rad, lock_of(mods), fmt, balancer);
        }
    }
    if resp.clicked() && mods.alt {
        crate::mixstate::checkpoint(app, "Surround Panner");
        pan.angle = 0.0;
        pan.diversity = 0.0;
        pan.amount = 0.0;
    }
    // the values
    egui::Grid::new(("sur-vals", strip)).num_columns(4).show(ui, |ui| {
        ui.label("Angle");
        ui.add(egui::DragValue::new(&mut pan.angle).range(-180.0..=180.0).speed(0.5).suffix("°"));
        if balancer {
            ui.label("Amount");
            ui.add(egui::DragValue::new(&mut pan.amount).range(0.0..=1.0).speed(0.005));
        } else {
            ui.label("Diversity");
            ui.add(egui::DragValue::new(&mut pan.diversity).range(0.0..=1.0).speed(0.005));
        }
        ui.end_row();
        if fmt.has_height() {
            ui.label("Elevation");
            ui.add(egui::DragValue::new(&mut pan.elevation).range(0.0..=90.0).speed(0.5).suffix("°"));
        }
        if stereo {
            ui.label("Spread");
            ui.add(egui::DragValue::new(&mut pan.spread).range(-180.0..=180.0).speed(0.5).suffix("°"));
        }
        ui.end_row();
        let center_on = fmt.index(Ch::C).is_some() && !pan.is_off(Ch::C);
        ui.add_enabled_ui(center_on, |ui| {
            db_field(ui, "Center", &mut pan.center_db);
        });
        if fmt.has_lfe() {
            db_field(ui, "LFE", &mut pan.lfe_db);
        }
        ui.end_row();
    });
    if sep_ok {
        ui.horizontal(|ui| {
            ui.checkbox(&mut pan.separation, "Separation");
            ui.add_enabled_ui(pan.separation, |ui| {
                for (label, v) in [("XF", &mut pan.sep_xf), ("XR", &mut pan.sep_xr), ("Y", &mut pan.sep_y)] {
                    ui.label(label);
                    let r = ui.add(egui::DragValue::new(v).range(0.0..=1.0).speed(0.005));
                    // (Option-click: back to the default)
                    if r.clicked() && ui.input(|i| i.modifiers.alt) {
                        *v = 1.0;
                    }
                }
            });
        });
    }
    if pan != before {
        if !resp.dragged() && speaker_hit.is_none() {
            crate::mixstate::checkpoint(app, "Surround Panner");
        }
        app.sur.pans.insert(strip, pan);
        push(app, strip);
        s::record(app, strip, &[s::P_ANGLE, s::P_DIVERSITY, s::P_ELEVATION, s::P_SPREAD, s::P_LFE, s::P_CENTER, s::P_AMOUNT]);
    }
}

fn object_panner(app: &mut App, ui: &mut egui::Ui, strip: usize) {
    let stereo = s::strip_stereo(app, strip);
    let mut o = app.sur.object(strip);
    let before = o;
    let mods = ui.input(|x| x.modifiers);
    // two grids: left-right × front-back (top) and left-right × down-up (bottom)
    for grid in 0..2 {
        let (rect, resp) = ui.allocate_exact_size(vec2(240.0, if grid == 0 { 200.0 } else { 100.0 }), Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 4.0, Color32::from_gray(0x24));
        for k in 1..4 {
            let x = rect.min.x + rect.width() * k as f32 / 4.0;
            p.vline(x, rect.y_range(), Stroke::new(0.5, Color32::from_gray(0x40)));
            let y = rect.min.y + rect.height() * k as f32 / 4.0;
            p.hline(rect.x_range(), y, Stroke::new(0.5, Color32::from_gray(0x40)));
        }
        p.text(rect.left_top() + vec2(4.0, 4.0), Align2::LEFT_TOP, if grid == 0 { "Front" } else { "Top" }, look::regular(9.0), look::text_dim());
        let to_px = |x: f32, v: f32| -> Pos2 {
            let vy = if grid == 0 { (1.0 - v) / 2.0 } else { 1.0 - v };
            pos2(rect.min.x + (x + 1.0) / 2.0 * rect.width(), rect.min.y + vy * rect.height())
        };
        let v_of = |o: &Object| if grid == 0 { o.y } else { o.z };
        let centre = to_px(o.x, v_of(&o));
        if o.size > 0.01 {
            p.circle_filled(centre, 6.0 + o.size * 30.0, GREEN.gamma_multiply(0.2));
        }
        let mut dragging_side = None;
        if stereo {
            let (l, r) = o.stereo();
            for (side, ob, col, t) in [(-1.0f32, l, BLUE, "L"), (1.0, r, GREEN, "R")] {
                let pt = to_px(ob.x, v_of(&ob));
                p.circle_filled(pt, 7.0, col);
                p.text(pt, Align2::CENTER_CENTER, t, look::bold(9.0), Color32::BLACK);
                let h = ui.interact(Rect::from_center_size(pt, vec2(16.0, 16.0)), ui.scope_id().with(("olr", strip, grid, t)), Sense::drag());
                if h.drag_started() {
                    crate::mixstate::checkpoint(app, "Spread");
                }
                if h.dragged() {
                    dragging_side = Some(side);
                    if let Some(pp) = h.interact_pointer_pos() {
                        let x = ((pp.x - rect.min.x) / rect.width() * 2.0 - 1.0).clamp(-1.0, 1.0);
                        // (dragging across the centre swaps left and right: the spread turns negative)
                        o.spread = ((x - o.x) * side).clamp(-1.0, 1.0);
                    }
                }
            }
            p.circle_filled(centre, 4.0, Color32::WHITE);
        } else {
            p.circle_filled(centre, 7.0, GREEN);
        }
        if resp.drag_started() {
            crate::mixstate::checkpoint(app, "3D Object Panner");
        }
        if resp.dragged() && dragging_side.is_none() {
            if let Some(pp) = resp.interact_pointer_pos() {
                o.x = ((pp.x - rect.min.x) / rect.width() * 2.0 - 1.0).clamp(-1.0, 1.0);
                let f = (pp.y - rect.min.y) / rect.height();
                if grid == 0 {
                    o.y = (1.0 - 2.0 * f).clamp(-1.0, 1.0);
                } else {
                    o.z = (1.0 - f).clamp(0.0, 1.0);
                }
            }
        }
        if resp.clicked() && mods.alt {
            o.x = 0.0;
            o.y = 1.0;
        }
    }
    egui::Grid::new(("obj-vals", strip)).num_columns(4).show(ui, |ui| {
        ui.label("Left/Right");
        ui.add(egui::DragValue::new(&mut o.x).range(-1.0..=1.0).speed(0.005));
        ui.label("Back/Front");
        ui.add(egui::DragValue::new(&mut o.y).range(-1.0..=1.0).speed(0.005));
        ui.end_row();
        ui.label("Elevation");
        ui.add(egui::DragValue::new(&mut o.z).range(0.0..=1.0).speed(0.005));
        ui.label("Size");
        ui.add(egui::DragValue::new(&mut o.size).range(0.0..=1.0).speed(0.005));
        ui.end_row();
        if stereo {
            ui.label("Spread");
            ui.add(egui::DragValue::new(&mut o.spread).range(-1.0..=1.0).speed(0.005));
            ui.end_row();
        }
    });
    ui.label(egui::RichText::new("Objects have no LFE send.").small().color(look::text_dim()));
    if o != before {
        app.sur.objects.insert(strip, o);
        push(app, strip);
        s::record(app, strip, &[s::P_X, s::P_Y, s::P_Z, s::P_SIZE, s::P_OSPREAD]);
    }
}

// ---- the Dolby Atmos plug-in window -----------------------------------------------------------------

const VIEWS: [(&str, f32, f32); 6] = [("Front", 0.0, 10.0), ("Back", 180.0, 10.0), ("Left", -90.0, 10.0), ("Right", 90.0, 10.0), ("Top", 0.0, 89.0), ("Perspective", -35.0, 30.0)];

fn atmos_window(app: &mut App, ctx: &egui::Context) {
    let mut open = true;
    egui::Window::new("Dolby Atmos").id(egui::Id::unique("sur-atmos")).open(&mut open).resizable(false).default_width(760.0).show(ctx, |ui| {
        // header: Settings, Monitoring Format, the renderer's buttons, List
        ui.horizontal(|ui| {
            ui.menu_button("Settings ⌄", |ui| atmos_settings_menu(app, ui));
            ui.label("Monitoring Format:");
            let mut m = app.sur.render.monitoring;
            egui::ComboBox::from_id_salt("atmos-mon").selected_text(m.name()).width(190.0).show_ui(ui, |ui| {
                ui.label(egui::RichText::new("Binaural").color(look::text_dim()));
                for x in [Monitoring::DolbyRenderer, Monitoring::AppleRenderer] {
                    ui.selectable_value(&mut m, x, x.name());
                }
                ui.separator();
                ui.label(egui::RichText::new("Speaker virtualization").color(look::text_dim()));
                for x in [Monitoring::BuiltInSpeakers, Monitoring::DisplaySpeakers] {
                    ui.selectable_value(&mut m, x, x.name());
                }
                ui.separator();
                for x in [Monitoring::Direct20, Monitoring::Downmix20, Monitoring::M51, Monitoring::M71, Monitoring::M512, Monitoring::M514, Monitoring::M714] {
                    ui.selectable_value(&mut m, x, x.name());
                }
            });
            if m != app.sur.render.monitoring {
                crate::mixstate::checkpoint(app, "Monitoring Format");
                app.sur.render.monitoring = m;
                app.load_engine();
            }
            match m {
                Monitoring::DolbyRenderer => {
                    ui.add_enabled(false, egui::Button::new("Head Tracking")).on_disabled_hover_text("Needs head-tracking headphones (AirPods Pro or Max)");
                }
                Monitoring::AppleRenderer => {
                    ui.add_enabled(false, egui::Button::new("Head Tracking")).on_disabled_hover_text("Needs head-tracking headphones on a Mac");
                    ui.add_enabled(false, egui::Button::new("Personalized Spatial Audio")).on_disabled_hover_text("Needs a profile made on an iPhone");
                    let mut movie = app.sur.render.movie_mode;
                    ui.selectable_value(&mut movie, false, "Music Mode");
                    ui.selectable_value(&mut movie, true, "Movie Mode");
                    if movie != app.sur.render.movie_mode {
                        app.sur.render.movie_mode = movie;
                        app.load_engine();
                    }
                }
                _ => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::selectable(app.sur_ui.atmos_list, crate::glyphs::ph::LIST)).on_hover_text("Show or hide the bed and object list").clicked() {
                    app.sur_ui.atmos_list = !app.sur_ui.atmos_list;
                }
            });
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            object_viewer(app, ui);
            if app.sur_ui.atmos_list {
                ui.separator();
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        for (p, t) in [(AtmosPane::List, "Surround Bed & 3D Objects"), (AtmosPane::Binaural, "Binaural Renderer"), (AtmosPane::Trim, "Downmix & Trim")] {
                            if ui.add(egui::Button::selectable(app.sur_ui.atmos_pane == p, t)).clicked() {
                                app.sur_ui.atmos_pane = p;
                            }
                        }
                    });
                    match app.sur_ui.atmos_pane {
                        AtmosPane::List => bed_and_objects(app, ui, false),
                        AtmosPane::Binaural => bed_and_objects(app, ui, true),
                        AtmosPane::Trim => downmix_trim(app, ui),
                    }
                });
            }
        });
    });
    if !open {
        app.sur_ui.atmos_open = false;
    }
}

fn atmos_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Music/Audio Music Apps/Plug-In Settings/Dolby Atmos")
}

/// Save / recall the plug-in's settings (monitoring, downmix, trims) as a setting file.
fn atmos_settings_menu(app: &mut App, ui: &mut egui::Ui) {
    let dir = atmos_dir();
    if ui.button("Save Setting As…").clicked() {
        let _ = std::fs::create_dir_all(&dir);
        if let Some(f) = (!app.automated).then(|| kn_ui::files::FileDialog::new().set_directory(&dir).set_file_name("Downmix & Trim.txt").save_file()).flatten() {
            let text: String = s::to_values(app).into_iter().filter(|(k, _)| k.starts_with("Surround / render") || k.starts_with("Surround / trim")).map(|(k, v)| format!("{k}={v}\n")).collect();
            let _ = std::fs::write(f, text);
        }
        ui.close();
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    files.sort();
    if !files.is_empty() {
        ui.separator();
    }
    for f in files {
        if ui.button(f.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default()).clicked() {
            if let Ok(t) = std::fs::read_to_string(&f) {
                let vals: std::collections::BTreeMap<String, String> = t.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
                let (st, _) = s::from_values(&vals);
                crate::mixstate::checkpoint(app, "Load Setting");
                let mon = if vals.contains_key("Surround / render / monitoring") { st.render.monitoring } else { app.sur.render.monitoring };
                app.sur.render = st.render;
                app.sur.render.monitoring = mon;
                app.load_engine();
            }
            ui.close();
        }
    }
}

/// The 3D Object Viewer: a room with a head at the listening position; objects as spheres
/// (blue while sounding, white when selected), a halo for their size; drag to rotate.
fn object_viewer(app: &mut App, ui: &mut egui::Ui) {
    ui.vertical(|ui| {
        let (rect, resp) = ui.allocate_exact_size(vec2(380.0, 300.0), Sense::click_and_drag());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 6.0, Color32::from_gray(0x1c));
        if resp.dragged() {
            let d = resp.drag_delta();
            app.sur_ui.view.0 += d.x * 0.5;
            app.sur_ui.view.1 = (app.sur_ui.view.1 + d.y * 0.5).clamp(-10.0, 89.0);
            app.sur_ui.view_preset = 6;
            app.sur_ui.custom_view = app.sur_ui.view;
        }
        let (yaw, pitch) = (app.sur_ui.view.0.to_radians(), app.sur_ui.view.1.to_radians());
        let c = rect.center() + vec2(0.0, 20.0);
        let scale = 95.0;
        // room coords (x right, y front, z up; z 0..1 drawn from −0.3) to the screen
        let proj = |x: f32, y: f32, z: f32| -> (Pos2, f32) {
            let (sx, cy) = (yaw.sin(), yaw.cos());
            let rx = x * cy - y * sx;
            let ry = x * sx + y * cy;
            let (sp, cp) = (pitch.sin(), pitch.cos());
            let depth = ry * cp + (z - 0.3) * sp;
            let up = (z - 0.3) * cp - ry * sp;
            let k = 3.2 / (3.2 + depth);
            (pos2(c.x + rx * scale * k, c.y - up * scale * k), k)
        };
        let edges = [((-1.0, -1.0, 0.0), (1.0, -1.0, 0.0)), ((1.0, -1.0, 0.0), (1.0, 1.0, 0.0)), ((1.0, 1.0, 0.0), (-1.0, 1.0, 0.0)), ((-1.0, 1.0, 0.0), (-1.0, -1.0, 0.0)), ((-1.0, -1.0, 1.0), (1.0, -1.0, 1.0)), ((1.0, -1.0, 1.0), (1.0, 1.0, 1.0)), ((1.0, 1.0, 1.0), (-1.0, 1.0, 1.0)), ((-1.0, 1.0, 1.0), (-1.0, -1.0, 1.0)), ((-1.0, -1.0, 0.0), (-1.0, -1.0, 1.0)), ((1.0, -1.0, 0.0), (1.0, -1.0, 1.0)), ((1.0, 1.0, 0.0), (1.0, 1.0, 1.0)), ((-1.0, 1.0, 0.0), (-1.0, 1.0, 1.0))];
        for (a, b) in edges {
            let (pa, _) = proj(a.0, a.1, a.2);
            let (pb, _) = proj(b.0, b.1, b.2);
            p.line_segment([pa, pb], Stroke::new(1.0, Color32::from_gray(0x55)));
        }
        // the listener's head, facing the front wall
        let (hp, hk) = proj(0.0, 0.0, 0.0);
        p.circle_filled(hp, 9.0 * hk, Color32::from_gray(0x8a));
        let (np, _) = proj(0.0, 0.12, 0.0);
        p.circle_filled(np, 3.0 * hk, Color32::from_gray(0xaa));
        let (fp, _) = proj(0.0, 1.0, 0.0);
        p.text(fp + vec2(0.0, 8.0), Align2::CENTER_TOP, "Front", look::regular(9.0), look::text_dim());
        // the objects
        let peaks = app.engine.lock().ok().and_then(|e| e.surround.as_ref().map(|s| s.object_peaks.clone())).unwrap_or_default();
        let n_tracks = app.project.as_ref().map_or(0, |p| p.tracks.len());
        let mut list: Vec<(f32, Pos2, f32, Color32, f32)> = Vec::new();
        for (sid, _, stereo) in s::object_list(app) {
            let node = if sid >= AUX { n_tracks + sid - AUX } else { sid };
            let o = app.sur.object(sid);
            let sel = app.sur_ui.selected_objects.contains(&sid);
            let sounding = peaks.get(&node).copied().unwrap_or(0.0) > 0.001;
            if !sounding && !sel {
                continue;
            }
            let col = if sel { Color32::WHITE } else { Color32::from_rgb(0x3d, 0x8b, 0xfd) };
            let pts = if stereo { let (a, b) = o.stereo(); vec![a, b] } else { vec![o] };
            for q in pts {
                let (pp, k) = proj(q.x, q.y, q.z);
                list.push((k, pp, q.size, col, k));
            }
        }
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, pp, size, col, k) in list {
            if size > 0.01 {
                p.circle_filled(pp, (6.0 + size * 26.0) * k, col.gamma_multiply(0.18));
            }
            p.circle_filled(pp, 6.0 * k, col);
        }
        ctx_repaint(ui);
        // Rotate buttons and Custom
        ui.horizontal(|ui| {
            for (k, (name, y, pch)) in VIEWS.iter().enumerate() {
                if ui.add(egui::Button::selectable(app.sur_ui.view_preset == k, *name)).clicked() {
                    app.sur_ui.view = (*y, *pch);
                    app.sur_ui.view_preset = k;
                }
            }
            if ui.add(egui::Button::selectable(app.sur_ui.view_preset == 6, "Custom")).clicked() {
                app.sur_ui.view = app.sur_ui.custom_view;
                app.sur_ui.view_preset = 6;
            }
        });
    });
}

fn ctx_repaint(ui: &egui::Ui) {
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
}

/// The Surround Bed and 3D Objects columns (or, `binaural`, their render-mode menus).
fn bed_and_objects(app: &mut App, ui: &mut egui::Ui, binaural: bool) {
    let dolby = app.sur.render.monitoring == Monitoring::DolbyRenderer;
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Surround Bed").strong());
            for c in app.sur.format.channels().to_vec() {
                ui.horizontal(|ui| {
                    ui.label(c.name());
                    if binaural {
                        let mut m = app.sur.bin_mode_bed(c);
                        ui.add_enabled_ui(dolby && c != Ch::Lfe, |ui| {
                            egui::ComboBox::from_id_salt(("binbed", c.label())).selected_text(m.name()).width(60.0).show_ui(ui, |ui| {
                                for x in BinMode::ALL {
                                    ui.selectable_value(&mut m, x, x.name());
                                }
                            });
                        });
                        if m != app.sur.bin_mode_bed(c) {
                            crate::mixstate::checkpoint(app, "Binaural Render Mode");
                            app.sur.bin_bed.insert(c, m);
                            app.load_engine();
                        }
                    }
                });
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("3D Objects").strong());
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for (sid, name, stereo) in s::object_list(app) {
                    ui.horizontal(|ui| {
                        let sel = app.sur_ui.selected_objects.contains(&sid);
                        let sym = if stereo { "◎" } else { "○" };
                        let text = egui::RichText::new(format!("{sym} {name}")).color(if sel { Color32::WHITE } else { look::text() });
                        if ui.add(egui::Button::selectable(sel, text)).clicked() {
                            if sel {
                                app.sur_ui.selected_objects.remove(&sid);
                            } else {
                                app.sur_ui.selected_objects.insert(sid);
                            }
                        }
                        if binaural {
                            let mut m = app.sur.bin_obj.get(&sid).copied().unwrap_or_default();
                            ui.add_enabled_ui(dolby, |ui| {
                                egui::ComboBox::from_id_salt(("binobj", sid)).selected_text(m.name()).width(60.0).show_ui(ui, |ui| {
                                    for x in BinMode::ALL {
                                        ui.selectable_value(&mut m, x, x.name());
                                    }
                                });
                            });
                            if m != app.sur.bin_obj.get(&sid).copied().unwrap_or_default() {
                                crate::mixstate::checkpoint(app, "Binaural Render Mode");
                                app.sur.bin_obj.insert(sid, m);
                                app.load_engine();
                            }
                        }
                    });
                }
            });
            ui.label(format!("Input Object Channels: {} / {}", s::objects_used(app), s::MAX_OBJECTS));
        });
    });
    if binaural && !dolby {
        ui.label(egui::RichText::new("Binaural render modes apply with the Dolby Renderer only.").small().color(look::text_dim()));
    }
}

fn downmix_trim(app: &mut App, ui: &mut egui::Ui) {
    let mut r = app.sur.render.clone();
    egui::Grid::new("atmos-dm").num_columns(2).show(ui, |ui| {
        ui.label("5.1 and 5.1.x:");
        egui::ComboBox::from_id_salt("dm51").selected_text(r.down51.name()).width(230.0).show_ui(ui, |ui| {
            for x in sur::Down51::ALL {
                ui.selectable_value(&mut r.down51, x, x.name());
            }
        });
        ui.end_row();
        ui.label("5.1 to 2.0:");
        egui::ComboBox::from_id_salt("dm20").selected_text(r.down20.name()).width(230.0).show_ui(ui, |ui| {
            for x in sur::Down20::ALL {
                ui.selectable_value(&mut r.down20, x, x.name());
            }
        });
        ui.end_row();
    });
    ui.separator();
    ui.horizontal(|ui| {
        for (k, name) in sur::TRIM_LAYOUTS.iter().enumerate() {
            if ui.add(egui::Button::selectable(app.sur_ui.trim_layout == k, *name)).clicked() {
                app.sur_ui.trim_layout = k;
            }
        }
    });
    let k = app.sur_ui.trim_layout;
    let mut t = r.trims[k];
    ui.horizontal(|ui| {
        ui.label("Mode:");
        egui::ComboBox::from_id_salt("trim-mode").selected_text(if t.manual { "Manual" } else { "Automatic" }).show_ui(ui, |ui| {
            ui.selectable_value(&mut t.manual, false, "Automatic");
            ui.selectable_value(&mut t.manual, true, "Manual");
        });
    });
    let shown = t.effective(k);
    let mut vals = (shown.surround_db, shown.height_db, shown.overhead * 100.0, shown.listener * 100.0);
    ui.add_enabled_ui(t.manual, |ui| {
        egui::Grid::new("trims").num_columns(2).show(ui, |ui| {
            let db_fmt = |n: f64, _: std::ops::RangeInclusive<usize>| if n <= -40.0 { "-∞ dB".to_string() } else { format!("{n:.1} dB") };
            ui.label("Surround trim");
            ui.add(egui::Slider::new(&mut vals.0, -40.0..=0.0).custom_formatter(db_fmt));
            ui.end_row();
            ui.label("Height trim");
            ui.add(egui::Slider::new(&mut vals.1, -40.0..=0.0).custom_formatter(db_fmt));
            ui.end_row();
            ui.label("Overhead");
            ui.add(egui::Slider::new(&mut vals.2, -100.0..=100.0).suffix(" %").text("back … front"));
            ui.end_row();
            ui.label("Listener plane");
            ui.add(egui::Slider::new(&mut vals.3, -100.0..=100.0).suffix(" %").text("back … front"));
            ui.end_row();
        });
    });
    if t.manual {
        t.surround_db = vals.0;
        t.height_db = vals.1;
        t.overhead = vals.2 / 100.0;
        t.listener = vals.3 / 100.0;
    } else if t.manual != r.trims[k].manual {
        // (switching to Manual starts from Automatic's values)
    }
    if t.manual && !r.trims[k].manual {
        let a = sur::Trim::automatic(k);
        t.surround_db = a.surround_db;
        t.height_db = a.height_db;
    }
    r.trims[k] = t;
    ui.label(egui::RichText::new("Trims apply before the downmix; they don't change binaural monitoring or the 7.1.4 render.").small().color(look::text_dim()));
    if r != app.sur.render {
        crate::mixstate::checkpoint(app, "Downmix & Trim");
        app.sur.render = r.clone();
        if let Ok(mut e) = app.engine.lock() {
            if let Some(s) = e.surround.as_mut() {
                s.render = r;
            }
        }
        app.dirty = true;
    }
}

// ---- Bounce, Export, Import ------------------------------------------------------------------------

pub fn open_bounce(app: &mut App) {
    let (start, end) = range(app, true);
    app.sur_ui.bounce = Some(BounceDlg { pcm: true, mp3: false, m4a: false, burn: false, file_format: 1, bits: 1, surround: true, split: false, music_library: false, add_to_project: false, start, end });
}

/// The bounce / export range: the cycle when on, else (selection) the selected regions, else
/// the whole project.
fn range(app: &App, cycle_first: bool) -> (u32, u32) {
    let Some(p) = &app.project else { return (0, 3840) };
    if cycle_first && app.cycle_on() {
        return app.cycle;
    }
    let sel: Vec<(u32, u32)> = app.selected_regions.iter().chain(app.selected_region.iter()).filter_map(|(t, r)| p.tracks.get(*t).and_then(|x| x.regions.get(*r))).map(|r| (r.start, r.start + r.length)).collect();
    if !cycle_first && !sel.is_empty() {
        return (sel.iter().map(|x| x.0).min().unwrap_or(0), sel.iter().map(|x| x.1).max().unwrap_or(3840));
    }
    (0, p.tracks.iter().flat_map(|t| t.regions.iter()).map(|r| r.start + r.length).max().unwrap_or(3840))
}

const FILE_FORMATS: [&str; 3] = ["AIFF", "WAVE", "CAF"];
const BITS: [(&str, u16); 3] = [("16-bit", 16), ("24-bit", 24), ("32-bit Float", 32)];

fn bounce_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.sur_ui.bounce.clone() else { return };
    let mut open = true;
    let mut go = false;
    let mut export = false;
    egui::Window::new("Bounce").id(egui::Id::unique("sur-bounce")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("Destination").strong());
                ui.checkbox(&mut d.pcm, "PCM");
                let was = (d.mp3, d.m4a);
                ui.checkbox(&mut d.mp3, "MP3");
                ui.checkbox(&mut d.m4a, "M4A: AAC");
                if d.surround && (d.mp3, d.m4a) != was && (d.mp3 || d.m4a) {
                    d.mp3 = false;
                    d.m4a = false;
                    app.sur_ui.notice = Some("A compressed surround or split stereo bounce is not possible.".into());
                }
                if ui.checkbox(&mut d.burn, "Burn to CD / DVD").changed() && d.burn {
                    d.burn = false;
                    app.sur_ui.notice = Some("Studio doesn't burn CDs or DVD-A discs: bounce to a file and burn it with your disc tool.".into());
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                egui::Grid::new("sb").num_columns(2).show(ui, |ui| {
                    ui.label("Start:");
                    ui.add(egui::DragValue::new(&mut d.start).speed(48.0).custom_formatter(|n, _| bar_text(n as u32)));
                    ui.end_row();
                    ui.label("End:");
                    ui.add(egui::DragValue::new(&mut d.end).speed(48.0).custom_formatter(|n, _| bar_text(n as u32)));
                    ui.end_row();
                    ui.label("File Format:");
                    egui::ComboBox::from_id_salt("sb-ff").selected_text(FILE_FORMATS[d.file_format]).show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.file_format, 0, "AIFF");
                        ui.selectable_value(&mut d.file_format, 1, "WAVE");
                        ui.add_enabled(false, egui::Button::new("CAF"));
                    });
                    ui.end_row();
                    ui.label("Resolution:");
                    egui::ComboBox::from_id_salt("sb-bits").selected_text(BITS[d.bits].0).show_ui(ui, |ui| {
                        for (k, (n, _)) in BITS.iter().enumerate() {
                            ui.selectable_value(&mut d.bits, k, *n);
                        }
                    });
                    ui.end_row();
                    ui.label("Sample Rate:");
                    ui.label("48 kHz");
                    ui.end_row();
                    ui.label("Format:");
                    ui.add_enabled_ui(d.surround, |ui| {
                        egui::ComboBox::from_id_salt("sb-split").selected_text(if d.split { "Split" } else { "Interleaved" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut d.split, false, "Interleaved");
                            ui.selectable_value(&mut d.split, true, "Split");
                        });
                    });
                    ui.end_row();
                    ui.label("Dithering:");
                    ui.label("None");
                    ui.end_row();
                });
                if ui.checkbox(&mut d.surround, "Surround Bounce").changed() && d.surround {
                    // (Apple Music doesn't take surround files)
                    d.music_library = false;
                    d.mp3 = false;
                    d.m4a = false;
                }
                ui.add_enabled(!d.surround, egui::Checkbox::new(&mut d.music_library, "Add to Apple Music Library"));
                ui.checkbox(&mut d.add_to_project, "Add to Project");
                let mon = app.sur.render.monitoring;
                let layout = if app.sur.atmos && app.sur.atmos_slot().is_some() { mon.layout() } else { app.sur.format.channels().to_vec() };
                ui.label(egui::RichText::new(format!("Channels: {}", layout.iter().map(|c| c.label()).collect::<Vec<_>>().join(" "))).small().color(look::text_dim()));
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            if app.sur.atmos && ui.button("Export as Spatial Audio…").clicked() {
                export = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("OK").clicked() {
                    go = true;
                }
                if ui.button("Cancel").clicked() {
                    app.sur_ui.bounce = None;
                }
            });
        });
    });
    if !open {
        app.sur_ui.bounce = None;
        return;
    }
    if app.sur_ui.bounce.is_none() {
        return;
    }
    app.sur_ui.bounce = Some(d.clone());
    if export {
        app.sur_ui.bounce = None;
        open_export(app);
        return;
    }
    if go {
        app.sur_ui.bounce = None;
        run_bounce(app, d);
    }
}

fn bar_text(t: u32) -> String {
    let bar = t / 3840 + 1;
    let beat = (t % 3840) / 960 + 1;
    format!("{bar} {beat} 1 1")
}

/// The offline engine: everything the live one plays (tracks, auxes, routing, the masters and
/// surround), from bar `from`.
pub fn offline(app: &App) -> Option<crate::audio::Engine> {
    let (tracks, auxes, out, byp, mi, mb, comp, pa, tl, tempo, map, master, sf) = {
        let e = app.engine.lock().ok()?;
        (e.tracks.clone(), e.auxes.clone(), e.track_out.clone(), e.track_bypass.clone(), e.master_inserts.clone(), e.master_bypass.clone(), e.compensate, e.param_auto.clone(), e.timeline.clone(), e.tempo, e.tempo_map.clone(), e.master, e.soundfont.clone())
    };
    let mut e = crate::audio::Engine::new();
    e.tempo = tempo;
    e.tempo_map = map;
    e.tracks = tracks;
    e.auxes = auxes;
    e.track_out = out;
    e.track_bypass = byp;
    e.master_inserts = mi;
    e.master_bypass = mb;
    e.compensate = comp;
    e.param_auto = pa;
    e.timeline = tl;
    e.master = master;
    e.load_soundfont(sf.as_deref());
    s::to_engine(app, &mut e);
    Some(e)
}

/// Renders `from`..`to` (plus a two-second tail) and returns what was captured.
fn render(mut e: crate::audio::Engine, from: u32, to: u32, adm: bool) -> s::Capture {
    if let Some(s) = e.surround.as_mut() {
        s.capture = Some(s::Capture { adm, ..Default::default() });
    }
    e.pos = from as f64;
    e.playing = true;
    let mut buf = vec![0f32; 2 * 256];
    let mut stereo: Vec<f32> = Vec::new();
    while e.pos < to as f64 {
        e.render(&mut buf);
        stereo.extend_from_slice(&buf);
    }
    e.playing = false;
    e.all_off();
    for _ in 0..(2 * crate::audio::RATE as usize / 256) {
        e.render(&mut buf);
        stereo.extend_from_slice(&buf);
    }
    let mut cap = e.surround.as_mut().and_then(|s| s.capture.take()).unwrap_or_default();
    if cap.out.is_empty() {
        cap.layout = vec![Ch::L, Ch::R];
        cap.out = vec![stereo.iter().step_by(2).copied().collect(), stereo.iter().skip(1).step_by(2).copied().collect()];
    }
    cap
}

fn run_bounce(app: &mut App, d: BounceDlg) {
    let Some(p) = &app.project else { return };
    let ext = if d.file_format == 0 { "aif" } else { "wav" };
    let name = format!("{}.{ext}", p.name);
    let Some(path) = (if app.automated { None } else { kn_ui::files::FileDialog::new().set_title("Bounce").set_file_name(&name).save_file() }) else { return };
    let Some(engine) = offline(app) else { return };
    let exts: Vec<(Ch, String)> = Ch::ALL.iter().map(|c| (*c, extension(app, *c))).collect();
    let bits = BITS[d.bits].1;
    let job = Arc::new(Mutex::new(None));
    app.sur_ui.job = Some(job.clone());
    app.tell("Bouncing…");
    let add = d.add_to_project;
    let (from, to) = (d.start, d.end.max(d.start + 1));
    std::thread::spawn(move || {
        let cap = render(engine, from, to, false);
        let msg = match write_bounce(&path, &cap, d.surround, d.split, bits, &exts) {
            Ok(files) => format!("Bounced {}{}", files.iter().map(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string()).collect::<Vec<_>>().join(", "), if add { " (Add to Project: drag it from the Browser)" } else { "" }),
            Err(e) => format!("Couldn't bounce: {e}"),
        };
        if let Ok(mut j) = job.lock() {
            *j = Some(msg);
        }
    });
}

/// Writes a bounce: surround as one interleaved file or split (one mono file per channel with
/// the Bounce Extensions), else a stereo file (a surround mix folded Lo/Ro).
pub fn write_bounce(path: &std::path::Path, cap: &s::Capture, surround: bool, split: bool, bits: u16, exts: &[(Ch, String)]) -> std::io::Result<Vec<PathBuf>> {
    if surround && cap.out.len() > 2 {
        if split {
            let e: Vec<String> = cap.layout.iter().map(|c| exts.iter().find(|x| x.0 == *c).map_or_else(|| c.ext(), |x| x.1.clone())).collect();
            return lg_audio::adm::write_split(path, crate::audio::RATE, &cap.out, &e, bits);
        }
        lg_audio::adm::write_interleaved(path, crate::audio::RATE, &cap.out, bits)?;
        return Ok(vec![path.to_path_buf()]);
    }
    let n = cap.out.first().map_or(0, |c| c.len());
    let (mut l, mut r) = (vec![0f32; n], vec![0f32; n]);
    if cap.out.len() == 2 {
        l.copy_from_slice(&cap.out[0]);
        r.copy_from_slice(&cap.out[1]);
    } else {
        for (k, c) in cap.layout.iter().enumerate() {
            let (gl, gr) = sur::fold_gains(*c);
            for f in 0..n {
                l[f] += cap.out[k][f] * gl;
                r[f] += cap.out[k][f] * gr;
            }
        }
    }
    lg_audio::adm::write_interleaved(path, crate::audio::RATE, &[l, r], bits)?;
    Ok(vec![path.to_path_buf()])
}

const EXPORT_FORMATS: [&str; 3] = ["Dolby Atmos ADM BWF", "Dolby Digital Plus with Dolby Atmos", "Dolby AC-4 L4"];

fn export_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut d) = app.sur_ui.export.clone() else { return };
    let mut open = true;
    let mut go = false;
    egui::Window::new("Export Spatial Audio File as").id(egui::Id::unique("sur-export")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Range:");
            ui.selectable_value(&mut d.selection, false, "Project");
            ui.selectable_value(&mut d.selection, true, "Selection");
        });
        ui.horizontal(|ui| {
            ui.label("Format:");
            egui::ComboBox::from_id_salt("exp-fmt").selected_text(EXPORT_FORMATS[d.format]).show_ui(ui, |ui| {
                ui.selectable_value(&mut d.format, 0, EXPORT_FORMATS[0]);
                for f in &EXPORT_FORMATS[1..] {
                    ui.add_enabled(false, egui::Button::new(*f)).on_disabled_hover_text("Needs Dolby's encoder, which Studio doesn't have");
                }
            });
        });
        ui.label(egui::RichText::new("The master holds every bed channel and every object (stereo objects as two), the objects' position automation, and the Downmix & Trim settings. The monitoring format doesn't change it.").small().color(look::text_dim()));
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                app.sur_ui.export = None;
            }
            if ui.button("Save…").clicked() {
                go = true;
            }
        });
    });
    if !open {
        app.sur_ui.export = None;
        return;
    }
    if app.sur_ui.export.is_none() {
        return;
    }
    app.sur_ui.export = Some(d.clone());
    if go {
        app.sur_ui.export = None;
        let (from, to) = range(app, !d.selection);
        let (from, to) = if d.selection { if app.cycle_on() { app.cycle } else { range(app, false) } } else { (0, to.max(from)) };
        let name = format!("{}.wav", app.project.as_ref().map_or("Untitled".into(), |p| p.name.clone()));
        let Some(path) = (if app.automated { None } else { kn_ui::files::FileDialog::new().set_title("Export Spatial Audio File as").set_file_name(&name).add_filter("ADM BWF", &["wav"]).save_file() }) else { return };
        export_adm(app, path, from, to);
    }
}

/// Renders and writes the ADM BWF master (in the background).
pub fn export_adm(app: &mut App, path: PathBuf, from: u32, to: u32) {
    let Some(engine) = offline(app) else { return };
    let fmt = app.sur.format;
    let meta: Vec<(String, String)> = s::to_values(app).into_iter().filter(|(k, _)| k.starts_with("Surround / render") || k.starts_with("Surround / trim")).collect();
    let name = app.project.as_ref().map_or("Untitled".into(), |p| p.name.clone());
    let job = Arc::new(Mutex::new(None));
    app.sur_ui.job = Some(job.clone());
    app.tell("Exporting Spatial Audio…");
    std::thread::spawn(move || {
        let cap = render(engine, from, to, true);
        let adm = adm_of(&cap, fmt, &name, meta);
        let msg = match adm.write(&path) {
            Ok(()) => format!("Exported {} ({} bed channels, {} objects)", path.file_name().unwrap_or_default().to_string_lossy(), adm.bed.len(), adm.objects.len()),
            Err(e) => format!("Couldn't export: {e}"),
        };
        if let Ok(mut j) = job.lock() {
            *j = Some(msg);
        }
    });
}

/// A capture as an ADM master: the bed in the project format, each object with its blocks.
pub fn adm_of(cap: &s::Capture, fmt: Format, name: &str, meta: Vec<(String, String)>) -> lg_audio::adm::Adm {
    let rate = crate::audio::RATE as f64;
    let total = cap.frames;
    let bed = fmt.channels().iter().map(|c| {
        let (az, el) = sur::position(fmt, *c);
        lg_audio::adm::BedChannel { label: c.label().into(), azimuth: az, elevation: el }
    }).collect();
    let objects = cap
        .objects
        .values()
        .map(|(n, audio, moves)| {
            let mut blocks = Vec::new();
            for (k, (at, o)) in moves.iter().enumerate() {
                let end = moves.get(k + 1).map_or(total, |m| m.0);
                blocks.push(lg_audio::adm::Block { start: *at as f64 / rate, duration: (end.saturating_sub(*at)) as f64 / rate, x: o.x, y: o.y, z: o.z, size: o.size });
            }
            let mut a = audio.clone();
            a.resize(total, 0.0);
            lg_audio::adm::AdmObject { name: n.clone(), blocks, audio: a }
        })
        .collect();
    let mut bed_audio = cap.bed.clone();
    bed_audio.resize_with(fmt.n(), Vec::new);
    for b in bed_audio.iter_mut() {
        b.resize(total, 0.0);
    }
    lg_audio::adm::Adm { rate: crate::audio::RATE, programme: name.to_string(), bed, bed_audio, objects, meta }
}

fn import_adm_dialog(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(f) = kn_ui::files::FileDialog::new().set_title("Create Project from ADM BWF").add_filter("ADM BWF", &["wav"]).pick_file() else { return };
    match import_adm(app, &f) {
        Ok(()) => {}
        Err(e) => app.tell(format!("Couldn't import: {e}")),
    }
}

/// File ▸ Import ▸ Create Project from ADM BWF: a new Atmos project with the bed merged into
/// one bed track, every object a mono object track with its positions as automation, faders
/// at 0 dB, only the Dolby Atmos plug-in on the surround master, the downmix and trim settings
/// in the Dolby Atmos plug-in.
pub fn import_adm(app: &mut App, path: &std::path::Path) -> std::io::Result<()> {
    let adm = lg_audio::adm::Adm::read(path)?;
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "ADM".into());
    let dir = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("Music/Bramble Studio").join(format!("{stem} Audio Files"));
    std::fs::create_dir_all(&dir)?;
    let chans: Vec<Ch> = adm.bed.iter().filter_map(|b| Ch::from_label(&b.label)).collect();
    let fmt = Format::of_channels(&chans).filter(|f| Format::ATMOS_BEDS.contains(f)).unwrap_or(Format::S712);
    let mut p = crate::demo::empty();
    p.name = stem.clone();
    p.tracks.clear();
    let tempo = p.tempo;
    let ppq = lg_core::events::PPQ as f64;
    let ticks = |sec: f64| (sec * tempo / 60.0 * ppq).round() as u32;
    let secs = adm.frames() as f64 / adm.rate.max(1) as f64;
    let len = ticks(secs).max(1);
    let mut outs = Vec::new();
    if !adm.bed.is_empty() {
        // the bed in the project format's order
        let ordered: Vec<Vec<f32>> = fmt.channels().iter().map(|c| chans.iter().position(|x| x == c).map(|k| adm.bed_audio[k].clone()).unwrap_or_else(|| vec![0.0; adm.frames()])).collect();
        let f = dir.join(format!("{stem} Bed.wav"));
        lg_audio::adm::write_interleaved(&f, adm.rate, &ordered, 24)?;
        let mut t = lg_core::Track::new("Bed", lg_core::TrackKind::Audio);
        t.regions.push(audio_region("Bed", f, len, secs));
        p.tracks.push(t);
        outs.push((p.tracks.len() - 1, Out::Surround));
    }
    let mut objects = Vec::new();
    for (k, o) in adm.objects.iter().enumerate() {
        let f = dir.join(format!("{stem} Object {:03}.wav", k + 1));
        lg_audio::adm::write_interleaved(&f, adm.rate, std::slice::from_ref(&o.audio), 24)?;
        let name = if o.name.is_empty() { format!("Object {}", k + 1) } else { o.name.clone() };
        let mut t = lg_core::Track::new(&name, lg_core::TrackKind::Audio);
        t.regions.push(audio_region(&name, f, len, secs));
        // the object's positions as automation
        for (param, get) in [(s::P_X, 0usize), (s::P_Y, 1), (s::P_Z, 2), (s::P_SIZE, 3)] {
            let pts: Vec<(u32, u8)> = o
                .blocks
                .iter()
                .map(|b| {
                    let v = [(b.x + 1.0) / 2.0, (b.y + 1.0) / 2.0, b.z, b.size][get];
                    (ticks(b.start), (v.clamp(0.0, 1.0) * 127.0).round() as u8)
                })
                .collect();
            if pts.len() > 1 {
                t.automation.push(lg_core::project::AutoLane { param, points: pts, ..Default::default() });
            }
        }
        p.tracks.push(t);
        let first = o.blocks.first().map_or(Object::default(), |b| Object { x: b.x, y: b.y, z: b.z, size: b.size, spread: 0.0 });
        objects.push((p.tracks.len() - 1, first));
        outs.push((p.tracks.len() - 1, Out::Object));
    }
    for (k, t) in p.tracks.iter_mut().enumerate() {
        t.row = k as u32 + 1;
        t.volume_db = 0.0;
    }
    app.stop();
    app.set_project(p, true);
    app.file = None;
    let vals: std::collections::BTreeMap<String, String> = adm.meta.iter().cloned().collect();
    let (st, _) = s::from_values(&vals);
    app.sur.atmos = true;
    app.sur.format = fmt;
    app.sur.render = st.render;
    app.sur.master = vec![crate::mixstate::new_plugin(ATMOS)];
    for (t, o) in objects {
        app.sur.objects.insert(t, o);
    }
    for (t, o) in outs {
        app.mix.outputs.insert(t, o);
    }
    app.proj.0.insert("Synchronization / General / Frame Rate:".into(), "24".into());
    app.load_engine();
    app.tell(format!("Created “{stem}” from the ADM BWF master: {} bed channels, {} objects", adm.bed.len(), adm.objects.len()));
    Ok(())
}

fn audio_region(name: &str, f: PathBuf, len: u32, secs: f64) -> lg_core::Region {
    lg_core::Region { name: name.to_string(), start: 0, length: len, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: f, seconds: secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let ctx = egui::Context::default();
        App::new(&eframe::CreationContext::_new_kittest(ctx), None)
    }

    #[test]
    fn initialize_layouts_match_logic() {
        let d: Vec<&str> = initialize(Format::S51, "Default").iter().map(|x| x.0.label()).collect();
        assert_eq!(d, vec!["L", "R", "Ls", "Rs", "C", "LFE"]);
        let i: Vec<&str> = initialize(Format::S51, "ITU").iter().map(|x| x.0.label()).collect();
        assert_eq!(i, vec!["L", "R", "C", "LFE", "Ls", "Rs"]);
    }

    #[test]
    fn surround_output_makes_a_surround_project_and_atmos_inserts_the_plugin() {
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        assert!(!s::active(&a));
        s::set_output(&mut a, 0, Out::Surround);
        assert!(s::active(&a));
        assert!(a.engine.lock().unwrap().surround.is_some());
        s::set_atmos(&mut a, true);
        assert_eq!(a.sur.format, Format::S712);
        assert_eq!(a.sur.master.first().map(|u| u.name.as_str()), Some(ATMOS));
        assert_eq!(a.proj.get("Synchronization / General / Frame Rate:"), "24");
        assert!((0..a.project.as_ref().unwrap().tracks.len()).all(|t| crate::mixstate::output_of(&a, t) == Out::Surround), "all bed tracks");
        s::set_output(&mut a, 1, Out::Object);
        assert_eq!(crate::mixstate::output_of(&a, 1), Out::Object);
        assert_eq!(crate::mixstate::out_label(Out::Object), "Object");
        // Mixer Undo restores the panners and outputs
        a.sur.objects.insert(1, Object { x: -1.0, ..Default::default() });
        crate::mixstate::checkpoint(&mut a, "x");
        a.sur.objects.insert(1, Object { x: 1.0, ..Default::default() });
        crate::mixstate::undo(&mut a);
        assert_eq!(a.sur.object(1).x, -1.0);
        // Spatial Audio off: object tracks become bed tracks, the plug-in goes
        s::set_atmos(&mut a, false);
        assert_eq!(crate::mixstate::output_of(&a, 1), Out::Surround);
        assert!(a.sur.atmos_slot().is_none());
    }

    #[test]
    fn surround_bounce_and_adm_export_round_trip() {
        let mut a = app();
        a.set_project(crate::demo::song(), true);
        s::set_atmos(&mut a, true);
        s::set_output(&mut a, 1, Out::Object);
        a.sur.objects.insert(1, Object { x: 0.5, y: -0.5, z: 0.5, size: 0.0, spread: 0.3 });
        a.load_engine();
        let e = offline(&a).unwrap();
        let cap = render(e, 0, 3840, true);
        assert_eq!(cap.layout.len(), a.sur.render.monitoring.layout().len());
        assert_eq!(cap.bed.len(), 10);
        assert!(!cap.objects.is_empty(), "the object track captured");
        let dir = std::env::temp_dir().join(format!("lg-sur-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exts: Vec<(Ch, String)> = Ch::ALL.iter().map(|c| (*c, c.ext())).collect();
        let files = write_bounce(&dir.join("Mix.wav"), &cap, true, true, 24, &exts).unwrap();
        assert_eq!(files.len(), cap.layout.len());
        assert!(files[0].to_string_lossy().ends_with("Mix.L.wav"));
        let adm = adm_of(&cap, a.sur.format, "Demo", s::to_values(&a).into_iter().filter(|(k, _)| k.starts_with("Surround / trim")).collect());
        let path = dir.join("Master.wav");
        adm.write(&path).unwrap();
        // and back as a new project
        import_adm(&mut a, &path).unwrap();
        assert!(a.sur.atmos);
        assert_eq!(a.sur.format, Format::S712);
        let p = a.project.as_ref().unwrap();
        assert_eq!(p.tracks[0].name, "Bed");
        assert_eq!(crate::mixstate::output_of(&a, 0), Out::Surround);
        assert!(p.tracks.len() >= 2 && (1..p.tracks.len()).all(|t| crate::mixstate::output_of(&a, t) == Out::Object));
        assert!(s::surround_input(&a, 0), "the bed track's file is multichannel");
        assert_eq!(a.sur.master.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
