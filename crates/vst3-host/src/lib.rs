//! Thin Rust wrapper around MixLink's Objective-C++ VST3 bridge.
//!
//! Do not rewrite the SDK host. Load/unload/editor/state stay on the UI thread.
//! `process` / `process_instance` are IOProc-safe (C++, no ObjC).

pub const SLOT_COUNT: u32 = 8;

#[repr(C)]
pub struct MixLinkVST3Instance {
    _private: [u8; 0],
}

pub type MixLinkVST3Ref = *mut MixLinkVST3Instance;

extern "C" {
    pub fn MixLinkVST3Process(
        slot: u32,
        in_l: *const f32,
        in_r: *const f32,
        out_l: *mut f32,
        out_r: *mut f32,
        frames: u32,
    );
    pub fn MixLinkVST3ProcessInstance(
        instance: MixLinkVST3Ref,
        bypassed: i32,
        in_l: *const f32,
        in_r: *const f32,
        out_l: *mut f32,
        out_r: *mut f32,
        frames: u32,
    );
    pub fn MixLinkVST3RetireInstance(instance: MixLinkVST3Ref);
    pub fn MixLinkVST3SlotExchange(slot: u32, next: MixLinkVST3Ref) -> MixLinkVST3Ref;
    pub fn MixLinkVST3SlotSetBypass(slot: u32, bypassed: i32);
    pub fn MixLinkVST3Unload(instance: MixLinkVST3Ref);
    pub fn MixLinkVST3Load(
        bundle_path: *const std::ffi::c_void,
        class_uid: *const std::ffi::c_void,
        sample_rate: f64,
        max_block: u32,
        error: *mut *const std::ffi::c_void,
    ) -> MixLinkVST3Ref;
    pub fn MixLinkVST3ShowEditor(instance: MixLinkVST3Ref, title: *const std::ffi::c_void);
    pub fn MixLinkVST3CloseEditor(instance: MixLinkVST3Ref);
    pub fn MixLinkVST3HasEditor(instance: MixLinkVST3Ref) -> i32;
    pub fn MixLinkVST3SetTempo(instance: MixLinkVST3Ref, bpm: f64);
    pub fn MixLinkVST3ScanPlugins() -> *const std::ffi::c_void;
    pub fn MixLinkVST3SaveState(instance: MixLinkVST3Ref) -> *const std::ffi::c_void;
    pub fn MixLinkVST3RestoreState(
        instance: MixLinkVST3Ref,
        state: *const std::ffi::c_void,
    ) -> i32;
    pub fn MixLinkVST3SetStateDirtyHandler(
        fn_ptr: Option<extern "C" fn(MixLinkVST3Ref, i32)>,
    );
}

/// IOProc: process a published send slot. Lock-free.
pub fn process_slot(slot: u32, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
    let n = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len()) as u32;
    unsafe {
        MixLinkVST3Process(
            slot,
            in_l.as_ptr(),
            in_r.as_ptr(),
            out_l.as_mut_ptr(),
            out_r.as_mut_ptr(),
            n,
        );
    }
}

/// IOProc: process a mix insert instance. Pair with [`retire_instance`].
pub fn process_instance(
    instance: MixLinkVST3Ref,
    bypassed: bool,
    in_l: &[f32],
    in_r: &[f32],
    out_l: &mut [f32],
    out_r: &mut [f32],
) {
    let n = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len()) as u32;
    unsafe {
        MixLinkVST3ProcessInstance(
            instance,
            i32::from(bypassed),
            in_l.as_ptr(),
            in_r.as_ptr(),
            out_l.as_mut_ptr(),
            out_r.as_mut_ptr(),
            n,
        );
    }
}

/// UI thread: wait until the RT is done with `instance`, then unload.
pub fn retire_instance(instance: MixLinkVST3Ref) {
    if instance.is_null() {
        return;
    }
    unsafe {
        MixLinkVST3RetireInstance(instance);
        MixLinkVST3Unload(instance);
    }
}

pub fn slot_exchange(slot: u32, next: MixLinkVST3Ref) -> MixLinkVST3Ref {
    unsafe { MixLinkVST3SlotExchange(slot, next) }
}

pub fn slot_set_bypass(slot: u32, bypassed: bool) {
    unsafe { MixLinkVST3SlotSetBypass(slot, i32::from(bypassed)) }
}

/// UI thread: load a bundle. Returns null when the SDK stub is linked.
pub fn load(
    bundle_path: &str,
    class_uid: Option<&str>,
    sample_rate: f64,
    max_block: u32,
) -> MixLinkVST3Ref {
    #[cfg(target_os = "macos")]
    {
        load_macos(bundle_path, class_uid, sample_rate, max_block)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (bundle_path, class_uid, sample_rate, max_block);
        std::ptr::null_mut()
    }
}

#[cfg(target_os = "macos")]
fn load_macos(
    bundle_path: &str,
    class_uid: Option<&str>,
    sample_rate: f64,
    max_block: u32,
) -> MixLinkVST3Ref {
    use objc2_foundation::NSString;
    let path = NSString::from_str(bundle_path);
    let uid = class_uid.map(NSString::from_str);
    let uid_ptr = uid
        .as_ref()
        .map(|s| &**s as *const NSString as *const std::ffi::c_void)
        .unwrap_or(std::ptr::null());
    let mut err: *const std::ffi::c_void = std::ptr::null();
    unsafe {
        MixLinkVST3Load(
            &*path as *const NSString as *const std::ffi::c_void,
            uid_ptr,
            sample_rate,
            max_block,
            &mut err,
        )
    }
}

/// UI thread: open the plugin editor window (ObjC++ `MixLinkVST3ShowEditor`).
pub fn show_editor(instance: MixLinkVST3Ref, title: &str) {
    if instance.is_null() {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSString;
        let title = NSString::from_str(title);
        unsafe { MixLinkVST3ShowEditor(instance, &*title as *const NSString as *const _) }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let c = std::ffi::CString::new(title).unwrap_or_default();
        unsafe { MixLinkVST3ShowEditor(instance, c.as_ptr() as *const _) }
    }
}

pub fn close_editor(instance: MixLinkVST3Ref) {
    if !instance.is_null() {
        unsafe { MixLinkVST3CloseEditor(instance) }
    }
}

pub fn has_editor(instance: MixLinkVST3Ref) -> bool {
    !instance.is_null() && unsafe { MixLinkVST3HasEditor(instance) != 0 }
}

pub fn set_tempo(instance: MixLinkVST3Ref, bpm: f64) {
    if !instance.is_null() {
        unsafe { MixLinkVST3SetTempo(instance, bpm) }
    }
}

/// UI thread: serialize component + controller state (MixLink `.state` blob).
pub fn save_state(instance: MixLinkVST3Ref) -> Option<Vec<u8>> {
    if instance.is_null() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        use std::ptr::NonNull;

        use objc2::rc::Id;
        use objc2_foundation::NSData;

        let ptr = unsafe { MixLinkVST3SaveState(instance) };
        if ptr.is_null() {
            return None;
        }
        let data = unsafe { Id::<NSData>::retain(ptr as *mut NSData) }?;
        let len = data.length() as usize;
        if len == 0 {
            return None;
        }
        let mut out = vec![0u8; len];
        unsafe {
            data.getBytes_length(NonNull::new_unchecked(out.as_mut_ptr().cast()), len as _);
        }
        Some(out)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = instance;
        None
    }
}

/// UI thread: restore a blob from [`save_state`].
pub fn restore_state(instance: MixLinkVST3Ref, bytes: &[u8]) -> bool {
    if instance.is_null() || bytes.is_empty() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::NSData;

        let data = unsafe {
            NSData::dataWithBytes_length(bytes.as_ptr() as *mut _, bytes.len() as _)
        };
        unsafe {
            MixLinkVST3RestoreState(
                instance,
                &*data as *const NSData as *const std::ffi::c_void,
            ) != 0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (instance, bytes);
        false
    }
}

use std::sync::Mutex;

#[derive(Clone, Copy, Debug)]
pub struct DirtyEvent {
    pub instance: MixLinkVST3Ref,
    pub immediate: bool,
}

// Pointers are only read on the main thread after draining; the queue itself is
// Sync so the ObjC callback can push from the main queue.
struct DirtyQueue(Mutex<Vec<DirtyEvent>>);
unsafe impl Sync for DirtyQueue {}

static DIRTY_QUEUE: DirtyQueue = DirtyQueue(Mutex::new(Vec::new()));

extern "C" fn dirty_handler(instance: MixLinkVST3Ref, immediate: i32) {
    if instance.is_null() {
        return;
    }
    if let Ok(mut q) = DIRTY_QUEUE.0.lock() {
        q.push(DirtyEvent {
            instance,
            immediate: immediate != 0,
        });
    }
}

/// Install the host dirty callback (idempotent). Call once at app boot.
pub fn install_state_dirty_handler() {
    unsafe { MixLinkVST3SetStateDirtyHandler(Some(dirty_handler)) }
}

/// Drain pending dirty notifications (main / UI thread).
pub fn take_dirty_events() -> Vec<DirtyEvent> {
    DIRTY_QUEUE.0.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// Publish `next` to a live send slot and retire the previous instance.
pub fn exchange_and_retire(slot: u32, next: MixLinkVST3Ref) {
    let prev = slot_exchange(slot, next);
    retire_instance(prev);
}

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub name: String,
    pub bundle_path: String,
    pub class_uid: Option<String>,
}

/// Scan without loading plugin code. Empty when the SDK stub is linked.
pub fn scan_plugins() -> Vec<PluginInfo> {
    #[cfg(mixlink_vst3_sdk)]
    {
        scan_plugins_objc()
    }
    #[cfg(not(mixlink_vst3_sdk))]
    {
        Vec::new()
    }
}

#[cfg(mixlink_vst3_sdk)]
fn scan_plugins_objc() -> Vec<PluginInfo> {
    scan_plugins_nsarray()
}

fn scan_plugins_nsarray() -> Vec<PluginInfo> {
    #[cfg(target_os = "macos")]
    {
        use objc2::rc::Id;
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, msg_send_id};
        use objc2_foundation::NSString;

        let ptr = unsafe { MixLinkVST3ScanPlugins() };
        if ptr.is_null() {
            return Vec::new();
        }
        let arr = unsafe { Id::<AnyObject>::retain(ptr as *mut AnyObject) };
        let Some(arr) = arr else {
            return Vec::new();
        };
        let count: usize = unsafe { msg_send![&*arr, count] };
        let path_key = NSString::from_str("bundlePath");
        let name_key = NSString::from_str("name");
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let dict: Option<Id<AnyObject>> = unsafe { msg_send_id![&*arr, objectAtIndex: i] };
            let Some(dict) = dict else {
                continue;
            };
            let path: Option<Id<NSString>> =
                unsafe { msg_send_id![&*dict, objectForKey: &*path_key] };
            let name: Option<Id<NSString>> =
                unsafe { msg_send_id![&*dict, objectForKey: &*name_key] };
            let Some(path) = path else {
                continue;
            };
            out.push(PluginInfo {
                name: name.map(|n| n.to_string()).unwrap_or_default(),
                bundle_path: path.to_string(),
                class_uid: None,
            });
        }
        out
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
