//! Visual checks: each one-feature research note (read by nt-core from the research NoteStore),
//! drawn by the editor alone in a window at 2x like the Mac captures, saved as PNG for the
//! montages against Apple's screenshots (tools in the shared notes; no human needed).
//!
//! `NT_EDITOR_SNAP_DIR=out TZ=America/New_York cargo test -p nt-app --lib editor::snapshot -- --ignored`
//! (NT_EDITOR_NOTES="34 Highlight Colors,19 Table" picks notes; NT_EDITOR_SIZE=WxH points.)

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Rect, pos2, vec2};
use nt_core::model;

use super::Editor;

const RESEARCH: &str = "CascadeProjects/notes-research-src/partial-3/Notes-Research";

fn research() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(RESEARCH)
}

/// The research notes, their attachments pointed at the sample inputs (the store ships no media).
fn notes() -> Vec<model::Note> {
    let want: Option<Vec<String>> = std::env::var("NT_EDITOR_NOTES").ok().filter(|s| !s.is_empty()).map(|s| s.split(',').map(|x| x.trim().to_string()).collect());
    let mut v = all_notes();
    v.retain(|n| want.as_ref().is_none_or(|w| w.contains(&n.title)));
    v
}

/// Every research note (not deleted), its attachments pointed at the sample inputs.
fn all_notes() -> Vec<model::Note> {
    let r = research();
    let lib = nt_core::import::read_library(&r.join("sample-files/store/NoteStore-BrambleResearch-only.sqlite")).expect("research store");
    let inputs = r.join("sample-files/inputs");
    let mut notes = lib.notes;
    for n in &mut notes {
        for a in &mut n.attachments {
            if a.file.as_ref().is_some_and(|f| f.exists()) {
                continue;
            }
            let f = match a.kind {
                model::AttachmentKind::Image => Some("bramble-gradient.png"),
                model::AttachmentKind::Pdf => Some("bramble-doc.pdf"),
                model::AttachmentKind::File => Some("bramble-text.txt"),
                _ => None,
            };
            if let Some(f) = f {
                a.file = Some(inputs.join(f));
                if a.name.is_empty() {
                    a.name = f.into();
                }
            }
        }
    }
    notes.retain(|n| !n.deleted);
    notes.push(not_downloaded_note());
    notes.sort_by(|a, b| a.title.cmp(&b.title));
    notes
}

/// The user's library's iCloud stubs (a gallery, a table, an untyped one, a drawing whose bundle
/// never arrived) and an attachment with no row at all, each on a line of its own.
pub(super) fn not_downloaded_note() -> model::Note {
    use model::{Attachment, AttachmentKind as K, ParaStyle, Paragraph, Span};
    let stub = |id: &str, uti: &str, kind: K, w: f64, h: f64| Attachment { id: id.into(), uti: uti.into(), kind, width: w, height: h, not_downloaded: true, ..Default::default() };
    let atts = vec![
        stub("ND-SCAN", "com.apple.notes.gallery", K::Scan, 0.0, 0.0),
        stub("ND-TABLE", model::UTI_TABLE, K::Table, 0.0, 0.0),
        stub("ND-FILE", "", K::File, 0.0, 0.0),
        stub("ND-DRAW", model::UTI_PAPER, K::Drawing, 768.0, 240.0),
    ];
    let mut paras = vec![Paragraph::plain(ParaStyle::Title, "zz Not Downloaded")];
    for (id, uti) in atts.iter().map(|a| (a.id.as_str(), a.uti.as_str())).chain([("ND-GONE", "public.jpeg")]) {
        paras.push(Paragraph { spans: vec![Span::token(id, uti)], ..Default::default() });
    }
    paras.push(Paragraph::plain(ParaStyle::Body, "after"));
    let mut n = model::Note { id: "zz-not-downloaded".into(), title: "zz Not Downloaded".into(), attachments: atts, ..Default::default() };
    n.body.paragraphs = paras;
    n
}

/// Scenes beyond the plain notes: (name, note title, what to do before the snapshot).
fn scenes() -> Vec<(String, String, String)> {
    let mut v: Vec<(String, String, String)> = Vec::new();
    for s in std::env::var("NT_EDITOR_SCENES").unwrap_or_default().split(';').filter(|s| !s.trim().is_empty()) {
        let mut it = s.splitn(3, '|');
        let (a, b, c) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
        v.push((a.trim().into(), b.trim().into(), c.trim().into()));
    }
    v
}

/// Input events for a scene: "ev click x y", "ev dclick x y", "ev drag x1 y1 x2 y2", "ev text …",
/// "ev key Enter" / "ev key ctrl+z" (editor points).
fn parse_events(action: &str) -> std::collections::VecDeque<Vec<egui::Event>> {
    use egui::{Event, Key, Modifiers, PointerButton};
    let mut out = std::collections::VecDeque::new();
    let btn = |p: egui::Pos2, pressed: bool| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    for step in action.split(',').map(str::trim) {
        let Some(rest) = step.strip_prefix("ev ") else { continue };
        let (cmd, arg) = rest.split_once(' ').unwrap_or((rest, ""));
        let n: Vec<f32> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        match (cmd, n.as_slice()) {
            ("click", [x, y]) | ("dclick", [x, y]) => {
                let p = pos2(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                for _ in 0..if cmd == "dclick" { 2 } else { 1 } {
                    out.push_back(vec![btn(p, true)]);
                    out.push_back(vec![btn(p, false)]);
                }
                out.push_back(vec![]);
            }
            ("rclick", [x, y]) => {
                let p = pos2(*x, *y);
                out.push_back(vec![Event::PointerMoved(p)]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE }]);
                out.push_back(vec![Event::PointerButton { pos: p, button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE }]);
                out.push_back(vec![]);
            }
            ("wheel", [dy]) => {
                out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, *dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE }]);
            }
            ("flick", [dy, n]) => {
                out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 0.0), phase: egui::TouchPhase::Start, source: egui::MouseWheelSource::Trackpad, modifiers: Modifiers::NONE }]);
                for _ in 0..*n as usize {
                    out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, *dy), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Trackpad, modifiers: Modifiers::NONE }]);
                }
                out.push_back(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 0.0), phase: egui::TouchPhase::End, source: egui::MouseWheelSource::Trackpad, modifiers: Modifiers::NONE }]);
            }
            ("wait", [n]) => {
                for _ in 0..*n as usize {
                    out.push_back(vec![]);
                }
            }
            // a menu command in the timeline: "ev cmd Insert Drawing"
            ("cmd", _) => out.push_back(vec![Event::Text(format!("\u{0}CMD:{arg}"))]),
            ("move", [x, y]) => {
                out.push_back(vec![Event::PointerMoved(pos2(*x, *y))]);
                out.push_back(vec![]);
            }
            // a stroke through several points: "ev stroke x1 y1 x2 y2 x3 y3 …"
            ("stroke", pts) if pts.len() >= 4 => {
                let ps: Vec<egui::Pos2> = pts.chunks(2).map(|c| pos2(c[0], c[1])).collect();
                out.push_back(vec![Event::PointerMoved(ps[0])]);
                out.push_back(vec![btn(ps[0], true)]);
                for w in ps.windows(2) {
                    for i in 1..=6 {
                        out.push_back(vec![Event::PointerMoved(w[0] + (w[1] - w[0]) * (i as f32 / 6.0))]);
                    }
                }
                out.push_back(vec![btn(*ps.last().unwrap(), false)]);
                out.push_back(vec![]);
            }
            ("drag", [x1, y1, x2, y2]) => {
                let (a, b) = (pos2(*x1, *y1), pos2(*x2, *y2));
                out.push_back(vec![Event::PointerMoved(a)]);
                out.push_back(vec![btn(a, true)]);
                for i in 1..=8 {
                    out.push_back(vec![Event::PointerMoved(a + (b - a) * (i as f32 / 8.0))]);
                }
                out.push_back(vec![btn(b, false)]);
                out.push_back(vec![]);
            }
            ("text", _) => {
                for c in arg.replace("\\s", " ").chars() {
                    out.push_back(vec![Event::Text(c.to_string())]);
                }
            }
            ("key", _) => {
                let (m, k) = match arg.split_once('+') {
                    Some((m, k)) => (m, k),
                    None => ("", arg),
                };
                let mut mods = Modifiers::NONE;
                if m.contains("ctrl") {
                    mods.ctrl = true;
                    mods.command = true;
                }
                if m.contains("shift") {
                    mods.shift = true;
                }
                if m.contains("alt") {
                    mods.alt = true;
                }
                if let Some(key) = Key::from_name(k) {
                    out.push_back(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }]);
                    out.push_back(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods }]);
                }
            }
            _ => {}
        }
    }
    out
}

struct Snap {
    pending_cmd: Vec<String>,
    said: String,
    events: std::collections::VecDeque<Vec<egui::Event>>,
    jobs: Vec<(String, model::Note, String)>,
    idx: usize,
    frames: u32,
    out: PathBuf,
    ed: Editor,
    done: Arc<AtomicBool>,
    waiting: bool,
    sized: bool,
    size: egui::Vec2,
}

impl Snap {
    fn act(&mut self, ctx: &egui::Context, action: &str) {
        for step in action.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (cmd, arg) = step.split_once(' ').unwrap_or((step, ""));
            match cmd {
                "focus" => self.ed.focused = true,
                "caret" => {
                    let n: Vec<usize> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if let [p, o] = n[..] {
                        self.ed.set_caret(super::Loc { cell: None, para: p, off: o }, false);
                    }
                }
                "select" => {
                    let n: Vec<usize> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if let [p, a, q, b] = n[..] {
                        self.ed.anchor = super::Loc { cell: None, para: p, off: a };
                        self.ed.head = super::Loc { cell: None, para: q, off: b };
                    }
                }
                "type" => {
                    for c in arg.replace("\\n", "\n").chars() {
                        if c == '\n' {
                            self.ed.return_key();
                        } else {
                            self.ed.type_text(&c.to_string());
                        }
                    }
                }
                "key" => match arg {
                    "Tab" => self.ed.tab_key(false),
                    "Return" => self.ed.return_key(),
                    _ => {}
                },
                "cmd" => self.ed.command_label(Some(ctx), arg),
                "hover" => {
                    let n: Vec<usize> = arg.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    self.ed.view.hover = n.first().copied();
                }
                _ => {}
            }
        }
    }
}

impl eframe::App for Snap {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // only the scene's own events: whoever's mouse is over the window plays no part
        raw.events.clear();
        raw.dropped_files.clear();
        if self.frames > 5 && !self.waiting {
            if let Some(batch) = self.events.pop_front() {
                for e in batch {
                    match &e {
                        egui::Event::Text(t) if t.starts_with("\u{0}CMD:") => self.pending_cmd.push(t[5..].to_string()),
                        _ => raw.events.push(e),
                    }
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 2 pixels a point, as the Mac's Retina captures
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        if !self.sized {
            kn_ui::theme::apply_appearance(&ctx, std::env::var_os("NT_EDITOR_DARK").is_some());
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(self.size * 2.0 / native));
            self.sized = true;
        }
        ctx.set_zoom_factor(2.0 / native);
        // (NT_EDITOR_LIGHT_PAPER: "Use dark backgrounds for note content" off)
        self.ed.light_paper = std::env::var_os("NT_EDITOR_LIGHT_PAPER").is_some();
        if self.waiting {
            if self.done.swap(false, Ordering::Relaxed) {
                self.waiting = false;
                self.idx += 1;
                self.frames = 0;
            } else {
                // keep drawing the same picture until the screenshot arrives
                ctx.request_repaint();
            }
        }
        let Some((name, note, action)) = self.jobs.get(self.idx).cloned() else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        };
        let mut note = note;
        if self.frames == 0 {
            self.ed = Editor::new();
            self.ed.load(&note);
            self.ed.focused = false;
        }
        if self.frames == 3 {
            self.act(&ctx, &action);
            self.events = parse_events(&action);
        }
        self.ed.date_line = crate::dates::long(note.modified);
        let rect = Rect::from_min_size(pos2(0.0, 0.0), self.size);
        let popover = action.contains("popover");
        let cmds = std::mem::take(&mut self.pending_cmd);
        let r = super::frame(&mut self.ed, ui, rect, &mut note, &cmds);
        if r.open_link.is_some() || r.open_note.is_some() || r.tag_clicked.is_some() {
            self.said = format!("{:?} {:?} {:?}", r.open_link, r.open_note, r.tag_clicked);
        }
        if popover {
            // where Notes' popover sat in popover-format-Aa.png (window points), as the shell's popup would
            let at = pos2(self.pop_x(), 56.0 - 52.0);
            egui::Area::new(egui::Id::unique("snap-pop")).fixed_pos(at).order(egui::Order::Foreground).show(&ctx, |ui| {
                let r = Rect::from_min_size(ui.cursor().min, super::popover::SIZE);
                ui.painter().rect_filled(r.translate(vec2(0.0, 4.0)).expand(6.0), egui::CornerRadius::same(22), egui::Color32::from_black_alpha(10));
                ui.painter().rect_filled(r, egui::CornerRadius::same(18), egui::Color32::from_gray(251));
                super::popover::show(&mut self.ed, ui);
            });
        }
        if std::env::var_os("NT_EDITOR_YLOG").is_some() && self.frames > 5 {
            if let Some(f) = &self.ed.view.flow {
                let ys: Vec<String> = f.boxes.iter().filter(|b| self.ed.doc.paras[b.para].style == nt_core::model::ParaStyle::Checklist).map(|b| format!("{}={:.1}", self.ed.doc.paras[b.para].text, self.ed.view.origin.y + b.top)).collect();
                eprintln!("ylog {} {:.3} {}", self.frames, ctx.input(|i| i.time), ys.join(" "));
            }
        }
        if std::env::var_os("NT_EDITOR_SCROLLLOG").is_some() && self.frames > 5 {
            eprintln!("scroll {} {:.3} {:.1}", self.frames, ctx.input(|i| i.time), self.ed.view.scroll.offset);
        }
        if std::env::var_os("NT_EDITOR_DEBUG").is_some() && self.frames % 5 == 0 {
            eprintln!("frame {} styles {:?} undo {} head {:?} focused {} ptr {:?} ev {}", self.frames, self.ed.doc.paras.iter().map(|p| p.style).collect::<Vec<_>>(), self.ed.undo.len(), self.ed.head, self.ed.focused, ctx.input(|i| i.pointer.hover_pos()), self.events.len());
        }
        self.frames += 1;
        let ready = self.frames > 40 && self.events.is_empty() && super::layout::install_faces(&ctx, true);
        if ready && !self.waiting {
            if !action.is_empty() {
                let lines: Vec<String> = self.ed.doc.paras.iter().map(|p| format!("{:?}{}{} {:?}", p.style, if p.checked { "✓" } else { "" }, if p.indent > 0 { format!(">{}", p.indent) } else { String::new() }, p.text.replace('\u{FFFC}', "◻"))).collect();
                let mut lines = lines;
                if let Some(f) = &self.ed.view.flow {
                    for b in &f.boxes {
                        lines.push(format!("box {} top {:.1} baseline {:.1} x {:.1} h {:.1}", b.para, self.ed.view.origin.y + b.top, self.ed.view.origin.y + b.baseline(), self.ed.view.origin.x + b.x, b.height));
                    }
                }
                lines.push(format!("sel {:?} {:?} table_sel {:?}", self.ed.anchor, self.ed.head, self.ed.table_sel));
                lines.push(format!("response {}", std::mem::take(&mut self.said)));
                if let Some(d) = &self.ed.draw {
                    lines.push(format!("draw tool {:?} colour {:?} ruler {} selected {}", d.tool, d.color, d.ruler.is_some(), d.selected.len()));
                }
                if let Some(d) = self.ed.drawing() {
                    for s in &d.strokes {
                        lines.push(format!("stroke {:?} {:?} {} pts", s.ink, s.color, s.points.len()));
                    }
                    for s in &d.shapes {
                        lines.push(format!("shape {:?} frame {:?} rot {:.2} stroke {:?} fill {:?} w {} text {:?}", s.kind, s.frame, s.rotation, s.stroke, s.fill, s.line_width, s.text));
                    }
                }
                let _ = std::fs::write(self.out.join(format!("{name}.txt")), lines.join("\n") + "\n");
            }
            let path = self.out.join(format!("{name}.png"));
            let done = self.done.clone();
            let cb = egui::ScreenshotCallback::new(move |img: Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    let _ = buf.save(&path);
                    eprintln!("saved {}", path.display());
                }
                done.store(true, Ordering::Relaxed);
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
            self.waiting = true;
        }
        ctx.request_repaint();
    }
}

impl Snap {
    /// The popover's left edge in editor points (popover-format-Aa.png: 967 px window x, the
    /// editor column at 343 pt there).
    fn pop_x(&self) -> f32 {
        std::env::var("NT_EDITOR_POP_X").ok().and_then(|v| v.parse().ok()).unwrap_or(967.0 / 2.0 - 343.0)
    }
}

#[test]
#[ignore]
fn snapshot() {
    let Some(out) = std::env::var_os("NT_EDITOR_SNAP_DIR").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    let notes = notes();
    let mut jobs: Vec<(String, model::Note, String)> = notes.iter().map(|n| (n.title.clone(), n.clone(), String::new())).collect();
    let all = all_notes();
    for (name, title, action) in scenes() {
        if let Some(n) = all.iter().find(|n| n.title == title && !n.deleted) {
            jobs.push((name, n.clone(), action));
        }
    }
    if std::env::var_os("NT_EDITOR_SCENES_ONLY").is_some() {
        jobs.retain(|j| !j.2.is_empty());
    }
    let size = std::env::var("NT_EDITOR_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some(vec2(w.parse().ok()?, h.parse().ok()?)))).unwrap_or(vec2(853.5, 600.0));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size(size).with_title("nt-editor-snap").with_decorations(false),
        event_loop_builder: Some(Box::new(|b| {
            use winit::platform::wayland::EventLoopBuilderExtWayland;
            EventLoopBuilderExtWayland::with_any_thread(b, true);
            use winit::platform::x11::EventLoopBuilderExtX11;
            EventLoopBuilderExtX11::with_any_thread(b, true);
        })),
        ..Default::default()
    };
    // SAFETY: tests run this alone (it is ignored by default)
    unsafe { std::env::set_var("NT_EDITOR_SNAP", "1") };
    let app = Snap { pending_cmd: Vec::new(), said: String::new(), events: Default::default(), jobs, idx: 0, frames: 0, out, ed: Editor::new(), done: Arc::new(AtomicBool::new(false)), waiting: false, sized: false, size };
    eframe::run_native("nt-editor-snap", options, Box::new(move |_| Ok(Box::new(app)))).unwrap();
}

#[test]
#[ignore]
fn dump_note() {
    let t = std::env::var("NT_EDITOR_NOTES").unwrap_or_default();
    for n in notes().iter().filter(|n| n.title == t) {
        for p in &n.body.paragraphs {
            eprintln!("{:?} {:?}", p.style, p.text());
        }
    }
}
