//! Simple ADSR envelope.

use crate::Sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdsrStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub attack_s: Sample,
    pub decay_s: Sample,
    pub sustain: Sample, // 0..1
    pub release_s: Sample,
    level: Sample,
    stage: AdsrStage,
    sample_rate: Sample,
}

impl Adsr {
    pub fn new(sample_rate: Sample) -> Self {
        Self {
            attack_s: 0.005,
            decay_s: 0.1,
            sustain: 0.7,
            release_s: 0.2,
            level: 0.0,
            stage: AdsrStage::Idle,
            sample_rate,
        }
    }

    pub fn gate_on(&mut self) {
        self.stage = AdsrStage::Attack;
    }

    pub fn gate_off(&mut self) {
        if self.stage != AdsrStage::Idle {
            self.stage = AdsrStage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != AdsrStage::Idle
    }

    #[inline]
    pub fn step(&mut self) -> Sample {
        let sr = self.sample_rate;
        match self.stage {
            AdsrStage::Idle => {
                self.level = 0.0;
            }
            AdsrStage::Attack => {
                let k = 1.0 / (self.attack_s.max(1e-4) * sr);
                self.level += k * (1.05 - self.level); // exponential-ish approach
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = AdsrStage::Decay;
                }
            }
            AdsrStage::Decay => {
                let k = 1.0 / (self.decay_s.max(1e-4) * sr);
                let target = self.sustain;
                self.level += k * (target - self.level);
                if (self.level - target).abs() < 1e-4 {
                    self.level = target;
                    self.stage = AdsrStage::Sustain;
                }
            }
            AdsrStage::Sustain => {
                self.level = self.sustain;
            }
            AdsrStage::Release => {
                let k = 1.0 / (self.release_s.max(1e-4) * sr);
                self.level -= k * self.level;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = AdsrStage::Idle;
                }
            }
        }
        self.level
    }

    pub fn stage(&self) -> AdsrStage {
        self.stage
    }

    pub fn reset(&mut self) {
        self.level = 0.0;
        self.stage = AdsrStage::Idle;
    }
}
