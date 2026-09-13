//! Overlays, sidebar menus, and text-focus commit.

use std::time::Instant;

use analog::MixerBus;
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
use winit::keyboard::{Key, NamedKey};

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
            SidebarHit::OpenChains => self.chrome.open_or_focus_chains(event_loop),
            SidebarHit::AssignReturn { lane } => self.open_return_chain_menu(lane, anchor),
            SidebarHit::AssignPlayback { id } => self.open_playback_menu_chain(id, anchor),
            SidebarHit::OpenPlugin { id } => self.open_plugin_editor(id),
            SidebarHit::AssignMix => {
                if let Some(lane) = self.timeline.selected_lane {
                    self.open_mix_chain_menu(lane, anchor);
                }
            }
            SidebarHit::ClearMixChain => {
                if let Some(lane) = self.timeline.selected_lane {
                    self.set_mix_chain(lane, None);
                }
            }
            SidebarHit::ToggleMixHardware => {
                if let Some(lane) = self.timeline.selected_lane {
                    let on = self
                        .session
                        .mix
                        .as_ref()
                        .and_then(|m| m.track(lane))
                        .map(|t| !t.hardware_chain_enabled)
                        .unwrap_or(true);
                    self.set_mix_hardware_enabled(lane, on);
                }
            }
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
        }
    }
}

impl Chrome {
    pub(crate) fn place_menu(&mut self, anchor: Rect, items: Vec<MenuItem>, action: MenuAction) {
        if let Some(ch) = self.channels.as_mut() {
            ch.menu = None;
        }
        if let Some(ch) = self.chains.as_mut() {
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
            StripKind::Return(lane) => self.open_return_chain_menu(lane, anchor),
            StripKind::Main => {}
        }
    }

    pub(crate) fn open_output_menu(&mut self, action: MenuAction, anchor: Rect) {
        let items = self.output_menu_items(&action);
        self.place_active_menu(anchor, items, action);
    }

    pub(crate) fn open_channels_mix_out_menu(&mut self, anchor: Rect) {
        let items = self.output_menu_items(&MenuAction::MixOut);
        self.chrome.place_channels_menu(anchor, items, MenuAction::MixOut);
    }

    fn output_menu_items(&self, action: &MenuAction) -> Vec<MenuItem> {
        let current = match action {
            MenuAction::MixOut => Some(self.surface.analog.config.main_output),
            MenuAction::HardwarePresetOutput { id } => {
                self.surface.analog.config.hardware_preset(*id).map(|e| e.output)
            }
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
            MenuAction::HardwarePresetInput { id } => {
                self.surface.analog.config.hardware_preset(id).map(|e| e.input)
            }
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
        self.place_active_menu(anchor, items, action);
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
        self.place_active_menu(anchor, items, action);
    }

    pub(crate) fn open_return_chain_menu(&mut self, lane: analog::ReturnLane, anchor: Rect) {
        let current = self.surface.analog.config.chain_ref(lane);
        let items = self.chain_menu_items(current, None);
        self.place_active_menu(anchor, items, MenuAction::ReturnEffect { lane });
    }

    pub(crate) fn open_mix_chain_menu(&mut self, lane: project::MixLane, anchor: Rect) {
        let current = self.session.mix.as_ref().and_then(|m| m.track(lane)).and_then(|t| t.effect_chain);
        let items = self.chain_menu_items(current, Some(lane));
        self.chrome.place_menu(anchor, items, MenuAction::MixChain { lane });
    }

    fn chain_menu_items(
        &self,
        current: Option<analog::ChainRef>,
        keep_mix: Option<project::MixLane>,
    ) -> Vec<MenuItem> {
        let mut items = vec![MenuItem {
            id: "none".into(),
            label: "No effect".into(),
            checked: current.is_none(),
            section: None,
        }];
        for chain in &self.surface.analog.config.hardware_chains {
            let used_elsewhere = self.chain_is_taken(chain.id, current, keep_mix);
            if used_elsewhere && current.is_none_or(|c| c.id != chain.id) {
                continue;
            }
            items.push(MenuItem {
                id: format!("hw:{}", chain.id),
                label: chain.title(),
                checked: current.is_some_and(|c| c.id == chain.id),
                section: Some("Hardware".into()),
            });
        }
        for chain in &self.surface.analog.config.plugin_chains {
            let used_elsewhere = self.chain_is_taken(chain.id, current, keep_mix);
            if used_elsewhere && current.is_none_or(|c| c.id != chain.id) {
                continue;
            }
            items.push(MenuItem {
                id: format!("pl:{}", chain.id),
                label: chain.title(),
                checked: current.is_some_and(|c| c.id == chain.id),
                section: Some("Plugins".into()),
            });
        }
        items
    }

    fn chain_is_taken(
        &self,
        id: uuid::Uuid,
        current: Option<analog::ChainRef>,
        keep_mix: Option<project::MixLane>,
    ) -> bool {
        if current.is_some_and(|c| c.id == id) {
            return false;
        }
        if self.surface.analog.config.lane_using_chain(id).is_some() {
            return true;
        }
        self.chain_in_use_mix(id).is_some_and(|lane| keep_mix != Some(lane))
    }

    pub(crate) fn open_preset_menu(&mut self, chain: uuid::Uuid, index: usize, anchor: Rect) {
        let current = self
            .surface
            .analog
            .config
            .hardware_chain(chain)
            .and_then(|c| c.stages.get(index).copied());
        let items: Vec<MenuItem> = self
            .surface
            .analog
            .config
            .hardware_presets
            .iter()
            .map(|p| MenuItem {
                id: p.id.to_string(),
                label: p.title(),
                checked: current == Some(p.id),
                section: None,
            })
            .collect();
        self.place_active_menu(anchor, items, MenuAction::HardwareStagePreset { chain, index });
    }

    pub(crate) fn open_playback_menu_chain(&mut self, id: uuid::Uuid, anchor: Rect) {
        let cfg = &self.surface.analog.config;
        let current = cfg.plugin_chain(id).map(|p| p.return_channel);
        let last = self
            .surface
            .analog
            .mixer
            .strips(MixerBus::Playback)
            .into_iter()
            .map(|ch| ch.id.index)
            .max()
            .unwrap_or(30);
        let items: Vec<MenuItem> = (0..=last)
            .step_by(2)
            .map(|pair| {
                let selectable = cfg.playback_pair_selectable(pair, id);
                MenuItem {
                    id: if selectable { pair.to_string() } else { String::new() },
                    label: cfg.playback_menu_label(pair),
                    checked: current == Some(pair),
                    section: None,
                }
            })
            .collect();
        self.place_active_menu(anchor, items, MenuAction::PluginChainPlayback { id });
    }

    fn place_active_menu(&mut self, anchor: Rect, items: Vec<MenuItem>, action: MenuAction) {
        if self.chrome.chains.is_some() {
            self.chrome.place_chains_menu(anchor, items, action);
        } else if self.chrome.channels.is_some() {
            self.chrome.place_channels_menu(anchor, items, action);
        } else {
            self.chrome.place_menu(anchor, items, action);
        }
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

    pub(crate) fn type_into_focus(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::Space) => {
                self.edit_focus(|s| s.push(' '));
                true
            }
            Key::Character(c) if c.chars().all(|ch| !ch.is_control()) => {
                let add = c.to_string();
                self.edit_focus(|s| s.push_str(&add));
                true
            }
            _ => false,
        }
    }

    pub(crate) fn edit_focus(&mut self, f: impl FnOnce(&mut String)) {
        match self.chrome.text_focus {
            TextFocus::Tempo | TextFocus::ProjectName => {
                f(&mut self.chrome.edit_buf);
            }
            TextFocus::HardwareName(_) | TextFocus::PluginName(_) => {}
            TextFocus::HardwarePresetName(id) => {
                let mut name = self
                    .surface
                    .analog
                    .config
                    .hardware_preset(id)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.surface.analog.set_hardware_preset_name(id, &name);
            }
            TextFocus::HardwareChainName(id) => {
                let mut name = self
                    .surface
                    .analog
                    .config
                    .hardware_chain(id)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.surface.analog.set_hardware_chain_name(id, &name);
            }
            TextFocus::PluginChainName(id) => {
                let mut name = self
                    .surface
                    .analog
                    .config
                    .plugin_chain(id)
                    .map(|h| h.name.clone())
                    .unwrap_or_default();
                f(&mut name);
                self.surface.analog.set_plugin_chain_name(id, &name);
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
            TextFocus::HardwarePresetName(id) => {
                if let Some(name) =
                    self.surface.analog.config.hardware_preset(id).map(|h| h.name.trim().to_string())
                {
                    self.surface.analog.set_hardware_preset_name(id, &name);
                }
            }
            TextFocus::HardwareChainName(id) => {
                if let Some(name) =
                    self.surface.analog.config.hardware_chain(id).map(|h| h.name.trim().to_string())
                {
                    self.surface.analog.set_hardware_chain_name(id, &name);
                }
            }
            TextFocus::PluginChainName(id) => {
                if let Some(name) =
                    self.surface.analog.config.plugin_chain(id).map(|h| h.name.trim().to_string())
                {
                    self.surface.analog.set_plugin_chain_name(id, &name);
                }
            }
            TextFocus::GearAlias(id) => {
                let name = self.surface.analog.config.gear_name(id);
                self.surface.analog.set_gear_name(id, name.trim());
            }
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
