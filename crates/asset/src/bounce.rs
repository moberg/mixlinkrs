//! Offline bounce and streaming take writers: 32-bit float stereo WAV via hound.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
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

/// Incremental 32-bit float stereo writer. MixLink opens one file per track at
/// record start and appends 4096-frame blocks until stop.
pub struct StreamingWav {
    writer: WavWriter<BufWriter<File>>,
}

impl StreamingWav {
    pub fn create(path: impl AsRef<Path>, sample_rate: u32) -> Result<Self, BounceError> {
        let spec = WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let file = File::create(path)?;
        Ok(Self { writer: WavWriter::new(BufWriter::new(file), spec)? })
    }

    pub fn write_block(&mut self, left: &[f32], right: &[f32]) -> Result<(), BounceError> {
        if left.len() != right.len() {
            return Err(BounceError::ChannelMismatch);
        }
        for (l, r) in left.iter().copied().zip(right.iter().copied()) {
            self.writer.write_sample(l)?;
            self.writer.write_sample(r)?;
        }
        Ok(())
    }

    pub fn finalize(self) -> Result<(), BounceError> {
        self.writer.finalize()?;
        Ok(())
    }
}

/// Frame count from a 32-bit float WAV header. Used when scanning takes.
pub fn wav_frame_count(path: impl AsRef<Path>) -> i64 {
    WavReader::open(path).map(|r| r.duration() as i64).unwrap_or(0)
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
        let live = dir.join("live.wav");
        let mut stream = StreamingWav::create(&live, 48_000).unwrap();
        stream.write_block(&[0.5], &[0.25]).unwrap();
        stream.write_block(&[-0.5], &[-0.25]).unwrap();
        stream.finalize().unwrap();
        assert_eq!(wav_frame_count(&live), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
