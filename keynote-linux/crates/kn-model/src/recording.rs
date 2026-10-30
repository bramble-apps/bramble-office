//! Play > Record Slideshow / Play Recorded Slideshow / Clear Recording, stored as Keynote 15.4
//! does (Mac round 21, K-recording.key): ShowArchive.recording -> KN.RecordingArchive with three
//! event tracks (navigation, movie, pause), a movie track whose one segment is the narration
//! (Data/MovieRecording-<UUID>.mov, AAC) and the duration. Navigation events name the slide node
//! (reference and UUID), the build event reached (0 = the slide itself) and the phase: None for
//! the first slide, Start / End around each transition or build.

use kn_format::{Archive, Msg, ObjId};

use crate::edit::History;
use crate::Document;

const NAV: &str = "com.apple.Keynote.recordingEventTrack-navigation";
const MOVIE: &str = "com.apple.iwork.Keynote.recordingEventTrack-movie";
const PAUSE: &str = "com.apple.iwork.Keynote.recordingEventTrack-pause";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    None,
    Start,
    End,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::None => "kRecordingNavigationEventAnimationNone",
            Phase::Start => "kRecordingNavigationEventAnimationStart",
            Phase::End => "kRecordingNavigationEventAnimationEnd",
        }
    }
}

/// One navigation of the recorded show.
#[derive(Clone, Debug, PartialEq)]
pub struct NavEvent {
    pub time: f64,
    pub slide_node: ObjId,
    /// The build event reached on that slide (0 = the slide shown, before its builds).
    pub event: u32,
    pub phase: Phase,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub duration: f64,
    pub nav: Vec<NavEvent>,
    /// The narration (Data/MovieRecording-….mov).
    pub audio: Option<u64>,
}

impl Document {
    fn show_id(&self) -> Option<ObjId> {
        self.file.first_of("KN.ShowArchive").map(|(id, _)| id)
    }

    /// The presentation's recorded slideshow, if it has one.
    pub fn recording(&self) -> Option<Recording> {
        let rec = self.file.obj(self.show()?.1.ref_id("recording")?)?;
        let mut nav = Vec::new();
        for t in rec.ref_ids("event_tracks") {
            let Some(tr) = self.file.obj(t) else { continue };
            if tr.str("type") != Some(NAV) {
                continue;
            }
            for e in tr.msgs("events") {
                let Some(n) = e.msg("navigation_event_archive") else { continue };
                let Some(node) = n.ref_id("target_slide_node_reference") else { continue };
                nav.push(NavEvent {
                    time: e.f64("start_time").unwrap_or(0.0),
                    slide_node: node,
                    event: n.u32("target_event_index").unwrap_or(0),
                    phase: match n.enum_name("animation_phase") {
                        Some("kRecordingNavigationEventAnimationStart") => Phase::Start,
                        Some("kRecordingNavigationEventAnimationEnd") => Phase::End,
                        _ => Phase::None,
                    },
                });
            }
        }
        nav.sort_by(|a, b| a.time.total_cmp(&b.time));
        let audio = rec
            .ref_id("movie_track")
            .and_then(|m| self.file.obj(m))
            .and_then(|m| m.msgs("movie_segments").next().and_then(|s| s.msg("movie_data")).and_then(|d| d.u64("identifier")));
        Some(Recording { duration: rec.f64("duration").unwrap_or(0.0), nav, audio })
    }

    /// Play > Clear Recording: the recording objects and the narration go (one undo step).
    pub fn clear_recording(&mut self, h: &mut History) -> bool {
        let Some(show) = self.show_id() else { return false };
        let Some(rec) = self.file.obj(show).and_then(|m| m.ref_id("recording")) else { return false };
        let audio = self.recording().and_then(|r| r.audio);
        self.touch_all(h);
        let mut gone = vec![rec];
        if let Some(m) = self.file.obj(rec) {
            gone.extend(m.ref_ids("event_tracks"));
            gone.extend(m.ref_id("movie_track"));
        }
        for id in gone {
            self.file.remove_archive(id);
        }
        if let Some(a) = self.file.archive_mut(show) {
            a.obj_mut().remove("recording");
            a.refresh_object_references();
        }
        if let Some(d) = audio {
            self.file.remove_data(d);
        }
        true
    }

    /// Play > Record Slideshow: store a recording (replacing one there was) with its narration
    /// (AAC in a .mov) and navigation events.
    pub fn set_recording(&mut self, h: &mut History, nav: &[NavEvent], audio_mov: Vec<u8>, duration: f64) -> Result<(), kn_format::Error> {
        let err = |s: &str| kn_format::Error::Invalid(s.into());
        if self.recording().is_some() {
            self.clear_recording(h);
        }
        let show = self.show_id().ok_or(err("no show"))?;
        let file = self.file.file_of(show).ok_or(err("no show file"))?.to_string();
        // the slide nodes' UUIDs (Keynote names them by both reference and UUID)
        let nodes: Vec<ObjId> = nav.iter().map(|e| e.slide_node).collect();
        for n in &nodes {
            if self.file.object_uuid_msg(*n).is_none() {
                self.file.add_uuid_entry(&file, *n);
            }
        }
        let uuid = |k: &kn_format::KeyFile, n: ObjId| k.object_uuid_msg(n);
        self.structural(h, &[show], |k| -> Result<(), kn_format::Error> {
            let name = format!("MovieRecording-{}.mov", uuid::Uuid::new_v4().to_string().to_uppercase());
            let data = k.add_file_data(audio_mov, &name)?;
            let ids = k.alloc_ids(5);
            let (rec_id, nav_id, movie_id, pause_id, track_id) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
            let mut navt = Msg::new("KN.RecordingEventTrackArchive");
            navt.set_str("type", NAV);
            for e in nav {
                let ev = navt.push_new("events");
                ev.set_f64("start_time", e.time);
                let n = ev.msg_mut_or_create("navigation_event_archive");
                n.set_ref("target_slide_node_reference", e.slide_node);
                n.set_u64("target_event_index", u64::from(e.event));
                n.set_enum("animation_phase", e.phase.name());
                if let Some(u) = uuid(k, e.slide_node) {
                    let t = n.msg_mut_or_create("target_slide_node_id");
                    t.set_u64("lower", u.u64("lower").unwrap_or(0));
                    t.set_u64("upper", u.u64("upper").unwrap_or(0));
                }
            }
            let mut movt = Msg::new("KN.RecordingEventTrackArchive");
            movt.set_str("type", MOVIE);
            let mut pauset = Msg::new("KN.RecordingEventTrackArchive");
            pauset.set_str("type", PAUSE);
            let ev = pauset.push_new("events");
            ev.set_f64("start_time", 0.0);
            ev.msg_mut_or_create("pause_event_archive").set_enum("pause_event_type", "kRecordingPauseEventTypeResume");
            let mut track = Msg::new("KN.RecordingMovieTrackArchive");
            let seg = track.push_new("movie_segments");
            seg.set_f64("start_time", 0.0);
            seg.set_ref("movie_data", data);
            track.set_bool("is_movie_segment_data_trimmed_to_duration", true);
            let mut rec = Msg::new("KN.RecordingArchive");
            for t in [nav_id, movie_id, pause_id] {
                rec.push_ref("event_tracks", t);
            }
            rec.set_ref("movie_track", track_id);
            rec.set_f64("duration", duration);
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) - 978_307_200.0;
            rec.msg_mut_or_create("modification_date").set_f64("seconds", now);
            rec.msg_mut_or_create("sync_state").set_bool("can_clear_out_of_sync_token", true);
            rec.msg_mut_or_create("correction_history").set_bool("did_add_missing_events_for_radar_49654305", false);
            for (id, m) in [(nav_id, navt), (movie_id, movt), (pause_id, pauset), (track_id, track), (rec_id, rec)] {
                let mut a = Archive::new_object(id, m);
                a.refresh_object_references();
                k.insert_archive(&file, a);
            }
            k.add_data_reference(&file, data, track_id);
            if let Some(a) = k.archive_mut(show) {
                a.obj_mut().set_ref("recording", rec_id);
                a.refresh_object_references();
            }
            k.ensure_external_refs(&file);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/ref")
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn reads_keynotes_recording() {
        let d = Document::open(refs().join("K-recording.key")).unwrap();
        let r = d.recording().unwrap();
        assert!((r.duration - 26.746).abs() < 0.01);
        assert_eq!(r.nav.len(), 3);
        assert_eq!(r.nav[0].phase, Phase::None);
        assert_eq!((r.nav[1].phase, r.nav[2].phase), (Phase::Start, Phase::End));
        assert_eq!(r.audio, Some(444));
        assert!(Document::open(refs().join("K-recording-0-before.key")).unwrap().recording().is_none());
    }

    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn clear_matches_keynote_and_undoes() {
        let mut d = Document::open(refs().join("K-recording.key")).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Clear Recording");
        assert!(d.clear_recording(&mut h));
        d.end(&mut h);
        assert!(d.recording().is_none());
        assert!(!d.file.entries.iter().any(|e| e.name.contains("MovieRecording")));
        assert!(d.file.first_of("KN.RecordingArchive").is_none());
        d.undo(&mut h);
        assert!(d.recording().is_some());
    }

    /// A recording we write reads back the same, in Keynote's shape.
    #[cfg_attr(not(private_testdata), ignore = "needs the private test files")]
    #[test]
    fn write_read_back() {
        let mut d = Document::open(refs().join("K-recording-0-before.key")).unwrap();
        let s = d.slides();
        let nav = vec![
            NavEvent { time: 0.0, slide_node: s[0].node, event: 0, phase: Phase::None },
            NavEvent { time: 2.0, slide_node: s[1].node, event: 0, phase: Phase::Start },
            NavEvent { time: 2.5, slide_node: s[1].node, event: 0, phase: Phase::End },
        ];
        let k = Document::open(refs().join("K-recording.key")).unwrap();
        let mov = k.file.data_entry(444).map(|e| match &e.data {
            kn_format::EntryData::Raw(b) => b.to_vec(),
            _ => Vec::new(),
        }).unwrap();
        let mut h = History::default();
        d.begin(&mut h, "Record Slideshow");
        d.set_recording(&mut h, &nav, mov, 5.0).unwrap();
        d.end(&mut h);
        let d2 = Document::from_file(kn_format::KeyFile::from_zip(&d.file.to_zip().unwrap()).unwrap());
        let r = d2.recording().unwrap();
        assert_eq!(r.nav, nav);
        assert_eq!(r.duration, 5.0);
        assert!(r.audio.is_some());
        // same objects as Keynote's
        let types = |doc: &Document| {
            let mut t: Vec<String> = ["KN.RecordingArchive", "KN.RecordingEventTrackArchive", "KN.RecordingMovieTrackArchive"].iter().map(|n| format!("{n}:{}", doc.file.objects_of(n).count())).collect();
            t.sort();
            t
        };
        assert_eq!(types(&d2), types(&k));
    }
}
