//! Thin wrappers that make atomic parameter storage ergonomic for the RT thread.

pub use atomic_float::AtomicF32;
pub use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[inline]
pub fn load_relaxed_f32(a: &AtomicF32) -> f32 {
    a.load(Ordering::Relaxed)
}

#[inline]
pub fn store_relaxed_f32(a: &AtomicF32, v: f32) {
    a.store(v, Ordering::Relaxed);
}
