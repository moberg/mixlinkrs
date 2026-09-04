//! MIDI session: connect to a Launch Control XL, parse inbound, write LEDs.
//!
//! Hardware I/O is behind [`MidiSink`] so tests never need a device.

use std::time::Duration;

use thiserror::Error;

use crate::leds::{is_template_select, led_messages, LedFrame, SYSEX_PREFIX};
use crate::profile::{describe, identify, is_device_release, is_pad_off, value_for, Control};

#[cfg(target_os = "macos")]
pub use crate::coremidi::MidiEndpoint;

/// Default `nameContains` needle — MixLink `SessionConfig.midiDeviceContains`.
pub const DEVICE_NAME_NEEDLE: &str = "Launch Control XL";

/// Hardware needs a second full write after connect / pad flash.
pub const LED_REFRESH_DELAY_MS: u64 = 80;
pub const LED_REFRESH_DELAY: Duration = Duration::from_millis(LED_REFRESH_DELAY_MS);

/// Delay before the follow-up full LED write (XL momentary flash).
#[must_use]
pub fn schedule_refresh() -> Duration {
    LED_REFRESH_DELAY
}

/// Status bar: `LED → {name} t{n}` plus `no MIDI out` / `SEND FAILED`.
#[must_use]
pub fn format_led_status(dest_name: Option<&str>, template: u8, send_failed: bool) -> String {
    match dest_name {
        None => "LED: no MIDI out".into(),
        Some(name) => {
            let mut s = format!("LED → {name} t{template}");
            if send_failed {
                s.push_str(" SEND FAILED");
            }
            s
        }
    }
}

/// Skip HUI / MIDIIN2 / MIDIOUT2 — those ports swallow LED SysEx and can
/// echo pad notes so Track Focus/Control never latch.
#[must_use]
pub fn is_protocol_port(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("hui")
        || n.contains("midiin2")
        || n.contains("midiout2")
        || n.contains("midi in 2")
        || n.contains("midi out 2")
}

/// Outbound MIDI bytes. Tests use [`RecordingSink`]; hardware uses
/// [`MidiEndpoint`] on macOS.
pub trait MidiSink {
    fn send(&mut self, bytes: &[u8]) -> bool;
}

/// Sink that records every accepted packet. Never talks to hardware.
#[derive(Clone, Debug, Default)]
pub struct RecordingSink {
    pub packets: Vec<Vec<u8>>,
    pub fail: bool,
}

impl MidiSink for RecordingSink {
    fn send(&mut self, bytes: &[u8]) -> bool {
        if self.fail {
            return false;
        }
        self.packets.push(bytes.to_vec());
        true
    }
}

/// Always reports send failure (no destination).
#[derive(Clone, Debug, Default)]
pub struct NullSink;

impl MidiSink for NullSink {
    fn send(&mut self, _bytes: &[u8]) -> bool {
        false
    }
}

#[derive(Debug, Error)]
pub enum MidiError {
    #[error("CoreMIDI error {0}")]
    CoreMidi(i32),
    #[error("no MIDI source matching “{0}” (skipped HUI/MIDIIN2)")]
    NoSource(String),
    #[error("MIDI is only available on macOS")]
    Unsupported,
}

/// Parsed inbound event after MixLink’s pad-off / Send Select rules.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    Control { control: Control, value: f32 },
    PadOff,
    TemplateChanged(u8),
    Unmapped { status: u8, data1: u8, data2: u8 },
}

/// XL session. `S` is a [`MidiSink`] so unit tests do not open CoreMIDI.
pub struct MidiSession<S: MidiSink> {
    sink: Option<S>,
    dest_name: String,
    has_output: bool,
    current_template: u8,
    last_send_failed: bool,
    pub connected_name: Option<String>,
    pub last_message: String,
    pub led_status: String,
    sysex_buffer: Vec<u8>,
    running_status: u8,
}

impl MidiSession<RecordingSink> {
    /// Test session that records outbound bytes as `Launch Control XL`.
    #[must_use]
    pub fn recording() -> Self {
        Self::with_sink(RecordingSink::default(), "Launch Control XL")
    }
}

impl MidiSession<NullSink> {
    /// No destination — `send_leds` reports `LED: no MIDI out`.
    #[must_use]
    pub fn no_output() -> Self {
        let mut s = Self {
            sink: None,
            dest_name: String::new(),
            has_output: false,
            current_template: 0,
            last_send_failed: false,
            connected_name: None,
            last_message: String::new(),
            led_status: String::new(),
            sysex_buffer: Vec::new(),
            running_status: 0,
        };
        s.led_status = format_led_status(None, 0, false);
        s
    }
}

#[cfg(not(target_os = "macos"))]
impl MidiSession<NullSink> {
    pub fn connect(_name_contains: &str) -> Result<Self, MidiError> {
        Err(MidiError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
impl MidiSession<MidiEndpoint> {
    /// Connect class-compliant XL sources whose name contains `name_contains`
    /// (default [`DEVICE_NAME_NEEDLE`]). Skips HUI / MIDIIN2 / MIDIOUT2.
    pub fn connect(name_contains: &str) -> Result<Self, MidiError> {
        let needle = if name_contains.is_empty() {
            DEVICE_NAME_NEEDLE
        } else {
            name_contains
        };
        let endpoint = MidiEndpoint::open(needle)?;
        let dest_name = endpoint.dest_name().to_string();
        let connected = endpoint.connected_name().map(str::to_string);
        let last_message = match (&connected, dest_name.as_str()) {
            (None, _) => format!("No source matching “{needle}” (skipped HUI/MIDIIN2)"),
            (Some(name), "") => format!("Connected {name} → no dest"),
            (Some(name), out) => format!("Connected {name} → {out}"),
        };
        let has_output = endpoint.has_dest();
        Ok(Self {
            sink: Some(endpoint),
            dest_name,
            has_output,
            current_template: 0,
            last_send_failed: false,
            connected_name: connected,
            last_message,
            led_status: String::new(),
            sysex_buffer: Vec::new(),
            running_status: 0,
        })
    }

    /// Drain CoreMIDI input and parse MixLink events.
    pub fn drain_input(&mut self) -> Vec<SessionEvent> {
        let packets = self
            .sink
            .as_mut()
            .map(MidiEndpoint::take_packets)
            .unwrap_or_default();
        let mut events = Vec::new();
        for bytes in packets {
            events.extend(self.ingest(&bytes));
        }
        events
    }
}

impl<S: MidiSink> MidiSession<S> {
    /// Session that writes through `sink`. `dest_name` appears in the status line.
    pub fn with_sink(sink: S, dest_name: impl Into<String>) -> Self {
        Self {
            sink: Some(sink),
            dest_name: dest_name.into(),
            has_output: true,
            current_template: 0,
            last_send_failed: false,
            connected_name: None,
            last_message: String::new(),
            led_status: String::new(),
            sysex_buffer: Vec::new(),
            running_status: 0,
        }
    }

    #[must_use]
    pub fn dest_name(&self) -> &str {
        &self.dest_name
    }

    #[must_use]
    pub fn current_template(&self) -> u8 {
        self.current_template
    }

    #[must_use]
    pub fn led_refresh_delay(&self) -> Duration {
        schedule_refresh()
    }

    /// Borrow the sink (e.g. recorded packets in tests).
    pub fn sink(&self) -> Option<&S> {
        self.sink.as_ref()
    }

    /// Mutable sink (inspect recorded packets after `send_leds`).
    pub fn sink_mut(&mut self) -> Option<&mut S> {
        self.sink.as_mut()
    }

    /// Hard stop: never emit Novation template-select (`F0 … 11 77 …`).
    pub fn send_midi(&mut self, bytes: &[u8]) {
        if is_template_select(bytes) {
            log::debug!("dropped template-select 0x77");
            return;
        }
        let Some(sink) = self.sink.as_mut() else {
            self.last_send_failed = true;
            return;
        };
        if bytes.is_empty() || bytes.len() > 256 {
            self.last_send_failed = true;
            return;
        }
        if !sink.send(bytes) {
            self.last_send_failed = true;
        }
    }

    /// Write every button LED. One full SysEx per template 0…15, plus
    /// Launchpad note-ons for Focus/Control only (never 105–108).
    pub fn send_leds(&mut self, frame: &LedFrame) {
        if !self.has_output || self.sink.is_none() {
            self.led_status = format_led_status(None, self.current_template, false);
            return;
        }
        self.last_send_failed = false;
        for msg in led_messages(frame) {
            self.send_midi(&msg);
        }
        let name = if self.dest_name.is_empty() {
            None
        } else {
            Some(self.dest_name.as_str())
        };
        self.led_status = format_led_status(name, self.current_template, self.last_send_failed);
    }

    /// Byte-wise ingest. Channel-status bytes abort an incomplete SysEx so
    /// CCs are never dropped.
    pub fn ingest(&mut self, bytes: &[u8]) -> Vec<SessionEvent> {
        let mut events = Vec::new();
        if bytes.is_empty() {
            return events;
        }
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == 0xF0 {
                self.sysex_buffer.clear();
                self.sysex_buffer.push(0xF0);
                self.running_status = 0;
                i += 1;
                continue;
            }
            if b == 0xF7 {
                if !self.sysex_buffer.is_empty() {
                    self.sysex_buffer.push(0xF7);
                    if let Some(ev) = self.handle_sysex() {
                        events.push(ev);
                    }
                    self.sysex_buffer.clear();
                }
                i += 1;
                continue;
            }
            if b >= 0xF8 {
                i += 1;
                continue;
            }
            if b >= 0x80 {
                self.sysex_buffer.clear();
                let len = channel_message_length(b);
                self.running_status = if (b & 0xF0) < 0xF0 { b } else { 0 };
                if i + len <= bytes.len() {
                    if let Some(ev) = self.parse_channel(&bytes[i..i + len]) {
                        events.push(ev);
                    }
                    i += len;
                } else {
                    break;
                }
                continue;
            }

            if !self.sysex_buffer.is_empty() {
                self.sysex_buffer.push(b);
                if self.sysex_buffer.len() > 16_384 {
                    self.sysex_buffer.clear();
                }
                i += 1;
                continue;
            }

            if self.running_status != 0 {
                let len = channel_message_length(self.running_status);
                if i + len - 1 <= bytes.len() {
                    let mut msg = Vec::with_capacity(len);
                    msg.push(self.running_status);
                    msg.extend_from_slice(&bytes[i..i + len - 1]);
                    if let Some(ev) = self.parse_channel(&msg) {
                        events.push(ev);
                    }
                    i += len - 1;
                } else {
                    break;
                }
                continue;
            }

            i += 1;
        }
        events
    }

    fn handle_sysex(&mut self) -> Option<SessionEvent> {
        let bytes = &self.sysex_buffer;
        self.last_message = format!(
            "SysEx {}B {}",
            bytes.len(),
            bytes
                .iter()
                .take(8)
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        if bytes.len() >= 8 && bytes.starts_with(&SYSEX_PREFIX) && bytes[6] == 0x77 {
            self.current_template = bytes[7];
            self.last_message.push_str(&format!(" template {}", bytes[7]));
            return Some(SessionEvent::TemplateChanged(bytes[7]));
        }
        None
    }

    fn parse_channel(&mut self, bytes: &[u8]) -> Option<SessionEvent> {
        if bytes.len() < 2 {
            return None;
        }
        let status = bytes[0];
        let d1 = bytes[1];
        let d2 = if bytes.len() > 2 { bytes[2] } else { 0 };
        let hex = format!("{status:02X} {d1:02X} {d2:02X}");
        self.last_message = hex.clone();
        let cmd = status & 0xF0;

        // Send Select is a hold modifier; release must reach onControl. Do not
        // treat CC 105 as Device pad-off (Device is note 105 / Factory CC 110).
        if let Some(control) = identify(status, d1, d2) {
            if matches!(control, Control::SendSelectUp | Control::SendSelectDown) {
                self.last_message = format!("{hex} {}", describe(control));
                let value = value_for(control, status, d1, d2);
                return Some(SessionEvent::Control { control, value });
            }
        }
        if is_pad_off(status, d1, d2) || is_device_release(status, d1, d2) {
            self.last_message = format!("{hex} pad-off");
            return Some(SessionEvent::PadOff);
        }
        if let Some(control) = identify(status, d1, d2) {
            self.last_message = format!("{hex} {}", describe(control));
            let value = value_for(control, status, d1, d2);
            return Some(SessionEvent::Control { control, value });
        }
        if cmd == 0xB0 || cmd == 0x90 || cmd == 0x80 || cmd == 0xE0 {
            self.last_message = format!("{hex} (unmapped)");
            return Some(SessionEvent::Unmapped { status, data1: d1, data2: d2 });
        }
        None
    }
}

fn channel_message_length(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leds::{
        assert_full_led_packet, check_full_led_packet, is_template_select, LED_OFF, SYSEX_PREFIX,
    };
    use crate::profile::{CONTROL_NOTES, FOCUS_NOTES};

    #[test]
    fn send_midi_drops_template_select() {
        let mut session = MidiSession::recording();
        let mut sel = SYSEX_PREFIX.to_vec();
        sel.extend_from_slice(&[0x77, 0x03, 0xF7]);
        assert!(is_template_select(&sel));
        session.send_midi(&sel);
        assert!(session.sink().unwrap().packets.is_empty());
        session.send_midi(&[0x90, 41, 0x3F]);
        assert_eq!(session.sink().unwrap().packets.len(), 1);
    }

    #[test]
    fn send_leds_writes_sixteen_full_packets_and_focus_control_notes() {
        let mut session = MidiSession::recording();
        session.send_leds(&LedFrame::default());
        let packets = &session.sink().unwrap().packets;

        let sysex: Vec<_> = packets.iter().filter(|m| m.first() == Some(&0xF0)).collect();
        assert_eq!(sysex.len(), 16);
        for (i, pkt) in sysex.iter().enumerate() {
            assert_full_led_packet(pkt);
            assert_eq!(pkt[7], i as u8);
            assert!(!pkt.contains(&0x77));
        }

        let notes: Vec<_> = packets.iter().filter(|m| m.first() == Some(&0x90)).collect();
        assert_eq!(notes.len(), 16);
        for n in notes {
            assert!(FOCUS_NOTES.contains(&n[1]) || CONTROL_NOTES.contains(&n[1]));
            assert!(!(105..=108).contains(&n[1]));
        }

        assert_eq!(session.led_status, "LED → Launch Control XL t0");
    }

    #[test]
    fn send_leds_no_output_status() {
        let mut session = MidiSession::no_output();
        session.send_leds(&LedFrame::default());
        assert_eq!(session.led_status, "LED: no MIDI out");
        assert!(session.led_status.contains("no MIDI out"));
    }

    #[test]
    fn send_leds_failed_status() {
        let mut session = MidiSession::with_sink(
            RecordingSink { packets: Vec::new(), fail: true },
            "Launch Control XL",
        );
        session.send_leds(&LedFrame::default());
        assert_eq!(session.led_status, "LED → Launch Control XL t0 SEND FAILED");
        assert!(session.led_status.contains("SEND FAILED"));
    }

    #[test]
    fn schedule_refresh_is_80ms() {
        assert_eq!(schedule_refresh(), Duration::from_millis(80));
        assert_eq!(LED_REFRESH_DELAY_MS, 80);
        let session = MidiSession::recording();
        assert_eq!(session.led_refresh_delay(), Duration::from_millis(80));
    }

    #[test]
    fn skips_hui_and_midiin2() {
        assert!(is_protocol_port("Launch Control XL HUI"));
        assert!(is_protocol_port("MIDIIN2 (Launch Control XL)"));
        assert!(is_protocol_port("MIDIOUT2 (Launch Control XL)"));
        assert!(is_protocol_port("Launch Control XL MIDI In 2"));
        assert!(is_protocol_port("Launch Control XL MIDI Out 2"));
        assert!(!is_protocol_port("Launch Control XL"));
        assert!(!is_protocol_port("Launch Control XL Mk2"));
    }

    #[test]
    fn ingest_template_select_updates_status_only() {
        let mut session = MidiSession::recording();
        let mut sel = SYSEX_PREFIX.to_vec();
        sel.extend_from_slice(&[0x77, 0x0A, 0xF7]);
        let events = session.ingest(&sel);
        assert_eq!(events, vec![SessionEvent::TemplateChanged(10)]);
        assert_eq!(session.current_template(), 10);
        session.send_leds(&LedFrame::default());
        assert_eq!(session.led_status, "LED → Launch Control XL t10");
        // Must not echo 0x77 back out.
        assert!(session
            .sink()
            .unwrap()
            .packets
            .iter()
            .all(|p| !is_template_select(p)));
    }

    #[test]
    fn ingest_user_fader_and_send_select_release() {
        let mut session = MidiSession::recording();
        let ev = session.ingest(&[0xB0, 77, 127]);
        assert_eq!(
            ev,
            vec![SessionEvent::Control { control: Control::Fader(0), value: 1.0 }]
        );
        let ev = session.ingest(&[0xB0, 105, 0]);
        match ev.as_slice() {
            [SessionEvent::Control { control: Control::SendSelectDown, value }] => {
                assert_eq!(*value, 0.0);
            }
            other => panic!("expected send select release, got {other:?}"),
        }
        let ev = session.ingest(&[0x80, 41, 0]);
        assert_eq!(ev, vec![SessionEvent::PadOff]);
    }

    #[test]
    fn subset_packet_is_rejected() {
        let subset = crate::leds::led_packet(0, &[(0x29, LED_OFF)]);
        assert!(check_full_led_packet(&subset).is_err());
    }
}
