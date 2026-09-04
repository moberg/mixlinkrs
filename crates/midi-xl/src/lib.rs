//! Launch Control XL Mk1/Mk2 MIDI identity, LED writer, and session.
//!
//! Factory templates (slots 8–15) and User templates (0–7) are left as the
//! user set them — this crate never sends template-change SysEx (`0x77`).
//!
//! The LED writer in [`leds`] is sacred. Tests lock the packet shape.

#![forbid(unsafe_op_in_unsafe_fn)]

pub mod leds;
pub mod profile;
pub mod session;

#[cfg(target_os = "macos")]
mod coremidi;

pub use leds::{
    assert_full_led_packet, build_led_pairs, check_full_led_packet, control_led, is_template_select,
    led_messages, led_note_on, led_packet, LedFrame, TrackControlMode, LED_AMBER, LED_GREEN,
    LED_OFF, LED_RED, LED_YELLOW, SYSEX_PREFIX,
};
pub use profile::{
    control_led_index, describe, focus_led_index, identify, is_device_release,
    is_focus_or_control_note, is_momentary_button_note, is_pad_off, midi_to_unit, value_for, Control,
    ALL_LED_INDICES, ARM_LED_INDEX, ARM_NOTES, CONTROL_LED_BASE, CONTROL_NOTES, DEVICE_CCS,
    DEVICE_LED_INDEX, DEVICE_NOTES, FADER_CCS, FACTORY_KNOB_BOT_CCS, FACTORY_KNOB_MID_CCS,
    FACTORY_KNOB_TOP_CCS, FOCUS_LED_BASE, FOCUS_NOTES, MUTE_CCS, MUTE_LED_INDEX, MUTE_NOTES,
    PAN_CCS, SEND_A_CCS, SEND_B_CCS, SEND_SELECT_DOWN_CCS, SEND_SELECT_DOWN_LED_INDEX,
    SEND_SELECT_UP_CCS, SEND_SELECT_UP_LED_INDEX, SOLO_CCS, SOLO_LED_INDEX, SOLO_NOTES,
    TRACK_SELECT_LEFT_CCS, TRACK_SELECT_LEFT_LED_INDEX, TRACK_SELECT_RIGHT_LED_INDEX,
};
pub use session::{
    format_led_status, is_protocol_port, schedule_refresh, MidiError, MidiSession, MidiSink,
    NullSink, RecordingSink, SessionEvent, DEVICE_NAME_NEEDLE, LED_REFRESH_DELAY,
    LED_REFRESH_DELAY_MS,
};

#[cfg(target_os = "macos")]
pub use session::MidiEndpoint;
