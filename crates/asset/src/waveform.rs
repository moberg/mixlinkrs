//! Waveform LOD: per-file min/max (and peak) bins, MixLink gamma, column downsample.
//!
//! Arrangement clips draw bipolar min/max (linear, per-file `maxPeak`) so
//! transients stay one-column spikes. Peak + `pow(n, 0.45)` remains available
//! for MixLink-style magnitude fills.

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

/// Do not expand a file to fill the clip below this peak (−40 dBFS).
/// Analog returns (Heat, etc.) sit on a −60…−80 dB noise floor; per-file
/// normalize would draw that hiss as a full take.
pub const WAVEFORM_REF_PEAK: f32 = 0.01;

/// Peak used to scale arrangement min/max. Quiet noise stays a hairline.
pub fn waveform_display_peak(max_peak: f32) -> f32 {
    max_peak.max(WAVEFORM_REF_PEAK)
}

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
///
/// Decode runs on Rayon's pool ([`WaveformCache::request`]); the UI thread only
/// queues work and reads [`WaveformStatus`].
#[derive(Clone, Debug)]
pub struct WaveformCache {
    inner: Arc<CacheInner>,
}

#[derive(Debug)]
struct CacheInner {
    map: Mutex<HashMap<String, CacheEntry>>,
    epoch: AtomicU64,
    next_gen: AtomicU64,
}

#[derive(Clone, Debug)]
enum CacheEntry {
    Pending(u64),
    Ready(Arc<WaveformLod>),
}

#[derive(Clone, Debug)]
pub enum WaveformStatus {
    Missing,
    Loading,
    Ready(Arc<WaveformLod>),
}

impl Default for WaveformCache {
    fn default() -> Self {
        Self::new()
    }
}

impl WaveformCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(CacheInner {
                map: Mutex::new(HashMap::new()),
                epoch: AtomicU64::new(0),
                next_gen: AtomicU64::new(1),
            }),
        }
    }

    pub fn epoch(&self) -> u64 {
        self.inner.epoch.load(Ordering::Relaxed)
    }

    pub fn status(&self, filename: &str) -> WaveformStatus {
        let Ok(map) = self.inner.map.lock() else {
            return WaveformStatus::Missing;
        };
        match map.get(filename) {
            Some(CacheEntry::Ready(lod)) => WaveformStatus::Ready(lod.clone()),
            Some(CacheEntry::Pending(_)) => WaveformStatus::Loading,
            None => WaveformStatus::Missing,
        }
    }

    pub fn get(&self, filename: &str) -> Option<Arc<WaveformLod>> {
        match self.status(filename) {
            WaveformStatus::Ready(lod) => Some(lod),
            _ => None,
        }
    }

    pub fn is_loading(&self, filename: &str) -> bool {
        matches!(self.status(filename), WaveformStatus::Loading)
    }

    pub fn insert(&self, filename: impl Into<String>, lod: WaveformLod) {
        if let Ok(mut map) = self.inner.map.lock() {
            map.insert(filename.into(), CacheEntry::Ready(Arc::new(lod)));
        }
    }

    /// Test / paint helper: treat `filename` as in-flight without decoding.
    pub fn mark_loading(&self, filename: impl Into<String>) {
        let gen = self.inner.next_gen.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut map) = self.inner.map.lock() {
            map.entry(filename.into()).or_insert(CacheEntry::Pending(gen));
        }
    }

    /// Recording (or bounce) finished — drop the stale bins and bump epoch.
    pub fn note_file_finished(&self, filename: &str) {
        if let Ok(mut map) = self.inner.map.lock() {
            map.remove(filename);
        }
        self.inner.epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Queue a background decode. No-op if this name is already pending or ready.
    /// Completions bump [`Self::epoch`].
    pub fn request(&self, filename: impl Into<String>, path: impl AsRef<Path>) {
        let filename = filename.into();
        let path = path.as_ref().to_path_buf();
        let gen = {
            let Ok(mut map) = self.inner.map.lock() else {
                return;
            };
            if map.contains_key(&filename) {
                return;
            }
            let gen = self.inner.next_gen.fetch_add(1, Ordering::Relaxed);
            map.insert(filename.clone(), CacheEntry::Pending(gen));
            gen
        };
        let inner = Arc::clone(&self.inner);
        rayon::spawn(move || {
            let lod = load_waveform(&path);
            let ready = Arc::new(lod);
            if let Ok(mut map) = inner.map.lock() {
                match map.get(&filename) {
                    Some(CacheEntry::Pending(g)) if *g == gen => {
                        map.insert(filename, CacheEntry::Ready(ready));
                        inner.epoch.fetch_add(1, Ordering::Relaxed);
                    }
                    _ => {}
                }
            }
        });
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
    let max_peak = waveform_display_peak(max_peak);
    let n = (peak / max_peak).clamp(0.0, 1.0);
    n.powf(GAMMA)
}

/// Half-height in pixels: `max(gamma * half, 0.6)`.
pub fn column_half_pixels(peak: f32, max_peak: f32, half: f32) -> f32 {
    (gamma_mag(peak, max_peak) * half).max(MIN_HALF_PX)
}

/// Per-column min/max matching the `bins_for_width` downsample / upsample.
///
/// Used for bipolar waveform drawing so a one-bin transient survives as a
/// full-height spike instead of being averaged into the body.
pub fn minmax_for_width(
    min: &[f32],
    max: &[f32],
    start: usize,
    count: usize,
    cols: usize,
    start_x: f64,
    full_width: f64,
) -> Vec<(f32, f32)> {
    let n = min.len().min(max.len());
    if count == 0 || cols == 0 || full_width <= 1.0 || start >= n {
        return Vec::new();
    }
    let count = count.min(n - start);
    let scale = count as f64 / full_width;
    let i0 = ((start_x * scale) as i64).clamp(0, (count as i64) - 1) as usize;
    let i1 = (((start_x + cols as f64) * scale + 0.999) as i64).clamp((i0 + 1) as i64, count as i64)
        as usize;

    let at_min = |i: usize| min[start + i];
    let at_max = |i: usize| max[start + i];
    let mut out = vec![(0.0f32, 0.0f32); cols];

    if scale >= 1.0 {
        let mut col = 0usize;
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        let mut next_i =
            (((start_x + 1.0) * scale) as i64).clamp((i0 + 1) as i64, count as i64) as usize;
        for i in i0..i1 {
            if i >= next_i && col < cols {
                out[col] = finite_pair(lo, hi);
                col += 1;
                lo = f32::INFINITY;
                hi = f32::NEG_INFINITY;
                next_i = ((next_i + 1) as i64)
                    .max(((start_x + col as f64 + 1.0) * scale) as i64)
                    .clamp(0, count as i64) as usize;
            }
            lo = lo.min(at_min(i));
            hi = hi.max(at_max(i));
        }
        while col < cols {
            out[col] = finite_pair(lo, hi);
            col += 1;
            lo = f32::INFINITY;
            hi = f32::NEG_INFINITY;
        }
    } else {
        for x in 0..cols {
            let i = (((start_x + x as f64) * scale) as i64).clamp(0, (count as i64) - 1) as usize;
            out[x] = (at_min(i), at_max(i));
        }
    }
    out
}

/// One `(min, max)` per LOD bin in the visible window, plus a neighbour on
/// each side so the strip can slope off-screen. `x0` is the first point's
/// offset from the clip left; spacing is pixels per bin.
pub fn minmax_zoom_points(
    min: &[f32],
    max: &[f32],
    start: usize,
    count: usize,
    start_x: f64,
    vis_width: f64,
    full_width: f64,
) -> (f64, f64, Vec<(f32, f32)>) {
    let n = min.len().min(max.len());
    if count == 0 || vis_width <= 0.0 || full_width <= 1.0 || start >= n {
        return (0.0, 1.0, Vec::new());
    }
    let count = count.min(n - start);
    if count == 0 {
        return (0.0, 1.0, Vec::new());
    }
    let px_per_bin = full_width / count as f64;
    let last_i = (count as i64) - 1;
    let first = ((start_x / px_per_bin).floor() as i64 - 1).clamp(0, last_i) as usize;
    let last = (((start_x + vis_width) / px_per_bin).ceil() as i64 + 1).clamp(first as i64, last_i)
        as usize;
    let mut out = Vec::with_capacity(last - first + 1);
    for i in first..=last {
        out.push((min[start + i], max[start + i]));
    }
    (first as f64 * px_per_bin, px_per_bin, out)
}

fn finite_pair(lo: f32, hi: f32) -> (f32, f32) {
    (if lo.is_finite() { lo } else { 0.0 }, if hi.is_finite() { hi } else { 0.0 })
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
    let i1 = (((start_x + cols as f64) * scale + 0.999) as i64).clamp((i0 + 1) as i64, count as i64)
        as usize;

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
    fn analog_noise_floor_does_not_fill_the_clip() {
        // Take 20 Heat: peak ≈ −67 dBFS, uncorrelated hiss.
        let heat = 0.00045f32;
        let scale = waveform_display_peak(heat);
        assert!((scale - WAVEFORM_REF_PEAK).abs() < 1e-6);
        assert!(heat / scale < 0.05);
        assert!(gamma_mag(heat, heat) < 0.3);
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
    fn downsample_keeps_transient_spike() {
        let mut min = vec![0.0f32; 64];
        let mut max = vec![0.0f32; 64];
        min[10] = -1.0;
        max[10] = 1.0;
        let cols = minmax_for_width(&min, &max, 0, 64, 16, 0.0, 16.0);
        assert_eq!(cols.len(), 16);
        let spikes = cols.iter().filter(|(lo, hi)| *lo <= -0.999 && *hi >= 0.999).count();
        let quiet = cols.iter().filter(|(lo, hi)| lo.abs() < 0.01 && hi.abs() < 0.01).count();
        assert_eq!(spikes, 1, "{cols:?}");
        assert_eq!(quiet, 15, "{cols:?}");
    }

    #[test]
    fn minmax_viewport_slice_matches_full_window() {
        let min: Vec<f32> = (0..8).map(|i| -(i as f32) / 8.0).collect();
        let max: Vec<f32> = (0..8).map(|i| (i as f32) / 8.0).collect();
        let full = minmax_for_width(&min, &max, 0, 8, 8, 0.0, 8.0);
        let mid = minmax_for_width(&min, &max, 0, 8, 3, 2.0, 8.0);
        assert_eq!(mid.len(), 3);
        assert!((mid[0].0 - full[2].0).abs() < 1e-6 && (mid[0].1 - full[2].1).abs() < 1e-6);
        assert!((mid[2].0 - full[4].0).abs() < 1e-6 && (mid[2].1 - full[4].1).abs() < 1e-6);
    }

    #[test]
    fn zoom_points_one_per_bin() {
        let min = [-0.2f32, -0.8, -0.4];
        let max = [0.2f32, 0.8, 0.4];
        let (x0, spacing, pts) = minmax_zoom_points(&min, &max, 0, 3, 10.0, 10.0, 30.0);
        assert!((spacing - 10.0).abs() < 1e-6);
        assert_eq!(pts.len(), 3);
        assert!((x0 - 0.0).abs() < 1e-6);
        assert!((pts[1].0 + 0.8).abs() < 1e-6);
    }

    #[test]
    fn zoom_points_stay_near_viewport() {
        let min: Vec<f32> = (0..40).map(|i| i as f32).collect();
        let max = min.clone();
        let (x0, spacing, pts) = minmax_zoom_points(&min, &max, 0, 40, 80.0, 30.0, 400.0);
        assert!((spacing - 10.0).abs() < 1e-6);
        assert!(pts.len() < 10, "{pts:?}");
        assert!(pts.len() >= 3);
        assert!(x0 >= 60.0 && x0 <= 80.0);
    }

    #[test]
    fn minmax_upsample_repeats_nearest() {
        let min = [-0.2f32, -0.8];
        let max = [0.2f32, 0.8];
        let cols = minmax_for_width(&min, &max, 0, 2, 4, 0.0, 4.0);
        assert_eq!(cols.len(), 4);
        assert!((cols[0].0 + 0.2).abs() < 1e-6 && (cols[0].1 - 0.2).abs() < 1e-6);
        assert!((cols[2].0 + 0.8).abs() < 1e-6 && (cols[2].1 - 0.8).abs() < 1e-6);
    }

    #[test]
    fn epoch_bumps_on_finish() {
        let cache = WaveformCache::new();
        cache.insert(
            "3-ch-01-Rytm.wav",
            WaveformLod { peaks: vec![0.2], max_peak: 0.2, ..Default::default() },
        );
        assert_eq!(cache.epoch(), 0);
        cache.note_file_finished("3-ch-01-Rytm.wav");
        assert_eq!(cache.epoch(), 1);
        assert!(cache.get("3-ch-01-Rytm.wav").is_none());
    }

    #[test]
    fn request_returns_before_decode() {
        let dir = tempfile::tempdir().unwrap();
        let frames = 80_000;
        let samples: Vec<f32> = (0..frames).map(|i| ((i % 32) as f32) / 32.0).collect();
        let names = ["a.wav", "b.wav", "c.wav", "d.wav"];
        let paths: Vec<_> = names
            .iter()
            .map(|name| {
                let path = dir.path().join(name);
                crate::write_bounce(&path, 48_000, &samples, &samples).unwrap();
                path
            })
            .collect();

        let cache = WaveformCache::new();
        let start = std::time::Instant::now();
        for (name, path) in names.iter().zip(&paths) {
            cache.request(*name, path);
        }
        assert!(
            start.elapsed().as_millis() < 80,
            "request must not decode on the caller, took {:?}",
            start.elapsed()
        );
        for name in names {
            assert!(
                cache.is_loading(name) || cache.get(name).is_some(),
                "{name} should be queued or already ready"
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        for name in names {
            while cache.get(name).is_none() {
                assert!(std::time::Instant::now() < deadline, "timed out waiting for {name}");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(!cache.is_loading(name));
            let lod = cache.get(name).unwrap();
            assert!(!lod.min.is_empty());
        }
    }

    #[test]
    fn mark_loading_stays_pending() {
        let cache = WaveformCache::new();
        cache.mark_loading("pending.wav");
        assert!(cache.is_loading("pending.wav"));
        assert!(cache.get("pending.wav").is_none());
        assert!(matches!(cache.status("pending.wav"), WaveformStatus::Loading));
    }
}
