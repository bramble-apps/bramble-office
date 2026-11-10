//! Logic Pro's key commands (U.S. preset, data/keys.txt from the research's key-commands.txt):
//! a key press runs the command Logic binds to it in the focused area (the Piano Roll, the
//! Mixer or the tracks), then the window-wide groups. Commands Studio has are run by their
//! Logic names (App::key_command); the others do nothing yet, as unassigned keys do.
//!
//! On Linux and Windows ⌘ is Ctrl; Logic's ⌃ bindings would collide with the ⌘ ones there, so
//! they're left out (as other ports do) until a key command editor lets people choose.

use std::sync::OnceLock;

use egui::{Key, Modifiers};

#[derive(Clone, Debug)]
pub struct Binding {
    pub area: String,
    pub command: String,
    pub key: Key,
    pub cmd: bool,
    pub alt: bool,
    pub shift: bool,
}

fn key_of(c: char) -> Option<Key> {
    Some(match c {
        'A'..='Z' => Key::from_name(&c.to_string())?,
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        '9' => Key::Num9,
        ',' => Key::Comma,
        '.' => Key::Period,
        '/' => Key::Slash,
        ';' => Key::Semicolon,
        '=' => Key::Equals,
        '-' => Key::Minus,
        '[' => Key::OpenBracket,
        ']' => Key::CloseBracket,
        '\\' => Key::Backslash,
        '`' => Key::Backtick,
        '\'' => Key::Quote,
        '˽' => Key::Space,
        '↩' => Key::Enter,
        '⌫' => Key::Backspace,
        '⌦' => Key::Delete,
        '←' => Key::ArrowLeft,
        '→' => Key::ArrowRight,
        '↑' => Key::ArrowUp,
        '↓' => Key::ArrowDown,
        '⇥' => Key::Tab,
        '⤒' => Key::Home,
        '⤓' => Key::End,
        '⇞' => Key::PageUp,
        '⇟' => Key::PageDown,
        _ => return None,
    })
}

fn parse() -> Vec<Binding> {
    let mut out = Vec::new();
    for line in include_str!("../data/keys.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        let (Some(area), Some(command), Some(spec)) = (it.next(), it.next(), it.next()) else { continue };
        if spec.contains('⌃') {
            continue;
        }
        let rest: String = spec.chars().filter(|c| !"⌘⌥⇧".contains(*c)).collect();
        let mut chars = rest.chars();
        let (Some(c), None) = (chars.next(), chars.next()) else { continue };
        let Some(key) = key_of(c) else { continue };
        out.push(Binding { area: area.to_string(), command: command.to_string(), key, cmd: spec.contains('⌘'), alt: spec.contains('⌥'), shift: spec.contains('⇧') });
    }
    out
}

pub fn bindings() -> &'static [Binding] {
    static B: OnceLock<Vec<Binding>> = OnceLock::new();
    B.get_or_init(parse)
}

/// Which areas' bindings apply, most specific first.
pub fn areas(focus: Focus) -> &'static [&'static str] {
    match focus {
        Focus::PianoRoll => &["Piano Roll", "Various Editors", "Main Window Tracks and Various Editors", "Various Windows", "Global Commands"],
        Focus::Mixer => &["Mixer", "Various Windows", "Global Commands"],
        Focus::Tracks => &["Main Window Tracks", "Main Window Tracks and Various Editors", "Various Windows", "Global Commands"],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Tracks,
    PianoRoll,
    Mixer,
}

/// The commands bound to a key press, in the order they should be tried.
pub fn commands_for(key: Key, m: Modifiers, focus: Focus) -> Vec<&'static str> {
    let mut out = Vec::new();
    for area in areas(focus) {
        for b in bindings().iter().filter(|b| b.area == *area) {
            if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift {
                out.push(b.command.as_str());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_keys_map_to_commands() {
        assert!(bindings().len() > 500, "{}", bindings().len());
        let none = Modifiers::NONE;
        assert_eq!(commands_for(Key::Space, none, Focus::Tracks).first(), Some(&"Play or Stop"));
        assert_eq!(commands_for(Key::X, none, Focus::Tracks).first(), Some(&"Show/Hide Mixer"));
        assert!(commands_for(Key::C, none, Focus::Tracks).contains(&"Toggle Cycle Mode"));
        assert!(commands_for(Key::K, none, Focus::Tracks).contains(&"Toggle Metronome Click"));
        // ⌘← in the tracks: zoom out
        assert!(commands_for(Key::ArrowLeft, Modifiers::COMMAND, Focus::Tracks).contains(&"Zoom Horizontal Out"));
        // ← selects the previous region in the tracks, the previous channel strip in the Mixer
        assert!(commands_for(Key::ArrowLeft, none, Focus::Mixer).contains(&"Select Previous (Left) Channel Strip"));
    }
}
