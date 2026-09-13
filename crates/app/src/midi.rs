//! Launch Control XL session: inbound mapping and LED writes.

use std::time::Instant;

use analog::{apply_xl, MixAssign, XlEffect};
use midi_xl::{describe, LedFrame, SessionEvent, TrackControlMode as XlMode};
use project::MixLane;
use ui_mixlink::chrome::Page;

use crate::state::AppState;

impl AppState {
    pub(crate) fn poll_midi_and_leds(&mut self) {
        let events = self.surface.midi.drain();
        let mut pad_off = false;
        for ev in events {
            pad_off |= matches!(ev, SessionEvent::PadOff);
            self.handle_midi(ev);
        }
        let msg = self.surface.midi.last_message();
        if !msg.is_empty() {
            self.surface.last_midi = msg;
        }
        self.sync_xl_leds(pad_off);
    }

    /// MixLink `pushPadLEDs` — write every LED, then refresh after the pad flash.

    pub(crate) fn push_pad_leds(&mut self) {
        let frame = self.led_frame();
        self.surface.send_led_frame(frame);
    }

    /// MixLink `pushPadLEDs`: full write now, then one more after 80 ms so the XL’s
    /// momentary flash does not leave Mute/Solo/Focus dark. Idle frames are silent.

    pub(crate) fn sync_xl_leds(&mut self, reassert: bool) {
        let frame = self.led_frame();
        let refresh_due = self.surface.led_refresh_at.is_some_and(|at| Instant::now() >= at);
        if reassert || self.surface.last_led != Some(frame) {
            self.surface.send_led_frame(frame);
            return;
        }
        if refresh_due {
            self.surface.send_led_refresh(frame);
        }
    }

    pub(crate) fn led_frame(&self) -> LedFrame {
        let analog = &self.surface.analog;
        let cfg = &analog.config;
        let mut focus = [false; 8];
        let mut control = [midi_xl::LED_OFF; 8];
        let mode = match analog.surface.track_control_mode {
            analog::TrackControlMode::BusAssign => XlMode::BusAssign,
            analog::TrackControlMode::Mute => XlMode::Mute,
            analog::TrackControlMode::Solo => XlMode::Solo,
        };
        if self.chrome.page == Page::Mix {
            let tracks = self.mixer_tracks();
            for i in 0..8 {
                let lane = MixLane::Strip(i as i32);
                let track = tracks.iter().find(|t| t.lane == lane);
                focus[i] = self.timeline.selected_lane == Some(lane);
                control[i] = midi_xl::control_led(
                    mode,
                    false,
                    track.map(|t| t.mute).unwrap_or(false),
                    track.map(|t| t.solo).unwrap_or(false),
                );
            }
        } else {
            for i in 0..8 {
                let strip = &analog.surface.strips[i];
                focus[i] = strip.assign == MixAssign::Bus1;
                let muted = analog.strip_muted(i);
                let soloed = analog.strip_soloed(i);
                control[i] =
                    midi_xl::control_led(mode, strip.assign == MixAssign::Bus2, muted, soloed);
            }
        }
        LedFrame {
            focus_on_bus1: focus,
            control_vel: control,
            device: crate::display_sleep::is_asleep(),
            mute: matches!(analog.surface.track_control_mode, analog::TrackControlMode::Mute),
            solo: matches!(analog.surface.track_control_mode, analog::TrackControlMode::Solo),
            arm: self.audio.recording,
            send_up: self.surface.xl.send_select_up,
            send_down: self.surface.xl.send_select_down,
            track_left: cfg.pan_knobs_control_send_c && cfg.effect_return_count >= 3,
        }
    }

    /// Mix-page XL writes the mix document only. Record analog faders stay put.

    pub(crate) fn handle_mix_xl(&mut self, control: midi_xl::Control, value: f32) -> bool {
        match control {
            midi_xl::Control::Fader(i) => {
                if let Some(track) = self.mix_index_for_lane(MixLane::Strip(i as i32)) {
                    self.set_mix_fader(track, value);
                }
                true
            }
            midi_xl::Control::Pan(i) => {
                if self.surface.analog.config.pan_knobs_control_send_c {
                    return false;
                }
                if let Some(track) = self.mix_index_for_lane(MixLane::Strip(i as i32)) {
                    self.set_mix_pan(track, value);
                }
                true
            }
            midi_xl::Control::Focus(i) => {
                self.timeline.selected_lane = Some(MixLane::Strip(i as i32));
                true
            }
            midi_xl::Control::Control(i) => {
                let Some(track) = self.mix_index_for_lane(MixLane::Strip(i as i32)) else {
                    return true;
                };
                match self.surface.analog.surface.track_control_mode {
                    analog::TrackControlMode::Mute => {
                        if let Some(t) = self.mix_track_mut(track) {
                            t.mute = !t.mute;
                        }
                        self.persist_mix();
                    }
                    analog::TrackControlMode::Solo => {
                        if let Some(t) = self.mix_track_mut(track) {
                            t.solo = !t.solo;
                        }
                        self.persist_mix();
                    }
                    analog::TrackControlMode::BusAssign => {}
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn handle_midi(&mut self, ev: SessionEvent) {
        match ev {
            SessionEvent::Control { control, value } => {
                self.surface.last_midi = describe(control);
                let mode_before = self.surface.analog.surface.track_control_mode;
                if self.chrome.page == Page::Mix && self.handle_mix_xl(control, value) {
                    if matches!(control, midi_xl::Control::Focus(_) | midi_xl::Control::Control(_))
                    {
                        self.push_pad_leds();
                    }
                    self.sync_rt();
                    return;
                }
                match apply_xl(&mut self.surface.analog, &mut self.surface.xl, control, value) {
                    XlEffect::ToggleRecord => self.toggle_record(),
                    XlEffect::ToggleSleep => crate::display_sleep::toggle(),
                    XlEffect::NudgeMain { next } => {
                        self.surface.analog.osc.send_float(
                            osc::output_fader_lin(self.surface.analog.config.main_output),
                            next,
                        );
                    }
                    XlEffect::None => {}
                }
                // MixLink `toggleMuteMode` / `toggleSoloMode` / `toggleControl` call
                // `pushPadLEDs` on the same MainActor turn as the button.
                if matches!(
                    control,
                    midi_xl::Control::Mute
                        | midi_xl::Control::Solo
                        | midi_xl::Control::Focus(_)
                        | midi_xl::Control::Control(_)
                        | midi_xl::Control::Arm
                        | midi_xl::Control::Device
                        | midi_xl::Control::SendSelectUp
                        | midi_xl::Control::SendSelectDown
                        | midi_xl::Control::TrackSelectLeft
                ) || self.surface.analog.surface.track_control_mode != mode_before
                {
                    self.push_pad_leds();
                }
            }
            SessionEvent::Unmapped { status, data1, data2 } => {
                self.surface.last_midi = format!("{status:02X} {data1:02X} {data2:02X} (unmapped)");
            }
            SessionEvent::PadOff => {}
            SessionEvent::TemplateChanged(_) => {}
        }
        self.sync_rt();
    }
}
