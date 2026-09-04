//! Display sleep / wake — MixLink Device button.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

static FORCED_OFF: AtomicBool = AtomicBool::new(false);

pub fn is_asleep() -> bool {
    FORCED_OFF.load(Ordering::Relaxed)
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
    let _ = Command::new("/usr/bin/pmset").arg("displaysleepnow").status();
}

pub fn wake() {
    FORCED_OFF.store(false, Ordering::Relaxed);
    // MixLink uses IOPMAssertionDeclareUserActivity; caffeinate -u is the CLI equivalent.
    let _ = Command::new("/usr/bin/caffeinate").args(["-u", "-t", "1"]).status();
}
