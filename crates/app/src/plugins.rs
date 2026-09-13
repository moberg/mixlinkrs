//! VST3 stage load on the UI thread, plus dual-scope chunk state.
//!
//! - **Project** (default when a project is open): `{project}/plugin-stage-…state`
//! - **Global** (lives with the chain in Application Support): `plugin-stage-…state`
//!
//! Loading a project restores project state (falls back to global if missing).
//! Switching the toggle reloads that scope into the live instance. Edits while
//! Global is selected write global state and overwrite the project copy.

use std::path::PathBuf;

use analog::ChainKind;
use project::{
    plugin_stage_global_preview_url, plugin_stage_global_state_url, plugin_stage_preview_url,
    plugin_stage_state_url, MixLane, ProjectStore,
};

use crate::state::{AppState, PluginStateScope};

impl AppState {
    pub(crate) fn load_plugin_stage(&mut self, id: uuid::Uuid) {
        let Some(stage) = self.surface.analog.config.plugin_stage(id).cloned() else { return };
        let sr = self.audio._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let block = self.audio._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        if let Some(prev) = self.audio.plugin_refs.remove(&id) {
            vst3_host::retire_instance(prev);
        }
        let inst = match stage.bundle_path.as_deref() {
            Some(path) if !path.is_empty() => {
                vst3_host::load(path, stage.class_uid.as_deref(), sr, block)
            }
            _ => std::ptr::null_mut(),
        };
        if inst.is_null() {
            return;
        }
        vst3_host::set_tempo(inst, self.timeline.tempo);
        self.audio.plugin_refs.insert(id, inst);
        self.restore_plugin_stage_state(id);
        self.load_plugin_preview(id);
    }

    pub(crate) fn load_configured_plugins(&mut self) {
        let ids: Vec<uuid::Uuid> = self
            .surface
            .analog
            .config
            .plugin_chains
            .iter()
            .flat_map(|c| c.stages.iter())
            .filter(|s| s.is_loaded())
            .map(|s| s.id)
            .collect();
        for id in ids {
            self.audio.plugin_state_scope.entry(id).or_insert(PluginStateScope::Project);
            self.load_plugin_stage(id);
        }
    }

    pub(crate) fn open_plugin_editor(&mut self, id: uuid::Uuid) {
        if !self.audio.plugin_refs.contains_key(&id) {
            self.load_plugin_stage(id);
        }
        let Some(&inst) = self.audio.plugin_refs.get(&id) else {
            return;
        };
        let title = self
            .surface
            .analog
            .config
            .plugin_stage(id)
            .map(|s| s.title())
            .unwrap_or_else(|| "Plugin".into());
        vst3_host::show_editor(inst, &title);
        self.audio.preview_due.insert(id, std::time::Instant::now() + std::time::Duration::from_millis(700));
    }

    pub(crate) fn unload_plugin_stage(&mut self, id: uuid::Uuid) {
        if let Some(prev) = self.audio.plugin_refs.remove(&id) {
            self.save_plugin_stage_state(id, prev);
            vst3_host::retire_instance(prev);
        }
        self.audio.plugin_state_scope.remove(&id);
    }

    pub(crate) fn plugin_state_scope(&self, id: uuid::Uuid) -> PluginStateScope {
        self.audio.plugin_state_scope.get(&id).copied().unwrap_or_default()
    }

    /// Flip Project ↔ Global and reload that scope into the live instance.
    pub(crate) fn toggle_plugin_state_scope(&mut self, id: uuid::Uuid) {
        let prev = self.plugin_state_scope(id);
        let next = prev.toggle();
        if let Some(&inst) = self.audio.plugin_refs.get(&id) {
            // Persist the outgoing scope before switching so edits are not lost.
            self.save_plugin_stage_state_to(id, inst, prev);
        }
        self.audio.plugin_state_scope.insert(id, next);
        self.restore_plugin_stage_state(id);
    }

    pub(crate) fn reset_plugin_scopes_for_project(&mut self) {
        self.audio.plugin_state_scope.clear();
        for chain in &self.surface.analog.config.plugin_chains {
            for stage in &chain.stages {
                if stage.is_loaded() {
                    self.audio
                        .plugin_state_scope
                        .insert(stage.id, PluginStateScope::Project);
                }
            }
        }
    }

    pub(crate) fn flush_plugin_dirty_state(&mut self) {
        let events = vst3_host::take_dirty_events();
        if events.is_empty() {
            return;
        }
        let mut seen = std::collections::HashSet::new();
        for ev in events.into_iter().rev() {
            if !seen.insert(ev.instance as usize) {
                continue;
            }
            let Some((&id, _)) =
                self.audio.plugin_refs.iter().find(|(_, &ptr)| ptr == ev.instance)
            else {
                continue;
            };
            self.save_plugin_stage_state(id, ev.instance);
            if ev.immediate {
                self.capture_plugin_preview(id, ev.instance);
            }
        }
    }

    pub(crate) fn poll_plugin_previews(&mut self) {
        let now = std::time::Instant::now();
        let due: Vec<uuid::Uuid> = self
            .audio
            .preview_due
            .iter()
            .filter(|(_, at)| now >= **at)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.audio.preview_due.remove(&id);
            if let Some(&inst) = self.audio.plugin_refs.get(&id) {
                self.capture_plugin_preview(id, inst);
            }
        }
    }

    fn capture_plugin_preview(&mut self, id: uuid::Uuid, inst: vst3_host::MixLinkVST3Ref) {
        let Some(png) = vst3_host::capture_editor(inst) else {
            return;
        };
        if png.is_empty() {
            return;
        }
        self.audio.plugin_previews.insert(id, png.clone());
        if let Some(path) = self.plugin_preview_write_path(id) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, png);
        }
    }

    fn load_plugin_preview(&mut self, id: uuid::Uuid) {
        if self.audio.plugin_previews.contains_key(&id) {
            return;
        }
        for path in self.plugin_preview_read_paths(id) {
            if let Ok(bytes) = std::fs::read(&path) {
                if !bytes.is_empty() {
                    self.audio.plugin_previews.insert(id, bytes);
                    return;
                }
            }
        }
    }

    fn plugin_preview_write_path(&self, id: uuid::Uuid) -> Option<PathBuf> {
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            return Some(plugin_stage_preview_url(&folder, id));
        }
        Some(plugin_stage_global_preview_url(id))
    }

    fn plugin_preview_read_paths(&self, id: uuid::Uuid) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
            paths.push(plugin_stage_preview_url(&folder, id));
        }
        paths.push(plugin_stage_global_preview_url(id));
        paths
    }

    fn restore_plugin_stage_state(&mut self, id: uuid::Uuid) {
        let Some(&inst) = self.audio.plugin_refs.get(&id) else { return };
        let Some(path) = self.plugin_state_read_path(id) else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        if bytes.is_empty() {
            return;
        }
        let _ = vst3_host::restore_state(inst, &bytes);
    }

    fn save_plugin_stage_state(&mut self, id: uuid::Uuid, inst: vst3_host::MixLinkVST3Ref) {
        let scope = self.plugin_state_scope(id);
        match scope {
            PluginStateScope::Project => {
                if self.plugin_state_write_path(id, PluginStateScope::Project).is_some() {
                    self.save_plugin_stage_state_to(id, inst, PluginStateScope::Project);
                } else {
                    // No open project — keep the chain's global preset up to date.
                    self.save_plugin_stage_state_to(id, inst, PluginStateScope::Global);
                }
            }
            PluginStateScope::Global => {
                self.save_plugin_stage_state_to(id, inst, PluginStateScope::Global);
                // Editing while Global overwrites the project copy too.
                self.save_plugin_stage_state_to(id, inst, PluginStateScope::Project);
            }
        }
    }

    fn save_plugin_stage_state_to(
        &mut self,
        id: uuid::Uuid,
        inst: vst3_host::MixLinkVST3Ref,
        scope: PluginStateScope,
    ) {
        let Some(path) = self.plugin_state_write_path(id, scope) else { return };
        let Some(bytes) = vst3_host::save_state(inst) else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            log::warn!("plugin state write {}: {e}", path.display());
        }
    }

    fn plugin_state_read_path(&self, id: uuid::Uuid) -> Option<PathBuf> {
        let stage = self.surface.analog.config.plugin_stage(id)?;
        let bundle = stage.bundle_path.as_deref().filter(|p| !p.is_empty())?;
        match self.plugin_state_scope(id) {
            PluginStateScope::Global => Some(plugin_stage_global_state_url(id, bundle)),
            PluginStateScope::Project => {
                if let Some(folder) = ProjectStore::current_url(&self.surface.analog.config) {
                    let project = plugin_stage_state_url(&folder, id, bundle);
                    if project.is_file() {
                        return Some(project);
                    }
                }
                // No project copy yet — fall back to the chain's global preset.
                Some(plugin_stage_global_state_url(id, bundle))
            }
        }
    }

    fn plugin_state_write_path(&self, id: uuid::Uuid, scope: PluginStateScope) -> Option<PathBuf> {
        let stage = self.surface.analog.config.plugin_stage(id)?;
        let bundle = stage.bundle_path.as_deref().filter(|p| !p.is_empty())?;
        match scope {
            PluginStateScope::Global => Some(plugin_stage_global_state_url(id, bundle)),
            PluginStateScope::Project => {
                let folder = ProjectStore::current_url(&self.surface.analog.config)?;
                Some(plugin_stage_state_url(&folder, id, bundle))
            }
        }
    }

    pub(crate) fn convert_mix_inserts_to_chains(&mut self) {
        let Some(mut mix) = self.session.mix.take() else { return };
        let mut changed = false;
        for track in &mut mix.tracks {
            if track.effect_chain.is_some() || track.inserts.is_empty() {
                continue;
            }
            let ret = self.surface.analog.config.next_free_playback_pair();
            let name = track.name.clone();
            let chain_id = self.surface.analog.config.add_plugin_chain(&format!("{name} inserts"), ret);
            if let Some(chain) = self.surface.analog.config.plugin_chain_mut(chain_id) {
                chain.stages.clear();
                for insert in &track.inserts {
                    chain.stages.push(analog::PluginStage {
                        id: insert.id,
                        name: insert.name.clone(),
                        bundle_path: insert.bundle_path.clone(),
                        class_uid: insert.class_uid.clone(),
                        bypassed: insert.bypassed,
                    });
                }
            }
            track.effect_chain = Some(analog::ChainRef::plugin(chain_id));
            track.inserts.clear();
            changed = true;
        }
        self.session.mix = Some(mix);
        if changed {
            self.surface.analog.persist();
            self.persist_project_meta();
            self.persist_mix();
            self.load_configured_plugins();
        }
    }

    pub(crate) fn set_mix_chain(&mut self, lane: MixLane, chain: Option<analog::ChainRef>) {
        if let Some(chain) = chain {
            self.clear_chain_elsewhere(chain.id, Some(lane));
        }
        if let Some(mut mix) = self.session.mix.take() {
            if let Some(track) = mix.track_mut(lane) {
                track.effect_chain = chain;
                if !matches!(chain.map(|c| c.kind), Some(ChainKind::Hardware)) {
                    track.hardware_chain_enabled = true;
                }
            }
            self.session.mix = Some(mix);
            self.persist_mix();
        }
        self.persist_project_meta();
        self.publish_schedule();
    }

    pub(crate) fn set_mix_hardware_enabled(&mut self, lane: MixLane, on: bool) {
        if let Some(mut mix) = self.session.mix.take() {
            if let Some(track) = mix.track_mut(lane) {
                track.hardware_chain_enabled = on;
            }
            self.session.mix = Some(mix);
            self.persist_mix();
        }
        self.publish_schedule();
    }

    pub(crate) fn clear_chain_elsewhere(&mut self, id: uuid::Uuid, keep_mix: Option<MixLane>) {
        let lanes: Vec<analog::ReturnLane> = analog::ReturnLane::ALL
            .into_iter()
            .filter(|lane| self.surface.analog.config.chain_ref(*lane).is_some_and(|r| r.id == id))
            .collect();
        for lane in lanes {
            self.surface.analog.set_return_chain(lane, None);
        }
        if let Some(mut mix) = self.session.mix.take() {
            for track in &mut mix.tracks {
                if keep_mix == Some(track.lane) {
                    continue;
                }
                if track.effect_chain.is_some_and(|c| c.id == id) {
                    track.effect_chain = None;
                }
            }
            self.session.mix = Some(mix);
        }
    }

    pub(crate) fn chain_in_use_mix(&self, id: uuid::Uuid) -> Option<MixLane> {
        self.session.mix.as_ref().and_then(|mix| {
            mix.tracks.iter().find(|t| t.effect_chain.is_some_and(|c| c.id == id)).map(|t| t.lane)
        })
    }
}
