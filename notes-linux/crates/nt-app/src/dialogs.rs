//! Notes' sheets and alerts: Delete Folder (“Are you sure you want to delete this folder?”), delete
//! for good from Recently Deleted, Import to Notes?, the Smart Folder sheet (Name, Make into Smart
//! Folder, Include notes matching all/any of the following filters, filter rows with + / −).

use egui::{vec2, RichText};

use crate::app::{App, Dialog};
use crate::look;

use crate::smart::{self, Checklist, Range, Rule, FILTERS, KINDS, UNITS};

fn buttons(ui: &mut egui::Ui, cancel: &str, ok: &str, destructive: bool) -> Option<bool> {
    let mut out = None;
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let fill = if destructive { egui::Color32::from_rgb(0xe5, 0x48, 0x3f) } else { look::accent() };
        let text = if destructive { egui::Color32::WHITE } else { look::on_accent() };
        if ui.add(egui::Button::new(RichText::new(ok).color(text)).fill(fill).min_size(vec2(84.0, 24.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            out = Some(true);
        }
        if !cancel.is_empty() && (ui.add(egui::Button::new(cancel).min_size(vec2(84.0, 24.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))) {
            out = Some(false);
        }
    });
    out
}

fn alert(ctx: &egui::Context, id: &str, title: &str, text: &str, cancel: &str, ok: &str, destructive: bool) -> Option<bool> {
    let mut out = None;
    kn_ui::winframe::modal(ctx, egui::Id::new(id), |ui| {
        ui.set_width(300.0);
        ui.vertical_centered(|ui| {
            crate::icons::draw(ui.painter(), egui::Rect::from_center_size(ui.cursor().center_top() + vec2(0.0, 24.0), vec2(40.0, 40.0)), crate::icons::Glyph::QuickNote, look::accent());
            ui.add_space(52.0);
            ui.label(RichText::new(title).font(look::bold(13.0)));
            if !text.is_empty() {
                ui.label(RichText::new(text).font(look::regular(11.5)));
            }
        });
        out = buttons(ui, cancel, ok, destructive);
    });
    out
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(d) = app.dialog.clone() else { return };
    match d {
        Dialog::RecordAudio => record_sheet(app, ctx),
        Dialog::Shortcuts => {
            let list = crate::menus::shortcut_list(ctx);
            let mut done = false;
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-shortcuts"), |ui| {
                ui.set_width(460.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Keyboard Shortcuts").font(look::bold(15.0)).color(look::text())));
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    egui::Grid::new("shortcut-grid").num_columns(2).spacing([14.0, 5.0]).striped(true).show(ui, |ui| {
                        // (fixed columns, the long ones wrapped: the sheet keeps its width)
                        let cell = |ui: &mut egui::Ui, w: f32, t: &str, c: egui::Color32| {
                            ui.allocate_ui_with_layout(vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.set_width(w);
                                ui.add(egui::Label::new(RichText::new(t).font(look::regular(12.0)).color(c)).wrap());
                            });
                        };
                        for (k, what) in &list {
                            cell(ui, 150.0, k, look::text());
                            cell(ui, 270.0, what, look::text_dim());
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Done").font(look::regular(13.0))).corner_radius(12.0).min_size(vec2(72.0, 24.0))).clicked() {
                        done = true;
                    }
                });
            });
            if done || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
                app.dialog = None;
            }
        }
        Dialog::Export(c) => crate::export_sheet::sheet(app, ctx, c),
        Dialog::SyncReplace(force) => {
            let (q, text, ok) = match force {
                nt_core::sync::Force::Push => ("Replace the notes on the server with the ones here?", "The server’s notes, and those of any other device syncing with it, will be the same as this library’s. Notes only on the server are removed.", "Replace Server"),
                _ => ("Replace the notes here with the ones on the server?", "This library will be the same as the server’s. Notes only here are removed.", "Replace Notes Here"),
            };
            if let Some(yes) = alert(ctx, "dlg-syncreplace", q, text, "Cancel", ok, true) {
                app.dialog = None;
                if yes {
                    crate::sync::start(app, ctx, force);
                }
            }
        }
        Dialog::ResetPassword(account) => {
            let name = app.account_name(&account);
            let text = format!("This allows you to create a new password for notes you lock in your “{name}” account from now on. Notes that already have a password aren’t affected.");
            if let Some(ok) = alert(ctx, "dlg-resetpw", &format!("Reset notes password for “{name}” account?"), &text, "Cancel", "Reset Password", false) {
                app.dialog = ok.then(|| Dialog::SetPassword { account, old: None, password: String::new(), verify: String::new(), hint: String::new(), then_lock: None });
            }
        }
        Dialog::MoveLibrary { to, existing } => {
            let (title, text, ok) = if existing {
                ("Use the notes in this folder?", format!("“{}” already holds a Notes library. Notes will switch to it; the notes in the current folder stay where they are.", to.display()), "Switch")
            } else {
                ("Move your notes to this folder?", format!("Your notes will be copied to “{}” and Notes will keep them there from now on. The current folder isn’t deleted.", to.display()), "Copy and Switch")
            };
            if let Some(ok) = alert(ctx, "dlg-movelib", title, &text, "Cancel", ok, false) {
                app.dialog = None;
                if ok {
                    crate::store::move_library(app, &to, existing);
                }
            }
        }
        Dialog::Alert(title, text) => {
            if alert(ctx, "dlg-alert", &title, &text, "", "OK", false).is_some() {
                app.dialog = None;
            }
        }
        Dialog::DeleteFolder(f) => {
            if let Some(ok) = alert(ctx, "dlg-delfolder", "Are you sure you want to delete this folder?", "All notes and any subfolders will be deleted.", "Cancel", "Delete", true) {
                app.dialog = None;
                if ok {
                    app.delete_folder(&f);
                }
            }
        }
        Dialog::DeleteForever(ids) => {
            let q = if ids.len() == 1 { "Are you sure you want to delete the note permanently?".to_string() } else { format!("Are you sure you want to delete the {} notes permanently?", ids.len()) };
            if let Some(ok) = alert(ctx, "dlg-delforever", &q, "You can’t undo this action.", "Cancel", "Delete", true) {
                app.dialog = None;
                if ok {
                    app.delete_forever(&ids);
                }
            }
        }
        Dialog::Import(ps) => {
            let what = if ps.len() == 1 && !ps[0].is_dir() { "This file" } else { "These files" };
            if let Some(ok) = alert(ctx, "dlg-import", "Import to Notes?", &format!("{what} will be added to an Imported Notes folder. It may look different in Notes."), "Cancel", "Import", false) {
                app.dialog = None;
                if ok {
                    start_import(app, ctx, ps);
                }
            }
        }
        Dialog::SetPassword { account, mut old, mut password, mut verify, mut hint, then_lock } => {
            let name = app.account_name(&account);
            match crate::lock::sheet(ctx, &name, old.as_mut(), &mut password, &mut verify, &mut hint) {
                Some(true) => {
                    app.dialog = None;
                    crate::lock::set_password(app, &account, old.as_deref(), &password, &hint, then_lock);
                }
                Some(false) => app.dialog = None,
                None => app.dialog = Some(Dialog::SetPassword { account, old, password, verify, hint, then_lock }),
            }
        }
        Dialog::EnterPassword { note, remove, mut password, wrong } => {
            let acct = app.note_by_id(&note).map(|n| app.account_of(n)).unwrap_or_default();
            let what = if remove { "to remove the lock from this note" } else { "to lock this note" };
            let text = format!("Enter the notes password for your “{}” account {what}.", app.account_name(&acct));
            let mut done = None;
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-enter-password"), |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new("Enter Password").font(look::bold(13.0)));
                ui.add(egui::Label::new(RichText::new(text).font(look::regular(12.0))).wrap());
                let te = ui.add(egui::TextEdit::singleline(&mut password).password(true).desired_width(320.0));
                if !te.has_focus() && password.is_empty() {
                    te.request_focus();
                }
                if wrong {
                    ui.label(RichText::new("That password isn’t right. Try again.").font(look::regular(11.5)).color(egui::Color32::from_rgb(0xe5, 0x48, 0x3f)));
                }
                done = buttons(ui, "Cancel", "OK", false);
            });
            match done {
                Some(true) => {
                    app.dialog = None;
                    if !crate::lock::with_password(app, &note, remove, &password) {
                        app.dialog = Some(Dialog::EnterPassword { note, remove, password: String::new(), wrong: true });
                    }
                }
                Some(false) => app.dialog = None,
                None => app.dialog = Some(Dialog::EnterPassword { note, remove, password, wrong }),
            }
        }
        Dialog::BiometricPassword { account, mut password, wrong } => {
            let text = format!("Enter the notes password for your “{}” account to use {} for its locked notes.", app.account_name(&account), crate::biometrics::your());
            let mut done = None;
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-biometric-password"), |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new(crate::biometrics::label()).font(look::bold(13.0)));
                ui.add(egui::Label::new(RichText::new(text).font(look::regular(12.0))).wrap());
                let te = ui.add(egui::TextEdit::singleline(&mut password).password(true).desired_width(320.0));
                if !te.has_focus() && password.is_empty() {
                    te.request_focus();
                }
                if wrong {
                    ui.label(RichText::new("That password isn’t right. Try again.").font(look::regular(11.5)).color(egui::Color32::from_rgb(0xe5, 0x48, 0x3f)));
                }
                done = buttons(ui, "Cancel", "OK", false);
            });
            match done {
                Some(true) => {
                    app.dialog = None;
                    if !crate::lock::check(app, &account, &password) {
                        app.dialog = Some(Dialog::BiometricPassword { account, password: String::new(), wrong: true });
                    } else if let Err(e) = crate::biometrics::store_key(&account, &password) {
                        app.dialog = Some(Dialog::Alert("The password couldn’t be kept in the keyring.".into(), e));
                    } else {
                        app.settings.set_biometrics(&account, true);
                        app.settings.save();
                    }
                }
                Some(false) => app.dialog = None,
                None => app.dialog = Some(Dialog::BiometricPassword { account, password, wrong }),
            }
        }
        Dialog::AddLink { mut link, mut name, mut use_title } => {
            let mut done = None;
            // notes whose titles start with what's typed (Notes' autocomplete under Link To)
            let q = link.trim().to_lowercase();
            let hits: Vec<(String, String)> = if q.is_empty() || q.contains("://") {
                Vec::new()
            } else {
                app.library.notes.iter().filter(|n| !n.deleted && n.title.to_lowercase().contains(&q)).take(6).map(|n| (n.id.clone(), n.title.clone())).collect()
            };
            let note = app.library.notes.iter().find(|n| !n.deleted && n.title.eq_ignore_ascii_case(link.trim())).map(|n| (n.id.clone(), n.title.clone()));
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-link"), |ui| {
                ui.set_width(380.0);
                egui::Grid::new("link-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                    ui.label("Link To:");
                    let te = ui.add(egui::TextEdit::singleline(&mut link).hint_text("URL or note title").desired_width(280.0));
                    if link.is_empty() && !te.has_focus() {
                        te.request_focus();
                    }
                    ui.end_row();
                    if !hits.is_empty() && note.is_none() {
                        ui.label("");
                        ui.vertical(|ui| {
                            for (_, t) in &hits {
                                if ui.selectable_label(false, t).clicked() {
                                    link = t.clone();
                                }
                            }
                        });
                        ui.end_row();
                    }
                    ui.label("Name:");
                    ui.add_enabled(!use_title, egui::TextEdit::singleline(&mut name).desired_width(280.0));
                    ui.end_row();
                    if note.is_some() {
                        ui.label("");
                        kn_ui::theme::checkbox(ui, &mut use_title, "Use Note Title");
                        ui.end_row();
                    }
                });
                done = buttons(ui, "Cancel", "OK", false);
            });
            match done {
                Some(true) if !link.trim().is_empty() => {
                    app.dialog = None;
                    match note {
                        Some((id, title)) => {
                            let shown = if use_title || name.trim().is_empty() { title } else { name.trim().to_string() };
                            app.editor_cmds.push(format!("Note Link:{shown}|{}", nt_core::model::Token::note_link_url(&id, None)));
                        }
                        None => {
                            let url = if link.contains("://") || link.starts_with("mailto:") { link.trim().to_string() } else { format!("https://{}", link.trim()) };
                            app.editor_cmds.push(format!("Add Link:{url}"));
                        }
                    }
                }
                Some(_) => app.dialog = None,
                None => app.dialog = Some(Dialog::AddLink { link, name, use_title }),
            }
        }
        Dialog::Print { pdf, printers, mut printer, mut copies } => {
            let mut done = None;
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-print"), |ui| {
                ui.set_width(360.0);
                ui.label(RichText::new("Print").font(look::bold(13.0)));
                if printers.is_empty() {
                    ui.add(egui::Label::new(RichText::new("No printers are set up. The note’s pages can be opened in your PDF viewer and printed from there.").font(look::regular(12.0))).wrap());
                } else {
                    ui.horizontal(|ui| {
                        ui.label("Printer:");
                        kn_ui::theme::combo("print-printer").selected_text(printers[printer.min(printers.len() - 1)].clone()).width(240.0).show_ui(ui, |ui| {
                            for (i, p) in printers.iter().enumerate() {
                                ui.selectable_value(&mut printer, i, p);
                            }
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Copies:");
                        ui.add(egui::DragValue::new(&mut copies).range(1..=99));
                    });
                }
                done = buttons(ui, "Cancel", if printers.is_empty() { "Open PDF" } else { "Print" }, false);
            });
            match done {
                Some(true) => {
                    app.dialog = None;
                    if printers.is_empty() {
                        let _ = std::process::Command::new("xdg-open").arg(&pdf).spawn();
                    } else {
                        let name = printers[printer.min(printers.len() - 1)].clone();
                        app.background(ctx, move || match crate::print::print(&pdf, &name, copies) {
                            Ok(()) => crate::app::JobDone::Message(format!("Sent to {name}")),
                            Err(e) => crate::app::JobDone::Message(format!("Printing failed: {e}")),
                        });
                    }
                }
                Some(false) => app.dialog = None,
                None => app.dialog = Some(Dialog::Print { pdf, printers, printer, copies }),
            }
        }
        Dialog::SmartFolder { mut name, mut smart, mut any, mut rules, convert } => {
            let mut done = None;
            // (the folders a Folders filter can name: the ordinary ones)
            let folders: Vec<(String, String)> = app.library.folders.iter().filter(|f| !App::is_smart(f) && f.kind != nt_core::model::FolderKind::Trash).map(|f| (f.id.clone(), f.name.clone())).collect();
            kn_ui::winframe::modal(ctx, egui::Id::new("dlg-smart"), |ui| {
                ui.set_width(600.0);
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(360.0));
                });
                kn_ui::theme::checkbox(ui, &mut smart, "Make into Smart Folder");
                if smart {
                    ui.horizontal(|ui| {
                        ui.label("Include notes matching");
                        kn_ui::theme::combo("smart-any").selected_text(if any { "any" } else { "all" }).width(60.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut any, false, "all");
                            ui.selectable_value(&mut any, true, "any");
                        });
                        ui.label("of the following filters.");
                    });
                    let mut remove = None;
                    let n = rules.len();
                    for (k, rule) in rules.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            let mut filter = rule.filter();
                            kn_ui::theme::combo(("smart-kind", k)).selected_text(FILTERS[filter]).width(120.0).show_ui(ui, |ui| {
                                for (i, f) in FILTERS.iter().enumerate() {
                                    ui.selectable_value(&mut filter, i, *f);
                                }
                            });
                            if filter != rule.filter() {
                                *rule = Rule::new(filter);
                            }
                            rule_row(ui, k, rule, &folders);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add_enabled(n > 1, egui::Button::new("−")).clicked() {
                                    remove = Some(k);
                                }
                                if ui.button("+").clicked() {
                                    remove = Some(usize::MAX - k);
                                }
                            });
                        });
                    }
                    match remove {
                        Some(k) if k > usize::MAX / 2 => rules.insert(usize::MAX - k + 1, Rule::new(0)),
                        Some(k) => {
                            rules.remove(k);
                        }
                        None => {}
                    }
                }
                done = buttons(ui, "Cancel", "OK", false);
            });
            match done {
                Some(true) => {
                    app.dialog = None;
                    make_smart(app, &name, smart, any, &rules, convert);
                }
                Some(false) => app.dialog = None,
                None => app.dialog = Some(Dialog::SmartFolder { name, smart, any, rules, convert }),
            }
        }
    }
}

/// A Smart Folder row's controls after its filter pop-up (the Mac's: what each filter takes).
fn rule_row(ui: &mut egui::Ui, k: usize, rule: &mut Rule, folders: &[(String, String)]) {
    // a text field whose raw text is kept while it's edited (a list of tags, a date)
    let text_field = |ui: &mut egui::Ui, key: &str, value: String, hint: &str, width: f32| -> Option<String> {
        let id = egui::Id::new(("smart-text", k, key.to_string()));
        let mut buf: String = ui.data(|d| d.get_temp(id)).unwrap_or(value);
        let changed = ui.add(egui::TextEdit::singleline(&mut buf).hint_text(hint).desired_width(width)).changed();
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
        changed.then_some(buf)
    };
    let split = |s: &str| s.split(',').map(|t| t.trim().trim_start_matches('#').to_string()).filter(|t| !t.is_empty()).collect::<Vec<_>>();
    let include = |ui: &mut egui::Ui, on: &mut bool, what: &str| {
        kn_ui::theme::combo(("smart-incl", k)).selected_text(format!("{} {what}", if *on { "Include" } else { "Exclude" })).width(200.0).show_ui(ui, |ui| {
            ui.selectable_value(on, true, format!("Include {what}"));
            ui.selectable_value(on, false, format!("Exclude {what}"));
        });
    };
    match rule {
        Rule::Tags { any_tag, included, excluded, all } => {
            kn_ui::theme::combo(("smart-tagmode", k)).selected_text(if *any_tag { "Notes with Tags" } else { "Selected Tags" }).width(130.0).show_ui(ui, |ui| {
                ui.selectable_value(any_tag, true, "Notes with Tags");
                ui.selectable_value(any_tag, false, "Selected Tags");
            });
            if !*any_tag {
                kn_ui::theme::combo(("smart-tagop", k)).selected_text(if *all { "all" } else { "any" }).width(50.0).show_ui(ui, |ui| {
                    ui.selectable_value(all, true, "all");
                    ui.selectable_value(all, false, "any");
                });
                if let Some(t) = text_field(ui, "inc", included.join(", "), "included tags", 110.0) {
                    *included = split(&t);
                }
                if let Some(t) = text_field(ui, "exc", excluded.join(", "), "excluded tags", 100.0) {
                    *excluded = split(&t);
                }
            }
        }
        Rule::Date { range, .. } => {
            // the range's kind: 0 Today, 1 Yesterday, 2 In the Past, 3 On, 4 Before, 5 After a Date
            let today = crate::dates::local(crate::dates::now()).days;
            let mut kind = match range {
                Range::Today => 0,
                Range::Yesterday => 1,
                Range::Past(..) => 2,
                Range::On(_) => 3,
                Range::Before(_) => 4,
                Range::After(_) => 5,
            };
            let labels = ["Today", "Yesterday", "In the Past", "On a Date", "Before a Date", "After a Date"];
            let was = kind;
            kn_ui::theme::combo(("smart-range", k)).selected_text(labels[kind]).width(110.0).show_ui(ui, |ui| {
                for (i, l) in labels.iter().enumerate() {
                    ui.selectable_value(&mut kind, i, *l);
                }
            });
            if kind != was {
                *range = match kind {
                    0 => Range::Today,
                    1 => Range::Yesterday,
                    2 => Range::Past(7, smart::Unit::Days),
                    3 => Range::On(today),
                    4 => Range::Before(today),
                    _ => Range::After(today),
                };
            }
            match range {
                Range::Past(n, unit) => {
                    ui.add(egui::DragValue::new(n).range(1..=999));
                    let current = UNITS.iter().find(|u| u.0 == *unit).map(|u| u.1).unwrap_or("Days");
                    kn_ui::theme::combo(("smart-unit", k)).selected_text(current).width(80.0).show_ui(ui, |ui| {
                        for (u, l) in UNITS {
                            ui.selectable_value(unit, u, l);
                        }
                    });
                }
                Range::On(d) | Range::Before(d) | Range::After(d) => {
                    if let Some(t) = text_field(ui, "day", crate::dates::format_day(*d), "YYYY-MM-DD", 100.0) {
                        if let Some(nd) = crate::dates::parse_day(&t) {
                            *d = nd;
                        }
                    }
                }
                _ => {}
            }
        }
        Rule::Checklists(c) => {
            let label = |c: Checklist| match c {
                Checklist::Any => "Any",
                Checklist::Done => "Completed",
                Checklist::Undone => "Incomplete",
            };
            kn_ui::theme::combo(("smart-check", k)).selected_text(label(*c)).width(110.0).show_ui(ui, |ui| {
                for x in [Checklist::Any, Checklist::Done, Checklist::Undone] {
                    ui.selectable_value(c, x, label(x));
                }
            });
        }
        Rule::Attachments(kind) => {
            let current = kind.and_then(|x| KINDS.iter().find(|k| k.0 == x)).map(|k| k.1).unwrap_or("Any Kind");
            kn_ui::theme::combo(("smart-kind2", k)).selected_text(current).width(150.0).show_ui(ui, |ui| {
                ui.selectable_value(kind, None, "Any Kind");
                for (x, l) in KINDS {
                    ui.selectable_value(kind, Some(x), l);
                }
            });
        }
        Rule::Folders { include: inc, folders: chosen } => {
            kn_ui::theme::combo(("smart-finc", k)).selected_text(if *inc { "Include" } else { "Exclude" }).width(80.0).show_ui(ui, |ui| {
                ui.selectable_value(inc, true, "Include");
                ui.selectable_value(inc, false, "Exclude");
            });
            let label = match chosen.len() {
                0 => "Choose Folders".to_string(),
                1 => folders.iter().find(|(id, _)| *id == chosen[0]).map(|(_, n)| n.clone()).unwrap_or_else(|| "1 Folder".into()),
                n => format!("{n} Folders"),
            };
            kn_ui::theme::combo(("smart-folders", k)).selected_text(label).width(150.0).show_ui(ui, |ui| {
                for (id, n) in folders {
                    let mut on = chosen.contains(id);
                    if kn_ui::theme::checkbox(ui, &mut on, n.as_str()).changed() {
                        if on {
                            chosen.push(id.clone());
                        } else {
                            chosen.retain(|x| x != id);
                        }
                    }
                }
            });
        }
        Rule::QuickNotes(on) => include(ui, on, "Quick Notes"),
        Rule::Pinned(on) => include(ui, on, "Pinned Notes"),
        Rule::Locked(on) => include(ui, on, "Locked Notes"),
    }
}

/// The Smart Folder sheet's OK: a new (or converted, or edited) folder with Notes' query JSON.
fn make_smart(app: &mut App, name: &str, smart: bool, any: bool, rules: &[Rule], convert: Option<String>) {
    let name = if name.trim().is_empty() { "New Smart Folder" } else { name.trim() };
    let query = smart::to_json(&smart::Query { any, rules: rules.to_vec() });
    if app.db.is_some() {
        let id = match &convert {
            Some(f) => {
                let (f, n, q) = (f.clone(), name.to_string(), query.clone());
                crate::store::with_db(app, "Changing the Smart Folder", |st| {
                    st.rename_folder(&f, &n)?;
                    if smart {
                        st.set_smart_query(&f, &q)?;
                    }
                    Ok(())
                })
                .map(|_| f)
            }
            None if smart => {
                let (n, q) = (name.to_string(), query.clone());
                crate::store::with_db(app, "Creating the Smart Folder", |st| st.create_smart_folder(&n, &q))
            }
            None => {
                let n = name.to_string();
                crate::store::with_db(app, "Creating the folder", |st| st.create_folder(&n, None))
            }
        };
        if let Some(id) = id {
            crate::store::reload(app);
            app.go(crate::app::Place::Folder(id));
        }
        return;
    }
    match convert {
        Some(f) => {
            // the folder's notes stay where they are (Notes moves them to Notes; tagged with the name)
            if let Some(fo) = app.library.folders.iter_mut().find(|x| x.id == f) {
                fo.name = name.to_string();
                if smart {
                    fo.smart = Some(query);
                    fo.kind = nt_core::model::FolderKind::Smart;
                }
            }
        }
        None => {
            app.new_folder(None);
            if let Some(id) = app.renaming.take().map(|r| r.folder) {
                if let Some(fo) = app.library.folders.iter_mut().find(|x| x.id == id) {
                    fo.name = name.to_string();
                    if smart {
                        fo.smart = Some(query);
                        fo.kind = nt_core::model::FolderKind::Smart;
                    }
                }
            }
        }
    }
    app.note = None;
    app.select_first();
}

/// Import to Notes… with these files or folders: a Mac library goes straight in; anything else
/// asks first (Import to Notes?).
pub fn ask_import(app: &mut App, ctx: &egui::Context, paths: Vec<std::path::PathBuf>) {
    // a Bramble library export (.bnotes): its notes and folders merged in, on a worker
    let (bnotes, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bnotes")));
    for b in bnotes {
        import_bnotes(app, ctx, b);
    }
    // UpNote's library (its folder, upnote.sqlite3 or a .upnx backup): File > Import UpNote Library…'s
    let (upnote, paths): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| is_upnote(p));
    for u in upnote {
        crate::menus::import_upnote(app, ctx, u);
    }
    let (stores, rest): (Vec<_>, Vec<_>) = paths.into_iter().partition(|p| crate::store::is_store(p));
    for s in stores {
        app.import_store(&s);
    }
    let rest: Vec<_> = rest.into_iter().filter(|p| p.is_dir() || crate::import::readable(p)).collect();
    if !rest.is_empty() {
        app.dialog = Some(Dialog::Import(rest));
    }
}

/// Whether `p` is UpNote's library: its folder (holding upnote.sqlite3), the database, or a .upnx backup.
pub fn is_upnote(p: &std::path::Path) -> bool {
    if p.is_dir() {
        return p.join("upnote.sqlite3").is_file();
    }
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("upnx") || e.eq_ignore_ascii_case("sqlite3"))
}

/// File > Import… with a .bnotes file (File > Export Library…'s): read into the library on a
/// worker with a Store of its own, then the library read again.
fn import_bnotes(app: &mut App, ctx: &egui::Context, path: std::path::PathBuf) {
    let Some(dir) = app.db.as_ref().map(|st| st.dir().to_path_buf()) else {
        app.dialog = Some(Dialog::Alert("Import to Notes failed.".into(), "A .bnotes file goes into the app's own library.".into()));
        return;
    };
    // (what's being typed saved first)
    let dirty: Vec<String> = app.dirty.keys().cloned().collect();
    for id in dirty {
        crate::store::save_note(app, &id);
    }
    app.notify("Importing…");
    app.background(ctx, move || match nt_core::store::Store::open(&dir).and_then(|mut st| nt_core::native::import_bnotes(&mut st, &path)) {
        Ok(r) => crate::app::JobDone::Reload(format!("Imported {} {} and {} {}{}", r.notes, if r.notes == 1 { "note" } else { "notes" }, r.folders, if r.folders == 1 { "folder" } else { "folders" }, if r.skipped > 0 { format!(" ({} already here)", r.skipped) } else { String::new() })),
        Err(e) => crate::app::JobDone::Message(format!("Import failed: {e}")),
    });
}

/// The files are read on a worker (a big folder takes a while), then [`imported`] adds them.
fn start_import(app: &mut App, ctx: &egui::Context, paths: Vec<std::path::PathBuf>) {
    app.notify("Importing…");
    app.background(ctx, move || {
        let mut tree = crate::import::Tree::default();
        let mut failed = Vec::new();
        for p in paths {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if p.is_dir() {
                tree.folders.push(crate::import::read_dir(&p));
            } else {
                match crate::import::read_file(&p) {
                    // (an Evernote export is a notebook: a folder of its own, its notes' dates kept)
                    Ok(items) if ext == "enex" => tree.folders.push(crate::import::Tree { name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), items, folders: Vec::new() }),
                    Ok(items) => tree.items.extend(items),
                    Err(e) => failed.push(e),
                }
            }
        }
        crate::app::JobDone::Import(tree, failed)
    });
}

/// Puts what [`start_import`] read in the account's "Imported Notes": loose files as notes in it,
/// folders (and .enex notebooks) as folders inside it.
pub fn imported(app: &mut App, tree: crate::import::Tree, failed: Vec<String>) {
    use nt_core::model::{Account, Folder, Library, Note};
    let count = |t: &crate::import::Tree| -> usize {
        fn c(t: &crate::import::Tree) -> usize {
            t.items.len() + t.folders.iter().map(c).sum::<usize>()
        }
        c(t)
    };
    let total = count(&tree);
    let now = crate::dates::now();
    let db = app.db.is_some();
    let note_of = |it: crate::import::Item, folder: &str, k: usize| {
        let lines: Vec<String> = it.body.paragraphs.iter().map(|p| p.text()).filter(|t| !t.trim().is_empty()).collect();
        Note {
            id: if db { nt_core::pb::new_uuid() } else { format!("IMPORT-{:X}-{k}", now as u64) },
            folder: folder.to_string(),
            title: lines.first().cloned().unwrap_or_default(),
            snippet: lines.get(1).cloned().unwrap_or_default(),
            created: it.created.unwrap_or(now),
            modified: it.modified.or(it.created).unwrap_or(now),
            body: it.body,
            ..Default::default()
        }
    };
    let mut first: Option<String> = None;
    let mut top: Option<String> = None;
    if app.db.is_some() {
        // a library of one account with "Imported Notes" at its top (the one already there if it
        // is: the store keeps a library's accounts as accounts and reuses folders by id): loose
        // files are notes in it, folders (and .enex notebooks) folders inside it, every note with
        // its own dates (so Sort By > Date Created shows them in their first order)
        let acct = nt_core::store::ACCOUNT_ID.to_string();
        let acct_name = app.library.accounts.iter().find(|a| a.id == acct).map(|a| a.name.clone()).unwrap_or_default();
        let top_id = app.library.folders.iter().find(|f| f.account == acct && f.name == "Imported Notes" && f.parent.is_none() && !App::is_smart(f)).map(|f| f.id.clone()).unwrap_or_else(nt_core::pb::new_uuid);
        let mut lib = Library { accounts: vec![Account { id: acct.clone(), name: acct_name, kind: 3 }], ..Library::default() };
        lib.folders.push(Folder { id: top_id.clone(), account: acct.clone(), name: "Imported Notes".into(), ..Default::default() });
        fn add(lib: &mut Library, t: crate::import::Tree, parent: Option<String>, k: &mut usize, note_of: &dyn Fn(crate::import::Item, &str, usize) -> Note) {
            *k += 1;
            let id = nt_core::pb::new_uuid();
            lib.folders.push(Folder { id: id.clone(), account: nt_core::store::ACCOUNT_ID.into(), name: t.name, parent, ..Default::default() });
            for it in t.items {
                *k += 1;
                let n = note_of(it, &id, *k);
                lib.notes.push(n);
            }
            for f in t.folders {
                add(lib, f, Some(id.clone()), k, note_of);
            }
        }
        let mut k = 0;
        for it in tree.items {
            k += 1;
            lib.notes.push(note_of(it, &top_id, k));
        }
        for f in tree.folders {
            add(&mut lib, f, Some(top_id.clone()), &mut k, &note_of);
        }
        if crate::store::with_db(app, "Import to Notes", |st| st.import_library(&lib)).is_some() {
            first = lib.notes.first().map(|n| n.id.clone());
            top = Some(top_id);
        }
        crate::store::reload(app);
        let top = top.or_else(|| app.library.folders.iter().find(|f| f.name == "Imported Notes" && f.parent.is_none()).map(|f| f.id.clone()));
        if let Some(t) = top {
            app.go(crate::app::Place::Folder(t));
        }
    } else {
        // (the demo: kept in memory)
        let acct = app.settings.default_account.clone();
        let fid = match app.library.folders.iter().find(|f| f.account == acct && f.name == "Imported Notes") {
            Some(f) => f.id.clone(),
            None => {
                let id = format!("IMPORTED-{acct}");
                app.library.folders.push(Folder { id: id.clone(), account: acct.clone(), name: "Imported Notes".into(), ..Default::default() });
                id
            }
        };
        fn add(t: crate::import::Tree, folder: &str, acct: &str, k: &mut usize, note_of: &dyn Fn(crate::import::Item, &str, usize) -> Note, out: &mut Library) {
            for it in t.items {
                *k += 1;
                out.notes.push(note_of(it, folder, *k));
            }
            for f in t.folders {
                *k += 1;
                let id = format!("{folder}-{k}");
                out.folders.push(Folder { id: id.clone(), account: acct.to_string(), name: f.name.clone(), parent: Some(folder.to_string()), ..Default::default() });
                add(f, &id, acct, k, note_of, out);
            }
        }
        let mut out = Library::default();
        add(tree, &fid, &acct, &mut 0, &note_of, &mut out);
        first = out.notes.first().map(|n| n.id.clone());
        app.library.notes.append(&mut out.notes);
        app.library.folders.append(&mut out.folders);
        app.go(crate::app::Place::Folder(fid));
    }
    if let Some(n) = &first {
        app.select_note(n);
    }
    if !failed.is_empty() {
        app.dialog = Some(Dialog::Alert("Some files couldn’t be imported.".into(), failed.join("\n")));
    }
    app.notify(format!("Imported {total} {}", if total == 1 { "note" } else { "notes" }));
}

/// Edit > Record Audio… (Notes' audio sheet): the round button records and stops, with the time
/// and the level while recording; Done puts the take in the note at the caret (as m4a when ffmpeg
/// is there, else WAV), Cancel throws it away.
fn record_sheet(app: &mut App, ctx: &egui::Context) {
    let mut toggle = false;
    let mut answer = None;
    let recording = app.recorder.is_some();
    let secs = app.recorder.as_ref().map(|r| r.seconds()).or_else(|| app.take.as_ref().map(|(s, r, c)| s.len() as f64 / (*r as f64 * *c as f64))).unwrap_or(0.0);
    let level = app.recorder.as_ref().map(|r| r.level()).unwrap_or(0.0);
    let failed = app.recorder.as_ref().and_then(|r| r.error());
    kn_ui::winframe::modal(ctx, egui::Id::new("dlg-record"), |ui| {
        ui.set_width(300.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Record Audio").font(look::bold(13.0)));
            ui.add_space(6.0);
            let t = secs as u64;
            ui.label(RichText::new(format!("{}:{:02}", t / 60, t % 60)).font(look::regular(28.0)).color(if recording { look::text() } else { look::text_dim() }));
            // the level (the loudest of the last moment)
            let (bar, _) = ui.allocate_exact_size(vec2(200.0, 4.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 2.0, look::text_dim().gamma_multiply(0.25));
            ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * level.sqrt().min(1.0), bar.height())), 2.0, look::accent());
            ui.add_space(10.0);
            let (r, resp) = ui.allocate_exact_size(vec2(52.0, 52.0), egui::Sense::click());
            let p = ui.painter();
            let red = egui::Color32::from_rgb(0xf0, 0x3c, 0x32);
            p.circle_stroke(r.center(), 25.0, egui::Stroke::new(2.5, look::text_dim()));
            if recording {
                p.rect_filled(egui::Rect::from_center_size(r.center(), vec2(18.0, 18.0)), 3.0, red);
            } else {
                p.circle_filled(r.center(), 20.0, red);
            }
            if resp.clicked() {
                toggle = true;
            }
            ui.add_space(4.0);
            let hint = match (&failed, recording, app.take.is_some()) {
                (Some(e), _, _) => e.clone(),
                (_, true, _) => "Recording…".into(),
                (_, false, true) => "Record again to replace it.".into(),
                _ => "Click to record.".into(),
            };
            ui.label(RichText::new(hint).font(look::regular(11.5)).color(look::text_dim()));
        });
        // (Done once there's a take, or while recording: stops and keeps it)
        answer = buttons(ui, "Cancel", "Done", false);
    });
    if recording {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
    if toggle {
        match app.recorder.take() {
            Some(r) => app.take = Some(r.finish()),
            None => {
                app.take = None;
                match crate::audio::Recorder::start(app.automated) {
                    Ok(r) => app.recorder = Some(r),
                    Err(e) => app.dialog = Some(Dialog::Alert("Recording couldn’t start.".into(), e)),
                }
            }
        }
    }
    match answer {
        Some(true) => {
            if let Some(r) = app.recorder.take() {
                app.take = Some(r.finish());
            }
            app.dialog = None;
            if let Some((s, rate, ch)) = app.take.take().filter(|(s, _, _)| !s.is_empty()) {
                app.background(ctx, move || match crate::audio::save_recording(&s, rate, ch) {
                    Ok(p) => crate::app::JobDone::Attach(p),
                    Err(e) => crate::app::JobDone::Message(format!("The recording couldn’t be saved: {e}")),
                });
            }
        }
        Some(false) => {
            app.recorder = None;
            app.take = None;
            app.dialog = None;
        }
        None => {}
    }
}
