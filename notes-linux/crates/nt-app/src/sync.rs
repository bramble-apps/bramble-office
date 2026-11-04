//! Sync with a WebDAV server (Nextcloud, Koofr, a NAS; nt_core::sync, as ChordPro's): Settings'
//! Sync pane holds the account; while it's on, the library syncs once at start, 5 s after the last
//! change and every minute, each time on a worker with a Store of its own (never the window's), and
//! the library is read again when the sync brought anything in.

use crate::app::App;
use nt_core::sync::{Force, Report, Settings, Status};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

/// After the last change, how long to wait before syncing (changes come in bursts).
const AFTER_CHANGE_S: f64 = 5.0;
/// How often to sync while nothing changes here (what changed elsewhere comes in).
const EVERY_S: f64 = 60.0;

#[derive(Default)]
pub struct Sync {
    /// the account, as the Settings pane edits it (saved in the library's sync.json)
    pub settings: Settings,
    pub status: Status,
    /// the library folder they belong to (read when the library opened)
    dir: Option<PathBuf>,
    /// a sync on its worker
    running: Option<Receiver<Result<Report, String>>>,
    /// Test Connection on its worker, then its answer (None: fine)
    testing: Option<Receiver<Option<String>>>,
    pub tested: Option<Option<String>>,
    /// when the library last changed here, and when a sync last started
    changed_at: Option<f64>,
    started_at: Option<f64>,
}

impl Sync {
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    pub fn testing(&self) -> bool {
        self.testing.is_some()
    }

    /// The library changed here (a note saved, a folder made…): sync a little later.
    pub fn changed(&mut self) {
        self.changed_at = Some(crate::dates::now());
    }

    /// Settings' fields changed: kept.
    pub fn save(&mut self) {
        if let Some(d) = &self.dir {
            if let Err(e) = self.settings.save(d) {
                self.status = Status { summary: format!("The sync settings couldn’t be saved: {e}"), failed: true, ..self.status.clone() };
            }
        }
    }
}

/// The library opened: its sync account read.
pub fn opened(app: &mut App) {
    let Some(dir) = app.db.as_ref().map(|st| st.dir().to_path_buf()) else { return };
    app.sync.settings = Settings::load(&dir);
    app.sync.status = Status::load(&dir);
    app.sync.dir = Some(dir);
}

/// Each frame: a finished sync taken in, the next one started when it's due.
pub fn tick(app: &mut App, ctx: &egui::Context) {
    if let Some(rx) = &app.sync.testing {
        if let Ok(r) = rx.try_recv() {
            app.sync.tested = Some(r);
            app.sync.testing = None;
        }
    }
    if let Some(rx) = &app.sync.running {
        match rx.try_recv() {
            Ok(r) => {
                app.sync.running = None;
                if let Some(d) = &app.sync.dir {
                    app.sync.status = Status::load(d);
                }
                match r {
                    // (what came in, or went, read again)
                    Ok(rep) if rep.received > 0 || rep.removed > 0 || rep.copies > 0 || rep.merged > 0 => crate::store::reload(app),
                    Ok(_) => {}
                    Err(e) => app.sync.status = Status { summary: format!("Sync failed: {e}"), failed: true, ..app.sync.status.clone() },
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => app.sync.running = None,
        }
        return;
    }
    let s = &app.sync;
    // (scripted runs don't sync, unless NT_SYNC=1 asks: a test of it)
    if !s.settings.on || !s.settings.ready() || s.dir.is_none() || (app.automated && std::env::var_os("NT_SYNC").is_none()) {
        return;
    }
    let now = crate::dates::now();
    let due = match s.started_at {
        None => true,
        Some(t) => s.changed_at.is_some_and(|c| c > t && now - c >= AFTER_CHANGE_S) || now - t >= EVERY_S,
    };
    if due {
        start(app, ctx, Force::None);
    }
    // (woken in time for the next one, though nothing else moves)
    ctx.request_repaint_after(std::time::Duration::from_secs(1));
}

/// Sync Now (or Replace…: `Force::Push` / `Force::Pull`), on a worker with its own Store.
pub fn start(app: &mut App, ctx: &egui::Context, force: Force) {
    if app.sync.running.is_some() {
        return;
    }
    let Some(dir) = app.sync.dir.clone() else { return };
    // (notes still being typed in saved first, so the sync takes them)
    let dirty: Vec<String> = app.dirty.keys().cloned().collect();
    for id in dirty {
        crate::store::save_note(app, &id);
    }
    let settings = app.sync.settings.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        // (each step it reports repaints the window, which draws sync::progress())
        let wake = ctx.clone();
        let r = nt_core::store::Store::open(&dir).and_then(|mut st| nt_core::sync::sync_now_with(&mut st, &settings, force, &|_| wake.request_repaint())).map_err(|e| e.to_string());
        let _ = tx.send(r);
        ctx.request_repaint();
    });
    app.sync.running = Some(rx);
    app.sync.started_at = Some(crate::dates::now());
    app.sync.status.summary = "Syncing…".into();
    app.sync.status.failed = false;
}

/// Test Connection, on a worker (it asks the server).
pub fn test(app: &mut App, ctx: &egui::Context) {
    if app.sync.testing.is_some() {
        return;
    }
    let settings = app.sync.settings.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(nt_core::sync::test_connection(&settings));
        ctx.request_repaint();
    });
    app.sync.testing = Some(rx);
    app.sync.tested = None;
}

/// While a sync runs: what it's doing ("Uploading 312 of 1,418 · 210 MB of 812 MB") and how far
/// along, 0..1, for Settings' bar and the sidebar's indicator.
pub fn progress(s: &Sync) -> Option<(String, f32)> {
    if !s.running() {
        return None;
    }
    Some(match nt_core::sync::progress() {
        Some(p) => (p.message(), p.fraction()),
        None => ("Syncing…".into(), 0.0),
    })
}

/// The sidebar's indicator while a sync runs: a small line at its foot, the sync glyph turning,
/// what it's doing and a thin bar.
pub fn indicator(app: &App, ui: &egui::Ui, side: egui::Rect) {
    let Some((msg, frac)) = progress(&app.sync) else { return };
    let ctx = ui.ctx();
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    let r = egui::Rect::from_min_max(egui::pos2(side.min.x + 10.0, side.max.y - 34.0), egui::pos2(side.max.x - 10.0, side.max.y - 8.0));
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sync-indicator"))).with_clip_rect(side);
    p.rect_filled(r, 8.0, crate::look::sidebar_bg());
    // (a circle of dots turning: busy)
    let t = ui.input(|i| i.time) as f32;
    let c = egui::pos2(r.min.x + 12.0, r.center().y - 2.0);
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU + t * 4.0;
        let alpha = 0.25 + 0.75 * (k as f32 / 8.0);
        p.circle_filled(c + egui::vec2(a.cos(), a.sin()) * 5.0, 1.2, crate::look::text_dim().gamma_multiply(alpha));
    }
    let short = crate::list::elide(&p, &msg, crate::look::regular(11.0), r.width() - 30.0);
    p.text(egui::pos2(r.min.x + 24.0, r.center().y - 2.0), egui::Align2::LEFT_CENTER, short, crate::look::regular(11.0), crate::look::text_dim());
    let bar = egui::Rect::from_min_max(egui::pos2(r.min.x + 24.0, r.max.y - 4.0), egui::pos2(r.max.x - 4.0, r.max.y - 2.0));
    p.rect_filled(bar, 1.0, crate::look::text_dim().gamma_multiply(0.2));
    if frac > 0.0 {
        p.rect_filled(egui::Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac.min(1.0), bar.height())), 1.0, crate::look::accent());
    }
}
