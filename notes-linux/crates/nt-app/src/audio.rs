//! Audio attachments (rodio over symphonia): what a recording is (rate, channels, length), for the
//! editor's inline player and Record Audio.

/// A file's sample rate, channels and length in seconds, or why it can't be read.
pub fn info(path: &std::path::Path) -> Result<(u32, u16, f64), String> {
    use rodio::Source;
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let d = rodio::Decoder::try_from(f).map_err(|e| e.to_string())?;
    let (rate, channels) = (d.sample_rate().get(), d.channels().get());
    let secs = match d.total_duration() {
        Some(t) => t.as_secs_f64(),
        // (no length in the header: counted)
        None => d.count() as f64 / (rate as f64 * channels as f64),
    };
    Ok((rate, channels, secs))
}

/// Edit > Record Audio…: the microphone read on a thread of its own until stopped (scripted runs
/// record a tone instead, never the room).
pub struct Recorder {
    samples: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    /// the loudest sample of the last ~50 ms, 0..1 (as bits), for the level meter
    level: std::sync::Arc<std::sync::atomic::AtomicU32>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    error: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    pub rate: u32,
    pub channels: u16,
    started: std::time::Instant,
}

impl Recorder {
    pub fn start(fake: bool) -> Result<Recorder, String> {
        use std::sync::atomic::Ordering;
        use std::sync::{Arc, Mutex};
        let samples = Arc::new(Mutex::new(Vec::new()));
        let level = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (s2, l2, st2, e2) = (samples.clone(), level.clone(), stop.clone(), error.clone());
        let (tx, rx) = std::sync::mpsc::channel::<Result<(u32, u16), String>>();
        std::thread::spawn(move || {
            let feed = |it: &mut dyn Iterator<Item = f32>, rate: u32, channels: u16, paced: bool| {
                let chunk = (rate as usize * channels as usize / 20).max(1);
                let t0 = std::time::Instant::now();
                let mut n = 0usize;
                while !st2.load(Ordering::Relaxed) {
                    let buf: Vec<f32> = (&mut *it).take(chunk).collect();
                    if buf.is_empty() {
                        break;
                    }
                    let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs())).min(1.0);
                    l2.store(peak.to_bits(), Ordering::Relaxed);
                    n += buf.len();
                    s2.lock().unwrap_or_else(|e| e.into_inner()).extend(buf);
                    if paced {
                        // (the tone comes as fast as it's asked for: kept to real time)
                        let due = std::time::Duration::from_secs_f64(n as f64 / (rate as f64 * channels as f64));
                        std::thread::sleep(due.saturating_sub(t0.elapsed()));
                    }
                }
            };
            if fake {
                let _ = tx.send(Ok((44100, 1)));
                let mut k = 0u64;
                let mut tone = std::iter::from_fn(move || {
                    k += 1;
                    Some((k as f32 * 440.0 * std::f32::consts::TAU / 44100.0).sin() * 0.3)
                });
                feed(&mut tone, 44100, 1, true);
                return;
            }
            let mic = rodio::microphone::MicrophoneBuilder::new().default_device().and_then(|b| b.default_config()).map_err(|e| e.to_string()).and_then(|b| b.open_stream().map_err(|e| e.to_string()));
            match mic {
                Ok(mut mic) => {
                    let (rate, channels) = (mic.config().sample_rate.get(), mic.config().channel_count.get());
                    let _ = tx.send(Ok((rate, channels)));
                    feed(&mut mic, rate, channels, false);
                    if !st2.load(Ordering::Relaxed) {
                        *e2.lock().unwrap_or_else(|e| e.into_inner()) = Some("The microphone stopped.".into());
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });
        let (rate, channels) = rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "The microphone didn’t answer.".to_string())??;
        Ok(Recorder { samples, level, stop, error, rate, channels, started: std::time::Instant::now() })
    }

    pub fn seconds(&self) -> f64 {
        self.samples.lock().map(|s| s.len()).unwrap_or(0) as f64 / (self.rate as f64 * self.channels as f64)
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(std::sync::atomic::Ordering::Relaxed))
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|e| e.clone())
    }

    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Stopped: the samples taken.
    pub fn finish(self) -> (Vec<f32>, u32, u16) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let s = std::mem::take(&mut *self.samples.lock().unwrap_or_else(|e| e.into_inner()));
        (s, self.rate, self.channels)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 16-bit PCM WAV.
pub fn write_wav(path: &std::path::Path, samples: &[f32], rate: u32, channels: u16) -> std::io::Result<()> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out)
}

/// The recording as a file to attach: AAC in m4a as Notes records (with ffmpeg, when it's
/// installed), else the WAV.
pub fn save_recording(samples: &[f32], rate: u32, channels: u16) -> Result<std::path::PathBuf, String> {
    // (a folder per take, so each is plainly "Audio Recording" in the note)
    let dir = std::env::temp_dir().join("bramble-notes-recordings").join(format!("{:x}-{}", (crate::dates::now() * 1000.0) as u64, std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let wav = dir.join("Audio Recording.wav");
    write_wav(&wav, samples, rate, channels).map_err(|e| e.to_string())?;
    let m4a = wav.with_extension("m4a");
    let ok = std::process::Command::new("ffmpeg").args(["-loglevel", "error", "-y", "-i"]).arg(&wav).args(["-c:a", "aac", "-b:a", "96k"]).arg(&m4a).status().is_ok_and(|s| s.success());
    if ok {
        let _ = std::fs::remove_file(&wav);
        Ok(m4a)
    } else {
        Ok(wav)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_notes_m4a() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/test-tone.m4a");
        let (rate, channels, secs) = super::info(&p).unwrap();
        assert_eq!((rate, channels), (44100, 1));
        assert!((secs - 0.5).abs() < 0.1, "{secs}");
    }

    #[test]
    fn records_and_saves() {
        let r = super::Recorder::start(true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(r.level() > 0.2);
        let (s, rate, ch) = r.finish();
        assert!(s.len() > 10_000, "{}", s.len());
        let p = super::save_recording(&s, rate, ch).unwrap();
        let (r2, c2, secs) = super::info(&p).unwrap();
        assert_eq!((r2, c2), (44100, 1));
        assert!(secs > 0.25, "{secs}");
        let _ = std::fs::remove_file(p);
    }
}
