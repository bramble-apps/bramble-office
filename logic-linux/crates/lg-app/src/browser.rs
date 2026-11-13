//! The Browser (Logic Pro 13's left pane), as the guide's Browser pages describe it:
//! - the header: "Browser", Link to Selection (follows the selected track: an instrument track
//!   shows Instruments, an audio track Effects, its patch listed first) and Replace Mode (a click
//!   on a patch loads it onto the selected track);
//! - the search field: typing lists the search modes (Name Contains, Sounds like) and matching
//!   tags; Return searches by description; the All Filters button opens every tag by category
//!   (genre, instrument with subtags, paired descriptors, group tags with values, sound packs,
//!   Your Sounds, Favorites); the active criteria show as chips above the results (click one to
//!   remove it, the cross clears them all);
//! - the navigation: All, Loops & One-Shots (with its Loops / One-Shots buttons), Instruments,
//!   Effects, Plug-in Presets (column layout, path bar), Sound Library, Bramble's SoundFonts, then
//!   Locations (the user's folders, analysed) and Smart Searches (saved searches); the Add
//!   button (Add Location, Add Smart Search); compact navigation as an icon row;
//! - the results: type colours (audio loops blue, MIDI green, Session Player yellow, pattern
//!   violet), the favourite star, a click previews (loops) or loads (patches), drag to the
//!   tracks (the right track type), the shortcut menu (New Track with Loop, Place Loop at
//!   Playhead, Create Track with Loop ▸ Quick Sampler / Sample Alchemy / Drum Machine Designer,
//!   Add to Drum Machine Designer, Find Similar, favourites, Rename, Delete, Show in Finder,
//!   View Info);
//! - the footer: sort, the count, the preview volume, More (Sort, Loop Settings, Loop Chords,
//!   Use Compact Navigation, Save Patch for Selected Track, Enable Patch Merging, Columns View).
//! No Apple loops ship: loops come from the user's folders and Bramble's sample library.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use egui::{pos2, vec2, Align2, Color32, Rect, Sense, Stroke};
use lg_core::TrackKind;

use crate::app::{App, SoundFontMode};
use crate::glyphs::{ph, ph_fill};
use crate::look;
use crate::soundlib::{self as sl, Item, Kind, LoopType, Search};

/// The Browser key commands and menu items this file runs.
pub const COMMANDS: &[&str] = &[
    "View Info", "Play/Stop Preview", "Clear Applied Tags", "Toggle All Filters", "Save Patch for Selected Track…", "Enable Patch Merging", "Add Location", "Add Smart Search", "New Track with Selected Item", "Replace Patch", "Find Similar", "Add to Favorites", "Rename", "Delete", "Show in Finder", "Toggle Columns View",
    "Search in Browser", "Show Loops & One-Shots in Browser", "Show Instruments in Browser", "Show Effects in Browser",
    // the loop shortcut menu
    "New Track with Loop", "Place Loop at Playhead", "Create Track with Loop", "Quick Sampler", "Sample Alchemy", "Drum Machine Designer",
    "Add to Browser…",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cat {
    All,
    Loops,
    Instruments,
    Effects,
    /// Bramble's: every plug-in, Studio's own and hosted (plugbrowser.rs).
    Plugins,
    Presets,
    SoundLibrary,
    SoundFonts,
    Location(usize),
    YourPatches,
    YourLoops,
    Smart(usize),
}

const NAV: [(Cat, &str, &str); 8] = [(Cat::All, "All", ph::SQUARES_FOUR), (Cat::Loops, "Loops & One-Shots", ph::WAVEFORM), (Cat::Instruments, "Instruments", ph::PIANO_KEYS), (Cat::Effects, "Effects", ph::FADERS), (Cat::Plugins, "Plug-ins", ph::PLUG), (Cat::Presets, "Plug-in Presets", ph::SLIDERS), (Cat::SoundLibrary, "Sound Library", ph::BOOKS), (Cat::SoundFonts, "SoundFonts", ph::MUSIC_NOTES)];

/// Logic's instrument patches: (category, subcategory, name).
pub fn patches() -> &'static [(String, String, String)] {
    static P: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    P.get_or_init(|| {
        include_str!("../data/patches.txt").lines().filter(|l| !l.starts_with('#') && !l.is_empty()).filter_map(|l| {
            let mut it = l.split('\t');
            Some((it.next()?.to_string(), it.next()?.to_string(), it.next()?.to_string()))
        }).collect()
    })
}

/// The favourites saved (the Browser's own list, for other areas).
pub fn load_favorites() -> Vec<String> {
    let mut v: Vec<String> = sl::Saved::load().favorites.into_iter().collect();
    v.sort();
    v
}

/// Logic's order for the list ("Relevance"): the everyday instruments first.
fn relevance(cat: &str) -> u8 {
    match cat {
        "Library" => 0,
        "Keyboard" => 1,
        "Bass" => 2,
        "Guitar" => 3,
        "Strings" => 4,
        "Brass & Woodwind" => 5,
        "Mallet" => 6,
        "Synthesizer" => 7,
        _ => 8,
    }
}

/// One thing being dragged out of a browser.
#[derive(Clone, Debug)]
pub enum DragItem {
    Item(Item),
    /// A file (from Project Audio / All Files), and the part of it (seconds).
    File(PathBuf, Option<(f64, f64)>),
}

/// Add Selected Files to Tracks (several audio files dropped below the tracks).
#[derive(Clone, Debug)]
pub struct MultiDrop {
    pub items: Vec<DragItem>,
    pub at: u32,
    pub choice: usize,
}

/// Add to Browser… (regions saved as loops or one-shots).
#[derive(Clone, Debug)]
pub struct AddToBrowser {
    pub regions: Vec<(usize, usize)>,
    pub name: String,
    pub one_shot: bool,
    pub key: String,
    pub minor: bool,
    pub can_loop: bool,
}

pub struct State {
    pub saved: sl::Saved,
    pub search: Search,
    pub typed: String,
    pub view: Cat,
    pub selected: Vec<String>,
    items: Vec<Item>,
    /// Link to Selection's reference patch: the selected track's patch when that track was
    /// selected (the list's order then stays put while sounds are clicked and loaded).
    pub link_anchor: Option<(usize, String)>,
    loaded: bool,
    pub all_filters: bool,
    pub expanded: HashSet<String>,
    pub show_more: HashSet<String>,
    pub info: Option<String>,
    pub renaming: Option<(String, String)>,
    pub deleting: Option<String>,
    pub smart_name: Option<String>,
    pub smart_rename: Option<(usize, String)>,
    pub location_rename: Option<(usize, String)>,
    pub patch_name: Option<String>,
    pub add_to_browser: Option<AddToBrowser>,
    pub volume: f32,
    pub drag: Option<Vec<DragItem>>,
    pub multi_drop: Option<MultiDrop>,
    /// The Tracks area's lanes (drop target), the Browser's rect and its search field.
    pub lanes: Rect,
    pub rect: Rect,
    search_rect: Rect,
    /// The track kind last seen (Link to Selection switches the view when it changes).
    linked: Option<(usize, TrackKind)>,
    /// Plug-in Presets: the plug-in (None = the instrument, Some(k) = Audio FX k), the path.
    pub preset_slot: Option<usize>,
    pub preset_path: Vec<String>,
    /// Columns view (patches by category and subcategory): the open column path.
    pub column_path: Vec<String>,
    locations_open: bool,
    smart_open: bool,
    /// Previews by dragging across icons / Option-hovering them (brpreview.rs): the sound last
    /// reached, and whether a drag started on an icon.
    pub swept: Option<String>,
    pub sweeping: bool,
}

impl Default for State {
    fn default() -> State {
        State { saved: sl::Saved::default(), search: Search::default(), typed: String::new(), view: Cat::Instruments, selected: Vec::new(), items: Vec::new(), link_anchor: None, loaded: false, all_filters: false, expanded: HashSet::new(), show_more: HashSet::new(), info: None, renaming: None, deleting: None, smart_name: None, smart_rename: None, location_rename: None, patch_name: None, add_to_browser: None, volume: 0.8, drag: None, multi_drop: None, lanes: Rect::NOTHING, rect: Rect::NOTHING, search_rect: Rect::NOTHING, linked: None, preset_slot: None, preset_path: Vec::new(), column_path: Vec::new(), locations_open: true, smart_open: true, swept: None, sweeping: false }
    }
}

// ---- content

fn effect_items() -> Vec<Item> {
    crate::units::effect_names().into_iter().map(|n| Item { id: format!("fx:{n}"), name: n.to_string(), kind: Kind::Patch, loop_type: LoopType::Audio, category: "Audio Effects".into(), path: None, tags: { let mut t = sl::infer_tags(n); t.extend(crate::allfilters::fx_category(n).map(str::to_string)); t }, groups: Vec::new(), pack: "Bramble Effects".into(), user: false, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None }).collect()
}

/// The user's saved patches (Save Patch for Selected Track).
fn user_patches() -> Vec<Item> {
    let mut v = Vec::new();
    for (sub, audio) in [("Instrument", false), ("Audio", true)] {
        let dir = sl::user_dir("Patches").join(sub);
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "patch")).collect();
        files.sort();
        for f in files {
            let name = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let inst = std::fs::read_to_string(&f).ok().and_then(|t| t.lines().find_map(|l| l.strip_prefix("instrument\t").map(str::to_string))).unwrap_or_default();
            v.push(Item { id: f.display().to_string(), tags: sl::infer_tags(&format!("{name} {inst}")), name, kind: Kind::Patch, loop_type: if audio { LoopType::Audio } else { LoopType::Midi }, category: if audio { "Audio Effects".into() } else { inst }, path: Some(f), groups: Vec::new(), pack: "Your Patches".into(), user: true, bpm: None, key: None, minor: false, bars: None, seconds: None, location: None });
        }
    }
    v
}

/// The user's loops saved with Add to Browser.
fn user_loops() -> Vec<Item> {
    let dir = sl::user_dir("Loops");
    sl::scan(&dir, 5000).iter().filter_map(|f| sl::analyze(f, "Your Loops", None, "")).collect()
}

fn ensure_loaded(app: &mut App) {
    if app.br.loaded {
        return;
    }
    app.br.loaded = true;
    if app.br.saved == sl::Saved::default() {
        app.br.saved = sl::Saved::load();
    }
    let mut items = sl::patch_items();
    items.extend(effect_items());
    items.extend(crate::plugbrowser::items());
    items.extend(user_patches());
    items.extend(user_loops());
    if let Some(root) = lg_sounds::root() {
        items.extend(sl::library_one_shots(root));
    }
    // (checks: LG_BROWSER_LOCATION=<folder> adds a location)
    if let Some(d) = std::env::var_os("LG_BROWSER_LOCATION").map(PathBuf::from) {
        if !app.br.saved.locations.iter().any(|l| l.path == d) {
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            app.br.saved.locations.push(sl::Location { name, path: d });
        }
    }
    for (i, l) in app.br.saved.locations.iter().enumerate() {
        items.extend(sl::location_items(&l.path, &l.name, i));
    }
    // (the attributes saved with loops made here)
    for it in items.iter_mut() {
        if let Some(p) = it.path.clone() {
            apply_sidecar(it, &p);
        }
    }
    app.br.items = items;
}

/// Forgets the content (it's read again when shown).
pub fn reload(app: &mut App) {
    app.br.loaded = false;
}

fn sidecar(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".tags");
    PathBuf::from(s)
}

/// A loop's saved attributes ("type=loop", "bpm=…", "key=A", "minor=1", "bars=…", "tags=…").
fn apply_sidecar(it: &mut Item, p: &Path) {
    let Ok(t) = std::fs::read_to_string(sidecar(p)) else { return };
    for l in t.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        match k {
            "type" => it.kind = if v == "one-shot" { Kind::OneShot } else { Kind::Loop },
            "bpm" => it.bpm = v.parse().ok(),
            "key" => it.key = (!v.is_empty()).then(|| v.to_string()),
            "minor" => it.minor = v == "1",
            "bars" => it.bars = v.parse().ok(),
            // (tags edited in the Info window replace the others)
            "settags" => it.tags = v.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            "tags" => {
                for tag in v.split(',').filter(|s| !s.is_empty()) {
                    if !it.tags.iter().any(|x| x == tag) {
                        it.tags.push(tag.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

fn item(app: &App, id: &str) -> Option<Item> {
    app.br.items.iter().find(|i| i.id == id).cloned()
}

fn selected_items(app: &App) -> Vec<Item> {
    app.br.selected.iter().filter_map(|id| item(app, id)).collect()
}

fn in_view(app: &App, it: &Item) -> bool {
    // (the Plug-ins view's items only there)
    if (it.pack == crate::plugbrowser::PACK) != (app.br.view == Cat::Plugins) {
        return false;
    }
    match app.br.view {
        Cat::All => true,
        Cat::Plugins => true,
        Cat::Loops => matches!(it.kind, Kind::Loop | Kind::OneShot) && app.br.search.loops_only.is_none_or(|l| (it.kind == Kind::Loop) == l),
        Cat::Instruments => it.kind == Kind::Patch && it.loop_type != LoopType::Audio,
        Cat::Effects => it.kind == Kind::Patch && it.loop_type == LoopType::Audio,
        Cat::Location(i) => it.location == Some(i),
        Cat::YourPatches => it.pack == "Your Patches",
        Cat::YourLoops => it.pack == "Your Loops",
        Cat::Smart(_) => true,
        Cat::Presets | Cat::SoundLibrary | Cat::SoundFonts => false,
    }
}

/// The selected track's kind, and whether it's a folder stack's main track.
fn track_kind(app: &App) -> Option<(usize, TrackKind, bool)> {
    let t = app.selected_track?;
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let folder = tr.extra.stack.is_some_and(|s| matches!(s.kind, lg_core::arrange::StackKind::Folder | lg_core::arrange::StackKind::PackedFolder));
    Some((t, tr.kind, folder))
}

/// The results: view, then search, then order.
pub fn results(app: &App) -> Vec<Item> {
    let s = &app.br.search;
    let fav = &app.br.saved.favorites;
    // a folder stack's main track: no patches (Track stacks and the Browser)
    let folder = track_kind(app).is_some_and(|k| k.2);
    let mut v: Vec<Item> = app.br.items.iter().filter(|it| in_view(app, it) && s.matches(it, fav) && !(folder && it.kind == Kind::Patch)).cloned().collect();
    if let Some((id, _)) = &s.similar {
        if let Some(r) = item(app, id).or_else(|| similar_ref(app, id)) {
            let mut scored: Vec<(f32, Item)> = v.into_iter().filter(|x| x.id != r.id).map(|x| (sl::similarity(&r, &x), x)).collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)).then_with(|| a.1.id.cmp(&b.1.id)));
            scored.truncate(sl::SIMILAR_MAX);
            return scored.into_iter().map(|x| x.1).collect();
        }
    }
    let order = sort_order(app);
    if order != "Relevance" {
        sort_items(&mut v, &order, fav);
        if app.br.saved.flag("sort_desc", false) {
            v.reverse();
        }
    } else {
        // Relevance: Logic's order for patches (its first 30 as listed, then by category);
        // Link to Selection with no search: the track's patch first, then similar ones
        let pos: std::collections::HashMap<&str, usize> = patches().iter().enumerate().map(|(i, p)| (p.2.as_str(), i)).collect();
        // (ties: by name, then by id, so the order never depends on how the items were read)
        v.sort_by_cached_key(|x| {
            let p = pos.get(x.name.as_str()).copied().unwrap_or(usize::MAX);
            let (a, b, n) = if x.kind == Kind::Patch && x.pack == "Logic Patches" {
                if p < 30 { (0, p, String::new()) } else { (1 + relevance(&x.category) as usize, 0, x.name.clone()) }
            } else {
                (20 + x.kind as usize, 0, x.name.to_lowercase())
            };
            (a, b, n, x.name.clone(), x.id.clone())
        });
        // (the reference is the patch the track had when it was selected, not the one a click
        // just loaded: clicking sounds never reorders the list)
        let anchor = app.br.link_anchor.as_ref().filter(|(t, _)| app.selected_track == Some(*t)).map(|a| a.1.as_str());
        if app.browser_link && s.is_empty() && matches!(app.br.view, Cat::Instruments | Cat::Effects) {
            if let Some(cur) = anchor.and_then(|n| v.iter().position(|x| x.name == n)) {
                let c = v.remove(cur);
                let mut rest: Vec<(f32, Item)> = v.into_iter().map(|x| (sl::similarity(&c, &x), x)).collect();
                rest.sort_by(|a, b| b.0.total_cmp(&a.0));
                v = std::iter::once(c).chain(rest.into_iter().map(|x| x.1)).collect();
            }
        }
    }
    v
}

/// More ▸ Sort By: the criteria (Logic's), the loop-only ones last.
pub const SORTS: [&str; 6] = ["Relevance", "Name", "Type", "User Content", "Favorites", "Date Added"];
pub const LOOP_SORTS: [&str; 3] = ["Tempo", "Bars", "Key"];

/// The results' sort order: More ▸ Sort By (or a column header); "Relevance" by default.
pub fn sort_order(app: &App) -> String {
    match app.br.saved.settings.get("sort") {
        Some(s) if !s.is_empty() => s.clone(),
        _ if app.browser_sort_name => "Name".into(),
        _ => "Relevance".into(),
    }
}

fn set_sort(app: &mut App, order: &str, desc: bool) {
    app.br.saved.settings.insert("sort".into(), order.into());
    app.br.saved.set_flag("sort_desc", desc && order != "Relevance");
    app.browser_sort_name = order == "Name";
    save_state(app);
}

/// Sorts results by a Sort By criterion, ascending (ties by name).
pub fn sort_items(v: &mut [Item], order: &str, fav: &HashSet<String>) {
    let key_pos = |it: &Item| it.key.as_deref().and_then(lg_core::timeline::root_of).map_or(99, |r| r as i32 * 2 + it.minor as i32);
    let added = |it: &Item| it.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.created().or_else(|_| m.modified()).ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    v.sort_by_cached_key(|it| {
        let k: (i64, i64) = match order {
            "Type" => (it.kind as i64, it.loop_type as i64),
            "User Content" => (!it.user as i64, 0),
            "Favorites" => (!fav.contains(&it.id) as i64, 0),
            "Date Added" => (added(it) as i64, 0),
            // (sounds without the attribute after the others)
            "Tempo" => (it.bpm.map_or(i64::MAX, |b| (b * 100.0) as i64), 0),
            "Bars" => (it.bars.map_or(i64::MAX, |b| b as i64), 0),
            "Key" => (key_pos(it) as i64, 0),
            _ => (0, 0),
        };
        (k, it.name.to_lowercase(), it.id.clone())
    });
}

/// A region dragged to the search field: a reference made from it.
fn similar_ref(app: &App, id: &str) -> Option<Item> {
    let (t, r) = id.strip_prefix("region:")?.split_once(':')?;
    let (t, r): (usize, usize) = (t.parse().ok()?, r.parse().ok()?);
    let tr = app.project.as_ref()?.tracks.get(t)?;
    let rg = tr.regions.get(r)?;
    let mut tags = sl::infer_tags(&format!("{} {} {}", tr.name, rg.name, app.instrument_for(t).unwrap_or_default()));
    if tr.kind == TrackKind::Drummer {
        tags.push("Drums".into());
    }
    Some(Item { id: id.into(), name: rg.name.clone(), kind: Kind::Loop, loop_type: if rg.audio.is_some() { LoopType::Audio } else { LoopType::Midi }, category: String::new(), path: None, tags, groups: Vec::new(), pack: String::new(), user: false, bpm: Some(app.tempo()), key: None, minor: false, bars: None, seconds: None, location: None })
}

fn current_patch(app: &App) -> Option<String> {
    let t = app.selected_track?;
    app.instrument_choice.get(&t).cloned().or_else(|| app.project.as_ref()?.tracks.get(t).map(|tr| tr.name.clone()))
}

/// Every pack name (for phrases naming one).
fn pack_names(app: &App) -> Vec<String> {
    let mut v: Vec<String> = app.br.items.iter().map(|i| i.pack.clone()).collect();
    v.sort();
    v.dedup();
    v
}

// ---- actions

pub(crate) fn save_state(app: &App) {
    app.br.saved.save();
}

fn toggle_favorite(app: &mut App, id: &str) {
    if !app.br.saved.favorites.remove(id) {
        app.br.saved.favorites.insert(id.to_string());
    }
    if app.br.saved.favorites.is_empty() {
        app.br.search.favorites = false;
    }
    save_state(app);
}

/// Loads a patch onto a track (Logic's library, Bramble's effects or a user patch).
pub fn load_patch(app: &mut App, t: usize, it: &Item) {
    if it.pack == crate::plugbrowser::PACK {
        crate::plugbrowser::load(app, t, it);
        return;
    }
    if it.loop_type == LoopType::Audio && it.pack == "Bramble Effects" {
        crate::mixstate::checkpoint(app, "Change Patch");
        let name = it.name.clone();
        crate::mixstate::edit_inserts(app, t, |v| {
            v.clear();
            v.push(crate::mixstate::new_plugin(&name));
        });
        app.load_engine();
        return;
    }
    if let Some(p) = it.path.as_ref().filter(|_| it.pack == "Your Patches") {
        let p = p.clone();
        load_user_patch(app, t, &p);
        return;
    }
    let cat = patch_category(&it.name);
    app.choose_patch(t, &cat, &it.name);
}

/// A user patch file: "instrument\t<choice>", "program\t<n>", "volume\t<dB>", "pan\t<p>",
/// "insert\t<name>\t<floats,…>".
fn write_user_patch(app: &App, t: usize, path: &Path) -> std::io::Result<()> {
    let Some(tr) = app.project.as_ref().and_then(|p| p.tracks.get(t)) else { return Err(std::io::ErrorKind::NotFound.into()) };
    let mut s = String::from("# Bramble Studio patch\n");
    if let Some(i) = app.instrument_for(t).filter(|_| tr.kind != TrackKind::Audio) {
        s += &format!("instrument\t{i}\n");
    }
    s += &format!("program\t{}\nvolume\t{}\npan\t{}\n", app.programs.get(t).copied().unwrap_or(0), tr.volume_db, tr.pan);
    for u in crate::mixstate::inserts(app, t) {
        s += &format!("insert\t{}\t{}\n", u.name, u.params.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(","));
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, s)
}

fn load_user_patch(app: &mut App, t: usize, path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        app.tell("That patch couldn't be read");
        return;
    };
    app.checkpoint("Change Patch");
    let merge = app.br.saved.flag("patch_merging", false);
    let mut inserts = Vec::new();
    for l in text.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        match f.as_slice() {
            ["instrument", i] => {
                app.instrument_choice.insert(t, i.to_string());
            }
            ["program", n] => {
                if let (Some(p), Ok(n)) = (app.programs.get_mut(t), n.parse()) {
                    *p = n;
                }
            }
            ["volume", v] if !merge => {
                if let (Some(tr), Ok(v)) = (app.project.as_mut().and_then(|p| p.tracks.get_mut(t)), v.parse()) {
                    tr.volume_db = v;
                }
            }
            ["pan", v] if !merge => {
                if let (Some(tr), Ok(v)) = (app.project.as_mut().and_then(|p| p.tracks.get_mut(t)), v.parse()) {
                    tr.pan = v;
                }
            }
            ["insert", n, floats] => {
                let mut u = crate::mixstate::new_plugin(n);
                u.params = floats.split(',').filter_map(|x| x.parse().ok()).collect();
                inserts.push(u);
            }
            _ => {}
        }
    }
    if let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) {
        if tr.kind != TrackKind::Audio && tr.kind != TrackKind::Drummer {
            tr.name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
    }
    // Patch Merging off: the patch's Audio FX replace the track's; on: the track's stay
    if !merge {
        crate::mixstate::edit_inserts(app, t, |v| *v = inserts);
    } else if !inserts.is_empty() {
        crate::mixstate::edit_inserts(app, t, |v| {
            for u in inserts {
                if !v.iter().any(|x| x.name == u.name) {
                    v.push(u);
                }
            }
        });
    }
    app.after_edit();
}

/// A new track at the end: its kind, name; returns its index.
fn new_track(app: &mut App, name: &str, kind: TrackKind) -> Option<usize> {
    let p = app.project.as_mut()?;
    let mut tr = lg_core::Track::new(name, kind);
    tr.row = p.tracks.len() as u32 + 1;
    p.tracks.push(tr);
    app.programs.push(if kind == TrackKind::Audio { 0 } else { crate::app::program_for(name, kind) });
    Some(app.project.as_ref()?.tracks.len() - 1)
}

fn snap_tick(app: &App, tick: f64) -> u32 {
    let beat = (app.bar_ticks() / app.project.as_ref().map_or(4, |p| p.signature.0.max(1)) as u32).max(1) as f64;
    ((tick / beat).round() * beat).max(0.0) as u32
}

/// The semitones a loop in `key` moves to sound in the project's key (−5 … +6).
pub fn key_shift(loop_key: &str, project_key: &str) -> i32 {
    let pc = |k: &str| -> Option<i32> {
        let k = k.trim().replace('♭', "b");
        let mut c = k.chars();
        let base: i32 = match c.next()? {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let acc = match c.next() {
            Some('#') => 1,
            Some('b') => -1,
            _ => 0,
        };
        Some((base + acc).rem_euclid(12))
    };
    let (Some(a), Some(b)) = (pc(loop_key), pc(project_key)) else { return 0 };
    let d = (b - a).rem_euclid(12);
    if d > 6 { d - 12 } else { d }
}

/// The first loop added to a project with no regions sets the project's tempo and key.
fn first_loop_sets_project(app: &mut App, it: &Item) {
    let Some(p) = app.project.as_mut() else { return };
    if p.tracks.iter().any(|t| !t.regions.is_empty()) || it.kind != Kind::Loop {
        return;
    }
    // (Project Settings ▸ Smart Tempo ▸ Keep Project Tempo: the tempo stays)
    if let Some(b) = it.bpm.filter(|_| !crate::projsettings::keep_tempo(&app.proj)) {
        p.tempo = b;
        p.tempo_map.clear();
    }
    if let Some(k) = &it.key {
        p.key = format!("{} {}", k.replace('♭', "b"), if it.minor { "minor" } else { "major" });
    }
}

/// A loop's (or file's) region on track `t` at `at`. Audio loops conform to the project tempo
/// (Use Project Tempo: a flex stretch from the loop's tempo); MIDI loops move to the project key
/// (Use Project Key). Auto Leveling sets the region's gain.
fn place(app: &mut App, t: usize, d: &DragItem, at: u32) -> bool {
    let (path, it, seg) = match d {
        DragItem::Item(it) => match &it.path {
            Some(p) => (p.clone(), Some(it.clone()), None),
            None => return false,
        },
        DragItem::File(p, seg) => (p.clone(), None, *seg),
    };
    let name = it.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    let use_tempo = app.br.saved.flag("use_project_tempo", true);
    let use_key = app.br.saved.flag("use_project_key", true);
    let bar = app.bar_ticks();
    if let Some(it) = &it {
        first_loop_sets_project(app, it);
    }
    let project_key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_default();
    let tempo = app.tempo();
    if sl::is_midi(&path) {
        let Some(m) = std::fs::read(&path).ok().and_then(|d| lg_core::midi::read(&d)) else { return false };
        let shift = match (&it, use_key) {
            (Some(i), true) if i.kind == Kind::Loop => i.key.as_ref().map_or(0, |k| key_shift(k, project_key.split(' ').next().unwrap_or("C"))),
            _ => 0,
        };
        let mut notes: Vec<lg_core::events::Note> = m.tracks.into_iter().flat_map(|t| t.2).map(|mut n| {
            n.pos += at;
            n.pitch = (n.pitch as i32 + shift).clamp(0, 127) as u8;
            n
        }).collect();
        notes.sort_by_key(|n| n.pos);
        let end = notes.iter().map(|n| n.pos + n.len).max().unwrap_or(at + bar) - at;
        let bars = it.as_ref().and_then(|i| i.bars).unwrap_or_else(|| end.div_ceil(bar).max(1));
        let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return false };
        tr.regions.push(lg_core::Region { name, start: at, length: bars * bar, notes, audio: None, muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
        return true;
    }
    let info = crate::projaudio::file_info(&path);
    let file_secs = info.as_ref().map_or(0.0, |i| i.seconds);
    let (off, secs) = seg.unwrap_or((0.0, file_secs));
    if secs <= 0.0 {
        return false;
    }
    let loop_bpm = it.as_ref().filter(|i| i.kind == Kind::Loop).and_then(|i| i.bpm);
    let stretch = match loop_bpm {
        Some(b) if use_tempo && (b - tempo).abs() > 0.01 => Some(b / tempo),
        _ => None,
    };
    let played = secs * stretch.unwrap_or(1.0);
    let length = match (loop_bpm, it.as_ref().and_then(|i| i.bars)) {
        (Some(_), Some(bars)) if use_tempo => bars * bar,
        _ => ((played * tempo / 60.0 * lg_core::events::PPQ as f64) as u32).max(1),
    };
    let Some(tr) = app.project.as_mut().and_then(|p| p.tracks.get_mut(t)) else { return false };
    tr.regions.push(lg_core::Region { name, start: at, length, notes: Vec::new(), audio: Some(lg_core::project::AudioRef { file: path.clone(), seconds: file_secs }), muted: false, source: None, origin: None, props: Default::default(), ext: Default::default() });
    let r = tr.regions.len() - 1;
    let rate = info.as_ref().map_or(crate::audio::RATE, |i| i.rate) as f64;
    let mut x = crate::audioedit::RegionAudio { file: path.clone(), offset: (off * rate) as usize, ..Default::default() };
    if let Some(k) = stretch {
        // one flex marker at the loop's end: the loop plays at the project tempo
        let src = (secs * rate) as usize;
        x.flex = vec![(src, (src as f64 * k) as usize)];
        app.flex.tracks.insert(t, crate::flex::TrackFlex { on: true, mode: lg_audio::flex::Mode::Polyphonic });
    }
    // Auto Leveling: the region gain brings loops to one level (the file is left as it is)
    if app.br.saved.flag("auto_level", true) && it.is_some() {
        if let Some(g) = level_gain(&path) {
            x.gain_db = g;
        }
    }
    crate::audioedit::set_extras(app, t, r, x);
    true
}

/// Auto Leveling's gain (dB) for a file: its RMS to −18 dBFS, within ±12 dB.
pub fn level_gain(path: &Path) -> Option<f32> {
    let s = crate::audio::load_audio(path)?;
    if s.is_empty() {
        return None;
    }
    let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
    if rms <= 1e-6 {
        return None;
    }
    Some((-18.0 - 20.0 * rms.log10()).clamp(-12.0, 12.0))
}

/// A patch, loop or file onto track `t` (None: a new track below the others) at `at`.
fn drop_one(app: &mut App, t: Option<usize>, d: &DragItem, at: u32) -> Option<usize> {
    let (is_patch, is_audio, it) = match d {
        DragItem::Item(it) => (it.kind == Kind::Patch, it.loop_type == LoopType::Audio || it.path.as_ref().is_some_and(|p| sl::is_audio(p)), Some(it.clone())),
        DragItem::File(p, _) => (false, sl::is_audio(p), None),
    };
    if is_patch {
        let it = it?;
        let t = match t {
            Some(t) => t,
            None => new_track(app, &it.name, if it.loop_type == LoopType::Audio { TrackKind::Audio } else { TrackKind::Instrument })?,
        };
        load_patch(app, t, &it);
        return Some(t);
    }
    let name = it.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| match d {
        DragItem::File(p, _) => p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        _ => String::new(),
    });
    let kind_ok = |k: TrackKind| if is_audio { k == TrackKind::Audio } else { k != TrackKind::Audio };
    let target = t.filter(|t| app.project.as_ref().and_then(|p| p.tracks.get(*t)).is_some_and(|tr| kind_ok(tr.kind)));
    let t = match target {
        Some(t) => t,
        None => {
            if t.is_some() {
                // (a MIDI loop on an audio track would add its audio preview: not available)
                app.tell(if is_audio { "Audio loops and one-shots go on audio tracks" } else { "MIDI loops go on software instrument tracks (their audio previews aren't available)" });
                return None;
            }
            let t = new_track(app, &name, if is_audio { TrackKind::Audio } else { TrackKind::Instrument })?;
            if !is_audio {
                // the loop's instrument: a patch for its instrument tag
                if let Some(p) = it.as_ref().and_then(|it| patch_for_tags(&it.tags)) {
                    app.instrument_choice.insert(t, p.clone());
                    app.set_program(t, crate::app::program_for(&p, TrackKind::Instrument), &name);
                }
            }
            t
        }
    };
    if place(app, t, d, at) {
        app.selected_track = Some(t);
        Some(t)
    } else {
        None
    }
}

/// A Logic patch for a loop's instrument tags.
fn patch_for_tags(tags: &[String]) -> Option<String> {
    let has = |x: &str| tags.iter().any(|t| t == x);
    let cat = if has("Drums") || has("Percussion") {
        "Drums & Percussion"
    } else if has("Bass") {
        "Bass"
    } else if has("Guitar") {
        "Guitar"
    } else if has("Strings") {
        "Strings"
    } else if has("Brass") || has("Woodwinds") {
        "Brass & Woodwind"
    } else if has("Synthesizer") {
        "Synthesizer"
    } else {
        "Keyboard"
    };
    patches().iter().find(|p| p.0 == cat).map(|p| p.2.clone())
}

/// Items dropped: on a track row, or below the tracks (a new track each; several audio files
/// ask how: Add Selected Files to Tracks).
pub fn drop_items(app: &mut App, items: Vec<DragItem>, row: Option<usize>, at: u32) {
    if items.is_empty() {
        return;
    }
    let audio_files = items.iter().filter(|d| match d {
        DragItem::Item(i) => i.kind != Kind::Patch && i.loop_type == LoopType::Audio,
        DragItem::File(p, _) => sl::is_audio(p),
    }).count();
    if row.is_none() && items.len() > 1 && audio_files == items.len() {
        app.br.multi_drop = Some(MultiDrop { items, at, choice: 0 });
        return;
    }
    crate::audioedit::checkpoint(app, "Add Loop");
    let mut last = None;
    for d in &items {
        last = drop_one(app, row, d, at).or(last);
    }
    if let Some(t) = last {
        app.selected_track = Some(t);
    }
    app.after_edit();
}

/// Add Selected Files to Tracks: new tracks, existing tracks, or one track.
fn multi_drop_done(app: &mut App, m: MultiDrop) {
    crate::audioedit::checkpoint(app, "Add Loops");
    match m.choice {
        0 => {
            for d in &m.items {
                drop_one(app, None, d, m.at);
            }
        }
        1 => {
            let audio: Vec<usize> = app.project.as_ref().map(|p| (0..p.tracks.len()).filter(|&t| p.tracks[t].kind == TrackKind::Audio).collect()).unwrap_or_default();
            for (k, d) in m.items.iter().enumerate() {
                drop_one(app, audio.get(k).copied(), d, m.at);
            }
        }
        _ => {
            let mut t = None;
            let mut at = m.at;
            for d in &m.items {
                if let Some(tt) = drop_one(app, t, d, at) {
                    t = Some(tt);
                    if let Some(end) = app.project.as_ref().and_then(|p| p.tracks.get(tt)).and_then(|tr| tr.regions.last()).map(|r| r.start + r.length) {
                        at = end;
                    }
                }
            }
        }
    }
    app.after_edit();
}

/// New Track with Loop / with Selected Item: a track per item, at the project start.
fn new_tracks_with(app: &mut App, items: Vec<Item>) {
    if items.is_empty() {
        return;
    }
    crate::audioedit::checkpoint(app, "New Track with Loop");
    for it in items {
        drop_one(app, None, &DragItem::Item(it), 0);
    }
    app.after_edit();
}

/// Place Loop at Playhead: on the selected track.
fn place_at_playhead(app: &mut App, items: Vec<Item>) {
    let Some(t) = app.selected_track else {
        app.tell("Select a track first");
        return;
    };
    let at = snap_tick(app, app.position());
    crate::audioedit::checkpoint(app, "Place Loop at Playhead");
    for it in items {
        drop_one(app, Some(t), &DragItem::Item(it), at);
    }
    app.after_edit();
}

/// Create Track with Loop ▸ Quick Sampler / Sample Alchemy / Drum Machine Designer.
fn sampler_track(app: &mut App, plugin: &str, items: Vec<Item>) {
    let files: Vec<PathBuf> = items.iter().filter_map(|i| i.path.clone()).filter(|p| sl::is_audio(p)).collect();
    if files.is_empty() {
        app.tell("Choose audio loops or one-shots");
        return;
    }
    app.checkpoint("Create Track with Loop");
    let name = if files.len() == 1 { items[0].name.clone() } else { plugin.to_string() };
    let Some(t) = new_track(app, &name, TrackKind::Instrument) else { return };
    let list: Vec<(u8, PathBuf)> = files.iter().enumerate().map(|(k, f)| (36 + k as u8, f.clone())).collect();
    let choice = match plugin {
        "Drum Machine Designer" => sl::sampler_choice(plugin, &list),
        _ => sl::sampler_choice(plugin, &list[..1]),
    };
    app.instrument_choice.insert(t, choice);
    app.selected_track = Some(t);
    app.after_edit();
}

/// Add Loop/One-Shot to Drum Machine Designer: the next empty pad of the selected track's.
fn add_to_dmd(app: &mut App, items: Vec<Item>) {
    let Some(t) = app.selected_track else { return };
    let cur = app.instrument_choice.get(&t).cloned().unwrap_or_default();
    if sl::plugin_of(&cur) != "Drum Machine Designer" {
        app.tell("Select a Drum Machine Designer track (Create Track with Loop ▸ Drum Machine Designer)");
        return;
    }
    let mut pads = sl::pads_of(&cur);
    for it in items {
        if let Some(p) = it.path.filter(|p| sl::is_audio(p)) {
            let n = sl::next_pad(&pads);
            pads.push((n, p));
        }
    }
    app.checkpoint("Add to Drum Machine Designer");
    app.instrument_choice.insert(t, sl::sampler_choice("Drum Machine Designer", &pads));
    app.after_edit();
}

fn find_similar(app: &mut App, id: &str, name: &str) {
    app.br.search.similar = Some((id.to_string(), name.to_string()));
    if matches!(app.br.view, Cat::Presets | Cat::SoundLibrary | Cat::SoundFonts) {
        app.br.view = Cat::All;
    }
    app.br.all_filters = false;
}

/// A patch's category (Bass, Strings…), for its preview's sound.
pub(crate) fn patch_category(name: &str) -> String {
    patches().iter().find(|p| p.2 == name).map(|p| p.0.clone()).unwrap_or_default()
}

/// Preview: a loop (looped) or one-shot plays at the preview volume (Auto Leveling on:
/// levelled).
pub(crate) fn preview_item(app: &mut App, it: &Item, looped: bool) {
    let Some(p) = it.path.clone() else { return };
    let mut g = app.br.volume;
    if app.br.saved.flag("auto_level", true) && sl::is_audio(&p) {
        if let Some(db) = level_gain(&p) {
            g *= 10f32.powf(db / 20.0);
        }
    }
    crate::preview::toggle(app, &it.id, &p, g, None, looped);
}

/// Runs a Browser command (key commands, menus). `focused`: the Browser has the keys (Rename
/// and Delete are other areas' too).
pub fn run(app: &mut App, name: &str, focused: bool) -> bool {
    ensure_loaded(app);
    let sel = selected_items(app);
    match name {
        "Search in Browser" => {
            app.show_browser = true;
            app.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("browser-search")));
        }
        "Show Loops & One-Shots in Browser" => {
            app.show_browser = true;
            app.br.view = Cat::Loops;
        }
        "Show Instruments in Browser" => {
            app.show_browser = true;
            app.br.view = Cat::Instruments;
        }
        "Show Effects in Browser" => {
            app.show_browser = true;
            app.br.view = Cat::Effects;
        }
        "View Info" => {
            if let Some(it) = sel.first() {
                app.br.info = Some(it.id.clone());
            }
        }
        "Play/Stop Preview" => {
            if let Some(it) = sel.first() {
                crate::brpreview::click(app, it, crate::brpreview::Click::Icon { option: false });
            }
        }
        "Clear Applied Tags" => {
            app.br.search.clear();
            app.br.typed.clear();
        }
        "Toggle All Filters" => app.br.all_filters = !app.br.all_filters,
        "Save Patch for Selected Track…" => {
            if let Some(t) = app.selected_track.filter(|_| !track_kind(app).is_some_and(|k| k.2)) {
                let n = app.project.as_ref().and_then(|p| p.tracks.get(t)).map(|tr| tr.name.clone()).unwrap_or_default();
                app.br.patch_name = Some(n);
            }
        }
        "Enable Patch Merging" => {
            let v = !app.br.saved.flag("patch_merging", false);
            app.br.saved.set_flag("patch_merging", v);
            save_state(app);
        }
        "Add Location" => add_location_dialog(app),
        "Add Smart Search" => {
            if app.br.search.is_empty() {
                app.tell("Search first: a Smart Search keeps the current search");
            } else {
                app.br.smart_name = Some(format!("Smart Search {}", app.br.saved.smart.len() + 1));
            }
        }
        "New Track with Selected Item" | "New Track with Loop" => new_tracks_with(app, sel),
        "Replace Patch" => {
            let (Some(t), Some(it)) = (app.selected_track, sel.into_iter().find(|i| i.kind == Kind::Patch)) else { return true };
            load_patch(app, t, &it);
        }
        "Place Loop at Playhead" => place_at_playhead(app, sel),
        "Quick Sampler" | "Sample Alchemy" | "Drum Machine Designer" if focused => sampler_track(app, name, sel),
        "Create Track with Loop" => sampler_track(app, "Quick Sampler", sel),
        "Find Similar" => {
            if let Some(it) = sel.first() {
                find_similar(app, &it.id, &it.name);
            }
        }
        "Add to Favorites" => {
            for it in sel {
                if !app.br.saved.favorites.contains(&it.id) {
                    toggle_favorite(app, &it.id);
                }
            }
        }
        "Rename" if focused => {
            if let Some(it) = sel.first().filter(|i| i.user) {
                app.br.renaming = Some((it.id.clone(), it.name.clone()));
            }
        }
        "Delete" if focused => {
            if let Some(it) = sel.first().filter(|i| i.user) {
                app.br.deleting = Some(it.id.clone());
            }
        }
        "Show in Finder" => {
            if let Some(p) = sel.first().filter(|i| i.user).and_then(|i| i.path.clone()) {
                crate::projaudio::reveal(&p);
            }
        }
        "Toggle Columns View" => {
            let v = !app.br.saved.flag("columns", false);
            app.br.saved.set_flag("columns", v);
        }
        "Add to Browser…" => add_to_browser_dialog(app),
        _ => return false,
    }
    true
}

/// The key_command hook: the Browser commands no other area has.
pub fn command(app: &mut App, name: &str) -> bool {
    if matches!(name, "Rename" | "Delete" | "Quick Sampler" | "Sample Alchemy" | "Drum Machine Designer") || !COMMANDS.contains(&name) {
        return false;
    }
    run(app, name, false)
}

/// Keys while the Browser has the key focus (clicked last: focus.rs): the Browser group, ↑ / ↓
/// through the results.
pub fn key_press(app: &mut App, key: egui::Key, m: egui::Modifiers) -> bool {
    let over = crate::focus::area(app) == crate::focus::Area::Browser;
    if !over || !app.show_browser {
        return false;
    }
    // the columns view: the arrows move through the folders
    let columns = app.br.saved.flag("columns", false) && matches!(app.br.view, Cat::Instruments | Cat::Effects) && app.br.search.is_empty() && !app.br.all_filters;
    if columns && matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::ArrowLeft | egui::Key::ArrowRight) && !m.command && !m.alt {
        let items = results(app);
        let cols = column_lists(&items, &app.br.column_path);
        let chosen = cols.get(2).and_then(|c| {
            let sel = app.br.selected.first()?;
            let it = items.iter().find(|i| &i.id == sel)?;
            c.iter().position(|n| *n == it.name)
        });
        let (path, pick) = column_key(&cols, &app.br.column_path, chosen, key);
        app.br.column_path = path;
        match pick {
            Some(k) if pick != chosen => {
                let cols = column_lists(&items, &app.br.column_path);
                if let Some(it) = cols.get(2).and_then(|c| c.get(k)).and_then(|n| items.iter().find(|i| &i.name == n)).cloned() {
                    app.br.selected = vec![it.id.clone()];
                    click_item(app, &it, false);
                }
            }
            None => app.br.selected.clear(),
            _ => {}
        }
        return true;
    }
    if matches!(key, egui::Key::ArrowUp | egui::Key::ArrowDown) && !m.command && !m.alt {
        let r = results(app);
        if r.is_empty() {
            return true;
        }
        let cur = app.br.selected.first().and_then(|s| r.iter().position(|i| &i.id == s));
        let i = match (cur, key == egui::Key::ArrowDown) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(r.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        };
        let it = r[i].clone();
        let was = app.br.selected.first() == Some(&it.id);
        app.br.selected = vec![it.id.clone()];
        if !was {
            click_item(app, &it, false);
        }
        return true;
    }
    // Right Arrow (list layout): starts or retriggers the selected sound's preview
    if key == egui::Key::ArrowRight && !m.command && !m.alt && !app.br.saved.flag("columns", false) {
        let r = results(app);
        if let Some(it) = app.br.selected.first().and_then(|s| r.iter().find(|i| &i.id == s)).cloned() {
            crate::brpreview::click(app, &it, crate::brpreview::Click::RightArrow);
            return true;
        }
    }
    for b in crate::keymap::bindings().iter().filter(|b| b.area == "Browser") {
        if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift {
            let c = b.command.clone();
            return run(app, &c, true);
        }
    }
    false
}

/// A click on a result: patches load (Replace Mode); the preview as Logic's rules say
/// (brpreview.rs: the selected sound toggles, Play Preview on Selection on the first click).
fn click_item(app: &mut App, it: &Item, was_selected: bool) {
    if it.kind == Kind::Patch && app.browser_replace && !was_selected {
        match app.selected_track {
            Some(t) => load_patch(app, t, it),
            None => app.tell("Select a track first"),
        }
    }
    // (as Logic: the first click on an instrument patch only selects it, loading it in Replace
    // Mode; clicking the highlighted patch again plays its preview. Loops and one-shots keep
    // Play Preview on Selection.)
    if it.kind == Kind::Patch && !was_selected {
        return;
    }
    crate::brpreview::click(app, it, crate::brpreview::Click::Row { was_selected });
}

fn add_location_dialog(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(dir) = kn_ui::files::FileDialog::new().set_title("Add Location").pick_folder() else { return };
    add_location(app, dir);
}

/// Adds a folder as a location: its loops and one-shots are analysed and marked as yours.
pub fn add_location(app: &mut App, dir: PathBuf) {
    ensure_loaded(app);
    if app.br.saved.locations.iter().any(|l| l.path == dir) {
        return;
    }
    let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| dir.display().to_string());
    let i = app.br.saved.locations.len();
    let items = sl::location_items(&dir, &name, i);
    let n = items.len();
    app.br.saved.locations.push(sl::Location { name, path: dir });
    app.br.items.extend(items);
    app.br.view = Cat::Location(i);
    app.br.all_filters = false;
    save_state(app);
    app.tell(format!("Added the location ({n} sound{})", if n == 1 { "" } else { "s" }));
}

fn remove_location(app: &mut App, i: usize) {
    if i >= app.br.saved.locations.len() {
        return;
    }
    app.br.saved.locations.remove(i);
    save_state(app);
    if matches!(app.br.view, Cat::Location(_)) {
        app.br.view = Cat::All;
    }
    reload(app);
}

// ---- Add to Browser (regions as loops / one-shots)

fn add_to_browser_dialog(app: &mut App) {
    let regions: Vec<(usize, usize)> = if app.selected_regions.is_empty() { app.selected_region.into_iter().collect() } else { app.selected_regions.clone() };
    let Some(&(t, r)) = regions.first() else {
        app.tell("Select a region first");
        return;
    };
    let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)) else { return };
    let name = rg.name.clone();
    let beat = (app.bar_ticks() / app.project.as_ref().map_or(4, |p| p.signature.0.max(1)) as u32).max(1);
    let can_loop = regions.iter().all(|&(t, r)| app.project.as_ref().and_then(|p| p.tracks.get(t)?.regions.get(r)).is_some_and(|rg| rg.length % beat == 0));
    let key = app.project.as_ref().map(|p| p.key.clone()).unwrap_or_else(|| "C major".into());
    let (root, minor) = (key.split(' ').next().unwrap_or("C").to_string(), key.contains("minor"));
    app.br.add_to_browser = Some(AddToBrowser { regions, name, one_shot: !can_loop, key: root, minor, can_loop });
}

fn add_to_browser_done(app: &mut App, a: AddToBrowser) {
    let dir = sl::user_dir("Loops");
    let _ = std::fs::create_dir_all(&dir);
    let tempo = app.tempo();
    let bar = app.bar_ticks();
    let mut n = 0;
    let many = a.regions.len() > 1;
    for (k, &(t, r)) in a.regions.iter().enumerate() {
        let Some(rg) = app.project.as_ref().and_then(|p| p.tracks.get(t)).and_then(|tr| tr.regions.get(r)).cloned() else { continue };
        let name = if many { format!("{} {}", a.name, k + 1) } else { a.name.clone() };
        let safe = name.replace('/', "-");
        let tags = sl::infer_tags(&format!("{} {} {}", app.project.as_ref().map(|p| p.tracks[t].name.clone()).unwrap_or_default(), rg.name, app.instrument_for(t).unwrap_or_default()));
        let path = if let Some(au) = &rg.audio {
            // the region's part of its file, as a WAV
            let Ok(src) = lg_synth::wav::read_audio(&au.file) else { continue };
            let off = app.audio_edit.regions.get(&(t, r)).map_or(0, |x| x.offset);
            let len_s = app.seconds_at((rg.start + rg.length) as f64) - app.seconds_at(rg.start as f64);
            let frames = (len_s * src.rate as f64) as usize;
            let chans: Vec<Vec<f32>> = src.channels.iter().map(|c| c[off.min(c.len())..(off + frames).min(c.len())].to_vec()).collect();
            let p = dir.join(format!("{safe}.wav"));
            if crate::projaudio::write_wav(&p, &chans, src.rate, src.rate, 24).is_err() {
                continue;
            }
            p
        } else {
            let mut proj = lg_core::Project { tempo, signature: app.project.as_ref().map_or((4, 4), |p| p.signature), ..Default::default() };
            let mut tr = lg_core::Track::new(&name, TrackKind::Instrument);
            let mut rr = rg.clone();
            for nn in rr.notes.iter_mut() {
                nn.pos = nn.pos.saturating_sub(rg.start);
            }
            rr.start = 0;
            tr.regions.push(rr);
            proj.tracks.push(tr);
            let p = dir.join(format!("{safe}.mid"));
            if std::fs::write(&p, lg_core::midi::write(&proj)).is_err() {
                continue;
            }
            p
        };
        let bars = rg.length.div_ceil(bar).max(1);
        let side = if a.one_shot {
            format!("type=one-shot\ntags={}\n", tags.join(","))
        } else {
            format!("type=loop\nbpm={tempo}\nkey={}\nminor={}\nbars={bars}\ntags={}\n", a.key, a.minor as u8, tags.join(","))
        };
        let _ = std::fs::write(sidecar(&path), side);
        n += 1;
    }
    reload(app);
    app.tell(format!("Added {n} item{} to the Browser (Your Loops)", if n == 1 { "" } else { "s" }));
}

// ---- drag and drop

/// Starts dragging a file (Project Audio, All Files) towards the tracks.
pub fn start_file_drag(app: &mut App, f: PathBuf, seg: Option<(f64, f64)>) {
    app.br.drag = Some(vec![DragItem::File(f, seg)]);
}

/// Each frame (after the panes): the drag's label at the pointer, and the drop.
pub fn frame(app: &mut App, ctx: &egui::Context) {
    let Some(items) = app.br.drag.clone() else { return };
    let (pos, released) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.any_released() || !i.pointer.any_down()));
    let label = match items.as_slice() {
        [DragItem::Item(i)] => i.name.clone(),
        [DragItem::File(f, _)] => f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        v => format!("{} items", v.len()),
    };
    if let Some(at) = pos {
        egui::Area::new(egui::Id::unique("browser-drag")).order(egui::Order::Tooltip).fixed_pos(at + vec2(12.0, 10.0)).interactable(false).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(30, 30, 32, 225)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.label(egui::RichText::new(label).font(look::regular(12.0)).color(Color32::WHITE));
            });
        });
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if !released {
        return;
    }
    app.br.drag = None;
    let Some(at) = pos else { return };
    // onto the search field: find similar sounds
    if app.br.search_rect.contains(at) {
        if let Some(DragItem::Item(i)) = items.first() {
            let (id, n) = (i.id.clone(), i.name.clone());
            find_similar(app, &id, &n);
        }
        return;
    }
    let lanes = app.br.lanes;
    if !lanes.contains(at) {
        return;
    }
    let rows = crate::trackarea::rows(app);
    let row = crate::trackarea::row_at(&rows, at.y - lanes.min.y + app.scroll_y).map(|r| r.t);
    let tick = ((at.x - lanes.min.x) / app.bar_w + app.scroll_x) * app.bar_ticks() as f32;
    let tick = snap_tick(app, tick.max(0.0) as f64);
    drop_items(app, items, row, tick);
}

/// The Tracks area's lanes (tracks.rs tells each frame).
pub fn note_lanes(app: &mut App, lanes: Rect) {
    app.br.lanes = lanes;
}

// ---- drawing

fn icon_button(ui: &mut egui::Ui, r: Rect, id: &str, icon: &str, on: bool, tip: &str) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(("bb", id)), Sense::click()).on_hover_text(tip);
    let p = ui.painter();
    p.rect_filled(r, 6.0, if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(p, r, icon, 16.0, if on { Color32::WHITE } else { look::text() });
    resp
}

pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    crate::brfocus::follow_view(app);
    ensure_loaded(app);
    app.br.rect = area;
    link_to_selection(app);
    let p = ui.painter_at(area);
    p.rect_filled(area, 0.0, look::pane_bg());
    p.vline(area.max.x, area.y_range(), Stroke::new(1.0, look::line()));
    // ---- the header: "Browser", Link to Selection, Replace Mode
    // (Logic's, measured at 2x: a 41 pt header, the title centred on the panel, two 33 × 24 pt
    // buttons 9 pt apart, 11 pt from the right edge; a narrow panel centres the title in what
    // the buttons leave, and drops it when there's no room)
    let head = Rect::from_min_size(area.min, vec2(area.width(), 41.0));
    let rb = Rect::from_center_size(pos2(head.max.x - 11.0 - 16.5, head.center().y), vec2(33.0, 24.0));
    let lb = rb.translate(vec2(-42.0, 0.0));
    let title_w = 56.0;
    let tx = if head.center().x + title_w / 2.0 + 8.0 < lb.min.x { head.center().x } else { (head.min.x + lb.min.x) / 2.0 };
    if tx - title_w / 2.0 > head.min.x + 4.0 {
        p.text(pos2(tx, head.center().y), Align2::CENTER_CENTER, "Browser", look::semibold(13.0), look::text());
    }
    if icon_button(ui, lb, "link", ph::LINK, app.browser_link, "Link to Selection: the Browser follows the selected track").clicked() {
        app.browser_link = !app.browser_link;
        app.br.linked = None;
    }
    if icon_button(ui, rb, "replace", ph::SWAP, app.browser_replace, "Replace Mode: choosing a patch replaces the selected track's").clicked() {
        app.browser_replace = !app.browser_replace;
    }
    p.hline(head.x_range(), head.max.y, Stroke::new(1.0, look::line()));
    // ---- search, All Filters
    // (Logic's: 24 pt tall, 9 pt under the header, the filter button 33 × 24 pt at the right)
    let sr = Rect::from_min_size(pos2(area.min.x + 10.5, head.max.y + 9.0), vec2(area.width() - 10.5 - 11.0 - 33.0 - 9.5, 24.0));
    app.br.search_rect = sr;
    p.rect_filled(sr, 7.0, look::group_bg());
    crate::glyphs::paint(&p, Rect::from_min_size(sr.min + vec2(4.0, 4.0), vec2(16.0, 16.0)), ph::MAGNIFYING_GLASS, 13.0, look::text_dim());
    let field = Rect::from_min_max(pos2(sr.min.x + 24.0, sr.min.y + 3.0), pos2(sr.max.x - 6.0, sr.max.y - 3.0));
    let te = ui.put(field, egui::TextEdit::singleline(&mut app.br.typed).id(egui::Id::unique("browser-search")).hint_text(if app.br.view == Cat::Presets { "Search Presets" } else { "Search" }).frame(egui::Frame::NONE).font(look::regular(13.0)));
    let entered = te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let typed = app.br.typed.trim().to_string();
    if entered && !typed.is_empty() && app.br.view != Cat::Presets {
        // Return: Sounds like (the description understood)
        sounds_like(app, &typed);
        app.br.typed.clear();
    }
    if te.has_focus() && !typed.is_empty() && app.br.view != Cat::Presets && app.br.view != Cat::SoundFonts {
        suggestions(app, ui, &te, &typed);
    }
    let fb = Rect::from_min_size(pos2(sr.max.x + 9.5, sr.min.y), vec2(33.0, 24.0));
    let active = !app.br.search.is_empty();
    let fon = app.br.all_filters || active;
    if icon_button(ui, fb, "filters", "", fon, "All Filters: every tag for building a search").clicked() {
        app.br.all_filters = !app.br.all_filters;
    }
    // (Logic 13's filter symbol: three lines, each shorter than the one above)
    for (i, w) in [9.0f32, 6.5, 3.5].into_iter().enumerate() {
        let y = fb.center().y + (i as f32 - 1.0) * 2.9;
        let c = if fon { Color32::WHITE } else { look::text() };
        p.line_segment([pos2(fb.center().x - w / 2.0, y), pos2(fb.center().x + w / 2.0, y)], Stroke::new(1.3, c));
    }
    let mut top = sr.max.y + 6.0;
    // ---- the chips (the search's criteria)
    let chips = app.br.search.chips();
    if !chips.is_empty() {
        let cr = Rect::from_min_size(pos2(area.min.x + 10.0, top), vec2(area.width() - 20.0, 22.0));
        let mut x = cr.min.x;
        let mut remove = None;
        let mut y = cr.min.y;
        for (k, c) in chips.iter().enumerate() {
            let label = c.label();
            let w = p.layout_no_wrap(label.clone(), look::regular(11.5), look::text()).size().x + 24.0;
            if x + w > cr.max.x - 24.0 && x > cr.min.x {
                x = cr.min.x;
                y += 24.0;
            }
            let r = Rect::from_min_size(pos2(x, y), vec2(w, 20.0));
            let resp = ui.interact(r, ui.scope_id().with(("chip", k)), Sense::click()).on_hover_text("Click to remove this filter");
            p.rect_filled(r, 10.0, if resp.hovered() { look::accent() } else { look::accent().gamma_multiply(0.75) });
            p.text(pos2(r.min.x + 9.0, r.center().y), Align2::LEFT_CENTER, &label, look::regular(11.5), Color32::WHITE);
            crate::glyphs::paint(&p, Rect::from_min_size(pos2(r.max.x - 15.0, r.min.y + 3.0), vec2(12.0, 14.0)), ph::X, 9.0, Color32::WHITE);
            if resp.clicked() {
                remove = Some(c.clone());
            }
            x += w + 4.0;
        }
        let clear = Rect::from_min_size(pos2(cr.max.x - 20.0, cr.min.y), vec2(20.0, 20.0));
        let cresp = ui.interact(clear, ui.scope_id().with("chips-clear"), Sense::click()).on_hover_text("Clear all filters");
        crate::glyphs::paint(&p, clear, ph_fill::X_CIRCLE, 15.0, if cresp.hovered() { look::text() } else { look::text_dim() });
        if cresp.clicked() {
            app.br.search.clear();
            app.br.typed.clear();
        }
        if let Some(c) = remove {
            app.br.search.remove(&c);
        }
        top = y + 26.0;
    }
    // ---- the navigation (sidebar, or compact: an icon row)
    let compact = app.br.saved.flag("compact", false);
    // (Logic's sidebar is labelled while its labels fit beside a usable results column, about
    // 200 pt; narrower, it collapses to icons. RESIZING.txt)
    let label_w = ui.fonts_mut(|f| f.layout_no_wrap("Loops & One-Shots".into(), look::regular(13.0), look::text()).size().x) + 30.0 + 18.0;
    let icons_only = area.width() - label_w.max(120.0) < 220.0;
    let (body_left, body_top) = if compact {
        let nr = Rect::from_min_size(pos2(area.min.x + 6.0, top), vec2(area.width() - 12.0, 28.0));
        compact_nav(app, ui, nr);
        (area.min.x + 1.0, nr.max.y + 4.0)
    } else {
        let side_w = if icons_only { 44.0 } else { (area.width() * 0.39).clamp(label_w.max(120.0), 200.0f32.max(label_w)) };
        let side = Rect::from_min_max(pos2(area.min.x, top), pos2(area.min.x + side_w, area.max.y - 40.0));
        sidebar(app, ui, side, icons_only);
        (side.max.x + 2.0, top)
    };
    // + at the bottom left: Add Location, Add Smart Search, Add SoundFont
    let plus = Rect::from_min_size(pos2(area.min.x + 10.0, area.max.y - 34.0), vec2(26.0, 24.0));
    let presp = ui.interact(plus, ui.scope_id().with("bplus"), Sense::click()).on_hover_text("Add");
    p.rect_filled(plus, 6.0, if presp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(&p, plus, ph::PLUS, 13.0, look::text());
    egui::Popup::menu(&presp).show(|ui| {
        if ui.button("Add Location…").clicked() {
            ui.close();
            run(app, "Add Location", true);
        }
        if ui.add_enabled(!app.br.search.is_empty(), egui::Button::new("Add Smart Search…")).clicked() {
            ui.close();
            run(app, "Add Smart Search", true);
        }
        ui.separator();
        if ui.button("Add SoundFont…").clicked() {
            ui.close();
            add_soundfont(app);
        }
    });
    // ---- the results and the footer
    // (the Preview Player, when shown, sits between the results and the footer: prevplayer.rs)
    let player_h = crate::prevplayer::height(app);
    let list = Rect::from_min_max(pos2(body_left, body_top), pos2(area.max.x - 1.0, area.max.y - 42.0 - player_h));
    p.rect_filled(list, 0.0, look::lane_bg());
    let count = {
        match app.br.view {
            Cat::SoundFonts => soundfont_list(app, ui, list),
            Cat::Presets => presets_view(app, ui, list),
            Cat::SoundLibrary => sound_library(app, ui, list),
            _ => {
                let mut l = list;
                if app.br.view == Cat::Loops {
                    l.min.y = loops_buttons(app, ui, list) + 2.0;
                }
                if app.br.saved.flag("columns", false) && matches!(app.br.view, Cat::Instruments | Cat::Effects) && app.br.search.is_empty() {
                    columns_view(app, ui, l)
                } else {
                    result_list(app, ui, l)
                }
            }
        }
    };
    if player_h > 0.0 {
        crate::prevplayer::show(app, ui, Rect::from_min_max(pos2(list.min.x, list.max.y), pos2(list.max.x, list.max.y + player_h)));
    }
    footer(app, ui, Rect::from_min_max(pos2(list.min.x, list.max.y + player_h), pos2(list.max.x, area.max.y)), count);
    // folders dropped from the desktop onto the Browser: new locations
    let dropped: Vec<PathBuf> = ui.ctx().input(|i| if i.pointer.hover_pos().is_some_and(|p| area.contains(p)) { i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| p.is_dir()).collect() } else { Vec::new() });
    for d in dropped {
        add_location(app, d);
    }
    // the All Filters window, floating beside the filter button
    if app.br.all_filters {
        all_filters(app, ui, fb);
    } else {
        crate::allfilters::closed(ui.ctx());
    }
    dialogs(app, ui.ctx());
}

/// Link to Selection: the view follows the selected track's type.
fn link_to_selection(app: &mut App) {
    if !app.browser_link {
        return;
    }
    let Some((t, kind, _)) = track_kind(app) else { return };
    if app.br.linked == Some((t, kind)) {
        return;
    }
    let changed_kind = app.br.linked.is_none_or(|(_, k)| (k == TrackKind::Audio) != (kind == TrackKind::Audio));
    app.br.linked = Some((t, kind));
    app.br.link_anchor = current_patch(app).map(|n| (t, n));
    if changed_kind && matches!(app.br.view, Cat::Instruments | Cat::Effects) {
        app.br.view = if kind == TrackKind::Audio { Cat::Effects } else { Cat::Instruments };
    }
}

fn sounds_like(app: &mut App, typed: &str) {
    let packs = pack_names(app);
    let mut s = sl::parse_phrase(typed, &packs);
    s.loops_only = app.br.search.loops_only;
    if let Some(k) = s.kind {
        match k {
            Kind::Patch => {
                if !matches!(app.br.view, Cat::Instruments | Cat::Effects) {
                    app.br.view = Cat::Instruments;
                }
            }
            Kind::Loop | Kind::OneShot => {
                app.br.view = Cat::Loops;
                s.loops_only = Some(k == Kind::Loop);
            }
        }
        s.kind = None;
    }
    app.br.search = s;
    app.br.all_filters = false;
}

/// The list under the search field while typing: the modes, then matching tags.
fn suggestions(app: &mut App, ui: &mut egui::Ui, te: &egui::Response, typed: &str) {
    let tags: Vec<String> = {
        let mut counts = sl::tag_counts(app.br.items.iter());
        counts.retain(|k, _| !k.contains('\u{1f}'));
        let mut v: Vec<String> = counts.into_keys().collect();
        v.sort();
        v
    };
    let sug = sl::suggest(typed, &tags);
    let at = te.rect.left_bottom() + vec2(-24.0, 6.0);
    let mut pick: Option<usize> = None;
    egui::Area::new(egui::Id::unique("browser-suggest")).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(te.rect.width() + 20.0);
            if ui.add(egui::Button::new(format!("Name Contains “{typed}”")).frame(false)).clicked() {
                pick = Some(0);
            }
            if ui.add(egui::Button::new(format!("Sounds like “{typed}”")).frame(false)).on_hover_text("Return").clicked() {
                pick = Some(1);
            }
            if !sug.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new("Tags").color(look::text_dim()).font(look::regular(11.0)));
            }
            for (k, t) in sug.iter().enumerate() {
                if ui.add(egui::Button::new(t).frame(false)).clicked() {
                    pick = Some(2 + k);
                }
            }
        });
    });
    match pick {
        Some(0) => {
            if !app.br.search.names.iter().any(|n| n == typed) {
                app.br.search.names.push(typed.to_string());
            }
            app.br.typed.clear();
        }
        Some(1) => {
            sounds_like(app, typed);
            app.br.typed.clear();
        }
        Some(k) => {
            if let Some(t) = sug.get(k - 2) {
                app.br.search.add_tag(t);
            }
            app.br.typed.clear();
        }
        None => {}
    }
}

fn nav_label(app: &App, c: Cat) -> String {
    match c {
        Cat::Location(i) => app.br.saved.locations.get(i).map(|l| l.name.clone()).unwrap_or_default(),
        Cat::Smart(i) => app.br.saved.smart.get(i).map(|s| s.0.clone()).unwrap_or_default(),
        Cat::YourPatches => "Your Patches".into(),
        Cat::YourLoops => "Your Loops".into(),
        _ => NAV.iter().find(|n| n.0 == c).map(|n| n.1.to_string()).unwrap_or_default(),
    }
}

fn nav_row(app: &mut App, ui: &mut egui::Ui, r: Rect, c: Cat, icon: &str, icons_only: bool) -> egui::Response {
    let p = ui.painter().clone();
    let resp = ui.interact(r, ui.scope_id().with(("nav", format!("{c:?}"))), Sense::click());
    if app.br.view == c && !app.br.all_filters {
        p.rect_filled(r, 6.0, look::group_bg());
    } else if resp.hovered() {
        p.rect_filled(r, 6.0, look::group_bg().gamma_multiply(0.5));
    }
    crate::glyphs::paint(&p, Rect::from_center_size(pos2(r.min.x + 14.0, r.center().y), vec2(16.0, 16.0)), icon, 14.0, look::text());
    let label = nav_label(app, c);
    if !icons_only {
        p.with_clip_rect(r).text(pos2(r.min.x + 30.0, r.center().y), Align2::LEFT_CENTER, &label, look::regular(13.0), look::text());
    }
    let resp = resp.on_hover_text(label);
    if resp.clicked() {
        app.br.view = c;
        app.br.all_filters = false;
    }
    resp
}

/// Has the user saved patches / loops (the auto-added locations)?
fn has_pack(app: &App, pack: &str) -> bool {
    app.br.items.iter().any(|i| i.pack == pack)
}

fn sidebar(app: &mut App, ui: &mut egui::Ui, side: Rect, icons_only: bool) {
    let p = ui.painter().clone();
    let mut y = side.min.y + 4.0;
    let row_h = 26.0;
    for (c, _, icon) in NAV {
        if matches!(c, Cat::Presets | Cat::SoundLibrary) {
            y += 4.0;
            p.hline(side.min.x + 10.0..=side.max.x - 10.0, y, Stroke::new(1.0, look::line()));
            y += 4.0;
        }
        let r = Rect::from_min_size(pos2(side.min.x + 8.0, y), vec2(side.width() - 16.0, row_h));
        nav_row(app, ui, r, c, icon, icons_only);
        y += row_h + 2.0;
    }
    // Locations (shown once there is one): the user's folders, Your Patches, Your Loops
    let mut locs: Vec<Cat> = (0..app.br.saved.locations.len()).map(Cat::Location).collect();
    if has_pack(app, "Your Patches") {
        locs.push(Cat::YourPatches);
    }
    if has_pack(app, "Your Loops") {
        locs.push(Cat::YourLoops);
    }
    if !locs.is_empty() && !icons_only {
        y += 8.0;
        let hr = Rect::from_min_size(pos2(side.min.x + 10.0, y), vec2(side.width() - 20.0, 18.0));
        let add = Rect::from_min_size(pos2(hr.max.x - 16.0, hr.min.y + 1.0), vec2(16.0, 16.0));
        let hresp = ui.interact(Rect::from_min_max(hr.min, pos2(add.min.x - 2.0, hr.max.y)), ui.scope_id().with("loc-head"), Sense::click());
        p.text(hr.left_center(), Align2::LEFT_CENTER, format!("{} Locations", if app.br.locations_open { "▾" } else { "▸" }), look::semibold(11.0), look::text_dim());
        // (hovering the label shows Add Folder)
        if ui.rect_contains_pointer(hr) {
            crate::glyphs::paint(&p, add, ph::FOLDER_PLUS, 13.0, look::text());
            if ui.interact(add, ui.scope_id().with("loc-add"), Sense::click()).on_hover_text("Add Location").clicked() {
                run(app, "Add Location", true);
            }
        }
        if hresp.clicked() {
            app.br.locations_open = !app.br.locations_open;
        }
        y += 20.0;
        if app.br.locations_open {
            for c in locs {
                if y > side.max.y - row_h {
                    break;
                }
                let r = Rect::from_min_size(pos2(side.min.x + 8.0, y), vec2(side.width() - 16.0, row_h));
                let resp = nav_row(app, ui, r, c, ph::FOLDER, false);
                if let Cat::Location(i) = c {
                    resp.context_menu(|ui| location_menu(app, ui, i));
                }
                y += row_h + 2.0;
            }
        }
    }
    // Smart Searches (shown once one is saved)
    if !app.br.saved.smart.is_empty() && !icons_only {
        y += 8.0;
        let hr = Rect::from_min_size(pos2(side.min.x + 10.0, y), vec2(side.width() - 20.0, 18.0));
        p.text(hr.left_center(), Align2::LEFT_CENTER, format!("{} Smart Searches", if app.br.smart_open { "▾" } else { "▸" }), look::semibold(11.0), look::text_dim());
        if ui.interact(hr, ui.scope_id().with("smart-head"), Sense::click()).clicked() {
            app.br.smart_open = !app.br.smart_open;
        }
        y += 20.0;
        if app.br.smart_open {
            for i in 0..app.br.saved.smart.len() {
                if y > side.max.y - row_h {
                    break;
                }
                let r = Rect::from_min_size(pos2(side.min.x + 8.0, y), vec2(side.width() - 16.0, row_h));
                let resp = nav_row(app, ui, r, Cat::Smart(i), ph::GEAR, false);
                if resp.clicked() {
                    // the saved search shows its criteria as chips
                    app.br.search = app.br.saved.smart[i].1.clone();
                }
                resp.context_menu(|ui| smart_menu(app, ui, i));
                y += row_h + 2.0;
            }
        }
    }
}

fn location_menu(app: &mut App, ui: &mut egui::Ui, i: usize) {
    if ui.button("Show in Finder").clicked() {
        if let Some(l) = app.br.saved.locations.get(i) {
            crate::projaudio::reveal(&l.path);
        }
        ui.close();
    }
    if ui.button("Rename").clicked() {
        app.br.location_rename = Some((i, nav_label(app, Cat::Location(i))));
        ui.close();
    }
    if ui.button("Remove").on_hover_text("Removes it from the Browser; the files stay").clicked() {
        remove_location(app, i);
        ui.close();
    }
}

fn smart_menu(app: &mut App, ui: &mut egui::Ui, i: usize) {
    if ui.button("Save").on_hover_text("Keep the current search in this Smart Search").clicked() {
        let s = app.br.search.clone();
        if let Some(x) = app.br.saved.smart.get_mut(i) {
            x.1 = s;
        }
        save_state(app);
        ui.close();
    }
    if ui.button("Rename").clicked() {
        app.br.smart_rename = Some((i, app.br.saved.smart[i].0.clone()));
        ui.close();
    }
    if ui.button("Delete").clicked() {
        app.br.saved.smart.remove(i);
        app.br.view = Cat::All;
        save_state(app);
        ui.close();
    }
}

/// Compact navigation: one icon row (only the selected view's label); folder and gear icons
/// open the Locations and Smart Searches menus.
fn compact_nav(app: &mut App, ui: &mut egui::Ui, r: Rect) {
    let p = ui.painter().clone();
    let mut x = r.min.x;
    for (c, name, icon) in NAV {
        let sel = app.br.view == c;
        let w = if sel { p.layout_no_wrap(name.to_string(), look::regular(12.0), look::text()).size().x + 34.0 } else { 28.0 };
        let b = Rect::from_min_size(pos2(x, r.min.y), vec2(w, r.height()));
        let resp = ui.interact(b, ui.scope_id().with(("cnav", name)), Sense::click()).on_hover_text(name);
        p.rect_filled(b, 6.0, if sel { look::group_bg() } else if resp.hovered() { look::group_bg().gamma_multiply(0.5) } else { Color32::TRANSPARENT });
        crate::glyphs::paint(&p, Rect::from_min_size(b.min + vec2(6.0, 6.0), vec2(16.0, 16.0)), icon, 14.0, look::text());
        if sel {
            p.text(pos2(b.min.x + 28.0, b.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
        }
        if resp.clicked() {
            app.br.view = c;
            app.br.all_filters = false;
        }
        x += w + 2.0;
    }
    let lb = Rect::from_min_size(pos2(x + 6.0, r.min.y), vec2(28.0, r.height()));
    let lresp = ui.interact(lb, ui.scope_id().with("cnav-loc"), Sense::click()).on_hover_text("Locations");
    crate::glyphs::paint(&p, lb, ph::FOLDER, 14.0, if matches!(app.br.view, Cat::Location(_) | Cat::YourPatches | Cat::YourLoops) { look::accent() } else { look::text() });
    egui::Popup::menu(&lresp).show(|ui| {
        for i in 0..app.br.saved.locations.len() {
            if ui.add(egui::Button::selectable(app.br.view == Cat::Location(i), nav_label(app, Cat::Location(i)))).clicked() {
                app.br.view = Cat::Location(i);
                ui.close();
            }
        }
        if has_pack(app, "Your Patches") && ui.add(egui::Button::selectable(app.br.view == Cat::YourPatches, "Your Patches")).clicked() {
            app.br.view = Cat::YourPatches;
            ui.close();
        }
        if has_pack(app, "Your Loops") && ui.add(egui::Button::selectable(app.br.view == Cat::YourLoops, "Your Loops")).clicked() {
            app.br.view = Cat::YourLoops;
            ui.close();
        }
        // management commands only for a selected location
        if let Cat::Location(i) = app.br.view {
            ui.separator();
            location_menu(app, ui, i);
        }
        ui.separator();
        if ui.button("Add Location…").clicked() {
            ui.close();
            run(app, "Add Location", true);
        }
    });
    let gb = Rect::from_min_size(pos2(lb.max.x + 2.0, r.min.y), vec2(28.0, r.height()));
    let gresp = ui.interact(gb, ui.scope_id().with("cnav-smart"), Sense::click()).on_hover_text("Smart Searches");
    crate::glyphs::paint(&p, gb, ph::GEAR, 14.0, if matches!(app.br.view, Cat::Smart(_)) { look::accent() } else { look::text() });
    egui::Popup::menu(&gresp).show(|ui| {
        for i in 0..app.br.saved.smart.len() {
            if ui.add(egui::Button::selectable(app.br.view == Cat::Smart(i), nav_label(app, Cat::Smart(i)))).clicked() {
                app.br.search = app.br.saved.smart[i].1.clone();
                app.br.view = Cat::Smart(i);
                ui.close();
            }
        }
        if let Cat::Smart(i) = app.br.view {
            ui.separator();
            smart_menu(app, ui, i);
        }
        ui.separator();
        if ui.add_enabled(!app.br.search.is_empty(), egui::Button::new("Add Smart Search…")).clicked() {
            ui.close();
            run(app, "Add Smart Search", true);
        }
    });
}

/// Loops & One-Shots: the Loops and One-Shots buttons (the active one again shows both).
fn loops_buttons(app: &mut App, ui: &mut egui::Ui, list: Rect) -> f32 {
    let p = ui.painter().clone();
    let r = Rect::from_min_size(list.min + vec2(8.0, 4.0), vec2((list.width() - 16.0).min(220.0), 22.0));
    let half = r.width() / 2.0;
    for (k, (name, val)) in [("Loops", true), ("One-Shots", false)].into_iter().enumerate() {
        let b = Rect::from_min_size(pos2(r.min.x + k as f32 * half, r.min.y), vec2(half - 2.0, r.height()));
        let on = app.br.search.loops_only == Some(val);
        let resp = ui.interact(b, ui.scope_id().with(("lo", k)), Sense::click());
        p.rect_filled(b, 11.0, if on { look::accent() } else if resp.hovered() { look::button_on() } else { look::group_bg() });
        p.text(b.center(), Align2::CENTER_CENTER, name, look::regular(12.0), if on { Color32::WHITE } else { look::text() });
        if resp.clicked() {
            app.br.search.loops_only = if on { None } else { Some(val) };
        }
    }
    r.max.y + 4.0
}

fn mark_color(it: &Item) -> Color32 {
    match it.kind {
        Kind::Patch if it.loop_type == LoopType::Audio => LoopType::Audio.color(),
        Kind::Patch if it.category == "Drums & Percussion" => Color32::from_rgb(0xe3, 0xb3, 0x2b),
        Kind::Patch => Color32::from_rgb(0x52, 0xc0, 0x3c),
        Kind::OneShot => LoopType::Audio.color(),
        Kind::Loop => it.loop_type.color(),
    }
}

/// The results list; returns how many.
fn result_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let items = results(app);
    let n = items.len();
    if n == 0 {
        let msg = if app.br.view == Cat::Loops && app.br.search.is_empty() {
            "No loops or one-shots yet: add a folder of yours with + ▸ Add Location…"
        } else if !app.br.search.is_empty() {
            "No sounds match this search"
        } else if track_kind(app).is_some_and(|k| k.2) {
            "A folder stack's main track has no patches"
        } else {
            "Nothing here"
        };
        ui.painter_at(list).text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, msg, look::regular(12.0), look::text_dim());
        return 0;
    }
    let _wide = list.width() > 330.0;
    // More ▸ Show Attributes: a table with column headers (All, Loops & One-Shots, locations,
    // Smart Searches; not Instruments / Effects)
    let (list, cols) = if attributes_shown(app) { attr_header(app, ui, list) } else { (list, Vec::new()) };
    let current = current_patch(app);
    let now = app.ctx.input(|i| i.time);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    let mut clicked: Option<(Item, egui::Modifiers, bool)> = None;
    let mut icon_click: Option<(Item, bool)> = None;
    let mut swept: Option<Item> = None;
    let (pointer, alt_held, down) = ui.input(|i| (i.pointer.hover_pos(), i.modifiers.alt, i.pointer.primary_down()));
    if !down {
        app.br.sweeping = false;
    }
    let mut dbl: Option<Item> = None;
    let mut fav: Option<String> = None;
    let mut drag: Option<Item> = None;
    let mut menu_cmd: Option<String> = None;
    egui::ScrollArea::vertical().id_salt(("browser-results", format!("{:?}", app.br.view))).auto_shrink([false, false]).show_rows(&mut child, 24.0, n, |ui, range| {
        for k in range {
            let it = &items[k];
            let (r, resp) = ui.allocate_exact_size(vec2(list.width(), 24.0), Sense::click_and_drag());
            let p = ui.painter().clone();
            let sel = app.br.selected.contains(&it.id);
            let loaded = it.kind == Kind::Patch && current.as_deref() == Some(it.name.as_str());
            if sel {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::accent().gamma_multiply(0.4));
            } else if resp.hovered() {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::group_bg().gamma_multiply(0.6));
            }
            if loaded {
                p.circle_filled(pos2(r.min.x + 8.0, r.center().y), 2.5, look::text());
            }
            // the type mark
            let m = Rect::from_center_size(pos2(r.min.x + 24.0, r.center().y), vec2(17.0, 13.0));
            p.rect_filled(m, 3.0, mark_color(it));
            let glyph = match it.kind {
                Kind::Patch if it.loop_type == LoopType::Audio => ph::FADERS,
                Kind::Patch if it.category == "Drums & Percussion" => crate::glyphs::br::DRUM,
                Kind::Patch => ph::PIANO_KEYS,
                Kind::Loop => ph::REPEAT,
                Kind::OneShot => ph::WAVEFORM,
            };
            let playing = app.preview.is_playing(&it.id, now);
            // (previewing: a small speaker in place of the type glyph)
            crate::glyphs::paint(&p, m, if playing { ph_fill::SPEAKER_HIGH } else { glyph }, 10.0, Color32::from_black_alpha(170));
            // the icon: a click previews (Option: from the start); a drag from it, or Option
            // held, previews each icon reached
            let iresp = ui.interact(m.expand(2.0), ui.scope_id().with(("bicon", k)), Sense::click_and_drag());
            if iresp.clicked() {
                icon_click = Some((it.clone(), ui.input(|i| i.modifiers.alt)));
            }
            if iresp.drag_started() {
                app.br.sweeping = true;
                app.br.swept = None;
            }
            if pointer.is_some_and(|pp| pp.y >= r.min.y && pp.y < r.max.y && (pp.x - m.center().x).abs() <= m.width() / 2.0 + 2.0) && ((app.br.sweeping && down) || alt_held) {
                swept = Some(it.clone());
            }
            let name_end = cols.first().map_or(r.max.x - 28.0, |c| c.1 - 4.0);
            let name_r = Rect::from_min_max(pos2(r.min.x + 40.0, r.min.y), pos2(name_end, r.max.y));
            p.with_clip_rect(name_r).text(pos2(name_r.min.x, r.center().y), Align2::LEFT_CENTER, &it.name, look::regular(13.0), look::text());
            for (c, x0, w) in &cols {
                let txt = attr_text(it, c);
                let cr = Rect::from_min_max(pos2(*x0 + 4.0, r.min.y), pos2(x0 + w - 2.0, r.max.y));
                p.with_clip_rect(cr).text(pos2(cr.min.x, r.center().y), Align2::LEFT_CENTER, txt, look::regular(11.5), look::text_dim());
            }
            // the favourite star: dim on hover, lit for a favourite
            let is_fav = app.br.saved.favorites.contains(&it.id);
            let star = Rect::from_center_size(pos2(r.max.x - 16.0, r.center().y), vec2(18.0, 18.0));
            let sresp = ui.interact(star, ui.scope_id().with(("star", k)), Sense::click()).on_hover_text(if is_fav { "Remove from Favorites" } else { "Add to Favorites" });
            if is_fav || resp.hovered() || sresp.hovered() {
                crate::glyphs::paint(&p, star, if is_fav { ph_fill::STAR } else { ph::STAR }, 13.0, if is_fav { Color32::from_rgb(0xf5, 0xc5, 0x18) } else { look::text_dim() });
            }
            if sresp.clicked() {
                fav = Some(it.id.clone());
            } else if resp.clicked() {
                clicked = Some((it.clone(), ui.input(|i| i.modifiers), sel));
            }
            if resp.double_clicked() {
                dbl = Some(it.clone());
            }
            if resp.drag_started() {
                drag = Some(it.clone());
            }
            resp.context_menu(|ui| {
                if !app.br.selected.contains(&it.id) {
                    app.br.selected = vec![it.id.clone()];
                }
                if let Some(c) = item_menu(app, ui, it) {
                    menu_cmd = Some(c);
                }
            });
        }
    });
    if let Some(id) = fav {
        toggle_favorite(app, &id);
    }
    if let Some((it, option)) = icon_click {
        app.br.selected = vec![it.id.clone()];
        crate::brpreview::click(app, &it, crate::brpreview::Click::Icon { option });
    }
    match swept {
        Some(it) => {
            if app.br.swept.as_deref() != Some(it.id.as_str()) {
                app.br.selected = vec![it.id.clone()];
            }
            crate::brpreview::sweep(app, &it);
        }
        None if !app.br.sweeping => app.br.swept = None,
        None => {}
    }
    if let Some((it, m, was_selected)) = clicked {
        if m.command || m.shift {
            if let Some(i) = app.br.selected.iter().position(|x| *x == it.id) {
                app.br.selected.remove(i);
            } else {
                app.br.selected.push(it.id.clone());
            }
        } else {
            app.br.selected = vec![it.id.clone()];
            click_item(app, &it, was_selected && !m.alt);
        }
    }
    if let Some(it) = dbl {
        match it.kind {
            Kind::Patch if !app.browser_replace => {
                if let Some(t) = app.selected_track {
                    load_patch(app, t, &it);
                }
            }
            Kind::Loop | Kind::OneShot => new_tracks_with(app, vec![it]),
            _ => {}
        }
    }
    if let Some(it) = drag {
        let items: Vec<DragItem> = if app.br.selected.contains(&it.id) { selected_items(app).into_iter().map(DragItem::Item).collect() } else { vec![DragItem::Item(it)] };
        app.br.drag = Some(items);
    }
    if let Some(c) = menu_cmd {
        run(app, &c, true);
    }
    n
}

/// The attribute columns Show Attributes can show, and those shown by default.
pub const ATTR_COLS: [&str; 5] = ["Key", "Bars", "Tempo", "Length", "Sound Pack"];
const ATTR_DEFAULT: &str = "Key,Bars,Tempo";

/// Show Attributes is on and the view has it (not Instruments or Effects).
pub fn attributes_shown(app: &App) -> bool {
    matches!(app.br.view, Cat::Loops | Cat::All | Cat::Location(_) | Cat::YourLoops | Cat::Smart(_)) && app.br.saved.flag("show_attributes", true)
}

fn attr_cols(app: &App) -> Vec<String> {
    let v = app.br.saved.settings.get("attr_cols").cloned().unwrap_or_else(|| ATTR_DEFAULT.into());
    ATTR_COLS.iter().filter(|c| v.split(',').any(|x| x == **c)).map(|c| c.to_string()).collect()
}

fn attr_width(app: &App, c: &str) -> f32 {
    app.br.saved.settings.get(&format!("colw_{c}")).and_then(|v| v.parse().ok()).unwrap_or(if c == "Sound Pack" { 110.0 } else { 46.0 })
}

/// A result's value in an attribute column.
pub fn attr_text(it: &Item, c: &str) -> String {
    match c {
        "Key" => it.key_text(),
        "Bars" => it.bars.map(|b| b.to_string()).unwrap_or_default(),
        "Tempo" => it.bpm.map(|b| format!("{b:.0}")).unwrap_or_default(),
        "Length" => it.seconds.map(|s| format!("{s:.1} s")).unwrap_or_default(),
        "Sound Pack" => it.pack.clone(),
        _ => String::new(),
    }
}

/// A header click: ascending, then descending, then back to relevance.
pub fn next_sort(cur: &str, desc: bool, col: &str) -> (String, bool) {
    if cur != col {
        (col.to_string(), false)
    } else if !desc {
        (col.to_string(), true)
    } else {
        ("Relevance".into(), false)
    }
}

/// The table header: the type column (sorts by type), Name and the attribute columns (click
/// sorts; Control-click shows or hides columns; drag a column's left border to resize it).
/// Returns the list below it and the columns' (name, left x, width).
fn attr_header(app: &mut App, ui: &mut egui::Ui, list: Rect) -> (Rect, Vec<(String, f32, f32)>) {
    let h = 20.0;
    let head = Rect::from_min_max(list.min, pos2(list.max.x, list.min.y + h));
    let p = ui.painter().clone();
    p.rect_filled(head, 0.0, look::header_bg());
    p.hline(head.x_range(), head.max.y - 0.5, Stroke::new(1.0, look::line()));
    let mut cols: Vec<(String, f32, f32)> = Vec::new();
    let mut x = list.max.x - 28.0;
    for c in attr_cols(app).iter().rev() {
        let w = attr_width(app, c);
        x -= w;
        cols.push((c.clone(), x, w));
    }
    cols.reverse();
    let order = sort_order(app);
    let desc = app.br.saved.flag("sort_desc", false);
    let name_end = cols.first().map_or(list.max.x - 28.0, |c| c.1);
    let mut heads: Vec<(String, String, Rect)> = vec![("Type".into(), String::new(), Rect::from_min_max(pos2(list.min.x + 12.0, head.min.y), pos2(list.min.x + 36.0, head.max.y))), ("Name".into(), "Name".into(), Rect::from_min_max(pos2(list.min.x + 36.0, head.min.y), pos2(name_end, head.max.y)))];
    for (c, x0, w) in &cols {
        let sort = match c.as_str() {
            "Length" => "Bars",
            "Sound Pack" => "Type",
            o => o,
        };
        heads.push((sort.to_string(), c.clone(), Rect::from_min_size(pos2(*x0, head.min.y), vec2(*w, h))));
    }
    let mut click: Option<String> = None;
    for (k, (sort, label, r)) in heads.iter().enumerate() {
        if k >= 2 {
            p.vline(r.min.x, (r.min.y + 4.0)..=(r.max.y - 4.0), Stroke::new(1.0, look::line()));
        }
        let on = order == *sort && (k != 0 || label.is_empty());
        p.with_clip_rect(*r).text(pos2(r.min.x + 4.0, r.center().y), Align2::LEFT_CENTER, label, if on { look::semibold(11.0) } else { look::regular(11.0) }, if on { look::text() } else { look::text_dim() });
        if on && r.width() > 30.0 {
            crate::glyphs::paint(&p, Rect::from_center_size(pos2(r.max.x - 9.0, r.center().y), vec2(10.0, 10.0)), if desc { ph::CARET_DOWN } else { ph::CARET_UP }, 9.0, look::text_dim());
        }
        let resp = ui.interact(*r, ui.scope_id().with(("attr-head", k)), Sense::click());
        if resp.clicked() {
            click = Some(sort.clone());
        }
        // Control-click: the columns
        egui::Popup::context_menu(&resp).show(|ui| {
            let shown = attr_cols(app);
            for c in ATTR_COLS {
                let on = shown.iter().any(|x| x == c);
                if ui.add(egui::Button::selectable(on, c)).clicked() {
                    let next: Vec<&str> = ATTR_COLS.iter().copied().filter(|x| if *x == c { !on } else { shown.iter().any(|y| y == x) }).collect();
                    app.br.saved.settings.insert("attr_cols".into(), next.join(","));
                    save_state(app);
                    ui.close();
                }
            }
        });
        // a column's left border: drag to resize
        if k >= 2 {
            let edge = Rect::from_center_size(pos2(r.min.x, r.center().y), vec2(6.0, h));
            let er = ui.interact(edge, ui.scope_id().with(("attr-edge", k)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeColumn);
            if er.dragged() {
                let w = (r.width() - er.drag_delta().x).clamp(28.0, 240.0);
                app.br.saved.settings.insert(format!("colw_{label}"), format!("{w:.0}"));
            }
            if er.drag_stopped() {
                save_state(app);
            }
        }
    }
    if let Some(c) = click {
        let (o, d) = next_sort(&order, desc, &c);
        set_sort(app, &o, d);
    }
    (Rect::from_min_max(pos2(list.min.x, head.max.y), list.max), cols)
}

/// The shortcut menu of a result (Logic's loop menu; patches: theirs).
fn item_menu(app: &mut App, ui: &mut egui::Ui, it: &Item) -> Option<String> {
    let mut cmd: Option<String> = None;
    let word = if it.kind == Kind::OneShot { "One-Shot" } else { "Loop" };
    let item = |ui: &mut egui::Ui, label: &str, enabled: bool| -> bool {
        let c = ui.add_enabled(enabled, egui::Button::new(label)).clicked();
        if c {
            ui.close();
        }
        c
    };
    if it.kind != Kind::Patch {
        if item(ui, &format!("New Track with {word}"), true) {
            cmd = Some("New Track with Loop".into());
        }
        if item(ui, &format!("Place {word} at Playhead"), app.selected_track.is_some()) {
            cmd = Some("Place Loop at Playhead".into());
        }
        let audio = it.loop_type == LoopType::Audio;
        ui.add_enabled_ui(audio, |ui| {
            ui.menu_button(format!("Create Track with {word}"), |ui| {
                for s in ["Quick Sampler", "Sample Alchemy", "Drum Machine Designer"] {
                    if ui.button(s).clicked() {
                        cmd = Some(s.to_string());
                        ui.close();
                    }
                }
            });
        });
        let dmd = app.selected_track.and_then(|t| app.instrument_choice.get(&t)).is_some_and(|c| sl::plugin_of(c) == "Drum Machine Designer");
        if dmd && audio && ui.button(format!("Add {word} to Drum Machine Designer")).clicked() {
            add_to_dmd(app, selected_items(app));
            ui.close();
        }
    } else {
        if item(ui, "New Track with Patch", true) {
            cmd = Some("New Track with Selected Item".into());
        }
        if item(ui, "Replace Patch", app.selected_track.is_some()) {
            cmd = Some("Replace Patch".into());
        }
    }
    ui.separator();
    if item(ui, "Find Similar", true) {
        cmd = Some("Find Similar".into());
    }
    ui.separator();
    let is_fav = app.br.saved.favorites.contains(&it.id);
    if ui.button(if is_fav { "Remove from Favorites" } else { "Add to Favorites" }).clicked() {
        let id = it.id.clone();
        toggle_favorite(app, &id);
        ui.close();
    }
    ui.separator();
    // (greyed out for sounds that came with a sound pack)
    for c in ["Rename", "Delete", "Show in Finder"] {
        if item(ui, c, it.user) {
            cmd = Some(c.into());
        }
    }
    ui.separator();
    if item(ui, "View Info", true) {
        cmd = Some("View Info".into());
    }
    cmd
}

/// Columns view: patches by category, then subcategory, then name.
/// The columns view's columns for the open path: categories, then the chosen category's
/// subcategories, then the chosen subcategory's patches.
pub const USER_PATCHES: &str = "User Patches";

pub(crate) fn column_lists(items: &[Item], path: &[String]) -> Vec<Vec<String>> {
    // (saved patches in a User Patches folder at the top; the rest by category, alphabetically
    // whatever the sort order, as Logic's column layout)
    let cat_of = |i: &Item| -> (String, String) {
        if i.user && i.kind == Kind::Patch {
            return (USER_PATCHES.to_string(), String::new());
        }
        patches().iter().find(|p| p.2 == i.name).map(|p| (p.0.clone(), p.1.clone())).unwrap_or((i.category.clone(), String::new()))
    };
    let mut cols: Vec<Vec<String>> = Vec::new();
    let mut cats: Vec<String> = items.iter().map(|i| cat_of(i).0).collect();
    cats.sort_by_key(|c| (c != USER_PATCHES, c.to_lowercase()));
    cats.dedup();
    cols.push(cats);
    if let Some(c) = path.first().cloned() {
        let mut subs: Vec<String> = items.iter().filter(|i| cat_of(i).0 == c).map(|i| cat_of(i).1).collect();
        subs.sort();
        subs.dedup();
        cols.push(subs);
        if let Some(s) = path.get(1).cloned() {
            let mut names: Vec<String> = items.iter().filter(|i| cat_of(i) == (c.clone(), s.clone())).map(|i| i.name.clone()).collect();
            names.sort_by_key(|n| n.to_lowercase());
            cols.push(names);
        }
    }
    cols
}

/// Arrow keys in the columns view (Logic: they move through the folders): ↑ / ↓ within the
/// focused column, → into the chosen folder, ← back out. Returns the new path and the patch
/// chosen in the last column (by its index there).
pub(crate) fn column_key(cols: &[Vec<String>], path: &[String], chosen: Option<usize>, key: egui::Key) -> (Vec<String>, Option<usize>) {
    let mut path = path.to_vec();
    // the focused column: the patches when one is chosen, else the last folder opened
    let focus = if chosen.is_some() && cols.len() == 3 { 2 } else { path.len().saturating_sub(1) };
    let pick = |col: &[String], cur: Option<usize>, down: bool| -> Option<usize> {
        if col.is_empty() {
            return None;
        }
        Some(match (cur, down) {
            (None, _) => 0,
            (Some(i), true) => (i + 1).min(col.len() - 1),
            (Some(i), false) => i.saturating_sub(1),
        })
    };
    match key {
        egui::Key::ArrowUp | egui::Key::ArrowDown => {
            let down = key == egui::Key::ArrowDown;
            if focus == 2 {
                return (path, pick(&cols[2], chosen, down).or(chosen));
            }
            let col = cols.get(focus).cloned().unwrap_or_default();
            let cur = path.get(focus).and_then(|n| col.iter().position(|c| c == n));
            if let Some(i) = pick(&col, cur, down) {
                path.truncate(focus);
                path.push(col[i].clone());
            }
            (path, None)
        }
        egui::Key::ArrowRight => {
            if focus == 2 || path.is_empty() {
                return (path, chosen);
            }
            // into the folder: its first entry (a subfolder, or the first patch)
            match cols.get(focus + 1) {
                Some(next) if !next.is_empty() && focus + 1 < 2 => {
                    path.push(next[0].clone());
                    (path, None)
                }
                Some(next) if !next.is_empty() => (path, Some(0)),
                _ => (path, None),
            }
        }
        egui::Key::ArrowLeft => {
            if focus == 2 {
                return (path, None);
            }
            if path.len() > 1 {
                path.pop();
            }
            (path, None)
        }
        _ => (path, chosen),
    }
}

fn columns_view(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let items: Vec<Item> = results(app);
    let col_w = (list.width() / 3.0).max(120.0);
    let p = ui.painter().clone();
    // the path bar below the columns: a click on a folder opens it
    let bar = Rect::from_min_max(pos2(list.min.x, list.max.y - 20.0), list.max);
    let list = Rect::from_min_max(list.min, pos2(list.max.x, bar.min.y));
    p.rect_filled(bar, 0.0, look::header_bg());
    p.hline(bar.x_range(), bar.min.y, Stroke::new(1.0, look::line()));
    let root = if app.br.view == Cat::Effects { "Effects" } else { "Instruments" };
    let mut x = bar.min.x + 8.0;
    let mut go_to: Option<usize> = None;
    for (k, part) in std::iter::once(root.to_string()).chain(app.br.column_path.iter().cloned()).enumerate() {
        if k > 0 {
            p.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, "›", look::regular(11.0), look::text_dim());
            x += 10.0;
        }
        let g = p.layout_no_wrap(part.clone(), look::regular(11.0), look::text_dim());
        let seg = Rect::from_min_size(pos2(x, bar.min.y), vec2(g.size().x + 2.0, bar.height()));
        let resp = ui.interact(seg, ui.scope_id().with(("bpath", k)), Sense::click());
        p.galley_with_override_text_color(pos2(x, bar.center().y - g.size().y / 2.0), g, if resp.hovered() { look::text() } else { look::text_dim() });
        if resp.clicked() {
            go_to = Some(k);
        }
        x = seg.max.x + 4.0;
    }
    if let Some(k) = go_to {
        app.br.column_path.truncate(k);
    }
    let cols = column_lists(&items, &app.br.column_path);
    let current = current_patch(app);
    let chosen_name = app.br.selected.first().and_then(|s| items.iter().find(|i| &i.id == s)).map(|i| i.name.clone());
    let mut pick: Option<(usize, String)> = None;
    for (ci, col) in cols.iter().enumerate() {
        let cr = Rect::from_min_size(pos2(list.min.x + ci as f32 * col_w, list.min.y), vec2(col_w, list.height()));
        p.vline(cr.max.x, cr.y_range(), Stroke::new(1.0, look::line()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cr));
        egui::ScrollArea::vertical().id_salt(("bcol", ci)).auto_shrink([false, false]).show_rows(&mut child, 22.0, col.len(), |ui, range| {
            for k in range {
                let name = &col[k];
                let (r, resp) = ui.allocate_exact_size(vec2(col_w, 22.0), Sense::click());
                let on = app.br.column_path.get(ci) == Some(name) || (ci == 2 && (current.as_deref() == Some(name.as_str()) || chosen_name.as_deref() == Some(name.as_str())));
                if on {
                    ui.painter().rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                }
                ui.painter().with_clip_rect(r).text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, if name.is_empty() { "—" } else { name }, look::regular(12.0), look::text());
                if ci < 2 {
                    crate::glyphs::paint(ui.painter(), Rect::from_min_size(pos2(r.max.x - 16.0, r.min.y + 4.0), vec2(14.0, 14.0)), ph::CARET_RIGHT, 9.0, look::text_dim());
                }
                if resp.clicked() {
                    pick = Some((ci, name.clone()));
                }
            }
        });
    }
    if let Some((ci, name)) = pick {
        if ci < 2 {
            app.br.column_path.truncate(ci);
            app.br.column_path.push(name);
        } else if let Some(it) = items.iter().find(|i| i.name == name).cloned() {
            let was = app.br.selected.first() == Some(&it.id);
            app.br.selected = vec![it.id.clone()];
            click_item(app, &it, was);
        }
    }
    items.len()
}

/// All Filters: every tag by category in Logic's coloured cards, in its own window beside the
/// filter button `fb` (allfilters.rs).
fn all_filters(app: &mut App, ui: &mut egui::Ui, fb: Rect) {
    let base: Vec<Item> = {
        let s = app.br.search.clone();
        let any_view = matches!(app.br.view, Cat::Presets | Cat::SoundLibrary | Cat::SoundFonts);
        app.br.items.iter().filter(|it| any_view || in_view(app, it)).filter(|it| s.matches(it, &app.br.saved.favorites)).cloned().collect()
    };
    crate::allfilters::window(app, ui.ctx(), fb, base);
}

/// Sound Library: the sound packs (installed or not) and what they hold.
fn sound_library(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink(10.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    let mut packs: Vec<(String, usize, usize)> = Vec::new();
    for it in &app.br.items {
        let sound = (it.kind != Kind::Patch) as usize;
        match packs.iter_mut().find(|p| p.0 == it.pack) {
            Some(p) => {
                p.1 += 1;
                p.2 += sound;
            }
            None => packs.push((it.pack.clone(), 1, sound)),
        }
    }
    let installed = lg_sounds::root().is_some();
    let mut go: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("sound-lib").show(&mut child, |ui| {
        ui.label(egui::RichText::new("Sound Packs").strong());
        if !installed {
            ui.label(egui::RichText::new("Bramble Sounds (drum kits, percussion, orchestra, guitars, piano) isn't installed: run crates/lg-sounds/fetch.sh").color(look::text_dim()));
        }
        for (name, n, sounds) in &packs {
            ui.horizontal(|ui| {
                if ui.link(name).clicked() {
                    go = Some(name.clone());
                }
                ui.label(egui::RichText::new(if *sounds > 0 { format!("{sounds} loops & one-shots") } else { format!("{n} patches") }).color(look::text_dim()));
            });
        }
    });
    if let Some(p) = go {
        app.br.search.clear();
        app.br.search.packs.push(p);
        app.br.view = Cat::All;
    }
    packs.len()
}

// ---- Plug-in Presets

/// The plug-ins of the selected track: (None = the instrument / Some(k) = Audio FX k, name).
fn track_plugins(app: &App) -> Vec<(Option<usize>, String)> {
    let Some(t) = app.selected_track else { return Vec::new() };
    let mut v = Vec::new();
    if let Some(i) = app.instrument_for(t) {
        if app.engine.lock().ok().and_then(|e| e.instrument_params(t)).is_some() {
            v.push((None, sl::plugin_of(&i).to_string()));
        }
    }
    for (k, u) in crate::mixstate::inserts(app, t).into_iter().enumerate() {
        v.push((Some(k), u.name));
    }
    v
}

/// A plug-in's presets: the user's (Studio's saved settings, at the top), then the folders of
/// .pst files in the Plug-In Settings folders: (path in the tree, file).
pub fn preset_tree(plugin: &str) -> Vec<(Vec<String>, PathBuf)> {
    let mut v: Vec<(Vec<String>, PathBuf)> = crate::pluginwin::saved_settings(plugin).into_iter().map(|(n, p)| (vec![n], p)).collect();
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    fn walk(d: &Path, path: Vec<String>, out: &mut Vec<(Vec<String>, PathBuf)>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        let mut ents: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        ents.sort();
        for p in ents {
            let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let mut np = path.clone();
            np.push(name);
            if p.is_dir() && depth > 0 {
                walk(&p, np, out, depth - 1);
            } else if p.extension().is_some_and(|e| e == "pst") {
                out.push((np, p));
            }
        }
    }
    for root in [home.join("Music/Audio Music Apps/Plug-In Settings").join(plugin), sl::user_dir("Plug-In Settings").join(plugin)] {
        walk(&root, Vec::new(), &mut v, 4);
    }
    v
}

/// Loads a preset file into a plug-in: Studio's "id value" settings or a Logic .pst.
fn load_preset(app: &mut App, slot: Option<usize>, file: &Path) {
    let Some(t) = app.selected_track else { return };
    let pairs: Vec<(u32, f32)> = if file.extension().is_some_and(|e| e == "pst") {
        let Some(f) = std::fs::read(file).ok().and_then(|b| lg_core::plugin::read_pst(&b)) else {
            app.tell("That preset couldn't be read");
            return;
        };
        f.into_iter().enumerate().map(|(i, v)| (i as u32, v)).collect()
    } else {
        std::fs::read_to_string(file).unwrap_or_default().lines().filter_map(|l| {
            let mut w = l.split_whitespace();
            Some((w.next()?.parse().ok()?, w.next()?.parse().ok()?))
        }).collect()
    };
    let name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match slot {
        None => {
            let ids: Vec<u32> = app.engine.lock().ok().and_then(|e| e.instrument_params(t)).map(|(p, _)| p.iter().map(|x| x.id).collect()).unwrap_or_default();
            if let Ok(mut e) = app.engine.lock() {
                for (id, v) in &pairs {
                    if ids.contains(id) {
                        e.set_instrument_param(t, *id, *v);
                    }
                }
            }
        }
        Some(k) => {
            let strip = crate::mixer::fx_strip(t);
            let ids: Vec<u32> = app.engine.lock().ok().and_then(|e| e.effect_params(strip, k)).map(|(p, _)| p.iter().map(|x| x.id).collect()).unwrap_or_default();
            if let Ok(mut e) = app.engine.lock() {
                for (id, v) in &pairs {
                    if ids.contains(id) {
                        e.set_effect_param(strip, k, *id, *v);
                    }
                }
            }
            crate::mixstate::edit_inserts(app, t, |list| {
                if let Some(u) = list.get_mut(k) {
                    for (id, v) in &pairs {
                        let i = *id as usize;
                        if u.params.len() <= i {
                            u.params.resize(i + 1, 0.0);
                        }
                        u.params[i] = *v;
                    }
                    u.preset = format!("{name}.pst");
                }
            });
        }
    }
    app.dirty = true;
    app.tell(format!("Loaded the preset “{name}”"));
}

fn presets_view(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let plugins = track_plugins(app);
    let p = ui.painter().clone();
    if plugins.is_empty() {
        p.text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, "Select a track with a plug-in to see its presets", look::regular(12.0), look::text_dim());
        return 0;
    }
    if !plugins.iter().any(|x| x.0 == app.br.preset_slot) {
        app.br.preset_slot = plugins[0].0;
        app.br.preset_path.clear();
    }
    let cur = plugins.iter().find(|x| x.0 == app.br.preset_slot).cloned().unwrap_or_else(|| plugins[0].clone());
    // the plug-in (as the Browser Focus triangle picks it)
    let pr = Rect::from_min_size(list.min + vec2(8.0, 4.0), vec2(list.width() - 16.0, 22.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pr));
    egui::ComboBox::from_id_salt("preset-plugin").selected_text(&cur.1).width(pr.width() - 8.0).show_ui(&mut child, |ui| {
        for (slot, n) in &plugins {
            let label = match slot {
                None => format!("Instrument: {n}"),
                Some(k) => format!("Audio FX {}: {n}", k + 1),
            };
            if ui.add(egui::Button::selectable(*slot == app.br.preset_slot, label)).clicked() {
                app.br.preset_slot = *slot;
                app.br.preset_path.clear();
            }
        }
    });
    let q = app.br.typed.trim().to_lowercase();
    let tree = preset_tree(&cur.1);
    let body = Rect::from_min_max(pos2(list.min.x, pr.max.y + 6.0), pos2(list.max.x, list.max.y - 22.0));
    if tree.is_empty() {
        p.text(pos2(body.center().x, body.min.y + 24.0), Align2::CENTER_CENTER, format!("No presets for {} yet: save one in its plug-in window", cur.1), look::regular(12.0), look::text_dim());
        return 0;
    }
    // the columns: each level of the tree (folders with an arrow)
    let depth = app.br.preset_path.len() + 1;
    let col_w = (body.width() / depth.min(3) as f32).max(110.0);
    let first = depth.saturating_sub(3);
    let mut pick: Option<(usize, String, Option<PathBuf>)> = None;
    for level in first..depth {
        let prefix = app.br.preset_path[..level].to_vec();
        let mut entries: Vec<(String, Option<PathBuf>)> = Vec::new();
        for (path, file) in &tree {
            if path.len() > level && path[..level] == prefix[..] {
                let name = path[level].clone();
                let leaf = path.len() == level + 1;
                if !q.is_empty() && leaf && !name.to_lowercase().contains(&q) {
                    continue;
                }
                if !entries.iter().any(|e| e.0 == name) {
                    entries.push((name, leaf.then(|| file.clone())));
                }
            }
        }
        let cr = Rect::from_min_size(pos2(body.min.x + (level - first) as f32 * col_w, body.min.y), vec2(col_w, body.height()));
        p.vline(cr.max.x, cr.y_range(), Stroke::new(1.0, look::line()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cr));
        egui::ScrollArea::vertical().id_salt(("pcol", level)).auto_shrink([false, false]).show_rows(&mut child, 22.0, entries.len(), |ui, range| {
            for k in range {
                let (name, file) = &entries[k];
                let (r, resp) = ui.allocate_exact_size(vec2(col_w, 22.0), Sense::click());
                if app.br.preset_path.get(level) == Some(name) {
                    ui.painter().rect_filled(r, 0.0, look::accent().gamma_multiply(0.45));
                }
                ui.painter().with_clip_rect(r).text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, name, look::regular(12.0), look::text());
                if file.is_none() {
                    crate::glyphs::paint(ui.painter(), Rect::from_min_size(pos2(r.max.x - 16.0, r.min.y + 4.0), vec2(14.0, 14.0)), ph::CARET_RIGHT, 9.0, look::text_dim());
                }
                if resp.clicked() {
                    pick = Some((level, name.clone(), file.clone()));
                }
            }
        });
    }
    // the path bar
    let bar = Rect::from_min_max(pos2(list.min.x, list.max.y - 20.0), list.max);
    p.hline(bar.x_range(), bar.min.y, Stroke::new(1.0, look::line()));
    let mut x = bar.min.x + 8.0;
    let mut go_to: Option<usize> = None;
    let parts: Vec<String> = std::iter::once(cur.1.clone()).chain(app.br.preset_path.iter().cloned()).collect();
    for (k, part) in parts.iter().enumerate() {
        let w = p.layout_no_wrap(part.clone(), look::regular(11.0), look::text()).size().x;
        let r = Rect::from_min_size(pos2(x, bar.min.y + 2.0), vec2(w, 16.0));
        if ui.interact(r, ui.scope_id().with(("ppath", k)), Sense::click()).clicked() {
            go_to = Some(k);
        }
        p.text(r.left_center(), Align2::LEFT_CENTER, part, look::regular(11.0), look::text_dim());
        x += w + 4.0;
        p.text(pos2(x, bar.center().y), Align2::LEFT_CENTER, "›", look::regular(11.0), look::text_dim());
        x += 10.0;
    }
    if let Some(k) = go_to {
        app.br.preset_path.truncate(k);
    }
    if let Some((level, name, file)) = pick {
        app.br.preset_path.truncate(level);
        match file {
            Some(f) => load_preset(app, cur.0, &f),
            None => app.br.preset_path.push(name),
        }
    }
    tree.len()
}

// ---- SoundFonts (Bramble's extra)

fn soundfont_list(app: &mut App, ui: &mut egui::Ui, list: Rect) -> usize {
    let q = app.br.typed.to_lowercase();
    let presets: Vec<(u16, u8, String)> = app.presets.iter().filter(|(_, _, n)| q.is_empty() || n.to_lowercase().contains(&q)).cloned().collect();
    if presets.is_empty() {
        ui.painter_at(list).text(pos2(list.center().x, list.min.y + 30.0), Align2::CENTER_CENTER, "No SoundFont: add one with +", look::regular(12.0), look::text_dim());
        return 0;
    }
    let current = app.selected_track.and_then(|t| app.programs.get(t).copied());
    let n = presets.len();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
    child.spacing_mut().item_spacing.y = 0.0;
    let mut pick = None;
    egui::ScrollArea::vertical().id_salt("browser-sf").auto_shrink([false, false]).show_rows(&mut child, 24.0, n, |ui, range| {
        for k in range {
            let (bank, prog, name) = &presets[k];
            let (r, resp) = ui.allocate_exact_size(vec2(list.width(), 24.0), Sense::click());
            let p = ui.painter();
            if current == Some(*prog) && *bank == 0 {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::accent().gamma_multiply(0.4));
            } else if resp.hovered() {
                p.rect_filled(r.shrink2(vec2(4.0, 1.0)), 5.0, look::group_bg().gamma_multiply(0.6));
            }
            let m = Rect::from_center_size(pos2(r.min.x + 24.0, r.center().y), vec2(17.0, 13.0));
            p.rect_filled(m, 3.0, if *bank == 128 { Color32::from_rgb(0xe3, 0xb3, 0x2b) } else { Color32::from_rgb(0x9b, 0x6c, 0xe6) });
            crate::glyphs::paint(&p, m, if *bank == 128 { crate::glyphs::br::DRUM } else { ph::PIANO_KEYS }, 10.0, Color32::from_black_alpha(170));
            p.with_clip_rect(r).text(pos2(r.min.x + 40.0, r.center().y), Align2::LEFT_CENTER, name.trim(), look::regular(13.0), look::text());
            if resp.clicked() {
                pick = Some((*prog, name.trim().to_string()));
            }
        }
    });
    if let Some((prog, name)) = pick {
        match app.selected_track {
            Some(t) => {
                app.instrument_choice.remove(&t);
                app.set_program(t, prog, &name);
                app.load_engine();
            }
            None => app.tell("Select a track first"),
        }
    }
    n
}

fn add_soundfont(app: &mut App) {
    if app.automated {
        return;
    }
    let Some(path) = kn_ui::files::FileDialog::new().set_title("Add a SoundFont").add_filter("SoundFont", &["sf2", "sf3", "SF2", "SF3"]).pick_file() else { return };
    match app.soundfont_mode {
        SoundFontMode::Native => {
            let ok = {
                let mut e = app.engine.lock().unwrap_or_else(|e| e.into_inner());
                e.load_soundfont(Some(&path));
                e.has_soundfont()
            };
            if ok {
                app.presets = crate::app::soundfont_presets(&path);
                crate::app::save_soundfont_setting(&path);
                app.load_engine();
                app.br.view = Cat::SoundFonts;
                app.tell(format!("{} added ({} sounds)", path.file_stem().unwrap_or_default().to_string_lossy(), app.presets.len()));
            } else {
                app.tell("That SoundFont couldn't be read");
            }
        }
        SoundFontMode::Sampler => app.tell("Converting SoundFonts into Sampler instruments isn't available yet: switch to “Play natively” in Settings"),
    }
}

// ---- the footer

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn flag_item(ui: &mut egui::Ui, app: &mut App, k: &str, label: &str, default: bool) {
    let on = app.br.saved.flag(k, default);
    if ui.add(egui::Button::selectable(on, label)).clicked() {
        app.br.saved.set_flag(k, !on);
        save_state(app);
        ui.close();
    }
}

fn footer(app: &mut App, ui: &mut egui::Ui, foot: Rect, count: usize) {
    let p = ui.painter().clone();
    p.hline(foot.x_range(), foot.min.y, Stroke::new(1.0, look::line()));
    let order = sort_order(app);
    let sorted = if app.br.search.similar.is_some() { "Similarity" } else { order.as_str() };
    p.text(pos2(foot.min.x + 10.0, foot.min.y + 13.0), Align2::LEFT_CENTER, "Sorted by:", look::regular(11.0), look::text_dim());
    let sw = p.layout_no_wrap("Sorted by: ".into(), look::regular(11.0), look::text_dim()).size().x;
    p.text(pos2(foot.min.x + 10.0 + sw, foot.min.y + 13.0), Align2::LEFT_CENTER, sorted, look::semibold(11.0), look::text());
    p.text(pos2(foot.min.x + 10.0, foot.min.y + 29.0), Align2::LEFT_CENTER, format!("{} Items", thousands(count)), look::regular(11.0), look::text_dim());
    // the Preview Volume button (views with loops and one-shots; Logic's Instruments footer
    // has only More): its slider in a pop-up (prevplayer.rs)
    if matches!(app.br.view, Cat::Loops | Cat::All | Cat::Location(_) | Cat::YourLoops) {
        let vr = Rect::from_min_size(pos2(foot.max.x - 76.0, foot.min.y + 8.0), vec2(30.0, 24.0));
        crate::prevplayer::volume_button(app, ui, vr);
    }
    let more = Rect::from_min_size(pos2(foot.max.x - 40.0, foot.min.y + 8.0), vec2(30.0, 24.0));
    let mresp = ui.interact(more, ui.scope_id().with("bmore"), Sense::click()).on_hover_text("More");
    p.rect_filled(more, 6.0, if mresp.hovered() { look::button_on() } else { look::group_bg() });
    crate::glyphs::paint(&p, more, ph::DOTS_THREE, 14.0, look::text());
    egui::Popup::menu(&mresp).show(|ui| {
        ui.set_min_width(240.0);
        let loops = matches!(app.br.view, Cat::Loops | Cat::All | Cat::Location(_) | Cat::YourLoops | Cat::Smart(_));
        ui.menu_button("Sort By", |ui| {
            ui.set_min_width(160.0);
            let cur = sort_order(app);
            let desc = app.br.saved.flag("sort_desc", false);
            let extra: &[&str] = if loops { &LOOP_SORTS } else { &[] };
            for o in SORTS.iter().chain(extra.iter()) {
                if ui.add(egui::Button::selectable(cur == *o, *o)).clicked() {
                    set_sort(app, o, desc);
                    ui.close();
                }
            }
            ui.separator();
            // (every criterion but Relevance can go either way)
            let rel = cur == "Relevance";
            if ui.add_enabled(!rel, egui::Button::selectable(!rel && !desc, "Ascending")).clicked() {
                set_sort(app, &cur, false);
                ui.close();
            }
            if ui.add_enabled(!rel, egui::Button::selectable(!rel && desc, "Descending")).clicked() {
                set_sort(app, &cur, true);
                ui.close();
            }
        });
        if loops {
            flag_item(ui, app, "show_attributes", "Show Attributes", true);
        }
        ui.separator();
        if matches!(app.br.view, Cat::Loops | Cat::All | Cat::Location(_) | Cat::YourLoops) {
            ui.menu_button("Loop Settings", |ui| {
                flag_item(ui, app, "use_project_key", "Use Project Key", true);
                flag_item(ui, app, "use_project_tempo", "Use Project Tempo", true);
                flag_item(ui, app, "auto_level", "Auto Leveling", true);
            });
            flag_item(ui, app, "show_waveform", "Show Waveform", true);
            ui.menu_button("Loop Chords", |ui| {
                let cur = app.br.saved.settings.get("chords").cloned().unwrap_or_else(|| "Automatic".into());
                for c in ["Keep Region Chords on Import", "Replace Global Chords with Region Chords", "Automatic"] {
                    if ui.add(egui::Button::selectable(cur == c, c)).clicked() {
                        app.br.saved.settings.insert("chords".into(), c.into());
                        save_state(app);
                        ui.close();
                    }
                }
            });
        }
        crate::brpreview::menu_item(app, ui);
        flag_item(ui, app, "compact", "Use Compact Navigation", false);
        if matches!(app.br.view, Cat::All | Cat::Instruments | Cat::Effects | Cat::YourPatches) {
            ui.separator();
            if ui.add_enabled(app.selected_track.is_some() && !track_kind(app).is_some_and(|k| k.2), egui::Button::new("Save Patch for Selected Track…")).clicked() {
                ui.close();
                run(app, "Save Patch for Selected Track…", true);
            }
            flag_item(ui, app, "patch_merging", "Enable Patch Merging", false);
            flag_item(ui, app, "columns", "Columns View", false);
        }
    });
}

// ---- dialogs

fn sheet(ctx: &egui::Context, title: &str, f: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(title).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, f);
}

/// A name entry: Some(Some(name)) when confirmed, Some(None) cancelled, None still open.
fn name_sheet(ctx: &egui::Context, title: &str, text: &mut String, ok: &str) -> Option<Option<String>> {
    let mut out = None;
    sheet(ctx, title, |ui| {
        let te = ui.text_edit_singleline(text);
        crate::widgets::focus_once(ui, &te);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                out = Some(None);
            }
            if ui.button(ok).clicked() || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                out = Some(Some(text.trim().to_string()));
            }
        });
    });
    out
}

/// A sound's type as the Info window names it.
pub fn type_name(it: &Item) -> &'static str {
    match it.kind {
        Kind::Patch => if it.loop_type == LoopType::Audio { "Audio Patch" } else { "Instrument Patch" },
        Kind::Loop => match it.loop_type {
            LoopType::Audio => "Audio Loop",
            LoopType::Midi => "MIDI Loop",
            LoopType::SessionPlayer => "Session Player Loop",
            LoopType::Pattern => "Pattern Loop",
        },
        Kind::OneShot => "One-Shot",
    }
}

/// The Info window's details for the selected sounds: (label, value), "*" where they differ.
pub fn info_rows(items: &[Item]) -> Vec<(&'static str, String)> {
    let same = |f: &dyn Fn(&Item) -> String| -> String {
        let v: Vec<String> = items.iter().map(f).collect();
        if v.iter().all(|x| *x == v[0]) { v[0].clone() } else { "*".into() }
    };
    let mut rows = vec![
        ("Name", same(&|i| i.name.clone())),
        ("Type", same(&|i| type_name(i).to_string())),
        ("File Format", same(&|i| i.path.as_ref().and_then(|p| p.extension()).map(|e| match e.to_string_lossy().to_lowercase().as_str() { "wav" | "wave" => "WAVE".to_string(), "aif" | "aiff" => "AIFF".to_string(), "mid" | "midi" => "MIDI".to_string(), o => o.to_uppercase() }).unwrap_or_else(|| if i.kind == Kind::Patch { "Patch".into() } else { String::new() }))),
    ];
    if items.iter().all(|i| i.kind == Kind::Loop) {
        rows.push(("Tempo", same(&|i| i.bpm.map(|b| format!("{b:.0} bpm")).unwrap_or_default())));
        rows.push(("Length", same(&|i| i.bars.map(|b| format!("{b} bar{}", if b == 1 { "" } else { "s" })).unwrap_or_default())));
        rows.push(("Time Signature", same(&|i| i.groups.iter().find(|g| g.0 == "Time Signature").map_or("4/4".into(), |g| g.1.clone()))));
        rows.push(("Key", same(&|i| i.key.as_ref().map(|k| format!("{k} {}", if i.minor { "minor" } else { "major" })).unwrap_or_default())));
    } else if items.iter().all(|i| i.kind == Kind::OneShot) {
        rows.push(("Length", same(&|i| i.seconds.map(|s| format!("{s:.2} s")).unwrap_or_default())));
    }
    rows
}

/// A tag on all the sounds (Some(true)), some of them (Some(false)) or none.
pub fn tag_state(items: &[Item], tag: &str) -> Option<bool> {
    let n = items.iter().filter(|i| i.tags.iter().any(|t| t == tag)).count();
    if n == 0 { None } else { Some(n == items.len()) }
}

/// Edits a user sound's tags (the Info window's Edit mode): saved beside its file.
fn set_user_tags(app: &mut App, id: &str, tags: Vec<String>) {
    let Some(it) = app.br.items.iter_mut().find(|i| i.id == id && i.user) else { return };
    it.tags = tags.clone();
    let Some(path) = it.path.clone() else { return };
    let side = sidecar(&path);
    let old = std::fs::read_to_string(&side).unwrap_or_default();
    let mut lines: Vec<String> = old.lines().filter(|l| !l.starts_with("settags=")).map(str::to_string).collect();
    lines.push(format!("settags={}", tags.join(",")));
    let _ = std::fs::write(side, lines.join("\n") + "\n");
}

/// The Info window, beside the Browser: the selected sounds' details and tags (follows the
/// selection); Edit (top left) shows every tag to assign or unassign on user sounds; Close at
/// the top right.
fn info_window(app: &mut App, ctx: &egui::Context) {
    let Some(first) = app.br.info.clone() else { return };
    let ids: Vec<String> = if app.br.selected.is_empty() { vec![first] } else { app.br.selected.clone() };
    let items: Vec<Item> = ids.iter().filter_map(|id| item(app, id)).collect();
    if items.is_empty() {
        app.br.info = None;
        return;
    }
    let edit_id = egui::Id::unique("lg-br-info-edit");
    let mut edit: bool = ctx.data(|d| d.get_temp(edit_id)).unwrap_or(false);
    let mut close = false;
    let at = pos2(app.br.rect.max.x + 6.0, app.br.rect.min.y + 40.0);
    let mut toggles: Vec<String> = Vec::new();
    let editable = items.iter().all(|i| i.user && i.path.is_some());
    egui::Area::new(egui::Id::unique("lg-br-info")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(270.0);
            // Edit · Info · Close
            ui.horizontal(|ui| {
                if ui.add(egui::Button::selectable(edit, "Edit")).on_hover_text("Edit the tags").clicked() {
                    edit = !edit;
                }
                ui.add_space(60.0);
                ui.label(egui::RichText::new("Info").font(look::semibold(13.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(egui::RichText::new(ph::X).font(egui::FontId::new(12.0, crate::glyphs::family())))).on_hover_text("Close").clicked() {
                        close = true;
                    }
                });
            });
            ui.separator();
            egui::Grid::new("binfo").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for (k, v) in info_rows(&items) {
                    if !v.is_empty() {
                        ui.label(egui::RichText::new(k).color(look::text_dim()));
                        ui.label(v);
                        ui.end_row();
                    }
                }
            });
            ui.separator();
            // the tags, by category (Type, Key, Tonality and Time Signature are set by Studio and
            // shown above; Sound Packs only belong to factory sounds)
            let mut cats: Vec<(&str, Vec<&str>)> = sl::categories().iter().map(|c| (c.name, c.tags.iter().flat_map(|(t, subs)| std::iter::once(*t).chain(subs.iter().copied())).collect())).collect();
            cats.push(("Descriptors", sl::PAIRS.iter().flat_map(|(a, b)| [*a, *b]).collect()));
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (cat, tags) in cats {
                    let shown: Vec<&str> = if edit { tags } else { tags.into_iter().filter(|t| tag_state(&items, t).is_some()).collect() };
                    if shown.is_empty() {
                        continue;
                    }
                    ui.label(egui::RichText::new(cat).color(look::text_dim()).font(look::semibold(11.0)));
                    ui.horizontal_wrapped(|ui| {
                        for t in shown {
                            let st = tag_state(&items, t);
                            // (a dot: on some of the selected sounds only)
                            let text = if st == Some(false) { format!("• {t}") } else { t.to_string() };
                            if edit {
                                let b = ui.add_enabled(editable, egui::Button::selectable(st.is_some(), text));
                                if b.clicked() {
                                    toggles.push(t.to_string());
                                }
                            } else {
                                ui.label(egui::RichText::new(text).background_color(look::group_bg()));
                            }
                        }
                    });
                }
            });
            if edit && !editable {
                ui.label(egui::RichText::new("Tags can be edited on your own sounds").color(look::text_dim()).font(look::regular(11.0)));
            }
        });
    });
    for t in toggles {
        let all = tag_state(&items, &t) == Some(true);
        for it in &items {
            let mut tags = it.tags.clone();
            tags.retain(|x| *x != t);
            if !all {
                tags.push(t.clone());
            }
            set_user_tags(app, &it.id, tags);
        }
    }
    ctx.data_mut(|d| d.insert_temp(edit_id, edit));
    if close {
        app.br.info = None;
    }
}

fn dialogs(app: &mut App, ctx: &egui::Context) {
    info_window(app, ctx);
    if let Some((id, mut name)) = app.br.renaming.take() {
        match name_sheet(ctx, "Rename", &mut name, "Rename") {
            Some(Some(n)) if !n.is_empty() => rename_item(app, &id, &n),
            Some(_) => {}
            None => app.br.renaming = Some((id, name)),
        }
    }
    if let Some(id) = app.br.deleting.clone() {
        let mut close = false;
        let mut go = false;
        sheet(ctx, "Delete", |ui| {
            ui.label("Delete this sound? This can't be undone.");
            ui.horizontal(|ui| {
                close = ui.button("Cancel").clicked();
                go = ui.button("Delete").clicked();
            });
        });
        if close || go {
            app.br.deleting = None;
        }
        if go {
            delete_item(app, &id);
        }
    }
    if let Some(mut name) = app.br.smart_name.take() {
        match name_sheet(ctx, "Add Smart Search", &mut name, "Save") {
            Some(Some(n)) if !n.is_empty() => {
                let s = app.br.search.clone();
                app.br.saved.smart.push((n, s));
                app.br.view = Cat::Smart(app.br.saved.smart.len() - 1);
                save_state(app);
            }
            Some(_) => {}
            None => app.br.smart_name = Some(name),
        }
    }
    if let Some((i, mut name)) = app.br.smart_rename.take() {
        match name_sheet(ctx, "Rename Smart Search", &mut name, "Rename") {
            Some(Some(n)) if !n.is_empty() => {
                if let Some(s) = app.br.saved.smart.get_mut(i) {
                    s.0 = n;
                }
                save_state(app);
            }
            Some(_) => {}
            None => app.br.smart_rename = Some((i, name)),
        }
    }
    if let Some((i, mut name)) = app.br.location_rename.take() {
        match name_sheet(ctx, "Rename Location", &mut name, "Rename") {
            Some(Some(n)) if !n.is_empty() => {
                // (the Browser's name only: the folder keeps its own)
                if let Some(l) = app.br.saved.locations.get_mut(i) {
                    l.name = n;
                }
                save_state(app);
                reload(app);
            }
            Some(_) => {}
            None => app.br.location_rename = Some((i, name)),
        }
    }
    if let Some(mut name) = app.br.patch_name.take() {
        match name_sheet(ctx, "Save Patch", &mut name, "Save") {
            Some(Some(n)) if !n.is_empty() => {
                if let Some(t) = app.selected_track {
                    let audio = app.project.as_ref().and_then(|p| p.tracks.get(t)).is_some_and(|tr| tr.kind == TrackKind::Audio);
                    let path = sl::user_dir("Patches").join(if audio { "Audio" } else { "Instrument" }).join(format!("{}.patch", n.replace('/', "-")));
                    match write_user_patch(app, t, &path) {
                        Ok(()) => {
                            reload(app);
                            app.tell(format!("Saved the patch “{n}” (Your Patches)"));
                        }
                        Err(e) => app.tell(format!("Couldn't save the patch: {e}")),
                    }
                }
            }
            Some(_) => {}
            None => app.br.patch_name = Some(name),
        }
    }
    if let Some(mut m) = app.br.multi_drop.take() {
        let mut done = None;
        sheet(ctx, "Add Selected Files to Tracks", |ui| {
            ui.radio_value(&mut m.choice, 0, "Create new tracks");
            ui.radio_value(&mut m.choice, 1, "Use existing tracks");
            ui.radio_value(&mut m.choice, 2, "Place all files on one track");
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("OK").clicked() {
                    done = Some(true);
                }
            });
        });
        match done {
            Some(true) => multi_drop_done(app, m),
            Some(false) => {}
            None => app.br.multi_drop = Some(m),
        }
    }
    if let Some(mut a) = app.br.add_to_browser.take() {
        let mut done = None;
        sheet(ctx, "Add Region to Browser", |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut a.name);
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(a.can_loop, |ui| {
                    ui.radio_value(&mut a.one_shot, false, "Loop").on_hover_text("Follows the project tempo, key and scale");
                });
                ui.radio_value(&mut a.one_shot, true, "One-Shot").on_hover_text("Plays at its own length, tempo and key");
            });
            if !a.can_loop {
                ui.label(egui::RichText::new("A loop needs a whole number of beats").color(look::text_dim()));
            }
            ui.add_enabled_ui(!a.one_shot, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Key").selected_text(a.key.clone()).show_ui(ui, |ui| {
                        for k in ["C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B"] {
                            ui.selectable_value(&mut a.key, k.to_string(), k);
                        }
                    });
                    egui::ComboBox::from_label("Scale").selected_text(if a.minor { "Minor" } else { "Major" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut a.minor, false, "Major");
                        ui.selectable_value(&mut a.minor, true, "Minor");
                    });
                });
            });
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = Some(false);
                }
                if ui.button("Create").clicked() {
                    done = Some(true);
                }
            });
        });
        match done {
            Some(true) => add_to_browser_done(app, a),
            Some(false) => {}
            None => app.br.add_to_browser = Some(a),
        }
    }
}

/// Rename a user sound: the file (and its tags) on disk.
fn rename_item(app: &mut App, id: &str, name: &str) {
    let Some(it) = item(app, id) else { return };
    let Some(p) = it.path.clone().filter(|_| it.user) else { return };
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let to = p.with_file_name(format!("{}{ext}", name.replace('/', "-")));
    if to.exists() {
        app.tell("A sound with that name is already there");
        return;
    }
    if std::fs::rename(&p, &to).is_ok() {
        let _ = std::fs::rename(sidecar(&p), sidecar(&to));
        if app.br.saved.favorites.remove(id) {
            app.br.saved.favorites.insert(to.display().to_string());
            save_state(app);
        }
        app.br.selected = vec![to.display().to_string()];
        reload(app);
    }
}

/// Delete a user sound (after the confirmation: it can't be undone).
fn delete_item(app: &mut App, id: &str) {
    let Some(it) = item(app, id) else { return };
    let Some(p) = it.path.clone().filter(|_| it.user) else { return };
    if std::fs::remove_file(&p).is_ok() {
        let _ = std::fs::remove_file(sidecar(&p));
        app.br.saved.favorites.remove(id);
        save_state(app);
        app.br.selected.clear();
        reload(app);
    }
}

#[cfg(test)]
mod tests {
    fn it(name: &str, bpm: Option<f64>, key: Option<&str>, user: bool) -> Item {
        Item { id: name.into(), name: name.into(), kind: if bpm.is_some() { Kind::Loop } else { Kind::Patch }, loop_type: LoopType::Audio, category: String::new(), path: None, tags: Vec::new(), groups: Vec::new(), pack: String::new(), user, bpm, key: key.map(str::to_string), minor: false, bars: Some(4), seconds: None, location: None }
    }

    #[test]
    fn info_rows_mark_differences_and_partial_tags() {
        let mut a = it("Groove A", Some(120.0), Some("C"), true);
        a.path = Some(PathBuf::from("/x/Groove A.wav"));
        a.tags = vec!["Drums".into(), "Funk".into()];
        let mut b = it("Groove B", Some(100.0), Some("C"), true);
        b.path = Some(PathBuf::from("/x/Groove B.aif"));
        b.tags = vec!["Drums".into()];
        let one = info_rows(std::slice::from_ref(&a));
        assert!(one.contains(&("Type", "Audio Loop".to_string())));
        assert!(one.contains(&("File Format", "WAVE".to_string())));
        assert!(one.contains(&("Tempo", "120 bpm".to_string())));
        assert!(one.contains(&("Length", "4 bars".to_string())));
        assert!(one.contains(&("Time Signature", "4/4".to_string())));
        let both = info_rows(&[a.clone(), b.clone()]);
        assert!(both.contains(&("Name", "*".to_string())));
        assert!(both.contains(&("Tempo", "*".to_string())));
        assert!(both.contains(&("Key", "C major".to_string())));
        assert_eq!(tag_state(&[a.clone(), b.clone()], "Drums"), Some(true));
        assert_eq!(tag_state(&[a.clone(), b.clone()], "Funk"), Some(false));
        assert_eq!(tag_state(&[a, b], "Jazz"), None);
    }

    #[test]
    fn sort_by_criteria_and_header_cycle() {
        let fav: HashSet<String> = ["b".to_string()].into_iter().collect();
        let mut v = vec![it("c", Some(90.0), Some("D"), false), it("a", Some(120.0), Some("C"), true), it("b", None, None, false)];
        sort_items(&mut v, "Name", &fav);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        sort_items(&mut v, "Tempo", &fav);
        assert_eq!(v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["c", "a", "b"]);
        sort_items(&mut v, "Key", &fav);
        assert_eq!(v[0].name, "a");
        sort_items(&mut v, "Favorites", &fav);
        assert_eq!(v[0].name, "b");
        sort_items(&mut v, "User Content", &fav);
        assert_eq!(v[0].name, "a");
        sort_items(&mut v, "Type", &fav);
        assert_eq!(v[0].name, "b");
        // a header: ascending, descending, then relevance
        assert_eq!(next_sort("Relevance", false, "Tempo"), ("Tempo".to_string(), false));
        assert_eq!(next_sort("Tempo", false, "Tempo"), ("Tempo".to_string(), true));
        assert_eq!(next_sort("Tempo", true, "Tempo"), ("Relevance".to_string(), false));
        assert_eq!(attr_text(&v[1], "Bars"), "4");
    }

    #[test]
    fn column_arrows_move_through_the_folders() {
        use egui::Key::*;
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let cols = vec![s(&["Bass", "Keys"]), s(&["Electric", "Synth"]), s(&["P1", "P2"])];
        // nothing open: ↓ opens the first category
        assert_eq!(super::column_key(&cols[..1], &[], None, ArrowDown), (s(&["Bass"]), None));
        // → into the category: its first subcategory; → again: the first patch
        assert_eq!(super::column_key(&cols[..2], &s(&["Bass"]), None, ArrowRight), (s(&["Bass", "Electric"]), None));
        assert_eq!(super::column_key(&cols, &s(&["Bass", "Electric"]), None, ArrowRight), (s(&["Bass", "Electric"]), Some(0)));
        // ↓ / ↑ among the patches
        assert_eq!(super::column_key(&cols, &s(&["Bass", "Electric"]), Some(0), ArrowDown).1, Some(1));
        // ← out of the patches, then out of the subcategory
        assert_eq!(super::column_key(&cols, &s(&["Bass", "Electric"]), Some(1), ArrowLeft), (s(&["Bass", "Electric"]), None));
        assert_eq!(super::column_key(&cols, &s(&["Bass", "Electric"]), None, ArrowLeft), (s(&["Bass"]), None));
        // ↓ in the subcategory column changes it
        assert_eq!(super::column_key(&cols, &s(&["Bass", "Electric"]), None, ArrowDown).0, s(&["Bass", "Synth"]));
    }

    use super::*;

    #[test]
    fn key_shifts_take_the_short_way() {
        assert_eq!(key_shift("A", "C"), 3);
        assert_eq!(key_shift("C", "A"), -3);
        assert_eq!(key_shift("F#", "C"), 6);
        assert_eq!(key_shift("B♭", "C"), 2);
        assert_eq!(key_shift("C", "C"), 0);
    }

    fn rss_mb() -> usize {
        std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find_map(|l| l.strip_prefix("VmRSS:").and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<usize>().ok()))).unwrap_or(0) / 1024
    }

    fn instruments_app() -> App {
        let mut app = crate::tm::test_app();
        app.selected_track = Some(1);
        app.show_browser = true;
        app.br.view = Cat::Instruments;
        ensure_loaded(&mut app);
        app
    }

    fn ids(app: &App) -> Vec<String> {
        results(app).into_iter().map(|i| i.id).collect()
    }

    /// Clicking (selecting, previewing, Replace Mode loading the patch) keeps the list's order.
    #[test]
    fn clicking_sounds_keeps_the_order() {
        let mut app = instruments_app();
        link_to_selection(&mut app);
        let before = ids(&app);
        assert!(before.len() > 40);
        for k in [35usize, 3, 50, 12, 35, 7, 60] {
            let it = results(&app)[k.min(before.len() - 1)].clone();
            let was = app.br.selected.first() == Some(&it.id);
            app.br.selected = vec![it.id.clone()];
            click_item(&mut app, &it, was);
            link_to_selection(&mut app);
            assert_eq!(ids(&app), before, "the order changed after clicking {}", it.name);
        }
    }

    /// Many previews in a row (kits, keys, synths): at most one renders at a time, nothing panics.
    #[test]
    fn many_previews_in_a_row() {
        let mut app = instruments_app();
        let m0 = rss_mb();
        let list = results(&app);
        let kits = list.iter().filter(|i| crate::brpreview::kit_named(&i.name)).take(10);
        let rest = list.iter().filter(|i| !crate::brpreview::kit_named(&i.name)).take(40);
        for it in kits.chain(rest).cloned().collect::<Vec<_>>() {
            // (a patch's first click only selects it; the second, on the highlighted patch, previews)
            app.br.selected = vec![it.id.clone()];
            click_item(&mut app, &it, false);
            click_item(&mut app, &it, true);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let t = std::time::Instant::now();
        while crate::preview::rendering() && t.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!crate::preview::rendering(), "the last preview never finished");
        assert!(crate::brpreview::most_at_once() <= 1, "{} previews rendered at once", crate::brpreview::most_at_once());
        eprintln!("RSS {m0} MB before, {} MB after 50 previews", rss_mb());
    }

    /// As Logic: the first click on an instrument patch selects it without a preview; a click on
    /// the highlighted patch plays it.
    #[test]
    fn a_patch_previews_on_its_second_click() {
        let mut app = instruments_app();
        let it = results(&app).into_iter().find(|i| i.kind == Kind::Patch && crate::brpreview::previewable(i)).unwrap();
        app.br.selected = vec![it.id.clone()];
        click_item(&mut app, &it, false);
        assert!(app.preview.playing.is_none(), "the first click previewed");
        click_item(&mut app, &it, true);
        assert_eq!(app.preview.playing.as_deref(), Some(it.id.as_str()));
    }

    #[test]
    fn loop_instruments_pick_a_patch() {
        let p = patch_for_tags(&["Bass".to_string()]).unwrap();
        assert!(patches().iter().any(|x| x.2 == p && x.0 == "Bass"));
        assert!(patch_for_tags(&["Drums".to_string()]).is_some());
    }
}
