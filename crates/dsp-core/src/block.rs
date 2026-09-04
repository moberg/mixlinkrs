//! Block-level buffer helpers. These operate on pre-allocated slices and never
//! allocate themselves, so they are safe to call from the RT thread.

use crate::Sample;

/// In-place gain.
#[inline]
pub fn apply_gain(buf: &mut [Sample], gain: Sample) {
    for s in buf.iter_mut() {
        *s *= gain;
    }
}

/// In-place linear gain ramp from `from` to `to` across the slice.
#[inline]
pub fn apply_gain_ramp(buf: &mut [Sample], from: Sample, to: Sample) {
    let n = buf.len();
    if n == 0 {
        return;
    }
    let inv = 1.0 / n as Sample;
    for (i, s) in buf.iter_mut().enumerate() {
        let t = i as Sample * inv;
        *s *= from + (to - from) * t;
    }
}

/// Accumulate `src` into `dst` with gain: `dst[i] += src[i] * gain`.
#[inline]
pub fn add_scaled(dst: &mut [Sample], src: &[Sample], gain: Sample) {
    let n = dst.len().min(src.len());
    for i in 0..n {
        dst[i] += src[i] * gain;
    }
}

/// Copy with scale.
#[inline]
pub fn copy_scaled(dst: &mut [Sample], src: &[Sample], gain: Sample) {
    let n = dst.len().min(src.len());
    for i in 0..n {
        dst[i] = src[i] * gain;
    }
}

/// Zero a buffer.
#[inline]
pub fn clear(buf: &mut [Sample]) {
    for s in buf.iter_mut() {
        *s = 0.0;
    }
}

/// Peak absolute value.
#[inline]
pub fn peak_abs(buf: &[Sample]) -> Sample {
    let mut m = 0.0;
    for &s in buf {
        let a = s.abs();
        if a > m {
            m = a;
        }
    }
    m
}

/// Mean-square (not square-root-taken).
#[inline]
pub fn mean_sq(buf: &[Sample]) -> Sample {
    if buf.is_empty() {
        return 0.0;
    }
    let mut acc = 0.0;
    for &s in buf {
        acc += s * s;
    }
    acc / buf.len() as Sample
}

/// A simple one-pole parameter smoother.
#[derive(Clone, Copy, Debug)]
pub struct Smoother {
    pub value: Sample,
    pub target: Sample,
    pub coef: Sample,
}

impl Smoother {
    /// Build a smoother with a time constant in milliseconds at a given sample rate.
    pub fn new(initial: Sample, ms: Sample, sample_rate: Sample) -> Self {
        let tau = (ms * 0.001).max(1e-5);
        let coef = 1.0 - (-1.0 / (tau * sample_rate)).exp();
        Self { value: initial, target: initial, coef }
    }

    #[inline]
    pub fn set_target(&mut self, t: Sample) {
        self.target = t;
    }

    #[inline]
    pub fn set_immediate(&mut self, v: Sample) {
        self.value = v;
        self.target = v;
    }

    #[inline]
    pub fn step(&mut self) -> Sample {
        self.value += self.coef * (self.target - self.value);
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_ramp_endpoints() {
        let mut b = [1.0f32; 64];
        apply_gain_ramp(&mut b, 0.0, 1.0);
        assert!((b[0] - 0.0).abs() < 1e-6);
        assert!(b[63] > 0.9);
    }

    #[test]
    fn smoother_approaches_target() {
        let mut s = Smoother::new(0.0, 5.0, 48_000.0);
        s.set_target(1.0);
        for _ in 0..10_000 {
            s.step();
        }
        assert!((s.value - 1.0).abs() < 1e-3);
    }
}
