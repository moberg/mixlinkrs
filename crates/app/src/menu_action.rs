//! Menu command enum and the domain mutations it applies.

use analog::{ChainRef, ChannelID, MixerBus};
use project::MixGrid;
use ui_mixlink::widgets::MenuItem;

use crate::chains::DeleteKind;
use crate::state::AppState;

#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    StripSource { strip: usize },
    ReturnEffect { lane: analog::ReturnLane },
    MixChain { lane: project::MixLane },
    HardwarePresetOutput { id: uuid::Uuid },
    HardwarePresetInput { id: uuid::Uuid },
    MixOut,
    AudioDevice,
    AudioBuffer,
    PluginStageBundle { id: uuid::Uuid },
    PluginChainPlayback { id: uuid::Uuid },
    HardwareStagePreset { chain: uuid::Uuid, index: usize },
    MixContext { id: uuid::Uuid },
    TakeContext { number: i32 },
    Arrange,
    Grid,
    SwitchProject,
    ConfirmDelete { kind: DeleteKind, id: uuid::Uuid },
}

impl AppState {
    pub(crate) fn apply_menu(&mut self, action: MenuAction, item: &MenuItem) {
        match action {
            MenuAction::StripSource { strip } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface
                        .analog
                        .set_strip_source(strip, ChannelID::new(MixerBus::Input, idx));
                    self.persist_project_meta();
                }
            }
            MenuAction::ReturnEffect { lane } => {
                let ref_ = parse_chain_item(&item.id);
                if let Some(r) = ref_ {
                    self.clear_chain_elsewhere(r.id, None);
                }
                self.surface.analog.set_return_chain(lane, ref_);
                self.persist_project_meta();
            }
            MenuAction::MixChain { lane } => {
                self.set_mix_chain(lane, parse_chain_item(&item.id));
            }
            MenuAction::HardwarePresetOutput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface.analog.set_hardware_preset_io(id, Some(idx), None);
                }
            }
            MenuAction::HardwarePresetInput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface.analog.set_hardware_preset_io(id, None, Some(idx));
                }
            }
            MenuAction::MixOut => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface.analog.set_main_output(idx);
                }
            }
            MenuAction::AudioDevice => {
                self.surface.analog.set_audio_device(&item.id);
                self.audio.restart_audio(&self.surface.analog.config);
            }
            MenuAction::AudioBuffer => {
                if let Ok(n) = item.id.parse::<i32>() {
                    self.surface.analog.set_audio_buffer_frames(n);
                    self.audio.restart_audio(&self.surface.analog.config);
                }
            }
            MenuAction::PluginStageBundle { id } => {
                let path = if item.id.is_empty() { None } else { Some(item.id.clone()) };
                let name = if item.label == "None" { None } else { Some(item.label.clone()) };
                self.surface.analog.set_plugin_stage_bundle(id, path, name);
                if item.id.is_empty() {
                    self.unload_plugin_stage(id);
                } else {
                    self.load_plugin_stage(id);
                }
            }
            MenuAction::PluginChainPlayback { id } => {
                if let Ok(pair) = item.id.parse::<i32>() {
                    self.surface.analog.set_plugin_chain_playback(id, pair);
                }
            }
            MenuAction::HardwareStagePreset { chain, index } => {
                if let Ok(preset) = item.id.parse::<uuid::Uuid>() {
                    self.surface.analog.set_hardware_chain_stage(chain, index, preset);
                }
            }
            MenuAction::MixContext { id } => {
                if item.id == "delete" {
                    self.delete_mix_id(id);
                } else if let Some(n) = item.id.strip_prefix("take:").and_then(|s| s.parse().ok()) {
                    self.select_mix(id);
                    self.start_from_take(n);
                }
            }
            MenuAction::TakeContext { number } => match item.id.as_str() {
                "start" => self.start_from_take(number),
                "copy" => self.copy_take_to_clipboard(number),
                "delete" => self.delete_take(number),
                _ => {}
            },
            MenuAction::Arrange => match item.id.as_str() {
                "copy" => self.copy_selection(),
                "paste" => self.paste_clips(),
                "duplicate" => self.duplicate_clips(),
                "delete" => self.delete_clips(),
                "split" => self.split_clips(),
                _ => {}
            },
            MenuAction::Grid => {
                if let Ok(raw) = item.id.parse::<f64>() {
                    self.timeline.grid = MixGrid::from_raw(raw);
                    self.timeline.grid_enabled = true;
                    self.persist_project_meta();
                }
            }
            MenuAction::SwitchProject => self.switch_project(&item.id),
            MenuAction::ConfirmDelete { kind, id } => {
                if item.id == "delete" {
                    self.apply_delete(kind, id);
                }
            }
        }
    }

    pub(crate) fn apply_settings(&mut self) {
        let host = self.surface.analog.config.osc_host.clone();
        let send = self.surface.analog.config.osc_send_port;
        let listen = self.surface.analog.config.osc_listen_port;
        if let Err(e) = self.surface.analog.osc.start(&host, send, listen) {
            log::warn!("OSC reconnect: {e}");
        }
        self.surface.analog.osc.send_dump_requests();
        self.surface.analog.persist();
        self.audio.restart_audio(&self.surface.analog.config);
        #[cfg(target_os = "macos")]
        {
            match midi_xl::MidiSession::connect(&self.surface.analog.config.midi_device_contains) {
                Ok(s) => {
                    self.surface.midi = crate::state::MidiIo::Hw(s);
                    self.surface.xl.clear_on_connect();
                    self.surface.last_led = None;
                }
                Err(e) => log::warn!("MIDI: {e}"),
            }
        }
    }
}

fn parse_chain_item(id: &str) -> Option<ChainRef> {
    if id == "none" || id.is_empty() {
        return None;
    }
    if let Some(s) = id.strip_prefix("hw:") {
        return s.parse().ok().map(ChainRef::hardware);
    }
    if let Some(s) = id.strip_prefix("pl:") {
        return s.parse().ok().map(ChainRef::plugin);
    }
    None
}
