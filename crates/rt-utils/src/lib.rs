//! Real-time utilities. Every type here is designed to be safe to use on the
//! audio callback thread: no heap allocation, no locks, no syscalls except for
//! `mach_absolute_time` via `libc::clock_gettime`.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod alloc_guard;
pub mod atomics;
pub mod host_time;
pub mod memory;
pub mod spsc;
pub mod vec;

pub use alloc_guard::NoAllocGuard;
pub use spsc::{spsc_bounded, Consumer, Producer};
pub use vec::RtVec;

/// Compile-time non-negative assertion — used sparingly.
pub const fn _const_assert(cond: bool) {
    assert!(cond);
}
