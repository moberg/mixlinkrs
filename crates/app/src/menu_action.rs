//! Menu command enum and the domain mutations it applies.

use analog::{ChannelID, EffectRef, MixerBus};
use project::MixGrid;
use ui_mixlink::widgets::MenuItem;

use crate::state::AppState;

#[derive(Clone, Debug)]
pub(crate) enum MenuAction {
    StripSource { strip: usize },
    ReturnEffect { lane: analog::ReturnLane },
    HardwareOutput { id: i32 },
    HardwareInput { id: i32 },
    MixOut,
    AudioDevice,
    AudioBuffer,
    PluginBundle { id: i32 },
    PluginPlayback { id: i32 },
    InsertBundle { insert: uuid::Uuid },
    MixContext { id: uuid::Uuid },
    TakeContext { number: i32 },
    Arrange,
    Grid,
    SwitchProject,
}

impl AppState {
    pub(crate) fn apply_menu(&mut self, action: MenuAction, item: &MenuItem) {
        match action {
            MenuAction::StripSource { strip } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface
                        .analog
                        .set_strip_source(strip, ChannelID::new(MixerBus::Input, idx));
                }
            }
            MenuAction::ReturnEffect { lane } => {
                let ref_ = if item.id == "none" || item.id.is_empty() {
                    None
                } else if let Some(id) = item.id.strip_prefix("hw:").and_then(|s| s.parse().ok()) {
                    Some(EffectRef::Hardware(id))
                } else if let Some(id) = item.id.strip_prefix("pl:").and_then(|s| s.parse().ok()) {
                    Some(EffectRef::Plugin(id))
                } else {
                    None
                };
                self.surface.analog.set_return_effect(lane, ref_);
            }
            MenuAction::HardwareOutput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface.analog.set_hardware_effect_io(id, Some(idx), None);
                }
            }
            MenuAction::HardwareInput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.surface.analog.set_hardware_effect_io(id, None, Some(idx));
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
            MenuAction::PluginBundle { id } => {
                let path = if item.id.is_empty() { None } else { Some(item.id.clone()) };
                let name = if item.label == "None" { None } else { Some(item.label.clone()) };
                self.surface.analog.set_plugin_bundle(id, path, name);
                self.load_plugin_slot(id);
            }
            MenuAction::PluginPlayback { id } => {
                if let Ok(pair) = item.id.parse::<i32>() {
                    self.surface.analog.set_plugin_playback(id, pair);
                }
            }
            MenuAction::InsertBundle { insert } => {
                self.set_insert_bundle(insert, &item.id, &item.label);
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
