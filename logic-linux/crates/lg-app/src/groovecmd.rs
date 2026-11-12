//! Groove templates (lg-core groove.rs) and the Region inspector's extra Quantize values.
//!
//! Make Groove Template (key command, and the end of the Region inspector's Quantize pop-up):
//! the selected regions' timing becomes a template named after the (first) selected region,
//! listed near the bottom of the Quantize pop-up; choosing it quantizes regions to it
//! (Q-Strength, Q-Velocity, Q-Length apply). MIDI sources are read live (the template follows
//! the region; gone when it is deleted); audio sources need Flex on their track and keep their
//! transients. Remove Groove Template from List takes the selected regions' template out of the
//! list (their Quantize goes to Off, already quantized notes stay where they are). Choosing a
//! Quantize value with audio regions selected turns Flex on and quantizes the audio.

use lg_core::groove::{self, Groove, Points};

use crate::app::App;

pub const COMMANDS: &[&str] = &["Make Groove Template", "Remove Groove Template from List"];

/// The Quantize pop-up's values: Logic's grid values, the swing and tuplet values, then the
/// project's groove templates.
pub fn quantize_values(app: &App) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = crate::inspector::QUANTIZE.iter().map(|q| {
        // triplets with their straight equivalent, as Logic lists them
        let name = match q.0 {
            "1/2 Triplet" => "1/2 Triplet (1/3)".to_string(),
            "1/4 Triplet" => "1/4 Triplet (1/6)".to_string(),
            "1/8 Triplet" => "1/8 Triplet (1/12)".to_string(),
            "1/16 Triplet" => "1/16 Triplet (1/24)".to_string(),
            "1/32 Triplet" => "1/32 Triplet (1/48)".to_string(),
            n => n.to_string(),
        };
        (name, q.1)
    }).collect();
    v.extend(groove::extra_values());
    if let Some(p) = &app.project {
        for (k, g) in p.grooves.iter().enumerate() {
            v.push((g.name.clone(), groove::GROOVE | k as u32));
        }
    }
    v
}

/// After a Quantize choice in the Region inspector: audio regions quantized (Flex turned on).
pub fn after_quantize_choice(app: &mut App, q: u32) {
    if let Some((grid, _)) = groove::grid(q, 50) {
        let audio = app.region_selection().iter().any(|&(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|g| g.audio.is_some()));
        if audio {
            crate::flex::quantize_audio(app, grid.round() as u32);
        }
    }
}

pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Make Groove Template" => make(app),
        "Remove Groove Template from List" => {
            let q = app.selected_region.and_then(|(t, r)| app.project.as_ref()?.tracks.get(t)?.regions.get(r)).map_or(0, |g| g.props.quantize);
            remove(app, q);
            true
        }
        _ => false,
    }
}

fn make(app: &mut App) -> bool {
    let mut sel = app.region_selection();
    sel.sort_by_key(|&(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).map_or(0, |g| g.start));
    if let Some(first) = app.selected_region {
        if let Some(k) = sel.iter().position(|x| *x == first) {
            sel.remove(k);
            sel.insert(0, first);
        }
    }
    let Some(&(t, r)) = sel.first() else {
        app.tell("Select the regions to make a groove template from");
        return true;
    };
    let Some(p) = app.project.as_ref() else { return true };
    let rg = p.tracks[t].regions[r].clone();
    let bar = lg_core::project::bar_ticks(p.signature);
    // audio sources: transients, Flex on their track
    let mut notes: Vec<lg_core::events::Note> = Vec::new();
    let mut any_audio = false;
    for &(tt, rr) in &sel {
        let Some(g) = app.project.as_ref().and_then(|p| p.tracks.get(tt)?.regions.get(rr)).cloned() else { continue };
        if g.audio.is_some() {
            if !crate::flex::track(app, tt).on {
                app.tell("Turn on Flex for the audio track to make a groove template from it");
                return true;
            }
            any_audio = true;
            for tick in crate::flex::transient_ticks(app, tt, rr) {
                notes.push(lg_core::events::Note { channel: 0, pitch: 60, velocity: 100, pos: tick, len: 120, muted: false });
            }
        } else {
            notes.extend(g.notes.iter().copied());
        }
    }
    let start = sel.iter().filter_map(|&(tt, rr)| app.project.as_ref()?.tracks.get(tt)?.regions.get(rr).map(|g| g.start)).min().unwrap_or(rg.start);
    let end = sel.iter().filter_map(|&(tt, rr)| app.project.as_ref()?.tracks.get(tt)?.regions.get(rr).map(|g| g.start + g.length)).max().unwrap_or(rg.start + rg.length);
    // several regions or audio: their positions kept with the template
    let snapshot: Option<Points> = (any_audio || sel.len() > 1).then(|| groove::points_from(&notes, start, end - start, bar, 120));
    if snapshot.as_ref().is_some_and(|s| s.points.is_empty()) || (snapshot.is_none() && rg.notes.is_empty()) {
        app.tell("The selected regions have no notes or transients to make a groove template from");
        return true;
    }
    app.checkpoint("Make Groove Template");
    if let Some(p) = app.project.as_mut() {
        // the same source again replaces its template
        p.grooves.retain(|g| !(g.track == t && g.start == rg.start && g.region == rg.name));
        p.grooves.push(Groove { name: rg.name.clone(), track: t, start: rg.start, region: rg.name.clone(), audio: snapshot });
        let n = p.grooves.len();
        app.tell(format!("Groove template “{}” made ({} in the Quantize pop-up)", rg.name, n));
    }
    app.dirty = true;
    true
}

/// Takes a template out of the list: regions quantized to it go to Off, later templates keep
/// their regions.
pub fn remove(app: &mut App, q: u32) {
    let Some(k) = groove::groove_index(q) else {
        app.tell("The selected region isn't quantized to a groove template");
        return;
    };
    if app.project.as_ref().is_none_or(|p| k >= p.grooves.len()) {
        return;
    }
    app.checkpoint("Remove Groove Template from List");
    if let Some(p) = app.project.as_mut() {
        remove_from(p, k);
    }
    app.after_edit();
}

pub fn remove_from(p: &mut lg_core::Project, k: usize) {
    p.grooves.remove(k);
    for tr in &mut p.tracks {
        for rg in &mut tr.regions {
            match groove::groove_index(rg.props.quantize) {
                Some(i) if i == k => rg.props.quantize = 0,
                Some(i) if i > k => rg.props.quantize = groove::GROOVE | (i - 1) as u32,
                _ => {}
            }
        }
    }
}

/// Saved with the region parameters (Logic's file can't hold them yet).
pub fn extras_text(p: &lg_core::Project) -> String {
    let mut out = String::new();
    for g in &p.grooves {
        let pts = g.audio.as_ref().map_or(String::from("-"), |a| {
            let v: Vec<String> = a.points.iter().map(|x| format!("{}:{}:{}", x.0, x.1, x.2)).collect();
            format!("{};{};{}", a.start, a.period, v.join(","))
        });
        out += &format!("groove\t{}\t{}\t{}\t{}\t{}\n", g.track, g.start, g.name.replace(['\t', '\n'], " "), g.region.replace(['\t', '\n'], " "), pts);
    }
    out
}

pub fn apply_extras(p: &mut lg_core::Project, text: &str) -> bool {
    let mut any = false;
    for l in text.lines().filter(|l| l.starts_with("groove\t")) {
        let w: Vec<&str> = l.split('\t').collect();
        if w.len() < 6 {
            continue;
        }
        let (Ok(track), Ok(start)) = (w[1].parse(), w[2].parse()) else { continue };
        let audio = (w[5] != "-").then(|| {
            let mut it = w[5].split(';');
            let s = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let per = it.next().and_then(|x| x.parse().ok()).unwrap_or(3840);
            let points = it.next().unwrap_or("").split(',').filter_map(|x| {
                let v: Vec<u32> = x.split(':').filter_map(|y| y.parse().ok()).collect();
                (v.len() == 3).then(|| (v[0], v[1] as u8, v[2]))
            }).collect();
            Points { start: s, period: per, points }
        });
        p.grooves.push(Groove { name: w[3].to_string(), track, start, region: w[4].to_string(), audio });
        any = true;
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grooves_round_trip_and_remove() {
        let mut p = crate::demo::empty();
        p.grooves.push(Groove { name: "Funk".into(), track: 0, start: 0, region: "Funk".into(), audio: None });
        p.grooves.push(Groove { name: "Loop".into(), track: 0, start: 3840, region: "Loop".into(), audio: Some(Points { start: 3840, period: 3840, points: vec![(0, 100, 120), (470, 100, 120)] }) });
        let text = extras_text(&p);
        let mut q = crate::demo::empty();
        assert!(apply_extras(&mut q, &text));
        assert_eq!(q.grooves, p.grooves);
        // a region quantized to the second template keeps it when the first goes
        let mut r = crate::freeze::audio_region("x", 0, 3840, std::path::PathBuf::new(), 1.0);
        r.audio = None;
        r.props.quantize = groove::GROOVE | 1;
        p.tracks[0].regions.push(r);
        let k = p.tracks[0].regions.len() - 1;
        remove_from(&mut p, 0);
        assert_eq!(p.tracks[0].regions[k].props.quantize, groove::GROOVE);
        remove_from(&mut p, 0);
        assert_eq!(p.tracks[0].regions[k].props.quantize, 0);
        assert!(p.grooves.is_empty());
    }
}
