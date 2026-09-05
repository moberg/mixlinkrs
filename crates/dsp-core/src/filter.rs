//! Filters: State Variable (SVF) and a biquad utility.

use crate::Sample;
use std::f32::consts::PI;

/// A Chamberlin-style State Variable Filter (TPT form).
/// Produces lowpass, highpass and bandpass outputs in parallel.
#[derive(Clone, Copy, Debug)]
pub struct Svf {
    // state
    ic1eq: Sample,
    ic2eq: Sample,
    // coefficients
    g: Sample,
    k: Sample,
    a1: Sample,
    a2: Sample,
    a3: Sample,
}

#[derive(Clone, Copy, Debug)]
pub struct SvfOut {
    pub lp: Sample,
    pub bp: Sample,
    pub hp: Sample,
}

impl Svf {
    pub fn new() -> Self {
        Self { ic1eq: 0.0, ic2eq: 0.0, g: 0.0, k: 0.0, a1: 0.0, a2: 0.0, a3: 0.0 }
    }

    /// Set cutoff Hz and resonance Q (> 0.5 typical).
    pub fn set(&mut self, cutoff_hz: Sample, q: Sample, sample_rate: Sample) {
        let cutoff = cutoff_hz.clamp(10.0, 0.49 * sample_rate);
        self.g = (PI * cutoff / sample_rate).tan();
        self.k = 1.0 / q.max(0.01);
        self.a1 = 1.0 / (1.0 + self.g * (self.g + self.k));
        self.a2 = self.g * self.a1;
        self.a3 = self.g * self.a2;
    }

    #[inline]
    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: Sample) -> SvfOut {
        let v3 = x - self.ic2eq;
        let v1 = self.a1 * self.ic1eq + self.a2 * v3;
        let v2 = self.ic2eq + self.a2 * self.ic1eq + self.a3 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;
        SvfOut { lp: v2, bp: v1, hp: x - self.k * v1 - v2 }
    }
}

impl Default for Svf {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svf_stable_dc() {
        let mut f = Svf::new();
        f.set(1_000.0, 0.707, 48_000.0);
        let mut y = 0.0;
        for _ in 0..10_000 {
            y = f.process(1.0).lp;
        }
        assert!((y - 1.0).abs() < 1e-2);
    }
}
