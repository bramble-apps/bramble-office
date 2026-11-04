//! Edit > Spelling and Grammar: Check Document Now (the next misspelled word after the
//! selection, selected; around to the top) and Show Spelling and Grammar (the panel: the word,
//! its guesses, Change, Find Next, Ignore, Learn), on the same dictionaries as the red dots.

use super::{Editor, Loc};

/// Paragraphs' misspelled words checked on the workers: (text, byte ranges).
static CHECKED: std::sync::Mutex<Vec<(u64, String, Vec<(usize, usize)>)>> = std::sync::Mutex::new(Vec::new());
/// Bumped when a word is learned or ignored: checks from before it are dropped.
static GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Forgets the checked paragraphs (a word learned or ignored).
pub fn forget(view: &mut super::input::View) {
    GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    view.spell.clear();
    view.spell_asked.clear();
}

/// Checks a paragraph's spelling off the UI thread (once per text); its dots come with the
/// frame after.
pub fn check_later(view: &mut super::input::View, text: &str, ctx: &egui::Context) {
    if kn_ui::spell::dicts().is_none() || !view.spell_asked.insert(text.to_string()) {
        return;
    }
    let (t, ctx, generation) = (text.to_string(), ctx.clone(), GEN.load(std::sync::atomic::Ordering::Relaxed));
    super::media::spawn_job(Box::new(move || {
        let m = kn_ui::spell::misspelled(&t);
        CHECKED.lock().unwrap_or_else(|e| e.into_inner()).push((generation, t, m));
        ctx.request_repaint();
    }));
}

/// Takes in the spelling checked since the last frame.
pub fn poll(view: &mut super::input::View) {
    let now = GEN.load(std::sync::atomic::Ordering::Relaxed);
    for (generation, t, m) in std::mem::take(&mut *CHECKED.lock().unwrap_or_else(|e| e.into_inner())) {
        if generation == now {
            view.spell_asked.remove(&t);
            view.spell.insert(t, m);
        }
    }
    // (an edited note leaves many old texts behind: forgotten past a few thousand)
    if view.spell.len() > 4000 {
        view.spell.clear();
    }
}

impl Editor {
    /// The next misspelled word from the selection's end (round to the top once); selected.
    /// False when the note has none.
    pub fn check_document_now(&mut self) -> bool {
        if kn_ui::spell::dicts().is_none() {
            return false;
        }
        let start = if (self.head.para, self.head.off) >= (self.anchor.para, self.anchor.off) { self.head } else { self.anchor };
        let n = self.doc.paras.len();
        for k in 0..=n {
            let i = (start.para + k) % n.max(1);
            let p = &self.doc.paras[i];
            if self.doc.block_of(p).is_some() {
                continue;
            }
            let from = if k == 0 && start.cell.is_none() { start.off } else { 0 };
            // (the first paragraph again at the end: only what's before where it started)
            let to = if k == n { start.off } else { p.text.len() };
            let words = self.view.spell.entry(p.text.clone()).or_insert_with(|| kn_ui::spell::misspelled(&p.text)).clone();
            if let Some(&(a, b)) = words.iter().find(|(a, b)| *a >= from && *b <= to && p.fmt_of(*a).is_none_or(|f| f.att.is_none() && f.link.is_none())) {
                self.anchor = Loc { cell: None, para: i, off: a };
                self.head = Loc { cell: None, para: i, off: b };
                self.view.scroll_to_caret = true;
                return true;
            }
        }
        false
    }

    /// The selected word (the panel's), if a single word is selected.
    fn selected_word(&self) -> Option<String> {
        let (a, b) = (self.anchor, self.head);
        if a.cell.is_some() || a.para != b.para || a.off == b.off {
            return None;
        }
        let (a, b) = (a.off.min(b.off), a.off.max(b.off));
        Some(self.doc.paras[self.head.para].text[a..b].to_string())
    }
}

/// The Spelling and Grammar panel, while open.
pub fn panel(ed: &mut Editor, ctx: &egui::Context) {
    if !ed.view.spell_panel {
        return;
    }
    let word = ed.selected_word().unwrap_or_default();
    let guesses = if word.is_empty() { Vec::new() } else { kn_ui::spell::suggestions(&word).into_iter().take(8).collect::<Vec<_>>() };
    let mut open = true;
    let mut act: Option<u8> = None;
    let mut pick = ed.view.spell_pick.clone().filter(|p| guesses.contains(p)).or_else(|| guesses.first().cloned());
    egui::Window::new("Spelling and Grammar").id(egui::Id::unique("nt-spelling")).open(&mut open).collapsible(false).resizable(false).default_width(300.0).show(ctx, |ui| {
        let missing = !word.is_empty() && !kn_ui::spell::misspelled(&word).is_empty();
        ui.label(if word.is_empty() { "No misspelled word selected.".to_string() } else if missing { format!("“{word}” isn’t in the dictionary.") } else { format!("“{word}”") });
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_height(110.0);
            ui.set_width(ui.available_width());
            if guesses.is_empty() {
                ui.weak("No Guesses Found");
            }
            for g in &guesses {
                if ui.selectable_label(pick.as_deref() == Some(g.as_str()), g).clicked() {
                    pick = Some(g.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(pick.is_some() && !word.is_empty(), egui::Button::new("Change")).clicked() {
                act = Some(0);
            }
            if ui.button("Find Next").clicked() {
                act = Some(1);
            }
            if ui.add_enabled(!word.is_empty(), egui::Button::new("Ignore")).clicked() {
                act = Some(2);
            }
            if ui.add_enabled(!word.is_empty(), egui::Button::new("Learn")).clicked() {
                act = Some(3);
            }
        });
    });
    ed.view.spell_pick = pick.clone();
    match act {
        Some(0) => {
            if let Some(g) = pick {
                ed.push_undo(super::ops::Kind::Other);
                let a = if (ed.anchor.para, ed.anchor.off) <= (ed.head.para, ed.head.off) { ed.anchor } else { ed.head };
                let f = ed.para(a).fmt_at(a.off);
                ed.insert_text(&g, &f);
                ed.changed = true;
            }
            ed.check_document_now();
        }
        Some(1) => {
            ed.check_document_now();
        }
        Some(2) | Some(3) => {
            if act == Some(2) {
                kn_ui::spell::ignore(&word);
            } else {
                kn_ui::spell::learn(&word);
            }
            forget(&mut ed.view);
            ed.check_document_now();
        }
        _ => {}
    }
    if !open {
        ed.view.spell_panel = false;
    }
}
