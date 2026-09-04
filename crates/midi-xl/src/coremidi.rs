//! Thin CoreMIDI client for the Launch Control XL (macOS only).
//!
//! Input packets are copied onto a channel; output implements [`crate::MidiSink`].
//!
//! CoreMIDI’s `MIDIPacket` / `MIDIPacketList` are `#pragma pack(push, 4)` (see
//! `MIDIServices.h`). MixLink’s `copyPackets` uses Swift `MemoryLayout` offsets
//! from those packed types: first packet at **4**, payload at **+10**. A naive
//! Rust `#[repr(C)]` list inserts 4 bytes of padding after `numPackets` (because
//! `MIDITimeStamp` is `u64` / align 8), so `addr_of!(list.packet)` points at the
//! middle of the timestamp and every CC is garbage. Walk with the pack(4)
//! offsets — do not project a padded Rust struct onto the list.

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
    for bytes in copy_packets(pkt_list) {
        let _ = tx.send(bytes);
    }
}

/// MixLink `MidiSession.copyPackets`: copy each packet’s `length` bytes,
/// including SysEx longer than the 256-byte `data` tuple.
///
/// Offsets match CoreMIDI `#pragma pack(push, 4)` / Swift `MemoryLayout`:
/// `MIDIPacketList.packet` at 4, `MIDIPacket.length` at 8, `MIDIPacket.data` at 10.
fn copy_packets(pkt_list: *const ffi::MIDIPacketList) -> Vec<Vec<u8>> {
    if pkt_list.is_null() {
        return Vec::new();
    }
    let mut collected = Vec::new();
    // SAFETY: `pkt_list` is a live CoreMIDI packet list; we only read the
    // documented pack(4) fields and `length` payload bytes.
    unsafe {
        let base = pkt_list.cast::<u8>();
        let num_packets = std::ptr::read_unaligned(base.cast::<u32>());
        let mut pkt = base.add(ffi::PACKET_LIST_PACKET_OFFSET);
        for _ in 0..num_packets {
            let len =
                std::ptr::read_unaligned(pkt.add(ffi::PACKET_LENGTH_OFFSET).cast::<u16>()) as usize;
            let payload = pkt.add(ffi::PACKET_DATA_OFFSET);
            collected.push(std::slice::from_raw_parts(payload, len).to_vec());
            pkt = ffi::midi_packet_next(pkt, len);
        }
    }
    collected
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

    /// CoreMIDI `MIDIServices.h`: `#pragma pack(push, 4)` around these structs.
    /// First packet starts immediately after `UInt32 numPackets`.
    pub const PACKET_LIST_PACKET_OFFSET: usize = 4;
    /// `MIDITimeStamp` (8) + `UInt16 length` (2).
    pub const PACKET_DATA_OFFSET: usize = 10;
    pub const PACKET_LENGTH_OFFSET: usize = 8;

    /// Packed to match CoreMIDI. Do not walk a live list through these fields —
    /// `copy_packets` uses the offsets above (first packet is 4-byte aligned,
    /// so `timeStamp` is unaligned).
    #[repr(C, packed(4))]
    pub struct MIDIPacket {
        pub timeStamp: MIDITimeStamp,
        pub length: u16,
        pub data: [u8; 256],
    }

    #[repr(C, packed(4))]
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
    /// On ARM the next packet is 4-byte aligned; on Intel it is packed tight.
    #[inline]
    pub unsafe fn midi_packet_next(pkt: *const u8, len: usize) -> *const u8 {
        // SAFETY: caller asserts `pkt` is a valid packet with `len` data bytes.
        let after = unsafe { pkt.add(PACKET_DATA_OFFSET + len) };
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            let addr = after as usize;
            ((addr + 3) & !3usize) as *const u8
        }
        #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
        {
            after
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a pack(4) `MIDIPacketList` in a byte buffer (no CoreMIDI needed).
    fn write_packet(buf: &mut [u8], offset: usize, data: &[u8]) -> usize {
        let len = data.len() as u16;
        buf[offset + ffi::PACKET_LENGTH_OFFSET..offset + ffi::PACKET_LENGTH_OFFSET + 2]
            .copy_from_slice(&len.to_ne_bytes());
        let start = offset + ffi::PACKET_DATA_OFFSET;
        buf[start..start + data.len()].copy_from_slice(data);
        let after = start + data.len();
        #[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
        {
            (after + 3) & !3
        }
        #[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
        {
            after
        }
    }

    #[test]
    fn packed_layout_matches_coremidi_and_mixlink() {
        assert_eq!(ffi::PACKET_LIST_PACKET_OFFSET, 4);
        assert_eq!(ffi::PACKET_LENGTH_OFFSET, 8);
        assert_eq!(ffi::PACKET_DATA_OFFSET, 10);
        assert_eq!(std::mem::align_of::<ffi::MIDIPacket>(), 4);
        assert_eq!(std::mem::align_of::<ffi::MIDIPacketList>(), 4);

        let list = ffi::MIDIPacketList {
            numPackets: 0,
            packet: ffi::MIDIPacket { timeStamp: 0, length: 0, data: [0; 256] },
        };
        let base = &list as *const _ as usize;
        let pkt = std::ptr::addr_of!(list.packet) as usize;
        let len = std::ptr::addr_of!(list.packet.length) as usize;
        let data = std::ptr::addr_of!(list.packet.data) as usize;
        assert_eq!(pkt - base, 4, "naive #[repr(C)] would put packet at 8");
        assert_eq!(len - pkt, 8);
        assert_eq!(data - pkt, 10);
    }

    #[test]
    fn copy_packets_reads_user_fader_cc() {
        let mut buf = [0u8; 64];
        buf[0..4].copy_from_slice(&1u32.to_ne_bytes());
        write_packet(&mut buf, ffi::PACKET_LIST_PACKET_OFFSET, &[0xB0, 77, 127]);
        let packets = copy_packets(buf.as_ptr().cast());
        assert_eq!(packets, vec![vec![0xB0, 77, 127]]);
    }

    #[test]
    fn copy_packets_walks_two_aligned_packets() {
        let mut buf = [0u8; 128];
        buf[0..4].copy_from_slice(&2u32.to_ne_bytes());
        let second = write_packet(&mut buf, ffi::PACKET_LIST_PACKET_OFFSET, &[0xB0, 13, 64]);
        write_packet(&mut buf, second, &[0xB0, 49, 32]);
        let packets = copy_packets(buf.as_ptr().cast());
        assert_eq!(packets, vec![vec![0xB0, 13, 64], vec![0xB0, 49, 32]]);
    }

    #[test]
    fn naive_repr_c_offset_would_miss_first_cc() {
        // Document the bug: a padded list would start the packet at 8, reading
        // length from the first two MIDI bytes instead of the UInt16 at +8.
        let mut buf = [0u8; 64];
        buf[0..4].copy_from_slice(&1u32.to_ne_bytes());
        write_packet(&mut buf, 4, &[0xB0, 77, 127]);
        let wrong_len = u16::from_ne_bytes([buf[8 + 8], buf[8 + 9]]);
        assert_ne!(wrong_len, 3, "offset-8 walk must not see the real length");
        let packets = copy_packets(buf.as_ptr().cast());
        assert_eq!(packets[0], vec![0xB0, 77, 127]);
    }
}
