//! Modal dialogs on control surface displays (Logic's "modal dialog display"): while a dialog
//! is open its text shows on the Mackie LCD's upper row (scrolling after three seconds when it
//! doesn't fit), its buttons on the lower row, one per strip; Enter triggers the default button,
//! Cancel the cancel button, a strip's Select or V-Pot push the button above it. When the dialog
//! closes the display returns to what it showed. Dialogs take part by calling `offer` each
//! frame they are shown.

use std::cell::RefCell;
use std::time::Instant;

pub const COMMANDS: &[&str] = &[];

#[derive(Clone, Debug)]
struct Dlg {
    text: String,
    buttons: Vec<String>,
    default: usize,
    cancel: Option<usize>,
    since: Instant,
    /// Surface frames since the dialog last offered itself (gone after a few).
    age: u8,
}

thread_local! {
    static CUR: RefCell<Option<Dlg>> = const { RefCell::new(None) };
    static PRESSED: RefCell<Option<usize>> = const { RefCell::new(None) };
}

/// A dialog shown this frame: its text and buttons (`default` for Enter, `cancel` for Cancel).
/// Returns the button pressed on a surface, if one was.
pub fn offer(text: &str, buttons: &[&str], default: usize, cancel: Option<usize>) -> Option<usize> {
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_mut() {
            Some(d) if d.text == text => d.age = 0,
            _ => *c = Some(Dlg { text: text.to_string(), buttons: buttons.iter().map(|s| s.to_string()).collect(), default, cancel, since: Instant::now(), age: 0 }),
        }
    });
    PRESSED.with(|p| p.borrow_mut().take())
}

pub fn active() -> bool {
    CUR.with(|c| c.borrow().is_some())
}

/// Each surface frame: a dialog no longer offered is gone.
pub fn tick() {
    CUR.with(|c| {
        let mut c = c.borrow_mut();
        if let Some(d) = c.as_mut() {
            d.age += 1;
            if d.age > 3 {
                *c = None;
            }
        }
    });
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Press {
    Enter,
    Cancel,
    Strip(usize),
}

/// A surface button while a dialog is open: true when the dialog took it.
pub fn press(p: Press) -> bool {
    let Some(d) = CUR.with(|c| c.borrow().clone()) else { return false };
    let k = match p {
        Press::Enter => Some(d.default),
        Press::Cancel => d.cancel,
        Press::Strip(k) => (k < d.buttons.len()).then_some(k),
    };
    if let Some(k) = k {
        PRESSED.with(|x| *x.borrow_mut() = Some(k));
        CUR.with(|c| *c.borrow_mut() = None);
    }
    true
}

/// The upper row's 56 characters `secs` after the dialog opened: the start, then scrolling
/// (four characters a second) when the text is longer.
pub fn scrolled(text: &str, secs: f32) -> String {
    let chars: Vec<char> = text.chars().filter(|c| c.is_ascii()).collect();
    if chars.len() <= 56 || secs < 3.0 {
        let t: String = chars.iter().take(56).collect();
        return format!("{t:<56}");
    }
    let looped: Vec<char> = chars.iter().copied().chain("    ".chars()).collect();
    let off = ((secs - 3.0) * 4.0) as usize % looped.len();
    looped.iter().cycle().skip(off).take(56).collect()
}

/// The LCD's rows while a dialog is open.
pub fn lcd() -> Option<(String, String)> {
    let d = CUR.with(|c| c.borrow().clone())?;
    let top = scrolled(&d.text, d.since.elapsed().as_secs_f32());
    let bottom: String = (0..8).map(|k| lg_core::ctrlsurf::lcd_cell(d.buttons.get(k).map_or("", |s| s.as_str()))).collect();
    Some((top, bottom))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_on_the_display() {
        assert!(offer("The track is locked.", &["OK"], 0, Some(0)).is_none());
        let (top, bottom) = lcd().unwrap();
        assert!(top.starts_with("The track is locked.") && top.len() == 56);
        assert!(bottom.starts_with("OK     "));
        // Enter: the default button
        assert!(press(Press::Enter));
        assert_eq!(offer("The track is locked.", &["OK"], 0, Some(0)), Some(0));
        // two buttons: the strip below picks; Cancel the cancel one
        offer("Use the new device?", &["Use", "Ignore"], 0, Some(1));
        assert!(press(Press::Cancel));
        assert_eq!(offer("Use the new device?", &["Use", "Ignore"], 0, Some(1)), Some(1));
        offer("Use the new device?", &["Use", "Ignore"], 0, Some(1));
        assert!(press(Press::Strip(0)));
        assert_eq!(offer("x", &["Use"], 0, None), Some(0));
        // gone when no longer offered: the display returns
        for _ in 0..5 {
            tick();
        }
        assert!(lcd().is_none());
        assert!(!press(Press::Enter));
    }

    #[test]
    fn long_text_scrolls_after_three_seconds() {
        let long = "A".repeat(40) + &"B".repeat(40);
        assert!(scrolled(&long, 1.0).starts_with("AAAA"));
        assert_eq!(scrolled(&long, 2.9), scrolled(&long, 0.0));
        assert_ne!(scrolled(&long, 6.0), scrolled(&long, 0.0));
        assert_eq!(scrolled("short", 10.0).trim_end(), "short");
    }
}
