//! Transient / onset detection.
//!
//! A minimal energy-based onset detector suitable for creating warp markers
//! at load time. Spectral-flux detection is a post-v1 upgrade.

use crate::Sample;

/// Find sample indices of transients using a windowed energy novelty function.
///
/// - `samples`: mono signal
/// - `window`: analysis window (power of two, 512–2048)
/// - `hop`: hop size (window/2 typical)
/// - `threshold`: peak sensitivity (0..1)
///
/// Returns a sorted list of sample indices.
pub fn energy_onsets(
    samples: &[Sample],
    window: usize,
    hop: usize,
    threshold: Sample,
) -> Vec<usize> {
    if samples.len() < window || hop == 0 {
        return Vec::new();
    }
    let n = (samples.len() - window) / hop;
    let mut energy = Vec::with_capacity(n);
    for i in 0..n {
        let start = i * hop;
        let mut e = 0.0;
        for k in 0..window {
            let s = samples[start + k];
            e += s * s;
        }
        energy.push((e / window as f32).sqrt());
    }
    // Novelty = positive difference of energy.
    let mut novelty = Vec::with_capacity(n);
    novelty.push(0.0);
    for i in 1..n {
        novelty.push((energy[i] - energy[i - 1]).max(0.0));
    }
    // Peak-pick with local maximum + threshold.
    let max_nov = novelty.iter().cloned().fold(0f32, f32::max).max(1e-9);
    let thresh = threshold * max_nov;
    let mut onsets = Vec::new();
    for i in 2..novelty.len() - 2 {
        let v = novelty[i];
        if v > thresh
            && v > novelty[i - 1]
            && v > novelty[i - 2]
            && v >= novelty[i + 1]
            && v >= novelty[i + 2]
        {
            onsets.push(i * hop);
        }
    }
    onsets
}
