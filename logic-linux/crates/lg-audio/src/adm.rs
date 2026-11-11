//! ADM BWF masters (Audio Definition Model in a Broadcast Wave file, ITU-R BS.2076 / BS.2088):
//! the bed's channels and every object's audio interleaved in one WAV, with a `chna` chunk
//! naming each track and an `axml` chunk describing beds (DirectSpeakers with their positions)
//! and objects (Objects with timed position blocks: Dolby's cartesian room coordinates), plus
//! the project's downmix and trim settings. Written and read here from scratch (no SDK).
//!
//! Track order: the bed channels first (in the bed's channel order), then the objects, a stereo
//! object as two mono ones. ADM's azimuth is positive to the left; positions given here use
//! Studio's (positive right) and are flipped on the way in and out.

use std::fmt::Write as _;
use std::io;
use std::path::Path;

use crate::file::AudioFile;

/// One bed channel: Logic's label ("L", "Ltm", "LFE"), azimuth (degrees, positive right),
/// elevation, and whether it is the LFE.
#[derive(Clone, Debug, PartialEq)]
pub struct BedChannel {
    pub label: String,
    pub azimuth: f32,
    pub elevation: f32,
}

/// One position block of an object: from `start` for `duration` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub start: f64,
    pub duration: f64,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdmObject {
    pub name: String,
    pub blocks: Vec<Block>,
    pub audio: Vec<f32>,
}

/// A whole master.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Adm {
    pub rate: u32,
    pub programme: String,
    pub bed: Vec<BedChannel>,
    /// One buffer per bed channel.
    pub bed_audio: Vec<Vec<f32>>,
    pub objects: Vec<AdmObject>,
    /// Downmix and trim settings (and anything else to keep), as (key, value).
    pub meta: Vec<(String, String)>,
}

/// BS.2051 speaker label for one of Logic's channels.
fn speaker_label(label: &str, az: f32, el: f32) -> String {
    if label == "LFE" {
        return "LFE1".into();
    }
    let layer = if el >= 30.0 { 'U' } else { 'M' };
    // (ADM azimuth: positive to the left)
    let a = -az.round() as i32;
    let sign = if a < 0 { '-' } else { '+' };
    format!("{layer}{sign}{:03}", a.abs())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn time(t: f64) -> String {
    let t = t.max(0.0);
    let h = (t / 3600.0).floor();
    let m = ((t - h * 3600.0) / 60.0).floor();
    let s = t - h * 3600.0 - m * 60.0;
    // samples-exact enough: five decimals, as BS.2076's hh:mm:ss.fffff
    format!("{:02}:{:02}:{:08.5}", h as u32, m as u32, s)
}

fn parse_time(s: &str) -> f64 {
    let p: Vec<&str> = s.trim().split(':').collect();
    if p.len() != 3 {
        return 0.0;
    }
    let h: f64 = p[0].parse().unwrap_or(0.0);
    let m: f64 = p[1].parse().unwrap_or(0.0);
    // (BS.2076-2 also allows "ss.fffffSnnnn" sample counts; the fraction part is what we write)
    let sec: f64 = p[2].split('S').next().unwrap_or("0").parse().unwrap_or(0.0);
    h * 3600.0 + m * 60.0 + sec
}

impl Adm {
    pub fn frames(&self) -> usize {
        self.bed_audio.iter().map(|c| c.len()).chain(self.objects.iter().map(|o| o.audio.len())).max().unwrap_or(0)
    }

    pub fn tracks(&self) -> usize {
        self.bed.len() + self.objects.len()
    }

    /// The `axml` chunk's document.
    pub fn xml(&self) -> String {
        let dur = time(self.frames() as f64 / self.rate.max(1) as f64);
        let mut x = String::new();
        let _ = writeln!(x, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        let _ = writeln!(x, r#"<ebuCoreMain xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns="urn:ebu:metadata-schema:ebuCore_2016" schema="EBU_CORE_20161222.xsd" xml:lang="en">"#);
        let _ = writeln!(x, "<coreMetadata><format><audioFormatExtended version=\"ITU-R_BS.2076-2\">");
        let _ = writeln!(x, r#"<audioProgramme audioProgrammeID="APR_1001" audioProgrammeName="{}" start="00:00:00.00000" end="{dur}">"#, esc(&self.programme));
        let n_content = (!self.bed.is_empty()) as usize + self.objects.len();
        for k in 0..n_content {
            let _ = writeln!(x, "<audioContentIDRef>ACO_{:04}</audioContentIDRef>", 1001 + k);
        }
        let _ = writeln!(x, "</audioProgramme>");
        let mut content = 0usize;
        let mut track = 1usize;
        let mut chna: Vec<(usize, String, String, String)> = Vec::new();
        if !self.bed.is_empty() {
            let _ = writeln!(x, r#"<audioContent audioContentID="ACO_{:04}" audioContentName="Bed"><audioObjectIDRef>AO_{:04}</audioObjectIDRef></audioContent>"#, 1001 + content, 1001 + content);
            let _ = write!(x, r#"<audioObject audioObjectID="AO_{:04}" audioObjectName="Bed" start="00:00:00.00000" duration="{dur}"><audioPackFormatIDRef>AP_00011001</audioPackFormatIDRef>"#, 1001 + content);
            for k in 0..self.bed.len() {
                let _ = write!(x, "<audioTrackUIDRef>ATU_{:08}</audioTrackUIDRef>", track + k);
            }
            let _ = writeln!(x, "</audioObject>");
            let _ = write!(x, r#"<audioPackFormat audioPackFormatID="AP_00011001" audioPackFormatName="Bed {}" typeLabel="0001" typeDefinition="DirectSpeakers">"#, self.bed.len());
            for k in 0..self.bed.len() {
                let _ = write!(x, "<audioChannelFormatIDRef>AC_{:08X}</audioChannelFormatIDRef>", 0x0001_1001 + k);
            }
            let _ = writeln!(x, "</audioPackFormat>");
            for (k, b) in self.bed.iter().enumerate() {
                let id = 0x0001_1001 + k;
                let _ = writeln!(
                    x,
                    r#"<audioChannelFormat audioChannelFormatID="AC_{id:08X}" audioChannelFormatName="{}" typeLabel="0001" typeDefinition="DirectSpeakers"><audioBlockFormat audioBlockFormatID="AB_{id:08X}_00000001"><speakerLabel>{}</speakerLabel><position coordinate="azimuth">{:.1}</position><position coordinate="elevation">{:.1}</position><position coordinate="distance">1.0</position></audioBlockFormat></audioChannelFormat>"#,
                    esc(&b.label),
                    speaker_label(&b.label, b.azimuth, b.elevation),
                    -b.azimuth,
                    b.elevation
                );
                let _ = writeln!(x, r#"<audioStreamFormat audioStreamFormatID="AS_{id:08X}" audioStreamFormatName="PCM_{}" formatLabel="0001" formatDefinition="PCM"><audioChannelFormatIDRef>AC_{id:08X}</audioChannelFormatIDRef><audioTrackFormatIDRef>AT_{id:08X}_01</audioTrackFormatIDRef></audioStreamFormat>"#, esc(&b.label));
                let _ = writeln!(x, r#"<audioTrackFormat audioTrackFormatID="AT_{id:08X}_01" audioTrackFormatName="PCM_{}" formatLabel="0001" formatDefinition="PCM"><audioStreamFormatIDRef>AS_{id:08X}</audioStreamFormatIDRef></audioTrackFormat>"#, esc(&b.label));
                chna.push((track, format!("ATU_{:08}", track), format!("AT_{id:08X}_01"), "AP_00011001".into()));
                let _ = writeln!(x, r#"<audioTrackUID UID="ATU_{:08}" sampleRate="{}" bitDepth="24"><audioTrackFormatIDRef>AT_{id:08X}_01</audioTrackFormatIDRef><audioPackFormatIDRef>AP_00011001</audioPackFormatIDRef></audioTrackUID>"#, track, self.rate);
                track += 1;
            }
            content += 1;
        }
        for (k, o) in self.objects.iter().enumerate() {
            let id = 0x0003_1001 + k;
            let pack = format!("AP_{id:08X}");
            let _ = writeln!(x, r#"<audioContent audioContentID="ACO_{:04}" audioContentName="{}"><audioObjectIDRef>AO_{:04}</audioObjectIDRef></audioContent>"#, 1001 + content, esc(&o.name), 1001 + content);
            let _ = writeln!(x, r#"<audioObject audioObjectID="AO_{:04}" audioObjectName="{}" start="00:00:00.00000" duration="{dur}"><audioPackFormatIDRef>{pack}</audioPackFormatIDRef><audioTrackUIDRef>ATU_{:08}</audioTrackUIDRef></audioObject>"#, 1001 + content, esc(&o.name), track);
            let _ = writeln!(x, r#"<audioPackFormat audioPackFormatID="{pack}" audioPackFormatName="{}" typeLabel="0003" typeDefinition="Objects"><audioChannelFormatIDRef>AC_{id:08X}</audioChannelFormatIDRef></audioPackFormat>"#, esc(&o.name));
            let _ = write!(x, r#"<audioChannelFormat audioChannelFormatID="AC_{id:08X}" audioChannelFormatName="{}" typeLabel="0003" typeDefinition="Objects">"#, esc(&o.name));
            for (b, blk) in o.blocks.iter().enumerate() {
                let _ = write!(
                    x,
                    r#"<audioBlockFormat audioBlockFormatID="AB_{id:08X}_{:08X}" rtime="{}" duration="{}"><cartesian>1</cartesian><position coordinate="X">{:.4}</position><position coordinate="Y">{:.4}</position><position coordinate="Z">{:.4}</position><width>{:.4}</width><depth>{:.4}</depth><height>{:.4}</height><jumpPosition>0</jumpPosition></audioBlockFormat>"#,
                    b + 1,
                    time(blk.start),
                    time(blk.duration),
                    blk.x,
                    blk.y,
                    blk.z,
                    blk.size,
                    blk.size,
                    blk.size
                );
            }
            let _ = writeln!(x, "</audioChannelFormat>");
            let _ = writeln!(x, r#"<audioStreamFormat audioStreamFormatID="AS_{id:08X}" audioStreamFormatName="PCM_{}" formatLabel="0001" formatDefinition="PCM"><audioChannelFormatIDRef>AC_{id:08X}</audioChannelFormatIDRef><audioTrackFormatIDRef>AT_{id:08X}_01</audioTrackFormatIDRef></audioStreamFormat>"#, esc(&o.name));
            let _ = writeln!(x, r#"<audioTrackFormat audioTrackFormatID="AT_{id:08X}_01" audioTrackFormatName="PCM_{}" formatLabel="0001" formatDefinition="PCM"><audioStreamFormatIDRef>AS_{id:08X}</audioStreamFormatIDRef></audioTrackFormat>"#, esc(&o.name));
            let _ = writeln!(x, r#"<audioTrackUID UID="ATU_{:08}" sampleRate="{}" bitDepth="24"><audioTrackFormatIDRef>AT_{id:08X}_01</audioTrackFormatIDRef><audioPackFormatIDRef>{pack}</audioPackFormatIDRef></audioTrackUID>"#, track, self.rate);
            chna.push((track, format!("ATU_{:08}", track), format!("AT_{id:08X}_01"), pack));
            track += 1;
            content += 1;
        }
        let _ = writeln!(x, "</audioFormatExtended>");
        // the downmix and trim settings (Studio's own element; other readers skip it)
        if !self.meta.is_empty() {
            let _ = write!(x, "<bramble:renderSettings xmlns:bramble=\"urn:bramble-studio:adm\">");
            for (k, v) in &self.meta {
                let _ = write!(x, r#"<bramble:setting name="{}" value="{}"/>"#, esc(k), esc(v));
            }
            let _ = writeln!(x, "</bramble:renderSettings>");
        }
        let _ = writeln!(x, "</format></coreMetadata></ebuCoreMain>");
        x
    }

    /// The `chna` chunk: each track's index, UID, track format and pack format.
    pub fn chna(&self) -> Vec<u8> {
        let n = self.tracks();
        let mut b = Vec::new();
        b.extend_from_slice(&(n as u16).to_le_bytes());
        b.extend_from_slice(&(n as u16).to_le_bytes());
        let mut track = 1usize;
        let push = |b: &mut Vec<u8>, track: usize, at: String, ap: String| {
            b.extend_from_slice(&(track as u16).to_le_bytes());
            b.extend_from_slice(format!("ATU_{track:08}").as_bytes());
            let mut at = at.into_bytes();
            at.resize(14, 0);
            b.extend_from_slice(&at);
            let mut ap = ap.into_bytes();
            ap.resize(11, 0);
            b.extend_from_slice(&ap);
            b.push(0);
        };
        for k in 0..self.bed.len() {
            push(&mut b, track, format!("AT_{:08X}_01", 0x0001_1001 + k), "AP_00011001".into());
            track += 1;
        }
        for k in 0..self.objects.len() {
            push(&mut b, track, format!("AT_{:08X}_01", 0x0003_1001 + k), format!("AP_{:08X}", 0x0003_1001 + k));
            track += 1;
        }
        b
    }

    /// The whole file: 24-bit interleaved PCM, bed then objects, with `chna` and `axml`.
    pub fn file(&self) -> AudioFile {
        let n = self.frames();
        let tracks = self.tracks();
        let mut data = vec![0f32; n * tracks];
        let cols: Vec<&[f32]> = self.bed_audio.iter().map(|c| c.as_slice()).chain(self.objects.iter().map(|o| o.audio.as_slice())).collect();
        for (t, c) in cols.iter().enumerate() {
            for (f, x) in c.iter().enumerate() {
                data[f * tracks + t] = *x;
            }
        }
        let mut af = AudioFile::new(self.rate, tracks.max(1), data);
        af.bits = 24;
        af.extra.push((*b"chna", self.chna()));
        af.extra.push((*b"axml", self.xml().into_bytes()));
        af
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        self.file().write(path)
    }

    pub fn read(path: &Path) -> io::Result<Adm> {
        Adm::parse(&std::fs::read(path)?)
    }

    /// Reads a master: tracks by the `axml` document (beds by their channel formats, objects
    /// with their blocks) in track order.
    pub fn parse(b: &[u8]) -> io::Result<Adm> {
        let af = AudioFile::parse(b)?;
        let axml = af.extra.iter().find(|(id, _)| id == b"axml").map(|(_, d)| String::from_utf8_lossy(d).to_string()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no ADM (axml) metadata: not an ADM BWF file"))?;
        let chans = af.channels;
        let frames = af.frames();
        let column = |t: usize| -> Vec<f32> { (0..frames).map(|f| af.data[f * chans + t]).collect() };
        let mut adm = Adm { rate: af.rate, ..Default::default() };
        adm.programme = attr(&axml, "audioProgramme", "audioProgrammeName").unwrap_or_default();
        // track UID → track format, in order
        let uids: Vec<(String, String)> = elements(&axml, "audioTrackUID").into_iter().map(|e| (attr_in(&e, "UID").unwrap_or_default(), inner(&e, "audioTrackFormatIDRef").unwrap_or_default())).collect();
        // (the chna chunk's order, when there is one, decides track positions)
        let order: Vec<(usize, String)> = af.extra.iter().find(|(id, _)| id == b"chna").map(|(_, d)| parse_chna(d)).unwrap_or_else(|| uids.iter().enumerate().map(|(k, u)| (k + 1, u.0.clone())).collect());
        let channel_formats = elements(&axml, "audioChannelFormat");
        for (track, uid) in order {
            let Some((_, at)) = uids.iter().find(|u| u.0 == uid) else { continue };
            // AT_xxxxxxxx_01 → AC_xxxxxxxx
            let ac = format!("AC_{}", at.trim_start_matches("AT_").split('_').next().unwrap_or(""));
            let Some(cf) = channel_formats.iter().find(|c| attr_in(c, "audioChannelFormatID").as_deref() == Some(ac.as_str())) else { continue };
            let name = attr_in(cf, "audioChannelFormatName").unwrap_or_default();
            let audio = if track >= 1 && track <= chans { column(track - 1) } else { vec![0.0; frames] };
            if attr_in(cf, "typeDefinition").as_deref() == Some("Objects") || attr_in(cf, "typeLabel").as_deref() == Some("0003") {
                let mut blocks = Vec::new();
                for blk in elements(cf, "audioBlockFormat") {
                    let pos = |c: &str| positions(&blk).into_iter().find(|p| p.0.eq_ignore_ascii_case(c)).map(|p| p.1);
                    blocks.push(Block {
                        start: attr_in(&blk, "rtime").map(|t| parse_time(&t)).unwrap_or(0.0),
                        duration: attr_in(&blk, "duration").map(|t| parse_time(&t)).unwrap_or(0.0),
                        x: pos("X").unwrap_or(0.0),
                        y: pos("Y").unwrap_or(1.0),
                        z: pos("Z").unwrap_or(0.0),
                        size: inner(&blk, "width").and_then(|v| v.trim().parse().ok()).unwrap_or(0.0),
                    });
                }
                // the object's name: its audioObject's, else the channel format's
                let oname = elements(&axml, "audioObject").into_iter().find(|o| o.contains(&format!(">{uid}<"))).and_then(|o| attr_in(&o, "audioObjectName")).unwrap_or(name);
                adm.objects.push(AdmObject { name: oname, blocks, audio });
            } else {
                let blk = elements(cf, "audioBlockFormat").into_iter().next().unwrap_or_default();
                let pos = |c: &str| positions(&blk).into_iter().find(|p| p.0.eq_ignore_ascii_case(c)).map(|p| p.1);
                let label = if name.is_empty() { inner(&blk, "speakerLabel").unwrap_or_default() } else { name };
                adm.bed.push(BedChannel { label, azimuth: -pos("azimuth").unwrap_or(0.0), elevation: pos("elevation").unwrap_or(0.0) });
                adm.bed_audio.push(audio);
            }
        }
        for e in elements(&axml, "bramble:setting") {
            if let (Some(k), Some(v)) = (attr_in(&e, "name"), attr_in(&e, "value")) {
                adm.meta.push((k, v));
            }
        }
        Ok(adm)
    }
}

fn unesc(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

fn parse_chna(d: &[u8]) -> Vec<(usize, String)> {
    let mut v = Vec::new();
    if d.len() < 4 {
        return v;
    }
    let n = u16::from_le_bytes([d[2], d[3]]) as usize;
    for k in 0..n {
        let o = 4 + k * 40;
        if o + 14 > d.len() {
            break;
        }
        let track = u16::from_le_bytes([d[o], d[o + 1]]) as usize;
        let uid = String::from_utf8_lossy(&d[o + 2..o + 14]).trim_end_matches('\0').to_string();
        if track > 0 {
            v.push((track, uid));
        }
    }
    v
}

/// Every `<tag …>…</tag>` (or `<tag …/>`) in `s`, whole.
fn elements(s: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut i = 0;
    while let Some(a) = s[i..].find(&open).map(|a| a + i) {
        let after = s[a + open.len()..].chars().next().unwrap_or(' ');
        if !(after == ' ' || after == '>' || after == '/' || after == '\n') {
            i = a + open.len();
            continue;
        }
        let gt = s[a..].find('>').map_or(s.len(), |g| a + g);
        if s[..gt].ends_with('/') {
            out.push(s[a..=gt].to_string());
            i = gt + 1;
            continue;
        }
        match s[gt..].find(&close) {
            Some(c) => {
                let end = gt + c + close.len();
                out.push(s[a..end].to_string());
                i = end;
            }
            None => break,
        }
    }
    out
}

/// An attribute of the element's opening tag.
fn attr_in(e: &str, name: &str) -> Option<String> {
    let head = &e[..e.find('>').unwrap_or(e.len())];
    let key = format!(" {name}=\"");
    let a = head.find(&key)? + key.len();
    let b = head[a..].find('"')? + a;
    Some(unesc(&head[a..b]))
}

fn attr(s: &str, tag: &str, name: &str) -> Option<String> {
    elements(s, tag).into_iter().next().and_then(|e| attr_in(&e, name))
}

/// The text inside the first `<tag>` in `e`.
fn inner(e: &str, tag: &str) -> Option<String> {
    let el = elements(e, tag).into_iter().next()?;
    let a = el.find('>')? + 1;
    let b = el.rfind("</")?;
    Some(el[a..b].to_string())
}

/// The `<position coordinate="…">v</position>` values of a block.
fn positions(blk: &str) -> Vec<(String, f32)> {
    elements(blk, "position").into_iter().filter_map(|p| Some((attr_in(&p, "coordinate")?, inner_text(&p)?.trim().parse().ok()?))).collect()
}

fn inner_text(e: &str) -> Option<String> {
    let a = e.find('>')? + 1;
    let b = e.rfind("</")?;
    Some(e[a..b].to_string())
}

/// Writes `chans` (one buffer each) as one interleaved file (WAV or AIFF by extension).
pub fn write_interleaved(path: &Path, rate: u32, chans: &[Vec<f32>], bits: u16) -> io::Result<()> {
    let n = chans.iter().map(|c| c.len()).max().unwrap_or(0);
    let k = chans.len().max(1);
    let mut data = vec![0f32; n * k];
    for (c, buf) in chans.iter().enumerate() {
        for (f, x) in buf.iter().enumerate() {
            data[f * k + c] = *x;
        }
    }
    let mut af = AudioFile::new(rate, k, data);
    af.bits = bits;
    af.format = crate::file::Format::of(path);
    af.write(path)
}

/// Split: one mono file per channel, the channel's extension (".L", ".LFE") before the file
/// extension ("Mix.L.wav"). Returns the files written.
pub fn write_split(path: &Path, rate: u32, chans: &[Vec<f32>], exts: &[String], bits: u16) -> io::Result<Vec<std::path::PathBuf>> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = path.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "wav".into());
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let mut out = Vec::new();
    for (c, buf) in chans.iter().enumerate() {
        let tag = exts.get(c).cloned().unwrap_or_else(|| format!(".{}", c + 1));
        let p = dir.join(format!("{stem}{tag}.{ext}"));
        let mut af = AudioFile::new(rate, 1, buf.clone());
        af.bits = bits;
        af.format = crate::file::Format::of(&p);
        af.write(&p)?;
        out.push(p);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Adm {
        let bed = ["L", "R", "C", "LFE"].iter().zip([(-30.0, 0.0), (30.0, 0.0), (0.0, 0.0), (0.0, -30.0)]).map(|(l, (a, e))| BedChannel { label: l.to_string(), azimuth: a, elevation: e }).collect();
        Adm {
            rate: 48000,
            programme: "Song & Mix".into(),
            bed,
            bed_audio: (0..4).map(|k| vec![0.1 * k as f32; 480]).collect(),
            objects: vec![AdmObject { name: "Vox <lead>".into(), blocks: vec![Block { start: 0.0, duration: 0.005, x: -0.5, y: 1.0, z: 0.0, size: 0.0 }, Block { start: 0.005, duration: 0.005, x: 0.5, y: -1.0, z: 1.0, size: 0.25 }], audio: vec![0.5; 480] }],
            meta: vec![("Down51".into(), "Lo/Ro".into())],
        }
    }

    #[test]
    fn writes_and_reads_a_master() {
        let a = sample();
        let af = a.file();
        assert_eq!(af.channels, 5);
        let bytes = af.wav_bytes();
        let b = Adm::parse(&bytes).unwrap();
        assert_eq!(b.programme, "Song & Mix");
        assert_eq!(b.bed.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["L", "R", "C", "LFE"]);
        assert!((b.bed[0].azimuth + 30.0).abs() < 0.01, "azimuth flips back to ours");
        assert_eq!(b.objects.len(), 1);
        assert_eq!(b.objects[0].name, "Vox <lead>");
        assert_eq!(b.objects[0].blocks.len(), 2);
        let k = b.objects[0].blocks[1];
        assert!((k.x - 0.5).abs() < 1e-3 && (k.y + 1.0).abs() < 1e-3 && (k.z - 1.0).abs() < 1e-3 && (k.size - 0.25).abs() < 1e-3);
        assert!((k.start - 0.005).abs() < 1e-5);
        assert!((b.objects[0].audio[10] - 0.5).abs() < 1e-3);
        assert!((b.bed_audio[3][10] - 0.3).abs() < 1e-3);
        assert_eq!(b.meta, vec![("Down51".to_string(), "Lo/Ro".to_string())]);
        // ADM's own conventions in the document
        let x = a.xml();
        assert!(x.contains("<speakerLabel>M+030</speakerLabel>") && x.contains("<speakerLabel>LFE1</speakerLabel>"));
        assert!(x.contains(r#"typeDefinition="Objects""#) && x.contains("<cartesian>1</cartesian>"));
    }

    #[test]
    fn splits_with_channel_extensions() {
        let dir = std::env::temp_dir().join(format!("lg-adm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files = write_split(&dir.join("Mix.wav"), 48000, &[vec![0.1; 10], vec![0.2; 10]], &[".L".into(), ".R".into()], 24).unwrap();
        assert_eq!(files[0].file_name().unwrap().to_string_lossy(), "Mix.L.wav");
        assert_eq!(AudioFile::read(&files[1]).unwrap().channels, 1);
        write_interleaved(&dir.join("Mix.wav"), 48000, &vec![vec![0.1; 10]; 6], 24).unwrap();
        assert_eq!(AudioFile::read(&dir.join("Mix.wav")).unwrap().channels, 6);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
