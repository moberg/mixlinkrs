//! macOS CoreAudio duplex + thread policy. All unsafe FFI stays here.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod host_time;

#[cfg(target_os = "macos")]
pub mod coreaudio;
#[cfg(target_os = "macos")]
pub mod display_sleep;
#[cfg(target_os = "macos")]
pub mod permission;
#[cfg(target_os = "macos")]
pub mod rt_thread;

#[cfg(not(target_os = "macos"))]
pub mod coreaudio {
    pub use crate::stub::*;
}

#[cfg(not(target_os = "macos"))]
pub mod stub {
    use thiserror::Error;
    #[derive(Debug, Error)]
    pub enum CaError {
        #[error("CoreAudio only on macOS")]
        Unsupported,
    }
    pub struct DeviceInfo {
        pub id: u32,
        pub name: String,
        pub inputs: u32,
        pub outputs: u32,
    }
    impl DeviceInfo {
        pub fn usable(&self) -> bool {
            self.inputs >= 2 && self.outputs >= 2
        }
    }
    pub fn enumerate_devices() -> Vec<DeviceInfo> {
        Vec::new()
    }
    pub struct DeviceStream;
    impl DeviceStream {
        pub fn open_named(
            _contains: &str,
            _frames: Option<u32>,
            _cb: DuplexCb,
        ) -> Result<Self, CaError> {
            Err(CaError::Unsupported)
        }
        pub fn sample_rate(&self) -> u32 {
            48_000
        }
        pub fn buffer_frames(&self) -> u32 {
            128
        }
    }
    pub type DuplexCb =
        Box<dyn FnMut(engine::BufferList, engine::BufferList, Timing) + Send + 'static>;
    #[derive(Clone, Copy, Debug)]
    pub struct Timing {
        pub host_time: u64,
        pub sample_time: i64,
        pub frames: u32,
        pub sample_rate: f64,
    }
}

#[cfg(not(target_os = "macos"))]
pub mod rt_thread {
    pub fn promote_to_realtime(_: u32, _: u32) {}
}

pub use host_time::HostTimeAnchor;
