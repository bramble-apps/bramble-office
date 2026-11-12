//! Join Regions for audio regions, and MIDI regions across tracks (Logic's Join).
//!
//! Audio regions are joined by a digital mixdown into a new file in the project's audio folder,
//! named after the first region, which replaces the regions (one undo step):
//! - regions cut from one original region, still at their relative positions: one region again,
//!   no new file;
//! - two mono regions on tracks panned hard left and right: one stereo file (no mixdown);
//! - regions on one track: mixed without volume change (region gain, fades and reverse as they
//!   play); overlapping regions ask first; regions on several tracks: each track's volume and
//!   pan in the mix;
//! - Clipscan: the mix is made in 32-bit float and, when it would clip, scaled to the highest
//!   level that doesn't;
//! - crossfades at the joins: Audio Crossfade Options for Merge… (⌥X): Crossfade Time [ms]
//!   (0: off) and Crossfade Curve (0 linear, ± exponential, always symmetrical); defaults 0 / 0.
//!
//! Transposed audio regions can't be joined. MIDI regions on adjacent tracks join into one
//! region on the upper track.

use std::path::PathBuf;

use crate::app::App;
use crate::audio::RATE;

pub const COMMANDS: &[&str] = &["Audio Crossfade Options for Merge…"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MergeXf {
    pub ms: u32,
    pub curve: i8,
}

impl Default for MergeXf {
    fn default() -> Self {
        MergeXf { ms: 0, curve: 0 }
    }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("merge-crossfade.txt"))
}

pub fn merge_xf() -> MergeXf {
    let Some(t) = settings_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return MergeXf::default() };
    let mut it = t.split_whitespace().filter_map(|x| x.parse::<i64>().ok());
    MergeXf { ms: it.next().unwrap_or(0).clamp(0, 10_000) as u32, curve: it.next().unwrap_or(0).clamp(-99, 99) as i8 }
}

fn save_xf(x: MergeXf) {
    if let Some(p) = settings_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, format!("{} {}\n", x.ms, x.curve));
    }
}

#[derive(Clone, Debug, Default)]
struct Ui {
    sheet: Option<MergeXf>,
    /// an overlapping join waiting for the answer: the regions
    confirm: Option<(Vec<(usize, usize)>, String)>,
}

fn id() -> egui::Id {
    egui::Id::unique("lg-joinaudio")
}

fn ui_state(ctx: &egui::Context) -> Ui {
    ctx.data(|d| d.get_temp::<Ui>(id())).unwrap_or_default()
}

fn set_ui(ctx: &egui::Context, u: Ui) {
    ctx.data_mut(|d| d.insert_temp(id(), u));
}

pub fn command(app: &mut App, name: &str) -> bool {
    if name == "Audio Crossfade Options for Merge…" {
        let mut u = ui_state(&app.ctx);
        u.sheet = Some(merge_xf());
        set_ui(&app.ctx, u);
        return true;
    }
    false
}

/// The crossfade gain of a join (fade-in side) at t (0..1): linear at curve 0, exponential
/// otherwise, the fade-out being its mirror.
pub fn xf_gain(t: f32, curve: i8) -> f32 {
    if curve == 0 {
        t.clamp(0.0, 1.0)
    } else {
        lg_audio::fade::curve_gain(t, curve)
    }
}

/// Join with audio in the selection, or MIDI across tracks; false leaves it to the usual Join.
pub fn join(app: &mut App) -> bool {
    let sel = app.region_selection();
    if sel.len() < 2 {
        return false;
    }
    let Some(p) = app.project.as_ref() else { return false };
    let audio: Vec<bool> = sel.iter().map(|&(t, r)| p.tracks[t].regions[r].audio.is_some()).collect();
    if audio.iter().any(|a| !a) {
        if audio.iter().any(|a| *a) {
            app.tell("Audio and MIDI regions can't be joined together");
            return true;
        }
        // MIDI: on one track as before; across adjacent tracks into the upper one
        let mut ts: Vec<usize> = sel.iter().map(|x| x.0).collect();
        ts.sort_unstable();
        ts.dedup();
        if ts.len() < 2 {
            return false;
        }
        if ts.windows(2).any(|w| w[1] != w[0] + 1) {
            app.tell("MIDI regions to be joined must be on the same or adjacent tracks");
            return true;
        }
        join_midi_tracks(app, &sel, ts[0]);
        return true;
    }
    if sel.iter().any(|&(t, r)| p.tracks[t].regions[r].props.transpose != 0) {
        app.tell("Transposed audio regions can't be joined");
        return true;
    }
    let one_track = sel.iter().all(|x| x.0 == sel[0].0);
    if one_track && overlapping(app, &sel) {
        let name = p.tracks[sel[0].0].regions[first(app, &sel).1].name.clone();
        let mut u = ui_state(&app.ctx);
        u.confirm = Some((sel, name));
        set_ui(&app.ctx, u);
        return true;
    }
    join_audio(app, &sel);
    true
}

fn first(app: &App, sel: &[(usize, usize)]) -> (usize, usize) {
    let p = app.project.as_ref().expect("project");
    *sel.iter().min_by_key(|&&(t, r)| (p.tracks[t].regions[r].start, t)).expect("selection")
}

fn overlapping(app: &App, sel: &[(usize, usize)]) -> bool {
    let Some(p) = app.project.as_ref() else { return false };
    let mut v: Vec<(u32, u32)> = sel.iter().map(|&(t, r)| (p.tracks[t].regions[r].start, p.tracks[t].regions[r].start + p.tracks[t].regions[r].length)).collect();
    v.sort_unstable();
    v.windows(2).any(|w| w[1].0 < w[0].1)
}

fn join_midi_tracks(app: &mut App, sel: &[(usize, usize)], top: usize) {
    app.checkpoint("Join Regions");
    crate::regionparams::before_join(app, sel);
    let Some(p) = app.project.as_mut() else { return };
    let start = sel.iter().map(|&(t, r)| p.tracks[t].regions[r].start).min().unwrap_or(0);
    let end = sel.iter().map(|&(t, r)| p.tracks[t].regions[r].start + p.tracks[t].regions[r].length).max().unwrap_or(0);
    let lead = *sel.iter().find(|x| x.0 == top).unwrap_or(&sel[0]);
    let mut joined = p.tracks[lead.0].regions[lead.1].clone();
    joined.start = start;
    joined.length = end - start;
    joined.notes = sel.iter().flat_map(|&(t, r)| p.tracks[t].regions[r].notes.clone()).collect();
    joined.notes.sort_by_key(|n| (n.pos, n.pitch));
    joined.ext.events = sel.iter().flat_map(|&(t, r)| {
        let g = &p.tracks[t].regions[r];
        g.ext.events.iter().map(move |e| lg_core::events::Control { pos: e.pos + g.start - start, ..*e })
    }).collect();
    joined.ext.events.sort_by_key(|e| e.pos);
    joined.origin = joined.source.or(joined.origin);
    joined.source = None;
    let mut del = sel.to_vec();
    del.sort_by(|a, b| b.cmp(a));
    for (t, r) in del {
        p.tracks[t].regions.remove(r);
    }
    p.tracks[top].regions.push(joined);
    let k = p.tracks[top].regions.len() - 1;
    app.selected_regions = vec![(top, k)];
    app.selected_region = Some((top, k));
    app.after_edit();
}

/// One region's audio as it plays, at RATE, stereo: (first frame in the output, samples).
struct Piece {
    at: usize,
    data: Vec<f32>,
    track: usize,
}

fn render_region(app: &mut App, t: usize, r: usize, t0: f64) -> Option<Piece> {
    let rg = app.project.as_ref()?.tracks.get(t)?.regions.get(r)?.clone();
    let file = rg.audio.as_ref()?.file.clone();
    let x = crate::audioedit::extras(app, t, r);
    let (s0, s1) = (app.seconds_at(rg.start as f64), app.seconds_at((rg.start + rg.length) as f64));
    let loaded = crate::audioedit::load(app, &file)?;
    let a = &loaded.audio;
    let (rate, ch) = (a.rate as f64, a.channels.max(1));
    let frames_in = a.data.len() / ch;
    let n = ((s1 - s0) * RATE as f64).round().max(0.0) as usize;
    let mut data = vec![0.0f32; n * 2];
    let gain = 10f32.powf(x.gain_db / 20.0);
    let span = (s1 - s0) * rate;
    for i in 0..n {
        let rel = i as f64 * rate / RATE as f64;
        let src = if x.reverse { x.offset as f64 + span - rel } else { x.offset as f64 + rel };
        let k = src.floor() as usize;
        if k + 1 >= frames_in {
            continue;
        }
        let f = (src - k as f64) as f32;
        let at = |fr: usize, c: usize| a.data[fr * ch + c.min(ch - 1)];
        let l = at(k, 0) * (1.0 - f) + at(k + 1, 0) * f;
        let rr = at(k, 1) * (1.0 - f) + at(k + 1, 1) * f;
        // the region's fades
        let secs = i as f64 / RATE as f64;
        let left = (s1 - s0) - secs;
        let mut g = gain;
        if x.fades.in_ms > 0 && secs * 1000.0 < x.fades.in_ms as f64 {
            g *= lg_audio::fade::curve_gain((secs * 1000.0 / x.fades.in_ms as f64) as f32, x.fades.in_curve);
        }
        if x.fades.out_ms > 0 && left * 1000.0 < x.fades.out_ms as f64 {
            g *= lg_audio::fade::curve_gain((left * 1000.0 / x.fades.out_ms as f64) as f32, x.fades.out_curve);
        }
        data[i * 2] = l * g;
        data[i * 2 + 1] = rr * g;
    }
    Some(Piece { at: ((s0 - t0) * RATE as f64).round().max(0.0) as usize, data, track: t })
}

/// Regions cut from one original and still at their relative positions (same file, each one
/// starting in the file where the one before ends, nothing changed on them).
fn same_original(app: &mut App, sel: &[(usize, usize)]) -> bool {
    let Some(p) = app.project.as_ref() else { return false };
    if sel.iter().any(|x| x.0 != sel[0].0) {
        return false;
    }
    let mut v: Vec<(usize, usize)> = sel.to_vec();
    v.sort_by_key(|&(t, r)| p.tracks[t].regions[r].start);
    let file = |t: usize, r: usize| p.tracks[t].regions[r].audio.as_ref().map(|a| a.file.clone());
    let f0 = file(v[0].0, v[0].1);
    if v.iter().any(|&(t, r)| file(t, r) != f0) {
        return false;
    }
    let Some(f0) = f0 else { return false };
    let Some(rate) = crate::audioedit::load(app, &f0).map(|l| l.audio.rate as f64) else { return false };
    for w in v.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (xa, xb) = (crate::audioedit::extras(app, a.0, a.1), crate::audioedit::extras(app, b.0, b.1));
        let p = app.project.as_ref().expect("project");
        let (ga, gb) = (&p.tracks[a.0].regions[a.1], &p.tracks[b.0].regions[b.1]);
        if ga.start + ga.length != gb.start || xa.gain_db != xb.gain_db || xa.reverse || xb.reverse || !xa.flex.is_empty() || !xb.flex.is_empty() {
            return false;
        }
        let frames_a = (app.seconds_at((ga.start + ga.length) as f64) - app.seconds_at(ga.start as f64)) * rate;
        if ((xa.offset as f64 + frames_a) - xb.offset as f64).abs() > 2.0 {
            return false;
        }
    }
    true
}

pub fn join_audio(app: &mut App, sel: &[(usize, usize)]) {
    let (ft, fr) = first(app, sel);
    let Some(p) = app.project.as_ref() else { return };
    let start = sel.iter().map(|&(t, r)| p.tracks[t].regions[r].start).min().unwrap_or(0);
    let end = sel.iter().map(|&(t, r)| p.tracks[t].regions[r].start + p.tracks[t].regions[r].length).max().unwrap_or(0);
    let name = p.tracks[ft].regions[fr].name.clone();
    // one original again: no new file
    if same_original(app, sel) {
        app.checkpoint("Join Regions");
        let keep = crate::audioedit::extras(app, ft, fr);
        let Some(p) = app.project.as_mut() else { return };
        let mut del: Vec<(usize, usize)> = sel.iter().copied().filter(|x| *x != (ft, fr)).collect();
        del.sort_by(|a, b| b.cmp(a));
        p.tracks[ft].regions[fr].length = end - start;
        let mut k = fr;
        for (t, r) in del {
            p.tracks[t].regions.remove(r);
            if t == ft && r < k {
                k -= 1;
            }
        }
        crate::audioedit::set_extras(app, ft, k, crate::audioedit::RegionAudio { fades: lg_audio::fade::RegionFades { out_ms: 0, ..keep.fades }, ..keep });
        app.selected_regions = vec![(ft, k)];
        app.selected_region = Some((ft, k));
        app.tell("Joined: the regions were one region before (no new file)");
        app.after_edit();
        return;
    }
    let t0 = app.seconds_at(start as f64);
    let total = ((app.seconds_at(end as f64) - t0) * RATE as f64).round() as usize;
    let mut tracks: Vec<usize> = sel.iter().map(|x| x.0).collect();
    tracks.sort_unstable();
    tracks.dedup();
    let mut pieces = Vec::new();
    for &(t, r) in sel {
        if let Some(pc) = render_region(app, t, r, t0) {
            pieces.push(pc);
        }
    }
    let files: Vec<PathBuf> = sel.iter().filter_map(|&(t, r)| app.project.as_ref()?.tracks[t].regions[r].audio.as_ref().map(|a| a.file.clone())).collect();
    let mono = files.iter().all(|f| crate::audioedit::load(app, f).is_some_and(|l| l.audio.channels == 1));
    let p = app.project.as_ref().expect("project");
    // two mono regions panned hard left and right: one stereo file
    let lr = tracks.len() == 2 && {
        let (a, b) = (&p.tracks[tracks[0]], &p.tracks[tracks[1]]);
        mono && a.pan * b.pan < 0.0 && a.pan.abs() >= 0.99 && b.pan.abs() >= 0.99
    };
    let mut mix = vec![0.0f32; total * 2];
    let xf = merge_xf();
    let xf_frames = (xf.ms as f64 / 1000.0 * RATE as f64) as usize;
    // the pieces' order on each track, for the crossfades at the joins
    pieces.sort_by_key(|pc| (pc.track, pc.at));
    for k in 0..pieces.len() {
        let pc = &pieces[k];
        let tr = &p.tracks[pc.track];
        let next_same = pieces.get(k + 1).filter(|n| n.track == pc.track).map(|n| n.at);
        let prev_same = (k > 0 && pieces[k - 1].track == pc.track).then(|| pieces[k - 1].at + pieces[k - 1].data.len() / 2);
        let (gl, gr) = if lr {
            if tr.pan < 0.0 { (1.0, 0.0) } else { (0.0, 1.0) }
        } else if tracks.len() > 1 {
            // the track's volume and pan (constant power)
            let v = 10f32.powf(tr.volume_db / 20.0);
            let a = (tr.pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
            (v * a.cos() * std::f32::consts::SQRT_2, v * a.sin() * std::f32::consts::SQRT_2)
        } else {
            (1.0, 1.0)
        };
        let n = pc.data.len() / 2;
        for i in 0..n {
            let o = pc.at + i;
            if o >= total {
                break;
            }
            let mut g = 1.0f32;
            if xf_frames > 0 {
                if let Some(e) = prev_same {
                    if o < e + xf_frames / 2 && i < xf_frames {
                        g *= xf_gain(i as f32 / xf_frames as f32, xf.curve);
                    }
                }
                if let Some(s) = next_same {
                    let left = n - i;
                    if left < xf_frames && o + xf_frames / 2 >= s.saturating_sub(xf_frames / 2) {
                        g *= xf_gain(left as f32 / xf_frames as f32, xf.curve);
                    }
                }
            }
            let (l, r) = (pc.data[i * 2] * g, pc.data[i * 2 + 1] * g);
            if lr {
                mix[o * 2] += l * gl;
                mix[o * 2 + 1] += l * gr;
            } else {
                mix[o * 2] += l * gl;
                mix[o * 2 + 1] += r * gr;
            }
        }
    }
    // Clipscan: the highest level that doesn't clip
    let peak = clipscan(&mut mix);
    app.checkpoint("Join Regions");
    let Some((path, secs)) = crate::freeze::write_bounce(app, &name, &mix) else { return };
    let region = crate::freeze::audio_region(&name, start, end - start, path, secs);
    let Some(p) = app.project.as_mut() else { return };
    let mut del = sel.to_vec();
    del.sort_by(|a, b| b.cmp(a));
    for (t, r) in del {
        p.tracks[t].regions.remove(r);
    }
    p.tracks[ft].regions.push(region);
    p.tracks[ft].regions.sort_by_key(|r| r.start);
    let k = p.tracks[ft].regions.iter().position(|r| r.start == start && r.name == name).unwrap_or(0);
    app.selected_regions = vec![(ft, k)];
    app.selected_region = Some((ft, k));
    app.tell(if peak > 1.0 { format!("Joined into a new file “{name}” (Clipscan lowered it {:.1} dB)", 20.0 * peak.log10()) } else { format!("Joined into a new file “{name}”") });
    app.after_edit();
}

/// Scales a mix that would clip to just under full scale; returns the peak before.
pub fn clipscan(mix: &mut [f32]) -> f32 {
    let peak = mix.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 1.0 {
        let g = 0.999 / peak;
        mix.iter_mut().for_each(|x| *x *= g);
    }
    peak
}

/// The sheets: Audio Crossfade Options for Merge, and the overlapping-join question.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let mut u = ui_state(ctx);
    let mut changed = false;
    if let Some(mut x) = u.sheet {
        let mut open = true;
        let mut close = false;
        egui::Window::new("Crossfade Options for Merge").id(egui::Id::unique("lg-merge-xf")).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("merge-xf").num_columns(2).show(ui, |ui| {
                ui.label("Crossfade Time [ms]:");
                let mut ms = x.ms as i32;
                if ui.add(egui::DragValue::new(&mut ms).range(0..=2000)).changed() {
                    x.ms = ms as u32;
                }
                ui.end_row();
                ui.label("Crossfade Curve:");
                let mut c = x.curve as i32;
                if ui.add(egui::DragValue::new(&mut c).range(-99..=99)).changed() {
                    x.curve = c as i8;
                }
                ui.end_row();
            });
            ui.label(egui::RichText::new("0 ms: no crossfade. Curve 0: linear; other values are exponential, always symmetrical.").small().color(crate::look::text_dim()));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
                if ui.button("OK").clicked() {
                    save_xf(x);
                    close = true;
                }
            });
        });
        u.sheet = if !open || close { None } else { Some(x) };
        changed = true;
    }
    if let Some((sel, name)) = u.confirm.clone() {
        let mut answer = None;
        egui::Window::new("Join Regions").id(egui::Id::unique("lg-join-confirm")).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("The regions overlap. Join them into a new audio file named “{name}”?"));
            ui.label(egui::RichText::new("They are mixed without volume change.").small().color(crate::look::text_dim()));
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
                if ui.button("Create").clicked() {
                    answer = Some(true);
                }
            });
        });
        if let Some(a) = answer {
            u.confirm = None;
            changed = true;
            if a {
                set_ui(ctx, u.clone());
                join_audio(app, &sel);
                return;
            }
        }
    }
    if changed {
        set_ui(ctx, u);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipscan_keeps_the_highest_level_without_clipping() {
        let mut m = vec![0.5, -2.0, 1.0];
        let peak = clipscan(&mut m);
        assert_eq!(peak, 2.0);
        assert!((m[1] + 0.999).abs() < 1e-6 && (m[0] - 0.24975).abs() < 1e-6);
        let mut q = vec![0.5, -0.9];
        clipscan(&mut q);
        assert_eq!(q, vec![0.5, -0.9]);
    }

    #[test]
    fn merge_crossfade_curves() {
        assert_eq!(MergeXf::default(), MergeXf { ms: 0, curve: 0 });
        assert!((xf_gain(0.25, 0) - 0.25).abs() < 1e-6);
        // exponential, the fade-out being the same shape mirrored
        assert!(xf_gain(0.5, 50) < 0.5 && xf_gain(0.5, -50) > 0.5);
    }
}
