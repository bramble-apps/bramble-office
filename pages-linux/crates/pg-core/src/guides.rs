//! Alignment guides dragged from the rulers (View > Guides). Pages keeps a page's guides in its
//! section (TP.SectionArchive.user_defined_guide_storage → TP.UserDefinedGuideMapArchive:
//! page index within the section → TSD.GuideStorageArchive) and a page template's own guides
//! on the template (TP.PageTemplateArchive.guide_storage).

use kn_format::{Msg, ObjId};
use kn_model::edit::History;
pub use kn_model::slides::Guide;

use crate::flow::Layout;
use crate::Pages;

fn read(m: &Msg) -> Vec<Guide> {
    m.msgs("userDefinedGuides").map(|g| Guide { vertical: g.enum_name("type") == Some("Vertical"), pos: g.f32("position").unwrap_or(0.0) }).collect()
}

fn write(m: &mut Msg, guides: &[Guide]) {
    m.remove("userDefinedGuides");
    for g in guides {
        let u = m.push_new("userDefinedGuides");
        u.set_enum("type", if g.vertical { "Vertical" } else { "Horizontal" });
        u.set_f32("position", g.pos);
    }
}

impl Pages {
    /// Page `pi`'s section object and its index within that section.
    fn guide_slot(&self, l: &Layout, pi: usize) -> Option<(ObjId, u64)> {
        let page = l.pages.get(pi)?;
        let in_section = l.pages[..pi].iter().rev().take_while(|p| p.section == page.section).count();
        Some((self.sections().get(page.section)?.id, in_section as u64))
    }

    fn guide_storage(&self, section: ObjId, index: u64) -> Option<ObjId> {
        let k = self.file();
        let map = k.obj(section)?.ref_id("user_defined_guide_storage").and_then(|m| k.obj(m))?;
        map.msgs("user_defined_guide_storages").find(|e| e.u64("page_index") == Some(index))?.ref_id("guide_storage")
    }

    /// The guides on page `pi`.
    pub fn page_guides(&self, l: &Layout, pi: usize) -> Vec<Guide> {
        self.guide_slot(l, pi).and_then(|(s, i)| self.guide_storage(s, i)).and_then(|g| self.file().obj(g)).map(read).unwrap_or_default()
    }

    /// The guides of page `pi`'s page template (page layout documents).
    pub fn template_guides(&self, l: &Layout, pi: usize) -> Vec<Guide> {
        let k = self.file();
        self.page_template_of(l, pi)
            .and_then(|t| k.obj(t))
            .filter(|t| t.type_name() == "TP.PageTemplateArchive")
            .and_then(|t| t.ref_id("guide_storage"))
            .and_then(|g| k.obj(g))
            .map(read)
            .unwrap_or_default()
    }

    /// Replace page `pi`'s guides (creating its section's guide map and storage as needed).
    pub fn set_page_guides(&mut self, h: &mut History, l: &Layout, pi: usize, guides: &[Guide]) -> bool {
        let Some((section, index)) = self.guide_slot(l, pi) else { return false };
        if let Some(g) = self.guide_storage(section, index) {
            self.doc.touch(h, g);
            if let Some(m) = self.doc.file.obj_mut(g) {
                write(m, guides);
                return true;
            }
        }
        if guides.is_empty() {
            return true;
        }
        let mut gm = Msg::new("TSD.GuideStorageArchive");
        write(&mut gm, guides);
        let Some(g) = self.new_object(h, section, gm) else { return false };
        let map = match self.file().obj(section).and_then(|s| s.ref_id("user_defined_guide_storage")) {
            Some(m) => m,
            None => {
                let Some(m) = self.new_object(h, section, Msg::new("TP.UserDefinedGuideMapArchive")) else { return false };
                self.doc.touch(h, section);
                if let Some(a) = self.doc.file.archive_mut(section) {
                    a.obj_mut().set_ref("user_defined_guide_storage", m);
                    a.refresh_object_references();
                }
                m
            }
        };
        self.doc.touch(h, map);
        let Some(a) = self.doc.file.archive_mut(map) else { return false };
        let e = a.obj_mut().push_new("user_defined_guide_storages");
        e.set_u64("page_index", index);
        e.set_ref("guide_storage", g);
        a.refresh_object_references();
        true
    }
}
