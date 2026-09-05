//! VST3 slot and mix-insert load on the UI thread.

use project::{MixInsert, MixLane};

use crate::state::AppState;

impl AppState {
    pub(crate) fn load_plugin_slot(&mut self, id: i32) {
        let Some(plugin) = self.analog.config.plugin(id).cloned() else { return };
        let sr = self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let block = self._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        let inst = match plugin.bundle_path.as_deref() {
            Some(path) if !path.is_empty() => {
                vst3_host::load(path, plugin.class_uid.as_deref(), sr, block)
            }
            _ => std::ptr::null_mut(),
        };
        vst3_host::exchange_and_retire(id as u32, inst);
        vst3_host::slot_set_bypass(id as u32, plugin.bypassed);
        if inst.is_null() {
            self.plugin_refs.remove(&id);
        } else {
            vst3_host::set_tempo(inst, self.tempo);
            self.plugin_refs.insert(id, inst);
        }
    }

    pub(crate) fn load_configured_plugins(&mut self) {
        let ids: Vec<i32> =
            self.analog.config.plugins.iter().filter(|p| p.is_loaded()).map(|p| p.id).collect();
        for id in ids {
            self.load_plugin_slot(id);
        }
    }

    pub(crate) fn load_insert(&mut self, id: uuid::Uuid) {
        let Some(mix) = &self.mix else { return };
        let Some(insert) = mix.tracks.iter().flat_map(|t| t.inserts.iter()).find(|i| i.id == id)
        else {
            return;
        };
        let Some(path) = insert.bundle_path.clone() else { return };
        let sr = self._stream.as_ref().map(|s| s.sample_rate() as f64).unwrap_or(48_000.0);
        let block = self._stream.as_ref().map(|s| s.buffer_frames()).unwrap_or(128);
        if let Some(prev) = self.insert_refs.remove(&id) {
            vst3_host::retire_instance(prev);
        }
        let inst = vst3_host::load(&path, insert.class_uid.as_deref(), sr, block);
        if !inst.is_null() {
            vst3_host::set_tempo(inst, self.tempo);
            self.insert_refs.insert(id, inst);
        }
    }

    pub(crate) fn set_insert_bundle(&mut self, id: uuid::Uuid, path: &str, name: &str) {
        if let Some(mut mix) = self.mix.take() {
            for t in &mut mix.tracks {
                if let Some(ins) = t.inserts.iter_mut().find(|i| i.id == id) {
                    if path.is_empty() {
                        ins.bundle_path = None;
                        ins.name.clear();
                    } else {
                        ins.bundle_path = Some(path.to_string());
                        if name != "None" {
                            ins.name = name.to_string();
                        }
                    }
                }
            }
            self.mix = Some(mix);
            self.persist_mix();
        }
        if path.is_empty() {
            if let Some(prev) = self.insert_refs.remove(&id) {
                vst3_host::retire_instance(prev);
            }
        } else {
            self.load_insert(id);
        }
    }

    pub(crate) fn add_insert(&mut self) {
        let lane = self.selected_lane.unwrap_or(MixLane::Main);
        if let Some(mut mix) = self.mix.take() {
            if let Some(track) = mix.tracks.iter_mut().find(|t| t.lane == lane) {
                track.inserts.push(MixInsert::new("Plugin"));
            }
            self.mix = Some(mix);
            self.persist_mix();
        }
    }
}
