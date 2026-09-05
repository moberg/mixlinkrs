//! Take WAV I/O: disk streamers, waveform LOD, bounce writer.
//!
//! Take files are 32-bit float stereo (what MixLink’s recorder writes). The
//! streamer never decodes the whole file on spawn — a worker reads from
//! `start_frame` into a lock-free ring.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod bounce;
pub mod streamer;
pub mod waveform;

pub use bounce::{wav_frame_count, write_bounce, BounceError, StreamingWav};
pub use streamer::{Streamer, StreamerError};
pub use waveform::{
    bins_for_width, column_half_pixels, gamma_mag, minmax_for_width, minmax_zoom_points,
    WaveformCache, WaveformLod, GAMMA, MIN_HALF_PX, SAMPLES_PER_BIN,
};
