//! Thin wrapper over `realfft` so the engine doesn't pull the dep directly.
//! FFT planning allocates, so this type must be built on non-RT threads.

use realfft::{num_complex::Complex32, ComplexToReal, RealFftPlanner, RealToComplex};

pub struct RealFft {
    size: usize,
    forward: std::sync::Arc<dyn RealToComplex<f32>>,
    inverse: std::sync::Arc<dyn ComplexToReal<f32>>,
}

impl RealFft {
    pub fn new(size: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(size);
        let inverse = planner.plan_fft_inverse(size);
        Self { size, forward, inverse }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn scratch_len(&self) -> usize {
        self.forward.get_scratch_len().max(self.inverse.get_scratch_len())
    }

    pub fn forward(&self, input: &mut [f32], output: &mut [Complex32], scratch: &mut [Complex32]) {
        // realfft signature is (indata, outdata, scratch)
        self.forward.process_with_scratch(input, output, scratch).expect("realfft forward failed");
    }

    pub fn inverse(&self, input: &mut [Complex32], output: &mut [f32], scratch: &mut [Complex32]) {
        self.inverse.process_with_scratch(input, output, scratch).expect("realfft inverse failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let n = 64;
        let fft = RealFft::new(n);
        let mut input: Vec<f32> = (0..n).map(|i| (i as f32 * 0.1).sin()).collect();
        let original = input.clone();
        let mut spec = vec![Complex32::new(0.0, 0.0); n / 2 + 1];
        let mut scratch = vec![Complex32::new(0.0, 0.0); fft.scratch_len()];
        fft.forward(&mut input, &mut spec, &mut scratch);
        let mut back = vec![0.0f32; n];
        fft.inverse(&mut spec, &mut back, &mut scratch);
        for (a, b) in original.iter().zip(back.iter()) {
            // realfft inverse scales by N; normalise.
            assert!((a - b / n as f32).abs() < 1e-3);
        }
    }
}
