//! Overlays, sidebar menus, and text-focus commit.

use std::time::Instant;

use analog::{ChannelID, EffectRef, MixerBus};
use engine_api::UiCommand;
use midi_xl::MidiSession;
use project::{MixGrid, ProjectStore};
use render::Rect;
use ui_mixlink::chrome::{self};
use ui_mixlink::mixer::{self, MixerLayout, StripKind};
use ui_mixlink::overlay::{self, MenuAction, Overlay, TextFocus};
use ui_mixlink::sidebar::SidebarHit;
use ui_mixlink::theme::Layout;
use ui_mixlink::widgets::MenuItem;
use winit::event_loop::ActiveEventLoop;

use crate::mix_doc::project_parts;
use crate::state::{AppState, MidiIo};

impl AppState {
    pub(crate) fn handle_overlay_press(
        &mut self,
        overlay: &Overlay,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool {
        match overlay {
            Overlay::Menu { rect, items, action } => {
                if overlay::contains(*rect, x, y) {
                    if let Some(i) = overlay::menu_at(*rect, items, x, y) {
                        if let Some(item) = items.get(i).cloned() {
                            self.apply_menu(action.clone(), &item);
                        }
                    }
                    self.overlay = None;
                    return true;
                }
                self.overlay = None;
                self.text_focus = TextFocus::None;
                true
            }
            Overlay::Settings => {
                let hits = overlay::settings_hits(w, h);
                if !overlay::contains(hits.panel, x, y) {
                    self.overlay = None;
                    self.text_focus = TextFocus::None;
                    return true;
                }
                if overlay::contains(hits.post_fader, x, y) {
                    let on = !self.analog.config.sends_post_fader;
                    self.analog.set_sends_post_fader(on);
                } else if overlay::contains(hits.hardware_strips, x, y) {
                    let on = !self.analog.config.hardware_strips;
                    self.analog.set_hardware_strips(on);
                } else if overlay::contains(hits.osc_host, x, y) {
                    self.text_focus = TextFocus::OscHost;
                } else if overlay::contains(hits.osc_send, x, y) {
                    self.text_focus = TextFocus::OscSend;
                } else if overlay::contains(hits.osc_listen, x, y) {
                    self.text_focus = TextFocus::OscListen;
                } else if overlay::contains(hits.midi, x, y) {
                    self.text_focus = TextFocus::MidiNeedle;
                } else if overlay::contains(hits.apply, x, y) {
                    self.apply_settings();
                }
                true
            }
        }
    }

    pub(crate) fn handle_sidebar(
        &mut self,
        hit: SidebarHit,
        anchor: Rect,
        event_loop: &ActiveEventLoop,
    ) {
        match hit {
            SidebarHit::AddHardware => self.analog.add_hardware_effect(),
            SidebarHit::RemoveHardware(id) => self.analog.remove_hardware_effect(id),
            SidebarHit::EditHardwareName(id) => self.text_focus = TextFocus::HardwareName(id),
            SidebarHit::HardwareOutput(id) => {
                self.open_output_menu(MenuAction::HardwareOutput { id }, anchor)
            }
            SidebarHit::HardwareInput(id) => {
                self.open_input_menu(MenuAction::HardwareInput { id }, anchor)
            }
            SidebarHit::AddPlugin => self.analog.add_plugin(),
            SidebarHit::RemovePlugin(id) => {
                vst3_host::exchange_and_retire(id as u32, std::ptr::null_mut());
                self.plugin_refs.remove(&id);
                self.analog.remove_plugin(id);
            }
            SidebarHit::EditPluginName(id) => self.text_focus = TextFocus::PluginName(id),
            SidebarHit::PluginBundle(id) => {
                self.open_plugin_menu(MenuAction::PluginBundle { id }, anchor)
            }
            SidebarHit::PluginEdit(id) => {
                if let Some(&inst) = self.plugin_refs.get(&id) {
                    let title = self
                        .analog
                        .config
                        .plugin(id)
                        .map(|p| p.title())
                        .unwrap_or_else(|| "Plugin".into());
                    vst3_host::show_editor(inst, &title);
                } else {
                    self.load_plugin_slot(id);
                    if let Some(&inst) = self.plugin_refs.get(&id) {
                        let title = self
                            .analog
                            .config
                            .plugin(id)
                            .map(|p| p.title())
                            .unwrap_or_else(|| "Plugin".into());
                        vst3_host::show_editor(inst, &title);
                    }
                }
            }
            SidebarHit::PluginBypass(id) => {
                let on = self.analog.config.plugin(id).map(|p| !p.bypassed).unwrap_or(true);
                self.analog.set_plugin_bypass(id, on);
                vst3_host::slot_set_bypass(id as u32, on);
            }
            SidebarHit::PluginPlayback(id) => self.open_playback_menu(id, anchor),
            SidebarHit::ProjectsFolder => {
                if let Some(path) = crate::native::pick_projects_folder() {
                    if let Some(data) = ProjectStore::bookmark_for(&path) {
                        self.analog.config.projects_root_bookmark = Some(data);
                        self.analog.persist();
                        self.reload_mix();
                    }
                }
            }
            SidebarHit::ProjectName => self.text_focus = TextFocus::ProjectName,
            SidebarHit::NewProject => {
                if let Err(e) = self.project.create_project(&mut self.analog.config) {
                    log::warn!("new project: {e}");
                } else {
                    self.analog.persist();
                    self.reload_mix();
                }
            }
            SidebarHit::MixOut => self.open_output_menu(MenuAction::MixOut, anchor),
            SidebarHit::AudioDevice => self.open_device_menu(anchor),
            SidebarHit::AudioBuffer => self.open_buffer_menu(anchor),
            SidebarHit::Channels => self.open_or_focus_channels(event_loop),
            SidebarHit::Settings => {
                self.overlay = Some(Overlay::Settings);
                self.text_focus = TextFocus::None;
            }
            SidebarHit::AddInsert => self.add_insert(),
            SidebarHit::RemoveInsert(id) => {
                if let Some(inst) = self.insert_refs.remove(&id) {
                    vst3_host::retire_instance(inst);
                }
                if let Some(mut mix) = self.mix.take() {
                    for t in &mut mix.tracks {
                        t.inserts.retain(|i| i.id != id);
                    }
                    self.mix = Some(mix);
                    self.persist_mix();
                }
            }
            SidebarHit::InsertBundle(id) => {
                self.open_plugin_menu(MenuAction::InsertBundle { insert: id }, anchor)
            }
            SidebarHit::InsertEdit(id) => {
                if let Some(&inst) = self.insert_refs.get(&id) {
                    vst3_host::show_editor(inst, "Insert");
                } else {
                    self.load_insert(id);
                    if let Some(&inst) = self.insert_refs.get(&id) {
                        vst3_host::show_editor(inst, "Insert");
                    }
                }
            }
            SidebarHit::InsertBypass(id) => {
                if let Some(mut mix) = self.mix.take() {
                    for t in &mut mix.tracks {
                        if let Some(ins) = t.inserts.iter_mut().find(|i| i.id == id) {
                            ins.bypassed = !ins.bypassed;
                        }
                    }
                    self.mix = Some(mix);
                    self.persist_mix();
                }
            }
        }
    }

    pub(crate) fn place_menu(&mut self, anchor: Rect, items: Vec<MenuItem>, action: MenuAction) {
        let (w, h) = self.renderer.logical_size();
        let rect = overlay::layout_popup(anchor, &items, w, h);
        self.overlay = Some(Overlay::Menu { rect, items, action });
    }

    pub(crate) fn name_row_anchor(&self, kind: StripKind) -> Rect {
        let (bx, by, bw, bh) = self.body_rect();
        let send_count = self.analog.config.effect_return_count as usize;
        let layout = MixerLayout::new(bx, by, bw, bh, send_count);
        let (sx, sw) = mixer::strip_frame(&layout, send_count, kind);
        let (bay_y, bay_h) = mixer::fader_bay_frame(&layout, send_count);
        Rect { x: sx, y: bay_y + bay_h, w: sw, h: Layout::NAME_ROW }
    }

    pub(crate) fn open_name_menu(&mut self, kind: StripKind, _x: f32, _y: f32) {
        let anchor = self.name_row_anchor(kind);
        match kind {
            StripKind::Input(i) => {
                let current = self.analog.config.strips.get(i).map(|s| s.channel_id().index);
                let items: Vec<MenuItem> = self
                    .analog
                    .mixer
                    .strips(MixerBus::Input)
                    .into_iter()
                    .map(|ch| MenuItem {
                        id: ch.id.index.to_string(),
                        label: self.analog.display_name(ch.id),
                        checked: current == Some(ch.id.index),
                        section: None,
                    })
                    .collect();
                self.place_menu(anchor, items, MenuAction::StripSource { strip: i });
            }
            // MixLink `ReturnEffectMenu` applies to effect AND bus returns
            // (`case .effectReturn, .busReturn`). Main is plain Text — no menu.
            StripKind::Return(lane) => {
                let current = self.analog.config.effect_ref(lane);
                let mut items = vec![MenuItem {
                    id: "none".into(),
                    label: "No effect".into(),
                    checked: current.is_none(),
                    section: None,
                }];
                for hw in &self.analog.config.hardware_effects {
                    items.push(MenuItem {
                        id: format!("hw:{}", hw.id),
                        label: hw.title(),
                        checked: current == Some(EffectRef::Hardware(hw.id)),
                        section: Some("Hardware".into()),
                    });
                }
                for p in &self.analog.config.plugins {
                    items.push(MenuItem {
                        id: format!("pl:{}", p.id),
                        label: p.title(),
                        checked: current == Some(EffectRef::Plugin(p.id)),
                        section: Some("Plugins".into()),
                    });
                }
                self.place_menu(anchor, items, MenuAction::ReturnEffect { lane });
            }
            StripKind::Main => {}
        }
    }

    pub(crate) fn open_output_menu(&mut self, action: MenuAction, anchor: Rect) {
        let current = match action {
            MenuAction::MixOut => Some(self.analog.config.main_output),
            MenuAction::HardwareOutput { id } => {
                self.analog.config.hardware_effects.iter().find(|e| e.id == id).map(|e| e.output)
            }
            _ => None,
        };
        let items: Vec<MenuItem> = self
            .analog
            .mixer
            .strips(MixerBus::Output)
            .into_iter()
            .map(|ch| MenuItem {
                id: ch.id.index.to_string(),
                label: self.analog.output_name(ch.id.index),
                checked: current == Some(ch.id.index),
                section: None,
            })
            .collect();
        self.place_menu(anchor, items, action);
    }

    pub(crate) fn open_input_menu(&mut self, action: MenuAction, anchor: Rect) {
        let current = match action {
            MenuAction::HardwareInput { id } => {
                self.analog.config.hardware_effects.iter().find(|e| e.id == id).map(|e| e.input)
            }
            _ => None,
        };
        let items: Vec<MenuItem> = self
            .analog
            .mixer
            .strips(MixerBus::Input)
            .into_iter()
            .map(|ch| MenuItem {
                id: ch.id.index.to_string(),
                label: self.analog.display_name(ch.id),
                checked: current == Some(ch.id.index),
                section: None,
            })
            .collect();
        self.place_menu(anchor, items, action);
    }

    pub(crate) fn open_plugin_menu(&mut self, action: MenuAction, anchor: Rect) {
        let mut items = vec![MenuItem {
            id: String::new(),
            label: "None".into(),
            checked: false,
            section: None,
        }];
        for p in vst3_host::scan_plugins() {
            items.push(MenuItem {
                id: p.bundle_path,
                label: p.name,
                checked: false,
                section: Some("VST3".into()),
            });
        }
        self.place_menu(anchor, items, action);
    }

    pub(crate) fn open_playback_menu(&mut self, id: i32, anchor: Rect) {
        let current = self.analog.config.plugin(id).map(|p| p.return_channel);
        let items: Vec<MenuItem> = self
            .analog
            .mixer
            .strips(MixerBus::Playback)
            .into_iter()
            .map(|ch| MenuItem {
                id: ch.id.index.to_string(),
                label: format!("{}/{}", ch.id.index + 1, ch.id.index + 2),
                checked: current == Some(ch.id.index),
                section: None,
            })
            .collect();
        self.place_menu(anchor, items, MenuAction::PluginPlayback { id });
    }

    pub(crate) fn open_device_menu(&mut self, anchor: Rect) {
        let current = self.analog.config.audio_device_contains.clone();
        let items: Vec<MenuItem> = audio_io::enumerate_devices()
            .into_iter()
            .filter(|d| d.usable())
            .map(|d| MenuItem {
                id: d.name.clone(),
                label: format!("{}  {}/{}", d.name, d.inputs, d.outputs),
                checked: !current.is_empty() && d.name.contains(&current),
                section: None,
            })
            .collect();
        self.place_menu(anchor, items, MenuAction::AudioDevice);
    }

    pub(crate) fn open_grid_menu(&mut self) {
        let items: Vec<MenuItem> = MixGrid::ALL
            .iter()
            .map(|grid| MenuItem {
                id: format!("{}", grid.raw()),
                label: grid.help(),
                checked: self.grid == *grid,
                section: None,
            })
            .collect();
        self.place_menu(chrome::grid_step_rect(), items, MenuAction::Grid);
    }

    pub(crate) fn open_buffer_menu(&mut self, anchor: Rect) {
        let current = self.analog.config.audio_buffer_frames.unwrap_or(64);
        let items: Vec<MenuItem> = [32, 64, 128, 256, 512]
            .into_iter()
            .map(|n| MenuItem {
                id: n.to_string(),
                label: format!("{n} frames"),
                checked: current == n,
                section: None,
            })
            .collect();
        self.place_menu(anchor, items, MenuAction::AudioBuffer);
    }

    pub(crate) fn apply_menu(&mut self, action: MenuAction, item: &MenuItem) {
        match action {
            MenuAction::StripSource { strip } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.analog.set_strip_source(strip, ChannelID::new(MixerBus::Input, idx));
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
                self.analog.set_return_effect(lane, ref_);
            }
            MenuAction::HardwareOutput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.analog.set_hardware_effect_io(id, Some(idx), None);
                }
            }
            MenuAction::HardwareInput { id } => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.analog.set_hardware_effect_io(id, None, Some(idx));
                }
            }
            MenuAction::MixOut => {
                if let Ok(idx) = item.id.parse::<i32>() {
                    self.analog.set_main_output(idx);
                }
            }
            MenuAction::AudioDevice => {
                self.analog.set_audio_device(&item.id);
                self.restart_audio();
            }
            MenuAction::AudioBuffer => {
                if let Ok(n) = item.id.parse::<i32>() {
                    self.analog.set_audio_buffer_frames(n);
                    self.restart_audio();
                }
            }
            MenuAction::PluginBundle { id } => {
                let path = if item.id.is_empty() { None } else { Some(item.id.clone()) };
                let name = if item.label == "None" { None } else { Some(item.label.clone()) };
                self.analog.set_plugin_bundle(id, path, name);
                self.load_plugin_slot(id);
            }
            MenuAction::PluginPlayback { id } => {
                if let Ok(pair) = item.id.parse::<i32>() {
                    self.analog.set_plugin_playback(id, pair);
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
                    self.grid = MixGrid::from_raw(raw);
                    self.grid_enabled = true;
                    self.persist_project_meta();
                }
            }
        }
    }

    pub(crate) fn edit_focus(&mut self, f: impl FnOnce(&mut String)) {
        match self.text_focus {
            TextFocus::Tempo => {
                f(&mut self.edit_buf);
            }
            TextFocus::ProjectName => {
                let (_, mut suffix) = project_parts(
                    self.analog.config.current_project_relative.as_deref().unwrap_or(""),
                );
                f(&mut suffix);
                if let Err(e) = self.project.rename_current(&suffix, &mut self.analog.config) {
                    log::warn!("rename: {e}");
                } else {
                    self.analog.persist();
                }
            }
            TextFocus::HardwareName(id) => {
                let mut name = self
                    .analog
                    .config
                    .hardware_effects
                    .iter()
                    .find(|h| h.id == id)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.analog.set_hardware_effect_name(id, &name);
            }
            TextFocus::PluginName(id) => {
                let mut name =
                    self.analog.config.plugin(id).map(|p| p.name.clone()).unwrap_or_default();
                f(&mut name);
                self.analog.set_plugin_name(id, &name);
            }
            TextFocus::GearAlias(id) => {
                let mut name = self.analog.config.gear_name(id);
                f(&mut name);
                self.analog.set_gear_name(id, &name);
            }
            TextFocus::OscHost => {
                f(&mut self.analog.config.osc_host);
                self.analog.persist();
            }
            TextFocus::OscSend => {
                let mut s = self.analog.config.osc_send_port.to_string();
                f(&mut s);
                if let Ok(v) = s.parse::<u16>() {
                    self.analog.config.osc_send_port = v;
                    self.analog.persist();
                }
            }
            TextFocus::OscListen => {
                let mut s = self.analog.config.osc_listen_port.to_string();
                f(&mut s);
                if let Ok(v) = s.parse::<u16>() {
                    self.analog.config.osc_listen_port = v;
                    self.analog.persist();
                }
            }
            TextFocus::MidiNeedle => {
                f(&mut self.analog.config.midi_device_contains);
                self.analog.persist();
            }
            TextFocus::None => {}
        }
    }

    pub(crate) fn commit_focus(&mut self) {
        match self.text_focus {
            TextFocus::Tempo => {
                if let Ok(v) = self.edit_buf.parse::<f64>() {
                    self.set_tempo(crate::arrange::clamp_tempo(v));
                    self.persist_project_meta();
                }
            }
            TextFocus::ProjectName => self.reload_mix(),
            _ => {}
        }
        self.text_focus = TextFocus::None;
        self.edit_replace = false;
    }

    pub(crate) fn begin_tempo_edit(&mut self) {
        self.text_focus = TextFocus::Tempo;
        self.edit_buf = crate::arrange::format_tempo(self.tempo);
        self.edit_replace = true;
        self.caret_on = true;
        self.caret_at = Instant::now();
    }

    pub(crate) fn set_tempo(&mut self, bpm: f64) {
        let bpm = crate::arrange::clamp_tempo(bpm);
        self.tempo = bpm;
        let _ = self.engine_handles.cmd_tx.try_push(UiCommand::SetTempo { bpm });
        for inst in self.insert_refs.values() {
            vst3_host::set_tempo(*inst, bpm);
        }
        for inst in self.plugin_refs.values() {
            vst3_host::set_tempo(*inst, bpm);
        }
    }

    pub(crate) fn apply_settings(&mut self) {
        let host = self.analog.config.osc_host.clone();
        let send = self.analog.config.osc_send_port;
        let listen = self.analog.config.osc_listen_port;
        if let Err(e) = self.analog.osc.start(&host, send, listen) {
            log::warn!("OSC reconnect: {e}");
        }
        self.analog.osc.send_dump_requests();
        self.analog.persist();
        self.restart_audio();
        #[cfg(target_os = "macos")]
        {
            match MidiSession::connect(&self.analog.config.midi_device_contains) {
                Ok(s) => {
                    self.midi = MidiIo::Hw(s);
                    self.xl.clear_on_connect();
                    self.last_led = None;
                }
                Err(e) => log::warn!("MIDI: {e}"),
            }
        }
    }
}
