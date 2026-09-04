//! Offline bounce: 32-bit float stereo WAV via hound.

use std::path::Path;

use hound::{SampleFormat, WavSpec, WavWriter};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BounceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("WAV error: {0}")]
    Wav(#[from] hound::Error),
    #[error("left/right length mismatch")]
    ChannelMismatch,
}

/// Write interleaved or split stereo 32-bit float PCM.
pub fn write_bounce(
    path: impl AsRef<Path>,
    sample_rate: u32,
    left: &[f32],
    right: &[f32],
) -> Result<(), BounceError> {
    if left.len() != right.len() {
        return Err(BounceError::ChannelMismatch);
    }
    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)?;
    for (l, r) in left.iter().copied().zip(right.iter().copied()) {
        writer.write_sample(l)?;
        writer.write_sample(r)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounce_roundtrip_header() {
        let dir = std::env::temp_dir().join(format!("mixlink-bounce-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("mix.wav");
        write_bounce(&path, 48_000, &[0.1, -0.2], &[0.3, -0.4]).unwrap();
        let mut reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.bits_per_sample, 32);
        assert_eq!(spec.sample_format, SampleFormat::Float);
        let samples: Vec<f32> = reader.samples::<f32>().map(|s| s.unwrap()).collect();
        assert_eq!(samples, vec![0.1, 0.3, -0.2, -0.4]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
