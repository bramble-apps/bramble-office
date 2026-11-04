//! Locked notes as Notes 4.13 shows them: File > Lock Note / Remove Lock, Notes > Close All
//! Locked Notes, the locked note's view in the editor column ("This note is locked.", the
//! password field, the hint), the Set Password sheet (Settings > Locked notes) and the lock in
//! the note list.
//!
//! With the app's library (nt-core's Store) the lock is Apple's: the account has one notes
//! password (Notes' "separate password"), a locked note is encrypted with it, opening one
//! decrypts it into memory (and its files into nt_core::lock::unlocked_dir) until Close All
//! Locked Notes or quitting, and saving an open locked note keeps it encrypted. On the demo
//! library (snapshots) a stand-in keeps the password for the session and only hides the note.

use std::collections::{HashMap, HashSet};

use egui::{pos2, vec2, Align2, RichText};

use crate::app::{App, Dialog};
use crate::icons::{draw, Glyph};
use crate::look;

#[derive(Default)]
pub struct LockState {
    /// the demo library's stand-in: account id -> (password, hint), this session only
    passwords: HashMap<String, (String, String)>,
    /// locked notes opened with the password (until Close All Locked Notes)
    pub unlocked: HashSet<String>,
    /// the notes password entered this session (Notes asks once, then locks without asking)
    session: Option<String>,
    entry: String,
    wrong: bool,
    show_hint: bool,
    /// the biometric check running for a note: (note id, its answer)
    bio: Option<(String, std::sync::mpsc::Receiver<Result<(), String>>)>,
    /// the note it was last started for (it starts on its own once per note shown)
    bio_tried: Option<String>,
    /// why the last check didn't open the note
    bio_msg: Option<String>,
}

pub fn has_password(app: &App, account: &str) -> bool {
    match &app.db {
        Some(st) => st.has_password().unwrap_or(false),
        None => app.lock.passwords.contains_key(account),
    }
}

fn hint(app: &App, account: &str) -> String {
    match &app.db {
        Some(st) => st.password_hint().ok().flatten().unwrap_or_default(),
        None => app.lock.passwords.get(account).map(|(_, h)| h.clone()).unwrap_or_default(),
    }
}

pub fn check(app: &App, account: &str, pw: &str) -> bool {
    match &app.db {
        Some(st) => st.check_password(pw).unwrap_or(false),
        None => app.lock.passwords.get(account).is_some_and(|(p, _)| p == pw),
    }
}

/// Whether the selected note is locked and not opened (its view replaces the editor).
pub fn hidden(app: &App) -> bool {
    app.selected_note().is_some_and(|n| n.locked && !app.lock.unlocked.contains(&n.id))
}

/// Whether a note may be edited (locked ones only once opened).
pub fn open(app: &App, n: &nt_core::model::Note) -> bool {
    !n.locked || app.lock.unlocked.contains(&n.id)
}

/// File > Lock Note: lock the selected note (asking for a password first when the account has
/// none, or for the password when none was entered this session), or, on a locked one, Remove
/// Lock.
pub fn toggle(app: &mut App) {
    let Some(n) = app.selected_note() else { return };
    let (id, locked, acct) = (n.id.clone(), n.locked, app.account_of(n));
    if !locked && !has_password(app, &acct) {
        app.dialog = Some(Dialog::SetPassword { account: acct, old: None, password: String::new(), verify: String::new(), hint: String::new(), then_lock: Some(id) });
        return;
    }
    match app.lock.session.clone() {
        Some(pw) if check(app, &acct, &pw) => {
            with_password(app, &id, locked, &pw);
        }
        _ => app.dialog = Some(Dialog::EnterPassword { note: id, remove: locked, password: String::new(), wrong: false }),
    }
}

/// Lock (or, with `remove`, unlock for good) a note with the password; false when it's wrong.
pub fn with_password(app: &mut App, id: &str, remove: bool, pw: &str) -> bool {
    let acct = app.note_by_id(id).map(|n| app.account_of(n)).unwrap_or_default();
    if !check(app, &acct, pw) {
        return false;
    }
    app.lock.session = Some(pw.to_string());
    if app.dirty.contains_key(id) {
        crate::store::save_note(app, id);
    }
    if app.db.is_some() {
        let ok = if remove {
            crate::store::with_db(app, "Removing the lock", |st| st.remove_lock(id, pw)).is_some()
        } else {
            crate::store::with_db(app, "Locking the note", |st| st.lock_note(id, pw)).is_some()
        };
        if !ok {
            return true;
        }
        // the note as the Store now has it: locked and opened (Notes leaves a note you just
        // locked open), or unlocked
        let fresh = if remove { app.db.as_ref().and_then(|st| st.note(id).ok()) } else { app.db.as_ref().and_then(|st| st.unlock_note(id, pw).ok()) };
        if let Some(fresh) = fresh {
            if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == id) {
                *n = fresh;
            }
        }
    } else if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == id) {
        n.locked = !remove;
    }
    if remove {
        app.lock.unlocked.remove(id);
    } else {
        app.lock.unlocked.insert(id.to_string());
    }
    true
}

/// Notes > Close All Locked Notes: every opened locked note is closed again (saved first, its
/// decrypted text dropped, its decrypted files deleted).
pub fn close_all(app: &mut App) {
    let open: Vec<String> = app.lock.unlocked.drain().collect();
    for id in &open {
        if app.dirty.contains_key(id) {
            crate::store::save_note(app, id);
        }
        let closed = app.db.as_ref().and_then(|st| st.note(id).ok());
        if let Some(c) = closed {
            if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == *id) {
                *n = c;
            }
        }
    }
    nt_core::lock::forget_unlocked();
    app.lock.entry.clear();
    app.lock.session = None;
}

/// The Set Password sheet's OK (a first password, or a change from `old`).
pub fn set_password(app: &mut App, account: &str, old: Option<&str>, password: &str, hint: &str, then_lock: Option<String>) {
    let h = Some(hint).filter(|h| !h.is_empty());
    let ok = if app.db.is_some() {
        match old {
            Some(o) => crate::store::with_db(app, "Changing the password", |st| st.change_password(o, password, h)).is_some(),
            // (a first password, or Reset Password…: a new one for notes locked from now on)
            None => crate::store::with_db(app, "Setting the password", |st| if st.has_password()? { st.reset_password(password, h) } else { st.set_password(password, h) }).is_some(),
        }
    } else {
        if old.is_some_and(|o| !check(app, account, o)) {
            app.dialog = Some(Dialog::Alert("Changing the password failed.".into(), "The old password isn’t right.".into()));
            return;
        }
        app.lock.passwords.insert(account.to_string(), (password.to_string(), hint.to_string()));
        true
    };
    if !ok {
        return;
    }
    app.lock.session = Some(password.to_string());
    // (the biometric unlock keeps the new password)
    if app.settings.uses_biometrics(account) && crate::biometrics::store_key(account, password).is_err() {
        crate::biometrics::forget_key(account);
        app.settings.set_biometrics(account, false);
        app.settings.save();
    }
    if let Some(id) = then_lock {
        with_password(app, &id, false, password);
    }
}

/// The locked note's view in the editor column.
pub fn view(app: &mut App, ui: &mut egui::Ui) {
    let r = ui.max_rect();
    ui.painter().rect_filled(r, app.corners_for(r), look::content_bg());
    let Some(n) = app.selected_note() else { return };
    let (id, acct_id) = (n.id.clone(), app.account_of(n));
    let acct = app.account_name(&acct_id);
    let c = pos2(r.center().x, r.min.y + r.height() * 0.32);
    let p = ui.painter().clone();
    draw(&p, egui::Rect::from_center_size(c, vec2(44.0, 44.0)), Glyph::Lock, look::text_dim());
    p.text(c + vec2(0.0, 44.0), Align2::CENTER_CENTER, "This note is locked.", look::bold(15.0), look::text());
    let has = has_password(app, &acct_id);
    // the system's biometrics (Notes' Touch ID): offered once turned on for the account
    let bio = has && app.settings.uses_biometrics(&acct_id) && crate::biometrics::available() == Some(true);
    let msg = if has && bio {
        let your = crate::biometrics::your();
        let your = your[..1].to_uppercase() + &your[1..];
        format!("{your} or enter the password you created for notes in your “{acct}” account to view this note.")
    } else if has {
        format!("Enter the password you created for notes in your “{acct}” account to view this note.")
    } else {
        "This note was locked on another Mac. Import it again with its Mac password to open it here.".to_string()
    };
    let msg_rect = egui::Rect::from_center_size(c + vec2(0.0, 74.0), vec2(340.0, 34.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(msg_rect).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
        ui.add(egui::Label::new(RichText::new(msg).font(look::regular(12.0)).color(look::text_dim())).wrap());
    });
    if !has {
        return;
    }
    let field = egui::Rect::from_center_size(c + vec2(0.0, 116.0), vec2(220.0, 26.0));
    let mut submitted = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(field), |ui| {
        let te = ui.add(egui::TextEdit::singleline(&mut app.lock.entry).password(true).hint_text("Password").desired_width(220.0).id(egui::Id::new("lock-password")));
        if !te.has_focus() && app.lock.entry.is_empty() && app.dialog.is_none() {
            te.request_focus();
        }
        if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submitted = true;
        }
    });
    if submitted {
        let pw = std::mem::take(&mut app.lock.entry);
        app.lock.wrong = !open_with(app, &id, &acct_id, pw);
    }
    if bio {
        biometric(app, ui, &id, &acct_id, c + vec2(0.0, 196.0));
    }
    let below = c + vec2(0.0, 146.0);
    if app.lock.wrong {
        p.text(below, Align2::CENTER_CENTER, format!("Try again or enter the notes password for your “{acct}” account."), look::regular(11.5), egui::Color32::from_rgb(0xe5, 0x48, 0x3f));
    }
    let hint = hint(app, &acct_id);
    if !hint.is_empty() {
        let hr = egui::Rect::from_center_size(below + vec2(0.0, 22.0), vec2(220.0, 18.0));
        let shown = app.lock.show_hint;
        ui.scope_builder(egui::UiBuilder::new().max_rect(hr).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
            if shown {
                ui.label(RichText::new(format!("Hint: {hint}")).font(look::regular(11.5)).color(look::text_dim()));
            } else if ui.link(RichText::new("Show Hint").font(look::regular(11.5))).clicked() {
                app.lock.show_hint = true;
            }
        });
    }
}

/// Opens the locked note with the notes password (typed, or released by the biometric check).
fn open_with(app: &mut App, id: &str, acct_id: &str, pw: String) -> bool {
    let opened = match &app.db {
        Some(st) => st.unlock_note(id, &pw).ok(),
        None => check(app, acct_id, &pw).then(|| app.selected_note().cloned()).flatten(),
    };
    let Some(o) = opened else { return false };
    if let Some(n) = app.library.notes.iter_mut().find(|n| n.id == id) {
        *n = o;
    }
    app.lock.unlocked.insert(id.to_string());
    app.lock.session = Some(pw);
    app.lock.show_hint = false;
    true
}

/// The lock screen's biometric check: it starts on its own when the note is shown (Apple's
/// Touch ID prompt), says what to do while it runs, and offers it again after; a success opens
/// the note with the password from the secret store.
fn biometric(app: &mut App, ui: &mut egui::Ui, id: &str, acct_id: &str, at: egui::Pos2) {
    let snapshot = std::env::var_os("NT_SNAP").is_some();
    let start = |app: &mut App, ui: &egui::Ui| {
        app.lock.bio = Some((id.to_string(), crate::biometrics::verify(ui.ctx(), "open the locked note")));
        app.lock.bio_tried = Some(id.to_string());
        app.lock.bio_msg = None;
    };
    // (never in snapshots: they don't touch the real reader)
    if !snapshot && app.lock.bio.is_none() && app.lock.bio_tried.as_deref() != Some(id) && app.dialog.is_none() {
        start(app, ui);
    }
    if let Some(answer) = app.lock.bio.as_ref().filter(|(n, _)| n == id).and_then(|(_, rx)| rx.try_recv().ok()) {
        app.lock.bio = None;
        match answer {
            Ok(()) => {
                if crate::biometrics::load_key(acct_id).is_some_and(|pw| open_with(app, id, acct_id, pw)) {
                    return;
                }
                // the stored password is gone or no longer right (changed elsewhere): off until
                // it's turned on again with the new one
                crate::biometrics::forget_key(acct_id);
                app.settings.set_biometrics(acct_id, false);
                app.settings.save();
                app.lock.bio_msg = Some("Enter the password: the one kept for this has changed.".into());
            }
            Err(e) => app.lock.bio_msg = Some(e),
        }
    }
    let running = app.lock.bio.as_ref().is_some_and(|(n, _)| n == id);
    let p = ui.painter().clone();
    if running {
        let what = if cfg!(target_os = "linux") { "Touch the fingerprint reader…".to_string() } else { format!("Waiting for {}…", crate::biometrics::your()) };
        p.text(at, Align2::CENTER_CENTER, what, look::regular(11.5), look::text_dim());
        return;
    }
    if let Some(m) = &app.lock.bio_msg {
        p.text(at + vec2(0.0, 28.0), Align2::CENTER_CENTER, m, look::regular(11.5), look::text_dim());
    }
    let r = egui::Rect::from_center_size(at, vec2(180.0, 24.0));
    if ui.put(r, egui::Button::new(RichText::new(crate::biometrics::label()).font(look::regular(12.0)))).clicked() && !snapshot {
        start(app, ui);
    }
}

/// The Set Password sheet (Settings > Locked notes > Set Password… / Change Password…, or Lock
/// Note without one). `old` is shown when changing.
pub fn sheet(ctx: &egui::Context, account: &str, old: Option<&mut String>, password: &mut String, verify: &mut String, hint: &mut String) -> Option<bool> {
    let mut done = None;
    let changing = old.is_some();
    kn_ui::winframe::modal(ctx, egui::Id::new("dlg-set-password"), |ui| {
        ui.set_width(400.0);
        ui.label(RichText::new(if changing { "Change Password" } else { "Set Password" }).font(look::bold(13.0)));
        ui.label(RichText::new(format!("Create a password for the locked notes in your “{account}” account.")).font(look::regular(12.0)));
        ui.add_space(6.0);
        egui::Grid::new("set-password").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            if let Some(o) = old {
                ui.label("Old Password:");
                ui.add(egui::TextEdit::singleline(o).password(true).desired_width(260.0));
                ui.end_row();
            }
            ui.label("Password:");
            ui.add(egui::TextEdit::singleline(password).password(true).desired_width(260.0));
            ui.end_row();
            ui.label("Verify:");
            ui.add(egui::TextEdit::singleline(verify).password(true).desired_width(260.0));
            ui.end_row();
            ui.label("Password Hint:");
            ui.add(egui::TextEdit::singleline(hint).desired_width(260.0));
            ui.end_row();
        });
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new("Creating a separate password for locked notes can provide additional security, but if you forget this password, there is no way to recover access to these locked notes.").font(look::regular(11.0)).color(look::text_dim())).wrap());
        let ok = !password.is_empty() && password == verify;
        ui.add_space(6.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(ok, egui::Button::new(RichText::new(if changing { "Change Password" } else { "Set Password" }).color(look::on_accent())).fill(look::accent())).clicked() {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        });
    });
    done
}
