//! mach host-time → sample-time conversion anchor.
//!
//! The RT IOProc refreshes the anchor every callback. Other threads use
//! `host_to_sample()` to map external timestamps (CoreMIDI, clock) onto the
//! audio sample clock.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

#[cfg(target_os = "macos")]
pub fn mach_sec_per_tick() -> f64 {
    use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};
    let mut info = mach_timebase_info_data_t { numer: 0, denom: 0 };
    // SAFETY: read-only syscall.
    let rc = unsafe { mach_timebase_info(&mut info) };
    if rc != 0 || info.denom == 0 {
        return 1e-9;
    }
    (info.numer as f64) / (info.denom as f64) * 1e-9
}

#[cfg(not(target_os = "macos"))]
pub fn mach_sec_per_tick() -> f64 {
    1e-9
}

#[cfg(target_os = "macos")]
pub fn now_host_time() -> u64 {
    use mach2::mach_time::mach_absolute_time;
    // SAFETY: read-only syscall.
    unsafe { mach_absolute_time() }
}

#[cfg(not(target_os = "macos"))]
pub fn now_host_time() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

pub struct HostTimeAnchor {
    host_time: AtomicU64,
    sample_time: AtomicI64,
    generation: AtomicU64,
    sample_rate: f64,
    mach_sec_per_tick: f64,
}

impl HostTimeAnchor {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            host_time: AtomicU64::new(0),
            sample_time: AtomicI64::new(0),
            generation: AtomicU64::new(0),
            sample_rate,
            mach_sec_per_tick: mach_sec_per_tick(),
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn publish(&self, host_time: u64, sample_time: i64) {
        self.generation.fetch_add(1, Ordering::Release);
        self.host_time.store(host_time, Ordering::Release);
        self.sample_time.store(sample_time, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
    }

    pub fn host_to_sample(&self, host_time: u64) -> Option<i64> {
        loop {
            let g1 = self.generation.load(Ordering::Acquire);
            if g1 & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let h = self.host_time.load(Ordering::Acquire);
            let s = self.sample_time.load(Ordering::Acquire);
            let g2 = self.generation.load(Ordering::Acquire);
            if g1 == g2 {
                if h == 0 {
                    return None;
                }
                let dh = host_time as i128 - h as i128;
                let dsec = dh as f64 * self.mach_sec_per_tick;
                let dsamples = (dsec * self.sample_rate).round() as i64;
                return Some(s + dsamples);
            }
        }
    }
}
