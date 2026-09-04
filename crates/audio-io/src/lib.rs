//! Thin safe wrapper around the duplex CoreAudio stream.

#[cfg(target_os = "macos")]
pub use platform_mac::coreaudio::{
    enumerate_devices, CaError, DeviceInfo, DeviceStream, DuplexCb, Timing, DEFAULT_BUFFER_FRAMES,
};

#[cfg(not(target_os = "macos"))]
pub use platform_mac::coreaudio::{
    enumerate_devices, CaError, DeviceInfo, DeviceStream, DuplexCb, Timing,
};

#[cfg(not(target_os = "macos"))]
pub const DEFAULT_BUFFER_FRAMES: u32 = 128;
