//! File > Save as Template… and the template chooser (File > New): a template is a spreadsheet
//! kept in ~/.config/numdeck/templates (Add to Template Chooser) or saved anywhere as an
//! .nmbtemplate; a new spreadsheet from it starts untitled with its sheets, tables and styles,
//! as Numbers does.

use std::path::{Path, PathBuf};

use egui::RichText;
use kn_ui::theme;

use crate::app::App;

#[derive(Default)]
pub struct Templates {
    /// The chooser is open.
    pub chooser: bool,
    /// Where the toolbar ends (the start screen goes below it).
    pub top: f32,
    /// Save as Template…: the name being typed.
    pub saving: Option<String>,
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("numdeck").join("templates"))
}

/// My Templates: (name, file).
pub fn mine() -> Vec<(String, PathBuf)> {
    let Some(d) = dir() else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "nmbtemplate"))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), p))
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// A new, untitled spreadsheet from template file `p`.
pub fn new_from(app: &mut App, p: &Path) {
    let name = p.file_stem().map(|s| s.to_string_lossy().to_string());
    match nm_core::kn_format::KeyFile::from_zip(&std::fs::read(p).unwrap_or_default()) {
        Ok(k) => {
            let mut doc = nm_core::kn_model::Document::from_file(k);
            // a template is never saved over: the new spreadsheet has no file yet
            doc.path = None;
            app.load(nm_core::Workbook { doc });
            app.untitled_name = name;
            app.excel = None;
            app.apply_rotations();
        }
        Err(e) => app.notify(format!("Couldn't open the template: {e}")),
    }
}

fn template_bytes(app: &App) -> Option<Vec<u8>> {
    app.doc()?.file.to_zip().ok()
}

/// The windows: Save as Template… and the chooser.
pub fn ui(app: &mut App, ctx: &egui::Context) {
    // ---- Save as Template…
    if let Some(mut name) = app.templates.saving.take() {
        let mut open = true;
        let (mut add, mut save_as) = (false, false);
        egui::Window::new("Save as Template").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            ui.label("Add this spreadsheet to the template chooser, or save it as a template file.");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.add(egui::TextEdit::singleline(&mut name).desired_width(240.0));
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                add = ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Add to Template Chooser")).clicked();
                save_as = ui.button("Save…").clicked();
            });
        });
        if add {
            match (dir(), template_bytes(app)) {
                (Some(d), Some(b)) => {
                    let _ = std::fs::create_dir_all(&d);
                    let file = d.join(format!("{}.nmbtemplate", name.trim().replace('/', "-")));
                    match std::fs::write(&file, b) {
                        Ok(()) => app.notify(format!("\u{201c}{}\u{201d} is in the template chooser (File > New)", name.trim())),
                        Err(e) => app.notify(format!("Couldn't save the template: {e}")),
                    }
                }
                _ => app.notify("Couldn't save the template"),
            }
        } else if save_as {
            let file = rfd::FileDialog::new().set_file_name(format!("{}.nmbtemplate", name.trim())).add_filter("Numbers Template", &["nmbtemplate"]).save_file();
            if let (Some(f), Some(b)) = (file, template_bytes(app)) {
                if let Err(e) = std::fs::write(&f, b) {
                    app.notify(format!("Couldn't save the template: {e}"));
                }
            }
        } else if open {
            app.templates.saving = Some(name);
        }
    }

    // ---- the chooser: the start screen, as Keydeck's
    if !app.templates.chooser {
        return;
    }
    let list = mine();
    // Blank, then a spreadsheet for each of the Table popover's other styles
    let mut templates: Vec<kn_ui::welcome::Template> = STARTERS.iter().map(|(n, _)| kn_ui::welcome::Template { name: n.to_string(), preview: None }).collect();
    templates.extend(list.iter().map(|(n, _)| kn_ui::welcome::Template { name: n.clone(), preview: None }));
    let recent = app.recent();
    // a little table on each tile (the Blank one with a header row)
    let draw = |p: &egui::Painter, tile: egui::Rect, i: usize| {
        let t = egui::Rect::from_center_size(tile.center() - egui::vec2(0.0, 4.0), egui::vec2(tile.width() * 0.72, tile.height() * 0.56));
        let (cols, rows) = (4, 5);
        for k in 0..=rows {
            let y = t.min.y + k as f32 * t.height() / rows as f32;
            p.line_segment([egui::pos2(t.min.x, y), egui::pos2(t.max.x, y)], egui::Stroke::new(0.8, egui::Color32::from_gray(0xb0)));
        }
        for k in 0..=cols {
            let x = t.min.x + k as f32 * t.width() / cols as f32;
            p.line_segment([egui::pos2(x, t.min.y), egui::pos2(x, t.max.y)], egui::Stroke::new(0.8, egui::Color32::from_gray(0xb0)));
        }
        let (cw, rh) = (t.width() / cols as f32, t.height() / rows as f32);
        let style = STARTERS.get(i).map(|s| s.1);
        // the style's own touches: a header column, a footer row, check boxes, a header colour
        if style != Some(2) {
            p.rect_filled(egui::Rect::from_min_size(t.min, egui::vec2(t.width(), rh)), 0.0, egui::Color32::from_gray(if i < STARTERS.len() { 0xd8 } else { 0xc0 }));
        }
        if matches!(style, Some(0) | Some(3)) {
            p.rect_filled(egui::Rect::from_min_size(t.min + egui::vec2(0.0, rh), egui::vec2(cw, t.height() - rh)), 0.0, egui::Color32::from_gray(0xe6));
        }
        if style == Some(3) {
            p.rect_filled(egui::Rect::from_min_size(egui::pos2(t.min.x, t.max.y - rh), egui::vec2(t.width(), rh)), 0.0, egui::Color32::from_gray(0xd0));
        }
        if style == Some(4) {
            for k in 1..rows {
                let c = egui::pos2(t.min.x + cw * 0.5, t.min.y + rh * (k as f32 + 0.5));
                p.rect_stroke(egui::Rect::from_center_size(c, egui::vec2(rh * 0.5, rh * 0.5)), 1.0, egui::Stroke::new(0.8, egui::Color32::from_gray(0x90)), egui::StrokeKind::Middle);
            }
        }
        if style == Some(5) {
            p.rect_filled(egui::Rect::from_min_size(t.min, egui::vec2(t.width(), rh)), 0.0, egui::Color32::from_rgb(0x2a, 0x6e, 0x4f));
        }
        for k in 0..=rows {
            let y = t.min.y + k as f32 * rh;
            p.line_segment([egui::pos2(t.min.x, y), egui::pos2(t.max.x, y)], egui::Stroke::new(0.8, egui::Color32::from_gray(0xb0)));
        }
    };
    let spec = kn_ui::welcome::Spec {
        app: "Values",
        id: "values",
        heading: "Choose a Template",
        section: "Templates",
        all: "All Templates",
        hint: "or drop a .numbers or .xlsx file here",
        recent: &recent,
        templates: &templates,
        default: "Blank",
        tile: egui::vec2(176.0, 132.0),
        draw: &draw,
        cancel: true,
        loading: false,
        render: None,
    };
    match kn_ui::welcome::overlay(ctx, app.templates.top, &spec) {
        Some(kn_ui::welcome::Action::Create(i)) if i < STARTERS.len() => {
            app.templates.chooser = false;
            let style = STARTERS[i].1;
            app.new_document_with(|doc| with_table_style(doc, style));
        }
        Some(kn_ui::welcome::Action::Create(i)) => {
            app.templates.chooser = false;
            if let Some((_, f)) = list.get(i - STARTERS.len()) {
                new_from(app, f);
            }
        }
        Some(kn_ui::welcome::Action::Open(p)) => {
            app.templates.chooser = false;
            app.open(&p);
        }
        Some(kn_ui::welcome::Action::Browse) => {
            app.templates.chooser = false;
            app.open_dialog();
        }
        Some(kn_ui::welcome::Action::Cancel) => app.templates.chooser = false,
        None => {}
    }
}

/// The start screen's own templates: (name, Table popover style).
const STARTERS: [(&str, usize); 6] = [("Blank", 0), ("Basic", 1), ("Plain", 2), ("Totals", 3), ("Checklist", 4), ("Stocks", 5)];

/// Blank's table swapped for one in Table popover style `style` (Blank's own is style 0), in
/// the same place.
fn with_table_style(doc: &mut nm_core::kn_model::Document, style: usize) {
    if style == 0 {
        return;
    }
    let Some(&sheet) = nm_core::sheets::sheet_ids(doc).first() else { return };
    let tables: Vec<_> = doc.file.obj(sheet).map(|m| m.ref_ids("drawable_infos")).unwrap_or_default().into_iter().filter(|d| doc.file.type_name(*d) == "TST.TableInfoArchive").collect();
    let Some(&old) = tables.first() else { return };
    let Some(g) = doc.file.obj(old).and_then(|m| m.find_super("TSD.DrawableArchive")).map(nm_core::kn_model::scene::geometry) else { return };
    let Ok(donor) = nm_core::sheets::table_style_donor(style) else { return };
    let mut h = nm_core::kn_model::edit::History::default();
    doc.begin(&mut h, "Template");
    nm_core::sheets::detach_drawables(doc, &mut h, sheet, &[old]);
    let _ = nm_core::sheets::insert_table(doc, &mut h, sheet, &donor.0, donor.1, g.x, g.y);
    doc.end(&mut h);
}

/// File > Advanced > Language & Region…: the document's language and region (how dates typed
/// in read, the formats it offers), as Numbers' sheet.
pub const LANGUAGES: [(&str, &str); 10] = [("en", "English"), ("fr", "French"), ("es", "Spanish"), ("de", "German"), ("it", "Italian"), ("pt", "Portuguese"), ("nl", "Dutch"), ("ja", "Japanese"), ("zh", "Chinese"), ("ko", "Korean")];
pub const REGIONS: [(&str, &str); 14] = [
    ("US", "United States"),
    ("CA", "Canada"),
    ("GB", "United Kingdom"),
    ("AU", "Australia"),
    ("FR", "France"),
    ("BE", "Belgium"),
    ("CH", "Switzerland"),
    ("DE", "Germany"),
    ("ES", "Spain"),
    ("MX", "Mexico"),
    ("IT", "Italy"),
    ("BR", "Brazil"),
    ("JP", "Japan"),
    ("CN", "China"),
];

pub fn language_window(app: &mut App, ctx: &egui::Context) {
    let Some((mut lang, mut region)) = app.language_edit.take() else { return };
    let mut open = true;
    let mut ok = false;
    egui::Window::new("Language & Region").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        ui.label("Choose the language and region for this spreadsheet's formats.");
        ui.add_space(6.0);
        egui::Grid::new("lang-region").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Language");
            let name = LANGUAGES.iter().find(|l| l.0 == lang).map(|l| l.1).unwrap_or("English");
            theme::combo("lang").selected_text(name).width(220.0).show_ui(ui, |ui| {
                for (code, n) in LANGUAGES {
                    ui.selectable_value(&mut lang, code.to_string(), n);
                }
            });
            ui.end_row();
            ui.label("Region");
            let name = REGIONS.iter().find(|r| r.0 == region).map(|r| r.1).unwrap_or("United States");
            theme::combo("region").selected_text(name).width(220.0).show_ui(ui, |ui| {
                for (code, n) in REGIONS {
                    ui.selectable_value(&mut region, code.to_string(), n);
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        // what the formats look like
        let us = region == "US";
        let comma = matches!(region.as_str(), "FR" | "BE" | "DE" | "ES" | "IT" | "BR" | "CH");
        ui.label(RichText::new(format!(
            "Date: {}    Number: {}",
            if us { "3/9/27" } else if region == "JP" || region == "CN" || region == "CA" { "2027-03-09" } else { "09/03/27" },
            if comma { "1 234,56" } else { "1,234.56" }
        )).color(theme::TEXT_DIM()));
        ui.add_space(8.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ok = ui.button("OK").clicked();
        });
    });
    if ok {
        let locale = format!("{lang}_{region}");
        app.edit("Language & Region", |d, h| nm_core::sheets::set_document_locale(d, h, &locale));
    } else if open {
        app.language_edit = Some((lang, region));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn starter_templates_have_one_table_of_their_style() {
        nm_core::init();
        for (name, style) in super::STARTERS {
            let k = nm_core::kn_format::KeyFile::from_zip(nm_core::BLANK).unwrap();
            let mut doc = nm_core::kn_model::Document::from_file(k);
            super::with_table_style(&mut doc, style);
            let sheet = nm_core::sheets::sheet_ids(&doc)[0];
            let tables = doc.file.obj(sheet).unwrap().ref_ids("drawable_infos").into_iter().filter(|d| doc.file.type_name(*d) == "TST.TableInfoArchive").count();
            assert_eq!(tables, 1, "{name}");
            // and it saves and opens again
            let bytes = doc.file.to_zip().unwrap();
            nm_core::kn_format::KeyFile::from_zip(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
