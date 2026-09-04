//! LED writer — **sacred**.
//!
//! Learned by breaking it. Do not “simplify”:
//!
//! 1. `Set LEDs` (`F0 00 20 29 02 11 78 <template> …`) only affects the
//!    template named in the message. Write **all 16 templates** (`0…15`)
//!    every time. The XL never answers a template query — it only notifies
//!    (`0x77`) when the user turns the knob.
//! 2. **One SysEx must list every LED** (indices `0x18…0x2F`). A second
//!    subset `Set LEDs` (e.g. only Mute/Solo) blanks Track Focus/Control.
//!    That two-message pattern is **wrong**. [`crate::session::MidiSession::send_leds`]
//!    must send one full message per template.
//! 3. **Never note-on 105–108.** Those notes are the Device / Mute / Solo /
//!    Record Arm *buttons*, not LED addresses. Writing them clears the pads.
//!    Side LEDs are SysEx index `0x28…0x2B` only.
//! 4. **Never send `0x77`** (template select). MixLink must not change the
//!    template the user selected. [`crate::session::MidiSession::send_midi`]
//!    drops such messages. Outgoing packets use command `0x78` only.
//!
//! Velocities: Copy+Clear flags set (`0x0C`). Off / Amber full / Red full /
//! Green full / Yellow (Device, Mute, Solo, Rec Arm are yellow-only).

use serde::{Deserialize, Serialize};

use crate::profile::{
    control_led_index, focus_led_index, ALL_LED_INDICES, ARM_LED_INDEX, CONTROL_NOTES,
    DEVICE_LED_INDEX, FOCUS_NOTES, MUTE_LED_INDEX, SEND_SELECT_DOWN_LED_INDEX,
    SEND_SELECT_UP_LED_INDEX, SOLO_LED_INDEX, TRACK_SELECT_LEFT_LED_INDEX,
    TRACK_SELECT_RIGHT_LED_INDEX,
};

/// Copy+Clear flags set (`0x0C`): off.
pub const LED_OFF: u8 = 0x0C;
/// Amber full.
pub const LED_AMBER: u8 = 0x3F;
/// Red full.
pub const LED_RED: u8 = 0x0F;
/// Green full.
pub const LED_GREEN: u8 = 0x3C;
/// Device / Mute / Solo / Rec Arm are yellow-only.
pub const LED_YELLOW: u8 = 0x3E;

/// `F0 00 20 29 02 11` — Novation Launch Control XL SysEx prefix.
pub const SYSEX_PREFIX: [u8; 6] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x11];

/// Track Control row mode. Control pads show Bus 2, mute, or solo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackControlMode {
    #[default]
    BusAssign,
    Mute,
    Solo,
}

/// Snapshot of every pad/side LED MixLink writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedFrame {
    pub focus_on_bus1: [bool; 8],
    pub control_vel: [u8; 8],
    pub device: bool,
    pub mute: bool,
    pub solo: bool,
    pub arm: bool,
    pub send_up: bool,
    pub send_down: bool,
    pub track_left: bool,
}

impl Default for LedFrame {
    fn default() -> Self {
        Self {
            focus_on_bus1: [false; 8],
            control_vel: [LED_OFF; 8],
            device: false,
            mute: false,
            solo: false,
            arm: false,
            send_up: false,
            send_down: false,
            track_left: false,
        }
    }
}

impl LedFrame {
    /// 24 index/velocity pairs covering `0x18…0x2F`.
    pub fn pairs(&self) -> [(u8, u8); 24] {
        build_led_pairs(
            self.focus_on_bus1,
            self.control_vel,
            self.device,
            self.mute,
            self.solo,
            self.arm,
            self.send_up,
            self.send_down,
            self.track_left,
        )
    }
}

/// `F0 00 20 29 02 11 78 <template> <index> <vel> … F7` — LEDs only; does
/// not change the template. Command is always `0x78`, never `0x77`.
pub fn led_packet(template: u8, pairs: &[(u8, u8)]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SYSEX_PREFIX.len() + 3 + pairs.len() * 2);
    bytes.extend_from_slice(&SYSEX_PREFIX);
    bytes.push(0x78);
    bytes.push(template);
    for &(index, velocity) in pairs {
        bytes.push(index);
        bytes.push(velocity);
    }
    bytes.push(0xF7);
    debug_assert!(
        !is_template_select(&bytes),
        "led_packet must never emit template-select 0x77"
    );
    bytes
}

/// Inbound (or forbidden outbound) template-select notify: byte `[6] == 0x77`.
pub fn is_template_select(bytes: &[u8]) -> bool {
    bytes.len() >= 7 && bytes.starts_with(&SYSEX_PREFIX) && bytes[6] == 0x77
}

/// Launchpad-protocol LED write for the currently selected template.
/// `note` must be a Focus or Control pad — never 105–108.
pub fn led_note_on(note: u8, velocity: u8) -> Vec<u8> {
    debug_assert!(
        !matches!(note, 105..=108),
        "notes 105–108 are buttons, not LED addresses"
    );
    vec![0x90, note, velocity]
}

/// Control-row colour: BusAssign red if Bus 2, Mute red if muted, Solo green if soloed.
pub fn control_led(mode: TrackControlMode, assign_bus2: bool, muted: bool, soloed: bool) -> u8 {
    match mode {
        TrackControlMode::BusAssign => {
            if assign_bus2 {
                LED_RED
            } else {
                LED_OFF
            }
        }
        TrackControlMode::Mute => {
            if muted {
                LED_RED
            } else {
                LED_OFF
            }
        }
        TrackControlMode::Solo => {
            if soloed {
                LED_GREEN
            } else {
                LED_OFF
            }
        }
    }
}

/// 24 pairs covering **all** indices `0x18…0x2F`. Track Select Right is always off.
#[allow(clippy::too_many_arguments)]
pub fn build_led_pairs(
    focus_on_bus1: [bool; 8],
    control_vel: [u8; 8],
    device: bool,
    mute: bool,
    solo: bool,
    arm: bool,
    send_up: bool,
    send_down: bool,
    track_left: bool,
) -> [(u8, u8); 24] {
    let mut pairs = [(0u8, LED_OFF); 24];
    let mut n = 0;
    for i in 0..8 {
        let focus_vel = if focus_on_bus1[i] { LED_AMBER } else { LED_OFF };
        pairs[n] = (focus_led_index(i), focus_vel);
        n += 1;
        pairs[n] = (control_led_index(i), control_vel[i]);
        n += 1;
    }
    pairs[n] = (DEVICE_LED_INDEX, on_yellow(device));
    n += 1;
    pairs[n] = (MUTE_LED_INDEX, on_yellow(mute));
    n += 1;
    pairs[n] = (SOLO_LED_INDEX, on_yellow(solo));
    n += 1;
    pairs[n] = (ARM_LED_INDEX, on_yellow(arm));
    n += 1;
    pairs[n] = (SEND_SELECT_UP_LED_INDEX, on_red(send_up));
    n += 1;
    pairs[n] = (SEND_SELECT_DOWN_LED_INDEX, on_red(send_down));
    n += 1;
    pairs[n] = (TRACK_SELECT_LEFT_LED_INDEX, on_red(track_left));
    n += 1;
    pairs[n] = (TRACK_SELECT_RIGHT_LED_INDEX, LED_OFF);
    debug_assert_eq!(n + 1, 24);
    pairs
}

#[inline]
fn on_yellow(on: bool) -> u8 {
    if on {
        LED_YELLOW
    } else {
        LED_OFF
    }
}

#[inline]
fn on_red(on: bool) -> u8 {
    if on {
        LED_RED
    } else {
        LED_OFF
    }
}

/// Focus/Control note-ons (selected template) plus one full SysEx per template 0…15.
///
/// Never includes `0x77` or notes 105–108.
pub fn led_messages(frame: &LedFrame) -> Vec<Vec<u8>> {
    let pairs = frame.pairs();
    let mut out = Vec::with_capacity(16 + 16);
    for i in 0..8 {
        let focus_vel = if frame.focus_on_bus1[i] { LED_AMBER } else { LED_OFF };
        out.push(led_note_on(FOCUS_NOTES[i], focus_vel));
        out.push(led_note_on(CONTROL_NOTES[i], frame.control_vel[i]));
    }
    for template in 0u8..=15 {
        out.push(led_packet(template, &pairs));
    }
    out
}

/// Reject a `Set LEDs` packet that does not list every pad/side LED.
///
/// A follow-up subset write blanks the rest. `send_leds` must emit one full
/// message per template — never a second subset message.
pub fn check_full_led_packet(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 9 {
        return Err("packet too short for a full Set LEDs message".into());
    }
    if !bytes.starts_with(&SYSEX_PREFIX) {
        return Err("missing Novation SysEx prefix F0 00 20 29 02 11".into());
    }
    if bytes[6] == 0x77 {
        return Err("template-select 0x77 must never be sent".into());
    }
    if bytes[6] != 0x78 {
        return Err(format!("expected Set LEDs 0x78, got {:02X}", bytes[6]));
    }
    if bytes.last() != Some(&0xF7) {
        return Err("missing F7 terminator".into());
    }
    if bytes.iter().any(|&b| b == 0x77) {
        return Err("0x77 must never appear in outgoing LED SysEx".into());
    }
    for w in bytes.windows(2) {
        if w[0] == 0x90 && (105..=108).contains(&w[1]) {
            return Err("notes 105–108 are buttons, not LED addresses".into());
        }
    }
    let payload = &bytes[8..bytes.len() - 1];
    if payload.len() % 2 != 0 {
        return Err("index/velocity pairs must be even-length".into());
    }
    let mut seen = [false; 24];
    for pair in payload.chunks_exact(2) {
        let idx = pair[0];
        if (0x18..=0x2F).contains(&idx) {
            seen[usize::from(idx - 0x18)] = true;
        }
    }
    for (i, present) in seen.iter().enumerate() {
        if !present {
            return Err(format!("missing LED index {:02X}", 0x18 + i as u8));
        }
    }
    Ok(())
}

/// Panic if [`check_full_led_packet`] fails.
pub fn assert_full_led_packet(bytes: &[u8]) {
    if let Err(e) = check_full_led_packet(bytes) {
        panic!("incomplete LED packet: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{
        identify, CONTROL_NOTES, FADER_CCS, FACTORY_KNOB_BOT_CCS, FACTORY_KNOB_MID_CCS,
        FACTORY_KNOB_TOP_CCS, FOCUS_NOTES, PAN_CCS, SEND_A_CCS, SEND_B_CCS,
    };
    use crate::Control;

    fn sample_frame() -> LedFrame {
        let mut frame = LedFrame {
            focus_on_bus1: [true, false, true, false, false, false, false, true],
            device: true,
            mute: true,
            solo: false,
            arm: true,
            send_up: true,
            send_down: false,
            track_left: true,
            ..LedFrame::default()
        };
        frame.control_vel[0] = LED_RED;
        frame.control_vel[2] = LED_GREEN;
        frame
    }

    #[test]
    fn one_packet_per_template_is_full_no_77_no_side_note_ons() {
        let messages = led_messages(&sample_frame());
        let sysex: Vec<_> = messages
            .iter()
            .filter(|m| m.first() == Some(&0xF0))
            .collect();
        assert_eq!(sysex.len(), 16, "one Set LEDs packet per template 0..15");

        let mut seen_templates = [false; 16];
        for pkt in &sysex {
            assert_full_led_packet(pkt);
            assert!(!is_template_select(pkt));
            assert_eq!(pkt[6], 0x78);
            assert!(!pkt.contains(&0x77));
            let t = pkt[7] as usize;
            assert!(t <= 15);
            seen_templates[t] = true;
            let payload = &pkt[8..pkt.len() - 1];
            let mut indices = Vec::new();
            for pair in payload.chunks_exact(2) {
                indices.push(pair[0]);
            }
            for idx in ALL_LED_INDICES {
                assert!(indices.contains(&idx), "missing index {idx:02X}");
            }
            assert_eq!(indices.len(), 24);
        }
        assert!(seen_templates.iter().all(|&s| s));

        for pkt in &sysex {
            for w in pkt.windows(2) {
                assert!(
                    !(w[0] == 0x90 && (105..=108).contains(&w[1])),
                    "no 105–108 note-ons in SysEx"
                );
            }
        }

        let notes: Vec<_> = messages.iter().filter(|m| m.first() == Some(&0x90)).collect();
        assert_eq!(notes.len(), 16);
        for n in notes {
            assert_eq!(n.len(), 3);
            assert!(
                FOCUS_NOTES.contains(&n[1]) || CONTROL_NOTES.contains(&n[1]),
                "Launchpad note-ons are Focus/Control only, got {}",
                n[1]
            );
            assert!(!(105..=108).contains(&n[1]));
        }
    }

    #[test]
    fn second_subset_message_is_the_wrong_pattern() {
        // A second Set LEDs listing only Mute/Solo blanks Track Focus/Control
        // on the hardware. send_leds must send one full message per template —
        // never follow a complete write with a subset write.
        let full = led_packet(0, &sample_frame().pairs());
        assert_full_led_packet(&full);

        let subset = led_packet(
            0,
            &[(MUTE_LED_INDEX, LED_YELLOW), (SOLO_LED_INDEX, LED_YELLOW)],
        );
        assert!(
            check_full_led_packet(&subset).is_err(),
            "subset packet must not pass as a complete LED write"
        );
        assert_eq!(
            led_messages(&LedFrame::default())
                .iter()
                .filter(|m| m.first() == Some(&0xF0))
                .count(),
            16,
            "send_leds / led_messages emits exactly one SysEx per template"
        );
    }

    #[test]
    fn identify_user_ccs_and_factory_aliases_map_to_same_control() {
        for i in 0..8 {
            assert_eq!(identify(0xB0, FADER_CCS[i], 1), Some(Control::Fader(i)));
            assert_eq!(identify(0xB0 | i as u8, 7, 1), Some(Control::Fader(i)));
            assert_eq!(identify(0xE0 | i as u8, 0, 64), Some(Control::Fader(i)));

            assert_eq!(identify(0xB0, SEND_A_CCS[i], 1), Some(Control::AuxA(i)));
            assert_eq!(identify(0xB0 | i as u8, 91, 1), Some(Control::AuxA(i)));
            assert_eq!(identify(0xB0, FACTORY_KNOB_TOP_CCS[i], 1), Some(Control::AuxA(i)));

            assert_eq!(identify(0xB0, SEND_B_CCS[i], 1), Some(Control::AuxB(i)));
            assert_eq!(identify(0xB0 | i as u8, 93, 1), Some(Control::AuxB(i)));
            assert_eq!(identify(0xB0, FACTORY_KNOB_MID_CCS[i], 1), Some(Control::AuxB(i)));

            assert_eq!(identify(0xB0, PAN_CCS[i], 1), Some(Control::Pan(i)));
            assert_eq!(identify(0xB0 | i as u8, 10, 1), Some(Control::Pan(i)));
            assert_eq!(identify(0xB0, FACTORY_KNOB_BOT_CCS[i], 1), Some(Control::Pan(i)));
        }
    }

    #[test]
    fn two_message_subset_write_is_rejected_by_assert_full_led_packet() {
        let full = led_packet(3, &LedFrame::default().pairs());
        assert_full_led_packet(&full);

        for missing in ALL_LED_INDICES {
            let pairs: Vec<(u8, u8)> = ALL_LED_INDICES
                .iter()
                .filter(|&&i| i != missing)
                .map(|&i| (i, LED_OFF))
                .collect();
            let pkt = led_packet(3, &pairs);
            let err = check_full_led_packet(&pkt).expect_err("missing index must fail");
            assert!(
                err.contains(&format!("{missing:02X}")) || err.contains("missing"),
                "expected missing-index error, got {err}"
            );
            let panicked = std::panic::catch_unwind(|| assert_full_led_packet(&pkt)).is_err();
            assert!(panicked, "assert_full_led_packet must reject a packet missing {missing:02X}");
        }
    }

    #[test]
    fn control_led_color_per_mode() {
        assert_eq!(control_led(TrackControlMode::BusAssign, true, true, true), LED_RED);
        assert_eq!(control_led(TrackControlMode::BusAssign, false, true, true), LED_OFF);
        assert_eq!(control_led(TrackControlMode::Mute, true, true, true), LED_RED);
        assert_eq!(control_led(TrackControlMode::Mute, true, false, true), LED_OFF);
        assert_eq!(control_led(TrackControlMode::Solo, true, true, true), LED_GREEN);
        assert_eq!(control_led(TrackControlMode::Solo, true, true, false), LED_OFF);
    }

    #[test]
    fn constants_match_novation_copy_clear() {
        assert_eq!(LED_OFF, 0x0C);
        assert_eq!(LED_AMBER, 0x3F);
        assert_eq!(LED_RED, 0x0F);
        assert_eq!(LED_GREEN, 0x3C);
        assert_eq!(LED_YELLOW, 0x3E);
        assert_eq!(SYSEX_PREFIX, [0xF0, 0x00, 0x20, 0x29, 0x02, 0x11]);
    }

    #[test]
    fn track_right_always_off() {
        let pairs = build_led_pairs([true; 8], [LED_RED; 8], true, true, true, true, true, true, true);
        let right = pairs.iter().find(|(i, _)| *i == TRACK_SELECT_RIGHT_LED_INDEX);
        assert_eq!(right, Some(&(TRACK_SELECT_RIGHT_LED_INDEX, LED_OFF)));
    }
}
