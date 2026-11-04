//! File > Export…: one card per format, each with its own Export button and options (Text; HTML's
//! collapsible sections; PDF's orientation and sections; Markdown's additional data), for the note
//! or every note where the list is (each its own file, in a folder named after it). The files are
//! made on a worker: the notes are read in full there, from a Store of its own.

use crate::app::{App, Dialog};
use crate::look;
use egui::{vec2, RichText};
use nt_core::export::{Format, Options, Sections};
use std::path::PathBuf;

/// The sheet's choices (kept while it's open, and for the next time).
#[derive(Clone, Debug, PartialEq)]
pub struct Choices {
    /// every note where the list is, not just the selected one
    pub all: bool,
    pub html: Sections,
    pub pdf_landscape: bool,
    pub pdf: Sections,
    pub md_data: bool,
}

impl Default for Choices {
    fn default() -> Choices {
        Choices { all: false, html: Sections::ExpandAll, pdf_landscape: false, pdf: Sections::ExpandAll, md_data: false }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Text,
    Html,
    Pdf,
    Markdown,
}

fn sections_name(s: Sections) -> &'static str {
    match s {
        Sections::ExpandAll => "Expand all",
        Sections::CollapseAll => "Collapse all",
        Sections::AsShown => "As shown",
    }
}

/// File > Export…: the sheet, with last time's choices.
pub fn open(app: &mut App, ctx: &egui::Context) {
    let c = ctx.data(|d| d.get_temp::<Choices>(egui::Id::new("export-choices"))).unwrap_or_default();
    app.dialog = Some(Dialog::Export(c));
}

/// The notes the sheet would export: the selected one, or all the list shows (locked ones only
/// when open, Recently Deleted's never).
fn notes_for(app: &App, all: bool) -> Vec<String> {
    if all {
        app.ordered().into_iter().filter(|id| app.note_by_id(id).is_some_and(|n| !n.deleted && (!n.locked || crate::lock::open(app, n)))).collect()
    } else {
        // (the selection: one note, or the notes picked in the list)
        app.selection().into_iter().filter(|id| app.note_by_id(id).is_some_and(|n| !n.deleted && (!n.locked || crate::lock::open(app, n)))).collect()
    }
}

/// A folder's path from its account's top ("Travel/Japan 2026"), for Markdown's data.
fn folder_path(app: &App, f: &str) -> String {
    let mut parts = Vec::new();
    let mut at = app.folder(f);
    let mut hops = 0;
    while let Some(fo) = at {
        parts.push(fo.name.clone());
        hops += 1;
        if hops > 64 {
            break;
        }
        at = fo.parent.as_deref().and_then(|p| app.folder(p));
    }
    parts.reverse();
    parts.join("/")
}

/// The sheet. Returns when it's done with (closed or exported).
pub fn sheet(app: &mut App, ctx: &egui::Context, mut c: Choices) {
    let picked = notes_for(app, false).len();
    let one = if picked > 1 { Some(format!("{picked} Selected Notes")) } else { app.selected_note().filter(|n| !n.deleted).map(crate::list::note_title) };
    let place = app.place_title();
    let all_n = notes_for(app, true).len();
    if one.is_none() {
        c.all = true;
    }
    let mut go: Option<Kind> = None;
    let mut close = false;
    kn_ui::winframe::modal(ctx, egui::Id::new("dlg-export"), |ui| {
        ui.set_width(440.0);
        ui.set_max_width(440.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Export").font(look::bold(15.0)).color(look::text()));
        });
        ui.add_space(8.0);
        // what: the note, or the list's notes
        ui.horizontal(|ui| {
            ui.label(RichText::new("Notes:").font(look::regular(13.0)).color(look::text()));
            let this = one.clone().map(|t| if picked > 1 { t } else { format!("“{t}”") }).unwrap_or_else(|| "No note selected".into());
            let all = format!("All {all_n} {} in “{place}”", if all_n == 1 { "note" } else { "notes" });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                kn_ui::theme::combo("export-scope").selected_text(if c.all { all.clone() } else { this.clone() }).width(300.0).show_ui(ui, |ui| {
                    if one.is_some() {
                        ui.selectable_value(&mut c.all, false, this.clone());
                    }
                    ui.selectable_value(&mut c.all, true, all.clone());
                });
            });
        });
        ui.add_space(6.0);
        let mut card = |ui: &mut egui::Ui, title: &str, kind: Kind, body: &mut dyn FnMut(&mut egui::Ui)| {
            egui::Frame::new().fill(if kn_ui::theme::is_light() { egui::Color32::from_white_alpha(165) } else { egui::Color32::from_white_alpha(10) }).stroke(egui::Stroke::new(1.0, look::line())).corner_radius(10.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).font(look::regular(13.0)).color(look::text()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let enabled = if c.all { all_n > 0 } else { one.is_some() };
                        let b = egui::Button::new(RichText::new("Export").font(look::regular(13.0)).color(look::on_accent())).fill(look::accent()).corner_radius(12.0).min_size(vec2(72.0, 24.0));
                        if ui.add_enabled(enabled, b).clicked() {
                            go = Some(kind);
                        }
                    });
                });
                body(ui);
            });
            ui.add_space(8.0);
        };
        let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).font(look::regular(12.5)).color(look::text_dim()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| add(ui));
            });
        };
        card(ui, "Export to Text", Kind::Text, &mut |_| {});
        let mut html = c.html;
        card(ui, "Export to HTML", Kind::Html, &mut |ui| {
            row(ui, "Collapsible Section", &mut |ui| {
                kn_ui::theme::combo("export-html-sections").selected_text(sections_name(html)).width(150.0).show_ui(ui, |ui| {
                    for s in [Sections::ExpandAll, Sections::CollapseAll, Sections::AsShown] {
                        ui.selectable_value(&mut html, s, sections_name(s));
                    }
                });
            });
        });
        c.html = html;
        let (mut land, mut pdf) = (c.pdf_landscape, c.pdf);
        card(ui, "Export to PDF", Kind::Pdf, &mut |ui| {
            row(ui, "Orientation", &mut |ui| {
                kn_ui::theme::combo("export-pdf-orientation").selected_text(if land { "Landscape" } else { "Portrait" }).width(150.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut land, false, "Portrait");
                    ui.selectable_value(&mut land, true, "Landscape");
                });
            });
            row(ui, "Collapsible Section", &mut |ui| {
                // (a page can't fold: expanded, or as the note shows it)
                kn_ui::theme::combo("export-pdf-sections").selected_text(sections_name(pdf)).width(150.0).show_ui(ui, |ui| {
                    for s in [Sections::ExpandAll, Sections::AsShown] {
                        ui.selectable_value(&mut pdf, s, sections_name(s));
                    }
                });
            });
        });
        (c.pdf_landscape, c.pdf) = (land, pdf);
        let mut data = c.md_data;
        card(ui, "Export to Markdown", Kind::Markdown, &mut |ui| {
            ui.add_space(2.0);
            kn_ui::theme::checkbox(ui, &mut data, RichText::new("Include additional data").font(look::regular(12.5)).color(look::text()));
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.add(egui::Label::new(RichText::new("Such as the creation date, the updated date and folder names (front matter at the top of each file).").font(look::regular(11.5)).color(look::text_dim())).wrap());
            });
        });
        c.md_data = data;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(egui::Button::new(RichText::new("Done").font(look::regular(13.0))).corner_radius(12.0).min_size(vec2(72.0, 24.0))).clicked() {
                close = true;
            }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("export-choices"), c.clone()));
    if let Some(kind) = go {
        app.dialog = None;
        export(app, ctx, kind, &c);
    } else if close {
        app.dialog = None;
    } else {
        app.dialog = Some(Dialog::Export(c));
    }
}

/// Where to: a file for one note, a folder (made inside the one picked) for several.
/// (NT_EXPORT_TO=<path>: there, for tests.)
fn destination(one_name: Option<&str>, ext: &str) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("NT_EXPORT_TO") {
        return Some(PathBuf::from(p));
    }
    match one_name {
        Some(name) => crate::app::file_dialog(|d| d.set_file_name(format!("{name}.{ext}")).save_file()),
        None => crate::app::file_dialog(|d| d.set_title("Export To").pick_folder()),
    }
}

/// Export the notes as `kind` with the sheet's choices, on a worker.
pub fn export(app: &mut App, ctx: &egui::Context, kind: Kind, c: &Choices) {
    let ids = notes_for(app, c.all);
    if ids.is_empty() {
        return;
    }
    let ext = match kind {
        Kind::Text => "txt",
        Kind::Html => "html",
        Kind::Pdf => "pdf",
        Kind::Markdown => "md",
    };
    // (several: the list's notes, or a multi-selection: each its own file, in a folder)
    let several = c.all || ids.len() > 1;
    let one_name = (!several).then(|| app.note_by_id(&ids[0]).map(|n| nt_core::export::file_stem(&n.title))).flatten();
    let Some(dest) = destination(one_name.as_deref(), ext) else { return };
    // (several: into a folder named after where they are, inside the one picked)
    let dest = if several { dest.join(nt_core::export::file_stem(&app.place_title())) } else { dest };
    // the notes as the worker will need them: full ones now (the demo's, open locked ones), else
    // read there from its own Store; each with its folder's path for Markdown's data
    let dir = app.db.as_ref().map(|st| st.dir().to_path_buf());
    let mut have: Vec<nt_core::model::Note> = Vec::new();
    let mut want: Vec<String> = Vec::new();
    let mut folders: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for id in &ids {
        let Some(n) = app.note_by_id(id) else { continue };
        folders.insert(id.clone(), folder_path(app, &n.folder));
        if !n.light || dir.is_none() {
            have.push(n.clone());
        } else {
            want.push(id.clone());
        }
    }
    // (Text and Markdown have no section choice: everything)
    let sections = match kind {
        Kind::Html => c.html,
        Kind::Pdf => c.pdf,
        _ => Sections::ExpandAll,
    };
    let opts = Options { sections, data: c.md_data };
    let layout = crate::print::Layout { landscape: c.pdf_landscape, sections: c.pdf };
    app.notify(if several { format!("Exporting {} notes…", ids.len()) } else { "Exporting…".into() });
    app.background(ctx, move || {
        let mut notes = have;
        if !want.is_empty() {
            if let Some(st) = dir.as_ref().and_then(|d| nt_core::store::Store::open(d).ok()) {
                notes.extend(want.iter().filter_map(|id| st.note(id).ok()));
            }
        }
        // (in the list's order)
        notes.sort_by_key(|n| ids.iter().position(|i| *i == n.id).unwrap_or(usize::MAX));
        let r = write(&notes, &dest, kind, &opts, layout, several, &folders);
        crate::app::JobDone::Message(match r {
            Ok(n) if several => format!("Exported {n} {} to “{}”", if n == 1 { "note" } else { "notes" }, dest.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Ok(_) => format!("Exported {}", dest.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            Err(e) => format!("Export failed: {e}"),
        })
    });
}

/// The files: one at `dest`, or each note in the folder `dest`. Returns how many notes.
fn write(notes: &[nt_core::model::Note], dest: &std::path::Path, kind: Kind, opts: &Options, layout: crate::print::Layout, several: bool, folders: &std::collections::HashMap<String, String>) -> Result<usize, String> {
    let folder_of = |n: &nt_core::model::Note| folders.get(&n.id).cloned().unwrap_or_default();
    if kind == Kind::Pdf {
        if several {
            std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
            let mut used = std::collections::HashSet::new();
            for n in notes {
                let stem = nt_core::export::file_stem(&n.title);
                let mut name = format!("{stem}.pdf");
                let mut k = 2;
                while !used.insert(name.clone()) {
                    name = format!("{stem} {k}.pdf");
                    k += 1;
                }
                crate::print::to_pdf_with(n, &dest.join(name), layout)?;
            }
        } else if let Some(n) = notes.first() {
            crate::print::to_pdf_with(n, dest, layout)?;
        }
        return Ok(notes.len());
    }
    let format = match kind {
        Kind::Text => Format::PlainText,
        Kind::Html => Format::Html,
        _ => Format::Markdown,
    };
    if several {
        let refs: Vec<&nt_core::model::Note> = notes.iter().collect();
        nt_core::export::write_notes_with(&refs, dest, format, opts, &folder_of).map_err(|e| e.to_string())?;
    } else if let Some(n) = notes.first() {
        let ex = nt_core::export::export_with(n, format, opts, &folder_of(n));
        std::fs::write(dest, &ex.text).map_err(|e| e.to_string())?;
        let dir = dest.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        for (rel, src) in &ex.files {
            let to = dir.join(rel);
            if let Some(d) = to.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::copy(src, &to);
        }
    }
    Ok(notes.len())
}
