//! Object hyperlinks: Format > Add Link (Webpage / Email / Phone / Slide) and clicking links in
//! the slideshow. Web/email/phone links are opened with the system's default app (kn_ui::open_url).

use egui::{Pos2, Rect, RichText};
use kn_model::{Point, SlideScene};

use crate::app::App;

/// What a link does when clicked in a slideshow.
pub enum LinkAction {
    Open(String),
    /// 0-based slide index
    Slide(usize),
    Next,
    Previous,
    First,
    Last,
    /// Keynote's "Last slide viewed".
    LastViewed,
    /// "?slideid=<slide node UUID>" (resolve with Document::slide_for_link_uuid).
    SlideId(String),
    Exit,
}

/// Interpret a stored hyperlink_url.
pub fn action(url: &str) -> LinkAction {
    let u = url.trim();
    // Keynote's slide links (format confirmed against a Keynote-made file when available)
    let lower = u.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("?slide=").or_else(|| lower.strip_prefix("slide:")) {
        return match rest {
            "next" | "+1" => LinkAction::Next,
            "previous" | "prev" | "-1" => LinkAction::Previous,
            "first" => LinkAction::First,
            "last" => LinkAction::Last,
            n => n.parse::<usize>().map(|n| LinkAction::Slide(n.saturating_sub(1))).unwrap_or(LinkAction::Next),
        };
    }
    // Keynote's own forms (verified): ?action=exitpresentation, ?action=retreat, ?slideid=UUID
    if lower == "?exit" || lower == "exit:" || lower == "?action=exitpresentation" {
        return LinkAction::Exit;
    }
    if lower == "?action=retreat" {
        return LinkAction::LastViewed;
    }
    if let Some(id) = u.strip_prefix("?slideid=") {
        return LinkAction::SlideId(id.to_string());
    }
    LinkAction::Open(u.to_string())
}

/// Open a URL with the desktop's default handler without blocking the UI.
pub fn open_url(url: &str) {
    kn_ui::open_url(url);
}

/// The hyperlink of the topmost linked object under a screen point (slide drawn into `rect`).
pub fn link_at(scene: &SlideScene, rect: Rect, pos: Pos2) -> Option<String> {
    if !rect.contains(pos) {
        return None;
    }
    let k = rect.width() / scene.size.w;
    let p = Point::new((pos.x - rect.min.x) / k, (pos.y - rect.min.y) / k);
    scene.drawables.iter().rev().find_map(|d| {
        // a web video opens its page (Keynote plays it in place)
        let web = match &d.kind {
            kn_model::DrawableKind::Image(im) => im.web_video.as_ref().map(|w| w.url.clone()).filter(|u| !u.is_empty()),
            _ => None,
        };
        let url = d.hyperlink.clone().or(web)?;
        let l = d.geom.to_local(p);
        (l.x >= 0.0 && l.y >= 0.0 && l.x <= d.geom.w.max(4.0) && l.y <= d.geom.h.max(4.0)).then_some(url)
    })
}

#[derive(Default)]
pub struct LinkDialog {
    pub open: bool,
    kind: usize,
    text: String,
    /// Slide links: 0 next, 1 previous, 2 first, 3 last, 4 slide number, 5 exit slideshow.
    slide_kind: usize,
    slide_no: usize,
}

impl LinkDialog {
    pub fn show(&mut self, current: Option<&str>) {
        self.open = true;
        let cur = current.unwrap_or("");
        (self.kind, self.text) = if let Some(m) = cur.strip_prefix("mailto:") {
            (1, m.to_string())
        } else if let Some(t) = cur.strip_prefix("tel:") {
            (2, t.to_string())
        } else {
            (0, cur.to_string())
        };
        self.slide_no = self.slide_no.max(1);
        if !cur.is_empty() {
            let slide = match action(cur) {
                LinkAction::Next => Some((0, 1)),
                LinkAction::Previous => Some((1, 1)),
                LinkAction::First => Some((2, 1)),
                LinkAction::Last => Some((3, 1)),
                LinkAction::Slide(n) => Some((4, n + 1)),
                LinkAction::SlideId(_) => Some((4, 1)),
                LinkAction::Exit => Some((5, 1)),
                LinkAction::LastViewed => Some((6, 1)),
                LinkAction::Open(_) => None,
            };
            if let Some((k, n)) = slide {
                (self.kind, self.slide_kind, self.slide_no, self.text) = (3, k, n, String::new());
            }
        }
    }

    /// Open on a given kind (Format > Add Link > Slide / Webpage / Email / Phone Number).
    pub fn show_kind(&mut self, current: Option<&str>, kind: usize) {
        self.show(current);
        if current.is_none() {
            self.kind = kind;
        }
    }
}

pub fn dialog(app: &mut App, ctx: &egui::Context) {
    if !app.link_dialog.open {
        return;
    }
    let mut open = true;
    let mut apply: Option<Option<String>> = None;
    let mut slide_target: Option<usize> = None;
    egui::Window::new("Link").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 120.0]).open(&mut open).show(ctx, |ui| {
        let d = &mut app.link_dialog;
        ui.horizontal(|ui| {
            ui.label("Link to:");
            for (i, l) in ["Webpage", "Email", "Phone Number", "Slide"].iter().enumerate() {
                ui.selectable_value(&mut d.kind, i, *l);
            }
        });
        let r = if d.kind == 3 {
            for (i, l) in ["Next slide", "Previous slide", "First slide", "Last slide", "Slide", "Exit Slideshow", "Last slide viewed"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.slide_kind, i, *l);
                    if i == 4 {
                        ui.add_enabled_ui(d.slide_kind == 4, |ui| {
                            let mut n = d.slide_no as f64;
                            if crate::theme::stepper(ui, &mut n, 1.0, 1.0..=9999.0, "", 0) {
                                d.slide_no = n as usize;
                            }
                        });
                    }
                });
            }
            ui.label("")
        } else {
            let hint = ["https://example.com", "name@example.com", "+1 555 123 4567"][d.kind];
            let r = ui.add(egui::TextEdit::singleline(&mut d.text).hint_text(hint).desired_width(320.0));
            r.request_focus();
            r
        };
        ui.label(RichText::new("Links work when you play the slideshow.").small().color(crate::theme::TEXT_DIM()));
        ui.horizontal(|ui| {
            if ui.button("Remove Link").clicked() {
                apply = Some(None);
            }
            if ui.button(RichText::new("Done").strong()).clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                let t = d.text.trim();
                let slide_url = ["?slide=next", "?slide=previous", "?slide=first", "?slide=last", "", "?action=exitpresentation", "?action=retreat"][d.slide_kind.min(6)];
                if d.kind == 3 && d.slide_kind == 4 {
                    slide_target = Some(d.slide_no.saturating_sub(1));
                }
                let url = match d.kind {
                    3 if d.slide_kind == 4 => String::new(),
                    3 => slide_url.to_string(),
                    1 => format!("mailto:{t}"),
                    2 => format!("tel:{}", t.replace(' ', "")),
                    _ if t.contains("://") => t.to_string(),
                    _ => format!("https://{t}"),
                };
                apply = Some((d.kind == 3 || !t.is_empty()).then_some(url));
            }
        });
    });
    app.link_dialog.open = open;
    if let Some(url) = apply {
        app.link_dialog.open = false;
        let ids = app.sel.clone();
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, if url.is_some() { "Add Link" } else { "Remove Link" });
            for id in ids {
                match (slide_target, slide) {
                    // a particular slide: Keynote's ?slideid=<UUID> with the back-index
                    (Some(t), Some(s)) if url.is_some() => {
                        doc.set_slide_link(&mut app.hist, id, s, t);
                    }
                    _ => doc.set_hyperlink(&mut app.hist, id, url.as_deref()),
                }
            }
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
    }
}
