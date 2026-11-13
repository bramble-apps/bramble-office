//! Settings ▸ View ▸ Mixer ▸ Show recent plug-in list in plug-in menu: the plug-ins picked last
//! (effects and instruments, each their own list) at the top of the slot menus. The lists
//! persist in ~/.config/bramble-studio/recent-plugins.txt ("fx\tName" / "inst\tName").

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const KEY: &str = "View / Mixer / Show recent plug-in list in plug-in menu";
/// How many each list keeps.
pub const MAX: usize = 5;

static ON: AtomicBool = AtomicBool::new(true);
static LISTS: Mutex<Option<(Vec<String>, Vec<String>)>> = Mutex::new(None);

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join("recent-plugins.txt"))
}

fn parse(t: &str) -> (Vec<String>, Vec<String>) {
    let (mut fx, mut inst) = (Vec::new(), Vec::new());
    for l in t.lines() {
        match l.split_once('\t') {
            Some(("fx", n)) if fx.len() < MAX => fx.push(n.to_string()),
            Some(("inst", n)) if inst.len() < MAX => inst.push(n.to_string()),
            _ => {}
        }
    }
    (fx, inst)
}

fn with<R>(f: impl FnOnce(&mut (Vec<String>, Vec<String>)) -> R) -> R {
    let mut g = LISTS.lock().unwrap_or_else(|e| e.into_inner());
    let l = g.get_or_insert_with(|| path().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse(&t)).unwrap_or_default());
    f(l)
}

/// Follows the setting (called where the menus' owner has the settings).
pub fn sync(v: &crate::settings::Values) {
    ON.store(v.get(KEY) != "0", Ordering::Relaxed);
}

/// Puts `name` first in a list (once), keeping MAX.
pub fn push_to(list: &mut Vec<String>, name: &str) {
    list.retain(|n| n != name);
    list.insert(0, name.to_string());
    list.truncate(MAX);
}

fn save(l: &(Vec<String>, Vec<String>)) {
    let Some(p) = path() else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let text: String = l.0.iter().map(|n| format!("fx\t{n}\n")).chain(l.1.iter().map(|n| format!("inst\t{n}\n"))).collect();
    let _ = std::fs::write(p, text);
}

pub fn used(instrument: bool, name: &str) {
    if name.is_empty() {
        return;
    }
    with(|l| {
        push_to(if instrument { &mut l.1 } else { &mut l.0 }, name);
        save(l);
    });
}

/// The recent list as menu items at the top of a slot menu (none while the setting is off);
/// returns the one clicked.
pub fn menu_items(ui: &mut egui::Ui, instrument: bool) -> Option<String> {
    if !ON.load(Ordering::Relaxed) {
        return None;
    }
    let list = with(|l| if instrument { l.1.clone() } else { l.0.clone() });
    if list.is_empty() {
        return None;
    }
    let mut picked = None;
    for n in &list {
        if ui.button(n).clicked() {
            picked = Some(n.clone());
            ui.close();
        }
    }
    ui.separator();
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recent_first_once_and_capped() {
        let mut l = Vec::new();
        for n in ["A", "B", "C", "A", "D", "E", "F"] {
            push_to(&mut l, n);
        }
        assert_eq!(l, vec!["F", "E", "D", "A", "C"]);
        let (fx, inst) = parse("fx\tChannel EQ\ninst\tES2\nfx\tCompressor\n");
        assert_eq!(fx, vec!["Channel EQ", "Compressor"]);
        assert_eq!(inst, vec!["ES2"]);
    }
}
