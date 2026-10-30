//! Adding, changing and removing builds (object animations), the way the research writer did it:
//! copy a build Keynote made (the catalog's Pop build-in, or its Rotate action) and its Build
//! Order chunk, then set the effect. Effect ids come from Keynote's own binaries (see
//! effect-id-strings-from-keynote-binary.txt). Keynote 15.4 recognised 52 of these (it exported them
//! to PowerPoint as matching effects); Move got a proper motion path afterwards.

use std::sync::OnceLock;

use kn_format::{KeyFile, Msg, ObjId};

use crate::edit::History;
use crate::Document;

/// One effect option of a build (stored in its BuildAttributesArchive, as Keynote does).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuildOption {
    /// 0 none, 1 ease in, 2 ease out, 3 ease in and out.
    Acceleration(u32),
    /// Rotate action: degrees.
    RotationAngle(f64),
    RotationClockwise(bool),
    /// Scale action: 2.0 = 200 %.
    ScaleSize(f64),
    /// Opacity action: 0..1.
    Opacity(f64),
    Bounce(bool),
    /// Travel direction (11 left→right, 12 right→left, 13 top→bottom, 14 bottom→top,
    /// 21–24 diagonals, 41/42 Iris in/out, 43/44 Drift and Scale up/down; verified on the Mac).
    Direction(u32),
}

/// When a Build Order step starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildStart {
    OnClick,
    AfterPrevious,
    WithPrevious,
}

impl BuildStart {
    pub const ALL: [BuildStart; 3] = [BuildStart::OnClick, BuildStart::AfterPrevious, BuildStart::WithPrevious];
    pub fn name(self) -> &'static str {
        match self {
            BuildStart::OnClick => "On Click",
            BuildStart::AfterPrevious => "After Previous",
            BuildStart::WithPrevious => "With Previous",
        }
    }
}

/// One build copied with Copy Animation.
#[derive(Clone, Debug)]
pub struct CopiedBuild {
    pub kind: BuildKind,
    pub effect: String,
    pub duration: f64,
    /// The build's whole attributes message (direction, delivery options, motion path…).
    pub attrs: Msg,
    pub delivery: Option<String>,
    pub automatic: bool,
    pub delay: f64,
}

/// How a text build is delivered (Keynote's Delivery / Text Animation; verified on the Mac).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    AllAtOnce,
    /// One Build Order step per paragraph.
    ByParagraph,
    /// Text Animation by word / by character (still one step).
    ByWord,
    ByCharacter,
    /// One step per top-level paragraph, with its indented paragraphs.
    ByParagraphGroup,
    /// One step per paragraph; the ones before it are dimmed.
    ByHighlightedParagraph,
}

impl Delivery {
    pub const ALL: [Delivery; 6] = [Delivery::AllAtOnce, Delivery::ByParagraph, Delivery::ByParagraphGroup, Delivery::ByHighlightedParagraph, Delivery::ByWord, Delivery::ByCharacter];
    pub fn name(self) -> &'static str {
        match self {
            Delivery::AllAtOnce => "All at Once",
            Delivery::ByParagraph => "By Paragraph",
            Delivery::ByParagraphGroup => "By Paragraph Group",
            Delivery::ByHighlightedParagraph => "By Highlighted Paragraph",
            Delivery::ByWord => "By Word",
            Delivery::ByCharacter => "By Character",
        }
    }

    /// The string Keynote stores in BuildArchive.delivery (legacy names for the bullet ones,
    /// checked against Keynote 15.4).
    pub fn stored(self) -> &'static str {
        match self {
            Delivery::ByParagraphGroup => "By Bullet Group",
            Delivery::ByHighlightedParagraph => "By Highlighted Bullet",
            d => d.name(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildKind {
    In,
    Out,
    Action,
}

impl BuildKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BuildKind::In => "In",
            BuildKind::Out => "Out",
            BuildKind::Action => "Action",
        }
    }
}

/// (menu name, effect id, kinds it applies to, verified in a real Keynote file)
pub const EFFECTS: &[(&str, &str, &[BuildKind], bool)] = &[
    ("Appear", "apple:bc-appear", &[BuildKind::In], true),
    ("Disappear", "apple:bc-appear", &[BuildKind::Out], false),
    ("Dissolve", "apple:dissolve", &[BuildKind::In, BuildKind::Out], false),
    ("Dissolve (by character)", "apple:dissolve character", &[BuildKind::In, BuildKind::Out], true),
    ("Move In", "apple:move in", &[BuildKind::In], false),
    ("Move In (by character)", "apple:move in character", &[BuildKind::In], true),
    ("Move Out", "apple:move in", &[BuildKind::Out], false),
    ("Fly In", "apple:drive-by", &[BuildKind::In], false),
    ("Fly Out", "apple:drive-by", &[BuildKind::Out], false),
    ("Fade and Move", "apple:fade and move", &[BuildKind::In, BuildKind::Out], false),
    ("Fade and Scale", "apple:bc-zoom", &[BuildKind::In, BuildKind::Out], false),
    ("Blur", "com.apple.iWork.Keynote.Blur", &[BuildKind::In, BuildKind::Out], true),
    ("Compress", "apple:compress", &[BuildKind::In], false),
    ("Drift", "apple:drift", &[BuildKind::In], false),
    ("Drift and Scale", "apple:drift and scale", &[BuildKind::In], false),
    ("Drop", "apple:bc-drop", &[BuildKind::In], false),
    ("Iris", "apple:wipe-iris", &[BuildKind::In, BuildKind::Out], false),
    ("Keyboard", "apple:keyboard", &[BuildKind::In, BuildKind::Out], false),
    ("Line Draw", "com.apple.iWork.Keynote.LineDrawForLine", &[BuildKind::In], true),
    ("Radial Wipe", "apple:wipe-radial", &[BuildKind::In, BuildKind::Out], false),
    ("Shift", "apple:shift", &[BuildKind::In, BuildKind::Out], false),
    ("Shift and Scale", "apple:shift and scale", &[BuildKind::In, BuildKind::Out], false),
    ("Wipe", "apple:wipe", &[BuildKind::In, BuildKind::Out], false),
    ("Blinds", "com.apple.iWork.Keynote.BLTBlinds", &[BuildKind::In, BuildKind::Out], false),
    ("Cube", "apple:bc-3D-cube", &[BuildKind::In, BuildKind::Out], false),
    ("Flip", "apple:bc-flip", &[BuildKind::In, BuildKind::Out], false),
    ("Orbital", "apple:bc-orbital", &[BuildKind::In, BuildKind::Out], false),
    ("Pivot", "apple:pivot-build", &[BuildKind::In, BuildKind::Out], false),
    ("Pop", "apple:bc-pop", &[BuildKind::In, BuildKind::Out], true),
    ("Scale", "apple:scale", &[BuildKind::In, BuildKind::Out], false),
    ("Scale Big", "apple:bc-zoom-big", &[BuildKind::In, BuildKind::Out], true),
    ("Spin", "apple:spin", &[BuildKind::In, BuildKind::Out], false),
    ("Swoosh", "com.apple.iWork.Keynote.BLTSwoosh", &[BuildKind::In, BuildKind::Out], false),
    ("Twirl", "apple:bc-twirl", &[BuildKind::In, BuildKind::Out], false),
    ("Twist and Scale", "apple:twist-and-scale", &[BuildKind::In, BuildKind::Out], false),
    ("Anvil", "com.apple.iWork.Keynote.BUKAnvil", &[BuildKind::In], false),
    ("Blast", "com.apple.iWork.Keynote.KLNBCBlast", &[BuildKind::In, BuildKind::Out], false),
    ("Bouncy", "com.apple.iWork.Keynote.KLNBouncy", &[BuildKind::In, BuildKind::Out], false),
    ("Comet", "com.apple.iWork.Keynote.KLNComet", &[BuildKind::In, BuildKind::Out], false),
    ("Confetti", "com.apple.iWork.Keynote.KLNConfetti", &[BuildKind::In, BuildKind::Out], false),
    ("Fireworks", "com.apple.iWork.Keynote.KNFireworks", &[BuildKind::In, BuildKind::Out], false),
    ("Flame", "com.apple.iWork.Keynote.KLNFlame", &[BuildKind::In, BuildKind::Out], false),
    ("Flash Bulbs", "com.apple.iWork.Keynote.BUKFlashBulbs", &[BuildKind::In], false),
    ("Lens Flare", "com.apple.iWork.Keynote.BUKLensFlare", &[BuildKind::In], false),
    ("Shimmer", "com.apple.iWork.Keynote.KLNShimmer", &[BuildKind::In, BuildKind::Out], false),
    ("Skid", "com.apple.iWork.Keynote.KNBuildSkid", &[BuildKind::In, BuildKind::Out], false),
    ("Sparkle", "com.apple.iWork.Keynote.KLNSparkle", &[BuildKind::In, BuildKind::Out], false),
    ("Squish", "com.apple.iWork.Keynote.KLNSquish", &[BuildKind::In, BuildKind::Out], false),
    ("Trace", "com.apple.iWork.Keynote.Trace", &[BuildKind::In], false),
    ("Move", "apple:action-motion-path", &[BuildKind::Action], true),
    ("Rotate", "apple:action-rotation", &[BuildKind::Action], true),
    ("Opacity", "apple:action-opacity", &[BuildKind::Action], false),
    ("Scale (action)", "apple:action-scale", &[BuildKind::Action], false),
    // Emphasis actions (verified on the Mac)
    ("Blink", "apple:action-blink", &[BuildKind::Action], false),
    ("Bounce", "apple:action-bounce", &[BuildKind::Action], false),
    ("Jiggle", "apple:action-jiggle", &[BuildKind::Action], false),
    ("Pulse", "apple:action-pulse", &[BuildKind::Action], false),
    ("Pop (action)", "apple:action-pop", &[BuildKind::Action], false),
    ("Flip (action)", "apple:action-flip", &[BuildKind::Action], false),
];

/// Display name of an effect id (for the inspector).
pub fn effect_name(id: &str, kind: &str) -> Option<&'static str> {
    EFFECTS
        .iter()
        .find(|(_, e, kinds, _)| *e == id && kinds.iter().any(|k| k.as_str() == kind))
        .or_else(|| EFFECTS.iter().find(|(_, e, _, _)| *e == id))
        .map(|(n, _, _, _)| *n)
}

static CATALOG: &[u8] = include_bytes!("../data/donors/build-donor.key");

fn catalog() -> &'static KeyFile {
    static K: OnceLock<KeyFile> = OnceLock::new();
    K.get_or_init(|| KeyFile::from_zip(CATALOG).expect("embedded catalog"))
}

/// A donor build (and its chunk) for a kind: from the open document when it has one of that
/// kind, else Keynote's own from the catalog (Pop build-in / Rotate action).
fn donor(doc: &KeyFile, kind: BuildKind) -> Option<(Msg, Msg)> {
    let want = |m: &Msg| {
        let t = m.msg_path("attributes.animationAttributes").and_then(|a| a.str("animation_type"));
        match kind {
            BuildKind::Action => t == Some("Action"),
            _ => t == Some("In") || t == Some("Out"),
        }
    };
    for k in [doc, catalog()] {
        for (id, b) in k.objects_of("KN.BuildArchive") {
            if !want(b) || b.str("delivery") == Some("Continuous") {
                continue;
            }
            let chunk = k.objects_of("KN.BuildChunkArchive").find(|(_, c)| c.ref_id("build") == Some(id)).map(|(_, c)| c.clone());
            if let Some(c) = chunk {
                return Some((b.clone(), c));
            }
        }
    }
    None
}

fn new_uuid(m: &mut Msg) {
    let u = uuid::Uuid::new_v4().as_u128();
    m.set_u64("lower", u as u64);
    m.set_u64("upper", (u >> 64) as u64);
}

fn set_effect(b: &mut Msg, kind: BuildKind, effect: &str, duration: f64, direction: Option<u32>) {
    let attrs = b.msg_mut_or_create("attributes");
    if kind != BuildKind::Action {
        for f in ["action_rotationAngle", "action_rotationDirection", "action_scaleSize", "action_colorAlpha", "action_motionPathSource", "motionPathString"] {
            attrs.remove(f);
        }
    }
    match effect {
        "apple:action-rotation" => {
            if !attrs.has("action_rotationAngle") {
                attrs.set_f64("action_rotationAngle", 360.0);
            }
        }
        "apple:action-opacity" => attrs.set_f64("action_colorAlpha", 0.5),
        "apple:action-motion-path" => {
            if !attrs.has("action_motionPathSource") {
                attrs.push_msg("action_motionPathSource", straight_motion_path(300.0, 0.0));
            }
            attrs.set_enum("action_acceleration", "kEaseBoth");
        }
        "apple:action-scale" => attrs.set_f64("action_scaleSize", 2.0),
        // emphasis actions with Keynote's defaults (verified on the Mac)
        "apple:action-blink" => {
            attrs.set_bool("custom_action_decay", false);
            attrs.set_u64("custom_action_repeatCount", 4);
        }
        "apple:action-bounce" => {
            attrs.set_bool("custom_action_decay", true);
            attrs.set_u64("custom_action_repeatCount", 3);
        }
        "apple:action-flip" => attrs.set_u64("custom_action_repeatCount", 4),
        "apple:action-jiggle" => attrs.set_enum("custom_action_jiggleIntensity", "kJiggleIntensityMedium"),
        "apple:action-pop" => attrs.set_f64("custom_action_scale", 120.0),
        "apple:action-pulse" => {
            attrs.set_u64("custom_action_repeatCount", 4);
            attrs.set_f64("custom_action_scale", 110.0);
        }
        _ => {}
    }
    let aa = attrs.msg_mut_or_create("animationAttributes");
    aa.set_str("animation_type", kind.as_str());
    aa.set_str("effect", effect);
    aa.set_f64("duration", duration);
    if let Some(d) = direction {
        aa.set_u64("direction", u64::from(d));
    }
    aa.set_u64("random_number_seed", u64::from(rand_seed()));
}

/// A Move action's path (laid out like the ones Keynote writes): editable bezier from the
/// object's position to (dx, dy), two sharp nodes, open.
fn straight_motion_path(dx: f32, dy: f32) -> Msg {
    let pt = |x: f32, y: f32| {
        let mut p = Msg::new("TSP.Point");
        p.set_f32("x", x);
        p.set_f32("y", y);
        p
    };
    let mut sub = Msg::new("TSD.EditableBezierPathSourceArchive.Subpath");
    for (x, y) in [(0.0, 0.0), (dx, dy)] {
        let mut n = Msg::new("TSD.EditableBezierPathSourceArchive.Node");
        n.push_msg("inControlPoint", pt(x, y));
        n.push_msg("nodePoint", pt(x, y));
        n.push_msg("outControlPoint", pt(x, y));
        n.set_enum("type", "sharp");
        sub.push_msg("nodes", n);
    }
    sub.set_bool("closed", false);
    let mut eb = Msg::new("TSD.EditableBezierPathSourceArchive");
    eb.push_msg("subpaths", sub);
    let mut ns = Msg::new("TSP.Size");
    ns.set_f32("width", dx.abs());
    ns.set_f32("height", dy.abs());
    eb.push_msg("naturalSize", ns);
    let mut ps = Msg::new("TSD.PathSourceArchive");
    ps.set_bool("horizontalFlip", false);
    ps.set_bool("verticalFlip", false);
    ps.push_msg("editable_bezier_path_source", eb);
    ps
}

fn rand_seed() -> u32 {
    (uuid::Uuid::new_v4().as_u128() & 0x7fff_ffff) as u32
}

impl Document {
    fn node_of(&self, slide: ObjId) -> Option<ObjId> {
        self.slides().into_iter().find(|s| s.slide == slide).map(|s| s.node)
    }

    fn update_build_flags(&mut self, h: &mut History, slide: ObjId) {
        let builds = self.file.obj(slide).map(|s| s.ref_ids("builds")).unwrap_or_default();
        let n = builds.len();
        // a gallery's Content build doesn't count as "has builds" (Keynote, T60); the event
        // count is the number of click steps
        let content = |b: &ObjId| {
            self.file.obj(*b).and_then(|m| m.msg("attributes")).and_then(|a| a.msg("animationAttributes")).and_then(|a| a.str("animation_type")) == Some("Content")
        };
        let has = builds.iter().any(|b| !content(b));
        let gallery_only = n > 0 && !has;
        let clicks = self.file.obj(slide).map(|s| s.ref_ids("buildChunks")).unwrap_or_default().iter().filter(|c| self.file.obj(**c).is_some_and(|m| !m.bool("automatic").unwrap_or(false))).count();
        if let Some(node) = self.node_of(slide) {
            self.touch(h, node);
            if let Some(m) = self.file.obj_mut(node) {
                m.set_bool("hasBuilds", has);
                m.set_bool("has_explicit_builds", n > 0);
                m.set_u64("build_event_count", if gallery_only { clicks as u64 } else { n as u64 });
                if gallery_only {
                    m.set_u64("build_event_count_cache_version", 2);
                }
            }
        }
    }

    /// Add a build to `drawable` on `slide` (appended to the Build Order, starting on click).
    pub fn add_build(&mut self, h: &mut History, slide: ObjId, drawable: ObjId, kind: BuildKind, effect: &str, duration: f64) -> Result<ObjId, kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        let (mut b, mut c) = donor(&self.file, kind).ok_or(err("no donor build"))?;
        let file = self.file.file_of(slide).ok_or(err("slide file"))?.to_string();
        let src_info = |k: &KeyFile, ty: &str| k.objects_of(ty).next().and_then(|(id, _)| k.archive(id)).map(|a| a.info.clone());
        let b_info = src_info(&self.file, "KN.BuildArchive").or_else(|| src_info(catalog(), "KN.BuildArchive")).ok_or(err("donor"))?;
        let c_info = src_info(&self.file, "KN.BuildChunkArchive").or_else(|| src_info(catalog(), "KN.BuildChunkArchive")).ok_or(err("donor"))?;
        self.structural(h, &[slide], |k| -> Result<ObjId, kn_format::Error> {
            let ids = k.alloc_ids(2);
            let (bid, cid) = (ids[0], ids[1]);
            b.set_ref("drawable", drawable);
            b.set_str("delivery", "All at Once");
            set_effect(&mut b, kind, effect, duration, None);
            c.set_ref("build", bid);
            c.set_f64("delay", 0.0);
            c.set_f64("duration", duration);
            c.set_bool("automatic", false);
            if let Some(u) = c.msg_mut("build_id") {
                new_uuid(u);
            }
            let u = c.msg("build_id").cloned();
            if let (Some(ci), Some(u)) = (c.msg_mut("build_chunk_identifier"), u) {
                ci.remove("build_id");
                ci.insert_msg("build_id", 0, u);
            }
            for (id, payload, info) in [(bid, b, b_info), (cid, c, c_info)] {
                let mut a = kn_format::Archive { info, payloads: vec![payload] };
                a.info.set_u64("identifier", id);
                a.set_object_references(0, &[]);
                a.set_data_references(0, &[]);
                k.insert_archive(&file, a);
            }
            if let Some(sa) = k.archive_mut(slide) {
                sa.obj_mut().push_ref("builds", bid);
                sa.obj_mut().push_ref("buildChunks", cid);
            }
            k.refresh_refs(slide);
            Ok(bid)
        })
        .inspect(|_| self.update_build_flags(h, slide))
    }

    /// An image gallery's build (Keynote adds "Appear" to every new gallery): one On Click
    /// step per image after the first, each showing the next image.
    pub fn add_gallery_build(&mut self, h: &mut History, slide: ObjId, gallery: ObjId, effect: &str, images: usize) -> Result<ObjId, kn_format::Error> {
        let bid = self.add_build(h, slide, gallery, BuildKind::Action, effect, 0.5)?;
        let steps = images.saturating_sub(1).max(1);
        self.structural(h, &[slide], |k| -> Result<(), kn_format::Error> {
            if let Some(b) = k.obj_mut(bid) {
                b.set_f64("duration", 0.0);
                b.set_u64("chunk_id_seed", steps as u64);
                // exactly what Keynote writes for a gallery build (nothing left from the donor)
                let seed = b.msg("attributes").and_then(|a| a.msg("animationAttributes")).and_then(|a| a.u64("random_number_seed")).unwrap_or(rand_seed() as u64);
                let mut aa = kn_format::Msg::new("KN.AnimationAttributesArchive");
                aa.set_str("animation_type", "Content");
                aa.set_str("effect", effect);
                aa.set_f64("duration", 0.5);
                aa.set_f64("delay", 0.0);
                aa.set_u64("random_number_seed", seed);
                aa.set_bool("writing_direction_is_rtl", false);
                let mut at = kn_format::Msg::new("KN.BuildAttributesArchive");
                at.set_u64("eventTrigger", 1);
                at.set_f64("ChartRotation3D", 60.0);
                at.push_msg("animationAttributes", aa);
                b.remove("attributes");
                b.push_msg("attributes", at);
            }
            let first = k.obj(slide).map(|s| s.ref_ids("buildChunks")).unwrap_or_default().into_iter().rev().find(|c| k.obj(*c).and_then(|m| m.ref_id("build")) == Some(bid));
            let Some(first) = first else { return Ok(()) };
            let set_chunk = |m: &mut kn_format::Msg, n: u64| {
                m.set_u64("referent", 1);
                if let Some(ci) = m.msg_mut("build_chunk_identifier") {
                    ci.set_u64("build_chunk_id", n);
                }
            };
            if let Some(m) = k.obj_mut(first) {
                set_chunk(m, 1);
            }
            let file = k.file_of(slide).unwrap_or_default().to_string();
            for n in 2..=steps as u64 {
                let Some(mut a) = k.archive(first).cloned() else { break };
                let id = k.alloc_ids(1)[0];
                a.info.set_u64("identifier", id);
                set_chunk(&mut a.payloads[0], n);
                k.insert_archive(&file, a);
                if let Some(sa) = k.archive_mut(slide) {
                    sa.obj_mut().push_ref("buildChunks", id);
                }
            }
            k.refresh_refs(slide);
            Ok(())
        })?;
        self.update_build_flags(h, slide);
        Ok(bid)
    }

    /// Insert > Web Video's "Load video on click": the automatic build Keynote adds with a web
    /// video (effect apple:web-video-start, In, one On Click step; Mac round R37).
    pub fn add_web_video_build(&mut self, h: &mut History, slide: ObjId, video: ObjId) -> Result<ObjId, kn_format::Error> {
        let bid = self.add_build(h, slide, video, BuildKind::In, "apple:web-video-start", 0.5)?;
        self.structural(h, &[slide], |k| {
            if let Some(b) = k.obj_mut(bid) {
                b.set_f64("duration", 0.0);
                b.set_u64("chunk_id_seed", 1);
                b.set_str("delivery", "All at Once");
                let seed = b.msg("attributes").and_then(|a| a.msg("animationAttributes")).and_then(|a| a.u64("random_number_seed")).unwrap_or(rand_seed() as u64);
                let mut aa = kn_format::Msg::new("KN.AnimationAttributesArchive");
                aa.set_str("animation_type", "In");
                aa.set_str("effect", "apple:web-video-start");
                aa.set_f64("duration", 0.5);
                aa.set_f64("delay", 0.0);
                aa.set_u64("random_number_seed", seed);
                aa.set_bool("writing_direction_is_rtl", false);
                let mut at = kn_format::Msg::new("KN.BuildAttributesArchive");
                at.set_u64("eventTrigger", 1);
                at.set_f64("ChartRotation3D", 60.0);
                at.push_msg("animationAttributes", aa);
                b.remove("attributes");
                b.push_msg("attributes", at);
            }
            k.refresh_refs(slide);
        });
        self.update_build_flags(h, slide);
        Ok(bid)
    }

    /// The web video build of `video` on `slide`, if it has one.
    pub fn web_video_build(&self, slide: ObjId, video: ObjId) -> Option<ObjId> {
        self.file.obj(slide)?.ref_ids("builds").into_iter().find(|b| {
            self.file.obj(*b).is_some_and(|m| {
                m.ref_id("drawable") == Some(video)
                    && m.msg("attributes").and_then(|a| a.msg("animationAttributes")).and_then(|a| a.str("effect")) == Some("apple:web-video-start")
            })
        })
    }

    /// After a gallery's images changed: its build gets one step per image after the first
    /// again (keeping its effect; a gallery without one gets Keynote's "Appear").
    pub fn sync_gallery_build(&mut self, h: &mut History, slide: ObjId, gallery: ObjId, images: usize) -> Result<(), kn_format::Error> {
        let builds: Vec<ObjId> = self.file.obj(slide).map(|s| s.ref_ids("builds")).unwrap_or_default();
        let mut effect = "apple:gallery-appear".to_string();
        for b in builds {
            let Some(m) = self.file.obj(b) else { continue };
            if m.ref_id("drawable") != Some(gallery) {
                continue;
            }
            let e = m.msg("attributes").and_then(|a| a.msg("animationAttributes")).and_then(|a| a.str("effect")).unwrap_or("").to_string();
            if e.starts_with("apple:gallery") {
                effect = e;
                self.remove_build(h, slide, b);
            }
        }
        if images > 1 {
            self.add_gallery_build(h, slide, gallery, &effect, images)?;
        }
        Ok(())
    }

    /// Change a build's effect / duration.
    pub fn set_build_effect(&mut self, h: &mut History, slide: ObjId, build: ObjId, effect: &str, duration: f64) {
        self.touch(h, build);
        let kind = match self.file.obj(build).and_then(|b| b.msg_path("attributes.animationAttributes")).and_then(|a| a.str("animation_type")) {
            Some("Out") => BuildKind::Out,
            Some("Action") => BuildKind::Action,
            _ => BuildKind::In,
        };
        if let Some(b) = self.file.obj_mut(build) {
            set_effect(b, kind, effect, duration, None);
        }
        // keep the Build Order chunk's duration in step
        let chunks: Vec<ObjId> = self.file.obj(slide).map(|s| s.ref_ids("buildChunks")).unwrap_or_default();
        for c in chunks {
            if self.file.obj(c).and_then(|m| m.ref_id("build")) == Some(build) {
                self.touch(h, c);
                if let Some(m) = self.file.obj_mut(c) {
                    m.set_f64("duration", duration);
                }
            }
        }
    }

    /// Remove a build (and its Build Order chunks) from a slide.
    pub fn remove_build(&mut self, h: &mut History, slide: ObjId, build: ObjId) {
        self.touch(h, slide);
        let chunks: Vec<ObjId> = self
            .file
            .obj(slide)
            .map(|s| s.ref_ids("buildChunks"))
            .unwrap_or_default()
            .into_iter()
            .filter(|c| self.file.obj(*c).and_then(|m| m.ref_id("build")) == Some(build))
            .collect();
        if let Some(a) = self.file.archive_mut(slide) {
            let s = a.obj_mut();
            s.retain_msgs("builds", |r| r.u64("identifier") != Some(build));
            s.retain_msgs("buildChunks", |r| !chunks.contains(&r.u64("identifier").unwrap_or(0)));
        }
        self.file.refresh_refs(slide);
        self.update_build_flags(h, slide);
    }

    /// Format > Copy Animation: the builds of `drawable` on `slide`, in Build Order.
    pub fn copy_animation(&self, slide: ObjId, drawable: ObjId) -> Vec<CopiedBuild> {
        let Some(s) = self.file.obj(slide) else { return Vec::new() };
        let chunks: Vec<&Msg> = s.ref_ids("buildChunks").into_iter().filter_map(|c| self.file.obj(c)).collect();
        s.ref_ids("builds")
            .into_iter()
            .filter_map(|b| self.file.obj(b).map(|m| (b, m)))
            .filter(|(_, m)| m.ref_id("drawable") == Some(drawable))
            .filter_map(|(b, m)| {
                let attrs = m.msg("attributes")?.clone();
                let aa = attrs.msg("animationAttributes")?;
                let kind = match aa.str("animation_type") {
                    Some("Out") => BuildKind::Out,
                    Some("Action") => BuildKind::Action,
                    _ => BuildKind::In,
                };
                let chunk = chunks.iter().find(|c| c.ref_id("build") == Some(b));
                Some(CopiedBuild {
                    kind,
                    effect: aa.str("effect").unwrap_or("apple:dissolve").to_string(),
                    duration: aa.f64("duration").unwrap_or(1.0),
                    attrs,
                    delivery: m.str("delivery").map(str::to_string),
                    automatic: chunk.and_then(|c| c.bool("automatic")).unwrap_or(false),
                    delay: chunk.and_then(|c| c.f64("delay")).unwrap_or(0.0),
                })
            })
            .collect()
    }

    /// Format > Paste Animation: replace `drawable`'s builds with copies of `builds`
    /// (added at the end of the Build Order, with the same effects, options and start settings).
    pub fn paste_animation(&mut self, h: &mut History, slide: ObjId, drawable: ObjId, builds: &[CopiedBuild]) -> Result<(), kn_format::Error> {
        let old: Vec<ObjId> = self
            .file
            .obj(slide)
            .map(|s| s.ref_ids("builds"))
            .unwrap_or_default()
            .into_iter()
            .filter(|b| self.file.obj(*b).and_then(|m| m.ref_id("drawable")) == Some(drawable))
            .collect();
        for b in old {
            self.remove_build(h, slide, b);
        }
        for cb in builds {
            let id = self.add_build(h, slide, drawable, cb.kind, &cb.effect, cb.duration)?;
            self.touch(h, id);
            if let Some(m) = self.file.obj_mut(id) {
                m.remove("attributes");
                m.push_msg("attributes", cb.attrs.clone());
                if let Some(d) = &cb.delivery {
                    m.set_str("delivery", d);
                }
            }
            let chunk = self.file.obj(slide).and_then(|s| s.ref_ids("buildChunks").into_iter().find(|c| self.file.obj(*c).and_then(|m| m.ref_id("build")) == Some(id)));
            if let Some(c) = chunk {
                self.set_build_start(h, c, cb.automatic, cb.delay);
            }
        }
        Ok(())
    }

    /// Move Build Order step `from` to position `to` (the order of the slide's build chunks).
    pub fn move_build_step(&mut self, h: &mut History, slide: ObjId, from: usize, to: usize) -> bool {
        let mut ids = self.file.obj(slide).map(|s| s.ref_ids("buildChunks")).unwrap_or_default();
        if from >= ids.len() || to >= ids.len() || from == to {
            return false;
        }
        self.touch(h, slide);
        let c = ids.remove(from);
        ids.insert(to, c);
        let Some(a) = self.file.archive_mut(slide) else { return false };
        let s = a.obj_mut();
        s.remove("buildChunks");
        for id in ids {
            s.push_ref("buildChunks", id);
        }
        a.refresh_object_references();
        true
    }

    /// Change one effect option of a build.
    pub fn set_build_option(&mut self, h: &mut History, build: ObjId, opt: BuildOption) {
        self.touch(h, build);
        let Some(a) = self.file.obj_mut(build).map(|b| b.msg_mut_or_create("attributes")) else { return };
        match opt {
            BuildOption::Acceleration(v) => a.set_enum("action_acceleration", ["kNone", "kEaseIn", "kEaseOut", "kEaseBoth", "kCustom"][v.min(4) as usize]),
            BuildOption::RotationAngle(v) => a.set_f64("action_rotationAngle", v.clamp(0.0, 3600.0)),
            BuildOption::RotationClockwise(c) => a.set_enum("action_rotationDirection", if c { "kClockwise" } else { "kCounterclockwise" }),
            BuildOption::ScaleSize(v) => a.set_f64("action_scaleSize", v.clamp(0.01, 50.0)),
            BuildOption::Opacity(v) => a.set_f64("action_colorAlpha", v.clamp(0.0, 1.0)),
            BuildOption::Bounce(b) => a.set_bool("custom_bounce", b),
            BuildOption::Direction(d) => a.msg_mut_or_create("animationAttributes").set_u64("direction", u64::from(d)),
        }
    }

    /// The build's current delivery.
    pub fn build_delivery(&self, build: ObjId) -> Delivery {
        let Some(b) = self.file.obj(build) else { return Delivery::AllAtOnce };
        match (b.str("delivery"), b.msg("attributes").and_then(|a| a.enum_name("custom_textDelivery"))) {
            (Some("By Paragraph"), _) => Delivery::ByParagraph,
            (Some("By Bullet Group" | "By Paragraph Group"), _) => Delivery::ByParagraphGroup,
            (Some("By Highlighted Bullet" | "By Highlighted Paragraph"), _) => Delivery::ByHighlightedParagraph,
            (_, Some("kTextDeliveryByWord")) => Delivery::ByWord,
            (_, Some("kTextDeliveryByCharacter")) => Delivery::ByCharacter,
            _ => Delivery::AllAtOnce,
        }
    }

    /// Change a text build's delivery. By Paragraph gives the build one Build Order chunk per
    /// paragraph of its text (build_chunk_id 1…n, as Keynote does); the others use one chunk.
    pub fn set_build_delivery(&mut self, h: &mut History, slide: ObjId, build: ObjId, d: Delivery) -> bool {
        let Some(drawable) = self.file.obj(build).and_then(|b| b.ref_id("drawable")) else { return false };
        let paragraphs = self
            .file
            .obj(drawable)
            .and_then(|m| m.find_super("TSWP.ShapeInfoArchive"))
            .and_then(|i| i.ref_id("owned_storage"))
            .map(|st| self.storage_text(st).split(['\n', '\r', '\u{2029}']).filter(|p| !p.trim().is_empty()).count())
            .unwrap_or(1)
            .max(1);
        // paragraph groups: the top-level paragraphs
        let groups = {
            let slides = self.slides();
            slides
                .iter()
                .position(|s| s.slide == slide)
                .and_then(|i| self.scene(&slides, i))
                .and_then(|sc| sc.drawables.into_iter().find(|x| x.id == drawable))
                .and_then(|x| match x.kind {
                    crate::DrawableKind::Shape(s) => s.text,
                    _ => None,
                })
                .map(|t| t.paragraphs.iter().filter(|p| !t.text[p.start..p.end].trim().is_empty() && p.level == 0).count())
                .unwrap_or(paragraphs)
                .max(1)
        };
        let want = match d {
            Delivery::ByParagraph | Delivery::ByHighlightedParagraph => paragraphs,
            Delivery::ByParagraphGroup => groups,
            _ => 1,
        };
        let chunks: Vec<ObjId> = self
            .file
            .obj(slide)
            .map(|s| s.ref_ids("buildChunks"))
            .unwrap_or_default()
            .into_iter()
            .filter(|c| self.file.obj(*c).and_then(|m| m.ref_id("build")) == Some(build))
            .collect();
        let Some(&first) = chunks.first() else { return false };
        let file = match self.file.file_of(slide) {
            Some(f) => f.to_string(),
            None => return false,
        };
        self.structural(h, &[slide, build, first], |k| {
            if let Some(b) = k.obj_mut(build) {
                b.set_str(
                    "delivery",
                    match d {
                        Delivery::ByParagraph | Delivery::ByParagraphGroup | Delivery::ByHighlightedParagraph => d.stored(),
                        _ => "All at Once",
                    },
                );
                let seed = b.u64("chunk_id_seed").unwrap_or(1).max(want as u64);
                b.set_u64("chunk_id_seed", seed);
                let a = b.msg_mut_or_create("attributes");
                a.set_enum(
                    "custom_textDelivery",
                    match d {
                        Delivery::ByWord => "kTextDeliveryByWord",
                        Delivery::ByCharacter => "kTextDeliveryByCharacter",
                        _ => "kTextDeliveryByObject",
                    },
                );
                if a.enum_name("custom_deliveryOption").is_none() {
                    a.set_enum("custom_deliveryOption", "kDeliveryOptionForward");
                }
            }
            // the chunk list: keep the first, then add or drop paragraph chunks right after it
            let mut order = k.obj(slide).map(|s| s.ref_ids("buildChunks")).unwrap_or_default();
            order.retain(|c| *c == first || !chunks.contains(c));
            let at = order.iter().position(|c| *c == first).unwrap_or(0) + 1;
            let template = k.archive(first).cloned();
            if let Some(t) = template {
                let ids = k.alloc_ids(want as u64 - 1);
                for (j, id) in ids.iter().enumerate() {
                    let mut a = t.clone();
                    a.info.set_u64("identifier", *id);
                    if let Some(ci) = a.obj_mut().msg_mut("build_chunk_identifier") {
                        ci.set_u64("build_chunk_id", j as u64 + 2);
                    }
                    k.insert_archive(&file, a);
                    order.insert(at + j, *id);
                }
            }
            if let Some(c) = k.obj_mut(first).and_then(|m| m.msg_mut("build_chunk_identifier")) {
                c.set_u64("build_chunk_id", 1);
            }
            if let Some(sa) = k.archive_mut(slide) {
                let s = sa.obj_mut();
                s.remove("buildChunks");
                for c in order {
                    s.push_ref("buildChunks", c);
                }
                sa.refresh_object_references();
            }
        });
        true
    }

    /// Set when a Build Order step starts: on click, or automatically with/after the previous.
    /// Build Order Start: On Click, After the previous build, or With it (plus a delay).
    pub fn set_build_start_mode(&mut self, h: &mut History, chunk: ObjId, start: BuildStart, delay: f64) {
        self.touch(h, chunk);
        if let Some(c) = self.file.obj_mut(chunk) {
            c.set_bool("automatic", start != BuildStart::OnClick);
            c.set_bool("referent", start != BuildStart::WithPrevious);
            c.set_f64("delay", delay.max(0.0));
        }
    }

    pub fn set_build_start(&mut self, h: &mut History, chunk: ObjId, automatic: bool, delay: f64) {
        self.touch(h, chunk);
        if let Some(c) = self.file.obj_mut(chunk) {
            c.set_bool("automatic", automatic);
            c.set_f64("delay", delay);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DrawableKind;

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn add_change_remove_builds_survive_save() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/02-shape.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let shape = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| x.placeholder.is_none() && !x.from_template && matches!(x.kind, DrawableKind::Shape(_))).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Add Build");
        let b1 = d.add_build(&mut h, s[0].slide, shape.id, BuildKind::In, "apple:dissolve", 1.5).unwrap();
        let b2 = d.add_build(&mut h, s[0].slide, shape.id, BuildKind::Action, "apple:action-rotation", 1.0).unwrap();
        d.end(&mut h);
        let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        assert_eq!(sc.builds.len(), 2);
        assert_eq!(sc.build_order.len(), 2);
        assert_eq!(sc.builds[0].effect, "apple:dissolve");
        assert_eq!(sc.builds[0].kind, "In");
        assert_eq!(sc.builds[1].kind, "Action");
        assert!(d2.file.obj(d2.slides()[0].node).unwrap().bool("hasBuilds").unwrap());
        d.begin(&mut h, "Change");
        d.set_build_effect(&mut h, s[0].slide, b1, "apple:bc-pop", 0.75);
        d.remove_build(&mut h, s[0].slide, b2);
        d.end(&mut h);
        let sc = d.scene(&d.slides(), 0).unwrap();
        assert_eq!(sc.builds.len(), 1);
        assert_eq!(sc.builds[0].effect, "apple:bc-pop");
        assert_eq!(sc.build_order.len(), 1);
        assert!(d.undo(&mut h) && d.undo(&mut h));
        assert!(d.scene(&d.slides(), 0).unwrap().builds.is_empty());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn copy_and_paste_animation() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/Keynote-Feature-Catalog.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        // a slide where one object has builds and another has none
        let (i, from, to) = (0..s.len())
            .find_map(|i| {
                let sc = d.scene(&s, i)?;
                let own: Vec<ObjId> = sc.drawables.iter().filter(|x| !x.from_template).map(|x| x.id).collect();
                let from = *own.iter().find(|x| sc.builds.iter().any(|b| b.drawable == **x))?;
                let to = *own.iter().find(|x| !sc.builds.iter().any(|b| b.drawable == **x))?;
                Some((i, from, to))
            })
            .unwrap();
        let copied = d.copy_animation(s[i].slide, from);
        assert!(!copied.is_empty());
        let mut h = History::default();
        d.begin(&mut h, "Paste Animation");
        d.paste_animation(&mut h, s[i].slide, to, &copied).unwrap();
        d.end(&mut h);
        let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), i).unwrap();
        let effects = |id: ObjId| sc.builds.iter().filter(|b| b.drawable == id).map(|b| (b.kind.clone(), b.effect.clone())).collect::<Vec<_>>();
        assert_eq!(effects(from), effects(to));
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn delivery_by_paragraph_matches_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("R6-dissolve-all-at-once.key")).unwrap();
        let k = Document::open(dir.join("R6-dissolve-by-paragraph.key")).unwrap();
        let s = d.slides();
        let build = d.file.obj(s[0].slide).unwrap().ref_ids("builds")[0];
        let mut h = History::default();
        d.begin(&mut h, "Delivery");
        assert!(d.set_build_delivery(&mut h, s[0].slide, build, Delivery::ByParagraph));
        d.end(&mut h);
        let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        // same as Keynote's: delivery string, and one chunk per paragraph numbered 1..n
        let shape = |doc: &Document| {
            let sl = doc.slides()[0].slide;
            let b = doc.file.obj(sl).unwrap().ref_ids("builds")[0];
            let chunks: Vec<u64> = doc
                .file
                .obj(sl)
                .unwrap()
                .ref_ids("buildChunks")
                .into_iter()
                .filter_map(|c| doc.file.obj(c)?.msg("build_chunk_identifier")?.u64("build_chunk_id"))
                .collect();
            (doc.file.obj(b).unwrap().str("delivery").map(str::to_string), chunks, doc.build_delivery(b))
        };
        assert_eq!(shape(&d2), shape(&k));
        assert_eq!(shape(&d2).2, Delivery::ByParagraph);
        // and back
        d.begin(&mut h, "Delivery");
        assert!(d.set_build_delivery(&mut h, s[0].slide, build, Delivery::ByWord));
        d.end(&mut h);
        assert_eq!(d.file.obj(s[0].slide).unwrap().ref_ids("buildChunks").len(), 1);
        assert_eq!(d.build_delivery(build), Delivery::ByWord);
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn group_and_highlighted_delivery_survive_save() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref");
        let mut d = Document::open(dir.join("R6-dissolve-all-at-once.key")).unwrap();
        let s = d.slides();
        let build = d.file.obj(s[0].slide).unwrap().ref_ids("builds")[0];
        let mut h = History::default();
        for dv in [Delivery::ByParagraphGroup, Delivery::ByHighlightedParagraph] {
            d.begin(&mut h, "Delivery");
            assert!(d.set_build_delivery(&mut h, s[0].slide, build, dv));
            d.end(&mut h);
            let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
            assert_eq!(d2.build_delivery(build), dv);
            assert_eq!(d2.file.obj(build).unwrap().str("delivery"), Some(dv.stored()));
            assert!(!d2.file.obj(s[0].slide).unwrap().ref_ids("buildChunks").is_empty());
        }
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn effect_options_survive_save() {
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/02-shape.key");
        let mut d = Document::open(p).unwrap();
        let s = d.slides();
        let shape = d.scene(&s, 0).unwrap().drawables.into_iter().find(|x| !x.from_template).unwrap().id;
        let mut h = History::default();
        d.begin(&mut h, "Add");
        let rot = d.add_build(&mut h, s[0].slide, shape, BuildKind::Action, "apple:action-rotation", 1.0).unwrap();
        let mv = d.add_build(&mut h, s[0].slide, shape, BuildKind::In, "apple:move in", 1.0).unwrap();
        d.set_build_option(&mut h, rot, BuildOption::RotationAngle(180.0));
        d.set_build_option(&mut h, rot, BuildOption::RotationClockwise(true));
        d.set_build_option(&mut h, rot, BuildOption::Acceleration(2));
        d.set_build_option(&mut h, mv, BuildOption::Direction(13));
        d.set_build_option(&mut h, mv, BuildOption::Bounce(true));
        d.end(&mut h);
        let d2 = Document::from_file(KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let sc = d2.scene(&d2.slides(), 0).unwrap();
        let r = sc.builds.iter().find(|b| b.id == rot).unwrap();
        assert_eq!((r.params.rotation_angle, r.params.rotation_clockwise, r.params.acceleration), (Some(180.0), true, 2));
        let m = sc.builds.iter().find(|b| b.id == mv).unwrap();
        assert_eq!((m.direction, m.params.bounce), (13, true));
    }
}

#[cfg(test)]
mod web_video_tests {
    use crate::edit::History;
    use crate::Document;

    /// R37: a web video's build and flags are what Keynote writes.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn web_video_like_keynote() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/r37");
        let theirs = Document::open(dir.join("R37-1-webvideo.key")).unwrap();
        let mut d = Document::open(dir.join("R37-0-before.key")).unwrap();
        let s = d.slides()[0].slide;
        let poster = std::fs::read(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/kn-app/data/icons/keydeck-256.png")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Web Video");
        let id = d.insert(&mut h, s, &crate::insert::NewObject::Image(poster, "hqdefault.jpg".into())).unwrap();
        assert!(d.make_web_video(&mut h, id, "https://www.youtube.com/watch?v=jNQXAC9IVRw", "Me at the zoo", "jawed"));
        d.add_web_video_build(&mut h, s, id).unwrap();
        d.end(&mut h);
        let d = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let summary = |d: &Document| {
            let sl = d.slides()[0].slide;
            let b = d.file.obj(sl).unwrap().ref_ids("builds")[0];
            let m = d.file.obj(b).unwrap();
            let aa = m.msg("attributes").unwrap().msg("animationAttributes").unwrap();
            let node = d.slides()[0].node;
            (
                aa.str("effect").map(str::to_string),
                aa.str("animation_type").map(str::to_string),
                m.str("delivery").map(str::to_string),
                d.file.obj(sl).unwrap().ref_ids("buildChunks").len(),
                d.file.obj(node).unwrap().bool("has_explicit_builds"),
            )
        };
        assert_eq!(summary(&d), summary(&theirs));
        let sl = d.slides()[0].slide;
        assert!(d.web_video_build(sl, id).is_some());
    }
}
