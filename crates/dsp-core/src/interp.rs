//! Interpolation helpers for resampling and granular playback.

use crate::Sample;

/// Linear interpolation into a buffer at fractional index `pos`.
#[inline]
pub fn lerp(buf: &[Sample], pos: f64) -> Sample {
    if buf.is_empty() {
        return 0.0;
    }
    let i = pos.floor() as isize;
    let f = (pos - i as f64) as Sample;
    let i0 = i.max(0) as usize;
    let i1 = (i + 1).max(0) as usize;
    if i0 >= buf.len() {
        return 0.0;
    }
    let a = buf[i0];
    let b = if i1 < buf.len() { buf[i1] } else { 0.0 };
    a + (b - a) * f
}

/// 4-point Lagrange interpolation for better quality resampling.
#[inline]
pub fn lagrange4(buf: &[Sample], pos: f64) -> Sample {
    if buf.len() < 4 {
        return lerp(buf, pos);
    }
    let i = pos.floor() as isize;
    let t = (pos - i as f64) as Sample;
    let idx = |k: isize| -> Sample {
        let j = (i + k).clamp(0, buf.len() as isize - 1) as usize;
        buf[j]
    };
    let x0 = idx(-1);
    let x1 = idx(0);
    let x2 = idx(1);
    let x3 = idx(2);
    // Lagrange over points [-1, 0, 1, 2]
    let t2 = t * t;
    let t3 = t2 * t;
    let c0 = x1;
    let c1 = 0.5 * (x2 - x0);
    let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
    let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
    c0 + c1 * t + c2 * t2 + c3 * t3
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lerp_endpoints() {
        let b = [0.0, 1.0, 2.0, 3.0];
        assert!((lerp(&b, 0.0) - 0.0).abs() < 1e-6);
        assert!((lerp(&b, 2.5) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn lagrange_monotonic_on_linear() {
        let b: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let y = lagrange4(&b, 5.25);
        assert!((y - 5.25).abs() < 1e-4);
    }
}
