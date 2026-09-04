//! Memory residency helpers.

/// Write-touch every page in a slice so it is resident and not copy-on-write.
/// Call from non-RT code before entering the audio callback.
pub fn touch_pages<T: Copy + Default>(s: &mut [T]) {
    for x in s.iter_mut() {
        *x = T::default();
    }
}

/// Best-effort `mlockall` on current and future pages.
/// Returns Ok(()) on success, Err(errno) otherwise (never panics).
#[cfg(target_os = "macos")]
pub fn mlockall_best_effort() -> Result<(), i32> {
    // SAFETY: FFI call with no preconditions; we read errno on failure.
    let rc = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) };
    if rc == 0 {
        Ok(())
    } else {
        // SAFETY: reading errno.
        let err = unsafe { *libc::__error() };
        Err(err)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn mlockall_best_effort() -> Result<(), i32> {
    Err(-1)
}
