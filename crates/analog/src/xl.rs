//! Launch Control XL → AnalogEngine. Shared by the app and golden-path tests.

use std::time::Instant;

use midi_xl::Control;

use crate::engine::AnalogEngine;
use crate::surface::TrackControlMode;
use crate::types::{MixAssign, ReturnLane};

const DEBOUNCE_MS: u128 = 250;

/// Side / transport buttons that MixLink debounces independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XlButton {
    Device = 0,
    Mute = 1,
    Solo = 2,
    Arm = 3,
    TrackLeft = 4,
}

/// Hold / relative-main / debounce state that is not part of [`AnalogEngine`].
#[derive(Clone, Debug)]
pub struct XlRuntime {
    pub send_select_up: bool,
    pub send_select_down: bool,
    pub relative_main: Option<(f32, f32)>,
    debounce: [Option<Instant>; 5],
    /// MixLink `lastModeToggle` — Mute, Solo, and Track Left share one 250 ms timer.
    last_mode_toggle: Option<Instant>,
}

impl Default for XlRuntime {
    fn default() -> Self {
        Self {
            send_select_up: false,
            send_select_down: false,
            relative_main: None,
            debounce: [None; 5],
            last_mode_toggle: None,
        }
    }
}

impl XlRuntime {
    #[must_use]
    pub fn send_select_held(&self) -> bool {
        self.send_select_up || self.send_select_down
    }

    /// MixLink clears Send Select on MIDI connect so a stuck hold cannot steal
    /// channel 8 Send A as Main.
    pub fn clear_on_connect(&mut self) {
        self.send_select_up = false;
        self.send_select_down = false;
        self.relative_main = None;
    }

    /// MixLink `allowModeToggle()` — one 250 ms gate for Mute / Solo / Track Left.
    pub fn allow_mode_toggle(&mut self) -> bool {
        if let Some(t) = self.last_mode_toggle {
            if t.elapsed().as_millis() < DEBOUNCE_MS {
                return false;
            }
        }
        self.last_mode_toggle = Some(Instant::now());
        true
    }

    pub fn debounce_ready(&mut self, button: XlButton) -> bool {
        let slot = &mut self.debounce[button as usize];
        if let Some(t) = *slot {
            if t.elapsed().as_millis() < DEBOUNCE_MS {
                return false;
            }
        }
        *slot = Some(Instant::now());
        true
    }

    /// First CC after hold only stores the knob so Main does not jump.
    pub fn nudge_main(&mut self, current_fader: f32, value: f32) -> Option<f32> {
        match self.relative_main {
            None => {
                self.relative_main = Some((value, current_fader));
                None
            }
            Some((last, stored)) => {
                let next = (stored + (value - last)).clamp(0.0, 1.0);
                self.relative_main = Some((value, next));
                Some(next)
            }
        }
    }
}

/// Side effects the app must run (record / display sleep / OSC Main).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum XlEffect {
    None,
    ToggleRecord,
    ToggleSleep,
    NudgeMain { next: f32 },
}

/// Apply a mapped XL control. Same order as MixLink `handleMIDI`.
pub fn apply_xl(
    engine: &mut AnalogEngine,
    rt: &mut XlRuntime,
    control: Control,
    value: f32,
) -> XlEffect {
    match control {
        Control::Fader(i) => {
            engine.apply_fader(i, value);
            XlEffect::None
        }
        Control::Pan(i) => {
            if engine.config.pan_knobs_control_send_c && engine.config.effect_return_count >= 3 {
                engine.apply_aux(i, ReturnLane::SendC, value);
            } else {
                engine.apply_pan(i, value);
            }
            XlEffect::None
        }
        Control::AuxA(i) => {
            if rt.send_select_held() && i == 7 {
                if let Some(next) = rt.nudge_main(engine.mixer.main_fader, value) {
                    engine.mixer.main_fader = next;
                    return XlEffect::NudgeMain { next };
                }
                return XlEffect::None;
            }
            engine.apply_aux_a(i, value);
            XlEffect::None
        }
        Control::AuxB(i) => {
            engine.apply_aux_b(i, value);
            XlEffect::None
        }
        Control::Focus(i) => {
            let prev = engine.surface.strips[i].assign;
            let next = if prev == MixAssign::Bus1 {
                MixAssign::Main
            } else {
                MixAssign::Bus1
            };
            engine.apply_assign(i, prev, next);
            XlEffect::None
        }
        Control::Control(i) => {
            match engine.surface.track_control_mode {
                TrackControlMode::BusAssign => {
                    let prev = engine.surface.strips[i].assign;
                    let next = if prev == MixAssign::Bus2 {
                        MixAssign::Main
                    } else {
                        MixAssign::Bus2
                    };
                    engine.apply_assign(i, prev, next);
                }
                TrackControlMode::Mute => engine.toggle_mute(i),
                TrackControlMode::Solo => engine.toggle_solo(i),
            }
            XlEffect::None
        }
        Control::Mute => {
            if rt.allow_mode_toggle() {
                engine.surface.toggle_mute_mode();
            }
            XlEffect::None
        }
        Control::Solo => {
            if rt.allow_mode_toggle() {
                engine.surface.toggle_solo_mode();
            }
            XlEffect::None
        }
        Control::Arm => {
            if rt.debounce_ready(XlButton::Arm) {
                return XlEffect::ToggleRecord;
            }
            XlEffect::None
        }
        Control::Device => {
            if rt.debounce_ready(XlButton::Device) {
                return XlEffect::ToggleSleep;
            }
            XlEffect::None
        }
        Control::TrackSelectLeft => {
            if rt.allow_mode_toggle() && engine.config.effect_return_count >= 3 {
                let on = !engine.config.pan_knobs_control_send_c;
                engine.set_pan_knobs_control_send_c(on);
            }
            XlEffect::None
        }
        Control::SendSelectUp => {
            rt.send_select_up = value > 0.0;
            if !rt.send_select_held() {
                rt.relative_main = None;
            }
            XlEffect::None
        }
        Control::SendSelectDown => {
            rt.send_select_down = value > 0.0;
            if !rt.send_select_held() {
                rt.relative_main = None;
            }
            XlEffect::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionConfig;
    use crate::mixer::MixerState;
    use crate::surface::SurfaceState;
    use crate::types::{EffectRef, MixAssign};
    use crate::AnalogEngine;
    use midi_xl::{describe, identify, value_for, SessionEvent};
    use osc::OscSession;

    fn test_engine() -> AnalogEngine {
        let config = SessionConfig::new();
        let mut mixer = MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface = SurfaceState::new();
        surface.load_returns(&config);
        AnalogEngine::new(mixer, surface, config, OscSession::new())
    }

    fn apply_raw(engine: &mut AnalogEngine, rt: &mut XlRuntime, status: u8, d1: u8, d2: u8) -> XlEffect {
        let mut session = midi_xl::MidiSession::recording();
        let events = session.ingest(&[status, d1, d2]);
        assert!(
            !session.last_message.contains("(unmapped)"),
            "footer must show a mapped name, got {}",
            session.last_message
        );
        match events.as_slice() {
            [SessionEvent::Control { control, value }] => apply_xl(engine, rt, *control, *value),
            other => panic!("expected Control, got {other:?} ({})", session.last_message),
        }
    }

    #[test]
    fn user_notes_mute_solo_arm_focus_control() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();

        assert_eq!(identify(0x90, 106, 127), Some(Control::Mute));
        apply_raw(&mut engine, &mut rt, 0x90, 106, 127);
        assert_eq!(engine.surface.track_control_mode, TrackControlMode::Mute);
        assert_eq!(describe(Control::Mute), "mute");

        // MixLink `allowModeToggle` is shared — a Solo press in the same 250 ms
        // is ignored, same as the hardware.
        apply_raw(&mut engine, &mut rt, 0x90, 107, 127);
        assert_eq!(engine.surface.track_control_mode, TrackControlMode::Mute);
        engine.surface.track_control_mode = TrackControlMode::Solo;
        assert_eq!(describe(Control::Solo), "solo");

        let arm = apply_raw(&mut engine, &mut rt, 0x90, 108, 127);
        assert_eq!(arm, XlEffect::ToggleRecord);
        assert_eq!(describe(Control::Arm), "arm");

        engine.surface.track_control_mode = TrackControlMode::BusAssign;
        apply_raw(&mut engine, &mut rt, 0x90, 41, 127);
        assert_eq!(engine.surface.strips[0].assign, MixAssign::Bus1);
        assert_eq!(describe(Control::Focus(0)), "focus 1");

        apply_raw(&mut engine, &mut rt, 0x90, 73, 127);
        assert_eq!(engine.surface.strips[0].assign, MixAssign::Bus2);
        assert_eq!(describe(Control::Control(0)), "control 1");
    }

    #[test]
    fn control_pad_mutes_and_solos_in_mode() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();
        let id = engine.config.strips[0].channel_id();

        apply_raw(&mut engine, &mut rt, 0x90, 106, 127);
        apply_raw(&mut engine, &mut rt, 0x90, 73, 127);
        assert!(engine.mixer.channel(id).unwrap().mute);

        engine.surface.track_control_mode = TrackControlMode::Solo;
        apply_raw(&mut engine, &mut rt, 0x90, 73, 127);
        assert!(engine.mixer.channel(id).unwrap().solo);
        assert!(!engine.mixer.channel(id).unwrap().mute);
    }

    #[test]
    fn send_select_plus_ch8_send_a_nudges_main_without_jump() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();
        engine.mixer.main_fader = 0.4;

        apply_raw(&mut engine, &mut rt, 0xB0, 104, 127);
        assert!(rt.send_select_up);

        let first = apply_raw(&mut engine, &mut rt, 0xB0, 20, 64);
        assert_eq!(first, XlEffect::None);
        assert!((engine.mixer.main_fader - 0.4).abs() < 1e-5);

        let second = apply_raw(&mut engine, &mut rt, 0xB0, 20, 80);
        let expected = (0.4_f32 + (80.0 - 64.0) / 127.0).clamp(0.0, 1.0);
        match second {
            XlEffect::NudgeMain { next } => assert!((next - expected).abs() < 1e-5),
            other => panic!("expected NudgeMain, got {other:?}"),
        }
        assert!((engine.mixer.main_fader - expected).abs() < 1e-5);
    }

    #[test]
    fn factory_track_left_toggles_send_c_when_three_returns() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();
        engine.config.effect_return_count = 3;
        engine.config.ensure_return_lane(ReturnLane::SendC);

        assert_eq!(identify(0xB0, 106, 127), Some(Control::TrackSelectLeft));
        apply_raw(&mut engine, &mut rt, 0xB0, 106, 127);
        assert!(engine.config.pan_knobs_control_send_c);
        assert_eq!(describe(Control::TrackSelectLeft), "track left");

        let c = identify(0xB0, 49, 64).unwrap();
        apply_xl(&mut engine, &mut rt, c, value_for(c, 0xB0, 49, 64));
        assert!((engine.surface.strips[0].aux(ReturnLane::SendC) - 64.0 / 127.0).abs() < 1e-5);
        assert!((engine.surface.strips[0].pan - 0.5).abs() < 1e-5);
    }

    #[test]
    fn user_aux_a_b_ccs() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();
        for i in 0..8u8 {
            apply_raw(&mut engine, &mut rt, 0xB0, 13 + i, 64);
            apply_raw(&mut engine, &mut rt, 0xB0, 29 + i, 32);
            assert!((engine.surface.strips[i as usize].aux(ReturnLane::SendA) - 64.0 / 127.0).abs() < 1e-5);
            assert!((engine.surface.strips[i as usize].aux(ReturnLane::SendB) - 32.0 / 127.0).abs() < 1e-5);
        }
    }

    #[test]
    fn per_button_debounce_allows_mute_then_arm() {
        let mut engine = test_engine();
        let mut rt = XlRuntime::default();
        apply_raw(&mut engine, &mut rt, 0x90, 106, 127);
        let arm = apply_raw(&mut engine, &mut rt, 0x90, 108, 127);
        assert_eq!(arm, XlEffect::ToggleRecord);
        assert_eq!(engine.surface.track_control_mode, TrackControlMode::Mute);

        apply_raw(&mut engine, &mut rt, 0x90, 106, 127);
        assert_eq!(engine.surface.track_control_mode, TrackControlMode::Mute);
    }

    #[test]
    fn reconnect_clears_send_select_hold() {
        let mut rt = XlRuntime::default();
        rt.send_select_up = true;
        rt.relative_main = Some((0.5, 0.4));
        rt.clear_on_connect();
        assert!(!rt.send_select_held());
        assert!(rt.relative_main.is_none());
    }

    #[test]
    fn plugin_sends_follow_aux_assign_mute() {
        let mut engine = test_engine();
        engine.config.set_return_effect(ReturnLane::SendA, Some(EffectRef::Plugin(0)));
        engine.config.set_return_effect(ReturnLane::Bus1, Some(EffectRef::Plugin(0)));
        engine.apply_fader(0, 0.8);
        engine.apply_aux_a(0, 0.5);
        let aux = engine.plugin_send_gain(0, 0);
        assert!(aux > 0.0);

        engine.apply_assign(0, MixAssign::Main, MixAssign::Bus1);
        let with_bus = engine.plugin_send_gain(0, 0);
        assert!(with_bus > aux);

        engine.apply_mute(0, true);
        assert_eq!(engine.plugin_send_gain(0, 0), 0.0);
    }

    #[test]
    fn footer_names_and_led_colors_match_mixlink() {
        assert_eq!(describe(Control::Mute), "mute");
        assert_eq!(describe(Control::Solo), "solo");
        assert_eq!(describe(Control::Arm), "arm");
        assert_eq!(describe(Control::Focus(0)), "focus 1");
        assert_eq!(describe(Control::Control(0)), "control 1");
        assert_eq!(describe(Control::SendSelectUp), "send up");
        assert_eq!(describe(Control::TrackSelectLeft), "track left");
        assert_eq!(
            midi_xl::control_led(midi_xl::TrackControlMode::Mute, false, true, false),
            midi_xl::LED_RED
        );
        assert_eq!(
            midi_xl::control_led(midi_xl::TrackControlMode::Solo, false, false, true),
            midi_xl::LED_GREEN
        );
        assert_eq!(
            midi_xl::control_led(midi_xl::TrackControlMode::BusAssign, true, false, false),
            midi_xl::LED_RED
        );
        let frame = midi_xl::LedFrame {
            mute: true,
            solo: false,
            arm: true,
            send_up: true,
            track_left: true,
            ..midi_xl::LedFrame::default()
        };
        let pairs = frame.pairs();
        assert_eq!(pairs.iter().find(|(i, _)| *i == midi_xl::MUTE_LED_INDEX).unwrap().1, midi_xl::LED_YELLOW);
        assert_eq!(pairs.iter().find(|(i, _)| *i == midi_xl::ARM_LED_INDEX).unwrap().1, midi_xl::LED_YELLOW);
        assert_eq!(
            pairs.iter().find(|(i, _)| *i == midi_xl::SEND_SELECT_UP_LED_INDEX).unwrap().1,
            midi_xl::LED_RED
        );
        assert_eq!(
            pairs.iter().find(|(i, _)| *i == midi_xl::TRACK_SELECT_LEFT_LED_INDEX).unwrap().1,
            midi_xl::LED_RED
        );
    }
}
