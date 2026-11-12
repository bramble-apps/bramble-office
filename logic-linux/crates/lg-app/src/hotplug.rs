//! Audio devices while Studio is open (Logic's plug-and-play Core Audio handling, here on the
//! system's ALSA / PipeWire devices): a device that appears raises an alert to use it (as the
//! output, and as the input when it has inputs) or not; choosing an output device that has
//! inputs makes it the input device too; choosing an input other than the output advises a
//! common word clock.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::app::App;

pub const COMMANDS: &[&str] = &[];

/// Names in `now` that weren't in `before` (in `now`'s order); "System Default" never counts.
pub fn appeared(before: &BTreeSet<String>, now: &[String]) -> Vec<String> {
    now.iter().filter(|n| *n != "System Default" && !before.contains(*n)).cloned().collect()
}

/// After the Output Device changed to `out`: the input device to use (the same device when it
/// has inputs), else None (left as it is).
pub fn input_for_output(out: &str, inputs: &[String]) -> Option<String> {
    (out != "System Default" && inputs.iter().any(|i| i == out)).then(|| out.to_string())
}

/// The advice shown when the input and output devices differ.
pub fn clock_advice(input: &str, output: &str) -> Option<String> {
    (input != output && input != "System Default" && output != "System Default").then(|| format!("Input “{input}” and output “{output}” are different devices: for glitch-free audio, clock them from a common word clock (or use one device for both)."))
}

#[derive(Default)]
struct Scan {
    /// The output and input device lists the scanner found last (None: not yet).
    latest: Option<(Vec<String>, Vec<String>)>,
    started: bool,
}

#[derive(Default)]
pub struct State {
    shared: Arc<Mutex<Scan>>,
    known: Option<BTreeSet<String>>,
    inputs: Vec<String>,
    /// A device that appeared, waiting for the alert's answer.
    pub pending: Option<String>,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

fn start(shared: Arc<Mutex<Scan>>) {
    std::thread::Builder::new()
        .name("audio-hotplug".into())
        .spawn(move || loop {
            let outs = crate::prefs::devices(true);
            let ins = crate::prefs::devices(false);
            if let Ok(mut s) = shared.lock() {
                s.latest = Some((outs, ins));
            }
            std::thread::sleep(Duration::from_secs(3));
        })
        .ok();
}

/// Each frame: picks up the scanner's lists and shows the new-device alert.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    if std::env::var_os("LG_SNAP").is_some() || cfg!(test) {
        return;
    }
    let found = STATE.with(|st| {
        let mut st = st.borrow_mut();
        let latest = {
            let Ok(mut s) = st.shared.lock() else { return None };
            if !s.started {
                s.started = true;
                start(st.shared.clone());
            }
            s.latest.take()
        };
        let (outs, ins) = latest?;
        st.inputs = ins;
        let new = st.known.as_ref().map(|k| appeared(k, &outs)).unwrap_or_default();
        st.known = Some(outs.into_iter().collect());
        if st.pending.is_none() {
            st.pending = new.into_iter().next();
        }
        None::<()>
    });
    let _ = found;
    ctx.request_repaint_after(Duration::from_secs(3));
    let Some(name) = STATE.with(|st| st.borrow().pending.clone()) else { return };
    let mut answer = None;
    egui::Modal::new(egui::Id::unique("audio-hotplug")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(egui::RichText::new("New audio device found").strong());
        ui.add_space(4.0);
        ui.label(format!("“{name}” was connected. Do you want to use it for audio in Bramble Studio?"));
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Use").clicked() {
                answer = Some(true);
            }
            if ui.button("Ignore").clicked() {
                answer = Some(false);
            }
        });
    });
    if let Some(k) = crate::surfdialog::offer(&format!("New audio device: {name}. Use it?"), &["Use", "Ignore"], 0, Some(1)) {
        answer = Some(k == 0);
    }
    if let Some(yes) = answer {
        STATE.with(|st| st.borrow_mut().pending = None);
        if yes {
            app.logic_settings.0.insert(crate::prefs::OUTPUT_DEVICE.to_string(), name.clone());
            app.logic_settings.save();
            crate::prefs::changed(app, crate::prefs::OUTPUT_DEVICE);
            crate::prefs::open_output(app);
        }
    }
}

/// Settings ▸ Audio ▸ Devices: the output device changed (the input follows when it can),
/// or the input did (the word clock advice).
pub fn device_changed(app: &mut App, key: &str) {
    if key == crate::prefs::OUTPUT_DEVICE {
        let out = app.logic_settings.get(key);
        let inputs = STATE.with(|st| st.borrow().inputs.clone());
        let inputs = if inputs.is_empty() && std::env::var_os("LG_SNAP").is_none() && !cfg!(test) { crate::prefs::devices(false) } else { inputs };
        if let Some(i) = input_for_output(&out, &inputs) {
            if app.logic_settings.get(crate::prefs::INPUT_DEVICE) != i {
                app.logic_settings.0.insert(crate::prefs::INPUT_DEVICE.to_string(), i);
                app.logic_settings.save();
                app.rec.reset_device();
            }
        }
    } else if key == crate::prefs::INPUT_DEVICE {
        let (i, o) = (app.logic_settings.get(key), app.logic_settings.get(crate::prefs::OUTPUT_DEVICE));
        if let Some(msg) = clock_advice(&i, &o) {
            app.tell(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_devices() {
        let before: BTreeSet<String> = ["System Default", "HDA Intel PCH"].iter().map(|s| s.to_string()).collect();
        let now = vec!["System Default".to_string(), "HDA Intel PCH".to_string(), "Scarlett 2i2 USB".to_string()];
        assert_eq!(appeared(&before, &now), vec!["Scarlett 2i2 USB".to_string()]);
        assert!(appeared(&before, &now[..2]).is_empty());
    }

    #[test]
    fn input_follows_output() {
        let ins = vec!["System Default".to_string(), "Scarlett 2i2 USB".to_string()];
        assert_eq!(input_for_output("Scarlett 2i2 USB", &ins), Some("Scarlett 2i2 USB".into()));
        // an output-only device leaves the input
        assert_eq!(input_for_output("HDMI Out", &ins), None);
        assert!(clock_advice("Scarlett 2i2 USB", "HDA Intel PCH").unwrap().contains("word clock"));
        assert!(clock_advice("Scarlett 2i2 USB", "Scarlett 2i2 USB").is_none());
    }
}
