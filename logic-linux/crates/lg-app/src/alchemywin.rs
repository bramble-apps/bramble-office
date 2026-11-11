//! Alchemy's window: the sound engine's controls (as the Controls view lists them) and the
//! pages Logic's Alchemy has beyond them: Arpeggiator (mode, octaves, rate, note length, swing,
//! latch) with its 16-step sequencer (click a step to turn it on or off, drag its bar for the
//! velocity), MSEG (drag the points; the loop between two points; amounts to the master
//! filter, volume and pitch) and Effects (four slots of Studio's effects, mix, bypass, and each
//! effect's own controls).

use egui::{pos2, vec2, Color32, Rect, Sense, Stroke};
use lg_synth::alchemy_extra::{self as ax, FX, FX_PARAM, FX_SLOTS, FX_TYPES};

use crate::app::App;
use crate::instwin;
use crate::look;

pub fn window(app: &mut App, ctx: &egui::Context, t: usize) {
    let Some((params, vals)) = instwin::values(app, t) else { return };
    let get = |pid: u32| params.iter().position(|x| x.id == pid).map(|i| vals[i]).unwrap_or(0.0);
    let track = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
    let tab_id = egui::Id::unique(("alchemy-tab", t));
    let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
    // the effects' own parameters, read from the running instrument
    let fx_params: Vec<Option<(Vec<lg_dsp::Param>, Vec<f32>)>> = (0..FX_SLOTS).map(|s| instwin::with_inst(app, t, |a: &mut lg_synth::alchemy::Alchemy| a.extra.fx[s].as_ref().map(|f| {
        let p = f.params().to_vec();
        let v = p.iter().map(|x| f.get(x.id)).collect();
        (p, v)
    })).flatten()).collect();
    let mut set: Vec<(u32, f32)> = Vec::new();
    let mut open = true;
    let resp = egui::Window::new(format!("{track} — Alchemy")).id(egui::Id::unique(("alchemy-window", t))).open(&mut open).collapsible(false).resizable(true).default_size(vec2(780.0, 600.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (k, n) in ["Main", "Arpeggiator", "MSEG", "Effects"].iter().enumerate() {
                if ui.add(egui::Button::selectable(tab == k, *n)).clicked() {
                    tab = k;
                }
            }
        });
        ui.separator();
        match tab {
            1 => arp_page(ui, &get, &mut set),
            2 => mseg_page(ui, &get, &mut set),
            3 => fx_page(ui, &get, &fx_params, &mut set),
            _ => {
                let main: Vec<lg_dsp::Param> = params.iter().filter(|p| p.id < 1000).cloned().collect();
                let mv: Vec<f32> = main.iter().map(|p| get(p.id)).collect();
                set.extend(crate::pluginwin::controls(ui, &main, &mv, "alchemy-main"));
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(tab_id, tab));
    app.inst.rect = resp.map(|r| r.response.rect);
    if !set.is_empty() {
        instwin::checkpoint_once(app, ctx, t, "Alchemy Setting");
        instwin::set_params(app, t, &set);
    }
    if !open {
        app.plugin_window = None;
        app.inst.rect = None;
    }
}

fn row(ui: &mut egui::Ui, label: &str, pid: u32, min: f32, max: f32, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(look::text_dim()));
        let mut v = get(pid);
        if ui.add(egui::Slider::new(&mut v, min..=max)).changed() {
            set.push((pid, v));
        }
    });
}

fn menu(ui: &mut egui::Ui, label: &str, pid: u32, choices: &[&str], get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(look::text_dim()));
        let cur = get(pid).round().max(0.0) as usize;
        ui.menu_button(format!("{} ⌄", choices.get(cur).copied().unwrap_or("")), |ui| {
            for (k, c) in choices.iter().enumerate() {
                if ui.add(egui::Button::selectable(k == cur, *c)).clicked() {
                    set.push((pid, k as f32));
                    ui.close();
                }
            }
        });
    });
}

fn check(ui: &mut egui::Ui, label: &str, pid: u32, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    let mut on = get(pid) >= 0.5;
    if ui.checkbox(&mut on, label).changed() {
        set.push((pid, if on { 1.0 } else { 0.0 }));
    }
}

fn arp_page(ui: &mut egui::Ui, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    let rates: Vec<&str> = lg_synth::arp::ARP_RATES.iter().map(|r| r.0).collect();
    ui.horizontal(|ui| {
        check(ui, "Arpeggiator On", ax::ARP, get, set);
        check(ui, "Latch", ax::ARP + 6, get, set);
        check(ui, "Step Velocity", ax::ARP + 8, get, set);
    });
    menu(ui, "Mode", ax::ARP + 1, lg_synth::arp::ARP_MODES, get, set);
    menu(ui, "Rate", ax::ARP + 3, &rates, get, set);
    row(ui, "Octaves", ax::ARP + 2, 1.0, 4.0, get, set);
    row(ui, "Note Length", ax::ARP + 4, 0.05, 1.0, get, set);
    row(ui, "Swing", ax::ARP + 5, 0.0, 1.0, get, set);
    row(ui, "Steps", ax::ARP + 7, 1.0, 16.0, get, set);
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Sequencer: click a step to turn it on or off, drag its bar for the velocity; the lower strip is its length").size(11.0).color(look::text_dim()));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, look::lane_bg());
    let steps = get(ax::ARP + 7).round().clamp(1.0, 16.0) as usize;
    let w = r.width() / 16.0;
    for k in 0..16 {
        let cell = Rect::from_min_size(pos2(r.min.x + k as f32 * w, r.min.y), vec2(w, r.height()));
        let bar = Rect::from_min_max(cell.min + vec2(3.0, 4.0), pos2(cell.max.x - 3.0, cell.max.y - 30.0));
        let len_r = Rect::from_min_max(pos2(cell.min.x + 3.0, cell.max.y - 24.0), cell.max - vec2(3.0, 4.0));
        let on = get(ax::STEP_ON + k as u32) >= 0.5;
        let vel = get(ax::STEP_VEL + k as u32);
        let len = get(ax::STEP_LEN + k as u32);
        let used = k < steps;
        let col = if !used { Color32::from_gray(0x40) } else if on { look::accent() } else { Color32::from_gray(0x60) };
        p.rect_filled(Rect::from_min_max(pos2(bar.min.x, bar.max.y - bar.height() * vel), bar.max), 3.0, col);
        p.rect_stroke(bar, 3.0, Stroke::new(1.0, look::line()), egui::StrokeKind::Inside);
        p.rect_filled(Rect::from_min_size(len_r.min, vec2(len_r.width() * len, len_r.height())), 2.0, col.gamma_multiply(0.7));
        let resp = ui.interact(bar, ui.scope_id().with(("step", k)), Sense::click_and_drag());
        if resp.clicked() {
            set.push((ax::STEP_ON + k as u32, if on { 0.0 } else { 1.0 }));
        }
        if resp.dragged()
            && let Some(pp) = resp.interact_pointer_pos()
        {
            set.push((ax::STEP_VEL + k as u32, ((bar.max.y - pp.y) / bar.height()).clamp(0.0, 1.0)));
        }
        let lresp = ui.interact(len_r, ui.scope_id().with(("steplen", k)), Sense::click_and_drag());
        if (lresp.dragged() || lresp.clicked())
            && let Some(pp) = lresp.interact_pointer_pos()
        {
            set.push((ax::STEP_LEN + k as u32, ((pp.x - len_r.min.x) / len_r.width()).clamp(0.05, 1.0)));
        }
    }
}

fn mseg_page(ui: &mut egui::Ui, get: &dyn Fn(u32) -> f32, set: &mut Vec<(u32, f32)>) {
    ui.horizontal(|ui| {
        check(ui, "MSEG On", ax::MSEG, get, set);
        check(ui, "Loop", ax::MSEG + 4, get, set);
    });
    row(ui, "Points", ax::MSEG + 7, 2.0, 16.0, get, set);
    row(ui, "Loop Start (point)", ax::MSEG + 5, 0.0, 15.0, get, set);
    row(ui, "Loop End (point)", ax::MSEG + 6, 0.0, 15.0, get, set);
    row(ui, "To Cutoff", ax::MSEG + 1, -1.0, 1.0, get, set);
    row(ui, "To Volume", ax::MSEG + 2, -1.0, 1.0, get, set);
    row(ui, "To Pitch", ax::MSEG + 3, -24.0, 24.0, get, set);
    let n = get(ax::MSEG + 7).round().clamp(2.0, 16.0) as usize;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 200.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(r, 6.0, look::lane_bg());
    p.hline(r.x_range(), r.center().y, Stroke::new(1.0, look::grid_line()));
    let times: Vec<f32> = (0..n).map(|k| if k == 0 { 0.0 } else { get(ax::MSEG_TIME + k as u32) }).collect();
    let total: f32 = times.iter().sum::<f32>().max(0.5) * 1.1;
    let mut x = 0.0;
    let mut pts = Vec::new();
    for k in 0..n {
        x += times[k];
        let lv = get(ax::MSEG_LEVEL + k as u32);
        pts.push(pos2(r.min.x + x / total * r.width(), r.center().y - lv * r.height() * 0.45));
    }
    p.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, look::accent())));
    if get(ax::MSEG + 4) >= 0.5 {
        let (a, b) = (get(ax::MSEG + 5).round() as usize, get(ax::MSEG + 6).round() as usize);
        if a < pts.len() && b < pts.len() && b > a {
            p.rect_filled(Rect::from_min_max(pos2(pts[a].x, r.min.y), pos2(pts[b].x, r.max.y)), 0.0, look::accent().gamma_multiply(0.12));
        }
    }
    for (k, pt) in pts.iter().enumerate() {
        p.circle_filled(*pt, 5.0, Color32::WHITE);
        let resp = ui.interact(Rect::from_center_size(*pt, vec2(14.0, 14.0)), ui.scope_id().with(("mseg-pt", k)), Sense::drag());
        if resp.dragged() {
            let d = resp.drag_delta();
            let lv = (get(ax::MSEG_LEVEL + k as u32) - d.y / (r.height() * 0.45)).clamp(-1.0, 1.0);
            set.push((ax::MSEG_LEVEL + k as u32, lv));
            if k > 0 {
                let tm = (get(ax::MSEG_TIME + k as u32) + d.x / r.width() * total).clamp(0.0, 10.0);
                set.push((ax::MSEG_TIME + k as u32, tm));
            }
        }
    }
}

fn fx_page(ui: &mut egui::Ui, get: &dyn Fn(u32) -> f32, fx: &[Option<(Vec<lg_dsp::Param>, Vec<f32>)>], set: &mut Vec<(u32, f32)>) {
    egui::ScrollArea::vertical().id_salt("alchemy-fx").show(ui, |ui| {
        for s in 0..FX_SLOTS {
            let b = FX + s as u32 * 10;
            egui::Frame::new().fill(look::group_bg()).corner_radius(6.0).inner_margin(6.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Effect {}", s + 1)).strong());
                    menu(ui, "", b, FX_TYPES, get, set);
                    check(ui, "Bypass", b + 2, get, set);
                    row(ui, "Mix", b + 1, 0.0, 1.0, get, set);
                });
                if let Some(Some((params, vals))) = fx.get(s) {
                    egui::Grid::new(("alch-fx", s)).num_columns(4).show(ui, |ui| {
                        for (i, prm) in params.iter().enumerate().take(24) {
                            ui.label(egui::RichText::new(prm.name).size(11.0).color(look::text_dim()));
                            let mut v = vals[i];
                            let id = FX_PARAM + s as u32 * 100 + prm.id;
                            let ch = match prm.unit {
                                lg_dsp::Unit::OnOff => {
                                    let mut on = v >= 0.5;
                                    let c = ui.checkbox(&mut on, "").changed();
                                    v = if on { 1.0 } else { 0.0 };
                                    c
                                }
                                lg_dsp::Unit::Menu => {
                                    let mut c = false;
                                    ui.menu_button(prm.choices.get(v.round() as usize).copied().unwrap_or(""), |ui| {
                                        for (j, n) in prm.choices.iter().enumerate() {
                                            if ui.button(*n).clicked() {
                                                v = j as f32;
                                                c = true;
                                                ui.close();
                                            }
                                        }
                                    });
                                    c
                                }
                                _ => ui.add(egui::DragValue::new(&mut v).range(prm.min..=prm.max).speed((prm.max - prm.min) / 300.0)).changed(),
                            };
                            if ch {
                                set.push((id, v));
                            }
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                }
            });
        }
    });
}
