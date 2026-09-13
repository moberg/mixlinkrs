//! Dedicated Chains wgpu window.

use std::sync::Arc;

use analog::AnalogEngine;
use ui_mixlink::chains::{self, ChainsHit, ChainsTab};
use ui_mixlink::overlay::{self, Overlay, TextFocus};
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::window::Window;

use crate::menu_action::MenuAction;
use crate::state::{AppState, ChainsWindow, Chrome};

impl Chrome {
    pub(crate) fn open_or_focus_chains(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(ch) = &self.chains {
            ch.window.set_minimized(false);
            ch.window.focus_window();
            ch.window.request_redraw();
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Effects")
                .with_inner_size(LogicalSize::new(chains::CHAINS_WINDOW_W, chains::CHAINS_WINDOW_H))
                .with_min_inner_size(LogicalSize::new(
                    chains::CHAINS_WINDOW_W,
                    chains::CHAINS_WINDOW_H,
                )),
        ) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                log::error!("chains window: {e}");
                return;
            }
        };
        let zoom = self.renderer.ui_zoom();
        let mut renderer = pollster::block_on(render::Renderer::new(window.clone()));
        renderer.set_ui_zoom(zoom);
        window.focus_window();
        window.request_redraw();
        self.chains = Some(ChainsWindow {
            renderer,
            window,
            cursor: (0.0, 0.0),
            scroll: 0.0,
            tab: ChainsTab::Hardware,
            menu: None,
        });
        self.text_focus = TextFocus::None;
    }

    pub(crate) fn close_chains(&mut self) {
        if matches!(
            self.text_focus,
            TextFocus::HardwarePresetName(_)
                | TextFocus::HardwareChainName(_)
                | TextFocus::PluginChainName(_)
        ) {
            self.text_focus = TextFocus::None;
        }
        if self.chains.as_ref().is_some_and(|c| c.menu.is_some()) {
            self.menu_action = None;
        }
        self.chains = None;
    }

    pub(crate) fn paint_chains_window(
        &mut self,
        analog: &AnalogEngine,
        scope_global: &std::collections::HashMap<uuid::Uuid, bool>,
        previews: &std::collections::HashMap<uuid::Uuid, Vec<u8>>,
    ) {
        let thumbs: std::collections::HashSet<uuid::Uuid> = previews.keys().copied().collect();
        let pngs: Vec<(u128, &[u8])> =
            previews.iter().map(|(id, png)| (id.as_u128(), png.as_slice())).collect();
        let (cmds, scroll) = {
            let Some(ch) = self.chains.as_mut() else { return };
            ch.renderer.sync_thumbs(&pngs);
            let (w, h) = ch.renderer.logical_size();
            let max = chains::chains_max_scroll(analog, ch.tab, w, h);
            let scroll = ch.scroll.min(max);
            let (mut cmds, _) = chains::paint_chains(
                analog,
                w,
                h,
                ch.tab,
                &self.text_focus,
                self.caret_on,
                scroll,
                scope_global,
                &thumbs,
            );
            if let Some(menu) = &ch.menu {
                let Overlay::Menu { rect, items } = menu;
                let hover = overlay::menu_at(*rect, items, ch.cursor.0, ch.cursor.1);
                cmds.extend(overlay::paint_menu(menu, hover));
            }
            (cmds, scroll)
        };
        let Some(ch) = self.chains.as_mut() else { return };
        ch.scroll = scroll;
        if let Err(e) = ch.renderer.render_scene(&cmds) {
            log::error!("chains render: {e}");
        }
    }

    pub(crate) fn on_chains_wheel(&mut self, analog: &AnalogEngine, dy: f32) {
        if self.chains.as_ref().is_some_and(|ch| ch.menu.is_some()) {
            return;
        }
        let (w, h, tab) = match self.chains.as_ref() {
            Some(ch) => {
                let (w, h) = ch.renderer.logical_size();
                (w, h, ch.tab)
            }
            None => return,
        };
        let max = chains::chains_max_scroll(analog, tab, w, h);
        if let Some(ch) = self.chains.as_mut() {
            ch.scroll = (ch.scroll - dy).clamp(0.0, max);
        }
    }

    pub(crate) fn place_chains_menu(
        &mut self,
        anchor: render::Rect,
        items: Vec<ui_mixlink::widgets::MenuItem>,
        action: MenuAction,
    ) {
        let Some(ch) = self.chains.as_ref() else { return };
        let (w, h) = ch.renderer.logical_size();
        let rect = overlay::layout_popup_window(anchor, &items, w, h);
        if let Some(ch) = self.chains.as_mut() {
            ch.menu = Some(Overlay::Menu { rect, items });
        }
        self.menu_action = Some(action);
    }
}

impl AppState {
    pub(crate) fn on_chains_press(&mut self) {
        let (x, y, w, h, scroll, tab) = {
            let Some(ch) = self.chrome.chains.as_ref() else { return };
            let (w, h) = ch.renderer.logical_size();
            (ch.cursor.0, ch.cursor.1, w, h, ch.scroll, ch.tab)
        };
        if let Some(Overlay::Menu { rect, items }) =
            self.chrome.chains.as_ref().and_then(|c| c.menu.clone())
        {
            if overlay::contains(rect, x, y) {
                if let Some(i) = overlay::menu_at(rect, &items, x, y) {
                    if let Some(item) = items.get(i).cloned() {
                        if let Some(action) = self.chrome.menu_action.take() {
                            self.apply_menu(action, &item);
                        }
                    }
                }
            }
            if let Some(ch) = self.chrome.chains.as_mut() {
                ch.menu = None;
            }
            self.chrome.menu_action = None;
            return;
        }
        let scopes: std::collections::HashMap<uuid::Uuid, bool> = self
            .audio
            .plugin_state_scope
            .iter()
            .map(|(id, s)| (*id, *s == crate::state::PluginStateScope::Global))
            .collect();
        let thumbs: std::collections::HashSet<uuid::Uuid> =
            self.audio.plugin_previews.keys().copied().collect();
        let (_, hits) = chains::paint_chains(
            &self.surface.analog,
            w,
            h,
            tab,
            &self.chrome.text_focus,
            false,
            scroll,
            &scopes,
            &thumbs,
        );
        if let Some((rect, hit)) = hits.into_iter().rev().find(|(r, _)| overlay::contains(*r, x, y))
        {
            self.handle_chains_hit(hit, rect);
        } else if matches!(
            self.chrome.text_focus,
            TextFocus::HardwarePresetName(_)
                | TextFocus::HardwareChainName(_)
                | TextFocus::PluginChainName(_)
        ) {
            self.commit_focus();
        }
    }

    fn handle_chains_hit(&mut self, hit: ChainsHit, anchor: render::Rect) {
        match hit {
            ChainsHit::Tab(tab) => {
                if let Some(ch) = self.chrome.chains.as_mut() {
                    ch.tab = tab;
                    ch.scroll = 0.0;
                }
            }
            ChainsHit::Close => self.chrome.close_chains(),
            ChainsHit::AddPreset => {
                self.surface.analog.add_hardware_preset();
                self.persist_project_meta();
            }
            ChainsHit::AddHardwareChain => {
                self.surface.analog.add_hardware_chain();
                self.persist_project_meta();
            }
            ChainsHit::AddPluginChain => {
                self.surface.analog.add_plugin_chain();
                self.persist_project_meta();
            }
            ChainsHit::DuplicatePreset(id) => {
                self.surface.analog.config.duplicate_hardware_preset(id);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
            ChainsHit::DuplicateHardwareChain(id) => {
                self.surface.analog.config.duplicate_hardware_chain(id);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
            ChainsHit::DuplicatePluginChain(id) => {
                self.surface.analog.config.duplicate_plugin_chain(id);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
            ChainsHit::RemovePreset(id) => self.confirm_or_delete(DeleteKind::Preset, id, anchor),
            ChainsHit::RemoveHardwareChain(id) => {
                self.confirm_or_delete(DeleteKind::HardwareChain, id, anchor)
            }
            ChainsHit::RemovePluginChain(id) => {
                self.confirm_or_delete(DeleteKind::PluginChain, id, anchor)
            }
            ChainsHit::EditPresetName(id) => {
                self.chrome.text_focus = TextFocus::HardwarePresetName(id)
            }
            ChainsHit::EditHardwareChainName(id) => {
                self.chrome.text_focus = TextFocus::HardwareChainName(id)
            }
            ChainsHit::EditPluginChainName(id) => {
                self.chrome.text_focus = TextFocus::PluginChainName(id)
            }
            ChainsHit::PresetOutput(id) => {
                self.open_output_menu(MenuAction::HardwarePresetOutput { id }, anchor)
            }
            ChainsHit::PresetInput(id) => {
                self.open_input_menu(MenuAction::HardwarePresetInput { id }, anchor)
            }
            ChainsHit::AddHardwareStage(id) => {
                if let Some(first) = self.surface.analog.config.hardware_presets.first().map(|p| p.id)
                {
                    self.surface.analog.add_hardware_chain_stage(id, first);
                    self.persist_project_meta();
                }
            }
            ChainsHit::AddPluginStage(id) => {
                self.surface.analog.config.add_plugin_stage(id);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
            ChainsHit::HardwareStagePreset { chain, index } => {
                self.open_preset_menu(chain, index, anchor)
            }
            ChainsHit::RemoveHardwareStage { chain, index } => {
                self.surface.analog.remove_hardware_chain_stage_at(chain, index);
                self.persist_project_meta();
            }
            ChainsHit::MoveHardwareStage { chain, index, delta } => {
                self.surface.analog.move_hardware_chain_stage(chain, index, delta);
                self.persist_project_meta();
            }
            ChainsHit::PluginStageBundle(id) => {
                self.open_plugin_menu(MenuAction::PluginStageBundle { id }, anchor)
            }
            ChainsHit::PluginStageEdit(id) | ChainsHit::PluginStageThumb(id) => {
                self.open_plugin_editor(id)
            }
            ChainsHit::PluginStageBypass(id) => {
                let on = self
                    .surface
                    .analog
                    .config
                    .plugin_stage(id)
                    .map(|s| !s.bypassed)
                    .unwrap_or(true);
                self.surface.analog.set_plugin_stage_bypass(id, on);
                self.persist_project_meta();
            }
            ChainsHit::PluginStageScope(id) => self.toggle_plugin_state_scope(id),
            ChainsHit::RemovePluginStage(id) => {
                self.unload_plugin_stage(id);
                self.surface.analog.config.remove_plugin_stage(id);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
            ChainsHit::MovePluginStage { chain, index, delta } => {
                self.surface.analog.config.move_plugin_stage(chain, index, delta);
                self.surface.analog.persist();
                self.persist_project_meta();
            }
        }
    }

    fn confirm_or_delete(&mut self, kind: DeleteKind, id: uuid::Uuid, anchor: render::Rect) {
        let mut used = match kind {
            DeleteKind::Preset => {
                project::ProjectStore::scan_preset_usage(&self.surface.analog.config, id)
            }
            DeleteKind::HardwareChain | DeleteKind::PluginChain => {
                project::ProjectStore::scan_chain_usage(&self.surface.analog.config, id)
            }
        };
        let mut live = Vec::new();
        if let Some(lane) = self.surface.analog.config.lane_using_chain(id) {
            live.push(format!("This session · {}", lane.title()));
        }
        if let Some(lane) = self.chain_in_use_mix(id) {
            live.push(format!("This session · Mix {}", lane.title()));
        }
        let current = project::ProjectStore::display_name(&self.surface.analog.config);
        if !live.is_empty() && !current.is_empty() {
            used.retain(|row| !row.starts_with(&format!("{current} ·")));
        }
        used.splice(0..0, live);
        if used.is_empty() {
            self.apply_delete(kind, id);
            return;
        }
        let mut items: Vec<ui_mixlink::widgets::MenuItem> =
            used.into_iter().take(8).map(|loc| ui_mixlink::widgets::MenuItem::info(loc, "Used in")).collect();
        items.push(ui_mixlink::widgets::MenuItem::action("delete", "Delete anyway", None));
        items.push(ui_mixlink::widgets::MenuItem::action("cancel", "Cancel", None));
        self.chrome.place_chains_menu(anchor, items, MenuAction::ConfirmDelete { kind, id });
    }

    pub(crate) fn apply_delete(&mut self, kind: DeleteKind, id: uuid::Uuid) {
        match kind {
            DeleteKind::Preset => self.surface.analog.remove_hardware_preset(id),
            DeleteKind::HardwareChain => self.surface.analog.remove_hardware_chain(id),
            DeleteKind::PluginChain => {
                if let Some(stages) =
                    self.surface.analog.config.plugin_chain(id).map(|c| c.stages.clone())
                {
                    for stage in stages {
                        self.unload_plugin_stage(stage.id);
                    }
                }
                self.surface.analog.remove_plugin_chain(id);
            }
        }
        self.persist_project_meta();
    }

    pub(crate) fn on_chains_key(&mut self, key: &Key) {
        if matches!(key, Key::Named(NamedKey::Escape)) {
            if self.chrome.chains.as_ref().is_some_and(|c| c.menu.is_some()) {
                if let Some(ch) = self.chrome.chains.as_mut() {
                    ch.menu = None;
                }
                self.chrome.menu_action = None;
                return;
            }
            self.commit_focus();
            return;
        }
        if !matches!(
            self.chrome.text_focus,
            TextFocus::HardwarePresetName(_)
                | TextFocus::HardwareChainName(_)
                | TextFocus::PluginChainName(_)
        ) {
            return;
        }
        match key {
            Key::Named(NamedKey::Enter) => self.commit_focus(),
            Key::Named(NamedKey::Backspace) => self.edit_focus(|s| {
                s.pop();
            }),
            other if self.type_into_focus(other) => {}
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DeleteKind {
    Preset,
    HardwareChain,
    PluginChain,
}
