//! The window layouts a Logic project keeps (Alternatives/NNN/DisplayState.plist): its
//! screensets (the slot number, name, lock) and, for each, the main window's panes (research:
//! format-analysis/DISPLAYSTATE.txt). Reading only; Studio keeps edits in the app.

use std::path::Path;

use plist::Value;

/// The bottom editor a screenset shows (docwEditorTabIndex).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Editor {
    None,
    Mixer,
    /// An audio region's editor (1).
    Audio,
    PianoRoll,
    SmartControls,
    /// The Session Player / Drummer editor (6).
    SessionPlayer,
    Other(i64),
}

impl Editor {
    pub fn from_index(i: i64) -> Editor {
        match i {
            -1 => Editor::None,
            0 => Editor::Mixer,
            1 => Editor::Audio,
            2 => Editor::PianoRoll,
            5 => Editor::SmartControls,
            6 => Editor::SessionPlayer,
            n => Editor::Other(n),
        }
    }
}

/// One screenset's main window as saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Screenset {
    /// Logic's number: 1-9 and 11-99 (recalled by the digit keys).
    pub slot: u32,
    pub name: String,
    pub locked: bool,
    pub inspector: Option<bool>,
    /// The Browser (Logic calls it the library).
    pub browser: Option<bool>,
    /// The List Editors pane.
    pub lists: Option<bool>,
    pub control_bar: Option<bool>,
    pub toolbar: Option<bool>,
    pub editor: Option<Editor>,
    /// The bottom pane's height in % of the main area, for the Mixer and for the other editors.
    pub mixer_percent: Option<f64>,
    pub other_percent: Option<f64>,
}

/// The project's screensets and the one shown when it was saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DisplayState {
    pub screensets: Vec<Screenset>,
    pub current: u32,
}

/// Reads `alternative`/DisplayState.plist; None when there is none (or it can't be read).
pub fn read(alternative: &Path) -> Option<DisplayState> {
    parse(&Value::from_file(alternative.join("DisplayState.plist")).ok()?)
}

pub fn parse(v: &Value) -> Option<DisplayState> {
    let d = v.as_dictionary()?;
    let current = d.get("screensetCurrSlot").and_then(|x| x.as_signed_integer()).unwrap_or(1).max(1) as u32;
    let mut out = DisplayState { screensets: Vec::new(), current };
    for s in d.get("screensetDictArray").and_then(|x| x.as_array()).map_or(&[][..], |a| a.as_slice()) {
        let Some(s) = s.as_dictionary() else { continue };
        let slot = s.get("screensetSlot").and_then(|x| x.as_signed_integer()).unwrap_or(0);
        if slot <= 0 {
            continue;
        }
        // the main window's state: the first layout with one (Logic's window type 1)
        let ws = s.get("layoutDictArray").and_then(|x| x.as_array()).into_iter().flatten().filter_map(|l| l.as_dictionary()).find_map(|l| l.get("docwWindowState").and_then(|w| w.as_dictionary()));
        let flag = |k: &str| ws.and_then(|w| w.get(k)).and_then(|x| x.as_boolean());
        let real = |k: &str| ws.and_then(|w| w.get(k)).and_then(|x| x.as_real());
        out.screensets.push(Screenset {
            slot: slot as u32,
            name: s.get("screensetName").and_then(|x| x.as_string()).unwrap_or("").to_string(),
            locked: s.get("screensetLocked").and_then(|x| x.as_boolean()).unwrap_or(false),
            inspector: flag("inspectorIsVisible"),
            browser: flag("libraryIsVisible"),
            lists: flag("listsAreVisible"),
            control_bar: flag("transportBarIsVisible"),
            toolbar: flag("toolbarIsVisible"),
            editor: ws.and_then(|w| w.get("docwEditorTabIndex")).and_then(|x| x.as_signed_integer()).map(Editor::from_index),
            mixer_percent: real("tabViewPercentMixer"),
            other_percent: real("tabViewPercentOther"),
        });
    }
    out.screensets.sort_by_key(|s| s.slot);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::Dictionary;

    fn screenset(slot: i64, name: &str, editor: i64, inspector: bool) -> Value {
        let mut ws = Dictionary::new();
        ws.insert("inspectorIsVisible".into(), Value::Boolean(inspector));
        ws.insert("libraryIsVisible".into(), Value::Boolean(false));
        ws.insert("docwEditorTabIndex".into(), Value::Integer(editor.into()));
        ws.insert("tabViewPercentMixer".into(), Value::Real(52.5));
        let mut layout = Dictionary::new();
        layout.insert("docwWindowType".into(), Value::Integer(1.into()));
        layout.insert("docwWindowState".into(), Value::Dictionary(ws));
        let mut s = Dictionary::new();
        s.insert("layoutDictArray".into(), Value::Array(vec![Value::Dictionary(layout)]));
        s.insert("screensetName".into(), Value::String(name.into()));
        s.insert("screensetLocked".into(), Value::Boolean(false));
        s.insert("screensetSlot".into(), Value::Integer(slot.into()));
        Value::Dictionary(s)
    }

    #[test]
    fn screensets_with_their_panes() {
        let mut d = Dictionary::new();
        d.insert("screensetCurrSlot".into(), Value::Integer(2.into()));
        d.insert("screensetDictArray".into(), Value::Array(vec![screenset(5, "Mix", 0, false), screenset(1, "", 2, true)]));
        let s = parse(&Value::Dictionary(d)).unwrap();
        assert_eq!(s.current, 2);
        assert_eq!(s.screensets.iter().map(|x| x.slot).collect::<Vec<_>>(), [1, 5]);
        assert_eq!(s.screensets[1].name, "Mix");
        assert_eq!(s.screensets[1].editor, Some(Editor::Mixer));
        assert_eq!(s.screensets[0].editor, Some(Editor::PianoRoll));
        assert_eq!(s.screensets[0].inspector, Some(true));
        assert_eq!(s.screensets[0].browser, Some(false));
        assert_eq!(s.screensets[0].mixer_percent, Some(52.5));
    }

    /// The research's samples, when this checkout has them: every project has screenset 1; the
    /// Songwriter template has five named ones.
    #[test]
    fn research_samples() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata-private/sample-files");
        let main = std::env::var("HOME").map(|h| Path::new(&h).join("CascadeProjects/bramble-office/logic-linux/testdata-private/sample-files")).ok();
        let Some(dir) = [Some(here), main].into_iter().flatten().find(|d| d.is_dir()) else { return };
        let e23 = read(&dir.join("E23-live-loops-base.logicx/Alternatives/000")).unwrap();
        assert_eq!(e23.current, 1);
        assert_eq!(e23.screensets.len(), 1);
        assert_eq!(e23.screensets[0].browser, Some(false));
        assert_eq!(e23.screensets[0].editor, Some(Editor::PianoRoll));
        let alt = dir.join("templates-reference/03 Songwriter.logicx/Alternatives/000");
        if let Some(t) = read(&alt) {
            assert_eq!(t.screensets.len(), 5);
            assert_eq!(t.screensets[4].slot, 5);
            assert!(!t.screensets[4].name.is_empty());
            assert_eq!(t.screensets[4].editor, Some(Editor::Mixer));
            assert_eq!(t.screensets[4].inspector, Some(false));
            assert_eq!(t.screensets[1].lists, Some(true));
        }
    }
}
