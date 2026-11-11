//! Logic Pro's key commands (U.S. preset, data/keys.txt from the research's key-commands.txt):
//! a key press runs the command Logic binds to it in the focused area (the Piano Roll, the
//! Mixer or the tracks), then the window-wide groups. Commands Studio has are run by their
//! Logic names (App::key_command); the others do nothing yet, as unassigned keys do.
//!
//! On Linux and Windows ⌘ is Ctrl, and so is Logic's ⌃: a Ctrl press first tries the ⌘
//! bindings, then the ⌃ ones (the first command Studio runs wins). The Tool Menu's letters
//! apply only while the T menu is open.

use std::sync::{OnceLock, RwLock};

use egui::{Key, Modifiers};

#[derive(Clone, Debug)]
pub struct Binding {
    pub area: String,
    pub command: String,
    pub key: Key,
    pub cmd: bool,
    pub alt: bool,
    pub shift: bool,
    /// Logic binds it with ⌃ (Control): tried after the ⌘ bindings.
    pub ctrl: bool,
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

/// A key spec as Logic writes it ("⌥⇧K", "⌘←", "˽"): the key and its modifiers (⌃ counts as
/// Ctrl, as ⌘ does). None: a key Studio can't press (keypad-only keys such as ⌅, or none).
pub fn parse_spec(spec: &str) -> Option<(Key, bool, bool, bool, bool)> {
    let rest: String = spec.chars().filter(|c| !"⌘⌥⇧⌃".contains(*c)).collect();
    let mut chars = rest.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return None };
    let key = key_of(c)?;
    let ctrl = spec.contains('⌃');
    Some((key, spec.contains('⌘') || ctrl, spec.contains('⌥'), spec.contains('⇧'), ctrl))
}

/// The spec Logic shows for a key press (⌃⌥⇧⌘ order, as its Key Commands window).
pub fn spec_of(key: Key, m: Modifiers) -> Option<String> {
    let c = match key {
        Key::Num0 => '0',
        Key::Num1 => '1',
        Key::Num2 => '2',
        Key::Num3 => '3',
        Key::Num4 => '4',
        Key::Num5 => '5',
        Key::Num6 => '6',
        Key::Num7 => '7',
        Key::Num8 => '8',
        Key::Num9 => '9',
        Key::Comma => ',',
        Key::Period => '.',
        Key::Slash => '/',
        Key::Semicolon => ';',
        Key::Equals => '=',
        Key::Minus => '-',
        Key::OpenBracket => '[',
        Key::CloseBracket => ']',
        Key::Backslash => '\\',
        Key::Backtick => '`',
        Key::Quote => '\'',
        Key::Space => '˽',
        Key::Enter => '↩',
        Key::Backspace => '⌫',
        Key::Delete => '⌦',
        Key::ArrowLeft => '←',
        Key::ArrowRight => '→',
        Key::ArrowUp => '↑',
        Key::ArrowDown => '↓',
        Key::Tab => '⇥',
        Key::Home => '⤒',
        Key::End => '⤓',
        Key::PageUp => '⇞',
        Key::PageDown => '⇟',
        k => {
            let n = k.name();
            let mut it = n.chars();
            match (it.next(), it.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() => c.to_ascii_uppercase(),
                _ => return None,
            }
        }
    };
    let mut s = String::new();
    // (Ctrl is ⌘ here: Linux and Windows have no separate Command key)
    if m.alt {
        s.push('⌥');
    }
    if m.shift {
        s.push('⇧');
    }
    if m.command || m.ctrl || m.mac_cmd {
        s.push('⌘');
    }
    s.push(c);
    Some(s)
}

/// Every Logic key command, assigned or not (data/commands.txt): (area, command, U.S. key).
pub fn all_commands() -> &'static [(String, String, String)] {
    static C: OnceLock<Vec<(String, String, String)>> = OnceLock::new();
    C.get_or_init(|| {
        include_str!("../data/commands.txt")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("None\t"))
            .filter_map(|l| {
                let mut it = l.split('\t');
                Some((it.next()?.to_string(), it.next()?.to_string(), it.next().unwrap_or("").trim().to_string()))
            })
            .collect()
    })
}

/// The bindings a set of (area, command, key spec) makes: specs Studio can't press are left out.
pub fn bindings_of<'a>(assigned: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> Vec<Binding> {
    let mut out = Vec::new();
    for (area, command, spec) in assigned {
        let Some((key, cmd, alt, shift, ctrl)) = parse_spec(spec) else { continue };
        out.push(Binding { area: area.to_string(), command: command.to_string(), key, cmd, alt, shift, ctrl });
    }
    out
}

fn parse() -> Vec<Binding> {
    let mut out = Vec::new();
    for line in include_str!("../data/keys.txt").lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut it = line.split('\t');
        let (Some(area), Some(command), Some(spec)) = (it.next(), it.next(), it.next()) else { continue };
        out.extend(bindings_of([(area, command, spec)]));
    }
    out
}

/// Logic's U.S. preset, as shipped.
pub fn defaults() -> &'static [Binding] {
    static B: OnceLock<Vec<Binding>> = OnceLock::new();
    B.get_or_init(parse)
}

/// The key command set in use (the Key Commands window edits it): the U.S. defaults until a set
/// is loaded or edited.
static CURRENT: RwLock<Option<&'static [Binding]>> = RwLock::new(None);

pub fn bindings() -> &'static [Binding] {
    CURRENT.read().ok().and_then(|c| *c).unwrap_or_else(defaults)
}

/// Makes `b` the set every key press reads. (Each set lives for the rest of the run: sets change
/// only when the Key Commands window edits or loads one.)
pub fn set_bindings(b: Vec<Binding>) {
    let leaked: &'static [Binding] = Box::leak(b.into_boxed_slice());
    if let Ok(mut c) = CURRENT.write() {
        *c = Some(leaked);
    }
}

/// Which areas' bindings apply, most specific first.
pub fn areas(focus: Focus) -> &'static [&'static str] {
    match focus {
        Focus::PianoRoll => &["Piano Roll", "Various Editors", "Main Window Tracks and Various Editors", "Views Showing Time Ruler", "Views Showing Automation", "Various Windows", "Global Commands"],
        Focus::Mixer => &["Mixer", "Various Windows", "Global Commands"],
        Focus::Browser => &["Browser", "Various Windows", "Global Commands"],
        Focus::Score => &["Score Editor", "Various Editors", "Main Window Tracks and Various Editors", "Views Showing Time Ruler", "Various Windows", "Global Commands"],
        Focus::StepSeq => &["Step Sequencer", "Various Windows", "Global Commands"],
        Focus::StepEditor => &["Step Editor", "Various Editors", "Main Window Tracks and Various Editors", "Views Showing Time Ruler", "Various Windows", "Global Commands"],
        Focus::EventList => &["Event Editor", "Various Editors", "Main Window Tracks and Various Editors", "Various Windows", "Global Commands"],
        Focus::Tracks => &["Main Window Tracks", "Main Window Tracks and Various Editors", "Views Showing Time Ruler", "Views Showing Automation", "Windows Showing Audio Files", "Various Windows", "Global Commands"],
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Tracks,
    PianoRoll,
    Mixer,
    /// The pointer over the Browser: its own keys (↩ New Track with Selected Item…) first.
    Browser,
    /// The Score, the Step Sequencer, the Step Editor window and the Event List (List Editors).
    Score,
    StepSeq,
    StepEditor,
    EventList,
}

/// The Tool Menu's letter for a tool (T, then the letter).
pub fn tool_for(key: Key, shift: bool) -> Option<&'static str> {
    bindings().iter().find(|b| b.area == "Tool Menu" && b.key == key && b.shift == shift && !b.cmd && !b.alt).map(|b| b.command.as_str())
}

/// The commands bound to a key press, in the order they should be tried.
pub fn commands_for(key: Key, m: Modifiers, focus: Focus) -> Vec<&'static str> {
    let mut out = Vec::new();
    for ctrl in [false, true] {
        for area in areas(focus) {
            for b in bindings().iter().filter(|b| b.area == *area && b.ctrl == ctrl) {
                if b.key == key && b.cmd == m.command && b.alt == m.alt && b.shift == m.shift {
                    out.push(b.command.as_str());
                }
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
