//! The Browser's Plug-ins view (a Bramble extra; Logic 13 has no one place for them): every
//! plug-in Studio can load in one list: its own instruments and effects, then the hosted CLAP /
//! VST3 / LV2 ones the Plug-in Manager found. The Browser's search, favourite stars and
//! Favorites filter work on it as on the other views; a click (Replace Mode) or New Track with
//! Selected Item puts an instrument in the track's instrument slot, an effect in its next free
//! Audio FX slot (undoable).

use crate::app::App;
use crate::soundlib::{self as sl, Item, Kind, LoopType};

/// The pack the view's items carry (kept out of the Instruments and Effects views).
pub const PACK: &str = "Plug-ins";

/// The menu items this file runs (Bramble's own: not in Logic's lists).
pub const COMMANDS: &[&str] = &[];

fn item(id: String, name: String, effect: bool, category: String, extra_tags: &[&str]) -> Item {
    let mut tags = sl::infer_tags(&name);
    tags.extend(extra_tags.iter().filter(|t| !t.is_empty()).map(|t| t.to_string()));
    Item { id, name, kind: Kind::Patch, loop_type: if effect { LoopType::Audio } else { LoopType::Midi }, category, path: None, tags, groups: Vec::new(), pack: PACK.into(), user: false, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None }
}

/// Every plug-in: Studio's instruments, Studio's effects, then the hosted ones (by name).
pub fn items() -> Vec<Item> {
    let mut v = Vec::new();
    let mut own: Vec<&str> = crate::instwin::SLOT_MENU.iter().flat_map(|(_, l)| l.iter().map(|x| x.1)).collect();
    own.push("Drum Machine Designer");
    own.sort_unstable();
    own.dedup();
    for n in own {
        v.push(item(format!("plug:inst:{n}"), n.to_string(), false, "Bramble Instrument".into(), &["Instrument", "Bramble"]));
    }
    let mut fx = crate::units::effect_names();
    fx.sort_unstable();
    for n in fx {
        v.push(item(format!("plug:fx:{n}"), n.to_string(), true, "Bramble Effect".into(), &["Effect", "Bramble"]));
    }
    let mut hosted = lg_host::plugins();
    hosted.sort_by_key(|p| p.name.to_lowercase());
    for p in hosted {
        let effect = p.kind == lg_host::Kind::Effect;
        let f = p.format.label();
        let what = if effect { "Effect" } else { "Instrument" };
        v.push(item(format!("plug:host:{}", p.studio_name()), p.studio_name(), effect, format!("{f} {what}"), &[what, f, p.manufacturer()]));
    }
    v
}

/// What a Plug-ins item puts where: (instrument?, Studio's name for it, hosted?).
pub fn target(it: &Item) -> Option<(bool, String, bool)> {
    let rest = it.id.strip_prefix("plug:")?;
    if let Some(n) = rest.strip_prefix("inst:") {
        return Some((true, n.to_string(), false));
    }
    if let Some(n) = rest.strip_prefix("fx:") {
        return Some((false, n.to_string(), false));
    }
    let n = rest.strip_prefix("host:")?;
    Some((it.loop_type != LoopType::Audio, n.to_string(), true))
}

/// Loads a Plug-ins item onto track `t`.
pub fn load(app: &mut App, t: usize, it: &Item) {
    let Some((inst, name, hosted)) = target(it) else { return };
    if inst {
        if hosted {
            crate::hosting::set_instrument(app, t, Some(name));
            return;
        }
        crate::mixstate::checkpoint(app, "Insert Plug-in");
        // (a hosted instrument there makes way for Studio's own)
        if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
            tr.plugins.retain(|u| !(u.slot == 2 && lg_host::parse_studio_name(&u.name).is_some()));
        }
        app.instrument_choice.insert(t, name);
        app.dirty = true;
        app.load_engine();
        app.plugin_window = Some(t);
        return;
    }
    crate::mixstate::checkpoint(app, "Insert Plug-in");
    crate::mixstate::edit_inserts(app, t, |v| v.push(crate::mixstate::new_plugin(&name)));
    app.dirty = true;
    app.load_engine();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_lists_studios_own_and_says_where_each_goes() {
        let v = items();
        let alchemy = v.iter().find(|i| i.name == "Alchemy").expect("Alchemy listed");
        assert_eq!(target(alchemy), Some((true, "Alchemy".into(), false)));
        let comp = v.iter().find(|i| i.name == "Compressor").expect("Compressor listed");
        assert_eq!(target(comp), Some((false, "Compressor".into(), false)));
        assert!(v.iter().all(|i| i.pack == PACK && i.kind == Kind::Patch));
        // ids are unique (favourites are kept by id)
        let ids: std::collections::HashSet<&str> = v.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids.len(), v.len());
        let h = item("plug:host:Surge XT (CLAP)".into(), "Surge XT (CLAP)".into(), false, "CLAP Instrument".into(), &["Instrument", "CLAP"]);
        assert_eq!(target(&h), Some((true, "Surge XT (CLAP)".into(), true)));
        assert!(h.tags.iter().any(|t| t == "CLAP"));
    }
}
