//! Insert menu / toolbar actions and selected-object keys: new objects go inline at the caret
//! in a word-processing document when they are tables or pictures typed into the text (Pages
//! puts a table inline on its own line), otherwise floating on the current page; arrow keys
//! nudge objects, Delete removes them, Return edits a text box's text.

use egui::{Event, Key};
use pg_core::kn_format::ObjId;
use pg_core::objects::{NewObject, Placement};

use crate::app::{App, Cursor, Owner};

/// Where a new floating object goes: the current page, in the middle of its text area (moved a
/// little for each object already there, so new objects don't hide each other).
pub(crate) fn floating_spot(app: &App, w: f32, h: f32) -> Placement {
    let page = app.current_page();
    let (cx, cy) = app.layout.as_ref().map(|l| (l.setup.width / 2.0, l.setup.height / 2.0)).unwrap_or((306.0, 396.0));
    let n = app.drawables_on(page).count() as f32;
    let off = (n % 6.0) * 18.0;
    Placement::Floating { page, x: (cx - w / 2.0 + off).max(10.0), y: (cy - h / 2.0 + off).max(10.0) }
}

/// The caret in the body (for inline objects), and a place for the object's own paragraph.
pub(crate) fn body_caret(app: &App) -> Option<usize> {
    match app.cursor {
        Some(Cursor { owner: Owner::Body, caret, .. }) if app.wb.as_ref().is_some_and(|w| w.is_word_processing()) => Some(caret),
        _ => None,
    }
}

/// Insert a new object and select it.
pub fn insert_object(app: &mut App, what: NewObject) {
    // tables (and charts) go inline at the caret in the body, on a line of their own
    let inline = matches!(what, NewObject::Table | NewObject::Chart) && body_caret(app).is_some();
    let place = if inline {
        let at = body_caret(app).unwrap();
        let st = app.cursor.unwrap().storage;
        let text = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
        // a paragraph of its own: break before (unless at a paragraph start) and after
        let needs_before = at > 0 && !text[..at].ends_with(|c: char| pg_core::body::is_para_end(c));
        let mut at2 = at;
        app.edit("Insert", |d, h| {
            if needs_before {
                pg_core::edit::replace(d, h, st, at, at, "\n");
                at2 = at + 1;
            }
            let t = d.storage_text(st);
            if !t[at2..].starts_with(|c: char| pg_core::body::is_para_end(c)) {
                pg_core::edit::replace(d, h, st, at2, at2, "\n");
            }
        });
        Placement::Inline { at: at2 }
    } else {
        let (w, h) = match what {
            NewObject::TextBox => (200.0, 60.0),
            NewObject::Line => (200.0, 0.0),
            NewObject::Chart => (370.0, 250.0),
            NewObject::Table => (300.0, 120.0),
            NewObject::Shape(_) => (150.0, 150.0),
        };
        floating_spot(app, w, h)
    };
    let label = match what {
        NewObject::TextBox => "Insert Text Box",
        NewObject::Shape(_) => "Insert Shape",
        NewObject::Line => "Insert Line",
        NewObject::Table => "Insert Table",
        NewObject::Chart => "Insert Chart",
    };
    let res = app.edit_pages(label, |p, h| p.insert(h, what, place));
    match res {
        Some(Ok(id)) => {
            app.refresh();
            crate::canvas::select_object(app, id);
            if what == NewObject::TextBox {
                crate::text::edit_drawable_text(app, id, None);
            }
            if let Some(p) = app.obj_page.get(&id).copied() {
                app.canvas.reveal_page(p);
            }
        }
        Some(Err(e)) => app.notify(format!("Couldn't insert: {e}")),
        None => {}
    }
}

/// Insert > Choose… (a picture): inline at the caret in a word-processing document's text
/// (Pages: Move with Text), floating on the page otherwise.
pub fn insert_image_bytes(app: &mut App, bytes: Vec<u8>, name: &str) {
    let max_w = app.layout.as_ref().map(|l| l.setup.text_width()).unwrap_or(468.0);
    let place = match body_caret(app) {
        Some(at) => Placement::Inline { at },
        None => {
            let (w, h) = pg_core::kn_format::objects::image_size(&bytes).map(|(w, h)| (w as f32, h as f32)).unwrap_or((400.0, 300.0));
            let s = (max_w.min(400.0) / w).min(1.0);
            floating_spot(app, w * s, h * s)
        }
    };
    let name = name.to_string();
    match app.edit_pages("Insert Image", |p, h| p.insert_image(h, bytes, &name, place, max_w)) {
        Some(Ok(id)) => {
            app.refresh();
            crate::canvas::select_object(app, id);
        }
        Some(Err(e)) => app.notify(format!("Couldn't insert the picture: {e}")),
        None => {}
    }
}

/// Objects dropped after a drag: moved by (dx, dy); onto another page when dropped there.
pub fn move_objects(app: &mut App, ids: &[ObjId], dx: f32, dy: f32, from: usize, to: usize) {
    let ph = app.layout.as_ref().map(|l| l.setup.height).unwrap_or(792.0);
    let two_up = app.canvas.two_up;
    app.edit_pages("Move", |p, h| {
        for id in ids {
            let floating = p.floating_page(*id).is_some();
            if floating && to != from && !two_up {
                // into the other page's coordinates
                let shift = (to as f32 - from as f32) * ph;
                p.detach_floating(h, &[*id]);
                p.attach_floating(h, *id, to);
                p.doc.move_by(h, *id, dx, dy - shift);
            } else if !floating {
                // an inline object dragged: it becomes floating where it was dropped (Pages
                // turns Move with Text objects into ones that stay where they are put)
                let g = p.file().obj(*id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(pg_core::kn_model::scene::geometry).unwrap_or_default();
                let _ = g;
                p.doc.move_by(h, *id, dx, dy);
            } else {
                p.doc.move_by(h, *id, dx, dy);
            }
        }
        p.update_connections(h, ids);
    });
}

/// Keys while objects are selected (not editing text).
pub fn object_keys(app: &mut App, ctx: &egui::Context, events: &[Event]) {
    let ids = app.selected_objs();
    if ids.is_empty() {
        return;
    }
    for e in events {
        match e {
            Event::Key { key, pressed: true, modifiers, .. } => {
                let step = if modifiers.shift { 10.0 } else { 1.0 };
                let (dx, dy) = match key {
                    Key::ArrowLeft => (-step, 0.0),
                    Key::ArrowRight => (step, 0.0),
                    Key::ArrowUp => (0.0, -step),
                    Key::ArrowDown => (0.0, step),
                    Key::Delete | Key::Backspace => {
                        delete_selected(app);
                        return;
                    }
                    Key::Enter => {
                        if let Some(id) = app.canvas.obj {
                            if app.drawables.iter().find(|d| d.id == id).and_then(crate::text::drawable_text).is_some() {
                                crate::text::edit_drawable_text(app, id, None);
                            }
                        }
                        return;
                    }
                    Key::Escape => {
                        crate::canvas::deselect_objects(app);
                        return;
                    }
                    _ => continue,
                };
                let locked: Vec<ObjId> = ids.iter().copied().filter(|i| app.drawables.iter().any(|d| d.id == *i && d.locked)).collect();
                let movable: Vec<ObjId> = ids.iter().copied().filter(|i| !locked.contains(i)).collect();
                if let Some(w) = app.wb.as_mut() {
                    w.doc.begin_merge(&mut app.hist, "Nudge");
                    for id in &movable {
                        w.doc.move_by(&mut app.hist, *id, dx, dy);
                    }
                    w.update_connections(&mut app.hist, &movable);
                    w.doc.end(&mut app.hist);
                    app.changed();
                }
            }
            Event::Copy => copy_objects(app, ctx),
            Event::Cut => {
                copy_objects(app, ctx);
                delete_selected(app);
                return;
            }
            Event::Paste(_) => paste_objects(app, 20.0),
            Event::Text(t) if !t.is_empty() => {
                // typing on a selected text box starts editing it (Pages)
                if let Some(id) = app.canvas.obj {
                    if app.drawables.iter().find(|d| d.id == id).and_then(crate::text::drawable_text).is_some() {
                        crate::text::edit_drawable_text(app, id, None);
                        crate::text::type_text(app, t);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn delete_selected(app: &mut App) {
    let ids = app.selected_objs();
    if ids.is_empty() {
        return;
    }
    app.edit_pages("Delete", |p, h| p.delete_objects(h, &ids));
    crate::canvas::deselect_objects(app);
}

thread_local! {
    /// Pastes of the copied objects so far: each one lands a step further on.
    static PASTES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn copy_objects(app: &mut App, ctx: &egui::Context) {
    PASTES.with(|p| p.set(0));
    let ids: Vec<ObjId> = app.selected_objs().into_iter().filter(|i| app.wb.as_ref().is_some_and(|w| w.file().type_name(*i) != "TST.TableInfoArchive")).collect();
    if let Some(d) = app.doc() {
        app.obj_clip = d.copy_objects(&ids);
        app.obj_clip_active = app.obj_clip.is_some();
        ctx.copy_text(String::new());
    }
}

/// Paste copied objects on the current page (moved by `offset`).
pub fn paste_objects(app: &mut App, offset: f32) {
    let Some(clip) = app.obj_clip.clone() else { return };
    let page = app.current_page();
    let n = PASTES.with(|p| {
        p.set(p.get() + 1);
        p.get()
    });
    let offset = offset * n as f32;
    let Some(root) = app.root_id() else { return };
    let res = app.edit_pages("Paste", |p, h| -> Result<Vec<ObjId>, pg_core::kn_format::Error> {
        let ids = p.doc.paste_objects(h, root, &clip, offset)?;
        for id in &ids {
            p.attach_floating(h, *id, page);
            p.doc.touch(h, *id);
            if let Some(d) = p.doc.file.obj_mut(*id).and_then(|m| m.find_super_mut("TSD.DrawableArchive")) {
                d.remove("parent");
            }
        }
        Ok(ids)
    });
    match res {
        Some(Ok(ids)) => {
            app.refresh();
            app.canvas.obj = ids.first().copied();
            app.canvas.objs = ids;
        }
        Some(Err(e)) => app.notify(format!("Couldn't paste: {e}")),
        None => {}
    }
}

/// Edit > Duplicate Selection.
pub fn duplicate(app: &mut App, ctx: &egui::Context) {
    copy_objects(app, ctx);
    paste_objects(app, 20.0);
}
