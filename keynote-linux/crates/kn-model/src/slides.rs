//! Slide-level operations built on the structural edits in kn-format.

use kn_format::ObjId;

use crate::edit::History;
use crate::Document;

impl Document {
    /// Keynote's New Slide: a slide with the same layout as slide `index`, inserted after it,
    /// with only its placeholders (emptied) and no transition or builds.
    pub fn new_slide_after(&mut self, h: &mut History, index: usize) -> Result<usize, kn_format::Error> {
        self.touch_all(h);
        let (_, new_slide) = self.file.clone_slide(index, index + 1)?;
        let placeholders: Vec<ObjId> = {
            let s = self.file.obj(new_slide).ok_or(kn_format::Error::Invalid("slide".into()))?;
            ["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder", "objectPlaceholder"]
                .iter()
                .filter_map(|f| s.ref_id(f))
                .collect()
        };
        // empty the placeholder texts
        let storages: Vec<ObjId> = placeholders
            .iter()
            .filter_map(|p| self.file.obj(*p))
            .filter(|m| m.enum_name("kind") != Some("kKindSlideNumberPlaceholder"))
            .filter_map(|m| m.find_super("TSWP.ShapeInfoArchive").and_then(|i| i.ref_id("owned_storage")))
            .collect();
        for st in storages {
            let len = self.storage_text(st).len();
            if len > 0 {
                self.replace_text(h, st, 0, len, "");
            }
        }
        // keep placeholders only; drop builds, transition and notes text
        if let Some(a) = self.file.archive_mut(new_slide) {
            let s = &mut a.payloads[0];
            s.retain_msgs("drawables_z_order", |r| placeholders.contains(&r.u64("identifier").unwrap_or(0)));
            s.retain_msgs("owned_drawables", |r| placeholders.contains(&r.u64("identifier").unwrap_or(0)));
            s.remove("builds");
            s.remove("buildChunks");
            if let Some(aa) = s.msg_path_mut("transition.attributes.animationAttributes") {
                aa.set_str("effect", "none");
            }
            a.refresh_object_references();
        }
        if let Some(st) = self.notes_storage(new_slide) {
            let len = self.storage_text(st).len();
            if len > 0 {
                self.replace_text(h, st, 0, len, "");
            }
        }
        let node = self.slides().into_iter().find(|s| s.slide == new_slide).map(|s| s.node);
        if let Some(n) = node.and_then(|n| self.file.obj_mut(n)) {
            for f in ["hasBuilds", "hasTransition", "hasNote", "isSkipped", "has_explicit_builds"] {
                if n.has(f) {
                    n.set_bool(f, false);
                }
            }
            if n.has("build_event_count") {
                n.set_u64("build_event_count", 0);
            }
        }
        Ok(index + 1)
    }
}

impl Document {
    /// Keynote's Add Slide with a chosen layout, inserted at top-level position `at`.
    ///
    /// Made from the theme file the presentation was created from (its Keynote-made slide for
    /// the layout), else built from the layout's placeholders (`slide_for_layout`). Both match a
    /// fresh Keynote slide (verified on the Mac, round 7); copying a deck slide that uses the
    /// layout does not (it keeps that slide's hidden placeholders). Converting the layout's own
    /// template slide is not accepted by Keynote at all.
    pub fn new_slide_from_layout(&mut self, h: &mut History, layout: ObjId, at: usize) -> Result<usize, kn_format::Error> {
        let new_slide = if let Some((donor, dnode, dslide)) = self.layout_donor.clone().and_then(|d| {
            let found = d.slide_nodes().into_iter().find(|(_, s)| d.obj(*s).and_then(|m| m.ref_id("template_slide")) == Some(layout));
            found.map(|(n, s)| (d, n, s))
        }) {
            self.touch_all(h);
            self.file.clone_slide_between(Some(&donor), dslide, dnode, at)?.1
        } else {
            self.touch_all(h);
            self.slide_for_layout(layout, at)?
        };
        self.empty_new_slide(h, new_slide);
        Ok(at)
    }

    /// Build a slide for a layout no slide uses yet, the way Keynote does: a real slide of this
    /// document (the one with the fewest objects) is copied and cleared, then it gets the layout's
    /// style and copies of the layout's placeholders and media placeholders (not the object
    /// placeholder), as in Keynote's own slides (White theme, one slide per layout).
    fn slide_for_layout(&mut self, layout: ObjId, at: usize) -> Result<ObjId, kn_format::Error> {
        const PH: [&str; 3] = ["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder"];
        let lm = self.file.obj(layout).ok_or(kn_format::Error::Invalid("no such layout".into()))?.clone();
        let weight = |s: ObjId| {
            let m = self.file.obj(s);
            let n = |f: &str| m.map(|m| m.ref_ids(f).len()).unwrap_or(99);
            let images = m.map(|m| m.ref_ids("owned_drawables")).unwrap_or_default().iter().filter(|d| self.file.type_name(**d) != "KN.PlaceholderArchive").count();
            n("owned_drawables") + n("builds") + 10 * images
        };
        let nodes = self.file.slide_nodes();
        let skeleton = (0..nodes.len()).min_by_key(|i| weight(nodes[*i].1)).ok_or(kn_format::Error::Invalid("no slide to start from".into()))?;
        let new = self.file.clone_slide(skeleton, at)?.1;

        // clear the copy: its objects, builds and placeholders go (the note stays and is emptied later)
        let m = self.file.obj(new).unwrap();
        let mut roots: Vec<ObjId> = ["drawables_z_order", "owned_drawables", "builds", "buildChunks"].iter().flat_map(|f| m.ref_ids(f)).collect();
        roots.extend(PH.iter().filter_map(|f| m.ref_id(f)));
        let note = m.ref_id("note");
        let mut dead = std::collections::HashSet::new();
        for r in roots {
            if let Ok(ex) = self.file.extract(r) {
                dead.extend(ex.archives.iter().map(|a| a.id()));
            }
        }
        if let Some(Ok(ex)) = note.map(|n| self.file.extract(n)) {
            for a in &ex.archives {
                dead.remove(&a.id());
            }
        }
        dead.remove(&new);
        for id in dead {
            self.file.remove_archive(id);
        }
        if let Some(a) = self.file.archive_mut(new) {
            let s = a.obj_mut();
            for f in ["drawables_z_order", "owned_drawables", "builds", "buildChunks", "sage_tag_to_info_map", "objectPlaceholder"].iter().chain(PH.iter()) {
                s.remove(f);
            }
            s.set_ref("template_slide", layout);
            if let Some(st) = lm.ref_id("style") {
                s.set_ref("style", st);
            }
            a.refresh_object_references();
        }

        // copies of the layout's placeholders and media placeholders
        let mut map = std::collections::HashMap::new();
        for id in lm.ref_ids("owned_drawables") {
            let ex = self.file.extract(id)?;
            map.insert(id, self.file.paste_extract(&ex, new, None)?);
        }
        for f in PH {
            let Some(id) = lm.ref_id(f) else { continue };
            if !map.contains_key(&id) {
                // hidden placeholders exist on the slide but are not drawn
                let ex = self.file.extract(id)?;
                let n = self.file.paste_extract(&ex, new, None)?;
                if let Some(a) = self.file.archive_mut(new) {
                    let s = a.obj_mut();
                    s.retain_msgs("drawables_z_order", |r| r.u64("identifier") != Some(n));
                    s.retain_msgs("owned_drawables", |r| r.u64("identifier") != Some(n));
                    a.refresh_object_references();
                }
                map.insert(id, n);
            }
        }
        for n in map.values() {
            if self.file.type_name(*n) == "TSD.ImageArchive" {
                if let Some(m) = self.file.obj_mut(*n) {
                    if let Some(fl) = m.u32("flags") {
                        m.set_u64("flags", (fl & !2) as u64); // 3 on the layout, 1 on Keynote's slides
                    }
                }
            }
        }
        let tags: Vec<(String, ObjId)> = lm.msgs("sage_tag_to_info_map").filter_map(|e| Some((e.str("tag")?.to_string(), *map.get(&e.ref_id("info")?)?))).collect();
        if let Some(a) = self.file.archive_mut(new) {
            let s = a.obj_mut();
            for f in PH {
                if let Some(n) = lm.ref_id(f).and_then(|id| map.get(&id)) {
                    s.set_ref(f, *n);
                }
            }
            for (tag, info) in tags {
                let mut e = kn_format::Msg::new("KN.SlideArchive.SageTagMapEntry");
                e.set_str("tag", &tag);
                e.set_ref("info", info);
                s.push_msg("sage_tag_to_info_map", e);
            }
            a.refresh_object_references();
        }
        Ok(new)
    }

    /// For a deck made from a theme in `dir` (all its layouts have the same ids and names there),
    /// keep that theme file as the donor of real slides for Add Slide. Returns whether one matched.
    pub fn find_layout_donor(&mut self, dir: &std::path::Path) -> bool {
        let mine = self.layouts();
        if mine.is_empty() {
            return false;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return false };
        for e in rd.flatten() {
            let p = e.path();
            let small = e.metadata().map(|m| m.len() < 64 << 20).unwrap_or(false);
            if !small || p.extension().and_then(|x| x.to_str()) != Some("key") {
                continue;
            }
            let Ok(t) = Document::open(&p) else { continue };
            let theirs = t.layouts();
            if mine.iter().all(|l| theirs.contains(l)) {
                self.layout_donor = Some(std::sync::Arc::new(t.file));
                return true;
            }
        }
        false
    }

    /// Document > Slideshow Settings / Presentation Type (KN.ShowArchive fields).
    pub fn set_show_setting(&mut self, h: &mut History, setting: ShowSetting) -> bool {
        let Some((id, _)) = self.file.first_of("KN.ShowArchive") else { return false };
        self.touch(h, id);
        let Some(m) = self.file.obj_mut(id) else { return false };
        match setting {
            ShowSetting::AutoPlayOnOpen(b) => m.set_bool("automatically_plays_upon_open", b),
            ShowSetting::Loop(b) => m.set_bool("loop_presentation", b),
            ShowSetting::RestartIfIdle(b) => m.set_bool("idle_timer_active", b),
            ShowSetting::IdleMinutes(v) => m.set_f64("idle_timer_delay", (v * 60.0).clamp(60.0, 24.0 * 3600.0)),
            ShowSetting::Mode(mode) => m.set_enum(
                "mode",
                match mode {
                    ShowMode::Normal => "kKNShowModeNormal",
                    ShowMode::SelfPlaying => "kKNShowModeAutoPlay",
                    ShowMode::LinksOnly => "kKNShowModeHyperlinksOnly",
                },
            ),
            ShowSetting::TransitionDelay(v) => m.set_f64("autoplay_transition_delay", v.clamp(0.0, 3600.0)),
            ShowSetting::BuildDelay(v) => m.set_f64("autoplay_build_delay", v.clamp(0.0, 3600.0)),
        }
        true
    }

    /// Document > Slide Size. Objects keep their size; slides' and layouts' objects are moved
    /// so they stay centred (the slide grows or shrinks around them).
    pub fn set_slide_size(&mut self, h: &mut History, w: f32, ht: f32) -> bool {
        let (w, ht) = (w.clamp(16.0, 10000.0).round(), ht.clamp(16.0, 10000.0).round());
        let old = self.slide_size();
        let Some((show, _)) = self.file.first_of("KN.ShowArchive") else { return false };
        if (old.w - w).abs() < 0.5 && (old.h - ht).abs() < 0.5 {
            return false;
        }
        self.touch(h, show);
        if let Some(m) = self.file.obj_mut(show) {
            let sz = m.msg_mut_or_create("size");
            sz.set_f32("width", w);
            sz.set_f32("height", ht);
        }
        // Keynote (verified): objects of slides and layouts scale by s = min(w ratio, h ratio)
        // and are centred; text sizes scale with the height ratio.
        let k = (w / old.w).min(ht / old.h);
        let (ox, oy) = ((w - old.w * k) / 2.0, (ht - old.h * k) / 2.0);
        let mut ids = Vec::new();
        // slides and layouts (layouts are slide archives too)
        for (_, sm) in self.file.objects_of("KN.SlideArchive") {
            ids.extend(sm.ref_ids("drawables_z_order"));
            for f in ["titlePlaceholder", "bodyPlaceholder", "objectPlaceholder", "slideNumberPlaceholder"] {
                ids.extend(sm.ref_ids(f));
            }
        }
        ids.sort();
        ids.dedup();
        // image masks are in their image's frame: scaled, not moved (verified)
        let masks: Vec<ObjId> = ids.iter().filter_map(|i| self.file.obj(*i).and_then(|m| m.ref_id("mask"))).collect();
        for id in ids {
            let Some(g) = self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(crate::scene::geometry) else { continue };
            let g2 = crate::Geometry { x: g.x * k + ox, y: g.y * k + oy, w: g.w * k, h: g.h * k, ..g };
            self.set_frame(h, id, g2);
        }
        for id in masks {
            let Some(g) = self.file.obj(id).and_then(|m| m.find_super("TSD.DrawableArchive")).map(crate::scene::geometry) else { continue };
            let g2 = crate::Geometry { x: g.x * k, y: g.y * k, w: g.w * k, h: g.h * k, ..g };
            self.set_frame(h, id, g2);
        }
        let fk = ht / old.h;
        if (fk - 1.0).abs() > 1e-4 {
            fn has_font(m: &kn_format::Msg) -> bool {
                (m.type_name() == "TSWP.CharacterStylePropertiesArchive" && m.has("font_size"))
                    || m.fields.iter().any(|f| matches!(&f.val, kn_format::Val::Msg(sub) if has_font(sub)))
            }
            fn scale(m: &mut kn_format::Msg, fk: f32) {
                if m.type_name() == "TSWP.CharacterStylePropertiesArchive" {
                    if let Some(v) = m.f32("font_size") {
                        // Keynote's rounding (verified on 116 sizes): truncate, and from 16 up an
                        // odd result goes down to the even size below
                        let mut n = (v * fk).floor().max(1.0);
                        if n >= 16.0 && (n as i64) % 2 == 1 {
                            n -= 1.0;
                        }
                        m.set_f32("font_size", n);
                    }
                }
                for f in m.fields.iter_mut() {
                    if let kn_format::Val::Msg(sub) = &mut f.val {
                        scale(sub, fk);
                    }
                }
            }
            // presenter notes and comment styles keep their size (Keynote leaves them alone)
            let keep = |a: &kn_format::Archive| {
                a.payloads.first().and_then(|p| p.find_super("TSS.StyleArchive")).and_then(|s| s.str("style_identifier")).is_some_and(|i| i.starts_with("note-") || i.starts_with("stickyComment-"))
            };
            let with_fonts: Vec<ObjId> = self.file.archives().filter(|(_, a)| !keep(a) && a.payloads.iter().any(has_font)).map(|(_, a)| a.id()).collect();
            for id in with_fonts {
                self.touch(h, id);
                if let Some(m) = self.file.obj_mut(id) {
                    scale(m, fk);
                }
            }
        }
        true
    }

    /// The presentation's slideshow settings.
    pub fn show_settings(&self) -> ShowSettings {
        let m = self.file.first_of("KN.ShowArchive").map(|(_, m)| m);
        let b = |f: &str| m.and_then(|m| m.bool(f)).unwrap_or(false);
        let n = |f: &str, d: f64| m.and_then(|m| m.f64(f)).unwrap_or(d);
        ShowSettings {
            auto_play_on_open: b("automatically_plays_upon_open"),
            looping: b("loop_presentation"),
            restart_if_idle: b("idle_timer_active"),
            idle_minutes: n("idle_timer_delay", 900.0) / 60.0,
            mode: match m.and_then(|m| m.enum_name("mode")) {
                Some("kKNShowModeAutoPlay") => ShowMode::SelfPlaying,
                Some("kKNShowModeHyperlinksOnly") => ShowMode::LinksOnly,
                _ => ShowMode::Normal,
            },
            transition_delay: n("autoplay_transition_delay", 5.0),
            build_delay: n("autoplay_build_delay", 2.0),
        }
    }

    /// Format > Slide > Appearance: show or hide the slide's title or body placeholder. Keynote
    /// keeps hidden placeholders on the slide but out of its drawables (as on its own slides).
    pub fn set_placeholder_visible(&mut self, h: &mut History, slide: ObjId, field: &str, visible: bool) -> bool {
        let Some(ph) = self.file.obj(slide).and_then(|m| m.ref_id(field)) else { return false };
        self.touch(h, slide);
        let Some(a) = self.file.archive_mut(slide) else { return false };
        let s = a.obj_mut();
        let listed = s.ref_ids("drawables_z_order").contains(&ph);
        if visible && !listed {
            s.push_ref("owned_drawables", ph);
            s.push_ref("drawables_z_order", ph);
        } else if !visible && listed {
            s.retain_msgs("drawables_z_order", |r| r.u64("identifier") != Some(ph));
            s.retain_msgs("owned_drawables", |r| r.u64("identifier") != Some(ph));
        }
        a.refresh_object_references();
        true
    }

    /// Change a slide's layout (the Slide Layout pop-up in Format > Slide): the slide takes the
    /// layout's background and its placeholders' frames; placeholders the layout doesn't have
    /// are hidden.
    pub fn change_slide_layout(&mut self, h: &mut History, slide: ObjId, layout: ObjId) -> bool {
        let old_layout = self.file.obj(slide).and_then(|m| m.ref_id("template_slide"));
        if old_layout == Some(layout) {
            return false;
        }
        let Some(layout_style) = self.file.obj(layout).and_then(|m| m.ref_id("style")) else { return false };
        self.touch(h, slide);
        if let Some(m) = self.file.obj_mut(slide) {
            m.set_ref("template_slide", layout);
            m.set_ref("style", layout_style);
        }
        self.file.refresh_refs(slide);
        if let Some(f) = self.file.file_of(slide).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        // the slide gets the new layout's placeholders, as Keynote does: title / body / slide
        // number shown or hidden as the layout has them (added when missing, text kept when
        // present), and the old layout's empty photo placeholders swapped for the new layout's
        const PH: [&str; 3] = ["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder"];
        let Some(lm) = self.file.obj(layout).cloned() else { return true };
        let image_data = |k: &kn_format::KeyFile, id: ObjId| k.obj(id).and_then(|m| m.msg("data")).and_then(|d| d.u64("identifier"));
        let old_media: std::collections::HashSet<u64> = old_layout
            .and_then(|l| self.file.obj(l))
            .map(|m| m.ref_ids("owned_drawables"))
            .unwrap_or_default()
            .into_iter()
            .filter(|d| self.file.type_name(*d) == "TSD.ImageArchive")
            .filter_map(|d| image_data(&self.file, d))
            .collect();
        let had: Vec<(&str, bool)> = PH.iter().map(|f| (*f, self.file.obj(slide).and_then(|m| m.ref_id(f)).is_some())).collect();
        let shown_in_layout = |f: &str| lm.ref_id(f).is_some_and(|id| lm.ref_ids("owned_drawables").contains(&id));
        let tag_of = |id: ObjId| lm.msgs("sage_tag_to_info_map").find(|e| e.ref_id("info") == Some(id)).and_then(|e| e.str("tag")).map(str::to_string);
        let _ = self.structural(h, &[slide], |k| -> Result<(), kn_format::Error> {
            // empty photo placeholders of the old layout go
            let tagged: Vec<ObjId> = k.obj(slide).map(|m| m.msgs("sage_tag_to_info_map").filter_map(|e| e.ref_id("info")).collect()).unwrap_or_default();
            let empty: Vec<ObjId> = tagged
                .into_iter()
                .filter(|d| k.type_name(*d) == "TSD.ImageArchive" && image_data(k, *d).is_some_and(|x| old_media.contains(&x)))
                .collect();
            if let Some(a) = k.archive_mut(slide) {
                let s = a.obj_mut();
                for f in ["drawables_z_order", "owned_drawables"] {
                    s.retain_msgs(f, |r| !empty.contains(&r.u64("identifier").unwrap_or(0)));
                }
                s.retain_msgs("sage_tag_to_info_map", |e| !e.ref_id("info").is_some_and(|i| empty.contains(&i)));
                a.refresh_object_references();
            }
            // the new layout's photo placeholders
            let mut added: Vec<(ObjId, ObjId)> = Vec::new();
            let photos: Vec<ObjId> = lm.ref_ids("owned_drawables").into_iter().filter(|d| k.type_name(*d) == "TSD.ImageArchive").collect();
            for id in photos {
                let ex = k.extract(id)?;
                let n = k.paste_extract(&ex, slide, None)?;
                if let Some(m) = k.obj_mut(n) {
                    if let Some(fl) = m.u32("flags") {
                        m.set_u64("flags", (fl & !2) as u64);
                    }
                }
                added.push((id, n));
            }
            // title / body / slide number the slide doesn't have yet
            for (f, present) in &had {
                let Some(theirs) = lm.ref_id(f) else { continue };
                if *present {
                    continue;
                }
                let ex = k.extract(theirs)?;
                let n = k.paste_extract(&ex, slide, None)?;
                if !shown_in_layout(f) {
                    if let Some(a) = k.archive_mut(slide) {
                        let s = a.obj_mut();
                        s.retain_msgs("drawables_z_order", |r| r.u64("identifier") != Some(n));
                        s.retain_msgs("owned_drawables", |r| r.u64("identifier") != Some(n));
                    }
                }
                if let Some(s) = k.obj_mut(slide) {
                    s.set_ref(f, n);
                }
                added.push((theirs, n));
            }
            if let Some(a) = k.archive_mut(slide) {
                let s = a.obj_mut();
                for (src, n) in &added {
                    if let Some(tag) = tag_of(*src) {
                        let mut e = kn_format::Msg::new("KN.SlideArchive.SageTagMapEntry");
                        e.set_str("tag", &tag);
                        e.set_ref("info", *n);
                        s.push_msg("sage_tag_to_info_map", e);
                    }
                }
                a.refresh_object_references();
            }
            Ok(())
        });
        for (f, present) in had {
            if present {
                self.set_placeholder_visible(h, slide, f, shown_in_layout(f));
            }
        }
        self.reapply_layout(h, slide);
        if let Some(node) = self.slides().into_iter().find(|i| i.slide == slide).map(|i| i.node) {
            self.touch(h, node);
            if let Some(n) = self.file.obj_mut(node) {
                n.set_bool("thumbnailsAreDirty", true);
            }
        }
        true
    }

    /// Format > Reapply Layout to Slide: the slide's title, body and slide-number placeholders get
    /// the layout's frame and shape style back (their text and its formatting stay).
    pub fn reapply_layout(&mut self, h: &mut History, slide: ObjId) -> usize {
        let Some(layout) = self.file.obj(slide).and_then(|m| m.ref_id("template_slide")) else { return 0 };
        let mut n = 0;
        for f in ["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder"] {
            let (Some(mine), Some(theirs)) = (self.file.obj(slide).and_then(|m| m.ref_id(f)), self.file.obj(layout).and_then(|m| m.ref_id(f))) else { continue };
            let Some(lm) = self.file.obj(theirs) else { continue };
            let geo = lm.find_super("TSD.DrawableArchive").and_then(|d| d.msg("geometry")).cloned();
            let style = lm.find_super("TSWP.ShapeInfoArchive").and_then(|i| i.find_super("TSD.ShapeArchive")).and_then(|s| s.ref_id("style"));
            let ps = lm.find_super("TSD.ShapeArchive").and_then(|s| s.msg("pathsource")).cloned();
            self.touch(h, mine);
            let Some(a) = self.file.archive_mut(mine) else { continue };
            let m = a.obj_mut();
            if let (Some(g), Some(d)) = (geo, m.find_super_mut("TSD.DrawableArchive")) {
                d.remove("geometry");
                d.push_msg("geometry", g);
            }
            if let Some(sh) = m.find_super_mut("TSD.ShapeArchive") {
                if let Some(st) = style {
                    sh.set_ref("style", st);
                }
                if let Some(p) = ps {
                    sh.remove("pathsource");
                    sh.push_msg("pathsource", p);
                }
            }
            a.refresh_object_references();
            n += 1;
        }
        if let Some(f) = self.file.file_of(slide).map(str::to_string) {
            self.file.ensure_external_refs(&f);
        }
        n
    }

    /// Strip a freshly copied slide down to its layout: placeholders emptied, other objects,
    /// builds, transition and notes removed.
    fn empty_new_slide(&mut self, h: &mut History, slide: ObjId) {
        let tagged: Vec<ObjId> = self.file.obj(slide).map(|s| s.msgs("sage_tag_to_info_map").filter_map(|e| e.ref_id("info")).collect()).unwrap_or_default();
        let keep: Vec<ObjId> = self
            .file
            .obj(slide)
            .map(|s| s.ref_ids("drawables_z_order"))
            .unwrap_or_default()
            .into_iter()
            .filter(|d| tagged.contains(d) || self.file.type_name(*d) == "KN.PlaceholderArchive")
            .collect();
        if let Some(a) = self.file.archive_mut(slide) {
            let s = a.obj_mut();
            s.retain_msgs("drawables_z_order", |r| keep.contains(&r.u64("identifier").unwrap_or(0)));
            s.retain_msgs("owned_drawables", |r| keep.contains(&r.u64("identifier").unwrap_or(0)));
            s.remove("builds");
            s.remove("buildChunks");
            if let Some(aa) = s.msg_path_mut("transition.attributes.animationAttributes") {
                aa.set_str("effect", "none");
            }
            a.refresh_object_references();
        }
        let hidden: Vec<ObjId> = ["titlePlaceholder", "bodyPlaceholder"].iter().filter_map(|f| self.file.obj(slide).and_then(|m| m.ref_id(f))).filter(|p| !keep.contains(p)).collect();
        let storages: Vec<ObjId> = keep
            .iter()
            .chain(hidden.iter())
            .filter_map(|p| self.file.obj(*p))
            .filter(|m| m.enum_name("kind") != Some("kKindSlideNumberPlaceholder"))
            .filter_map(|m| m.find_super("TSWP.ShapeInfoArchive").and_then(|i| i.ref_id("owned_storage")))
            .chain(self.notes_storage(slide))
            .collect();
        for st in storages {
            let len = self.storage_text(st).len();
            if len > 0 {
                self.replace_text(h, st, 0, len, "");
            }
        }
        if let Some(n) = self.slides().into_iter().find(|i| i.slide == slide).map(|i| i.node).and_then(|n| self.file.obj_mut(n)) {
            for f in ["hasBuilds", "hasTransition", "hasNote", "isSkipped", "has_explicit_builds"] {
                if n.has(f) {
                    n.set_bool(f, false);
                }
            }
        }
    }

    /// Slide > Show Slide Numbers on All Slides: make every slide's slide-number placeholder
    /// visible (it stays defined on hidden slides, only missing from the z-order).
    pub fn show_slide_numbers_everywhere(&mut self, h: &mut History, on: bool) {
        for info in self.slides() {
            let Some(p) = self.file.obj(info.slide).and_then(|s| s.ref_id("slideNumberPlaceholder")) else { continue };
            self.touch(h, info.slide);
            if let Some(a) = self.file.archive_mut(info.slide) {
                let s = a.obj_mut();
                let present = s.ref_ids("drawables_z_order").contains(&p);
                if on && !present {
                    s.push_ref("drawables_z_order", p);
                    if !s.ref_ids("owned_drawables").contains(&p) {
                        s.push_ref("owned_drawables", p);
                    }
                } else if !on && present {
                    s.retain_msgs("drawables_z_order", |r| r.u64("identifier") != Some(p));
                }
                a.refresh_object_references();
            }
            self.touch(h, info.node);
            if let Some(n) = self.file.obj_mut(info.node) {
                n.set_bool("isSlideNumberVisible", on);
            }
        }
        if let Some((show, _)) = self.file.first_of("KN.ShowArchive") {
            self.touch(h, show);
            if let Some(m) = self.file.obj_mut(show) {
                m.set_bool("slideNumbersVisible", on);
            }
        }
    }

    /// Start a new presentation from a theme file: keep its first slide only, emptied.
    pub fn new_from_theme(theme: kn_format::KeyFile) -> Result<Document, kn_format::Error> {
        let donor = std::sync::Arc::new(theme.clone());
        let mut d = Document::from_file(theme);
        d.layout_donor = Some(donor);
        let mut h = History::default();
        let n = d.slides().len();
        d.begin(&mut h, "new");
        if n > 1 {
            d.delete_slides(&mut h, &(1..n).collect::<Vec<_>>())?;
        }
        let first = d.slides()[0].slide;
        let stor: Vec<ObjId> = ["titlePlaceholder", "bodyPlaceholder"]
            .iter()
            .filter_map(|f| d.file.obj(first).and_then(|s| s.ref_id(f)))
            .filter_map(|p| d.file.obj(p).and_then(|m| m.find_super("TSWP.ShapeInfoArchive")).and_then(|i| i.ref_id("owned_storage")))
            .collect();
        for st in stor {
            let len = d.storage_text(st).len();
            d.replace_text(&mut h, st, 0, len, "");
        }
        if let Some(ns) = d.notes_storage(first) {
            let len = d.storage_text(ns).len();
            d.replace_text(&mut h, ns, 0, len, "");
        }
        d.end(&mut h);
        d.path = None;
        d.dirty = false;
        Ok(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn new_slide_keeps_layout_and_empties_placeholders() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let before = d.slides();
        let mut h = History::default();
        d.begin(&mut h, "New Slide");
        let i = d.new_slide_after(&mut h, 6).unwrap(); // "Title & Bullets"
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s = d2.slides();
        assert_eq!(s.len(), before.len() + 1);
        assert_eq!(d2.layout_name(s[i].slide), d2.layout_name(s[6].slide));
        let sc = d2.scene(&s, i).unwrap();
        let own: Vec<_> = sc.drawables.iter().filter(|x| !x.from_template).collect();
        assert!(own.iter().all(|x| x.placeholder.is_some()));
        for x in own {
            if let DrawableKind::Shape(sh) = &x.kind {
                if x.placeholder != Some(crate::PlaceholderKind::SlideNumber) {
                    assert!(sh.text.as_ref().is_none_or(|t| t.text.is_empty()));
                }
            }
        }
        assert!(sc.transition.is_none() && sc.builds.is_empty());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn slide_size_change_scales_and_centres() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/12-title-layout-text.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let old = d.slide_size();
        let title = d.file.obj(s[0].slide).and_then(|m| m.ref_id("titlePlaceholder")).unwrap();
        let before = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| x.id == title).unwrap().geom;
        let mut h = History::default();
        d.begin(&mut h, "Slide Size");
        assert!(d.set_slide_size(&mut h, 1024.0, 768.0));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let sz = d2.slide_size();
        assert_eq!((sz.w, sz.h), (1024.0, 768.0));
        let s2 = d2.slides();
        let after = d2.scene(&s2, 0).unwrap().drawables.into_iter().find(|x| x.id == title).unwrap().geom;
        let k = (1024.0 / old.w).min(768.0 / old.h);
        assert!((after.x - (before.x * k + (1024.0 - old.w * k) / 2.0)).abs() < 0.05);
        assert!((after.w - before.w * k).abs() < 0.05);
        // undo restores the size
        d.undo(&mut h);
        assert_eq!(d.slide_size().w, old.w);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn change_layout_round_trips() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let layouts = d.layouts();
        let cur = d.file.obj(s[1].slide).unwrap().ref_id("template_slide").unwrap();
        let other = layouts.iter().find(|l| l.0 != cur).unwrap().0;
        let mut h = History::default();
        d.begin(&mut h, "Layout");
        assert!(d.change_slide_layout(&mut h, s[1].slide, other));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let s2 = d2.slides();
        assert_eq!(d2.file.obj(s2[1].slide).unwrap().ref_id("template_slide"), Some(other));
        assert!(d2.scene(&s2, 1).is_some());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn change_theme_is_undoable() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut d = Document::open(dir.join("testdata/Keynote-Feature-Catalog.key")).unwrap();
        let before = d.file.to_zip().unwrap();
        let n = d.slides().len();
        let theme = kn_format::KeyFile::open(dir.join("crates/kn-app/data/themes/White.key")).unwrap();
        let (nd, _) = d.with_theme(theme).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Change Theme");
        d.replace_contents(&mut h, nd);
        d.end(&mut h);
        assert_eq!(d.slides().len(), n);
        assert_ne!(d.file.to_zip().unwrap(), before);
        assert_eq!(h.undo_label(), Some("Change Theme"));
        assert!(d.undo(&mut h));
        assert_eq!(d.file.to_zip().unwrap(), before);
        assert!(d.redo(&mut h));
        assert_ne!(d.file.to_zip().unwrap(), before);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
        fn change_layout_brings_its_placeholders() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R10-standard.key");
        let mut d = Document::open(p).unwrap();
        let slide = d.slides()[0].slide;
        let layout = |d: &Document, n: &str| d.layouts().into_iter().find(|l| l.1 == n).unwrap().0;
        let mut h = History::default();
        let shown = |d: &Document| {
            let sc = d.scene(&d.slides(), 0).unwrap();
            let ph = sc.drawables.iter().filter_map(|x| x.placeholder).collect::<Vec<_>>();
            let photos = sc.drawables.iter().filter(|x| matches!(x.kind, crate::DrawableKind::Image(_))).count();
            (ph, photos)
        };
        assert_eq!(shown(&d), (vec![], 0));
        let l = layout(&d, "Photo - Horizontal");
        d.begin(&mut h, "Change Layout");
        assert!(d.change_slide_layout(&mut h, slide, l));
        d.end(&mut h);
        let (ph, photos) = shown(&d);
        assert!(ph.contains(&crate::PlaceholderKind::Title) && photos == 1);
        // and back to Blank: placeholders hidden, the empty photo gone
        let l = layout(&d, "Blank");
        d.begin(&mut h, "Change Layout");
        assert!(d.change_slide_layout(&mut h, slide, l));
        d.end(&mut h);
        assert_eq!(shown(&d), (vec![], 0));
        // undo brings the photo layout back
        d.undo(&mut h);
        assert_eq!(shown(&d).1, 1);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        assert_eq!(shown(&d2).1, 1);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
        fn reapply_layout_moves_title_back() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/12-title-layout-text.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let title = d.file.obj(s[0].slide).and_then(|m| m.ref_id("titlePlaceholder")).unwrap();
        let before = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| x.id == title).unwrap().geom;
        let mut h = History::default();
        d.begin(&mut h, "Move");
        d.move_by(&mut h, title, 200.0, 150.0);
        d.end(&mut h);
        d.begin(&mut h, "Reapply");
        assert!(d.reapply_layout(&mut h, s[0].slide) >= 1);
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let after = d2.scene(&d2.slides(), 0).unwrap().drawables.into_iter().find(|x| x.id == title).unwrap().geom;
        assert!((after.x - before.x).abs() < 0.5 && (after.y - before.y).abs() < 0.5, "{before:?} vs {after:?}");
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn new_slides_from_theme_donor() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut d = Document::open(root.join("testdata/00-base.key")).unwrap();
        assert!(d.find_layout_donor(&root.join("crates/kn-app/data/themes")));
        let layouts = d.layouts();
        let mut h = History::default();
        d.begin(&mut h, "Add Slides");
        for (i, (l, _)) in layouts.iter().enumerate() {
            d.new_slide_from_layout(&mut h, *l, i + 1).unwrap();
        }
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s = d2.slides();
        for (i, (_, name)) in layouts.iter().enumerate() {
            assert_eq!(d2.layout_name(s[i + 1].slide).as_deref(), Some(name.as_str()));
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn new_slide_from_every_layout() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/00-base.key");
        let mut d = Document::open(p).unwrap();
        let layouts = d.layouts();
        assert!(layouts.len() > 5);
        let mut h = History::default();
        d.begin(&mut h, "Add Slides");
        for (i, (l, _)) in layouts.iter().enumerate() {
            d.new_slide_from_layout(&mut h, *l, i + 1).unwrap();
        }
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let s = d2.slides();
        assert_eq!(s.len(), layouts.len() + 1);
        for (i, (_, name)) in layouts.iter().enumerate() {
            assert_eq!(d2.layout_name(s[i + 1].slide).as_deref(), Some(name.as_str()));
            let sc = d2.scene(&s, i + 1).unwrap();
            // hidden placeholders too: Keynote keeps their text empty
            for f in ["titlePlaceholder", "bodyPlaceholder"] {
                let st = d2.file.obj(s[i + 1].slide).and_then(|m| m.ref_id(f)).and_then(|p| d2.file.obj(p)).and_then(|m| m.find_super("TSWP.ShapeInfoArchive")).and_then(|x| x.ref_id("owned_storage"));
                if let Some(st) = st {
                    assert_eq!(d2.storage_text(st), "", "{name}: {f} empty");
                }
            }
            // nothing drawn twice: no non-placeholder object both on the layout and the slide
            let own: Vec<_> = sc.drawables.iter().filter(|x| !x.from_template).collect();
            for x in &own {
                if let DrawableKind::Shape(sh) = &x.kind {
                    if matches!(x.placeholder, Some(crate::PlaceholderKind::Title) | Some(crate::PlaceholderKind::Body)) {
                        assert!(sh.text.as_ref().is_none_or(|t| t.text.is_empty()), "{name}: placeholder emptied");
                    }
                }
            }
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn new_document_from_theme() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/keywriter-template-White.key");
        let d = Document::new_from_theme(kn_format::KeyFile::open(p).unwrap()).unwrap();
        assert_eq!(d.slides().len(), 1);
        assert!(d.layouts().len() > 5);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShowMode {
    Normal,
    SelfPlaying,
    LinksOnly,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShowSettings {
    pub auto_play_on_open: bool,
    pub looping: bool,
    pub restart_if_idle: bool,
    pub idle_minutes: f64,
    pub mode: ShowMode,
    pub transition_delay: f64,
    pub build_delay: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShowSetting {
    AutoPlayOnOpen(bool),
    Loop(bool),
    RestartIfIdle(bool),
    IdleMinutes(f64),
    Mode(ShowMode),
    TransitionDelay(f64),
    BuildDelay(f64),
}

/// A user guide on a slide: vertical (x position) or horizontal (y position), in slide points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub pos: f32,
}

impl Document {
    /// The slide's own guides (KN.SlideArchive.userDefinedGuideStorage).
    pub fn guides(&self, slide: ObjId) -> Vec<Guide> {
        let Some(st) = self.file.obj(slide).and_then(|s| s.ref_id("userDefinedGuideStorage")).and_then(|g| self.file.obj(g)) else { return Vec::new() };
        st.msgs("userDefinedGuides")
            .map(|g| Guide { vertical: g.enum_name("type") == Some("Vertical"), pos: g.f32("position").unwrap_or(0.0) })
            .collect()
    }

    /// The guides of the slide's layout (View > Guides > Show Slide Layout Guides).
    pub fn layout_guides(&self, slide: ObjId) -> Vec<Guide> {
        self.file.obj(slide).and_then(|s| s.ref_id("template_slide")).map(|t| self.guides(t)).unwrap_or_default()
    }

    /// Replace the slide's guides (creates the guide storage when the slide has none).
    pub fn set_guides(&mut self, h: &mut History, slide: ObjId, guides: &[Guide]) -> bool {
        let fill = |m: &mut kn_format::Msg| {
            m.remove("userDefinedGuides");
            for g in guides {
                let mut u = kn_format::Msg::new("TSD.UserDefinedGuideArchive");
                u.set_enum("type", if g.vertical { "Vertical" } else { "Horizontal" });
                u.set_f32("position", g.pos);
                m.push_msg("userDefinedGuides", u);
            }
        };
        if let Some(st) = self.file.obj(slide).and_then(|s| s.ref_id("userDefinedGuideStorage")) {
            self.touch(h, st);
            if let Some(m) = self.file.obj_mut(st) {
                fill(m);
                return true;
            }
        }
        let Some(file) = self.file.file_of(slide).map(str::to_string) else { return false };
        let mut m = kn_format::Msg::new("TSD.GuideStorageArchive");
        fill(&mut m);
        self.structural(h, &[slide], |k| {
            let id = k.alloc_ids(1)[0];
            k.insert_archive(&file, kn_format::Archive::new_object(id, m));
            if let Some(s) = k.obj_mut(slide) {
                s.set_ref("userDefinedGuideStorage", id);
            }
            k.refresh_refs(slide);
            true
        })
    }
}

impl Document {
    /// Slide index of a "?slideid=<UUID>" link target (a slide node UUID).
    pub fn slide_for_link_uuid(&self, uuid: &str) -> Option<usize> {
        let node = self.file.object_by_uuid(uuid)?;
        self.slides().iter().position(|s| s.node == node)
    }

    /// Link `drawable` (on `from_slide`) to slide `target` the way Keynote does: "?slideid=<target
    /// node UUID>", plus the back-index on the source slide's node.
    pub fn set_slide_link(&mut self, h: &mut History, drawable: ObjId, from_slide: ObjId, target: usize) -> bool {
        let slides = self.slides();
        let Some(node) = slides.get(target).map(|s| s.node) else { return false };
        let Some(node_uuid) = self.file.object_uuid(node) else { return false };
        let file = self.file.file_of(drawable).map(str::to_string);
        if self.file.object_uuid(drawable).is_none() {
            if let Some(f) = &file {
                let f = f.clone();
                self.structural(h, &[], |k| k.add_uuid_entry(&f, drawable));
            }
        }
        let Some(obj_uuid) = self.file.object_uuid(drawable) else { return false };
        self.set_hyperlink(h, drawable, Some(&format!("?slideid={node_uuid}")));
        let Some(src) = slides.iter().find(|s| s.slide == from_slide).map(|s| s.node) else { return true };
        self.touch(h, src);
        if let Some(n) = self.file.obj_mut(src) {
            let has = n.msgs("slideSpecificHyperlinkMap").any(|e| e.str("slideNodeIDString") == Some(node_uuid.as_str()));
            if has {
                if let Some(e) = n.msgs_mut("slideSpecificHyperlinkMap").find(|e| e.str("slideNodeIDString") == Some(node_uuid.as_str())) {
                    if !e.strs("infoOrStorageIDString").contains(&obj_uuid.as_str()) {
                        let mut v: Vec<String> = e.strs("infoOrStorageIDString").into_iter().map(String::from).collect();
                        v.push(obj_uuid.clone());
                        let refs: Vec<&str> = v.iter().map(String::as_str).collect();
                        e.set_strs("infoOrStorageIDString", &refs);
                    }
                }
            } else {
                let mut e = kn_format::Msg::new("KN.SlideNodeArchive.SlideSpecificHyperlinkMapEntry");
                e.set_str("slideNodeIDString", &node_uuid);
                e.set_strs("infoOrStorageIDString", &[obj_uuid.as_str()]);
                n.push_msg("slideSpecificHyperlinkMap", e);
            }
            let count = n.count("slideSpecificHyperlinkMap") as u64;
            n.set_u64("slideSpecificHyperlinkCount", count);
        }
        true
    }
}

#[cfg(test)]
mod link_tests {
    use crate::edit::History;
    use crate::Document;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn slide_id_links_like_keynote() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R11-links.key")).unwrap();
        // Keynote's own link to slide 3
        assert_eq!(d.slide_for_link_uuid("F3EB27A9-E848-428A-8914-2EE6CD036ABD"), Some(2));
        let s = d.slides();
        let sc = d.scene(&s, 1).unwrap();
        let mut h = History::default();
        let target_obj = d.scene(&s, 0).unwrap().drawables.iter().find(|x| !x.from_template).unwrap().id;
        let _ = sc;
        d.begin(&mut h, "Link");
        assert!(d.set_slide_link(&mut h, target_obj, s[0].slide, 4));
        d.end(&mut h);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let url = d2.file.obj(target_obj).and_then(|m| m.find_super("TSD.DrawableArchive")).and_then(|x| x.str("hyperlink_url")).unwrap().to_string();
        let id = url.strip_prefix("?slideid=").unwrap();
        assert_eq!(d2.slide_for_link_uuid(id), Some(4));
        let node = d2.slides()[0].node;
        assert!(d2.file.obj(node).unwrap().msgs("slideSpecificHyperlinkMap").any(|e| e.str("slideNodeIDString") == Some(id)));
    }
}

impl Document {
    /// Take over `other`'s contents as one undoable step (File > Change Theme); this document's
    /// path, file type and password stay.
    pub fn replace_contents(&mut self, h: &mut History, other: Document) {
        self.touch_all(h);
        self.file = other.file;
        self.layout_donor = other.layout_donor;
    }

    /// File > Change Theme: the presentation rebuilt on `theme`. Each slide takes the theme's
    /// layout of the same name (else its first layout with a title), keeps its title and body
    /// text, objects (pictures, shapes, charts, tables), animations, notes and transition.
    pub fn with_theme(&self, theme: kn_format::KeyFile) -> Result<(Document, Vec<String>), kn_format::Error> {
        let mut nd = Document::new_from_theme(theme)?;
        let mut h = History::default();
        let mut problems = Vec::new();
        nd.begin(&mut h, "theme");
        let size = self.slide_size();
        let ns = nd.slide_size();
        if (size.w - ns.w).abs() > 0.5 || (size.h - ns.h).abs() > 0.5 {
            nd.set_slide_size(&mut h, size.w, size.h);
        }
        let layouts = nd.layouts();
        let fallback = layouts
            .iter()
            .find(|(id, _)| nd.file.obj(*id).is_some_and(|m| m.ref_id("titlePlaceholder").is_some() && m.ref_id("bodyPlaceholder").is_some()))
            .or(layouts.first())
            .map(|l| l.0)
            .ok_or(kn_format::Error::Invalid("the theme has no layouts".into()))?;
        let old = self.slides();
        let first_new = nd.slides()[0].slide;
        let source = std::sync::Arc::new(self.file.clone());
        for (i, info) in old.iter().enumerate() {
            let name = self.layout_name(info.slide).unwrap_or_default();
            let layout = layouts.iter().find(|l| l.1 == name).map(|l| l.0).unwrap_or(fallback);
            let at = nd.slides().len();
            let ni = nd.new_slide_from_layout(&mut h, layout, at)?;
            let ns = nd.slides()[ni].slide;
            // title / body / slide number placeholders: text and visibility
            for f in ["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder"] {
                let mine = self.file.obj(info.slide).and_then(|m| m.ref_id(f));
                let shown = mine.is_some_and(|p| self.file.obj(info.slide).is_some_and(|s| s.ref_ids("drawables_z_order").contains(&p)));
                let theirs = nd.file.obj(ns).and_then(|m| m.ref_id(f));
                if let (Some(p), Some(q)) = (mine, theirs) {
                    if let (Some(a), Some(b)) = (self.storage_of(p), nd.storage_of(q)) {
                        let t = self.storage_text(a).replace('\u{FFFC}', "");
                        nd.set_storage_text(&mut h, b, &t);
                    }
                }
                if theirs.is_some() {
                    nd.set_placeholder_visible(&mut h, ns, f, shown);
                }
            }
            // everything else on the slide
            let others: Vec<ObjId> = self
                .file
                .obj(info.slide)
                .map(|s| s.ref_ids("drawables_z_order"))
                .unwrap_or_default()
                .into_iter()
                .filter(|id| !matches!(self.file.type_name(*id), "KN.PlaceholderArchive"))
                .filter(|id| !["titlePlaceholder", "bodyPlaceholder", "slideNumberPlaceholder"].iter().any(|f| self.file.obj(info.slide).and_then(|m| m.ref_id(f)) == Some(*id)))
                .collect();
            for id in others {
                let pasted = if self.file.type_name(id) == "TST.TableInfoArchive" {
                    let touch: Vec<ObjId> = nd.file.table_insert_touches().into_iter().chain([ns]).collect();
                    nd.structural(&mut h, &touch, |k| k.paste_table(&source, id, ns)).map(|t| vec![t])
                } else {
                    match self.copy_objects(&[id]) {
                        Some(clip) => nd.paste_objects(&mut h, ns, &clip, 0.0),
                        None => Ok(Vec::new()),
                    }
                };
                match pasted {
                    Ok(new_ids) => {
                        if let Some(&nid) = new_ids.first() {
                            let builds = self.copy_animation(info.slide, id);
                            if !builds.is_empty() {
                                let _ = nd.paste_animation(&mut h, ns, nid, &builds);
                            }
                        }
                    }
                    Err(e) => problems.push(format!("slide {}: {e}", i + 1)),
                }
            }
            let notes = self.notes(info.slide);
            if !notes.trim().is_empty() {
                nd.set_notes(&mut h, ns, &notes);
            }
            if let Some(t) = self.scene(&old, i).and_then(|s| s.transition) {
                if t.effect != "none" && !t.effect.is_empty() {
                    nd.set_transition(&mut h, ns, &t.effect, t.duration, t.delay, t.automatic);
                }
            }
        }
        // the theme's starter slide goes
        if let Some(pos) = nd.slides().iter().position(|s| s.slide == first_new) {
            nd.delete_slides(&mut h, &[pos])?;
        }
        nd.end(&mut h);
        nd.path = self.path.clone();
        nd.dirty = true;
        Ok((nd, problems))
    }
}

impl Document {
    /// Format > Advanced > Define as Text / Media Placeholder (while editing a layout): the
    /// object becomes a tagged placeholder of `layout`, with `display` as its prompt. Every
    /// slide on the layout gets its own empty copy with the same tag (as Keynote does).
    pub fn define_placeholder(&mut self, h: &mut History, layout: ObjId, object: ObjId, tag: &str, display: &str) -> Result<usize, kn_format::Error> {
        self.touch(h, layout);
        {
            let l = self.file.obj_mut(layout).ok_or(kn_format::Error::Invalid("no layout".into()))?;
            l.retain_msgs("sage_tag_to_info_map", |e| e.ref_id("info") != Some(object));
            let mut e = kn_format::Msg::new("KN.SlideArchive.SageTagMapEntry");
            e.set_str("tag", tag);
            e.set_ref("info", object);
            l.push_msg("sage_tag_to_info_map", e);
            let m = l.msg_mut_or_create("instructional_text_map");
            m.retain_msgs("instructional_text_for_infos", |e| e.ref_id("info") != Some(object));
            let mut it = kn_format::Msg::new("KN.SlideArchive.InstructionalTextMap.InstructionalTextMapEntry");
            it.set_ref("info", object);
            it.set_str("instructional_text", display);
            m.push_msg("instructional_text_for_infos", it);
        }
        self.file.refresh_refs(layout);
        // the slides on this layout get their own copy, tagged the same
        let users: Vec<ObjId> = self.slides().into_iter().map(|s| s.slide).filter(|s| self.file.obj(*s).and_then(|m| m.ref_id("template_slide")) == Some(layout)).collect();
        let clip = self.copy_objects(&[object]).ok_or(kn_format::Error::Invalid("can't copy that object".into()))?;
        for s in &users {
            let ids = self.paste_objects(h, *s, &clip, 0.0)?;
            let Some(&copy) = ids.first() else { continue };
            if let Some(st) = self.storage_of(copy) {
                self.set_storage_text(h, st, "");
            }
            self.touch(h, *s);
            if let Some(m) = self.file.obj_mut(*s) {
                let mut e = kn_format::Msg::new("KN.SlideArchive.SageTagMapEntry");
                e.set_str("tag", tag);
                e.set_ref("info", copy);
                m.push_msg("sage_tag_to_info_map", e);
            }
            self.file.refresh_refs(*s);
        }
        Ok(users.len())
    }
}

#[cfg(test)]
mod placeholder_tests {
    use crate::edit::History;
    use crate::insert::NewObject;
    use crate::Document;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn define_text_placeholder() {
        let mut d = Document::open(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref/R13-placeholder-before.key")).unwrap();
        let s = d.slides();
        let layout = d.file.obj(s[0].slide).and_then(|m| m.ref_id("template_slide")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "box");
        let tb = d.insert(&mut h, layout, &NewObject::TextBox).unwrap();
        d.end(&mut h);
        d.begin(&mut h, "define");
        let n = d.define_placeholder(&mut h, layout, tb, "MyTag", "My display name").unwrap();
        d.end(&mut h);
        assert!(n >= 1);
        let k = kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap();
        let d2 = Document::from_file(k);
        let l = d2.file.obj(layout).unwrap();
        assert!(l.msgs("sage_tag_to_info_map").any(|e| e.str("tag") == Some("MyTag")));
        let s2 = d2.slides();
        let slide = d2.file.obj(s2[0].slide).unwrap();
        assert!(slide.msgs("sage_tag_to_info_map").any(|e| e.str("tag") == Some("MyTag")));
    }
}
