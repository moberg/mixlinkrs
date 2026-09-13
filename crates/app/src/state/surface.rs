//! Hardware control surface: analog mixer, XL, and MIDI I/O.

use std::time::Instant;

use analog::{AnalogEngine, ReturnLane, XlRuntime};
use midi_xl::{schedule_refresh, LedFrame, MidiSession, SessionEvent};
use ui_mixlink::mixer::StripKind;

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
        self.send_led_frame_refresh(frame, true);
    }

    /// Follow-up write after the XL's momentary flash. Must not schedule
    /// another refresh — that loop marked Focus/Control as LED echoes forever.
    pub(crate) fn send_led_refresh(&mut self, frame: LedFrame) {
        self.send_led_frame_refresh(frame, false);
    }

    fn send_led_frame_refresh(&mut self, frame: LedFrame, schedule: bool) {
        self.midi.send_leds(&frame);
        self.last_led = Some(frame);
        self.led_refresh_at = if schedule {
            Some(Instant::now() + schedule_refresh())
        } else {
            None
        };
    }

    pub(crate) fn current_knob(&self, kind: StripKind, lane: Option<ReturnLane>) -> f32 {
        match (kind, lane) {
            (StripKind::Input(i), Some(lane)) => self.analog.surface.strips[i].aux(lane),
            (StripKind::Input(i), None) => self.analog.surface.strips[i].pan,
            (StripKind::Return(r), None) => self
                .analog
                .surface
                .returns
                .iter()
                .find(|x| x.id == r as i32)
                .map(|x| x.pan)
                .unwrap_or(0.5),
            _ => 0.5,
        }
    }

    pub(crate) fn current_fader(&self, kind: StripKind) -> f32 {
        match kind {
            StripKind::Input(i) => self.analog.surface.strips[i].fader,
            StripKind::Return(lane) => self
                .analog
                .surface
                .returns
                .iter()
                .find(|x| x.id == lane as i32)
                .map(|x| x.fader)
                .unwrap_or(0.0),
            StripKind::Main => self.analog.mixer.main_fader,
        }
    }

    pub(crate) fn set_fader(&mut self, kind: StripKind, v: f32) {
        // Record analog only. Never writes MixDocument / take_view faders.
        match kind {
            StripKind::Input(i) => self.analog.apply_fader(i, v),
            StripKind::Return(lane) => self.analog.apply_return_fader(lane as i32, v),
            StripKind::Main => {
                self.analog.mixer.main_fader = v;
                self.analog
                    .osc
                    .send_float(osc::output_fader_lin(self.analog.config.main_output), v);
            }
        }
    }

    pub(crate) fn set_pan(&mut self, kind: StripKind, v: f32) {
        match kind {
            StripKind::Input(i) => self.analog.apply_pan(i, v),
            StripKind::Return(lane) => self.analog.apply_return_pan(lane as i32, v),
            StripKind::Main => {}
        }
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
