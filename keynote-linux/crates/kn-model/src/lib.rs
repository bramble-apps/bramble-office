//! Keynote document model: a typed, render-ready view over a [`kn_format::KeyFile`].
//!
//! The KeyFile stays the single source of truth (so saving is lossless); [`Document`] exposes
//! the slide list and builds [`types::SlideScene`]s on demand. Edits go through [`edit`].

pub mod boolops;
pub mod buildedit;
pub mod chart;
pub mod chartdata;
pub mod chartstyle;
pub mod comments;
pub mod edit;
pub mod group;
pub mod format;
pub mod formula;
mod functions_all;
pub mod imageedit;
pub mod insert;
pub mod links;
pub mod motionbg;
pub mod path;
pub mod points;
pub mod remotedata;
pub mod scene;
pub mod shapelib;
pub mod recording;
pub mod slides;
pub mod style;
pub mod table;
pub mod tablecells;
pub mod tablefill;
pub mod tablefmt;
pub mod tablelook;
pub mod tablecond;
pub mod tableformula;
pub mod tablemerge;
pub mod tablerows;
pub mod tablerich;
pub mod tablesort;
pub mod tabletiles;
pub mod text;
pub mod textedit;
pub mod textstyle;
pub mod types;

use std::path::{Path as FsPath, PathBuf};

use kn_format::{KeyFile, ObjId};

pub use types::*;

#[derive(Debug, Clone, PartialEq)]
pub struct SlideInfo {
    pub node: ObjId,
    pub slide: ObjId,
    pub skipped: bool,
    /// Indent depth in the navigator (slide groups).
    pub depth: u32,
    pub collapsed: bool,
}

pub struct Document {
    pub file: KeyFile,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// Saved as a package (a folder) rather than a single file.
    pub package: bool,
    /// File > Set Password: (password, hint).
    pub password: Option<(String, String)>,
    /// The theme file a new presentation was made from: one Keynote-made slide per layout,
    /// used as donors for Add Slide (it shares this document's style/layout ids).
    pub layout_donor: Option<std::sync::Arc<KeyFile>>,
}

impl Document {
    /// Open a password-protected presentation; it stays protected when saved.
    pub fn open_with_password(path: impl AsRef<FsPath>, password: &str, hint: &str) -> Result<Document, kn_format::Error> {
        let path = path.as_ref();
        let file = KeyFile::open_with_password(path, password)?;
        Ok(Document { file, path: Some(path.to_path_buf()), dirty: false, package: false, password: Some((password.to_string(), hint.to_string())), layout_donor: None })
    }

    pub fn open(path: impl AsRef<FsPath>) -> Result<Document, kn_format::Error> {
        let path = path.as_ref();
        Ok(Document { file: KeyFile::open(path)?, path: Some(path.to_path_buf()), dirty: false, package: path.is_dir(), password: None, layout_donor: None })
    }

    pub fn from_file(file: KeyFile) -> Document {
        Document { file, path: None, dirty: false, package: false, password: None, layout_donor: None }
    }

    pub fn save_as(&mut self, path: impl AsRef<FsPath>) -> Result<(), kn_format::Error> {
        self.declare_external_refs();
        // packages stay packages (File > Advanced > Change File Type); File > Set Password
        if let Some((pw, hint)) = &self.password {
            self.file.save_encrypted(path.as_ref(), pw, hint)?;
        } else if self.package {
            self.file.save_package(path.as_ref())?;
        } else {
            self.file.save(path.as_ref())?;
        }
        self.path = Some(path.as_ref().to_path_buf());
        self.dirty = false;
        Ok(())
    }

    /// Safety net before writing: every component declares the objects it references in other
    /// components. An undeclared one makes Keynote throw while laying out the slide (round 16).
    pub fn declare_external_refs(&mut self) {
        let files: Vec<String> = self.file.entries.iter().filter(|e| matches!(e.data, kn_format::EntryData::Iwa(_))).map(|e| e.name.clone()).collect();
        for f in files {
            self.file.ensure_external_refs(&f);
        }
    }

    pub fn show(&self) -> Option<(ObjId, &kn_format::Msg)> {
        self.file.first_of("KN.ShowArchive")
    }

    pub fn slide_size(&self) -> Size {
        self.show()
            .and_then(|(_, s)| s.msg("size"))
            .map(|s| Size { w: s.f32("width").unwrap_or(1920.0), h: s.f32("height").unwrap_or(1080.0) })
            .unwrap_or(Size { w: 1920.0, h: 1080.0 })
    }

    /// Slides in presentation order (depth-first through slide groups).
    pub fn slides(&self) -> Vec<SlideInfo> {
        let mut out = Vec::new();
        let Some((_, show)) = self.show() else { return out };
        let Some(tree) = show.msg("slideTree") else { return out };
        // Keynote keeps the slide order flat; indentation is each node's `depth` (1 = top level)
        // and collapsed groups are view state: UIStateArchive.collapsed_slide_nodes (verified on
        // the Mac). `children` is read too for older files.
        let collapsed: Vec<ObjId> = self.file.first_of("KN.UIStateArchive").map(|(_, u)| u.ref_ids("collapsed_slide_nodes")).unwrap_or_default();
        fn walk(k: &KeyFile, node: ObjId, extra: u32, collapsed: &[ObjId], out: &mut Vec<SlideInfo>) {
            let Some(n) = k.obj(node) else { return };
            if let Some(slide) = n.ref_id("slide") {
                out.push(SlideInfo {
                    node,
                    slide,
                    skipped: n.bool("isSkipped").unwrap_or(false),
                    depth: n.u32("depth").unwrap_or(1).saturating_sub(1) + extra,
                    collapsed: collapsed.contains(&node) || n.bool("isCollapsed").unwrap_or(false),
                });
            }
            for c in n.ref_ids("children") {
                walk(k, c, extra + 1, collapsed, out);
            }
        }
        for node in tree.ref_ids("slides") {
            walk(&self.file, node, 0, &collapsed, &mut out);
        }
        out
    }

    /// Slide number shown on a slide (skipped slides are not counted).
    pub fn slide_number(&self, slides: &[SlideInfo], index: usize) -> usize {
        slides[..=index.min(slides.len().saturating_sub(1))].iter().filter(|s| !s.skipped).count().max(1)
    }

    pub fn scene(&self, slides: &[SlideInfo], index: usize) -> Option<SlideScene> {
        let info = slides.get(index)?;
        scene::scene(&self.file, info.slide, self.slide_number(slides, index), self.slide_size())
    }

    pub fn notes(&self, slide: ObjId) -> String {
        self.notes_storage(slide).map(|st| text::plain_text(&self.file, st)).unwrap_or_default()
    }

    pub fn notes_storage(&self, slide: ObjId) -> Option<ObjId> {
        self.file.obj(slide)?.ref_id("note").and_then(|n| self.file.obj(n)).and_then(|n| n.ref_id("containedStorage"))
    }

    /// Layout (template slide) name, e.g. "Title & Bullets".
    pub fn layout_name(&self, slide: ObjId) -> Option<String> {
        let t = self.file.obj(slide)?.ref_id("template_slide")?;
        self.file.obj(t)?.str("name").map(str::to_string)
    }

    /// All layouts of the theme, in order: (template slide id, name).
    pub fn layouts(&self) -> Vec<(ObjId, String)> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        theme
            .ref_ids("templates")
            .into_iter()
            .filter_map(|n| {
                let node = self.file.obj(n)?;
                let slide = node.ref_id("slide").unwrap_or(n);
                let name = self.file.obj(slide)?.str("name").unwrap_or("").to_string();
                Some((slide, name))
            })
            .collect()
    }

    /// Layouts as navigator entries (for Edit Slide Layouts).
    pub fn layout_infos(&self) -> Vec<SlideInfo> {
        let Some((_, theme)) = self.file.first_of("KN.ThemeArchive") else { return Vec::new() };
        theme
            .ref_ids("templates")
            .into_iter()
            .filter_map(|n| {
                let slide = self.file.obj(n)?.ref_id("slide").unwrap_or(n);
                self.file.obj(slide)?;
                Some(SlideInfo { node: n, slide, skipped: false, depth: 0, collapsed: false })
            })
            .collect()
    }

    pub fn theme_name(&self) -> String {
        self.file
            .first_of("KN.ThemeArchive")
            .and_then(|(_, t)| t.find_super("TSS.ThemeArchive"))
            .and_then(|t| t.str("theme_identifier").map(str::to_string))
            .unwrap_or_default()
    }

    /// Bytes of a data file (image, movie, audio).
    pub fn data(&self, data_id: u64) -> Option<std::borrow::Cow<'_, [u8]>> {
        self.file.data(data_id).map(|(_, b)| b)
    }

    pub fn data_name(&self, data_id: u64) -> Option<&str> {
        self.file.data_entry(data_id).map(|e| e.name.as_str())
    }

    /// Scene of a layout (template slide), for the layout picker and Edit Slide Layouts.
    pub fn layout_scene(&self, template: ObjId) -> Option<SlideScene> {
        scene::scene(&self.file, template, 1, self.slide_size())
    }
}
