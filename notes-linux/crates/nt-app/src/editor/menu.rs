//! The editor's context menus, with Notes' items (features/context-menus/ctx-*.txt): selected
//! text and an empty line, a link, a table cell, a table's row and column handles, an attachment.
//! Shortcuts read as the suite shows them (⌘ = Ctrl, ⌥ = Alt, ⇧ = Shift). Spelling suggestions
//! for a misspelled word come first, as on a Mac.

use nt_core::model::ParaStyle;

use super::doc::AttKind;
use super::{Cmd, Editor, Loc};

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Target {
    #[default]
    Text,
    Link(String),
    Row,
    Column,
    Attachment(String),
}

#[derive(Default)]
pub struct MenuState {
    target: Target,
    /// a misspelled word under the click: (where, its suggestions)
    word: Option<(Loc, Loc, Vec<String>)>,
    /// a date, phone number or address under the click (data detectors)
    found: Option<(super::detect::Kind, String)>,
}

/// Opens the context menu for what is under a right-click.
pub fn context(ed: &mut Editor, ui: &mut egui::Ui, resp: &egui::Response) {
    if resp.secondary_clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            ed.menu.word = None;
            ed.menu.found = None;
            ed.menu.target = match super::table::handle_at(ed, p) {
                Some((para, Some(r), None)) => {
                    let cols = ed.doc.paras.get(para).and_then(|q| q.sole_att()).and_then(|id| ed.doc.table(id)).map_or(1, |t| t.cols());
                    ed.table_sel = Some(super::TableSel { para, r0: r, c0: 0, r1: r, c1: cols - 1 });
                    Target::Row
                }
                Some((para, None, Some(c))) => {
                    let rows = ed.doc.paras.get(para).and_then(|q| q.sole_att()).and_then(|id| ed.doc.table(id)).map_or(1, |t| t.rows());
                    ed.table_sel = Some(super::TableSel { para, r0: 0, c0: c, r1: rows - 1, c1: c });
                    Target::Column
                }
                _ => {
                    let l = ed.hit(p, true);
                    if let Some(l) = l {
                        let (a, b) = ed.range();
                        let inside = l.cell == a.cell && (l.para, l.off) >= (a.para, a.off) && (l.para, l.off) <= (b.para, b.off);
                        if !inside {
                            ed.set_caret(l, false);
                        }
                    }
                    match (super::paint::link_at(ed, p), l) {
                        (Some(super::paint::Link::Web(u)), _) => Target::Link(u),
                        (_, Some(l)) if l.cell.is_none() && ed.doc.block_of(ed.para(l)).is_some() => {
                            let id = ed.para(l).sole_att().unwrap_or_default().to_string();
                            if matches!(ed.doc.att(&id).map(|a| &a.kind), Some(AttKind::Table(_)) | Some(AttKind::Divider)) {
                                Target::Text
                            } else {
                                ed.anchor = Loc { off: 0, ..l };
                                ed.head = Loc { off: ed.para(l).len(), ..l };
                                Target::Attachment(id)
                            }
                        }
                        (_, Some(l)) => {
                            // a misspelled word: its suggestions
                            let t = ed.para(l).text.clone();
                            let (wa, wb) = super::ops::word_at(&t, l.off);
                            let w = &t[wa..wb];
                            if ed.settings.data_detectors {
                                ed.menu.found = super::detect::find(&t).into_iter().find(|(a, b, _)| l.off >= *a && l.off <= *b).map(|(a, b, k)| (k, t[a..b].to_string()));
                            }
                            if ed.settings.spelling && !w.is_empty() && !kn_ui::spell::misspelled(w).is_empty() {
                                ed.menu.word = Some((Loc { off: wa, ..l }, Loc { off: wb, ..l }, kn_ui::spell::suggestions(w).into_iter().take(5).collect()));
                            }
                            Target::Text
                        }
                        _ => Target::Text,
                    }
                }
            };
        }
    }
    let ctx = ui.ctx().clone();
    crate::menus::context_menu(resp, resp.id.with("ctx"), |ui| {
        ui.set_min_width(270.0);
        let target = ed.menu.target.clone();
        let mut run: Option<Cmd> = None;
        let mut close = false;
        match target {
            Target::Row => {
                item(ui, "Add Row Above", "", true, &mut run, Cmd::AddRowAbove);
                item(ui, "Add Row Below", "", true, &mut run, Cmd::AddRowBelow);
                ui.separator();
                item(ui, "Delete Row", "", true, &mut run, Cmd::DeleteRow);
            }
            Target::Column => {
                item(ui, "Add Column Before", "", true, &mut run, Cmd::AddColumnBefore);
                item(ui, "Add Column After", "", true, &mut run, Cmd::AddColumnAfter);
                ui.separator();
                item(ui, "Delete Column", "", true, &mut run, Cmd::DeleteColumn);
            }
            Target::Attachment(id) => {
                // (a picture's, NO1 ctx-image: Cut, Copy, Paste / View As, Rename / Quick Look, Open)
                item(ui, "Cut", "Ctrl+X", true, &mut run, Cmd::Cut);
                item(ui, "Copy", "Ctrl+C", true, &mut run, Cmd::Copy);
                item(ui, "Paste", "Ctrl+V", true, &mut run, Cmd::Paste);
                ui.separator();
                let kind = ed.doc.att(&id).map(|a| a.kind.clone());
                if let Some(AttKind::Image { view, .. }) = &kind {
                    let view = *view;
                    ui.menu_button("View As", |ui| {
                        for (v, l) in [(nt_core::model::ViewSize::Small, "Small"), (nt_core::model::ViewSize::Large, "Large")] {
                            let mark = if view == v || (v == nt_core::model::ViewSize::Large && view != nt_core::model::ViewSize::Small) { "\u{2713} " } else { "    " };
                            if ui.button(format!("{mark}{l}")).clicked() {
                                ed.view_as(&id, v);
                                close = true;
                            }
                        }
                    });
                }
                if matches!(kind, Some(AttKind::Image { .. } | AttKind::File { .. })) && ui.button("Rename Attachment\u{2026}").clicked() {
                    ed.start_rename(&id);
                    close = true;
                }
                ui.separator();
                let file = kind.as_ref().and_then(|k| match k {
                    AttKind::Image { path, .. } | AttKind::Drawing { path, .. } | AttKind::File { path, .. } | AttKind::Pdf { path, .. } => path.clone(),
                    _ => None,
                });
                if ui.add_enabled(file.is_some(), egui::Button::new("Quick Look Attachment")).clicked() || ui.add_enabled(file.is_some(), egui::Button::new("Open Attachment")).clicked() {
                    if let Some(f) = &file {
                        super::open_external(f);
                    }
                    close = true;
                }
                if let Some(AttKind::Url { url, .. }) = ed.doc.att(&id).map(|a| a.kind.clone()) {
                    if ui.button("Open Link").clicked() {
                        ed.resp.open_link = Some(url);
                        close = true;
                    }
                }
                ui.separator();
                item(ui, "Delete", "", true, &mut run, Cmd::Delete);
            }
            Target::Link(u) => {
                if ui.button("Open Link").clicked() {
                    ed.resp.open_link = Some(u.clone());
                    close = true;
                }
                if shortcut_button(ui, "Edit Link…", "Ctrl+K", true) {
                    ed.resp.add_link = true;
                    close = true;
                }
                ui.separator();
                if ui.button("Copy Link").clicked() {
                    ctx.copy_text(u.clone());
                    close = true;
                }
                item(ui, "Remove Link", "", true, &mut run, Cmd::RemoveLink);
                ui.separator();
                text_items(ed, ui, &mut run, &mut close);
            }
            Target::Text => {
                // a date, phone number or address: its actions first
                if let Some((kind, text)) = ed.menu.found.clone() {
                    for act in super::detect::actions(kind) {
                        let label = match *act {
                            "Call" | "Send Message" => format!("{act} \u{201c}{text}\u{201d}"),
                            a => a.to_string(),
                        };
                        if ui.button(label).clicked() {
                            match super::detect::action_url(act, &text) {
                                Some(u) => ed.resp.open_link = Some(u),
                                None => ctx.copy_text(text.clone()),
                            }
                            close = true;
                        }
                    }
                    ui.separator();
                }
                if let Some((a, b, sugg)) = ed.menu.word.clone() {
                    for s in &sugg {
                        if ui.button(egui::RichText::new(s).strong()).clicked() {
                            ed.push_undo(super::ops::Kind::Other);
                            ed.anchor = a;
                            ed.head = b;
                            let f = ed.para(a).fmt_at(a.off);
                            ed.insert_text(s, &f);
                            close = true;
                        }
                    }
                    if sugg.is_empty() {
                        ui.add_enabled(false, egui::Button::new("No Guesses Found"));
                    }
                    ui.separator();
                    let word = ed.para(a).text[a.off..b.off].to_string();
                    if ui.button("Learn Spelling").clicked() {
                        kn_ui::spell::learn(&word);
                        super::spelling::forget(&mut ed.view);
                        close = true;
                    }
                    if ui.button("Ignore Spelling").clicked() {
                        kn_ui::spell::ignore(&word);
                        super::spelling::forget(&mut ed.view);
                        close = true;
                    }
                    ui.separator();
                }
                text_items(ed, ui, &mut run, &mut close);
            }
        }
        if let Some(c) = run {
            ed.command(Some(&ctx), c);
            close = true;
        }
        if close {
            ui.close();
        }
    });
}

fn shortcut_button(ui: &mut egui::Ui, label: &str, sc: &str, enabled: bool) -> bool {
    let b = egui::Button::new(label).shortcut_text(sc);
    ui.add_enabled(enabled, b).clicked()
}

fn item(ui: &mut egui::Ui, label: &str, sc: &str, enabled: bool, run: &mut Option<Cmd>, cmd: Cmd) {
    if shortcut_button(ui, label, sc, enabled) {
        *run = Some(cmd);
    }
}

fn checked_item(ui: &mut egui::Ui, label: &str, sc: &str, on: bool, enabled: bool, run: &mut Option<Cmd>, cmd: Cmd) {
    let text = if on { format!("\u{2713} {label}") } else { format!("    {label}") };
    item(ui, &text, sc, enabled, run, cmd);
}

/// Cut / Copy / Paste, Paragraph Styles, Font, Checklist, Attachments, Spelling, Substitutions,
/// Transformations — the text menu (ctx-selected-text, ctx-editor-empty-line, ctx-table-cell).
fn text_items(ed: &mut Editor, ui: &mut egui::Ui, run: &mut Option<Cmd>, close: &mut bool) {
    let st = ed.state();
    let sel = st.has_selection;
    let in_table = st.in_table;
    item(ui, "Cut", "Ctrl+X", sel, run, Cmd::Cut);
    item(ui, "Copy", "Ctrl+C", sel, run, Cmd::Copy);
    item(ui, "Paste", "Ctrl+V", true, run, Cmd::Paste);
    item(ui, "Paste and Match Style", "Alt+Shift+Ctrl+V", true, run, Cmd::PasteMatchStyle);
    ui.separator();
    ui.menu_button("Paragraph Styles", |ui| {
        let styles = [
            (ParaStyle::Title, "Title", "Shift+Ctrl+T"),
            (ParaStyle::Heading, "Heading", "Shift+Ctrl+H"),
            (ParaStyle::Subheading, "Subheading", "Shift+Ctrl+J"),
            (ParaStyle::Body, "Body", "Shift+Ctrl+B"),
            (ParaStyle::Monostyled, "Monostyled", "Shift+Ctrl+M"),
            (ParaStyle::Bulleted, "Bulleted List", "Shift+Ctrl+7"),
            (ParaStyle::Dashed, "Dashed List", "Shift+Ctrl+8"),
            (ParaStyle::Numbered, "Numbered List", "Shift+Ctrl+9"),
        ];
        for (s, l, sc) in styles {
            checked_item(ui, l, sc, st.style == Some(s), !in_table, run, Cmd::Style(s));
        }
        let quote = ed.flow(ed.head.cell)[ed.sel_paras()].iter().all(|p| p.quote);
        checked_item(ui, "Block Quote", "Ctrl+'", quote, !in_table, run, Cmd::BlockQuote);
    });
    ui.menu_button("Font", |ui| {
        checked_item(ui, "Bold", "Ctrl+B", st.bold, true, run, Cmd::Bold);
        checked_item(ui, "Italic", "Ctrl+I", st.italic, true, run, Cmd::Italic);
        checked_item(ui, "Underline", "Ctrl+U", st.underline, true, run, Cmd::Underline);
        checked_item(ui, "Strikethrough", "", st.strike, true, run, Cmd::Strike);
        if !in_table {
            ui.separator();
            ui.menu_button("Highlight", |ui| {
                item(ui, "Accent", "", true, run, Cmd::Highlight(Some(3)));
                ui.separator();
                for (i, n) in ["Purple", "Pink", "Orange", "Mint", "Blue"].iter().enumerate() {
                    item(ui, n, "", true, run, Cmd::Highlight(Some(i as u8 + 1)));
                }
            });
        }
        ui.separator();
        ui.menu_button("Baseline", |ui| {
            item(ui, "Default", "", true, run, Cmd::BaselineDefault);
            item(ui, "Superscript", "", true, run, Cmd::Superscript);
            item(ui, "Subscript", "", true, run, Cmd::Subscript);
        });
    });
    ui.separator();
    item(ui, "Checklist", "Shift+Ctrl+L", !in_table, run, Cmd::Checklist);
    let checked = ed.flow(ed.head.cell)[ed.sel_paras()].iter().any(|p| p.checked);
    item(ui, if checked { "Mark as Unchecked" } else { "Mark as Checked" }, "Shift+Ctrl+U", st.in_checklist, run, Cmd::MarkChecked);
    ui.separator();
    ui.menu_button("Attachments", |ui| {
        if shortcut_button(ui, "Add Link…", "Ctrl+K", true) {
            ed.resp.add_link = true;
            *close = true;
        }
        item(ui, "Add Table", "Alt+Ctrl+T", !in_table, run, Cmd::InsertTable);
        if ui.add_enabled(!in_table, egui::Button::new("Insert Drawing")).clicked() {
            ed.insert_drawing();
            ed.changed = true;
            *close = true;
        }
        item(ui, "Insert Divider Line", "Ctrl+L", !in_table, run, Cmd::InsertDivider);
    });
    ui.separator();
    ui.menu_button("Spelling and Grammar", |ui| {
        let mut on = ed.settings.spelling;
        if ui.checkbox(&mut on, "Check Spelling While Typing").changed() {
            ed.settings.spelling = on;
        }
    });
    ui.menu_button("Substitutions", |ui| {
        ui.checkbox(&mut ed.settings.smart_quotes, "Smart Quotes");
        ui.checkbox(&mut ed.settings.smart_dashes, "Smart Dashes");
        ui.checkbox(&mut ed.settings.smart_links, "Smart Links");
        ui.checkbox(&mut ed.settings.smart_lists, "Smart Lists");
        ui.checkbox(&mut ed.settings.text_replacement, "Text Replacement");
    });
    if sel {
        ui.menu_button("Transformations", |ui| {
            for (l, k) in [("Make Upper Case", 0), ("Make Lower Case", 1), ("Capitalize", 2)] {
                if ui.button(l).clicked() {
                    ed.transform_case(k);
                    *close = true;
                }
            }
        });
    }
}

impl Editor {
    /// Transformations: upper case, lower case, capitalised words.
    pub fn transform_case(&mut self, kind: u8) {
        if !self.has_sel() {
            return;
        }
        self.push_undo(super::ops::Kind::Other);
        let (a, b) = self.range();
        let cell = a.cell;
        for i in a.para..=b.para {
            let p = &self.flow(cell)[i];
            let s = if i == a.para { a.off } else { 0 };
            let e = if i == b.para { b.off } else { p.len() };
            let part = p.slice(s..e);
            let mut out = part.clone();
            out.text.clear();
            out.runs.clear();
            let mut at_start = s == 0 || p.text[..s].ends_with(char::is_whitespace);
            for (r, f) in part.spans() {
                let t = &part.text[r];
                let new: String = match kind {
                    0 => t.to_uppercase(),
                    1 => t.to_lowercase(),
                    _ => t
                        .chars()
                        .map(|c| {
                            let o = if at_start { c.to_uppercase().collect::<String>() } else { c.to_lowercase().collect() };
                            at_start = c.is_whitespace();
                            o
                        })
                        .collect(),
                };
                let n = out.len();
                out.insert(n, &new, f);
            }
            let flow = self.flow_mut(cell);
            flow[i].remove(s..e);
            flow[i].insert_para(s, &out);
            if i == b.para {
                self.head = Loc { off: s + out.len(), ..b };
            }
        }
        self.changed = true;
    }
}
