//! Overlays, sidebar menus, and text-focus commit.

use std::time::Instant;

use analog::{EffectRef, MixerBus};
use engine_api::UiCommand;
use project::{MixGrid, ProjectMeta, ProjectStore};
use render::Rect;
use ui_mixlink::chrome::{self};
use ui_mixlink::mixer::{self, MixerLayout, StripKind};
use ui_mixlink::overlay::{self, Overlay, TextFocus};
use ui_mixlink::sidebar::SidebarHit;
use ui_mixlink::theme::Layout;
use ui_mixlink::widgets::MenuItem;
use winit::event_loop::ActiveEventLoop;

use crate::menu_action::MenuAction;
use crate::state::{AppState, Chrome};

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
            Overlay::Menu { rect, items } => {
                if overlay::contains(*rect, x, y) {
                    if let Some(i) = overlay::menu_at(*rect, items, x, y) {
                        if let Some(item) = items.get(i).cloned() {
                            if let Some(action) = self.chrome.menu_action.take() {
                                self.apply_menu(action, &item);
                            }
                        }
                    }
                    self.chrome.overlay = None;
                    self.chrome.menu_action = None;
                    return true;
                }
                let switching = matches!(self.chrome.menu_action, Some(MenuAction::SwitchProject));
                self.chrome.overlay = None;
                self.chrome.menu_action = None;
                self.chrome.text_focus = TextFocus::None;
                if switching {
                    let now = Instant::now();
                    let double = self
                        .chrome
                        .last_click
                        .map(|(t, lx, ly)| {
                            t.elapsed().as_millis() < 350 && (x - lx).hypot(y - ly) < 4.0
                        })
                        .unwrap_or(false);
                    self.chrome.last_click = Some((now, x, y));
                    if double
                        && matches!(
                            chrome::hit_chrome(self.chrome.page, w, h, x, y),
                            Some(
                                chrome::ChromeHit::ProjectSelector | chrome::ChromeHit::ProjectMenu
                            )
                        )
                    {
                        self.begin_rename_project();
                    }
                }
                true
            }
        }
    }

    pub(crate) fn choose_projects_root(&mut self) {
        if let Some(path) = crate::native::pick_projects_folder() {
            if let Some(data) = ProjectStore::bookmark_for(&path) {
                self.surface.analog.config.projects_root_bookmark = Some(data);
                self.surface.analog.persist();
                self.reload_mix();
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
            SidebarHit::AddHardware => self.surface.analog.add_hardware_effect(),
            SidebarHit::RemoveHardware(id) => self.surface.analog.remove_hardware_effect(id),
            SidebarHit::EditHardwareName(id) => {
                self.chrome.text_focus = TextFocus::HardwareName(id)
            }
            SidebarHit::HardwareOutput(id) => {
                self.open_output_menu(MenuAction::HardwareOutput { id }, anchor)
            }
            SidebarHit::HardwareInput(id) => {
                self.open_input_menu(MenuAction::HardwareInput { id }, anchor)
            }
            SidebarHit::AddPlugin => self.surface.analog.add_plugin(),
            SidebarHit::RemovePlugin(id) => {
                vst3_host::exchange_and_retire(id as u32, std::ptr::null_mut());
                self.audio.plugin_refs.remove(&id);
                self.surface.analog.remove_plugin(id);
            }
            SidebarHit::EditPluginName(id) => self.chrome.text_focus = TextFocus::PluginName(id),
            SidebarHit::PluginBundle(id) => {
                self.open_plugin_menu(MenuAction::PluginBundle { id }, anchor)
            }
            SidebarHit::PluginEdit(id) => {
                if let Some(&inst) = self.audio.plugin_refs.get(&id) {
                    let title = self
                        .surface
                        .analog
                        .config
                        .plugin(id)
                        .map(|p| p.title())
                        .unwrap_or_else(|| "Plugin".into());
                    vst3_host::show_editor(inst, &title);
                } else {
                    self.load_plugin_slot(id);
                    if let Some(&inst) = self.audio.plugin_refs.get(&id) {
                        let title = self
                            .surface
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
                let on = self.surface.analog.config.plugin(id).map(|p| !p.bypassed).unwrap_or(true);
                self.surface.analog.set_plugin_bypass(id, on);
                vst3_host::slot_set_bypass(id as u32, on);
            }
            SidebarHit::PluginPlayback(id) => self.open_playback_menu(id, anchor),
            SidebarHit::AudioDevice => self.open_device_menu(anchor),
            SidebarHit::AudioBuffer => self.open_buffer_menu(anchor),
            SidebarHit::Channels => self.chrome.open_or_focus_channels(event_loop),
            SidebarHit::Settings => {
                self.chrome.overlay = None;
                self.chrome.menu_action = None;
                if overlay::settings_text_focus(&self.chrome.text_focus) {
                    self.chrome.text_focus = TextFocus::None;
                }
                self.chrome.open_or_focus_settings(event_loop);
            }
            SidebarHit::AddInsert => self.add_insert(),
            SidebarHit::RemoveInsert(id) => {
                if let Some(inst) = self.audio.insert_refs.remove(&id) {
                    vst3_host::retire_instance(inst);
                }
                if let Some(mut mix) = self.session.mix.take() {
                    for t in &mut mix.tracks {
                        t.inserts.retain(|i| i.id != id);
                    }
                    self.session.mix = Some(mix);
                    self.persist_mix();
                }
            }
            SidebarHit::InsertBundle(id) => {
                self.open_plugin_menu(MenuAction::InsertBundle { insert: id }, anchor)
            }
            SidebarHit::InsertEdit(id) => {
                if let Some(&inst) = self.audio.insert_refs.get(&id) {
                    vst3_host::show_editor(inst, "Insert");
                } else {
                    self.load_insert(id);
                    if let Some(&inst) = self.audio.insert_refs.get(&id) {
                        vst3_host::show_editor(inst, "Insert");
                    }
                }
            }
            SidebarHit::InsertBypass(id) => {
                if let Some(mut mix) = self.session.mix.take() {
                    for t in &mut mix.tracks {
                        if let Some(ins) = t.inserts.iter_mut().find(|i| i.id == id) {
                            ins.bypassed = !ins.bypassed;
                        }
                    }
                    self.session.mix = Some(mix);
                    self.persist_mix();
                }
            }
        }
    }
}

impl Chrome {
    pub(crate) fn place_menu(&mut self, anchor: Rect, items: Vec<MenuItem>, action: MenuAction) {
        if let Some(ch) = self.channels.as_mut() {
            ch.menu = None;
        }
        let (w, h) = self.renderer.logical_size();
        let rect = overlay::layout_popup(anchor, &items, w, h);
        self.overlay = Some(Overlay::Menu { rect, items });
        self.menu_action = Some(action);
    }

    pub(crate) fn place_channels_menu(
        &mut self,
        anchor: Rect,
        items: Vec<MenuItem>,
        action: MenuAction,
    ) {
        let (w, h) = match self.channels.as_ref() {
            Some(ch) => ch.renderer.logical_size(),
            None => return,
        };
        let rect = overlay::layout_popup_window(anchor, &items, w, h);
        if let Some(ch) = self.channels.as_mut() {
            ch.menu = Some(Overlay::Menu { rect, items });
        }
        self.overlay = None;
        self.menu_action = Some(action);
    }
}

impl AppState {
    pub(crate) fn name_row_anchor(&self, kind: StripKind) -> Rect {
        let (bx, by, bw, bh) = self.chrome.body_rect();
        let send_count = self.surface.analog.config.effect_return_count as usize;
        let layout = MixerLayout::new(bx, by, bw, bh, send_count);
        let (sx, sw) = mixer::strip_frame(&layout, send_count, kind);
        let (bay_y, bay_h) = mixer::fader_bay_frame(&layout, send_count);
        Rect { x: sx, y: bay_y + bay_h, w: sw, h: Layout::NAME_ROW }
    }

    pub(crate) fn open_name_menu(&mut self, kind: StripKind, _x: f32, _y: f32) {
        let anchor = self.name_row_anchor(kind);
        match kind {
            StripKind::Input(i) => {
                let current =
                    self.surface.analog.config.strips.get(i).map(|s| s.channel_id().index);
                let items: Vec<MenuItem> = self
                    .surface
                    .analog
                    .mixer
                    .strips(MixerBus::Input)
                    .into_iter()
                    .map(|ch| MenuItem {
                        id: ch.id.index.to_string(),
                        label: self.surface.analog.display_name(ch.id),
                        checked: current == Some(ch.id.index),
                        section: None,
                    })
                    .collect();
                self.chrome.place_menu(anchor, items, MenuAction::StripSource { strip: i });
            }
            // MixLink `ReturnEffectMenu` applies to effect AND bus returns
            // (`case .effectReturn, .busReturn`). Main is plain Text — no menu.
            StripKind::Return(lane) => {
                let current = self.surface.analog.config.effect_ref(lane);
                let mut items = vec![MenuItem {
                    id: "none".into(),
                    label: "No effect".into(),
                    checked: current.is_none(),
                    section: None,
                }];
                for hw in &self.surface.analog.config.hardware_effects {
                    items.push(MenuItem {
                        id: format!("hw:{}", hw.id),
                        label: hw.title(),
                        checked: current == Some(EffectRef::Hardware(hw.id)),
                        section: Some("Hardware".into()),
                    });
                }
                for p in &self.surface.analog.config.plugins {
                    items.push(MenuItem {
                        id: format!("pl:{}", p.id),
                        label: p.title(),
                        checked: current == Some(EffectRef::Plugin(p.id)),
                        section: Some("Plugins".into()),
                    });
                }
                self.chrome.place_menu(anchor, items, MenuAction::ReturnEffect { lane });
            }
            StripKind::Main => {}
        }
    }

    pub(crate) fn open_output_menu(&mut self, action: MenuAction, anchor: Rect) {
        let items = self.output_menu_items(&action);
        self.chrome.place_menu(anchor, items, action);
    }

    pub(crate) fn open_channels_mix_out_menu(&mut self, anchor: Rect) {
        let items = self.output_menu_items(&MenuAction::MixOut);
        self.chrome.place_channels_menu(anchor, items, MenuAction::MixOut);
    }

    fn output_menu_items(&self, action: &MenuAction) -> Vec<MenuItem> {
        let current = match action {
            MenuAction::MixOut => Some(self.surface.analog.config.main_output),
            MenuAction::HardwareOutput { id } => self
                .surface
                .analog
                .config
                .hardware_effects
                .iter()
                .find(|e| e.id == *id)
                .map(|e| e.output),
            _ => None,
        };
        self.surface
            .analog
            .mixer
            .strips(MixerBus::Output)
            .into_iter()
            .map(|ch| MenuItem {
                id: ch.id.index.to_string(),
                label: self.surface.analog.output_name(ch.id.index),
                checked: current == Some(ch.id.index),
                section: None,
            })
            .collect()
    }

    pub(crate) fn open_input_menu(&mut self, action: MenuAction, anchor: Rect) {
        let current = match action {
            MenuAction::HardwareInput { id } => self
                .surface
                .analog
                .config
                .hardware_effects
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.input),
            _ => None,
        };
        let items: Vec<MenuItem> = self
            .surface
            .analog
            .mixer
            .strips(MixerBus::Input)
            .into_iter()
            .map(|ch| MenuItem {
                id: ch.id.index.to_string(),
                label: self.surface.analog.display_name(ch.id),
                checked: current == Some(ch.id.index),
                section: None,
            })
            .collect();
        self.chrome.place_menu(anchor, items, action);
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
        self.chrome.place_menu(anchor, items, action);
    }

    pub(crate) fn open_playback_menu(&mut self, id: i32, anchor: Rect) {
        let current = self.surface.analog.config.plugin(id).map(|p| p.return_channel);
        let items: Vec<MenuItem> = self
            .surface
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
        self.chrome.place_menu(anchor, items, MenuAction::PluginPlayback { id });
    }

    pub(crate) fn open_device_menu(&mut self, anchor: Rect) {
        let current = self.surface.analog.config.audio_device_contains.clone();
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
        self.chrome.place_menu(anchor, items, MenuAction::AudioDevice);
    }

    pub(crate) fn open_grid_menu(&mut self) {
        let items: Vec<MenuItem> = MixGrid::ALL
            .iter()
            .map(|grid| MenuItem {
                id: format!("{}", grid.raw()),
                label: grid.help(),
                checked: self.timeline.grid == *grid,
                section: None,
            })
            .collect();
        self.chrome.place_menu(chrome::grid_step_rect(), items, MenuAction::Grid);
    }

    pub(crate) fn open_project_menu(&mut self) {
        let current = ProjectStore::display_name(&self.surface.analog.config);
        let items: Vec<MenuItem> = ProjectStore::list_projects(&self.surface.analog.config)
            .into_iter()
            .map(|name| MenuItem {
                id: name.clone(),
                label: name.clone(),
                checked: current == name,
                section: None,
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let (w, _) = self.chrome.renderer.logical_size();
        self.chrome.place_menu(
            chrome::project_selector_rect(self.chrome.page, w),
            items,
            MenuAction::SwitchProject,
        );
    }

    pub(crate) fn reveal_current_project(&mut self) {
        let config = &self.surface.analog.config;
        let path = ProjectStore::current_url(config).or_else(|| ProjectStore::resolve_root(config));
        if let Some(path) = path {
            crate::native::reveal_in_finder(&path);
        }
    }

    pub(crate) fn create_new_project(&mut self) {
        self.persist_project_meta();
        if self.chrome.page == ui_mixlink::chrome::Page::Mix {
            self.persist_mix();
        }
        if let Err(e) = self.session.project.create_project(&mut self.surface.analog.config) {
            log::warn!("new project: {e}");
        } else {
            self.surface.analog.persist();
            self.reload_mix();
        }
    }

    pub(crate) fn switch_project(&mut self, name: &str) {
        if name.is_empty() {
            return;
        }
        if self.surface.analog.config.current_project_relative.as_deref() == Some(name) {
            return;
        }
        self.persist_project_meta();
        if self.chrome.page == ui_mixlink::chrome::Page::Mix {
            self.persist_mix();
        }
        self.surface.analog.config.current_project_relative = Some(name.to_string());
        self.surface.analog.persist();
        self.reload_mix();
    }

    pub(crate) fn begin_rename_project(&mut self) {
        let Some(name) = self.surface.analog.config.current_project_relative.clone() else {
            return;
        };
        if name.is_empty() {
            return;
        }
        self.chrome.text_focus = TextFocus::ProjectName;
        self.chrome.edit_buf = name;
        self.chrome.edit_replace = true;
        self.chrome.caret_on = true;
        self.chrome.caret_at = Instant::now();
    }

    pub(crate) fn open_buffer_menu(&mut self, anchor: Rect) {
        let current = self.surface.analog.config.audio_buffer_frames.unwrap_or(64);
        let items: Vec<MenuItem> = [32, 64, 128, 256, 512]
            .into_iter()
            .map(|n| MenuItem {
                id: n.to_string(),
                label: format!("{n} frames"),
                checked: current == n,
                section: None,
            })
            .collect();
        self.chrome.place_menu(anchor, items, MenuAction::AudioBuffer);
    }

    pub(crate) fn edit_focus(&mut self, f: impl FnOnce(&mut String)) {
        match self.chrome.text_focus {
            TextFocus::Tempo | TextFocus::ProjectName => {
                f(&mut self.chrome.edit_buf);
            }
            TextFocus::HardwareName(id) => {
                let mut name = self
                    .surface
                    .analog
                    .config
                    .hardware_effects
                    .iter()
                    .find(|h| h.id == id)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.surface.analog.set_hardware_effect_name(id, &name);
            }
            TextFocus::PluginName(id) => {
                let mut name = self
                    .surface
                    .analog
                    .config
                    .plugin(id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.surface.analog.set_plugin_name(id, &name);
            }
            TextFocus::GearAlias(id) => {
                let mut name = self.surface.analog.config.gear_name(id);
                f(&mut name);
                self.surface.analog.set_gear_name(id, &name);
            }
            TextFocus::OscHost => {
                f(&mut self.surface.analog.config.osc_host);
                self.surface.analog.persist();
            }
            TextFocus::OscSend => {
                let mut s = self.surface.analog.config.osc_send_port.to_string();
                f(&mut s);
                if let Ok(v) = s.parse::<u16>() {
                    self.surface.analog.config.osc_send_port = v;
                    self.surface.analog.persist();
                }
            }
            TextFocus::OscListen => {
                let mut s = self.surface.analog.config.osc_listen_port.to_string();
                f(&mut s);
                if let Ok(v) = s.parse::<u16>() {
                    self.surface.analog.config.osc_listen_port = v;
                    self.surface.analog.persist();
                }
            }
            TextFocus::MidiNeedle => {
                f(&mut self.surface.analog.config.midi_device_contains);
                self.surface.analog.persist();
            }
            TextFocus::MixName(_) | TextFocus::TakeName(_) => {
                f(&mut self.chrome.edit_buf);
            }
            TextFocus::None => {}
        }
    }

    pub(crate) fn commit_focus(&mut self) {
        match self.chrome.text_focus {
            TextFocus::Tempo => {
                if let Ok(v) = self.chrome.edit_buf.parse::<f64>() {
                    self.set_tempo(crate::arrange::clamp_tempo(v));
                    self.persist_project_meta();
                }
            }
            TextFocus::ProjectName => {
                if let Err(e) = self
                    .session
                    .project
                    .rename_current(&self.chrome.edit_buf, &mut self.surface.analog.config)
                {
                    log::warn!("rename: {e}");
                } else {
                    self.surface.analog.persist();
                }
            }
            TextFocus::MixName(id) => self.rename_mix(id, self.chrome.edit_buf.clone()),
            TextFocus::TakeName(n) => self.rename_take(n, self.chrome.edit_buf.clone()),
            _ => {}
        }
        self.chrome.text_focus = TextFocus::None;
        self.chrome.edit_replace = false;
    }

    pub(crate) fn begin_tempo_edit(&mut self) {
        self.chrome.text_focus = TextFocus::Tempo;
        self.chrome.edit_buf = crate::arrange::format_tempo(self.timeline.tempo);
        self.chrome.edit_replace = true;
        self.chrome.caret_on = true;
        self.chrome.caret_at = Instant::now();
    }

    pub(crate) fn begin_rename_mix(&mut self, id: uuid::Uuid) {
        let Some(name) = self.session.mixes.iter().find(|m| m.id == id).map(|m| m.name.clone())
        else {
            return;
        };
        self.select_mix(id);
        self.chrome.text_focus = TextFocus::MixName(id);
        self.chrome.edit_buf = name;
        self.chrome.edit_replace = true;
        self.chrome.caret_on = true;
        self.chrome.caret_at = Instant::now();
    }

    pub(crate) fn begin_rename_take(&mut self, number: i32) {
        if !self.session.takes.contains(&number) {
            return;
        }
        self.select_take(number);
        self.chrome.text_focus = TextFocus::TakeName(number);
        self.chrome.edit_buf = ProjectMeta::take_title(
            number,
            self.session.take_names.get(&number).map(String::as_str),
        );
        self.chrome.edit_replace = true;
        self.chrome.caret_on = true;
        self.chrome.caret_at = Instant::now();
    }

    pub(crate) fn set_tempo(&mut self, bpm: f64) {
        let bpm = crate::arrange::clamp_tempo(bpm);
        self.timeline.tempo = bpm;
        let _ = self.audio.engine_handles.cmd_tx.try_push(UiCommand::SetTempo { bpm });
        for inst in self.audio.insert_refs.values() {
            vst3_host::set_tempo(*inst, bpm);
        }
        for inst in self.audio.plugin_refs.values() {
            vst3_host::set_tempo(*inst, bpm);
        }
    }
}
