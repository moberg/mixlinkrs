//! MixLink `DisplaySleep`: `pmset displaysleepnow` and IOPM user-activity wake.
//!
//! On recent macOS `IODisplayWrangler` / `IORequestIdle` is ignored, which looks
//! like the Device button does nothing. Prefer `pmset`, matching MixLink.

use std::process::{Command, Stdio};

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

const IOPM_USER_ACTIVE_LOCAL: u32 = 0;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayIsAsleep(display: u32) -> u32;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMAssertionDeclareUserActivity(
        name: CFStringRef,
        activity_type: u32,
        assertion_id: *mut u32,
    ) -> i32;
}

/// Hardware display is actually off. Lags a moment after `pmset`.
#[must_use]
pub fn display_is_asleep() -> bool {
    unsafe { CGDisplayIsAsleep(CGMainDisplayID()) != 0 }
}

/// MixLink `Process.run` — do not wait; waiting + a Metal present wakes the panel.
pub fn request_sleep() {
    spawn("/usr/bin/pmset", &["displaysleepnow"]);
}

pub fn request_wake() {
    let name = CFString::new("MixLink display wake");
    let mut assertion = 0u32;
    unsafe {
        let _ = IOPMAssertionDeclareUserActivity(
            name.as_concrete_TypeRef(),
            IOPM_USER_ACTIVE_LOCAL,
            &mut assertion,
        );
    }
    spawn("/usr/bin/caffeinate", &["-u", "-t", "1"]);
}

fn spawn(path: &str, args: &[&str]) {
    match Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => log::info!("{path} {}", args.join(" ")),
        Err(e) => log::warn!("{path}: {e}"),
    }
}
