//! The sidebar (Notes 4.13, popover-view-options.png): Shared, then each account — its header
//! (bold 11 pt grey, collapsible), "All <account>", Quick Notes, Notes, folders with their
//! subfolders behind disclosure chevrons, Smart Folders with the gear — Recently Deleted when it
//! holds notes, and the Tags browser. Rows are 32 pt apart, 13 pt names, yellow outline icons,
//! counts in grey on the right.

use egui::{pos2, vec2, Align2, Color32, Rect, Sense};
use nt_core::model::FolderKind;

use crate::app::{App, Focus, Place};
use crate::icons::{draw, Glyph};
use crate::look;

const ROW: f32 = 32.0;

/// One line of the sidebar.
enum Line {
    Header { key: String, text: String },
    Row { place: Place, depth: usize, glyph: Glyph, name: String, count: usize, kids: Option<String>, folder: Option<String> },
    Tags(Vec<String>),
    Gap(f32),
}

fn lines(app: &App) -> Vec<Line> {
    let mut out = Vec::new();
    let shared = app.count(&Place::Shared);
    if shared > 0 {
        out.push(Line::Row { place: Place::Shared, depth: 0, glyph: Glyph::Shared, name: "Shared".into(), count: shared, kids: None, folder: None });
        out.push(Line::Gap(6.0));
    }
    for (k, a) in app.library.accounts.iter().enumerate() {
        if k > 0 || shared > 0 {
            out.push(Line::Gap(6.0));
        }
        let key = format!("acct:{}", a.id);
        out.push(Line::Header { key: key.clone(), text: crate::app::account_label(a) });
        if app.collapsed.contains(&key) {
            continue;
        }
        let folders: Vec<_> = app.library.folders.iter().filter(|f| f.account == a.id && f.kind != FolderKind::Trash).collect();
        let tops = folders.iter().filter(|f| f.parent.is_none()).count();
        if tops > 1 || folders.iter().any(|f| f.parent.is_some()) {
            out.push(Line::Row { place: Place::All(a.id.clone()), depth: 0, glyph: Glyph::Folder, name: crate::app::all_label(&crate::app::account_label(a)), count: app.count(&Place::All(a.id.clone())), kids: None, folder: None });
        }
        // Quick Notes, then Notes, then the rest by name (Smart Folders among them)
        let rank = |f: &nt_core::model::Folder| -> (u8, String) {
            let r = if f.name == "Quick Notes" { 0 } else if f.default || f.name == "Notes" { 1 } else { 2 };
            (r, f.name.to_lowercase())
        };
        fn walk(app: &App, all: &[&nt_core::model::Folder], parent: Option<&str>, depth: usize, out: &mut Vec<Line>, rank: &dyn Fn(&nt_core::model::Folder) -> (u8, String)) {
            let mut level: Vec<&&nt_core::model::Folder> = all.iter().filter(|f| f.parent.as_deref() == parent).collect();
            level.sort_by_key(|f| rank(f));
            for f in level {
                let has_kids = all.iter().any(|c| c.parent.as_deref() == Some(f.id.as_str()));
                let glyph = if App::is_smart(f) { Glyph::SmartFolder } else if f.name == "Quick Notes" && depth == 0 { Glyph::QuickNote } else { Glyph::Folder };
                let place = Place::Folder(f.id.clone());
                out.push(Line::Row { count: app.count(&place), place, depth, glyph, name: f.name.clone(), kids: has_kids.then(|| f.id.clone()), folder: Some(f.id.clone()) });
                if has_kids && !app.collapsed.contains(&f.id) {
                    walk(app, all, Some(&f.id), depth + 1, out, rank);
                }
            }
        }
        walk(app, &folders, None, 0, &mut out, &rank);
        let deleted = app.count(&Place::Deleted(a.id.clone()));
        if deleted > 0 {
            out.push(Line::Row { place: Place::Deleted(a.id.clone()), depth: 0, glyph: Glyph::Trash, name: "Recently Deleted".into(), count: deleted, kids: None, folder: None });
        }
    }
    let tags = app.all_tags();
    if !tags.is_empty() {
        out.push(Line::Gap(6.0));
        out.push(Line::Header { key: "tags".into(), text: "Tags".into() });
        if !app.collapsed.contains("tags") {
            out.push(Line::Tags(tags));
        }
    }
    out
}

pub fn show(app: &mut App, ui: &mut egui::Ui, top_pad: f32) {
    let full = ui.max_rect();
    ui.painter().rect_filled(full, egui::CornerRadius { sw: app.corner.round() as u8, ..Default::default() }, look::sidebar_bg());
    // (the rows' view runs up behind the toolbar's strip, their content starting below it: they
    // scroll on under the strip, where its feathered blur thins to them)
    let area = full;
    let strip = Rect::from_min_max(full.min, pos2(full.max.x, full.min.y + top_pad));
    // (a finger scrolls the sidebar: kn_ui::touch)
    kn_ui::touch::pan_area(area);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area).id_salt("sidebar-rows"));
    let ls = lines(app);
    let focused = app.focus == Focus::Sidebar;
    let mut go: Option<Place> = None;
    let mut toggle: Option<String> = None;
    let mut ctx_action: Option<String> = None;
    let mut tag_color: Option<String> = None;
    let mut sc = std::mem::take(&mut app.sidebar_scroll);
    // (each row as drawn, and whether it's the selected one: what the toolbar's strip blurs)
    let mut drawn: Vec<(Rect, bool)> = Vec::new();
    crate::app::scroll_area(&mut sc, &mut child, "sidebar", |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let w = area.width();
        ui.add_space(top_pad + 2.0);
        let mut after_rows = false;
        for line in &ls {
            match line {
                Line::Gap(h) => {
                    ui.add_space(*h);
                }
                Line::Header { key, text } => {
                    // (38 pt from the last row's middle to a header's; 26 from it to the first row's)
                    if after_rows {
                        ui.add_space(4.0);
                    }
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
                    // (a folder dropped on its account's header goes back to the top level)
                    if let Some(acct) = key.strip_prefix("acct:") {
                        let ours = |f: &str| app.folder(f).is_some_and(|fo| fo.account == acct && fo.parent.is_some());
                        if resp.dnd_hover_payload::<crate::app::FolderDrag>().is_some_and(|d| ours(&d.0)) {
                            ui.painter().rect_stroke(Rect::from_min_max(pos2(r.min.x + 10.0, r.min.y), pos2(r.max.x - 10.0, r.max.y)), 8.0, egui::Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
                        }
                        if let Some(d) = resp.dnd_release_payload::<crate::app::FolderDrag>() {
                            if ours(&d.0) {
                                ctx_action = Some(format!("nest:{}|", d.0));
                            }
                        }
                    }
                    let p = ui.painter();
                    let cy = r.center().y - 2.0;
                    p.text(pos2(r.min.x + 16.0, cy), Align2::LEFT_CENTER, text, look::bold(11.0), look::section_text());
                    if resp.hovered() {
                        let g = if app.collapsed.contains(key) { Glyph::ChevronRight } else { Glyph::ChevronDown };
                        draw(p, Rect::from_center_size(pos2(r.max.x - 22.0, cy), vec2(11.0, 11.0)), g, look::section_text());
                    }
                    if resp.clicked() {
                        toggle = Some(key.clone());
                    }
                    after_rows = false;
                }
                Line::Row { place, depth, glyph, name, count, kids, folder } => {
                    after_rows = true;
                    // (an ordinary folder takes notes and folders dropped on it, and is dragged to
                    // nest it; Notes and Quick Notes stay put)
                    let target = folder.as_ref().and_then(|f| app.folder(f)).filter(|fo| !App::is_smart(fo) && fo.kind != FolderKind::Trash).map(|fo| fo.id.clone());
                    let movable = target.as_ref().and_then(|f| app.folder(f)).is_some_and(|fo| !(fo.parent.is_none() && (fo.name == "Notes" || fo.name == "Quick Notes")));
                    let (r, resp) = ui.allocate_exact_size(vec2(w, ROW), if movable { Sense::click_and_drag() } else { Sense::click() });
                    if movable {
                        if let Some(f) = &target {
                            resp.dnd_set_drag_payload(crate::app::FolderDrag(f.clone()));
                        }
                    }
                    let p = ui.painter().clone();
                    let selected = app.place == *place && app.search.trim().is_empty();
                    let row = Rect::from_min_max(pos2(r.min.x + 10.0, r.min.y), pos2(r.max.x - 10.0, r.max.y));
                    drawn.push((row, selected));
                    if selected {
                        p.rect_filled(row, 8.0, look::sidebar_selection(focused));
                    }
                    if let Some(t) = &target {
                        // a note dropped here moves into the folder; a folder, nests in it (not in itself
                        // or a folder of its own)
                        let folder_ok = |dragged: &str| dragged != t && !is_within(app, t, dragged);
                        let hovering = resp.dnd_hover_payload::<crate::app::NoteDrag>().is_some() || resp.dnd_hover_payload::<crate::app::FolderDrag>().is_some_and(|d| folder_ok(&d.0));
                        if hovering {
                            p.rect_stroke(row, 8.0, egui::Stroke::new(2.0, look::accent()), egui::StrokeKind::Inside);
                        }
                        // (taking a release payload clears it whatever its type, so ask for the type first)
                        let carrying_note = egui::DragAndDrop::has_payload_of_type::<crate::app::NoteDrag>(ui.ctx());
                        if let Some(n) = carrying_note.then(|| resp.dnd_release_payload::<crate::app::NoteDrag>()).flatten() {
                            ctx_action = Some(format!("move:{}|{t}", n.0));
                        }
                        if let Some(d) = (!carrying_note).then(|| resp.dnd_release_payload::<crate::app::FolderDrag>()).flatten() {
                            if folder_ok(&d.0) {
                                ctx_action = Some(format!("nest:{}|{t}", d.0));
                            }
                        }
                    }
                    let d = *depth as f32;
                    let cy = r.center().y;
                    if let Some(fid) = kids {
                        let cr = Rect::from_center_size(pos2(r.min.x + 18.0 + 10.0 * d, cy), vec2(14.0, 20.0));
                        let g = if app.collapsed.contains(fid) { Glyph::ChevronRight } else { Glyph::ChevronDown };
                        draw(&p, Rect::from_center_size(cr.center(), vec2(9.0, 9.0)), g, look::count_text());
                        if ui.interact(cr, ui.id().with(("chev", fid)), Sense::click()).clicked() {
                            toggle = Some(fid.clone());
                        }
                    }
                    let ic = pos2(r.min.x + 34.0 + 11.0 * d, cy);
                    draw(&p, Rect::from_center_size(ic, vec2(19.5, 19.5)), *glyph, look::folder_icon());
                    let name_font = if selected { look::semibold(13.0) } else { look::regular(13.0) };
                    let tx = ic.x + 15.5;
                    let cw = if app.show_counts { 34.0 } else { 0.0 };
                    let room = r.max.x - 19.0 - cw - tx;
                    // (renaming: the name is a text field)
                    let renaming = folder.as_ref().is_some_and(|f| app.renaming.as_ref().is_some_and(|rn| rn.folder == *f));
                    if renaming {
                        let fr = Rect::from_min_max(pos2(tx - 3.0, cy - 10.0), pos2(r.max.x - 14.0, cy + 10.0));
                        let mut c = ui.new_child(egui::UiBuilder::new().max_rect(fr));
                        let rn = app.renaming.as_mut().unwrap();
                        let te = c.add(egui::TextEdit::singleline(&mut rn.text).id(egui::Id::new("folder-rename")).font(look::regular(13.0)).desired_width(fr.width()).margin(vec2(3.0, 1.0)));
                        if rn.fresh {
                            te.request_focus();
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), te.id) {
                                let n = rn.text.chars().count();
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                                st.store(ui.ctx(), te.id);
                            }
                            rn.fresh = false;
                        } else if te.lost_focus() {
                            let (fid, text) = (rn.folder.clone(), rn.text.clone());
                            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                            app.renaming = None;
                            if !cancel {
                                app.rename_folder(&fid, &text);
                            }
                        }
                    } else {
                        let g = p.layout_no_wrap(name.clone(), name_font.clone(), look::text());
                        let shown = if g.size().x > room { elide(&p, name, name_font.clone(), room) } else { name.clone() };
                        // (emoji in colour at every weight, selected or not)
                        let g = crate::list::emoji_galley(&p, &shown, name_font, if kn_ui::theme::is_light() { egui::Color32::BLACK } else { look::text() });
                        p.galley(pos2(tx, cy - g.size().y / 2.0), g, look::text());
                    }
                    if app.show_counts {
                        p.text(pos2(r.max.x - 19.0, cy), Align2::RIGHT_CENTER, count.to_string(), look::regular(13.0), look::count_text());
                    }
                    if resp.clicked() {
                        go = Some(place.clone());
                    }
                    if resp.double_clicked() {
                        if let Some(f) = folder.as_ref().filter(|f| app.folder(f).is_some_and(|fo| fo.name != "Notes" && fo.name != "Quick Notes")) {
                            ctx_action = Some(format!("rename:{f}"));
                        }
                    }
                    crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                        if let Some(a) = folder_menu(app, ui, place, folder.as_deref()) {
                            ctx_action = Some(a);
                        }
                    });
                }
                Line::Tags(tags) => {
                    // the Tags browser: chips that wrap (All Tags first)
                    let x0 = 16.0;
                    let mut x = x0;
                    let mut y = 0.0;
                    let chip_h = 26.0;
                    let mut chips: Vec<(Rect, String, String)> = Vec::new();
                    let base = ui.cursor().min;
                    let p = ui.painter().clone();
                    for t in std::iter::once(String::new()).chain(tags.iter().cloned()) {
                        let label = if t.is_empty() { "All Tags".to_string() } else { format!("#{t}") };
                        // (a coloured tag: its dot before the name)
                        let dot = if t.is_empty() { 0.0 } else { crate::tagcolor::of(&t).map_or(0.0, |_| 14.0) };
                        let tw = p.layout_no_wrap(label.clone(), look::regular(12.0), look::text()).size().x + 20.0 + dot;
                        if x + tw > w - 12.0 && x > x0 {
                            x = x0;
                            y += chip_h + 6.0;
                        }
                        chips.push((Rect::from_min_size(base + vec2(x, y + 4.0), vec2(tw, chip_h)), t, label));
                        x += tw + 6.0;
                    }
                    let (_, _) = ui.allocate_exact_size(vec2(w, y + chip_h + 14.0), Sense::hover());
                    // the selection (the Mac's tag browser: several tags, some excluded, all or
                    // any of them); click picks one, Ctrl-click adds or drops one, the chip's menu
                    // includes, excludes, and switches all/any
                    let current = if let Place::Tag(spec) = &app.place { Some(crate::smart::TagSelection::parse(spec)) } else { None };
                    let ctrl = ui.input(|i| i.modifiers.command);
                    for (r, t, label) in chips {
                        let sel = current.clone().unwrap_or_default();
                        let (included, excluded) = (sel.included.iter().any(|x| x.eq_ignore_ascii_case(&t)), sel.excluded.iter().any(|x| x.eq_ignore_ascii_case(&t)));
                        let on = if t.is_empty() { current.as_ref().is_some_and(|s| s.is_empty()) } else { included };
                        let resp = ui.interact(r, ui.id().with(("tag", &t)), Sense::click());
                        let fill = if on { look::accent() } else if kn_ui::theme::is_light() { egui::Color32::from_black_alpha(if resp.hovered() { 24 } else { 14 }) } else { egui::Color32::from_white_alpha(if resp.hovered() { 30 } else { 18 }) };
                        p.rect_filled(r, r.height() / 2.0, fill);
                        let col = if on { look::on_accent() } else { look::text() };
                        match (!t.is_empty()).then(|| crate::tagcolor::of(&t)).flatten() {
                            Some(c) => {
                                crate::tagcolor::dot(&p, pos2(r.min.x + 15.0, r.center().y), 4.5, c);
                                p.text(pos2(r.center().x + 7.0, r.center().y), Align2::CENTER_CENTER, &label, look::regular(12.0), col);
                            }
                            None => {
                                p.text(r.center(), Align2::CENTER_CENTER, &label, look::regular(12.0), col);
                            }
                        }
                        if excluded {
                            p.rect_stroke(r, r.height() / 2.0, egui::Stroke::new(1.5, look::accent()), egui::StrokeKind::Inside);
                            p.hline(r.min.x + 9.0..=r.max.x - 9.0, r.center().y, egui::Stroke::new(1.2, col));
                        }
                        let with = |f: &dyn Fn(&mut crate::smart::TagSelection)| {
                            let mut s = sel.clone();
                            s.included.retain(|x| !x.eq_ignore_ascii_case(&t));
                            s.excluded.retain(|x| !x.eq_ignore_ascii_case(&t));
                            f(&mut s);
                            Place::Tag(s.spec())
                        };
                        if resp.clicked() {
                            go = Some(if t.is_empty() {
                                Place::Tag(String::new())
                            } else if ctrl {
                                with(&|s| {
                                    if !included {
                                        s.included.push(t.clone());
                                    }
                                })
                            } else if included && sel.included.len() == 1 && sel.excluded.is_empty() {
                                Place::Tag(String::new())
                            } else {
                                Place::Tag(t.clone())
                            });
                        }
                        if !t.is_empty() {
                            let mut picked: Option<Place> = None;
                            crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                                if ui.button(format!("Include #{t}")).clicked() {
                                    picked = Some(with(&|s| s.included.push(t.clone())));
                                    ui.close();
                                }
                                if ui.button(format!("Exclude #{t}")).clicked() {
                                    picked = Some(with(&|s| s.excluded.push(t.clone())));
                                    ui.close();
                                }
                                if ui.add_enabled(included || excluded, egui::Button::new(format!("Deselect #{t}"))).clicked() {
                                    picked = Some(with(&|_| {}));
                                    ui.close();
                                }
                                ui.separator();
                                for (any, l) in [(false, "Match All Selected Tags"), (true, "Match Any Selected Tag")] {
                                    if ui.add(egui::Button::new(l).selected(sel.any == any)).clicked() {
                                        let mut s = sel.clone();
                                        s.any = any;
                                        picked = Some(Place::Tag(s.spec()));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                // the tag's colour: swatches with their names
                                ui.menu_button("Tag Color", |ui| {
                                    let cur = crate::tagcolor::name_of(&t);
                                    if crate::tagcolor::swatch_button(ui, None, "None", cur.is_none()).clicked() {
                                        tag_color = Some(format!("tagcolor:{t}|"));
                                        ui.close();
                                    }
                                    for (name, _, _) in crate::tagcolor::PALETTE {
                                        if crate::tagcolor::swatch_button(ui, crate::tagcolor::named(name), name, cur.as_deref() == Some(*name)).clicked() {
                                            tag_color = Some(format!("tagcolor:{t}|{name}"));
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            if picked.is_some() {
                                go = picked;
                            }
                        }
                    }
                }
            }
        }
        ui.add_space(12.0);
        // a right-click below the rows: New Folder
        let rest = ui.available_rect_before_wrap();
        if rest.height() > 0.0 {
            let resp = ui.allocate_rect(rest, Sense::click());
            crate::menus::context_menu(&resp, resp.id.with("ctx"), |ui| {
                if ui.button("New Folder").clicked() {
                    ctx_action = Some("cmd:New Folder".into());
                    ui.close();
                }
            });
            if resp.clicked() {
                app.focus = Focus::Sidebar;
            }
        }
    });
    strip_material(ui, strip, &drawn, sc.offset > 0.5, focused);
    app.sidebar_scroll = sc;
    if let Some(k) = toggle {
        if !app.collapsed.remove(&k) {
            app.collapsed.insert(k);
        }
    }
    if let Some(pl) = go {
        app.search.clear();
        app.focus = Focus::Sidebar;
        app.go(pl);
    }
    if let Some(a) = tag_color.or(ctx_action) {
        let ctx = ui.ctx().clone();
        crate::menus::menu_action(app, &ctx, &a);
    }
}

fn elide(p: &egui::Painter, s: &str, font: egui::FontId, room: f32) -> String {
    let mut t: String = s.to_string();
    while t.chars().count() > 1 {
        t.pop();
        let cand = format!("{}...", t.trim_end());
        if p.layout_no_wrap(cand.clone(), font.clone(), egui::Color32::WHITE).size().x <= room {
            return cand;
        }
    }
    t
}

/// A folder's right-click menu (ctx-folder.txt): Rename Folder, Delete Folder, New Folder, Sort
/// By ▸, Group By Date ▸, Convert to Smart Folder. Returns the action picked.
fn folder_menu(app: &App, ui: &mut egui::Ui, place: &Place, folder: Option<&str>) -> Option<String> {
    let mut out = None;
    let system = folder.and_then(|f| app.folder(f)).is_none_or(|f| f.name == "Notes" && f.parent.is_none() || f.name == "Quick Notes");
    let items = crate::menus::folder_menu(app, place, folder, system);
    if let Some(a) = crate::menus::show_items(ui, &items) {
        out = Some(a);
    }
    out
}

/// The toolbar's strip over the sidebar: the suite's feathered look (Keydeck's and Letters'
/// feathered toolbar, as the user describes the Mac's sidebar). The rows run on, sharp, under the
/// strip; over them the strip's material (its grey and the rows blurred) is opaque at the top and
/// fades out along a smoothstep to nothing at the toolbar's foot, the blur thinning on the way,
/// so it goes from strongly blurred to sharp with no line. Each row's highlight, icon and name
/// are Gaussian blurs of their shapes (the sidebar has no picture to blur). The strip takes the
/// clicks, so nothing under the toolbar is clicked.
fn strip_material(ui: &egui::Ui, strip: Rect, drawn: &[(Rect, bool)], scrolled: bool, focused: bool) {
    if strip.height() <= 0.0 {
        return;
    }
    // (its own column, inside the window, never past them)
    let strip = strip.intersect(kn_ui::winframe::inner_rect(ui.ctx())).intersect(ui.clip_rect());
    let _ = ui.interact(strip, ui.id().with("sidebar-strip"), Sense::click());
    // (NT_STRIP_OFF=1: the rows under it bare, a reference for tests of the feather)
    if std::env::var_os("NT_SNAP").is_some() && std::env::var_os("NT_STRIP_OFF").is_some() {
        return;
    }
    let p = ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(strip);
    // how much of the material there is at a height: all of it down to 30 % of the strip, then a
    // smoothstep to none at its foot
    let mask = |y: f32| {
        let t = (((y - strip.min.y) / strip.height() - 0.3) / 0.7).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    };
    // (at rest nothing is under it: just the sidebar's grey, which is what's there anyway)
    let bg = look::sidebar_bg();
    let mut fill = egui::Mesh::default();
    let rows = 24;
    for k in 0..=rows {
        let y = strip.min.y + strip.height() * k as f32 / rows as f32;
        let a = if scrolled { mask(y) } else { 1.0 };
        fill.colored_vertex(pos2(strip.min.x, y), bg.gamma_multiply(a));
        fill.colored_vertex(pos2(strip.max.x, y), bg.gamma_multiply(a));
        if k > 0 {
            let i = 2 * (k - 1);
            fill.add_triangle(i, i + 1, i + 2);
            fill.add_triangle(i + 1, i + 3, i + 2);
        }
    }
    p.add(egui::Shape::mesh(fill));
    if !scrolled {
        return;
    }
    let text = look::text().gamma_multiply(0.30);
    for (row, selected) in drawn.iter().filter(|(r, _)| r.intersects(strip)) {
        // (the blur thins the lower the row is: strongest at the top, slight at the foot)
        let k = ((strip.max.y - row.center().y) / strip.height()).clamp(0.0, 1.0);
        let sigma = 2.0 + 10.0 * k;
        let blur = |r: Rect, col: Color32| egui::Shape::mesh(crate::list::gaussian_rect_masked(r, sigma, col, strip, &mask));
        if *selected {
            p.add(blur(*row, look::sidebar_selection(focused)));
        }
        let icon = Rect::from_center_size(pos2(row.min.x + 24.0, row.center().y), vec2(17.0, 14.0));
        p.add(blur(icon, look::folder_icon().gamma_multiply(0.8)));
        let name = Rect::from_min_max(pos2(icon.max.x + 7.0, row.center().y - 5.0), pos2((icon.max.x + 7.0 + row.width() * 0.45).min(row.max.x - 40.0), row.center().y + 5.0));
        p.add(blur(name, text));
    }
    crate::list::dither(ui.ctx(), &p, strip);
}

/// Whether folder `f` is inside folder `ancestor` (at any depth).
fn is_within(app: &App, f: &str, ancestor: &str) -> bool {
    let mut at = app.folder(f).and_then(|x| x.parent.clone());
    let mut hops = 0;
    while let Some(p) = at {
        if p == ancestor {
            return true;
        }
        hops += 1;
        if hops > 64 {
            return false;
        }
        at = app.folder(&p).and_then(|x| x.parent.clone());
    }
    false
}
