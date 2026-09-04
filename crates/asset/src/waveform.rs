//! Waveform LOD: per-file min/max (and peak) bins, MixLink gamma, column downsample.
//!
//! MixLink `ArrangementView.drawClipWaveform`:
//! `mag = pow(peak / maxPeak, 0.45)`, floor 0.6 px, `scale >= 1` max-of-bucket
//! and `scale < 1` nearest-sample upsample.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use hound::{SampleFormat, WavReader};

/// MixLink hop: ~0.7 ms at 48 kHz.
pub const SAMPLES_PER_BIN: usize = 32;
const MAX_BINS: usize = 250_000;

/// MixLink `pow(n, 0.45)`.
pub const GAMMA: f32 = 0.45;

/// MixLink 0.6 px floor on the half-height magnitude.
pub const MIN_HALF_PX: f32 = 0.6;

#[derive(Clone, Debug, Default)]
pub struct WaveformLod {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
    /// Per-bin peak (max abs L/R) — what ArrangementView draws.
    pub peaks: Vec<f32>,
    /// Per-file peak, not per-clip.
    pub max_peak: f32,
}

impl WaveformLod {
    pub fn is_empty(&self) -> bool {
        self.peaks.is_empty()
    }
}

/// Cache keyed by filename. [`WaveformCache::note_file_finished`] bumps `epoch`
/// so the timeline redraws without treating playhead ticks as data changes.
#[derive(Debug)]
pub struct WaveformCache {
    inner: Mutex<HashMap<String, Arc<WaveformLod>>>,
    epoch: AtomicU64,
}

impl Default for WaveformCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformCache {
    pub fn new() -> Self {
        Self { inner: Mutex::new(HashMap::new()), epoch: AtomicU64::new(0) }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    pub fn get(&self, filename: &str) -> Option<Arc<WaveformLod>> {
        self.inner.lock().ok()?.get(filename).cloned()
    }

    pub fn insert(&self, filename: impl Into<String>, lod: WaveformLod) {
        if let Ok(mut map) = self.inner.lock() {
            map.insert(filename.into(), Arc::new(lod));
        }
    }

    /// Recording (or bounce) finished — drop the stale bins and bump epoch.
    pub fn note_file_finished(&self, filename: &str) {
        if let Ok(mut map) = self.inner.lock() {
            map.remove(filename);
        }
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }

    pub fn load_file(&self, filename: &str, path: impl AsRef<Path>) -> Arc<WaveformLod> {
        if let Some(cached) = self.get(filename) {
            return cached;
        }
        let lod = load_waveform(path.as_ref());
        let arc = Arc::new(lod);
        if let Ok(mut map) = self.inner.lock() {
            map.insert(filename.into(), arc.clone());
        }
        arc
    }
}

/// Stream the WAV and build min/max/peak bins. Does not keep the PCM.
pub fn load_waveform(path: &Path) -> WaveformLod {
    let Ok(mut reader) = WavReader::open(path) else {
        return WaveformLod::default();
    };
    let spec = reader.spec();
    if spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32 {
        return WaveformLod::default();
    }
    let channels = spec.channels.max(1) as usize;
    let total = reader.duration() as usize;
    if total == 0 {
        return WaveformLod::default();
    }
    let hop = SAMPLES_PER_BIN.max((total + MAX_BINS - 1) / MAX_BINS);
    let bin_count = (total + hop - 1) / hop;
    let mut min = vec![f32::INFINITY; bin_count];
    let mut max = vec![f32::NEG_INFINITY; bin_count];
    let mut peaks = vec![0.0f32; bin_count];
    let mut max_peak = 0.0f32;
    let mut frame = 0usize;
    let mut samples = reader.samples::<f32>();
    while frame < total {
        let mut frame_peak = 0.0f32;
        let mut frame_min = f32::INFINITY;
        let mut frame_max = f32::NEG_INFINITY;
        for ch in 0..channels {
            match samples.next() {
                Some(Ok(s)) => {
                    frame_min = frame_min.min(s);
                    frame_max = frame_max.max(s);
                    if ch < 2 {
                        frame_peak = frame_peak.max(s.abs());
                    }
                }
                _ => break,
            }
        }
        if !frame_min.is_finite() {
            break;
        }
        let bin = frame / hop;
        if bin < bin_count {
            min[bin] = min[bin].min(frame_min);
            max[bin] = max[bin].max(frame_max);
            if frame_peak > peaks[bin] {
                peaks[bin] = frame_peak;
            }
            if frame_peak > max_peak {
                max_peak = frame_peak;
            }
        }
        frame += 1;
    }
    for i in 0..bin_count {
        if !min[i].is_finite() {
            min[i] = 0.0;
        }
        if !max[i].is_finite() {
            max[i] = 0.0;
        }
    }
    WaveformLod { min, max, peaks, max_peak }
}

/// MixLink gamma: `pow(peak / maxPeak, 0.45)`, clamped 0…1.
pub fn gamma_mag(peak: f32, max_peak: f32) -> f32 {
    if max_peak <= 0.00001 {
        return 0.0;
    }
    let n = (peak / max_peak).clamp(0.0, 1.0);
    n.powf(GAMMA)
}

/// Half-height in pixels: `max(gamma * half, 0.6)`.
pub fn column_half_pixels(peak: f32, max_peak: f32, half: f32) -> f32 {
    (gamma_mag(peak, max_peak) * half).max(MIN_HALF_PX)
}

/// Per-column peaks matching `drawClipWaveform` downsample / upsample.
///
/// `peaks[start .. start+count]` is the clip slice; `maxPeak` is **not**
/// recomputed here (per-file, applied by [`gamma_mag`]).
pub fn bins_for_width(
    peaks: &[f32],
    start: usize,
    count: usize,
    cols: usize,
    start_x: f64,
    full_width: f64,
) -> Vec<f32> {
    if count == 0 || cols == 0 || full_width <= 1.0 || start >= peaks.len() {
        return Vec::new();
    }
    let count = count.min(peaks.len() - start);
    let scale = count as f64 / full_width;
    let i0 = ((start_x * scale) as i64).clamp(0, (count as i64) - 1) as usize;
    let i1 = (((start_x + cols as f64) * scale + 0.999) as i64)
        .clamp((i0 + 1) as i64, count as i64) as usize;

    let at = |i: usize| peaks[start + i];
    let mut out = vec![0.0f32; cols];

    if scale >= 1.0 {
        let mut col = 0usize;
        let mut peak = 0.0f32;
        let mut next_i =
            (((start_x + 1.0) * scale) as i64).clamp((i0 + 1) as i64, count as i64) as usize;
        for i in i0..i1 {
            if i >= next_i && col < cols {
                out[col] = peak;
                col += 1;
                peak = 0.0;
                next_i = ((next_i + 1) as i64)
                    .max(((start_x + col as f64 + 1.0) * scale) as i64)
                    .clamp(0, count as i64) as usize;
            }
            peak = peak.max(at(i));
        }
        while col < cols {
            out[col] = peak;
            col += 1;
            peak = 0.0;
        }
    } else {
        for x in 0..cols {
            let i = (((start_x + x as f64) * scale) as i64).clamp(0, (count as i64) - 1) as usize;
            out[x] = at(i);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File peak is 1.0 even though the clip slice max is 0.5.
    fn slice_peaks() -> (Vec<f32>, f32) {
        // indices: 0..8
        ([0.10, 0.50, 0.20, 0.80, 0.30, 0.40, 0.05, 0.25].to_vec(), 1.0)
    }

    #[test]
    fn downsample_scale_ge_1() {
        let (peaks, max_peak) = slice_peaks();
        // 6 bins into 3 columns, full_width 3 → scale = 2.
        let cols = bins_for_width(&peaks, 0, 6, 3, 0.0, 3.0);
        assert_eq!(cols.len(), 3);
        assert!((cols[0] - 0.50).abs() < 1e-6, "{cols:?}");
        assert!((cols[1] - 0.80).abs() < 1e-6, "{cols:?}");
        assert!((cols[2] - 0.40).abs() < 1e-6, "{cols:?}");
        let g: Vec<f32> = cols.iter().map(|p| gamma_mag(*p, max_peak)).collect();
        assert!((g[0] - 0.50f32.powf(0.45)).abs() < 1e-6);
        assert!((g[1] - 0.80f32.powf(0.45)).abs() < 1e-6);
        // per-file maxPeak: 0.8/1.0, not 0.8/0.8.
        assert!(g[1] < 1.0);
    }

    #[test]
    fn upsample_scale_lt_1() {
        let peaks = [0.2f32, 0.8];
        let cols = bins_for_width(&peaks, 0, 2, 4, 0.0, 4.0);
        assert_eq!(cols.len(), 4);
        assert!((cols[0] - 0.2).abs() < 1e-6);
        assert!((cols[1] - 0.2).abs() < 1e-6);
        assert!((cols[2] - 0.8).abs() < 1e-6);
        assert!((cols[3] - 0.8).abs() < 1e-6);
        let g: Vec<f32> = cols.iter().map(|p| gamma_mag(*p, 1.0)).collect();
        assert!((g[0] - 0.2f32.powf(0.45)).abs() < 1e-6);
        assert!((g[2] - 0.8f32.powf(0.45)).abs() < 1e-6);
    }

    #[test]
    fn gamma_uses_file_max_peak() {
        // Clip slice max is 0.5; file maxPeak is 1.0.
        let mag_file = gamma_mag(0.5, 1.0);
        let mag_local = gamma_mag(0.5, 0.5);
        assert!((mag_file - 0.5f32.powf(0.45)).abs() < 1e-6);
        assert!((mag_local - 1.0).abs() < 1e-6);
        assert!(mag_file < mag_local);
        assert!((column_half_pixels(0.0, 1.0, 10.0) - MIN_HALF_PX).abs() < 1e-6);
    }

    #[test]
    fn golden_bins_both_paths() {
        let peaks = [0.0f32, 0.25, 0.5, 1.0, 0.5, 0.25, 0.0, 0.1];
        let max_peak = 1.0;
        let down = bins_for_width(&peaks, 0, 8, 4, 0.0, 4.0);
        assert_eq!(down.len(), 4);
        let down_g: Vec<f32> = down.iter().map(|p| gamma_mag(*p, max_peak)).collect();
        // scale=2: max(0,0.25), max(0.5,1), max(0.5,0.25), max(0,0.1)
        assert!((down[0] - 0.25).abs() < 1e-6);
        assert!((down[1] - 1.00).abs() < 1e-6);
        assert!((down[2] - 0.50).abs() < 1e-6);
        assert!((down[3] - 0.10).abs() < 1e-6);
        assert!((down_g[1] - 1.0).abs() < 1e-6);
        assert!((down_g[0] - 0.25f32.powf(0.45)).abs() < 1e-6);

        let up = bins_for_width(&peaks, 2, 2, 4, 0.0, 4.0);
        // slice [0.5, 1.0], scale=0.5
        assert!((up[0] - 0.5).abs() < 1e-6);
        assert!((up[1] - 0.5).abs() < 1e-6);
        assert!((up[2] - 1.0).abs() < 1e-6);
        assert!((up[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn epoch_bumps_on_finish() {
        let cache = WaveformCache::new();
        cache.insert("3-ch-01-Rytm.wav", WaveformLod { peaks: vec![0.2], max_peak: 0.2, ..Default::default() });
        assert_eq!(cache.epoch(), 0);
        cache.note_file_finished("3-ch-01-Rytm.wav");
        assert_eq!(cache.epoch(), 1);
        assert!(cache.get("3-ch-01-Rytm.wav").is_none());
    }
}
