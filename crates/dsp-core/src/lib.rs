//! Core DSP primitives. No allocation on the audio path — functions that process
//! audio operate on pre-allocated slices only.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod block;
pub mod env;
pub mod fft;
pub mod filter;
pub mod interp;
pub mod onset;

/// Mono sample type used throughout the engine.
pub type Sample = f32;

/// Sample rate in Hz.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SampleRate(pub u32);

impl SampleRate {
    pub const CD: Self = Self(44_100);
    pub const PRO: Self = Self(48_000);

    #[inline]
    pub fn as_f32(self) -> f32 {
        self.0 as f32
    }

    #[inline]
    pub fn as_f64(self) -> f64 {
        self.0 as f64
    }
}

/// Internal block size for the engine. Must be a power of two and small.
pub const MAX_INTERNAL_BLOCK: usize = 64;

/// Convert dB to linear amplitude.
#[inline]
pub fn db_to_amp(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Convert linear amplitude to dB. Clamps at -inf for zero.
#[inline]
pub fn amp_to_db(amp: f32) -> f32 {
    if amp <= 1e-12 {
        -240.0
    } else {
        20.0 * amp.log10()
    }
}

/// MIDI note to Hz (A4 = 69 = 440 Hz).
#[inline]
pub fn midi_to_hz(note: f32) -> f32 {
    440.0 * 2f32.powf((note - 69.0) / 12.0)
}
