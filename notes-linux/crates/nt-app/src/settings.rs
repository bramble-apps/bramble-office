//! Notes > Settings… (Notes 4.13, settings-general-onmymac-locked.png): the General pane — Sort
//! notes by, New notes start with, Default account, the four switches with their explanations,
//! Enable the On This Device account (Notes' On My Mac), Default text size, Locked notes — and, as in Letters, an
//! Appearance pane with the look (Notes' own, ~/.config/bramble-notes/look.txt). The rest is
//! ~/.config/bramble-notes/settings.txt.

use std::path::PathBuf;

use egui::{vec2, RichText};
use kn_ui::theme;

use crate::app::App;
use crate::look;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Sort notes by: 0 Date Edited, 1 Date Created, 2 Title.
    pub sort: u8,
    /// New notes start with: 0 Title, 1 Heading, 2 Subheading, 3 Body.
    pub new_note_style: u8,
    pub default_account: String,
    pub group_by_date: bool,
    pub resume_quick_note: bool,
    pub auto_sort_checked: bool,
    pub mention_notifications: bool,
    pub on_my_mac: bool,
    /// Default text size: 0..=6 (the slider's stops; 3 = Notes' default).
    pub text_size: u8,
    pub dark_backgrounds: bool,
    pub locked_account: String,
    pub toolbar_items: String,
    /// Scaling: percent, 0 = the system's (kn_ui::scale).
    pub ui_scale: u32,
    pub spell_check: bool,
    pub grammar: bool,
    pub autocorrect: bool,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub text_replacement: bool,
    /// Edit > Substitutions' other ticks (the editor copies them across each frame).
    pub smart_lists: bool,
    pub smart_copy_paste: bool,
    pub smart_links: bool,
    pub smart_tags: bool,
    /// Accounts whose locked notes open with the system's biometrics (comma-separated ids).
    pub biometric_accounts: String,
    /// The sidebar's and the note list's widths (dragged at their dividers).
    pub sidebar_w: f32,
    pub list_w: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            sort: 0,
            new_note_style: 0,
            default_account: "icloud".into(),
            group_by_date: true,
            resume_quick_note: false,
            auto_sort_checked: true,
            mention_notifications: true,
            on_my_mac: true,
            text_size: 3,
            dark_backgrounds: true,
            locked_account: "local".into(),
            toolbar_items: String::new(),
            ui_scale: 0,
            spell_check: true,
            grammar: true,
            autocorrect: true,
            smart_quotes: true,
            smart_dashes: true,
            text_replacement: true,
            smart_lists: true,
            smart_copy_paste: true,
            smart_links: true,
            smart_tags: true,
            biometric_accounts: String::new(),
            sidebar_w: 220.0,
            list_w: 211.0,
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-notes"))
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = config_dir().and_then(|d| std::fs::read_to_string(d.join("settings.txt")).ok()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let on = v == "true";
            match k {
                "sort" => s.sort = v.parse().unwrap_or(0).min(2),
                "new_note_style" => s.new_note_style = v.parse().unwrap_or(0).min(3),
                "default_account" => s.default_account = v.to_string(),
                "group_by_date" => s.group_by_date = on,
                "resume_quick_note" => s.resume_quick_note = on,
                "auto_sort_checked" => s.auto_sort_checked = on,
                "mention_notifications" => s.mention_notifications = on,
                "on_my_mac" => s.on_my_mac = on,
                "text_size" => s.text_size = v.parse().unwrap_or(3).min(6),
                "dark_backgrounds" => s.dark_backgrounds = on,
                "locked_account" => s.locked_account = v.to_string(),
                "toolbar_items" => s.toolbar_items = v.to_string(),
                "ui_scale" => s.ui_scale = v.parse().unwrap_or(0).min(400),
                "spell_check" => s.spell_check = on,
                "grammar" => s.grammar = on,
                "autocorrect" => s.autocorrect = on,
                "smart_quotes" => s.smart_quotes = on,
                "smart_dashes" => s.smart_dashes = on,
                "text_replacement" => s.text_replacement = on,
                "smart_lists" => s.smart_lists = on,
                "smart_copy_paste" => s.smart_copy_paste = on,
                "smart_links" => s.smart_links = on,
                "smart_tags" => s.smart_tags = on,
                "biometric_accounts" => s.biometric_accounts = v.to_string(),
                "sidebar_w" => s.sidebar_w = v.parse().unwrap_or(220.0),
                "list_w" => s.list_w = v.parse().unwrap_or(211.0),
                _ => {}
            }
        }
        s
    }

    /// Whether the account's locked notes open with the system's biometrics.
    pub fn uses_biometrics(&self, account: &str) -> bool {
        !account.is_empty() && self.biometric_accounts.split(',').any(|a| a == account)
    }

    pub fn set_biometrics(&mut self, account: &str, on: bool) {
        let mut ids: Vec<&str> = self.biometric_accounts.split(',').filter(|a| !a.is_empty() && *a != account).collect();
        if on {
            ids.push(account);
        }
        self.biometric_accounts = ids.join(",");
    }

    pub fn save(&self) {
        if std::env::var_os("NT_SNAP").is_some() && std::env::var_os("NT_CONFIG").is_none() {
            return;
        }
        let Some(dir) = config_dir() else { return };
        let _ = std::fs::create_dir_all(&dir);
        let text = format!(
            "sort={}\nnew_note_style={}\ndefault_account={}\ngroup_by_date={}\nresume_quick_note={}\nauto_sort_checked={}\nmention_notifications={}\non_my_mac={}\ntext_size={}\ndark_backgrounds={}\nlocked_account={}\ntoolbar_items={}\nui_scale={}\nspell_check={}\ngrammar={}\nautocorrect={}\nsmart_quotes={}\nsmart_dashes={}\ntext_replacement={}\nsmart_lists={}\nsmart_copy_paste={}\nsmart_links={}\nsmart_tags={}\nbiometric_accounts={}\nsidebar_w={}\nlist_w={}\n",
            self.sort, self.new_note_style, self.default_account, self.group_by_date, self.resume_quick_note, self.auto_sort_checked, self.mention_notifications, self.on_my_mac, self.text_size, self.dark_backgrounds, self.locked_account, self.toolbar_items, self.ui_scale, self.spell_check, self.grammar, self.autocorrect, self.smart_quotes, self.smart_dashes, self.text_replacement, self.smart_lists, self.smart_copy_paste, self.smart_links, self.smart_tags, self.biometric_accounts, self.sidebar_w, self.list_w
        );
        let _ = std::fs::write(dir.join("settings.txt"), text);
    }

    /// The body text size the slider's stop stands for (Notes: 3 = 13 pt; ±2 pt a stop).
    pub fn body_size(&self) -> f32 {
        [9.0, 11.0, 12.0, 13.0, 15.0, 17.0, 20.0][self.text_size.min(6) as usize]
    }
}

/// A right-aligned label in the settings' first column (Notes' "Sort notes by:").
fn label(ui: &mut egui::Ui, w: f32, text: &str) {
    ui.allocate_ui_with_layout(vec2(w, 24.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(RichText::new(text).font(look::regular(13.0)).color(look::text()));
    });
}

/// A switch with its explanation under it in grey (Mac: 19 pt from the switch's line to the
/// explanation's, 35 pt on to the next switch).
fn check(ui: &mut egui::Ui, on: &mut bool, text: &str, note: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        theme::checkbox(ui, on, RichText::new(text).font(look::regular(13.0)));
        if !note.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                ui.add(egui::Label::new(RichText::new(note).font(look::regular(11.5)).color(look::text_dim())).wrap());
            });
        }
        ui.add_space(10.0);
    });
}

/// The Settings window (Notes > Settings…, Ctrl+,).
pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let (look0, s0) = (app.look.clone(), app.settings.clone());
    let key = egui::Id::new("settings-tab");
    let mut tab: usize = ctx.data(|d| d.get_temp(key)).unwrap_or(0);
    let accounts: Vec<(String, String)> = app.library.accounts.iter().map(|a| (a.id.clone(), crate::app::account_label(a))).collect();
    let app_lock_set = crate::lock::has_password(app, &app.settings.locked_account);
    let mut open_set_password: Option<String> = None;
    let mut reset_password: Option<String> = None;
    let mut bio_toggle: Option<(String, bool)> = None;
    let mut sync_action: Option<&str> = None;
    // (always inside the main window: never over its outline or out in the shadow margin; when
    // the window is too short, the content scrolls instead of its last rows being cut off)
    let inner = kn_ui::winframe::inner_rect(ctx).shrink(8.0);
    let max_h = (inner.height() - 48.0).max(120.0);
    let shown = egui::Window::new("Notes Settings").pivot(egui::Align2::CENTER_TOP).default_pos(inner.center_top() + vec2(0.0, 52.0)).constrain_to(inner).collapsible(false).resizable(false).open(&mut open).min_width(540.0).max_width(540.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                // (the selected tab as Letters': the accent, white text; Notes' paler selection colour
                // is for text)
                ui.visuals_mut().selection.bg_fill = theme::ACCENT();
                ui.visuals_mut().selection.stroke.color = egui::Color32::WHITE;
                for (i, name) in ["General", "Appearance", "Sync"].iter().enumerate() {
                    if ui.selectable_label(tab == i, RichText::new(*name).size(13.0)).clicked() {
                        tab = i;
                    }
                }
            });
            ui.separator();
            let lw = 140.0;
            let (l, s) = (&mut app.look, &mut app.settings);
            ui.spacing_mut().item_spacing.y = 8.0;
            if tab == 0 {
                let acct_name = |id: &str| accounts.iter().find(|a| a.0 == id).map(|a| a.1.clone()).unwrap_or_else(|| "iCloud".into());
                ui.horizontal(|ui| {
                    label(ui, lw, "Sort notes by:");
                    let names = ["Date Edited", "Date Created", "Title"];
                    theme::combo("st-sort").selected_text(names[s.sort as usize]).width(371.0).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut s.sort, i as u8, *n);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    label(ui, lw, "New notes start with:");
                    let names = ["Title", "Heading", "Subheading", "Body"];
                    theme::combo("st-new").selected_text(names[s.new_note_style as usize]).width(371.0).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut s.new_note_style, i as u8, *n);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    label(ui, lw, "Default account:");
                    theme::combo("st-acct").selected_text(acct_name(&s.default_account)).width(371.0).show_ui(ui, |ui| {
                        for (id, n) in &accounts {
                            ui.selectable_value(&mut s.default_account, id.clone(), n);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(lw + 14.0);
                    ui.label(RichText::new("New notes go to this account’s Notes folder.").font(look::regular(11.5)).color(look::text_dim()));
                });
                let col = |ui: &mut egui::Ui, f: &mut dyn FnMut(&mut egui::Ui)| {
                    ui.horizontal(|ui| {
                        ui.add_space(lw + 8.0);
                        ui.vertical(|ui| {
                            ui.set_max_width(371.0);
                            f(ui)
                        });
                    });
                };
                col(ui, &mut |ui| check(ui, &mut s.group_by_date, "Group notes by date", "When sorted by Date Edited or Date Created, group notes by date."));
                col(ui, &mut |ui| check(ui, &mut s.resume_quick_note, "Always resume to last Quick Note", "When invoking Quick Note using a hot corner or keyboard shortcut, resume to last Quick Note instead of creating a new one."));
                col(ui, &mut |ui| check(ui, &mut s.auto_sort_checked, "Automatically sort checked items", "Automatically move checklist items to the bottom of the list as they are checked."));
                col(ui, &mut |ui| check(ui, &mut s.mention_notifications, "Allow mention notifications", "Receive notifications on this computer when your name is mentioned in shared notes."));
                col(ui, &mut |ui| check(ui, &mut s.on_my_mac, "Enable the On This Device account", "Notes in On This Device are stored on this computer. Disabling this account doesn’t affect your other notes."));
                ui.separator();
                ui.horizontal(|ui| {
                    label(ui, lw, "Default text size:");
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("A").font(look::regular(10.0)));
                            ui.add_space(337.0);
                            ui.label(RichText::new("A").font(look::regular(18.0)));
                        });
                        let mut v = s.text_size as f32;
                        ui.spacing_mut().slider_width = 371.0;
                        if ui.add(egui::Slider::new(&mut v, 0.0..=6.0).step_by(1.0).show_value(false)).changed() {
                            s.text_size = v.round() as u8;
                        }
                    });
                });
                if l.dark {
                    ui.horizontal(|ui| {
                        ui.add_space(lw + 8.0);
                        theme::checkbox(ui, &mut s.dark_backgrounds, RichText::new("Use dark backgrounds for note content").font(look::regular(13.0)));
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    label(ui, lw, "Locked notes:");
                    theme::combo("st-locked").selected_text(acct_name(&s.locked_account)).width(371.0).show_ui(ui, |ui| {
                        for (id, n) in &accounts {
                            ui.selectable_value(&mut s.locked_account, id.clone(), n);
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(lw + 8.0);
                    let label = if app_lock_set { "Change Password…" } else { "Set Password…" };
                    if ui.add(egui::Button::new(RichText::new(label).font(look::regular(13.0))).min_size(vec2(130.0, 24.0))).clicked() {
                        open_set_password = Some(s.locked_account.clone());
                    }
                    // (a new password for notes locked from now on; the old one isn't asked for)
                    if app_lock_set && ui.add(egui::Button::new(RichText::new("Reset Password…").font(look::regular(13.0))).min_size(vec2(130.0, 24.0))).clicked() {
                        reset_password = Some(s.locked_account.clone());
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_space(lw + 8.0);
                    // the system's biometrics (Notes' Use Touch ID): greyed with how to set them up when
                    // the system has none, or until the account has a notes password
                    let acct = s.locked_account.clone();
                    let avail = crate::biometrics::available();
                    let ready = avail == Some(true) && app_lock_set;
                    let was = ready && s.uses_biometrics(&acct);
                    let mut on = was;
                    let note = match avail {
                        Some(false) => crate::biometrics::setup_hint().to_string(),
                        _ if !app_lock_set => format!("Set a password first to use {} for locked notes in this account.", crate::biometrics::your()),
                        _ => format!("Use {} to view locked notes in this account.", crate::biometrics::your()),
                    };
                    let note = note[..1].to_uppercase() + &note[1..];
                    ui.add_enabled_ui(ready, |ui| check(ui, &mut on, crate::biometrics::label(), &note));
                    if on != was {
                        bio_toggle = Some((acct, on));
                    }
                });
            } else if tab == 2 {
                // Sync (not on the Mac, whose notes sync through iCloud): a WebDAV server
                let sy = &mut app.sync;
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.add_space(lw + 8.0);
                    let was = sy.settings.on;
                    check(ui, &mut sy.settings.on, "Sync notes with a WebDAV server", "Nextcloud, Koofr, a NAS or any WebDAV folder. Your other computers and phones sync with the same folder.");
                    changed |= sy.settings.on != was;
                });
                ui.add_enabled_ui(sy.settings.on, |ui| {
                    ui.horizontal(|ui| {
                        label(ui, lw, "Service:");
                        theme::combo("st-sync-preset").selected_text("Fill in the address for…").width(371.0).show_ui(ui, |ui| {
                            for (name, url) in nt_core::sync::PRESETS {
                                if ui.selectable_label(false, *name).clicked() {
                                    sy.settings.url = url.to_string();
                                    changed = true;
                                }
                            }
                        });
                    });
                    let mut field = |ui: &mut egui::Ui, name: &str, v: &mut String, hint: &str, password: bool| {
                        ui.horizontal(|ui| {
                            label(ui, lw, name);
                            changed |= ui.add(egui::TextEdit::singleline(v).password(password).hint_text(hint).desired_width(371.0)).changed();
                        });
                    };
                    field(ui, "Address:", &mut sy.settings.url, "https://…", false);
                    field(ui, "User name:", &mut sy.settings.user, "", false);
                    field(ui, "Password:", &mut sy.settings.password, "an app password, if the service has them", true);
                    field(ui, "Folder:", &mut sy.settings.folder, "Bramble Notes", false);
                    ui.horizontal(|ui| {
                        ui.add_space(lw + 8.0);
                        let ready = sy.settings.ready();
                        if ui.add_enabled(ready && !sy.testing(), egui::Button::new(RichText::new(if sy.testing() { "Testing…" } else { "Test Connection" }).font(look::regular(13.0)))).clicked() {
                            sync_action = Some("test");
                        }
                        if ui.add_enabled(ready && !sy.running(), egui::Button::new(RichText::new("Sync Now").font(look::regular(13.0)))).clicked() {
                            sync_action = Some("now");
                        }
                    });
                    if let Some(t) = &sy.tested {
                        ui.horizontal(|ui| {
                            ui.add_space(lw + 8.0);
                            match t {
                                None => ui.label(RichText::new("The server answered: it works.").font(look::regular(12.0)).color(look::text_dim())),
                                Some(e) => ui.label(RichText::new(e).font(look::regular(12.0)).color(egui::Color32::from_rgb(0xd7, 0x3a, 0x2f))),
                            };
                        });
                    }
                    // (while it runs: a bar, and what it's doing under it)
                    if let Some((msg, frac)) = crate::sync::progress(sy) {
                        ui.horizontal(|ui| {
                            label(ui, lw, "Syncing:");
                            ui.vertical(|ui| {
                                let (bar, _) = ui.allocate_exact_size(vec2(371.0, 6.0), egui::Sense::hover());
                                ui.painter().rect_filled(bar, 3.0, look::text_dim().gamma_multiply(0.25));
                                let w = if frac > 0.0 { bar.width() * frac.min(1.0) } else { bar.width() * 0.04 };
                                ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(w, bar.height())), 3.0, look::accent());
                                ui.label(RichText::new(msg).font(look::regular(12.0)).color(look::text_dim()));
                            });
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
                    }
                    ui.horizontal(|ui| {
                        label(ui, lw, "Last sync:");
                        let when = sy.status.last_sync.map(|t| crate::dates::long(t)).unwrap_or_else(|| "Never".into());
                        let text = if sy.status.summary.is_empty() { when } else { format!("{when} — {}", sy.status.summary) };
                        let col = if sy.status.failed { egui::Color32::from_rgb(0xd7, 0x3a, 0x2f) } else { look::text() };
                        ui.label(RichText::new(text).font(look::regular(13.0)).color(col));
                    });
                    ui.horizontal(|ui| {
                        label(ui, lw, "Start over:");
                        let ready = sy.settings.ready() && !sy.running();
                        if ui.add_enabled(ready, egui::Button::new(RichText::new("Replace Server…").font(look::regular(13.0)))).clicked() {
                            sync_action = Some("push");
                        }
                        if ui.add_enabled(ready, egui::Button::new(RichText::new("Replace Notes Here…").font(look::regular(13.0)))).clicked() {
                            sync_action = Some("pull");
                        }
                    });
                });
                if changed {
                    sy.tested = None;
                    sy.save();
                }
            } else {
                egui::Grid::new("settings-look").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                    ui.label(RichText::new("Appearance:").font(look::regular(13.0)));
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut l.dark, false, "Light");
                        ui.radio_value(&mut l.dark, true, "Dark");
                    });
                    ui.end_row();
                    ui.label(RichText::new("Color:").font(look::regular(13.0)));
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                            for (id, name, main, accent) in theme::COLOR_THEMES {
                                // Default is Notes' own yellow
                                let (main, accent) = if id == "default" { (theme::default_accent(), theme::default_accent()) } else { (main, accent) };
                                let (r, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::click());
                                let p = ui.painter();
                                let c = r.center();
                                p.circle_filled(c, 11.0, theme::rgb(main));
                                if accent != main {
                                    p.circle_filled(c + vec2(5.5, 5.5), 4.5, theme::rgb(accent));
                                }
                                if l.color_theme == id {
                                    p.circle_stroke(c, 12.5, egui::Stroke::new(2.0, theme::TEXT()));
                                }
                                if resp.on_hover_text(if id == "default" { "Notes Yellow" } else { name }).clicked() {
                                    l.color_theme = id.to_string();
                                }
                            }
                        });
                    });
                    ui.end_row();
                    ui.label(RichText::new("Buttons:").font(look::regular(13.0)));
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut l.liquid_glass, "Glass (toolbar buttons)");
                        theme::checkbox(ui, &mut l.translucent_panes, "Translucent panes");
                    });
                    ui.end_row();
                    ui.label(RichText::new("Scaling:").font(look::regular(13.0)));
                    kn_ui::scale::combo(ui, "ui-scale", &mut s.ui_scale);
                    ui.end_row();
                    if !cfg!(target_os = "windows") {
                        ui.label(RichText::new("Window:").font(look::regular(13.0)));
                        ui.vertical(|ui| {
                            ui.radio_value(&mut l.system_title_bar, false, "Menus in the title bar");
                            ui.radio_value(&mut l.system_title_bar, true, "System title bar");
                            ui.label(RichText::new("Takes effect the next time Notes starts.").small().weak());
                            ui.add_enabled_ui(!l.system_title_bar, |ui| {
                                theme::checkbox(ui, &mut l.hide_menus, "Hide the menus (one menu button; Alt or F10 shows them)");
                            });
                        });
                        ui.end_row();
                    }
                    ui.label(RichText::new("Spelling:").font(look::regular(13.0)));
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.spell_check, "Check spelling while typing");
                        theme::checkbox(ui, &mut s.grammar, "Check grammar with spelling");
                        theme::checkbox(ui, &mut s.autocorrect, "Correct spelling automatically");
                    });
                    ui.end_row();
                    ui.label(RichText::new("Substitutions:").font(look::regular(13.0)));
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.smart_quotes, "Smart quotes");
                        theme::checkbox(ui, &mut s.smart_dashes, "Smart dashes");
                        theme::checkbox(ui, &mut s.text_replacement, "Text replacement");
                    });
                    ui.end_row();
                });
            }
        });
    });
    app.settings_rect = shown.map(|r| r.response.rect);
    match sync_action {
        Some("test") => crate::sync::test(app, ctx),
        Some("now") => crate::sync::start(app, ctx, nt_core::sync::Force::None),
        Some("push") => app.dialog = Some(crate::app::Dialog::SyncReplace(nt_core::sync::Force::Push)),
        Some("pull") => app.dialog = Some(crate::app::Dialog::SyncReplace(nt_core::sync::Force::Pull)),
        _ => {}
    }
    if let Some(account) = reset_password {
        app.dialog = Some(crate::app::Dialog::ResetPassword(account));
    }
    if let Some(account) = open_set_password {
        let old = app_lock_set.then(String::new);
        app.dialog = Some(crate::app::Dialog::SetPassword { account, old, password: String::new(), verify: String::new(), hint: String::new(), then_lock: None });
    }
    match bio_toggle {
        // turning it on asks for the notes password once, to keep it in the secret store
        Some((account, true)) => app.dialog = Some(crate::app::Dialog::BiometricPassword { account, password: String::new(), wrong: false }),
        Some((account, false)) => {
            crate::biometrics::forget_key(&account);
            app.settings.set_biometrics(&account, false);
        }
        None => {}
    }
    ctx.data_mut(|d| d.insert_temp(key, tab));
    app.show_settings = open;
    if app.look != look0 {
        app.look.apply(ctx);
        app.look.save();
    }
    if app.settings != s0 {
        app.settings.save();
        app.select_first();
    }
}
