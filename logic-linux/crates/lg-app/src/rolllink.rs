//! Window linking (View ▸ Link) for the Piano Roll, the Score and the Event List. Linked
//! (Content, Logic's default) an editor shows the region selected in the Tracks area; unlinked
//! (Off) it keeps showing the region it shows while the selection changes elsewhere. The Piano
//! Roll's View menu has a single Link item (Content; Same Level isn't available there); the
//! Score's and Event List's View ▸ Link offer Off, Same Level and Content (the Score's Link
//! button shows it: purple Same Level, yellow Content; scoretools.rs). Same Level in the Event
//! List shows the level above the region: the regions.

use std::cell::RefCell;
use std::sync::Mutex;

use crate::app::App;

pub const COMMANDS: &[&str] = &["Link"];

/// Each editor's link: 0 Off, 1 Same Level, 2 Content; and the region it keeps while Off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Links {
    pub roll: u8,
    pub roll_pin: Option<(usize, usize)>,
    pub events: u8,
    pub events_pin: Option<(usize, usize)>,
}

impl Default for Links {
    fn default() -> Self {
        Links { roll: 2, roll_pin: None, events: 2, events_pin: None }
    }
}

static LINKS: Mutex<Links> = Mutex::new(Links { roll: 2, roll_pin: None, events: 2, events_pin: None });

thread_local! {
    /// The local menu being drawn (its View ▸ Link submenu acts on that editor).
    static MENU: RefCell<String> = const { RefCell::new(String::new()) };
}

pub fn links() -> Links {
    *LINKS.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set(f: impl FnOnce(&mut Links)) {
    f(&mut LINKS.lock().unwrap_or_else(|e| e.into_inner()));
}

/// menus::local_bar: which local menu is open.
pub fn set_menu(name: &str) {
    MENU.with(|m| *m.borrow_mut() = name.to_string());
}

pub fn command(app: &mut App, name: &str) -> bool {
    let _ = app;
    if name != "Link" {
        return false;
    }
    set(|l| l.roll = if l.roll == 0 { 2 } else { 0 });
    true
}

pub fn checked(_app: &App, label: &str) -> Option<bool> {
    (label == "Link").then(|| links().roll != 0)
}

/// The region an editor shows: the selection when linked, else the one it kept.
pub fn shown(sel: Option<(usize, usize)>, mode: u8, pin: &mut Option<(usize, usize)>) -> Option<(usize, usize)> {
    if mode != 0 {
        *pin = None;
        return sel;
    }
    if pin.is_none() {
        *pin = sel;
    }
    *pin
}

/// Before an editor draws: with Link Off, the app's selected region is the editor's own for the
/// time it draws (returns the real selection, for `leave`).
pub fn enter(app: &mut App, events: bool) -> Option<Option<(usize, usize)>> {
    let mut l = links();
    let (mode, pin) = if events { (l.events, &mut l.events_pin) } else { (l.roll, &mut l.roll_pin) };
    let n_ok = |k: (usize, usize)| app.project.as_ref().and_then(|p| p.tracks.get(k.0)).is_some_and(|t| k.1 < t.regions.len());
    if pin.is_some_and(|k| !n_ok(k)) {
        *pin = None;
    }
    let show = shown(app.selected_region, mode, pin);
    set(|x| *x = l);
    if mode != 0 || show == app.selected_region {
        return None;
    }
    let real = app.selected_region;
    app.selected_region = show;
    Some(real)
}

/// After it drew: what it chose is kept, the app's selection is back.
pub fn leave(app: &mut App, events: bool, real: Option<Option<(usize, usize)>>) {
    let Some(real) = real else { return };
    let now = app.selected_region;
    set(|l| if events { l.events_pin = now } else { l.roll_pin = now });
    app.selected_region = real;
}

/// View ▸ Link ▸ Off / Same Level / Content in the Score and Event List menus.
pub fn menu_ui(_app: &mut App, ui: &mut egui::Ui, it: &crate::menus::Item) -> bool {
    if it.label != "Link" || it.children.is_empty() {
        return false;
    }
    let menu = MENU.with(|m| m.borrow().clone());
    let score = menu == "score-View";
    if !score && menu != "eventlist-View" {
        return false;
    }
    let cur = if score { crate::score::state(ui.ctx()).link } else { links().events };
    ui.menu_button("Link", |ui| {
        for (k, name) in ["Off", "Same Level", "Content"].iter().enumerate() {
            if ui.button(if cur == k as u8 { format!("✓ {name}") } else { name.to_string() }).clicked() {
                if score {
                    crate::score::with_state(ui.ctx(), |s| s.link = k as u8);
                } else {
                    set(|l| l.events = k as u8);
                }
                ui.close();
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlinked_editors_keep_their_region() {
        let mut pin = None;
        // linked: follows the selection
        assert_eq!(shown(Some((0, 1)), 2, &mut pin), Some((0, 1)));
        // Off: keeps what it showed when unlinked, whatever is selected then
        assert_eq!(shown(Some((0, 1)), 0, &mut pin), Some((0, 1)));
        assert_eq!(shown(Some((2, 0)), 0, &mut pin), Some((0, 1)));
        // linked again: back to the selection
        assert_eq!(shown(Some((2, 0)), 2, &mut pin), Some((2, 0)));
        assert_eq!(pin, None);
    }

    #[test]
    fn roll_link_off_swaps_the_selection_while_it_draws() {
        let mut app = crate::tm::test_app();
        set(|l| *l = Links::default());
        let regions: Vec<(usize, usize)> = app.project.as_ref().unwrap().tracks.iter().enumerate().flat_map(|(t, tr)| (0..tr.regions.len()).map(move |r| (t, r))).take(2).collect();
        assert_eq!(regions.len(), 2);
        app.selected_region = Some(regions[0]);
        assert!(command(&mut app, "Link"));
        assert_eq!(checked(&app, "Link"), Some(false));
        assert_eq!(enter(&mut app, false), None);
        // another region selected in the Tracks area: the Piano Roll still draws the first
        app.selected_region = Some(regions[1]);
        let real = enter(&mut app, false);
        assert_eq!(app.selected_region, Some(regions[0]));
        leave(&mut app, false, real);
        assert_eq!(app.selected_region, Some(regions[1]));
        command(&mut app, "Link");
        assert_eq!(enter(&mut app, false), None);
        set(|l| *l = Links::default());
    }
}
