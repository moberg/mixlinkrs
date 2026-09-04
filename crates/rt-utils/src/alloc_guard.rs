//! Lightweight in-scope no-alloc guard.
//!
//! This is a stand-in for the `assert_no_alloc` crate that works without a
//! global allocator override. It uses a thread-local counter: code that must
//! not allocate enters a guard; if anything calls into `ASSERT_NO_ALLOC.check`
//! while the counter is non-zero, it panics. The allocator hook is registered
//! by the app binary (see `GlobalAllocWrapper` in the `app` crate).

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

thread_local! {
    static NO_ALLOC_DEPTH: Cell<u32> = const { Cell::new(0) };
}

static VIOLATIONS: AtomicU64 = AtomicU64::new(0);
static PANIC_ON_VIOLATION: AtomicBool = AtomicBool::new(cfg!(debug_assertions));

/// RAII guard: while alive, `check()` returns false (i.e. allocation is forbidden).
pub struct NoAllocGuard {
    _priv: (),
}

impl NoAllocGuard {
    pub fn new() -> Self {
        NO_ALLOC_DEPTH.with(|d| d.set(d.get() + 1));
        Self { _priv: () }
    }
}

impl Default for NoAllocGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NoAllocGuard {
    fn drop(&mut self) {
        NO_ALLOC_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Called from the global allocator. Returns true when alloc is allowed.
pub fn is_alloc_allowed() -> bool {
    NO_ALLOC_DEPTH.with(|d| d.get() == 0)
}

/// Records an allocation attempt while inside a `NoAllocGuard`.
/// When `PANIC_ON_VIOLATION` is set, panics; otherwise increments a counter
/// so CI can assert a clean run.
pub fn report_violation() {
    VIOLATIONS.fetch_add(1, Ordering::Relaxed);
    if PANIC_ON_VIOLATION.load(Ordering::Relaxed) {
        // Use a bare eprintln! rather than panic! to avoid recursion through
        // the panic unwinder which itself allocates.
        eprintln!("[rt-utils] allocation inside NoAllocGuard");
        // Best effort: avoid unwind path.
        std::process::abort();
    }
}

pub fn violations() -> u64 {
    VIOLATIONS.load(Ordering::Relaxed)
}

pub fn set_panic_on_violation(on: bool) {
    PANIC_ON_VIOLATION.store(on, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nesting_counts() {
        assert!(is_alloc_allowed());
        let g1 = NoAllocGuard::new();
        assert!(!is_alloc_allowed());
        {
            let _g2 = NoAllocGuard::new();
            assert!(!is_alloc_allowed());
        }
        assert!(!is_alloc_allowed());
        drop(g1);
        assert!(is_alloc_allowed());
    }
}
