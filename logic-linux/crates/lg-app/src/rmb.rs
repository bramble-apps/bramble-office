//! Settings ▸ General ▸ Editing ▸ Right Mouse Button: Opens Shortcut Menu (the default), Opens
//! Tool Menu (a right-click shows the Tool menu with the Pointer tool under the pointer; a
//! tool's key picks it), Opens Tool and Shortcut Menu (the tools above the shortcut menu's
//! items).

use crate::app::App;

pub const COMMANDS: &[&str] = &[];

pub const KEY: &str = "General / Editing / Right Mouse Button:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rmb {
    Assignable,
    ToolMenu,
    Shortcut,
    ToolAndShortcut,
}

pub fn parse(s: &str) -> Rmb {
    match s {
        "Is Assignable to a Tool" => Rmb::Assignable,
        "Opens Tool Menu" => Rmb::ToolMenu,
        "Opens Tool and Shortcut Menu" => Rmb::ToolAndShortcut,
        _ => Rmb::Shortcut,
    }
}

/// Where tools apply (the Tracks area's lanes and regions, the Piano Roll).
pub fn tool_place(place: &str) -> bool {
    place.starts_with("region") || place.starts_with("tracks-area") || (place.starts_with("pianoroll") && place != "pianoroll-keyboard")
}

/// The setting for a right-click at `place`.
pub fn mode(app: &App, place: &str) -> Rmb {
    if tool_place(place) { parse(&app.logic_settings.get(KEY)) } else { Rmb::Shortcut }
}

/// The tool a key typed in the open Tool menu picks ("T", "P" …; single letters only).
pub fn tool_for_key<T: Copy>(tools: &[T], key_of: impl Fn(T) -> &'static str, typed: &str) -> Option<T> {
    tools.iter().copied().find(|t| key_of(*t).eq_ignore_ascii_case(typed))
}

/// Opens Tool and Shortcut Menu: the tools, above the shortcut menu's items.
pub fn tools_section(app: &mut App, ui: &mut egui::Ui, roll: bool) {
    use crate::edit::Tool;
    let tools: &[Tool] = if roll { &Tool::PIANO_ROLL } else { &Tool::TRACKS };
    let cur = if roll { app.roll_tool } else { app.tool };
    if let Some(t) = crate::rolltools::menu_rows(ui, tools, cur) {
        if roll {
            app.roll_tool = t;
        } else {
            app.tool = t;
        }
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_and_keys() {
        assert_eq!(parse("Opens Tool Menu"), Rmb::ToolMenu);
        assert_eq!(parse("Opens Shortcut Menu"), Rmb::Shortcut);
        assert_eq!(parse("Opens Tool and Shortcut Menu"), Rmb::ToolAndShortcut);
        assert!(tool_place("region-midi") && tool_place("pianoroll-note") && !tool_place("track-header"));
        let tools = [crate::edit::Tool::Pointer, crate::edit::Tool::Pencil, crate::edit::Tool::Eraser];
        assert_eq!(tool_for_key(&tools, |t| t.key(), "p"), Some(crate::edit::Tool::Pencil));
        assert_eq!(tool_for_key(&tools, |t| t.key(), "q"), None);
    }
}
