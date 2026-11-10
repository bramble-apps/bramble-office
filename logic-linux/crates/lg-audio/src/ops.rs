//! The Audio File Editor's Functions: destructive edits of a file's frames (Normalize, Change
//! Gain, Fade-In, Fade-Out, Silence, Invert, Reverse, Trim, Remove DC Offset), Search Peak and
//! Search Silence, zero crossings (Snap Edits to Zero Crossings), and the backup Logic keeps of
//! a file before changing it (Create Backup / Revert to Backup).

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::fade::curve_gain;
use crate::file::AudioFile;

fn clamp(a: &AudioFile, r: &Range<usize>) -> Range<usize> {
    let n = a.frames();
    r.start.min(n)..r.end.min(n).max(r.start.min(n))
}

/// The largest absolute sample in frames `r`.
pub fn peak(a: &AudioFile, r: Range<usize>) -> f32 {
    let r = clamp(a, &r);
    a.data[r.start * a.channels..r.end * a.channels].iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

/// Normalize: the selection's peak to `peak_db` dBFS (Function Settings; Logic's default
/// 0 dB). Returns the gain applied (dB).
pub fn normalize(a: &mut AudioFile, r: Range<usize>, peak_db: f32) -> f32 {
    let p = peak(a, r.clone());
    if p <= 0.0 {
        return 0.0;
    }
    let target = 10f32.powf(peak_db.min(0.0) / 20.0);
    let db = 20.0 * (target / p).log10();
    change_gain(a, r, db);
    db
}

/// Change Gain by `db` (positive louder; samples clip at full scale).
pub fn change_gain(a: &mut AudioFile, r: Range<usize>, db: f32) {
    let r = clamp(a, &r);
    let g = 10f32.powf(db / 20.0);
    let c = a.channels;
    for s in &mut a.data[r.start * c..r.end * c] {
        *s = (*s * g).clamp(-1.0, 1.0);
    }
}

/// Change Gain to make the peak `percent` of what it is (Logic's dialog's percent field).
pub fn percent_to_db(percent: f32) -> f32 {
    20.0 * (percent.max(0.01) / 100.0).log10()
}

/// Fade-In over the selection (silence at its start), with Logic's curve.
pub fn fade_in(a: &mut AudioFile, r: Range<usize>, curve: i8) {
    let r = clamp(a, &r);
    let n = (r.end - r.start).max(1);
    let c = a.channels;
    for (k, f) in r.clone().enumerate() {
        let g = curve_gain(k as f32 / n as f32, curve);
        for s in &mut a.data[f * c..f * c + c] {
            *s *= g;
        }
    }
}

/// Fade-Out over the selection (silence at its end).
pub fn fade_out(a: &mut AudioFile, r: Range<usize>, curve: i8) {
    let r = clamp(a, &r);
    let n = (r.end - r.start).max(1);
    let c = a.channels;
    for (k, f) in r.clone().enumerate() {
        let g = curve_gain(1.0 - (k + 1) as f32 / n as f32, curve);
        for s in &mut a.data[f * c..f * c + c] {
            *s *= g;
        }
    }
}

pub fn silence(a: &mut AudioFile, r: Range<usize>) {
    let r = clamp(a, &r);
    let c = a.channels;
    a.data[r.start * c..r.end * c].fill(0.0);
}

pub fn invert(a: &mut AudioFile, r: Range<usize>) {
    let r = clamp(a, &r);
    let c = a.channels;
    for s in &mut a.data[r.start * c..r.end * c] {
        *s = -*s;
    }
}

pub fn reverse(a: &mut AudioFile, r: Range<usize>) {
    let r = clamp(a, &r);
    let c = a.channels;
    let (mut i, mut j) = (r.start, r.end);
    while i + 1 < j {
        j -= 1;
        for ch in 0..c {
            a.data.swap(i * c + ch, j * c + ch);
        }
        i += 1;
    }
}

/// Trim: only the selection is kept (the file gets shorter).
pub fn trim(a: &mut AudioFile, r: Range<usize>) {
    let r = clamp(a, &r);
    let c = a.channels;
    a.data.truncate(r.end * c);
    a.data.drain(..r.start * c);
}

/// Remove DC Offset: each channel's mean over the selection taken away.
pub fn remove_dc(a: &mut AudioFile, r: Range<usize>) {
    let r = clamp(a, &r);
    let c = a.channels;
    let n = (r.end - r.start).max(1) as f64;
    for ch in 0..c {
        let mean = (r.start..r.end).map(|f| a.data[f * c + ch] as f64).sum::<f64>() / n;
        for f in r.clone() {
            a.data[f * c + ch] = (a.data[f * c + ch] as f64 - mean) as f32;
        }
    }
}

/// Search Peak: the frame of the loudest sample in `r`.
pub fn search_peak(a: &AudioFile, r: Range<usize>) -> Option<usize> {
    let r = clamp(a, &r);
    let c = a.channels;
    (r.start..r.end).max_by(|&x, &y| {
        let m = |f: usize| a.data[f * c..f * c + c].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        m(x).total_cmp(&m(y))
    })
}

/// Search Silence: the next stretch from `from` at least `min_frames` long whose samples all
/// stay under `threshold` (linear, e.g. -60 dBFS = 0.001).
pub fn search_silence(a: &AudioFile, from: usize, threshold: f32, min_frames: usize) -> Option<Range<usize>> {
    let c = a.channels;
    let n = a.frames();
    let mut start = None;
    for f in from.min(n)..n {
        let quiet = a.data[f * c..f * c + c].iter().all(|s| s.abs() < threshold);
        match (quiet, start) {
            (true, None) => start = Some(f),
            (false, Some(s)) => {
                if f - s >= min_frames {
                    return Some(s..f);
                }
                start = None;
            }
            _ => {}
        }
    }
    start.filter(|s| n - s >= min_frames).map(|s| s..n)
}

/// The zero crossing nearest `frame` (within `reach` frames), on the mixed channels: Snap Edits
/// to Zero Crossings.
pub fn zero_crossing(a: &AudioFile, frame: usize, reach: usize) -> usize {
    let n = a.frames();
    if n < 2 {
        return frame.min(n);
    }
    let v = |f: usize| (0..a.channels).map(|c| a.at(f, c)).sum::<f32>();
    let crosses = |f: usize| f > 0 && f < n && (v(f - 1) <= 0.0) != (v(f) <= 0.0);
    let frame = frame.min(n - 1);
    for d in 0..=reach {
        if crosses(frame + d) {
            return frame + d;
        }
        if d <= frame && crosses(frame - d) {
            return frame - d;
        }
    }
    frame
}

/// Where a file's backup is kept: beside it, "name.ext" → ".name.bak.ext" (hidden, as Logic's
/// in the same folder).
pub fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("audio");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("wav");
    path.with_file_name(format!(".{name}.bak.{ext}"))
}

pub fn has_backup(path: &Path) -> bool {
    backup_path(path).exists()
}

/// Create Backup: the file as it is now (replacing an older backup).
pub fn create_backup(path: &Path) -> std::io::Result<PathBuf> {
    let b = backup_path(path);
    std::fs::copy(path, &b)?;
    Ok(b)
}

/// Revert to Backup: the backup back in the file's place (the backup is kept).
pub fn revert_to_backup(path: &Path) -> std::io::Result<()> {
    let b = backup_path(path);
    if !b.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no backup"));
    }
    std::fs::copy(&b, path).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(data: Vec<f32>, ch: usize) -> AudioFile {
        AudioFile::new(1000, ch, data)
    }

    #[test]
    fn normalize_and_gain() {
        let mut a = file(vec![0.25, -0.5, 0.1, 0.2], 2);
        let db = normalize(&mut a, 0..2, 0.0);
        assert!((db - 6.0206).abs() < 1e-3);
        assert!((a.data[1] + 1.0).abs() < 1e-6 && (a.data[0] - 0.5).abs() < 1e-6);
        normalize(&mut a, 0..2, -6.0206);
        assert!((peak(&a, 0..2) - 0.5).abs() < 1e-4);
        change_gain(&mut a, 0..2, percent_to_db(50.0));
        assert!((peak(&a, 0..2) - 0.25).abs() < 1e-4);
        // silence stays silence
        let mut s = file(vec![0.0; 8], 1);
        assert_eq!(normalize(&mut s, 0..8, 0.0), 0.0);
    }

    #[test]
    fn fades_silence_invert_reverse_trim_dc() {
        let mut a = file(vec![1.0; 10], 1);
        fade_in(&mut a, 0..10, 0);
        assert_eq!(a.data[0], 0.0);
        assert!((a.data[5] - 0.5).abs() < 1e-6);
        let mut b = file(vec![1.0; 10], 1);
        fade_out(&mut b, 0..10, 0);
        assert_eq!(b.data[9], 0.0);
        assert!(b.data[0] > 0.85);
        let mut c = file((0..6).map(|i| i as f32 / 10.0).collect(), 1);
        reverse(&mut c, 1..5);
        assert_eq!(c.data, vec![0.0, 0.4, 0.3, 0.2, 0.1, 0.5]);
        invert(&mut c, 0..2);
        assert_eq!(&c.data[..2], &[-0.0, -0.4]);
        silence(&mut c, 4..100);
        assert_eq!(&c.data[4..], &[0.0, 0.0]);
        let mut st = file(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2);
        trim(&mut st, 1..2);
        assert_eq!(st.data, vec![3.0, 4.0]);
        let mut dc = file(vec![0.6, 0.4, 0.6, 0.4], 1);
        remove_dc(&mut dc, 0..4);
        assert!(dc.data.iter().map(|x| x.abs()).all(|x| (x - 0.1).abs() < 1e-6));
    }

    #[test]
    fn peak_silence_and_zero_crossings() {
        let mut d = vec![0.5f32; 100];
        d[37] = -0.9;
        for v in &mut d[50..80] {
            *v = 0.0;
        }
        let a = file(d, 1);
        assert_eq!(search_peak(&a, 0..100), Some(37));
        assert_eq!(search_silence(&a, 0, 0.001, 10), Some(50..80));
        assert_eq!(search_silence(&a, 0, 0.001, 40), None);
        let s = file((0..100).map(|i| (i as f32 * std::f32::consts::PI / 20.0).sin()).collect(), 1);
        let z = zero_crossing(&s, 23, 30);
        assert!(z == 21 || z == 20, "{z}");
    }

    #[test]
    fn backup_and_revert() {
        let dir = std::env::temp_dir().join(format!("lg-audio-ops-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("take.wav");
        let mut a = file(vec![0.1, 0.2, 0.3], 1);
        a.write(&p).unwrap();
        create_backup(&p).unwrap();
        assert!(has_backup(&p) && backup_path(&p).ends_with(".take.bak.wav"));
        invert(&mut a, 0..3);
        a.write(&p).unwrap();
        revert_to_backup(&p).unwrap();
        let back = AudioFile::read(&p).unwrap();
        assert!(back.data[2] > 0.29);
        let _ = std::fs::remove_dir_all(dir);
    }
}
