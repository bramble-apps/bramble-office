//! Audio attachments played in the note: a thread of its own owns the output device (opened on
//! the first Play) and the player, so the UI never waits on either; the card reads the shared
//! state (what's playing, where) and sends Play / Pause / Seek / Stop.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use egui::{Pos2, Rect};

use super::Editor;

#[derive(Debug)]
enum Cmd {
    Play(PathBuf, f64),
    Pause,
    Resume,
    Seek(f64),
    Stop,
}

/// What the player is doing.
#[derive(Clone, Debug, Default)]
pub struct State {
    pub path: Option<PathBuf>,
    /// seconds in
    pub pos: f64,
    pub playing: bool,
    pub error: Option<String>,
}

fn player() -> &'static (mpsc::Sender<Cmd>, Arc<Mutex<State>>) {
    static P: OnceLock<(mpsc::Sender<Cmd>, Arc<Mutex<State>>)> = OnceLock::new();
    P.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let state = Arc::new(Mutex::new(State::default()));
        let st = state.clone();
        let _ = std::thread::Builder::new().name("nt-audio".into()).spawn(move || run(rx, st));
        (tx, state)
    })
}

fn run(rx: mpsc::Receiver<Cmd>, state: Arc<Mutex<State>>) {
    let set = |f: &dyn Fn(&mut State)| f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
    // (no device under tests: the state follows the commands, nothing sounds)
    let mut sink: Option<rodio::MixerDeviceSink> = None;
    let mut p: Option<rodio::Player> = None;
    loop {
        let cmd = match rx.recv_timeout(std::time::Duration::from_millis(40)) {
            Ok(c) => Some(c),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        match cmd {
            Some(Cmd::Play(path, from)) => {
                if let Some(old) = p.take() {
                    old.stop();
                }
                set(&|s| {
                    s.path = Some(path.clone());
                    s.pos = from;
                    s.playing = true;
                    s.error = None;
                });
                if cfg!(test) {
                    continue;
                }
                if sink.is_none() {
                    match rodio::DeviceSinkBuilder::open_default_sink() {
                        Ok(mut s) => {
                            s.log_on_drop(false);
                            sink = Some(s);
                        }
                        Err(e) => {
                            let e = e.to_string();
                            set(&|s| {
                                s.playing = false;
                                s.error = Some(e.clone());
                            });
                            continue;
                        }
                    }
                }
                let opened = std::fs::File::open(&path).map_err(|e| e.to_string()).and_then(|f| rodio::Decoder::try_from(f).map_err(|e| e.to_string()));
                match (opened, sink.as_ref()) {
                    (Ok(d), Some(s)) => {
                        let pl = rodio::Player::connect_new(s.mixer());
                        pl.append(d);
                        if from > 0.0 {
                            let _ = pl.try_seek(std::time::Duration::from_secs_f64(from));
                        }
                        pl.play();
                        p = Some(pl);
                    }
                    (Err(e), _) => set(&|s| {
                        s.playing = false;
                        s.error = Some(e.clone());
                    }),
                    _ => {}
                }
            }
            Some(Cmd::Pause) => {
                if let Some(pl) = &p {
                    pl.pause();
                }
                set(&|s| s.playing = false);
            }
            Some(Cmd::Resume) => {
                if let Some(pl) = &p {
                    pl.play();
                }
                set(&|s| s.playing = true);
            }
            Some(Cmd::Seek(t)) => {
                if let Some(pl) = &p {
                    let _ = pl.try_seek(std::time::Duration::from_secs_f64(t.max(0.0)));
                }
                set(&|s| s.pos = t.max(0.0));
            }
            Some(Cmd::Stop) => {
                if let Some(pl) = p.take() {
                    pl.stop();
                }
                set(&|s| {
                    s.playing = false;
                    s.path = None;
                    s.pos = 0.0;
                });
            }
            None => {}
        }
        // where it is, and the end
        if let Some(pl) = &p {
            let pos = pl.get_pos().as_secs_f64();
            let ended = pl.empty();
            set(&|s| {
                if s.playing {
                    s.pos = pos;
                }
                if ended {
                    s.playing = false;
                    s.pos = 0.0;
                }
            });
            if ended {
                p = None;
            }
        }
    }
}

pub fn state() -> State {
    player().1.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn send(c: Cmd) {
    let _ = player().0.send(c);
}

/// Play / Pause on an audio card: this file from where it was, or paused.
pub fn toggle(path: &Path) {
    let st = state();
    match (st.path.as_deref() == Some(path), st.playing) {
        (true, true) => send(Cmd::Pause),
        (true, false) => send(Cmd::Resume),
        _ => send(Cmd::Play(path.to_path_buf(), 0.0)),
    }
}

/// A click on the card's bar: there.
pub fn seek(path: &Path, t: f64) {
    if state().path.as_deref() == Some(path) {
        send(Cmd::Seek(t));
    } else {
        send(Cmd::Play(path.to_path_buf(), t));
    }
}

/// The note closed or the card gone: stopped.
pub fn stop() {
    if state().path.is_some() {
        send(Cmd::Stop);
    }
}

/// "0:42", "1:05:09".
pub fn clock(t: f64) -> String {
    let s = t.max(0.0).round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A press on an audio card's button or bar (the hits painted last frame).
pub fn press(ed: &mut Editor, p: Pos2) -> bool {
    for (_, button, bar, path, len) in ed.media.audio_hits.clone() {
        if button.expand(4.0).contains(p) {
            toggle(&path);
            ed.request_repaint = true;
            return true;
        }
        if let (Some(len), true) = (len, bar.expand2(egui::vec2(0.0, 6.0)).contains(p)) {
            seek(&path, len * ((p.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0) as f64);
            ed.request_repaint = true;
            return true;
        }
    }
    false
}

/// The rects of an audio card: (the play button, the bar).
pub fn card_parts(r: Rect, s: f32) -> (Rect, Rect) {
    let button = Rect::from_center_size(egui::pos2(r.min.x + 12.0 * s + 17.0 * s, r.center().y), egui::vec2(34.0 * s, 34.0 * s));
    let bar = Rect::from_min_max(egui::pos2(button.max.x + 12.0 * s, r.min.y + 40.0 * s), egui::pos2(r.max.x - 14.0 * s - 70.0 * s, r.min.y + 44.0 * s));
    (button, bar)
}
