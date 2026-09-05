//! Hardware control surface: analog mixer, XL, and MIDI I/O.

use std::time::Instant;

use analog::{AnalogEngine, XlRuntime};
use midi_xl::{schedule_refresh, LedFrame, MidiSession, SessionEvent};

pub(crate) struct Surface {
    pub analog: AnalogEngine,
    pub xl: XlRuntime,
    pub midi: MidiIo,
    pub last_midi: String,
    pub led_refresh_at: Option<Instant>,
    pub last_led: Option<LedFrame>,
    pub control_room_fader: f32,
}

impl Surface {
    pub(crate) fn send_led_frame(&mut self, frame: LedFrame) {
        self.midi.send_leds(&frame);
        self.last_led = Some(frame);
        self.led_refresh_at = Some(Instant::now() + schedule_refresh());
    }
}

pub(crate) enum MidiIo {
    None(MidiSession<midi_xl::NullSink>),
    #[cfg(target_os = "macos")]
    Hw(MidiSession<midi_xl::MidiEndpoint>),
}

impl MidiIo {
    pub(crate) fn status(&self) -> String {
        match self {
            Self::None(s) => s.led_status.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.led_status.clone(),
        }
    }

    pub(crate) fn drain(&mut self) -> Vec<SessionEvent> {
        match self {
            Self::None(_) => Vec::new(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.drain_input(),
        }
    }

    pub(crate) fn send_leds(&mut self, frame: &LedFrame) {
        match self {
            Self::None(s) => s.send_leds(frame),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.send_leds(frame),
        }
    }

    pub(crate) fn last_message(&self) -> String {
        match self {
            Self::None(s) => s.last_message.clone(),
            #[cfg(target_os = "macos")]
            Self::Hw(s) => s.last_message.clone(),
        }
    }
}
