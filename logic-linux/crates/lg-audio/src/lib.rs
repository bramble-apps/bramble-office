//! Bramble Studio's audio-file work, for Logic's audio editors (Audio Track Editor, Audio File
//! Editor, Smart Tempo Editor): pure Rust, no dependencies.
//!
//! - [`file`]: WAV and AIFF/AIFC read and written as they were (rate, channels, bit depth),
//!   unknown chunks kept (Logic's LGBM beat map…), its stale overview chunks dropped;
//! - [`peaks`]: the waveform overview (min/max per pixel, mipmapped) for files of any length;
//! - [`transients`]: onsets (spectral flux, peak picking, sample-accurate placement);
//! - [`tempo`]: Smart Tempo's analysis: tempo, beats, downbeats and meter, constant or variable;
//! - [`ops`]: the Audio File Editor's destructive Functions, zero crossings, Search Peak /
//!   Silence, Create Backup / Revert to Backup;
//! - [`fade`]: audio region fades (Logic's lengths in ms and -99..99 curves);
//! - [`record`]: recording: WAV files written as the audio comes in, comps joined into one file;
//! - [`stretch`]: WSOLA time stretch, pitch shift and piecewise warping (Flex Time / Flex Pitch
//!   groundwork), and a YIN pitch detector.

pub mod fade;
mod fft;
pub mod file;
pub mod ops;
pub mod peaks;
pub mod record;
pub mod stretch;
pub mod tempo;
pub mod transients;

pub use file::{AudioFile, Format};
