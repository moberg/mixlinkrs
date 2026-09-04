//! Thin CoreMIDI client for the Launch Control XL (macOS only).
//!
//! Input packets are copied onto a channel; output implements [`crate::MidiSink`].

use std::ffi::c_void;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

use crate::session::{is_protocol_port, MidiError, MidiSink};

/// Hardware endpoint: XL sources connected, one destination for LED SysEx.
pub struct MidiEndpoint {
    _handles: RawHandles,
    dest: u32,
    dest_name: String,
    connected_name: Option<String>,
    rx: Receiver<Vec<u8>>,
}

struct RawHandles {
    client: u32,
    input_port: u32,
    output_port: u32,
    connected_sources: Vec<u32>,
    state: *mut CallbackState,
}

struct CallbackState {
    tx: Mutex<Sender<Vec<u8>>>,
}

impl Drop for RawHandles {
    fn drop(&mut self) {
        // SAFETY: ports and client were created by CoreMIDI in `MidiEndpoint::open`.
        unsafe {
            for src in &self.connected_sources {
                if self.input_port != 0 && *src != 0 {
                    let _ = ffi::MIDIPortDisconnectSource(self.input_port, *src);
                }
            }
            if self.input_port != 0 {
                let _ = ffi::MIDIPortDispose(self.input_port);
            }
            if self.output_port != 0 {
                let _ = ffi::MIDIPortDispose(self.output_port);
            }
            if self.client != 0 {
                let _ = ffi::MIDIClientDispose(self.client);
            }
        }
        if !self.state.is_null() {
            // SAFETY: state was Box::into_raw in `open`; we own it exclusively.
            drop(unsafe { Box::from_raw(self.state) });
        }
    }
}

impl MidiEndpoint {
    /// Open matching sources/destinations. Skips HUI / MIDIIN2 / MIDIOUT2.
    pub fn open(name_contains: &str) -> Result<Self, MidiError> {
        let (tx, rx) = mpsc::channel();
        let state = Box::into_raw(Box::new(CallbackState { tx: Mutex::new(tx) }));

        let client_cf = CFString::new("MixLink");
        let in_cf = CFString::new("MixLink In");
        let out_cf = CFString::new("MixLink Out");

        let mut client = 0u32;
        let mut input_port = 0u32;
        let mut output_port = 0u32;

        // SAFETY: CoreMIDI create; names are copied by the framework.
        unsafe {
            let st = ffi::MIDIClientCreate(
                TCFType::as_concrete_TypeRef(&client_cf),
                None,
                std::ptr::null_mut(),
                &mut client,
            );
            if st != 0 {
                drop(Box::from_raw(state));
                return Err(MidiError::CoreMidi(st));
            }
            let st = ffi::MIDIInputPortCreate(
                client,
                TCFType::as_concrete_TypeRef(&in_cf),
                Some(read_proc),
                state as *mut c_void,
                &mut input_port,
            );
            if st != 0 {
                let _ = ffi::MIDIClientDispose(client);
                drop(Box::from_raw(state));
                return Err(MidiError::CoreMidi(st));
            }
            let st = ffi::MIDIOutputPortCreate(
                client,
                TCFType::as_concrete_TypeRef(&out_cf),
                &mut output_port,
            );
            if st != 0 {
                let _ = ffi::MIDIPortDispose(input_port);
                let _ = ffi::MIDIClientDispose(client);
                drop(Box::from_raw(state));
                return Err(MidiError::CoreMidi(st));
            }
        }

        let mut connected_sources = Vec::new();
        let mut names = Vec::new();
        // SAFETY: source enumeration is valid after client create.
        unsafe {
            let nsrc = ffi::MIDIGetNumberOfSources();
            for i in 0..nsrc {
                let src = ffi::MIDIGetSource(i);
                if src == 0 {
                    continue;
                }
                let name = endpoint_name(src);
                if !name_contains.is_empty()
                    && !name.to_ascii_lowercase().contains(&name_contains.to_ascii_lowercase())
                {
                    continue;
                }
                if is_protocol_port(&name) {
                    continue;
                }
                let _ = ffi::MIDIPortConnectSource(input_port, src, std::ptr::null_mut());
                connected_sources.push(src);
                names.push(name);
            }
        }

        if names.is_empty() {
            drop(RawHandles {
                client,
                input_port,
                output_port,
                connected_sources,
                state,
            });
            return Err(MidiError::NoSource(name_contains.to_string()));
        }

        let connected_name = Some(names.join(" + "));
        let dest = unsafe { pick_destination(name_contains) };
        let dest_name = if dest == 0 {
            String::new()
        } else {
            unsafe { endpoint_name(dest) }
        };

        Ok(Self {
            _handles: RawHandles {
                client,
                input_port,
                output_port,
                connected_sources,
                state,
            },
            dest,
            dest_name,
            connected_name,
            rx,
        })
    }

    #[must_use]
    pub fn dest_name(&self) -> &str {
        &self.dest_name
    }

    #[must_use]
    pub fn connected_name(&self) -> Option<&str> {
        self.connected_name.as_deref()
    }

    #[must_use]
    pub fn has_dest(&self) -> bool {
        self.dest != 0
    }

    /// Drain packets received since the last call.
    pub fn take_packets(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(p) = self.rx.try_recv() {
            out.push(p);
        }
        out
    }
}

impl MidiSink for MidiEndpoint {
    fn send(&mut self, bytes: &[u8]) -> bool {
        if self.dest == 0 || self._handles.output_port == 0 || bytes.is_empty() || bytes.len() > 256
        {
            return false;
        }
        send_packet(self._handles.output_port, self.dest, bytes)
    }
}

fn send_packet(port: u32, dest: u32, bytes: &[u8]) -> bool {
    let mut buf = [0u8; 512];
    // SAFETY: `buf` is a MIDIPacketList workspace; Init/Add/Send are CoreMIDI.
    unsafe {
        let list = buf.as_mut_ptr().cast::<ffi::MIDIPacketList>();
        let pkt = ffi::MIDIPacketListInit(list);
        if ffi::MIDIPacketListAdd(
            list,
            512,
            pkt,
            0,
            bytes.len() as ffi::ByteCount,
            bytes.as_ptr(),
        )
        .is_null()
        {
            return false;
        }
        ffi::MIDISend(port, dest, list) == 0
    }
}

/// Prefer a non-protocol destination whose name contains `needle`.
/// Shorter display names win (the hardware port, not the DAW alias).
unsafe fn pick_destination(needle: &str) -> u32 {
    let mut primary: Vec<(usize, u32)> = Vec::new();
    let mut secondary = 0u32;
    // SAFETY: destination enumeration; names via MIDIObjectGetStringProperty.
    unsafe {
        let n = ffi::MIDIGetNumberOfDestinations();
        for i in 0..n {
            let ep = ffi::MIDIGetDestination(i);
            if ep == 0 {
                continue;
            }
            let name = endpoint_name(ep);
            if !needle.is_empty()
                && !name.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
            {
                continue;
            }
            if is_protocol_port(&name) {
                if secondary == 0 {
                    secondary = ep;
                }
                continue;
            }
            primary.push((name.len(), ep));
        }
    }
    primary
        .into_iter()
        .min_by_key(|(len, _)| *len)
        .map(|(_, ep)| ep)
        .unwrap_or(secondary)
}

unsafe fn endpoint_name(endpoint: u32) -> String {
    let mut param: CFStringRef = std::ptr::null();
    // SAFETY: caller asserts `endpoint` is a live MIDIObjectRef.
    let st = unsafe { ffi::MIDIObjectGetStringProperty(endpoint, ffi::kMIDIPropertyDisplayName, &mut param) };
    if st != 0 || param.is_null() {
        return "MIDI".into();
    }
    // SAFETY: GetStringProperty copies; we own `param` (create rule).
    let cf = unsafe { CFString::wrap_under_create_rule(param) };
    cf.to_string()
}

extern "C" fn read_proc(
    pkt_list: *const ffi::MIDIPacketList,
    read_ref_con: *mut c_void,
    _src_ref_con: *mut c_void,
) {
    if pkt_list.is_null() || read_ref_con.is_null() {
        return;
    }
    // SAFETY: `read_ref_con` is the CallbackState leaked in `open`.
    let state = unsafe { &*(read_ref_con.cast::<CallbackState>()) };
    let Ok(tx) = state.tx.lock() else {
        return;
    };
    // SAFETY: walk the variable-length MIDIPacketList via MIDIPacketNext.
    unsafe {
        let list = &*pkt_list;
        let mut pkt_ptr: *const ffi::MIDIPacket = std::ptr::addr_of!(list.packet);
        for _ in 0..list.numPackets {
            let pkt = &*pkt_ptr;
            let len = pkt.length as usize;
            let payload = std::ptr::addr_of!(pkt.data).cast::<u8>();
            let bytes = std::slice::from_raw_parts(payload, len);
            let _ = tx.send(bytes.to_vec());
            pkt_ptr = ffi::MIDIPacketNext(pkt_ptr);
        }
    }
}

mod ffi {
    #![allow(non_snake_case, non_camel_case_types, dead_code)]

    use core_foundation::string::CFStringRef;
    use std::ffi::c_void;

    pub type MIDIClientRef = u32;
    pub type MIDIPortRef = u32;
    pub type MIDIEndpointRef = u32;
    pub type MIDIObjectRef = u32;
    pub type OSStatus = i32;
    pub type MIDITimeStamp = u64;
    pub type ItemCount = libc::c_ulong;
    pub type ByteCount = libc::c_ulong;
    pub type MIDINotifyProc = Option<unsafe extern "C" fn(*const c_void, *mut c_void)>;
    pub type MIDIReadProc =
        Option<unsafe extern "C" fn(*const MIDIPacketList, *mut c_void, *mut c_void)>;

    #[repr(C)]
    pub struct MIDIPacket {
        pub timeStamp: MIDITimeStamp,
        pub length: u16,
        pub data: [u8; 256],
    }

    #[repr(C)]
    pub struct MIDIPacketList {
        pub numPackets: u32,
        pub packet: MIDIPacket,
    }

    #[link(name = "CoreMIDI", kind = "framework")]
    extern "C" {
        pub static kMIDIPropertyDisplayName: CFStringRef;

        pub fn MIDIClientCreate(
            name: CFStringRef,
            notify_proc: MIDINotifyProc,
            notify_ref_con: *mut c_void,
            out_client: *mut MIDIClientRef,
        ) -> OSStatus;
        pub fn MIDIClientDispose(client: MIDIClientRef) -> OSStatus;
        pub fn MIDIInputPortCreate(
            client: MIDIClientRef,
            port_name: CFStringRef,
            read_proc: MIDIReadProc,
            ref_con: *mut c_void,
            out_port: *mut MIDIPortRef,
        ) -> OSStatus;
        pub fn MIDIOutputPortCreate(
            client: MIDIClientRef,
            port_name: CFStringRef,
            out_port: *mut MIDIPortRef,
        ) -> OSStatus;
        pub fn MIDIPortDispose(port: MIDIPortRef) -> OSStatus;
        pub fn MIDIPortConnectSource(
            port: MIDIPortRef,
            source: MIDIEndpointRef,
            conn_ref_con: *mut c_void,
        ) -> OSStatus;
        pub fn MIDIPortDisconnectSource(port: MIDIPortRef, source: MIDIEndpointRef) -> OSStatus;
        pub fn MIDIGetNumberOfSources() -> ItemCount;
        pub fn MIDIGetSource(source_index: ItemCount) -> MIDIEndpointRef;
        pub fn MIDIGetNumberOfDestinations() -> ItemCount;
        pub fn MIDIGetDestination(dest_index: ItemCount) -> MIDIEndpointRef;
        pub fn MIDIObjectGetStringProperty(
            obj: MIDIObjectRef,
            property_id: CFStringRef,
            out: *mut CFStringRef,
        ) -> OSStatus;
        pub fn MIDIPacketListInit(pkt_list: *mut MIDIPacketList) -> *mut MIDIPacket;
        pub fn MIDIPacketListAdd(
            pkt_list: *mut MIDIPacketList,
            list_size: ByteCount,
            cur_packet: *mut MIDIPacket,
            time: MIDITimeStamp,
            n_data: ByteCount,
            data: *const u8,
        ) -> *mut MIDIPacket;
        pub fn MIDISend(
            port: MIDIPortRef,
            dest: MIDIEndpointRef,
            pkt_list: *const MIDIPacketList,
        ) -> OSStatus;
    }

    /// `MIDIPacketNext` is a `CF_INLINE` — not an exported symbol.
    #[inline]
    pub unsafe fn MIDIPacketNext(pkt: *const MIDIPacket) -> *const MIDIPacket {
        // SAFETY: caller asserts `pkt` is a valid packet with initialized `length`.
        let len = unsafe { (*pkt).length as usize };
        let base = pkt.cast::<u8>();
        // timeStamp (u64) + length (u16); matches the C MIDIPacket layout.
        const DATA_OFFSET: usize = 10;
        let after = unsafe { base.add(DATA_OFFSET + len) };
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            let addr = after as usize;
            ((addr + 3) & !3usize) as *const MIDIPacket
        }
        #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
        {
            after as *const MIDIPacket
        }
    }
}
