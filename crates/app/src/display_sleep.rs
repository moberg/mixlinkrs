//! Display sleep / wake — MixLink Device button.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static FORCED_OFF: AtomicBool = AtomicBool::new(false);
/// millis since epoch when sleep was requested — `CGDisplayIsAsleep` lags.
static SLEEP_AT_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn is_asleep() -> bool {
    if FORCED_OFF.load(Ordering::Relaxed) {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        return platform_mac::display_sleep::display_is_asleep();
    }
    #[cfg(not(target_os = "macos"))]
    false
}

pub fn toggle() {
    if is_asleep() {
        wake();
    } else {
        sleep();
    }
}

pub fn sleep() {
    FORCED_OFF.store(true, Ordering::Relaxed);
    SLEEP_AT_MS.store(now_ms(), Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    platform_mac::display_sleep::request_sleep();
}

pub fn wake() {
    FORCED_OFF.store(false, Ordering::Relaxed);
    SLEEP_AT_MS.store(0, Ordering::Relaxed);
    #[cfg(target_os = "macos")]
    platform_mac::display_sleep::request_wake();
}

/// MixLink `screensDidWake` / `noteExternalWake` — keyboard or lid, not Device.
pub fn poll_external_wake() {
    if !FORCED_OFF.load(Ordering::Relaxed) {
        return;
    }
    let at = SLEEP_AT_MS.load(Ordering::Relaxed);
    if at == 0 || now_ms().saturating_sub(at) < 2_000 {
        return;
    }
    #[cfg(target_os = "macos")]
    if !platform_mac::display_sleep::display_is_asleep() {
        note_external_wake();
    }
}

pub fn note_external_wake() {
    FORCED_OFF.store(false, Ordering::Relaxed);
    SLEEP_AT_MS.store(0, Ordering::Relaxed);
}
