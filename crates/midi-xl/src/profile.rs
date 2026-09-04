//! Launch Control XL Mk1/Mk2 MIDI identity.
//!
//! Official default **User** map (Novation Programmer’s Reference / Components
//! default): knobs CC 13–20 / 29–36 / 49–56, faders 77–84, pads as notes.
//! Factory 1 (Ableton) often uses that same map. Other Factory / Mk2 / Logic-style
//! layouts use per-channel GM CCs or Launchkey-family knob rows — [`identify`]
//! accepts all of these so knobs work regardless of which template is selected.

use serde::{Deserialize, Serialize};

/// User / Factory-Live default faders (Novation).
pub const FADER_CCS: [u8; 8] = [77, 78, 79, 80, 81, 82, 83, 84];
/// Send A / Aux A knobs.
pub const SEND_A_CCS: [u8; 8] = [13, 14, 15, 16, 17, 18, 19, 20];
/// Send B / Aux B knobs.
pub const SEND_B_CCS: [u8; 8] = [29, 30, 31, 32, 33, 34, 35, 36];
/// Pan knobs.
pub const PAN_CCS: [u8; 8] = [49, 50, 51, 52, 53, 54, 55, 56];

/// Track Focus pads (User notes).
pub const FOCUS_NOTES: [u8; 8] = [41, 42, 43, 44, 57, 58, 59, 60];
/// Track Control pads (User notes).
pub const CONTROL_NOTES: [u8; 8] = [73, 74, 75, 76, 89, 90, 91, 92];

/// Default User template: Device is note 105; Mute/Solo/Arm are 106–108.
pub const DEVICE_NOTES: [u8; 1] = [105];
pub const MUTE_NOTES: [u8; 1] = [106];
pub const SOLO_NOTES: [u8; 1] = [107];
pub const ARM_NOTES: [u8; 1] = [108];

/// Factory / Live Device is CC 110. Do not treat CC 105 as Device — that is
/// Send Select Down. Device still matches note 105 and CC 110.
pub const DEVICE_CCS: [u8; 1] = [110];
/// Factory Mute/Solo are CC 111/112. CC 106/107 are Track Select arrows.
pub const MUTE_CCS: [u8; 1] = [111];
pub const SOLO_CCS: [u8; 1] = [112];

/// Factory / Live Send Select (Ableton `Up` / `Down`).
pub const SEND_SELECT_UP_CCS: [u8; 1] = [104];
pub const SEND_SELECT_DOWN_CCS: [u8; 1] = [105];
/// Factory / Live Track Select Left (Ableton `Track_Left`).
pub const TRACK_SELECT_LEFT_CCS: [u8; 1] = [106];

/// Programmer’s ref: 18–1Fh = top channel row (Track Focus).
pub const FOCUS_LED_BASE: u8 = 0x18;
/// Programmer’s ref: 20–27h = bottom channel row (Track Control).
pub const CONTROL_LED_BASE: u8 = 0x20;
/// SysEx LED index 28h — Device; 29h Mute; 2Ah Solo; 2Bh Rec Arm.
/// Addressable by SysEx index only — notes 105–108 are button presses.
pub const DEVICE_LED_INDEX: u8 = 0x28;
pub const MUTE_LED_INDEX: u8 = 0x29;
pub const SOLO_LED_INDEX: u8 = 0x2A;
pub const ARM_LED_INDEX: u8 = 0x2B;
/// Programmer’s ref: 2C–2Fh = Up, Down, Left, Right. Red-only LEDs.
pub const SEND_SELECT_UP_LED_INDEX: u8 = 0x2C;
pub const SEND_SELECT_DOWN_LED_INDEX: u8 = 0x2D;
pub const TRACK_SELECT_LEFT_LED_INDEX: u8 = 0x2E;
pub const TRACK_SELECT_RIGHT_LED_INDEX: u8 = 0x2F;

/// Every LED index a full `Set LEDs` message must list (`0x18`…`0x2F`).
pub const ALL_LED_INDICES: [u8; 24] = [
    0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26,
    0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
];

/// Launchkey-family / some Factory & Mk2 custom modes (no overlap with User CCs).
pub const FACTORY_KNOB_TOP_CCS: [u8; 8] = [21, 22, 23, 24, 25, 26, 27, 28];
pub const FACTORY_KNOB_MID_CCS: [u8; 8] = [41, 42, 43, 44, 45, 46, 47, 48];
pub const FACTORY_KNOB_BOT_CCS: [u8; 8] = [61, 62, 63, 64, 65, 66, 67, 68];

/// Track Focus SysEx index for strip `0…7`.
#[inline]
pub fn focus_led_index(strip: usize) -> u8 {
    FOCUS_LED_BASE + strip as u8
}

/// Track Control SysEx index for strip `0…7`.
#[inline]
pub fn control_led_index(strip: usize) -> u8 {
    CONTROL_LED_BASE + strip as u8
}

/// 7-bit CC / note velocity → unit interval.
#[inline]
pub fn midi_to_unit(value: u8) -> f32 {
    f32::from(value) / 127.0
}

/// A mapped Launch Control XL control. Indexed variants are strip `0…7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    Fader(usize),
    AuxA(usize),
    AuxB(usize),
    Pan(usize),
    Focus(usize),
    Control(usize),
    Device,
    Mute,
    Solo,
    Arm,
    SendSelectUp,
    SendSelectDown,
    TrackSelectLeft,
}

/// Identify a channel-voice message. Order matches MixLink.
pub fn identify(status: u8, data1: u8, data2: u8) -> Option<Control> {
    let cmd = status & 0xF0;
    let channel = (status & 0x0F) as usize;

    // Some Factory / DAW mixer templates send faders as pitch-bend, one channel per strip.
    if cmd == 0xE0 && channel < 8 {
        return Some(Control::Fader(channel));
    }

    if cmd == 0xB0 {
        if let Some(i) = FADER_CCS.iter().position(|&c| c == data1) {
            return Some(Control::Fader(i));
        }
        if let Some(i) = SEND_A_CCS.iter().position(|&c| c == data1) {
            return Some(Control::AuxA(i));
        }
        if let Some(i) = SEND_B_CCS.iter().position(|&c| c == data1) {
            return Some(Control::AuxB(i));
        }
        if let Some(i) = PAN_CCS.iter().position(|&c| c == data1) {
            return Some(Control::Pan(i));
        }

        // Factory / Logic-style GM mixer: CC 7 / 10 / 91 / 93 on channels 1–8.
        if data1 == 7 && channel < 8 {
            return Some(Control::Fader(channel));
        }
        if data1 == 10 && channel < 8 {
            return Some(Control::Pan(channel));
        }
        if data1 == 91 && channel < 8 {
            return Some(Control::AuxA(channel));
        }
        if data1 == 93 && channel < 8 {
            return Some(Control::AuxB(channel));
        }

        if let Some(i) = FACTORY_KNOB_TOP_CCS.iter().position(|&c| c == data1) {
            return Some(Control::AuxA(i));
        }
        if let Some(i) = FACTORY_KNOB_MID_CCS.iter().position(|&c| c == data1) {
            return Some(Control::AuxB(i));
        }
        if let Some(i) = FACTORY_KNOB_BOT_CCS.iter().position(|&c| c == data1) {
            return Some(Control::Pan(i));
        }

        // Send Select press *and* release (value 0) — hold is a modifier.
        if SEND_SELECT_UP_CCS.contains(&data1) {
            return Some(Control::SendSelectUp);
        }
        if SEND_SELECT_DOWN_CCS.contains(&data1) {
            return Some(Control::SendSelectDown);
        }

        if data2 > 0 && DEVICE_CCS.contains(&data1) {
            return Some(Control::Device);
        }
        if data2 > 0 && MUTE_CCS.contains(&data1) {
            return Some(Control::Mute);
        }
        if data2 > 0 && SOLO_CCS.contains(&data1) {
            return Some(Control::Solo);
        }
        // Before muteNotes-as-CC: Factory Track Select Left is CC 106.
        if data2 > 0 && TRACK_SELECT_LEFT_CCS.contains(&data1) {
            return Some(Control::TrackSelectLeft);
        }

        // Factory templates may emit pads as CC (same numbers as the User notes).
        if data2 > 0 {
            if DEVICE_NOTES.contains(&data1) {
                return Some(Control::Device);
            }
            if MUTE_NOTES.contains(&data1) {
                return Some(Control::Mute);
            }
            if SOLO_NOTES.contains(&data1) {
                return Some(Control::Solo);
            }
            if ARM_NOTES.contains(&data1) {
                return Some(Control::Arm);
            }
            if let Some(i) = FOCUS_NOTES.iter().position(|&n| n == data1) {
                return Some(Control::Focus(i));
            }
            if let Some(i) = CONTROL_NOTES.iter().position(|&n| n == data1) {
                return Some(Control::Control(i));
            }
        }
    }

    if cmd == 0x90 && data2 > 0 {
        if DEVICE_NOTES.contains(&data1) {
            return Some(Control::Device);
        }
        if MUTE_NOTES.contains(&data1) {
            return Some(Control::Mute);
        }
        if SOLO_NOTES.contains(&data1) {
            return Some(Control::Solo);
        }
        if ARM_NOTES.contains(&data1) {
            return Some(Control::Arm);
        }
        if let Some(i) = FOCUS_NOTES.iter().position(|&n| n == data1) {
            return Some(Control::Focus(i));
        }
        if let Some(i) = CONTROL_NOTES.iter().position(|&n| n == data1) {
            return Some(Control::Control(i));
        }
    }
    None
}

/// Unit value for a mapped control. Buttons that fire only on press are 1.0.
pub fn value_for(control: Control, status: u8, d1: u8, d2: u8) -> f32 {
    match control {
        Control::Focus(_)
        | Control::Control(_)
        | Control::Device
        | Control::Mute
        | Control::Solo
        | Control::TrackSelectLeft => 1.0,
        Control::SendSelectUp | Control::SendSelectDown => {
            if d2 > 0 {
                1.0
            } else {
                0.0
            }
        }
        Control::Fader(_) if status & 0xF0 == 0xE0 => {
            let bend = u32::from(d1) | (u32::from(d2) << 7);
            bend as f32 / 16383.0
        }
        _ => midi_to_unit(d2),
    }
}

/// Note-off (or note-on velocity 0) of a momentary pad.
pub fn is_pad_off(status: u8, data1: u8, data2: u8) -> bool {
    let cmd = status & 0xF0;
    (cmd == 0x80 || (cmd == 0x90 && data2 == 0)) && is_momentary_button_note(data1)
}

/// Device / Mute / Solo / Arm / Track Left release. CC 104/105 are **not**
/// a device release — they are Send Select hold/release.
pub fn is_device_release(status: u8, data1: u8, data2: u8) -> bool {
    let cmd = status & 0xF0;
    if cmd == 0x80 || (cmd == 0x90 && data2 == 0) {
        return DEVICE_NOTES.contains(&data1)
            || MUTE_NOTES.contains(&data1)
            || SOLO_NOTES.contains(&data1)
            || ARM_NOTES.contains(&data1);
    }
    if cmd == 0xB0 && data2 == 0 {
        if SEND_SELECT_UP_CCS.contains(&data1) || SEND_SELECT_DOWN_CCS.contains(&data1) {
            return false;
        }
        return DEVICE_NOTES.contains(&data1)
            || DEVICE_CCS.contains(&data1)
            || MUTE_NOTES.contains(&data1)
            || MUTE_CCS.contains(&data1)
            || SOLO_NOTES.contains(&data1)
            || SOLO_CCS.contains(&data1)
            || ARM_NOTES.contains(&data1)
            || TRACK_SELECT_LEFT_CCS.contains(&data1);
    }
    false
}

/// Human-readable name matching MixLink’s status line.
pub fn describe(control: Control) -> String {
    match control {
        Control::Fader(i) => format!("fader {}", i + 1),
        Control::AuxA(i) => format!("auxA {}", i + 1),
        Control::AuxB(i) => format!("auxB {}", i + 1),
        Control::Pan(i) => format!("pan {}", i + 1),
        Control::Focus(i) => format!("focus {}", i + 1),
        Control::Control(i) => format!("control {}", i + 1),
        Control::Device => "device".into(),
        Control::Mute => "mute".into(),
        Control::Solo => "solo".into(),
        Control::Arm => "arm".into(),
        Control::SendSelectUp => "send up".into(),
        Control::SendSelectDown => "send down".into(),
        Control::TrackSelectLeft => "track left".into(),
    }
}

/// Focus or Control pad note (Launchpad-protocol LED write is allowed).
pub fn is_focus_or_control_note(note: u8) -> bool {
    FOCUS_NOTES.contains(&note) || CONTROL_NOTES.contains(&note)
}

/// Pads that flash momentarily and need an LED re-assert on release.
pub fn is_momentary_button_note(note: u8) -> bool {
    is_focus_or_control_note(note)
        || DEVICE_NOTES.contains(&note)
        || MUTE_NOTES.contains(&note)
        || SOLO_NOTES.contains(&note)
        || ARM_NOTES.contains(&note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cc(ch: u8, num: u8, val: u8) -> (u8, u8, u8) {
        (0xB0 | ch, num, val)
    }

    fn note_on(note: u8, vel: u8) -> (u8, u8, u8) {
        (0x90, note, vel)
    }

    #[test]
    fn user_ccs_and_factory_aliases_map_to_same_control() {
        for i in 0..8 {
            let user = identify(0xB0, FADER_CCS[i], 64);
            let gm = identify(0xB0 | i as u8, 7, 64);
            let bend = identify(0xE0 | i as u8, 0, 64);
            assert_eq!(user, Some(Control::Fader(i)));
            assert_eq!(gm, user);
            assert_eq!(bend, user);

            let user = identify(0xB0, SEND_A_CCS[i], 64);
            let gm = identify(0xB0 | i as u8, 91, 64);
            let launchkey = identify(0xB0, FACTORY_KNOB_TOP_CCS[i], 64);
            assert_eq!(user, Some(Control::AuxA(i)));
            assert_eq!(gm, user);
            assert_eq!(launchkey, user);

            let user = identify(0xB0, SEND_B_CCS[i], 64);
            let gm = identify(0xB0 | i as u8, 93, 64);
            let launchkey = identify(0xB0, FACTORY_KNOB_MID_CCS[i], 64);
            assert_eq!(user, Some(Control::AuxB(i)));
            assert_eq!(gm, user);
            assert_eq!(launchkey, user);

            let user = identify(0xB0, PAN_CCS[i], 64);
            let gm = identify(0xB0 | i as u8, 10, 64);
            let launchkey = identify(0xB0, FACTORY_KNOB_BOT_CCS[i], 64);
            assert_eq!(user, Some(Control::Pan(i)));
            assert_eq!(gm, user);
            assert_eq!(launchkey, user);

            // User template pads are notes. Factory templates may emit the
            // same numbers as CC, but MixLink matches knob CCs first (41–48,
            // GM 91/93), so only assert the note-on path here.
            assert_eq!(
                identify(0x90, FOCUS_NOTES[i], 127),
                Some(Control::Focus(i))
            );
            assert_eq!(
                identify(0x90, CONTROL_NOTES[i], 127),
                Some(Control::Control(i))
            );
        }

        assert_eq!(identify(0x90, 105, 127), Some(Control::Device));
        assert_eq!(identify(0xB0, 110, 127), Some(Control::Device));
        assert_eq!(identify(0x90, 106, 127), Some(Control::Mute));
        assert_eq!(identify(0xB0, 111, 127), Some(Control::Mute));
        assert_eq!(identify(0x90, 107, 127), Some(Control::Solo));
        assert_eq!(identify(0xB0, 112, 127), Some(Control::Solo));
        assert_eq!(identify(0x90, 108, 127), Some(Control::Arm));
        assert_eq!(identify(0xB0, 108, 127), Some(Control::Arm));
    }

    #[test]
    fn send_select_any_data2_and_cc105_is_not_device() {
        assert_eq!(identify(0xB0, 104, 0), Some(Control::SendSelectUp));
        assert_eq!(identify(0xB0, 104, 127), Some(Control::SendSelectUp));
        assert_eq!(identify(0xB0, 105, 0), Some(Control::SendSelectDown));
        assert_eq!(identify(0xB0, 105, 127), Some(Control::SendSelectDown));
        assert_ne!(identify(0xB0, 105, 127), Some(Control::Device));
    }

    #[test]
    fn track_select_left_cc106_is_not_mute() {
        assert_eq!(identify(0xB0, 106, 127), Some(Control::TrackSelectLeft));
        assert_ne!(identify(0xB0, 106, 127), Some(Control::Mute));
        assert_eq!(identify(0xB0, 106, 0), None);
    }

    #[test]
    fn device_mute_solo_cc_require_data2() {
        assert_eq!(identify(0xB0, 110, 0), None);
        assert_eq!(identify(0xB0, 111, 0), None);
        assert_eq!(identify(0xB0, 112, 0), None);
        assert_eq!(identify(cc(0, 110, 1).0, 110, 1), Some(Control::Device));
    }

    #[test]
    fn note_on_velocity_zero_is_not_identify() {
        assert_eq!(identify(0x90, FOCUS_NOTES[0], 0), None);
        assert_eq!(identify(0x90, 105, 0), None);
    }

    #[test]
    fn value_for_buttons_and_bend() {
        assert_eq!(value_for(Control::Focus(0), 0x90, 41, 64), 1.0);
        assert_eq!(value_for(Control::Control(1), 0x90, 74, 1), 1.0);
        assert_eq!(value_for(Control::Device, 0x90, 105, 127), 1.0);
        assert_eq!(value_for(Control::Mute, 0xB0, 111, 127), 1.0);
        assert_eq!(value_for(Control::Solo, 0xB0, 112, 1), 1.0);
        assert_eq!(value_for(Control::TrackSelectLeft, 0xB0, 106, 127), 1.0);
        assert_eq!(value_for(Control::SendSelectUp, 0xB0, 104, 127), 1.0);
        assert_eq!(value_for(Control::SendSelectDown, 0xB0, 105, 0), 0.0);
        let bend = value_for(Control::Fader(0), 0xE0, 0x00, 0x40);
        assert!((bend - (0x00 | (0x40 << 7)) as f32 / 16383.0).abs() < 1e-6);
        assert!((value_for(Control::Fader(0), 0xB0, 77, 127) - 1.0).abs() < 1e-6);
        assert!((value_for(Control::AuxA(0), 0xB0, 13, 64) - 64.0 / 127.0).abs() < 1e-6);
        assert!((value_for(Control::Arm, 0x90, 108, 127) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn pad_off_and_device_release() {
        assert!(is_pad_off(0x80, 41, 0));
        assert!(is_pad_off(0x90, 73, 0));
        assert!(is_pad_off(0x80, 105, 0));
        assert!(!is_pad_off(0x90, 41, 127));
        assert!(!is_pad_off(0xB0, 77, 0));

        assert!(is_device_release(0x80, 105, 0));
        assert!(is_device_release(0x90, 106, 0));
        assert!(is_device_release(0xB0, 110, 0));
        assert!(is_device_release(0xB0, 111, 0));
        assert!(is_device_release(0xB0, 106, 0));
        // CC 104/105 are Send Select — not a device release.
        assert!(!is_device_release(0xB0, 104, 0));
        assert!(!is_device_release(0xB0, 105, 0));
        assert!(!is_device_release(0xB0, 110, 127));
    }

    #[test]
    fn led_indices_cover_0x18_to_0x2f() {
        assert_eq!(ALL_LED_INDICES[0], 0x18);
        assert_eq!(ALL_LED_INDICES[23], 0x2F);
        assert_eq!(ALL_LED_INDICES.len(), 24);
        assert_eq!(focus_led_index(0), 0x18);
        assert_eq!(focus_led_index(7), 0x1F);
        assert_eq!(control_led_index(0), 0x20);
        assert_eq!(control_led_index(7), 0x27);
        assert_eq!(DEVICE_LED_INDEX, 0x28);
        assert_eq!(TRACK_SELECT_RIGHT_LED_INDEX, 0x2F);
    }
}
