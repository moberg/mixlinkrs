//! Duplex CoreAudio IOProc. MixLink model: input + output on one device.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use coreaudio_sys::{
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyDeviceNameCFString,
    kAudioDevicePropertyStreamConfiguration,
    kAudioDevicePropertyStreamFormat, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyElementMaster, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
    AudioBufferList, AudioDeviceCreateIOProcID, AudioDeviceDestroyIOProcID, AudioDeviceIOProcID,
    AudioDeviceID, AudioDeviceStart, AudioDeviceStop, AudioObjectGetPropertyData,
    AudioObjectGetPropertyDataSize, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, AudioStreamBasicDescription, AudioTimeStamp, CFStringRef, OSStatus,
};
use thiserror::Error;

use engine::{AudioBuf, BufferList};

use crate::host_time::HostTimeAnchor;

pub const DEFAULT_BUFFER_FRAMES: u32 = 128;

#[derive(Debug, Error)]
pub enum CaError {
    #[error("CoreAudio error: OSStatus={0}")]
    Os(OSStatus),
    #[error("no device matching '{0}'")]
    NoDevice(String),
    #[error("device is not duplex (in={inputs}, out={outputs})")]
    NotDuplex { inputs: u32, outputs: u32 },
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub id: AudioDeviceID,
    pub name: String,
    pub inputs: u32,
    pub outputs: u32,
}

impl DeviceInfo {
    pub fn usable(&self) -> bool {
        self.inputs >= 2 && self.outputs >= 2
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub host_time: u64,
    pub sample_time: i64,
    pub frames: u32,
    pub sample_rate: f64,
}

pub type DuplexCb = Box<dyn FnMut(BufferList<'_>, BufferList<'_>, Timing) + Send + 'static>;

/// RME is typically one interleaved buffer; other devices are non-interleaved.
/// 32 is well above any Fireface stream count we walk with `locateChannel`.
const MAX_CA_BUFS: usize = 32;

struct StreamState {
    cb: Mutex<DuplexCb>,
    anchor: HostTimeAnchor,
    in_bufs: UnsafeCell<[AudioBuf; MAX_CA_BUFS]>,
    out_bufs: UnsafeCell<[AudioBuf; MAX_CA_BUFS]>,
    buffer_frames: u32,
    sample_rate_u32: u32,
    xruns: AtomicU64,
    rt_promoted: AtomicBool,
}

/// IOProc is the only writer of the scratch arrays.
unsafe impl Send for StreamState {}
unsafe impl Sync for StreamState {}

pub struct DeviceStream {
    device: AudioDeviceID,
    proc_id: AudioDeviceIOProcID,
    state: Arc<StreamState>,
    started: AtomicBool,
}

impl DeviceStream {
    pub fn open_named(contains: &str, frames: Option<u32>, cb: DuplexCb) -> Result<Self, CaError> {
        let devices = enumerate_devices();
        let needle = contains.to_ascii_lowercase();
        let info = devices
            .into_iter()
            .filter(DeviceInfo::usable)
            .find(|d| d.name.to_ascii_lowercase().contains(&needle))
            .or_else(|| {
                default_duplex().ok().and_then(|id| {
                    enumerate_devices().into_iter().find(|d| d.id == id && d.usable())
                })
            })
            .ok_or_else(|| CaError::NoDevice(contains.into()))?;
        if !info.usable() {
            return Err(CaError::NotDuplex { inputs: info.inputs, outputs: info.outputs });
        }
        open_device(info.id, frames, cb)
    }

    pub fn sample_rate(&self) -> u32 {
        self.state.sample_rate_u32
    }

    pub fn buffer_frames(&self) -> u32 {
        self.state.buffer_frames
    }

    pub fn xruns(&self) -> u64 {
        self.state.xruns.load(Ordering::Relaxed)
    }
}

impl Drop for DeviceStream {
    fn drop(&mut self) {
        if self.started.load(Ordering::Relaxed) {
            unsafe {
                let _ = AudioDeviceStop(self.device, self.proc_id);
                let _ = AudioDeviceDestroyIOProcID(self.device, self.proc_id);
            }
            self.started.store(false, Ordering::Relaxed);
        }
        let raw = Arc::as_ptr(&self.state);
        unsafe {
            Arc::decrement_strong_count(raw);
        }
    }
}

fn open_device(device: AudioDeviceID, frames: Option<u32>, cb: DuplexCb) -> Result<DeviceStream, CaError> {
    let format = get_stream_format(device, kAudioObjectPropertyScopeOutput)?;
    let sample_rate = format.mSampleRate;
    if let Some(f) = frames {
        let _ = set_buffer_frame_size(device, f.clamp(32, 4096));
    }
    let actual = get_buffer_frame_size(device).unwrap_or(128);
    let state = Arc::new(StreamState {
        cb: Mutex::new(cb),
        anchor: HostTimeAnchor::new(sample_rate),
        in_bufs: UnsafeCell::new([AudioBuf { data: std::ptr::null_mut(), channels: 0, frames: 0 }; MAX_CA_BUFS]),
        out_bufs: UnsafeCell::new([AudioBuf { data: std::ptr::null_mut(), channels: 0, frames: 0 }; MAX_CA_BUFS]),
        buffer_frames: actual,
        sample_rate_u32: sample_rate as u32,
        xruns: AtomicU64::new(0),
        rt_promoted: AtomicBool::new(false),
    });
    let user = Arc::as_ptr(&state) as *mut c_void;
    let mut proc_id: AudioDeviceIOProcID = None;
    let status = unsafe { AudioDeviceCreateIOProcID(device, Some(io_proc), user, &mut proc_id) };
    if status != 0 {
        return Err(CaError::Os(status));
    }
    std::mem::forget(state.clone());
    let status = unsafe { AudioDeviceStart(device, proc_id) };
    if status != 0 {
        unsafe {
            let _ = AudioDeviceDestroyIOProcID(device, proc_id);
        }
        return Err(CaError::Os(status));
    }
    log::info!(
        "audio: {} Hz, {} frame buffer, device {device}",
        sample_rate as u32,
        actual
    );
    Ok(DeviceStream { device, proc_id, state, started: AtomicBool::new(true) })
}

extern "C" fn io_proc(
    _device: AudioObjectID,
    _now: *const AudioTimeStamp,
    in_data: *const AudioBufferList,
    _in_time: *const AudioTimeStamp,
    out_data: *mut AudioBufferList,
    out_time: *const AudioTimeStamp,
    client: *mut c_void,
) -> OSStatus {
    let state = unsafe { &*(client as *const StreamState) };
    if !state.rt_promoted.swap(true, Ordering::Relaxed) {
        crate::rt_thread::promote_to_realtime(state.buffer_frames, state.sample_rate_u32);
    }
    let (host_time, sample_time) = unsafe {
        if out_time.is_null() {
            (crate::host_time::now_host_time(), 0)
        } else {
            ((*out_time).mHostTime as u64, (*out_time).mSampleTime as i64)
        }
    };
    state.anchor.publish(host_time, sample_time);

    let in_slot = unsafe { &mut *state.in_bufs.get() };
    let out_slot = unsafe { &mut *state.out_bufs.get() };
    let n_in = fill_bufs(in_slot, in_data);
    let n_out = fill_bufs(out_slot, out_data as *const AudioBufferList);
    let frames = out_slot[..n_out]
        .first()
        .or(in_slot[..n_in].first())
        .map(|b| b.frames)
        .unwrap_or(0);
    if frames == 0 {
        return 0;
    }
    let timing = Timing {
        host_time,
        sample_time,
        frames,
        sample_rate: state.sample_rate_u32 as f64,
    };
    let input = BufferList { buffers: &in_slot[..n_in] };
    let output = BufferList { buffers: &out_slot[..n_out] };
    if let Ok(mut guard) = state.cb.try_lock() {
        (guard)(input, output, timing);
    } else {
        state.xruns.fetch_add(1, Ordering::Relaxed);
    }
    0
}

fn fill_bufs(dst: &mut [AudioBuf], list: *const AudioBufferList) -> usize {
    if list.is_null() {
        return 0;
    }
    let list = unsafe { &*list };
    let n = (list.mNumberBuffers as usize).min(dst.len());
    let first = list.mBuffers.as_ptr();
    for i in 0..n {
        let b = unsafe { &*first.add(i) };
        let ch = b.mNumberChannels;
        let bytes = b.mDataByteSize as usize;
        let frames = if ch == 0 { 0 } else { bytes / std::mem::size_of::<f32>() / ch as usize };
        dst[i] = AudioBuf { data: b.mData as *mut f32, channels: ch, frames: frames as u32 };
    }
    n
}

pub fn enumerate_devices() -> Vec<DeviceInfo> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut size = 0u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr, 0, std::ptr::null(), &mut size)
    };
    if status != 0 || size == 0 {
        return Vec::new();
    }
    let count = size as usize / std::mem::size_of::<AudioDeviceID>();
    let mut ids = vec![0u32; count];
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr() as *mut c_void,
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.into_iter()
        .map(|id| DeviceInfo {
            id,
            name: device_name(id).unwrap_or_else(|| format!("Device {id}")),
            inputs: channel_count(id, kAudioObjectPropertyScopeInput),
            outputs: channel_count(id, kAudioObjectPropertyScopeOutput),
        })
        .collect()
}

fn default_duplex() -> Result<AudioDeviceID, CaError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioHardwarePropertyDefaultOutputDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut id: AudioDeviceID = 0;
    let mut size = std::mem::size_of::<AudioDeviceID>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            kAudioObjectSystemObject,
            &addr,
            0,
            std::ptr::null(),
            &mut size,
            &mut id as *mut _ as *mut c_void,
        )
    };
    if status != 0 || id == 0 {
        let _ = kAudioHardwarePropertyDefaultInputDevice;
        return Err(CaError::Os(status));
    }
    Ok(id)
}

fn device_name(id: AudioDeviceID) -> Option<String> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyDeviceNameCFString,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut cf: CFStringRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(id, &addr, 0, std::ptr::null(), &mut size, &mut cf as *mut _ as *mut c_void)
    };
    if status != 0 || cf.is_null() {
        return None;
    }
    Some(cfstring_to_string(cf))
}

fn cfstring_to_string(cf: CFStringRef) -> String {
    use core_foundation_sys::string::{
        kCFStringEncodingUTF8, CFStringGetCString, CFStringGetLength,
        CFStringGetMaximumSizeForEncoding,
    };
    unsafe {
        let cf = cf as core_foundation_sys::string::CFStringRef;
        let len = CFStringGetLength(cf);
        let max = CFStringGetMaximumSizeForEncoding(len, kCFStringEncodingUTF8) + 1;
        let mut buf = vec![0i8; max as usize];
        if CFStringGetCString(cf, buf.as_mut_ptr(), max, kCFStringEncodingUTF8) != 0 {
            let bytes = std::slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len());
            let nul = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            String::from_utf8_lossy(&bytes[..nul]).into_owned()
        } else {
            String::new()
        }
    }
}

fn channel_count(id: AudioDeviceID, scope: u32) -> u32 {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamConfiguration,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut size = 0u32;
    let status = unsafe { AudioObjectGetPropertyDataSize(id, &addr, 0, std::ptr::null(), &mut size) };
    if status != 0 || size == 0 {
        return 0;
    }
    let mut buf = vec![0u8; size as usize];
    let status = unsafe {
        AudioObjectGetPropertyData(id, &addr, 0, std::ptr::null(), &mut size, buf.as_mut_ptr() as *mut c_void)
    };
    if status != 0 {
        return 0;
    }
    let list = unsafe { &*(buf.as_ptr() as *const AudioBufferList) };
    let n = list.mNumberBuffers as usize;
    let first = list.mBuffers.as_ptr();
    let mut total = 0u32;
    for i in 0..n {
        total += unsafe { (*first.add(i)).mNumberChannels };
    }
    total
}

fn get_stream_format(device: AudioDeviceID, scope: u32) -> Result<AudioStreamBasicDescription, CaError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyStreamFormat,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut asbd: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(device, &addr, 0, std::ptr::null(), &mut size, &mut asbd as *mut _ as *mut c_void)
    };
    if status != 0 {
        return Err(CaError::Os(status));
    }
    Ok(asbd)
}

fn set_buffer_frame_size(device: AudioDeviceID, frames: u32) -> Result<(), CaError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyBufferFrameSize,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut f = frames;
    let size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectSetPropertyData(device, &addr, 0, std::ptr::null(), size, &mut f as *mut _ as *mut c_void)
    };
    if status != 0 {
        Err(CaError::Os(status))
    } else {
        Ok(())
    }
}

fn get_buffer_frame_size(device: AudioDeviceID) -> Result<u32, CaError> {
    let addr = AudioObjectPropertyAddress {
        mSelector: kAudioDevicePropertyBufferFrameSize,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMaster,
    };
    let mut f = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(device, &addr, 0, std::ptr::null(), &mut size, &mut f as *mut _ as *mut c_void)
    };
    if status != 0 {
        Err(CaError::Os(status))
    } else {
        Ok(f)
    }
}
