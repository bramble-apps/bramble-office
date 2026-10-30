//! Format > Shapes and Lines > Save to My Shapes: a shape kept for later in
//! ~/.config/numdeck/myshapes (each a small Numbers document holding just it), offered in the
//! toolbar's Shape popover under My Shapes, as Numbers keeps its own.

use std::path::PathBuf;

use nm_core::kn_format::{KeyFile, ObjId};
use nm_core::kn_model::edit::History;
use nm_core::kn_model::Document;

use crate::app::App;

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("numdeck").join("myshapes"))
}

/// The saved shapes: (name, file), oldest first.
pub fn list() -> Vec<(String, PathBuf)> {
    let Some(d) = dir() else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "numbers"))
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), p))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Keep the selected shape.
pub fn save(app: &mut App) {
    let Some(id) = app.canvas.obj else { return app.notify("Select a shape first") };
    let Some(src) = app.doc().map(|d| d.file.clone()) else { return };
    let Some(d) = dir() else { return };
    let res = (|| -> Result<PathBuf, nm_core::kn_format::Error> {
        let (blank, table) = nm_core::sheets::blank_donor()?;
        let mut doc = Document::from_file(blank);
        let mut h = History::default();
        let sheet = nm_core::sheets::sheet_ids(&doc).first().copied().ok_or(nm_core::kn_format::Error::Invalid("no sheet".into()))?;
        doc.begin(&mut h, "My Shape");
        nm_core::sheets::insert_object(&mut doc, &mut h, sheet, &src, id, 0.0, 0.0)?;
        // (the blank document's table isn't part of the shape)
        nm_core::sheets::detach_drawables(&mut doc, &mut h, sheet, &[table]);
        doc.end(&mut h);
        let _ = std::fs::create_dir_all(&d);
        let n = (1..).find(|k| !d.join(format!("Shape {k:02}.numbers")).exists()).unwrap_or(1);
        let p = d.join(format!("Shape {n:02}.numbers"));
        std::fs::write(&p, doc.file.to_zip()?)?;
        Ok(p)
    })();
    match res {
        Ok(p) => app.notify(format!("Saved to My Shapes as \u{201c}{}\u{201d}", p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default())),
        Err(e) => app.notify(format!("Couldn't save the shape: {e}")),
    }
}

/// The shape kept in `file` (its document and the shape's id there).
pub fn donor(file: &PathBuf) -> Option<(KeyFile, ObjId)> {
    let k = KeyFile::from_zip(&std::fs::read(file).ok()?).ok()?;
    let doc = Document::from_file(k);
    let sheet = *nm_core::sheets::sheet_ids(&doc).first()?;
    let id = doc.file.obj(sheet)?.ref_ids("drawable_infos").into_iter().find(|d| doc.file.type_name(*d) != "TST.TableInfoArchive")?;
    Some((doc.file, id))
}

/// Insert a kept shape where there's room.
pub fn insert(app: &mut App, file: &PathBuf) {
    let Some((k, id)) = donor(file) else { return app.notify("That shape couldn't be read") };
    crate::canvas::commit(app);
    let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
    let (x, y) = app.free_spot_pub();
    match app.edit("Insert Shape", |d, h| nm_core::sheets::insert_object(d, h, sheet, &k, id, x, y)) {
        Some(Ok(new)) => {
            app.sel = None;
            app.canvas.obj = Some(new);
        }
        Some(Err(e)) => app.notify(format!("Couldn't insert the shape: {e}")),
        None => {}
    }
}

/// Delete a kept shape.
pub fn remove(file: &PathBuf) {
    let _ = std::fs::remove_file(file);
}
