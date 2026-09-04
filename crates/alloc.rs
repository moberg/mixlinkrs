//! Global allocator wrapper that checks `rt_utils::alloc_guard` on every
//! allocation. When the current thread is inside a `NoAllocGuard`, allocation
//! counts as a violation.
//!
//! Enabled only in debug builds by default; release builds fall through to the
//! system allocator with no overhead.

#[cfg(debug_assertions)]
mod impl_ {
    use std::alloc::{GlobalAlloc, Layout, System};

    pub struct CheckedAllocator;

    unsafe impl GlobalAlloc for CheckedAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if !rt_utils::alloc_guard::is_alloc_allowed() {
                rt_utils::alloc_guard::report_violation();
            }
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            if !rt_utils::alloc_guard::is_alloc_allowed() {
                rt_utils::alloc_guard::report_violation();
            }
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            if !rt_utils::alloc_guard::is_alloc_allowed() {
                rt_utils::alloc_guard::report_violation();
            }
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static ALLOC: CheckedAllocator = CheckedAllocator;
}

pub fn install_global_hook() {
    // Enabling the allocator panic in debug builds is informative; release builds
    // keep the counter available but don't abort the process.
    rt_utils::alloc_guard::set_panic_on_violation(cfg!(debug_assertions));
}
