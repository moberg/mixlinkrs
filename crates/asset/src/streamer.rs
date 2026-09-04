//! Worker reads 32-bit float stereo WAV from `start_frame` into an rtrb ring.
//! RT `try_pop` / `fill`; underrun writes zeros and increments a counter.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use hound::{SampleFormat, WavReader};
use rtrb::{Consumer, RingBuffer};
use thiserror::Error;

/// Default prefetch: 2 seconds at 48 kHz.
pub const DEFAULT_RING_FRAMES: usize = 96_000;
const READ_BLOCK: usize = 4096;

#[derive(Debug, Error)]
pub enum StreamerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("WAV error: {0}")]
    Wav(#[from] hound::Error),
    #[error("expected 32-bit float WAV, got {bits}-bit {format:?}")]
    Format { bits: u16, format: SampleFormat },
    #[error("could not spawn streamer thread: {0}")]
    Spawn(std::io::Error),
}

/// Lock-free stereo disk streamer. The worker owns the file handle.
pub struct Streamer {
    rx: Consumer<(f32, f32)>,
    underruns: Arc<AtomicU64>,
    eof: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Streamer {
    pub fn spawn(path: impl AsRef<Path>, start_frame: u64) -> Result<Self, StreamerError> {
        Self::spawn_with_ring(path, start_frame, DEFAULT_RING_FRAMES)
    }

    /// Start a worker that reads from `start_frame` into a ring of `ring_frames`.
    /// Does **not** load the whole file into RAM.
    pub fn spawn_with_ring(
        path: impl AsRef<Path>,
        start_frame: u64,
        ring_frames: usize,
    ) -> Result<Self, StreamerError> {
        let path = path.as_ref().to_path_buf();
        validate_header(&path)?;
        let cap = ring_frames.max(1024);
        let (producer, consumer) = RingBuffer::new(cap);
        let underruns = Arc::new(AtomicU64::new(0));
        let eof = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let eof_sig = eof.clone();
        let stop_sig = stop.clone();
        let worker = thread::Builder::new()
            .name("mixlink-streamer".into())
            .spawn(move || worker_loop(path, start_frame, producer, eof_sig, stop_sig))
            .map_err(StreamerError::Spawn)?;
        Ok(Self { rx: consumer, underruns, eof, stop, worker: Some(worker) })
    }

    /// RT-safe single-frame pop. `None` if the ring is empty.
    #[inline]
    pub fn try_pop(&mut self) -> Option<(f32, f32)> {
        self.rx.pop().ok()
    }

    /// RT-safe fill of planar stereo. Shortfall is zeroed; underrun counted
    /// when the file is not yet at EOF.
    pub fn fill(&mut self, left: &mut [f32], right: &mut [f32]) -> usize {
        let n = left.len().min(right.len());
        let mut i = 0;
        while i < n {
            match self.rx.pop() {
                Ok((l, r)) => {
                    left[i] = l;
                    right[i] = r;
                    i += 1;
                }
                Err(_) => break,
            }
        }
        if i < n {
            for j in i..n {
                left[j] = 0.0;
                right[j] = 0.0;
            }
            if !self.eof.load(Ordering::Acquire) {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        i
    }

    /// RT-safe interleaved stereo fill (`[L,R,L,R,…]`).
    pub fn fill_interleaved(&mut self, out: &mut [f32]) -> usize {
        debug_assert!(out.len() % 2 == 0);
        let frames = out.len() / 2;
        let mut n = 0;
        let mut i = 0;
        while n < frames {
            match self.rx.pop() {
                Ok((l, r)) => {
                    out[i] = l;
                    out[i + 1] = r;
                    i += 2;
                    n += 1;
                }
                Err(_) => break,
            }
        }
        if i < out.len() {
            for s in &mut out[i..] {
                *s = 0.0;
            }
            if !self.eof.load(Ordering::Acquire) {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        n
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    pub fn eof(&self) -> bool {
        self.eof.load(Ordering::Acquire)
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

fn validate_header(path: &Path) -> Result<(), StreamerError> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32 {
        return Err(StreamerError::Format {
            bits: spec.bits_per_sample,
            format: spec.sample_format,
        });
    }
    Ok(())
}

fn worker_loop(
    path: PathBuf,
    start_frame: u64,
    mut tx: rtrb::Producer<(f32, f32)>,
    eof: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    let Ok(mut reader) = WavReader::open(&path) else {
        eof.store(true, Ordering::Release);
        return;
    };
    let channels = reader.spec().channels.max(1) as usize;
    let mut samples = reader.samples::<f32>();
    // Skip to start_frame without buffering the prefix as a Vec.
    for _ in 0..start_frame.saturating_mul(channels as u64) {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match samples.next() {
            Some(Ok(_)) => {}
            _ => {
                eof.store(true, Ordering::Release);
                return;
            }
        }
    }

    let mut block = Vec::with_capacity(READ_BLOCK * channels);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        block.clear();
        while block.len() < READ_BLOCK * channels {
            match samples.next() {
                Some(Ok(s)) => block.push(s),
                Some(Err(_)) | None => break,
            }
        }
        if block.is_empty() {
            eof.store(true, Ordering::Release);
            return;
        }
        let frames = block.len() / channels;
        for i in 0..frames {
            let l = block[i * channels];
            let r = if channels > 1 { block[i * channels + 1] } else { l };
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match tx.push((l, r)) {
                    Ok(()) => break,
                    Err(rtrb::PushError::Full(_)) => {
                        thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        }
        if block.len() < READ_BLOCK * channels {
            eof.store(true, Ordering::Release);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounce::write_bounce;

    #[test]
    fn streams_from_start_frame_without_whole_file_vec() {
        let dir = std::env::temp_dir().join(format!("mixlink-stream-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("take.wav");
        let left: Vec<f32> = (0..64).map(|i| i as f32 * 0.01).collect();
        let right: Vec<f32> = (0..64).map(|i| i as f32 * -0.01).collect();
        write_bounce(&path, 48_000, &left, &right).unwrap();

        let mut s = Streamer::spawn_with_ring(&path, 10, 32).unwrap();
        let mut got_l = vec![0.0f32; 8];
        let mut got_r = vec![0.0f32; 8];
        // Wait for the worker to fill.
        let mut n = 0;
        for _ in 0..200 {
            n = s.fill(&mut got_l, &mut got_r);
            if n == 8 {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(n, 8);
        for i in 0..8 {
            assert!((got_l[i] - left[10 + i]).abs() < 1e-6);
            assert!((got_r[i] - right[10 + i]).abs() < 1e-6);
        }
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn underrun_zeros() {
        let dir = std::env::temp_dir().join(format!("mixlink-under-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("short.wav");
        write_bounce(&path, 48_000, &[0.5], &[0.25]).unwrap();
        let mut s = Streamer::spawn_with_ring(&path, 0, 8).unwrap();
        for _ in 0..200 {
            if s.eof() {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        // Drain the one real frame, then ask for more after EOF — zeros, no underrun.
        let _ = s.try_pop();
        let mut left = [1.0f32; 4];
        let mut right = [1.0f32; 4];
        let n = s.fill(&mut left, &mut right);
        assert_eq!(n, 0);
        assert!(left.iter().all(|v| *v == 0.0));
        assert_eq!(s.underruns(), 0);
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
