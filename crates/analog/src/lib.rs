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
pub use types::{
    unique_copy_name, ChainKind, ChainRef, ChannelID, EffectRef, HardwareChain, HardwareEffect,
    HardwarePreset, MixAssign, MixEvent, MixLane, MixNode, MixerBus, MixerChannel, PluginChain,
    PluginSlot, PluginStage, PluginStripSends, ReturnLane, ReturnLaneConfig, ReturnStrip,
    RoutingSlot, SendDestination, StripBinding, StripName, SurfaceStrip, ALL_SEND_LANES, BUS_LANES,
    MAX_PLUGIN_STAGES, MAX_SEND_COUNT, SEND_LANES,
};
pub use xl::{apply_xl, XlEffect, XlRuntime};

#[cfg(test)]
mod tests {
    use super::*;
    use osc::{
        balpan_to_pan_unit, fader_db, fader_lin_from_db, pan_unit_to_balpan, post_fader_lin,
        send_lin_from_post, DB_OFF, FADER_LIN_0DB, LIN_EPS,
    };

    fn test_engine() -> AnalogEngine {
        engine_from_config(SessionConfig::new())
    }

    fn engine_from_config(config: SessionConfig) -> AnalogEngine {
        let mut mixer = MixerState::new();
        mixer.monitored_output = config.main_output;
        let mut surface = SurfaceState::new();
        surface.load_returns(&config);
        AnalogEngine::new(mixer, surface, config, OscSession::new())
    }

    fn assign_plugin_send(engine: &mut AnalogEngine, lane: ReturnLane, name: &str, playback: i32) {
        let idx = match lane {
            ReturnLane::SendC => 1,
            _ => 0,
        };
        let chain = &mut engine.config.plugin_chains[idx];
        chain.return_channel = playback;
        if !chain.stages.iter().any(|s| s.name == name) {
            let mut stage = PluginStage::new(name);
            stage.bundle_path = Some(format!("/tmp/{name}.vst3"));
            chain.stages.push(stage);
        }
        let id = chain.id;
        engine.config.set_return_chain(lane, Some(ChainRef::plugin(id)));
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
    fn strip_mute_and_solo_gate_record_mix() {
        let mut engine = test_engine();
        assert!(!engine.strip_muted(0));
        assert!(!engine.strip_soloed(0));
        assert!(!engine.any_solo_active());
        engine.apply_mute(0, true);
        assert!(engine.strip_muted(0));
        engine.apply_mute(0, false);
        engine.apply_solo(1, true);
        assert!(engine.any_solo_active());
        assert!(engine.strip_soloed(1));
        assert!(!engine.strip_soloed(0));
        engine.config.strips[2].enabled = false;
        assert!(engine.strip_muted(2));
    }

    #[test]
    fn solo_zeros_other_strips_on_main() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        engine.apply_fader(1, 0.7);
        let main = engine.config.main_output;
        let src0 = engine.config.strips[0].channel_id();
        let src1 = engine.config.strips[1].channel_id();
        engine.apply_solo(1, true);
        assert_eq!(engine.mixer.send_level(src0, main), 0.0);
        assert!((engine.mixer.send_level(src1, main) - 0.7).abs() < 1e-5);
        engine.apply_solo(1, false);
        assert!((engine.mixer.send_level(src0, main) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn return_solo_zeros_input_strips_on_main() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        let main = engine.config.main_output;
        let src0 = engine.config.strips[0].channel_id();
        engine.apply_return_solo(ReturnLane::SendA, true);
        assert_eq!(engine.mixer.send_level(src0, main), 0.0);
        engine.apply_return_solo(ReturnLane::SendA, false);
        assert!((engine.mixer.send_level(src0, main) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn return_solo_keeps_hardware_send_to_that_return() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        engine.apply_aux(0, ReturnLane::SendA, 0.5);
        let src = engine.config.strips[0].channel_id();
        let send_a = engine.config.hardware_output(ReturnLane::SendA).expect("Send A hardware");
        let post = post_fader_lin(0.5, 0.8);
        engine.apply_return_solo(ReturnLane::SendA, true);
        assert_eq!(engine.mixer.send_level(src, engine.config.main_output), 0.0);
        assert!((engine.mixer.send_level(src, send_a) - post).abs() < 1e-5);
    }

    #[test]
    fn return_solo_keeps_plugin_send_to_that_return() {
        let mut engine = test_engine();
        let send = engine.config.plugin_chains[0].id;
        engine.config.set_return_chain(ReturnLane::SendA, Some(ChainRef::plugin(send)));
        engine.apply_fader(0, 0.8);
        engine.apply_aux_a(0, 0.5);
        let before = engine.plugin_send_gain(send, 0);
        assert!(before > 0.0);
        engine.apply_return_solo(ReturnLane::SendA, true);
        assert!((engine.plugin_send_gain(send, 0) - before).abs() < 1e-5);
    }

    #[test]
    fn return_solo_zeros_other_plugin_return() {
        let mut engine = test_engine();
        let a = engine.config.plugin_chains[0].id;
        let b = engine.config.add_plugin_chain("Other", 6);
        engine.config.set_return_chain(ReturnLane::SendA, Some(ChainRef::plugin(a)));
        engine.config.set_return_chain(ReturnLane::SendB, Some(ChainRef::plugin(b)));
        engine.apply_fader(0, 0.8);
        engine.apply_aux_a(0, 0.5);
        engine.apply_aux_b(0, 0.4);
        engine.apply_return_solo(ReturnLane::SendA, true);
        assert!(engine.plugin_send_gain(a, 0) > 0.0);
        assert_eq!(engine.plugin_send_gain(b, 0), 0.0);
    }

    #[test]
    fn strip_solo_still_cuts_other_strips_plugin_send() {
        let mut engine = test_engine();
        let send = engine.config.plugin_chains[0].id;
        engine.config.set_return_chain(ReturnLane::SendA, Some(ChainRef::plugin(send)));
        engine.apply_fader(0, 0.8);
        engine.apply_aux_a(0, 0.5);
        engine.apply_solo(1, true);
        assert_eq!(engine.plugin_send_gain(send, 0), 0.0);
    }

    #[test]
    fn mute_fades_send_before_totalmix_mute() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        let src = engine.config.strips[0].channel_id();
        let main = engine.config.main_output;
        engine.osc.sent_messages(); // drain constructor noise if any
        engine.apply_mute(0, true);
        let after_press = engine.osc.sent_messages();
        assert!(
            !after_press.iter().any(|m| m.address.contains("/mute") && m.values[0].float_value() > 0.5),
            "mute OSC must wait until the fade finishes: {:?}",
            after_press.iter().map(|m| &m.address).collect::<Vec<_>>()
        );
        let mid = engine.mixer.send_level(src, main);
        assert!(mid > 0.0 && mid < 0.8, "first mute step should be a partial fade, got {mid}");
        engine.finish_mute_fades();
        assert!(
            (engine.mixer.send_level(src, main) - 0.8).abs() < 1e-5,
            "after mute, TotalMix must keep the fader so a dump can restore it"
        );
        assert!(engine.osc.sent_messages().iter().any(|m| {
            m.address.contains("/mute") && m.values[0].float_value() > 0.5
        }));
    }

    #[test]
    fn unmute_opens_totalmix_before_fading_up() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        engine.apply_mute(0, true);
        engine.finish_mute_fades();
        assert!((engine.mixer.send_level(engine.config.strips[0].channel_id(), engine.config.main_output) - 0.8).abs() < 1e-5);
        engine.apply_mute(0, false);
        assert!(engine.osc.sent_messages().iter().any(|m| {
            m.address.contains("/mute") && m.values[0].float_value() < 0.5
        }));
        let src = engine.config.strips[0].channel_id();
        let main = engine.config.main_output;
        let mid = engine.mixer.send_level(src, main);
        assert!(mid > 0.0 && mid < 0.8, "first unmute step should be a partial fade, got {mid}");
        engine.finish_mute_fades();
        assert!((engine.mixer.send_level(src, main) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn dump_restores_muted_strip_fader() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.8);
        engine.apply_mute(0, true);
        engine.finish_mute_fades();
        let src = engine.config.strips[0].channel_id();
        let main = engine.config.main_output;
        let json = serde_json::to_string(&engine.config).unwrap();
        let mut restarted = engine_from_config(serde_json::from_str(&json).unwrap());
        ingest_mix(&mut restarted, &format!("/mix/in/{}/{}/faderlin", src.index, main), 0.8);
        apply_inbound(&mut restarted.mixer, "/input/0/mute", &osc::OscValue::Float(1.0));
        restarted.sync_surface_from_total_mix();
        assert!((restarted.surface.strips[0].fader - 0.8).abs() < 1e-5);
        assert!(restarted.strip_muted(0));
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
        assert!(sent
            .iter()
            .any(|m| m.address.ends_with("/fader") && !m.address.contains("faderlin")));
        let off = sent
            .iter()
            .find(|m| m.address == "/mix/in/0/12/fader" || m.address == "/mix/in/0/10/fader");
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
        assert_eq!(tagged, SendDestination::Plugin(uuid::Uuid::from_u128(1)));
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
        engine.add_hardware_preset();
        assert!(engine.config.hardware_presets.len() >= 5);
        let added = engine.config.hardware_presets.last().unwrap();
        assert!(engine
            .config
            .hardware_chains
            .iter()
            .any(|c| c.name == added.name && c.stages == [added.id]));
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
            s.bus == MixerBus::Input
                && s.index == i as i32 * 2
                && s.linked_stereo
                && s.enabled
                && s.has_input
        }));
        assert_eq!(c.plugin_chains[0].name, "FX A");
        assert_eq!(c.plugin_chains[1].name, "FX B");
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
        assert_eq!(c.aux_b, SendDestination::Plugin(uuid::Uuid::from_u128(0)));
        assert_eq!(c.returns[0].id, 2);
        assert!(c.returns[0].effect.is_none());
        c.normalize_after_load();
        assert!(c.plugin_chains.iter().any(|p| p.name == "FX A"));
        assert!(c.sends_post_fader);
        let wire = serde_json::to_value(&SessionConfig::new()).unwrap();
        assert!(wire.get("oscHost").is_some());
        assert!(wire.get("mainOutput").is_some());
        assert!(
            wire.get("projectsRootBookmark").is_none() || wire["projectsRootBookmark"].is_null()
        );
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

    #[test]
    fn totalmix_main_send_drives_ui_and_record_level() {
        let mut engine = test_engine();
        engine.config.main_output = 14;
        engine.mixer.monitored_output = 14;
        engine.config.strips[2].index = 12;
        let src = ChannelID::new(MixerBus::Input, 12);
        engine.mixer.set_send(src, 14, 0.0);
        engine.sync_surface_from_total_mix();
        assert_eq!(engine.strip_main_mix_lin(2), 0.0);
        assert_eq!(engine.surface.strips[2].fader, 0.0);

        engine.mixer.set_send(src, 14, FADER_LIN_0DB);
        engine.mixer.set_send_pan(src, 14, 0.0);
        engine.sync_surface_from_total_mix();
        assert!((engine.strip_main_mix_lin(2) - FADER_LIN_0DB).abs() < 1e-5);
        assert!((engine.surface.strips[2].fader - FADER_LIN_0DB).abs() < 1e-5);
        assert!((engine.strip_main_mix_pan(2) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn record_pan_follows_analog_knob_not_main_send() {
        let mut engine = test_engine();
        engine.config.main_output = 14;
        engine.config.strips[2].index = 12;
        let src = ChannelID::new(MixerBus::Input, 12);
        engine.mixer.set_send_pan(src, 14, 1.0);
        engine.surface.strips[2].pan = 0.25;
        assert!((engine.strip_record_pan(2) - 0.25).abs() < 1e-5);
        assert!((engine.strip_main_mix_pan(2) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn project_strips_replace_the_live_session_map() {
        let mut session = SessionConfig::new();
        let mut project = session.strips.clone();
        project[0].index = 12;
        project[0].enabled = false;
        assert!(session.apply_project_strips(&project));
        assert_eq!(session.strips[0].index, 12);
        assert!(!session.strips[0].enabled);
        assert!(!session.apply_project_strips(&project));
        assert!(!session.apply_project_strips(&[]));
        assert_eq!(session.strips[0].index, 12);
    }

    #[test]
    fn project_catalog_replaces_the_live_session() {
        let mut session = SessionConfig::new();
        let mut presets = session.hardware_presets.clone();
        presets[0].name = "1176".into();
        let mut hardware = session.hardware_chains.clone();
        hardware[0].name = "Comp chain".into();
        let mut plugins = session.plugin_chains.clone();
        plugins[0].name = "Verb".into();
        assert!(session.apply_project_catalog(&presets, &hardware, &plugins));
        assert_eq!(session.hardware_presets[0].name, "1176");
        assert_eq!(session.hardware_chains[0].name, "Comp chain");
        assert_eq!(session.plugin_chains[0].name, "Verb");
        assert!(!session.apply_project_catalog(&presets, &hardware, &plugins));
        assert!(!session.apply_project_catalog(&[], &[], &[]));
        assert_eq!(session.hardware_presets[0].name, "1176");
    }

    #[test]
    fn project_return_chains_replace_the_live_session_map() {
        let mut session = SessionConfig::new();
        let plugin = session.plugin_chains[0].id;
        session.set_return_chain(ReturnLane::SendB, Some(ChainRef::plugin(plugin)));
        let hardware = session.hardware_chains[0].id;
        let mut project = std::collections::HashMap::new();
        project.insert((ReturnLane::SendB as i32).to_string(), ChainRef::hardware(hardware));
        assert!(session.apply_project_return_chains(&project));
        assert_eq!(session.chain_ref(ReturnLane::SendB), Some(ChainRef::hardware(hardware)));
        assert!(!session.apply_project_return_chains(&project));
        assert!(!session.apply_project_return_chains(&std::collections::HashMap::new()));
        assert_eq!(session.chain_ref(ReturnLane::SendB), Some(ChainRef::hardware(hardware)));
    }

    #[test]
    fn return_chain_assignment_is_exclusive() {
        let mut c = SessionConfig::new();
        let id = c.hardware_chains[0].id;
        c.set_return_chain(ReturnLane::SendA, Some(ChainRef::hardware(id)));
        c.set_return_chain(ReturnLane::SendB, Some(ChainRef::hardware(id)));
        assert!(c.chain_ref(ReturnLane::SendA).is_none());
        assert_eq!(c.chain_ref(ReturnLane::SendB).map(|r| r.id), Some(id));
    }

    #[test]
    fn reordering_hardware_chain_stages_changes_send_and_return() {
        let mut c = SessionConfig::new();
        let first = c.hardware_presets[0].clone();
        let second = c.hardware_presets[1].clone();
        let chain = c.add_hardware_chain("Compressor", vec![first.id, second.id]);
        c.set_return_chain(ReturnLane::SendA, Some(ChainRef::hardware(chain)));
        assert_eq!(c.send_destination(ReturnLane::SendA), Some(SendDestination::Output(first.output)));
        assert_eq!(
            c.return_source_id(ReturnLane::SendA).map(|id| id.index),
            Some(second.input)
        );
        c.move_hardware_chain_stage(chain, 0, 1);
        assert_eq!(
            c.send_destination(ReturnLane::SendA),
            Some(SendDestination::Output(second.output))
        );
        assert_eq!(
            c.return_source_id(ReturnLane::SendA).map(|id| id.index),
            Some(first.input)
        );
    }

    #[test]
    fn adding_a_device_creates_a_same_named_chain() {
        let mut c = SessionConfig::new();
        let before = c.hardware_chains.len();
        let id = c.add_hardware_preset(4, 6, "Heat");
        assert_eq!(c.hardware_chains.len(), before + 1);
        assert!(c.hardware_chains.iter().any(|ch| ch.name == "Heat" && ch.stages == [id]));
        c.rename_hardware_preset(id, "Space");
        assert_eq!(c.hardware_preset(id).map(|p| p.name.as_str()), Some("Space"));
        assert!(c.hardware_chains.iter().any(|ch| ch.name == "Space" && ch.stages == [id]));
        assert!(!c.hardware_chains.iter().any(|ch| ch.name == "Heat"));
    }

    #[test]
    fn hardware_chain_duplicate_is_shallow() {
        let mut c = SessionConfig::new();
        let src = c.hardware_chains[0].clone();
        let copy = c.duplicate_hardware_chain(src.id).unwrap();
        let copied = c.hardware_chain(copy).unwrap();
        assert_ne!(copy, src.id);
        assert_eq!(copied.stages, src.stages);
        assert_eq!(copied.name, format!("{} copy", src.name));
    }

    #[test]
    fn names_keep_internal_spaces_while_typing() {
        let mut c = SessionConfig::new();
        let preset = c.hardware_presets[0].id;
        c.rename_hardware_preset(preset, "Big ");
        assert_eq!(c.hardware_preset(preset).map(|p| p.name.as_str()), Some("Big "));
        c.rename_hardware_preset(preset, "Big reverb");
        assert_eq!(c.hardware_preset(preset).map(|p| p.name.as_str()), Some("Big reverb"));

        let hw = c.hardware_chains[0].id;
        c.rename_hardware_chain(hw, "Vocal ");
        c.rename_hardware_chain(hw, "Vocal FX");
        assert_eq!(c.hardware_chain(hw).map(|ch| ch.name.as_str()), Some("Vocal FX"));

        let plugin = c.plugin_chains[0].id;
        c.rename_plugin_chain(plugin, "Plugin ");
        c.rename_plugin_chain(plugin, "Plugin chain");
        assert_eq!(c.plugin_chain(plugin).map(|ch| ch.name.as_str()), Some("Plugin chain"));
    }

    #[test]
    fn plugin_playback_skips_main_and_shows_occupants() {
        let mut c = SessionConfig::new();
        assert_eq!(c.mix_playback_channel(MixLane::Main), 0);
        assert_eq!(c.playback_occupants(0), vec!["Main".to_string()]);
        assert_eq!(c.playback_occupants(2), vec!["FX A".to_string()]);
        assert_eq!(c.next_free_playback_pair(), 6);
        c.set_return_chain(ReturnLane::SendB, Some(ChainRef::plugin(c.plugin_chains[0].id)));
        assert_eq!(c.playback_occupants(2), vec!["Send B · FX A".to_string()]);
        assert_eq!(c.playback_menu_label(2), "3/4 — Send B · FX A");
        assert_eq!(c.playback_menu_label(8), "9/10");
        let a = c.plugin_chains[0].id;
        let b = c.plugin_chains[1].id;
        assert!(c.playback_pair_selectable(2, a));
        assert!(!c.playback_pair_selectable(0, a));
        assert!(!c.playback_pair_selectable(4, a));
        assert!(c.playback_pair_selectable(6, a));
        assert!(c.playback_pair_selectable(4, b));
        assert!(!c.playback_pair_selectable(12, a), "Bus 1 / hardware I/O is not a playback pair");
        assert!(c.playback_occupants(12).iter().any(|n| n.contains("Device")));
    }

    #[test]
    fn plugin_playback_is_cut_from_hardware_fx() {
        let mut engine = test_engine();
        let heat = engine.config.mix_bus1;
        engine.config.plugin_chains[0].return_channel = heat;
        let src = ChannelID::new(MixerBus::Playback, heat);
        engine.mixer.set_send(src, heat, 1.0);
        engine.mixer.set_send(src, engine.config.main_output, 0.75);
        engine.apply_all_returns();
        assert_eq!(
            engine.mixer.send_level(src, heat),
            0.0,
            "plugin wet must not 1:1 onto Heat / Bus 1"
        );
        assert!(
            (engine.mixer.send_level(src, engine.config.main_output) - 0.75).abs() < 1e-5,
            "Main mix of the playback pair stays under the return fader"
        );
    }

    #[test]
    fn plugin_chain_duplicate_deep_copies_stages() {
        let mut c = SessionConfig::new();
        let src_id = c.plugin_chains[0].id;
        c.add_plugin_stage(src_id);
        let src = c.plugin_chain(src_id).unwrap().clone();
        let copy = c.duplicate_plugin_chain(src_id).unwrap();
        let copied = c.plugin_chain(copy).unwrap();
        assert_ne!(copy, src_id);
        assert_eq!(copied.stages.len(), src.stages.len());
        assert_ne!(copied.return_channel, src.return_channel);
        assert_eq!(copied.return_channel, 6);
        for (a, b) in src.stages.iter().zip(&copied.stages) {
            assert_ne!(a.id, b.id);
        }
    }

    #[test]
    fn legacy_effects_migrate_once_with_stable_ids() {
        let json = r#"{
            "strips":[{"id":0,"bus":"input","index":0,"linkedStereo":true,"enabled":true}],
            "mainOutput":0,
            "auxA":14,
            "auxB":16,
            "mixBus1":12,
            "mixBus2":10,
            "oscHost":"127.0.0.1",
            "oscSendPort":7001,
            "oscListenPort":9001,
            "midiDeviceContains":"Launch Control XL",
            "hardwareEffects":[{"id":0,"name":"Heat","output":20,"input":22}],
            "plugins":[{"id":0,"name":"Space","sendOutput":0,"inputChannel":0,"returnChannel":4,"returnDest":0,"bypassed":false}],
            "returns":[{"id":0,"input":0,"effect":{"kind":"hardware","id":0},"fader":0,"pan":0.5,"name":""}]
        }"#;
        let mut c: SessionConfig = serde_json::from_str(json).unwrap();
        c.normalize_after_load();
        assert_eq!(c.hardware_presets.len(), 1);
        assert_eq!(c.hardware_chains.len(), 1);
        assert_eq!(c.plugin_chains.len(), 1);
        assert_eq!(c.hardware_presets[0].name, "Heat");
        assert_eq!(c.plugin_chains[0].name, "Space");
        assert_eq!(c.plugin_chains[0].return_channel, 4);
        let hw = c.hardware_chains[0].id;
        assert_eq!(c.chain_ref(ReturnLane::SendA).map(|r| r.id), Some(hw));
        assert!(c.returns.iter().all(|r| r.effect.is_none()));
        let first = c.hardware_presets[0].id;
        c.normalize_after_load();
        assert_eq!(c.hardware_presets[0].id, first);
    }

    #[test]
    fn inbound_mix_updates_ui_without_writing_back() {
        let mut engine = test_engine();
        let dest = engine.config.main_output;
        let src = engine.config.strips[0].channel_id();
        let addr = format!("/mix/in/{}/{}/faderlin", src.index, dest);
        match apply_inbound(&mut engine.mixer, &addr, &osc::OscValue::Float(0.8)) {
            Some(MixEvent::Mix(node)) => engine.apply_inbound_mix(&node),
            other => panic!("expected mix event, got {other:?}"),
        }
        engine.sync_surface_from_total_mix();
        assert!((engine.surface.strips[0].fader - 0.8).abs() < 1e-5);
        assert!((engine.strip_main_mix_lin(0) - 0.8).abs() < 1e-5);
        assert!(engine.osc.sent_messages().is_empty());
    }

    #[test]
    fn missing_dump_volume_is_pushed_to_totalmix() {
        let mut engine = test_engine();
        engine.surface.strips[0].fader = 0.6;
        engine.push_unreported_volumes();
        let dest = engine.config.main_output;
        let src = engine.config.strips[0].channel_id();
        assert!((engine.mixer.send_level(src, dest) - 0.6).abs() < 1e-5);
        assert!(engine.osc.sent_messages().iter().any(|m| {
            m.address == format!("/mix/in/{}/{}/faderlin", src.index, dest)
                && (m.values[0].float_value() - 0.6).abs() < 1e-5
        }));
    }

    #[test]
    fn silent_unreported_nodes_leave_totalmix_alone() {
        let mut engine = test_engine();
        let dest = engine.config.main_output;
        let src = engine.config.strips[0].channel_id();
        let bus1 = engine.config.listen_output(MixAssign::Bus1).unwrap();
        engine.mixer.set_send(src, bus1, 0.77);
        engine.mixer.set_send(src, dest, 0.0);
        engine.surface.strips[0].fader = 0.0;
        engine.surface.strips[0].assign = MixAssign::Main;
        engine.push_unreported_volumes();
        assert!(
            (engine.mixer.send_level(src, bus1) - 0.77).abs() < 1e-5,
            "dump backfill must not zero a Bus send TotalMix still has"
        );
        assert!(!engine.osc.sent_messages().iter().any(|m| {
            m.address.contains(&format!("/{}/faderlin", bus1))
                || m.address.contains(&format!("/{dest}/faderlin"))
        }));
    }

    #[test]
    fn dump_restores_sends_and_plugin_return_without_writing_zero() {
        let mut engine = test_engine();
        let mut stage = PluginStage::new("Galaxy");
        stage.bundle_path = Some("/tmp/Galaxy.vst3".into());
        engine.config.plugin_chains[0].return_channel = 4;
        engine.config.plugin_chains[0].stages.push(stage);
        let galaxy = engine.config.plugin_chains[0].id;
        engine.config.set_return_chain(ReturnLane::SendC, Some(ChainRef::plugin(galaxy)));

        let src = engine.config.strips[0].channel_id();
        let main = engine.config.main_output;
        let send_a = engine.config.hardware_output(ReturnLane::SendA).expect("Send A hardware");
        let fader = 0.8;
        let send = 0.5;
        let post = post_fader_lin(send, fader);
        let return_level = 0.66;

        ingest_mix(&mut engine, &format!("/mix/in/{}/{}/faderlin", src.index, main), fader);
        ingest_mix(&mut engine, &format!("/mix/in/{}/{}/faderlin", src.index, send_a), post);
        ingest_mix(&mut engine, &format!("/mix/pb/4/{main}/faderlin"), return_level);
        engine.sync_surface_from_total_mix();
        engine.push_unreported_volumes();

        assert!((engine.surface.strips[0].fader - fader).abs() < 1e-5);
        assert!((engine.surface.strips[0].aux(ReturnLane::SendA) - send).abs() < 1e-5);
        let galaxy_fader = engine
            .surface
            .returns
            .iter()
            .find(|r| r.id == ReturnLane::SendC as i32)
            .map(|r| r.fader)
            .unwrap_or(0.0);
        assert!((galaxy_fader - return_level).abs() < 1e-5);
        assert!(
            engine.osc.sent_messages().is_empty(),
            "dump backfill must not push MixLink's default-0 surface: {:?}",
            engine.osc.sent_messages()
        );
        assert!((engine.mixer.send_level(src, send_a) - post).abs() < 1e-5);
        assert!(
            (engine.mixer.send_level(ChannelID::new(MixerBus::Playback, 4), main) - return_level)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn plugin_sends_survive_restart_hardware_send_comes_from_dump() {
        let mut engine = test_engine();
        assign_plugin_send(&mut engine, ReturnLane::SendB, "Shimmerer", 2);
        assign_plugin_send(&mut engine, ReturnLane::SendC, "Galaxy", 4);
        engine.apply_aux(0, ReturnLane::SendA, 0.4);
        engine.apply_aux(0, ReturnLane::SendB, 0.55);
        engine.apply_aux(1, ReturnLane::SendC, 0.31);
        engine.apply_return_fader(ReturnLane::SendC as i32, 0.66);
        engine.apply_return_fader(ReturnLane::SendA as i32, 0.8);

        let json = serde_json::to_value(&engine.config).unwrap();
        let sends = json.get("pluginSends").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        assert_eq!(sends.len(), 2, "{sends:?}");
        assert_eq!(sends[0]["id"], 0);
        assert!((sends[0]["auxB"].as_f64().unwrap() - 0.55).abs() < 1e-5);
        assert!(sends[0].get("auxA").is_none(), "hardware Send A must not persist: {sends:?}");
        assert_eq!(sends[1]["id"], 1);
        assert!((sends[1]["auxC"].as_f64().unwrap() - 0.31).abs() < 1e-5);
        let heat = engine.config.returns.iter().find(|r| r.id == ReturnLane::SendA as i32);
        assert!(
            heat.is_none_or(|r| r.fader == 0.0),
            "hardware Heat return must not persist: {heat:?}"
        );
        let json = serde_json::to_string(&engine.config).unwrap();

        let config: SessionConfig = serde_json::from_str(&json).unwrap();
        let mut restarted = engine_from_config(config);
        assert!((restarted.surface.strips[0].aux(ReturnLane::SendB) - 0.55).abs() < 1e-5);
        assert!((restarted.surface.strips[1].aux(ReturnLane::SendC) - 0.31).abs() < 1e-5);
        assert_eq!(restarted.surface.strips[0].aux(ReturnLane::SendA), 0.0);
        let galaxy = restarted
            .surface
            .returns
            .iter()
            .find(|r| r.id == ReturnLane::SendC as i32)
            .map(|r| r.fader)
            .unwrap_or(0.0);
        assert!((galaxy - 0.66).abs() < 1e-5);

        let src = restarted.config.strips[0].channel_id();
        let send_a = restarted.config.hardware_output(ReturnLane::SendA).expect("Send A hardware");
        let main = restarted.config.main_output;
        let fader = 0.8;
        let send = 0.42;
        let post = post_fader_lin(send, fader);
        ingest_mix(&mut restarted, &format!("/mix/in/{}/{}/faderlin", src.index, main), fader);
        ingest_mix(&mut restarted, &format!("/mix/in/{}/{}/faderlin", src.index, send_a), post);
        restarted.sync_surface_from_total_mix();
        restarted.pull_send_levels_from_total_mix(None, None, true);
        restarted.push_unreported_volumes();

        assert!((restarted.surface.strips[0].aux(ReturnLane::SendA) - send).abs() < 1e-5);
        assert!((restarted.surface.strips[0].aux(ReturnLane::SendB) - 0.55).abs() < 1e-5);
        assert!((restarted.surface.strips[1].aux(ReturnLane::SendC) - 0.31).abs() < 1e-5);
        assert!(
            (restarted
                .surface
                .returns
                .iter()
                .find(|r| r.id == ReturnLane::SendC as i32)
                .map(|r| r.fader)
                .unwrap_or(0.0)
                - 0.66)
                .abs()
                < 1e-5
        );
        assert!(
            !restarted.osc.sent_messages().iter().any(|m| {
                m.address.contains("faderlin") && m.values[0].float_value() <= LIN_EPS
            }),
            "dump backfill must not write 0: {:?}",
            restarted.osc.sent_messages()
        );
    }

    #[test]
    fn dump_plugin_return_wins_when_totalmix_reports_it() {
        let mut engine = test_engine();
        assign_plugin_send(&mut engine, ReturnLane::SendC, "Galaxy", 4);
        engine.apply_return_fader(ReturnLane::SendC as i32, 0.66);
        let json = serde_json::to_string(&engine.config).unwrap();
        let mut restarted = engine_from_config(serde_json::from_str(&json).unwrap());
        let main = restarted.config.main_output;
        ingest_mix(&mut restarted, &format!("/mix/pb/4/{main}/faderlin"), 0.5);
        restarted.sync_surface_from_total_mix();
        let galaxy = restarted
            .surface
            .returns
            .iter()
            .find(|r| r.id == ReturnLane::SendC as i32)
            .map(|r| r.fader)
            .unwrap_or(0.0);
        assert!((galaxy - 0.5).abs() < 1e-5);
        assert!(
            (restarted.config.return_lane(ReturnLane::SendC as i32).map(|r| r.fader).unwrap_or(0.0)
                - 0.5)
                .abs()
                < 1e-5
        );
    }

    #[test]
    fn dump_infers_bus_assign_before_pulling_fader() {
        let mut engine = test_engine();
        let src = engine.config.strips[0].channel_id();
        let bus1 = engine.config.listen_output(MixAssign::Bus1).unwrap();
        ingest_mix(&mut engine, &format!("/mix/in/{}/{}/faderlin", src.index, bus1), 0.7);
        engine.surface.strips[0].assign = MixAssign::Main;
        engine.surface.strips[0].fader = 0.0;
        engine.sync_surface_from_total_mix();
        assert_eq!(engine.surface.strips[0].assign, MixAssign::Bus1);
        assert!((engine.surface.strips[0].fader - 0.7).abs() < 1e-5);
        assert!(engine.osc.sent_messages().is_empty());
    }

    #[test]
    fn backfill_does_not_zero_other_assign_dests() {
        let mut engine = test_engine();
        let src = engine.config.strips[0].channel_id();
        let bus1 = engine.config.listen_output(MixAssign::Bus1).unwrap();
        engine.mixer.set_send(src, bus1, 0.77);
        engine.surface.strips[0].fader = 0.6;
        engine.surface.strips[0].assign = MixAssign::Main;
        engine.push_unreported_volumes();
        assert!(
            (engine.mixer.send_level(src, bus1) - 0.77).abs() < 1e-5,
            "backfill must not write_assign_sends (zeros Bus dests)"
        );
        assert!(!engine.osc.sent_messages().iter().any(|m| {
            m.address.contains(&format!("/{}/faderlin", bus1)) && m.values[0].float_value() == 0.0
        }));
    }

    #[test]
    fn isolate_playback_skips_main_and_unreported() {
        let mut engine = test_engine();
        let heat = engine.config.mix_bus1;
        let main = engine.config.main_output;
        engine.config.plugin_chains[0].return_channel = heat;
        let src = ChannelID::new(MixerBus::Playback, heat);
        engine.mixer.set_send(src, heat, 1.0);
        engine.mixer.set_send(src, main, 0.75);
        engine.isolate_plugin_playback_from_hardware();
        assert_eq!(engine.mixer.send_level(src, heat), 0.0);
        assert!((engine.mixer.send_level(src, main) - 0.75).abs() < 1e-5);
        assert!(!engine.osc.sent_messages().iter().any(|m| {
            m.address == format!("/mix/pb/{heat}/{main}/faderlin")
        }));
    }

    #[test]
    fn apply_all_returns_from_zero_surface_would_wipe_plugin_return() {
        let mut engine = test_engine();
        let mut stage = PluginStage::new("Galaxy");
        stage.bundle_path = Some("/tmp/Galaxy.vst3".into());
        engine.config.plugin_chains[0].return_channel = 4;
        engine.config.plugin_chains[0].stages.push(stage);
        let galaxy = engine.config.plugin_chains[0].id;
        engine.config.set_return_chain(ReturnLane::SendC, Some(ChainRef::plugin(galaxy)));
        let main = engine.config.main_output;
        let src = ChannelID::new(MixerBus::Playback, 4);
        engine.mixer.set_send(src, main, 0.66);
        engine.apply_all_returns();
        assert_eq!(
            engine.mixer.send_level(src, main),
            0.0,
            "boot must not call apply_all_returns with a default-0 surface"
        );
    }

    fn ingest_mix(engine: &mut AnalogEngine, addr: &str, value: f32) {
        match apply_inbound(&mut engine.mixer, addr, &osc::OscValue::Float(value)) {
            Some(MixEvent::Mix(node)) => engine.apply_inbound_mix(&node),
            other => panic!("expected mix event for {addr}, got {other:?}"),
        }
    }

    #[test]
    fn reported_dump_volume_is_not_overwritten() {
        let mut engine = test_engine();
        let dest = engine.config.main_output;
        let src = engine.config.strips[0].channel_id();
        let addr = format!("/mix/in/{}/{}/faderlin", src.index, dest);
        match apply_inbound(&mut engine.mixer, &addr, &osc::OscValue::Float(0.8)) {
            Some(MixEvent::Mix(node)) => engine.apply_inbound_mix(&node),
            other => panic!("expected mix event, got {other:?}"),
        }
        engine.sync_surface_from_total_mix();
        engine.surface.strips[0].fader = 0.6;
        engine.push_unreported_volumes();
        assert!((engine.mixer.send_level(src, dest) - 0.8).abs() < 1e-5);
        let echoed = format!("/mix/in/{}/{}/faderlin", src.index, dest);
        assert!(!engine.osc.sent_messages().iter().any(|m| m.address == echoed));
    }

    #[test]
    fn no_input_clears_hardware_and_label() {
        let mut engine = test_engine();
        engine.apply_fader(0, 0.6);
        let src = engine.config.strips[0].channel_id();
        let dest = engine.config.main_output;
        assert!((engine.mixer.send_level(src, dest) - 0.6).abs() < 1e-5);
        engine.clear_strip_source(0);
        assert!(!engine.config.strips[0].has_input);
        assert!(engine.source_ids(0).is_empty());
        assert_eq!(engine.strip_display_name(0), "No input");
        assert_eq!(engine.mixer.send_level(src, dest), 0.0);
        engine.set_strip_source(0, src);
        assert!(engine.config.strips[0].has_input);
        assert_eq!(engine.source_ids(0), vec![src]);
    }

    #[test]
    fn older_strip_json_defaults_to_having_input() {
        let json = r#"{"id":0,"bus":"input","index":4,"linkedStereo":false,"enabled":true}"#;
        let binding: StripBinding = serde_json::from_str(json).unwrap();
        assert!(binding.has_input);
        assert_eq!(binding.source(), Some(ChannelID::new(MixerBus::Input, 4)));
    }
}
