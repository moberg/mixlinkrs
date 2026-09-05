//! Analog mixer surface. UI never sends OSC — only [`AnalogEngine`] + [`osc::OscSession`].

pub mod config;
pub mod engine;
pub mod mixer;
pub mod surface;
pub mod types;
pub mod xl;

pub use config::SessionConfig;
pub use engine::AnalogEngine;
pub use mixer::{apply_inbound, MixerState};
pub use osc::OscSession;
pub use surface::{SurfaceState, TrackControlMode};
pub use xl::{apply_xl, XlEffect, XlRuntime};
pub use types::{
    ChannelID, EffectRef, HardwareEffect, MixAssign, MixEvent, MixLane, MixNode, MixerBus,
    MixerChannel, PluginSlot, ReturnLane, ReturnLaneConfig, ReturnStrip, RoutingSlot,
    SendDestination, StripBinding, StripName, SurfaceStrip, ALL_SEND_LANES, BUS_LANES,
    MAX_SEND_COUNT, SEND_LANES,
};

#[cfg(test)]
mod tests {
    use super::*;
    use osc::{
        balpan_to_pan_unit, fader_db, fader_lin_from_db, pan_unit_to_balpan, post_fader_lin,
        send_lin_from_post, DB_OFF, FADER_LIN_0DB, LIN_EPS,
    };

    fn test_engine() -> AnalogEngine {
        let config = SessionConfig::new();
        let mut mixer = MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface = SurfaceState::new();
        surface.load_returns(&config);
        AnalogEngine::new(mixer, surface, config, OscSession::new())
    }

    #[test]
    fn fader_lin_0db() {
        assert!((FADER_LIN_0DB - 65.0 / 71.0).abs() < 1e-6);
        assert!(fader_db(FADER_LIN_0DB).abs() < 1e-5);
        assert!((fader_lin_from_db(0.0) - FADER_LIN_0DB).abs() < 1e-5);
    }

    #[test]
    fn post_fader_lin_unity_leaves_send() {
        let send = 0.42;
        assert!((post_fader_lin(send, FADER_LIN_0DB) - send).abs() < 1e-5);
        assert_eq!(post_fader_lin(send, 0.0), 0.0);
        assert_eq!(post_fader_lin(0.0, FADER_LIN_0DB), 0.0);
    }

    #[test]
    fn send_lin_inverts_post() {
        let send = 0.4;
        let fader = 0.8;
        let post = post_fader_lin(send, fader);
        let recovered = send_lin_from_post(post, fader).unwrap();
        assert!((recovered - send).abs() < 1e-5);
        assert!(send_lin_from_post(0.5, 0.0).is_none());
        assert_eq!(send_lin_from_post(0.0, 0.5), Some(0.0));
    }

    #[test]
    fn pan_balpan_roundtrip() {
        assert!((pan_unit_to_balpan(0.5) - 0.0).abs() < 1e-5);
        assert!((pan_unit_to_balpan(0.0) + 1.0).abs() < 1e-5);
        assert!((pan_unit_to_balpan(1.0) - 1.0).abs() < 1e-5);
        assert!((balpan_to_pan_unit(0.0) - 0.5).abs() < 1e-5);
        assert!((balpan_to_pan_unit(pan_unit_to_balpan(0.25)) - 0.25).abs() < 1e-5);
    }

    #[test]
    fn off_node_is_minus_300() {
        assert_eq!(fader_db(0.0), DB_OFF);
        assert_eq!(fader_db(LIN_EPS), DB_OFF);
        assert!(fader_db(LIN_EPS + 0.001) > DB_OFF);
    }

    #[test]
    fn bus1_bus2_exclusive() {
        let mut engine = test_engine();
        let src = engine.config.strips[0].channel_id();
        engine.apply_fader(0, 0.8);
        assert!((engine.mixer.send_level(src, engine.config.main_output) - 0.8).abs() < 1e-5);
        let bus1 = engine.config.listen_output(MixAssign::Bus1).unwrap();
        let bus2 = engine.config.listen_output(MixAssign::Bus2).unwrap();
        assert_eq!(engine.mixer.send_level(src, bus1), 0.0);
        assert_eq!(engine.mixer.send_level(src, bus2), 0.0);

        engine.apply_assign(0, MixAssign::Main, MixAssign::Bus1);
        assert_eq!(engine.surface.strips[0].assign, MixAssign::Bus1);
        assert!((engine.mixer.send_level(src, bus1) - 0.8).abs() < 1e-5);
        assert_eq!(engine.mixer.send_level(src, engine.config.main_output), 0.0);
        assert_eq!(engine.mixer.send_level(src, bus2), 0.0);

        engine.apply_assign(0, MixAssign::Bus1, MixAssign::Bus2);
        assert_eq!(engine.surface.strips[0].assign, MixAssign::Bus2);
        assert!((engine.mixer.send_level(src, bus2) - 0.8).abs() < 1e-5);
        assert_eq!(engine.mixer.send_level(src, bus1), 0.0);
        assert_eq!(engine.mixer.send_level(src, engine.config.main_output), 0.0);
    }

    #[test]
    fn mute_clears_solo() {
        let mut engine = test_engine();
        let id = engine.config.strips[0].channel_id();
        engine.apply_solo(0, true);
        assert!(engine.mixer.channel(id).unwrap().solo);
        assert!(!engine.mixer.channel(id).unwrap().mute);
        engine.apply_mute(0, true);
        let ch = engine.mixer.channel(id).unwrap();
        assert!(ch.mute);
        assert!(!ch.solo);
        engine.apply_solo(0, true);
        let ch = engine.mixer.channel(id).unwrap();
        assert!(ch.solo);
        assert!(!ch.mute);
    }

    #[test]
    fn write_send_emits_faderlin_and_db() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.5);
        let sent = engine.osc.sent_messages();
        assert!(sent.iter().any(|m| m.address.ends_with("/faderlin")));
        assert!(sent.iter().any(|m| m.address.ends_with("/fader") && !m.address.contains("faderlin")));
        let off = sent.iter().find(|m| {
            m.address == "/mix/in/0/12/fader" || m.address == "/mix/in/0/10/fader"
        });
        if let Some(msg) = off {
            assert_eq!(msg.values[0].float_value(), DB_OFF);
        }
    }

    #[test]
    fn bare_json_int_is_output_dest() {
        let dest: SendDestination = serde_json::from_str("14").unwrap();
        assert_eq!(dest, SendDestination::Output(14));
        let tagged: SendDestination =
            serde_json::from_str(r#"{"kind":"plugin","index":1}"#).unwrap();
        assert_eq!(tagged, SendDestination::Plugin(1));
    }

    #[test]
    fn selected_name_prefers_gear_alias() {
        let mut engine = test_engine();
        let id = ChannelID::new(MixerBus::Input, 0);
        engine.set_gear_name(id, "Rytm");
        assert_eq!(engine.selected_name(id), "Rytm");
        assert!(engine.display_name(id).contains("Rytm"));
        engine.add_effect_return();
        assert_eq!(engine.config.effect_return_count, 3);
        engine.remove_last_effect_return();
        assert_eq!(engine.config.effect_return_count, 2);
        engine.add_hardware_effect();
        assert!(engine.config.hardware_effects.len() >= 5);
    }

    #[test]
    fn default_session_matches_mixlink() {
        let c = SessionConfig::new();
        assert_eq!(c.osc_host, "127.0.0.1");
        assert_eq!(c.osc_send_port, 7001);
        assert_eq!(c.osc_listen_port, 9001);
        assert_eq!(c.midi_device_contains, "Launch Control XL");
        assert_eq!(c.audio_device_contains, "Fireface");
        assert_eq!(c.audio_buffer_frames, None);
        assert_eq!(c.main_output, 0);
        assert_eq!(c.aux_a, SendDestination::Output(14));
        assert_eq!(c.aux_b, SendDestination::Output(16));
        assert_eq!(c.mix_bus1, 12);
        assert_eq!(c.mix_bus2, 10);
        assert_eq!(c.effect_return_count, 2);
        assert!(!c.pan_knobs_control_send_c);
        assert!(c.sends_post_fader);
        assert!(!c.hardware_strips);
        assert_eq!(c.strips.len(), 8);
        assert!(c.strips.iter().enumerate().all(|(i, s)| {
            s.bus == MixerBus::Input && s.index == i as i32 * 2 && s.linked_stereo && s.enabled
        }));
        assert_eq!(c.plugins[0].name, "FX A");
        assert_eq!(c.plugins[1].name, "FX B");
        let m = MixerState::new();
        assert_eq!((m.inputs.len(), m.playback.len(), m.outputs.len()), (32, 33, 33));
    }

    #[test]
    fn session_json_uses_mixlink_keys() {
        let json = r#"{
            "strips":[{"id":0,"bus":"input","index":0,"linkedStereo":true,"enabled":true}],
            "mainOutput":14,
            "auxA":14,
            "auxB":{"kind":"plugin","index":0},
            "mixBus1":4,
            "mixBus2":10,
            "oscHost":"127.0.0.1",
            "oscSendPort":7001,
            "oscListenPort":9001,
            "midiDeviceContains":"Launch Control XL",
            "returns":[{"id":2,"input":20,"pan":0.5,"name":"ADAT 21","fader":0}]
        }"#;
        let mut c: SessionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.main_output, 14);
        assert_eq!(c.aux_a, SendDestination::Output(14));
        assert_eq!(c.aux_b, SendDestination::Plugin(0));
        assert_eq!(c.returns[0].id, 2);
        assert!(c.returns[0].effect.is_none());
        c.normalize_after_load();
        assert_eq!(c.plugins[0].name, "FX A");
        assert!(c.sends_post_fader);
        let wire = serde_json::to_value(&SessionConfig::new()).unwrap();
        assert!(wire.get("oscHost").is_some());
        assert!(wire.get("mainOutput").is_some());
        assert!(wire.get("projectsRootBookmark").is_none() || wire["projectsRootBookmark"].is_null());
    }

    #[test]
    fn pan_knobs_control_send_c_persists_on_the_wire() {
        let mut engine = test_engine();
        engine.set_pan_knobs_control_send_c(true);
        assert!(engine.config.pan_knobs_control_send_c);
        let json = serde_json::to_string(&engine.config).unwrap();
        assert!(json.contains("\"panKnobsControlSendC\":true"));
        let back: SessionConfig = serde_json::from_str(&json).unwrap();
        assert!(back.pan_knobs_control_send_c);
    }
}
