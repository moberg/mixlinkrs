//! Session map: `~/Library/Application Support/MixLink/session.mixlinkmap`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{
    unique_copy_name, ChainRef, ChannelID, EffectRef, HardwareChain, HardwareEffect,
    HardwarePreset, MixAssign, MixLane, MixerBus, PluginChain, PluginSlot, PluginStage,
    ReturnLane, ReturnLaneConfig, RoutingSlot, SendDestination, StripBinding, ALL_SEND_LANES,
    MAX_PLUGIN_STAGES, MAX_SEND_COUNT,
};

/// MixLink session. Field names are camelCase on the wire (`CodingKeys`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
    /// Live strip → input map while a project is open. Durable assignments
    /// live on `project.json`; this vec is the working copy.
    pub strips: Vec<StripBinding>,
    pub main_output: i32,
    pub aux_a: SendDestination,
    pub aux_b: SendDestination,
    pub mix_bus1: i32,
    pub mix_bus2: i32,
    pub osc_host: String,
    pub osc_send_port: u16,
    pub osc_listen_port: u16,
    pub midi_device_contains: String,
    #[serde(default = "default_audio_device")]
    pub audio_device_contains: String,
    #[serde(default)]
    pub audio_buffer_frames: Option<i32>,
    #[serde(default)]
    pub gear_names: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hardware_effects: Vec<HardwareEffect>,
    #[serde(default)]
    pub hardware_presets: Vec<HardwarePreset>,
    #[serde(default)]
    pub hardware_chains: Vec<HardwareChain>,
    #[serde(default)]
    pub plugin_chains: Vec<PluginChain>,
    /// Live send/bus routing while a project is open. Durable assignments
    /// live on `project.json`; this map is the working copy.
    #[serde(default)]
    pub return_chains: HashMap<String, ChainRef>,
    #[serde(default = "ReturnLaneConfig::defaults")]
    pub returns: Vec<ReturnLaneConfig>,
    #[serde(default = "default_effect_return_count")]
    pub effect_return_count: i32,
    #[serde(default)]
    pub pan_knobs_control_send_c: bool,
    #[serde(default = "default_true")]
    pub sends_post_fader: bool,
    /// Cream caps, milled fader slots, and a recessed channel-strip well.
    #[serde(default)]
    pub hardware_strips: bool,
    #[serde(default, with = "opt_base64")]
    pub projects_root_bookmark: Option<Vec<u8>>,
    #[serde(default)]
    pub current_project_relative: Option<String>,
}

fn default_audio_device() -> String {
    "Fireface".into()
}

fn default_effect_return_count() -> i32 {
    2
}

fn default_true() -> bool {
    true
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionConfig {
    pub fn new() -> Self {
        let (hardware_presets, hardware_chains, return_chains) = Self::default_hardware_catalog();
        Self {
            strips: (0..8).map(|i| StripBinding::new(i, MixerBus::Input, i * 2, true)).collect(),
            main_output: 0,
            aux_a: SendDestination::Output(14),
            aux_b: SendDestination::Output(16),
            mix_bus1: 12,
            mix_bus2: 10,
            osc_host: "127.0.0.1".into(),
            osc_send_port: 7001,
            osc_listen_port: 9001,
            midi_device_contains: "Launch Control XL".into(),
            audio_device_contains: "Fireface".into(),
            audio_buffer_frames: None,
            gear_names: HashMap::new(),
            plugins: Vec::new(),
            hardware_effects: Vec::new(),
            hardware_presets,
            hardware_chains,
            plugin_chains: Self::default_plugin_chains(),
            return_chains,
            returns: ReturnLaneConfig::defaults(),
            effect_return_count: 2,
            pan_knobs_control_send_c: false,
            sends_post_fader: true,
            hardware_strips: false,
            projects_root_bookmark: None,
            current_project_relative: None,
        }
    }

    fn default_hardware_catalog() -> (Vec<HardwarePreset>, Vec<HardwareChain>, HashMap<String, ChainRef>)
    {
        let pairs = [(14, 16), (16, 18), (12, 20), (10, 22)];
        let mut presets = Vec::new();
        let mut chains = Vec::new();
        let mut return_chains = HashMap::new();
        for (i, (output, input)) in pairs.into_iter().enumerate() {
            let preset = HardwarePreset::new(format!("Device {}", i + 1), output, input);
            let chain = HardwareChain::new(preset.title(), vec![preset.id]);
            return_chains.insert(i.to_string(), ChainRef::hardware(chain.id));
            presets.push(preset);
            chains.push(chain);
        }
        (presets, chains, return_chains)
    }

    pub fn default_plugin_chains() -> Vec<PluginChain> {
        vec![
            PluginChain::new("FX A", 2),
            PluginChain::new("FX B", 4),
        ]
    }

    pub fn default_plugins() -> Vec<PluginSlot> {
        vec![
            PluginSlot {
                id: 0,
                name: "FX A".into(),
                bundle_path: None,
                class_uid: None,
                send_output: 14,
                input_channel: 14,
                return_channel: 2,
                return_dest: 0,
                bypassed: false,
                return_fader: 0.75,
                return_pan: 0.5,
            },
            PluginSlot {
                id: 1,
                name: "FX B".into(),
                bundle_path: None,
                class_uid: None,
                send_output: 16,
                input_channel: 16,
                return_channel: 4,
                return_dest: 0,
                bypassed: false,
                return_fader: 0.75,
                return_pan: 0.5,
            },
        ]
    }

    pub fn plugin(&self, id: i32) -> Option<&PluginSlot> {
        self.plugins.iter().find(|p| p.id == id)
    }

    pub fn hardware_preset(&self, id: uuid::Uuid) -> Option<&HardwarePreset> {
        self.hardware_presets.iter().find(|h| h.id == id)
    }

    pub fn hardware_preset_mut(&mut self, id: uuid::Uuid) -> Option<&mut HardwarePreset> {
        self.hardware_presets.iter_mut().find(|h| h.id == id)
    }

    pub fn hardware_chain(&self, id: uuid::Uuid) -> Option<&HardwareChain> {
        self.hardware_chains.iter().find(|c| c.id == id)
    }

    pub fn hardware_chain_mut(&mut self, id: uuid::Uuid) -> Option<&mut HardwareChain> {
        self.hardware_chains.iter_mut().find(|c| c.id == id)
    }

    pub fn plugin_chain(&self, id: uuid::Uuid) -> Option<&PluginChain> {
        self.plugin_chains.iter().find(|c| c.id == id)
    }

    pub fn plugin_chain_mut(&mut self, id: uuid::Uuid) -> Option<&mut PluginChain> {
        self.plugin_chains.iter_mut().find(|c| c.id == id)
    }

    pub fn plugin_stage(&self, id: uuid::Uuid) -> Option<&PluginStage> {
        self.plugin_chains.iter().flat_map(|c| c.stages.iter()).find(|s| s.id == id)
    }

    pub fn plugin_stage_mut(&mut self, id: uuid::Uuid) -> Option<&mut PluginStage> {
        self.plugin_chains.iter_mut().flat_map(|c| c.stages.iter_mut()).find(|s| s.id == id)
    }

    pub fn return_lane(&self, id: i32) -> Option<&ReturnLaneConfig> {
        self.returns.iter().find(|r| r.id == id)
    }

    pub fn visible_send_lanes(&self) -> Vec<ReturnLane> {
        let count = self.effect_return_count.clamp(2, MAX_SEND_COUNT) as usize;
        ALL_SEND_LANES.iter().copied().take(count).collect()
    }

    pub fn ensure_return_lane(&mut self, lane: ReturnLane) {
        let id = lane as i32;
        if self.returns.iter().any(|r| r.id == id) {
            return;
        }
        self.returns.push(ReturnLaneConfig::new(id, 0, None, 0.0, 0.5, ""));
        self.returns.sort_by_key(|r| r.id);
    }

    pub fn chain_ref(&self, lane: ReturnLane) -> Option<ChainRef> {
        self.return_chains.get(&(lane as i32).to_string()).copied()
    }

    pub fn chain_title(&self, ref_: ChainRef) -> String {
        match ref_.kind {
            crate::types::ChainKind::Hardware => self
                .hardware_chain(ref_.id)
                .map(|c| c.title())
                .unwrap_or_else(|| "Missing chain".into()),
            crate::types::ChainKind::Plugin => self
                .plugin_chain(ref_.id)
                .map(|c| c.title())
                .unwrap_or_else(|| "Missing chain".into()),
        }
    }

    pub fn lane_using_chain(&self, id: uuid::Uuid) -> Option<ReturnLane> {
        self.return_chains.iter().find_map(|(key, r)| {
            if r.id == id {
                key.parse().ok().and_then(ReturnLane::from_i32)
            } else {
                None
            }
        })
    }

    pub fn assigned_chain_ids(&self) -> std::collections::HashSet<uuid::Uuid> {
        self.return_chains.values().map(|r| r.id).collect()
    }

    /// Stereo playback pairs claimed by Main or a plugin chain.
    pub fn reserved_playback_pairs(&self) -> std::collections::HashSet<i32> {
        let mut used = std::collections::HashSet::new();
        used.insert(self.mix_playback_channel(MixLane::Main));
        for chain in &self.plugin_chains {
            used.insert(chain.return_channel);
        }
        used
    }

    pub fn next_free_playback_pair(&self) -> i32 {
        next_free_pair(&self.reserved_playback_pairs(), 0)
    }

    pub fn playback_pair_label(pair: i32) -> String {
        format!("{}/{}", pair + 1, pair + 2)
    }

    pub fn playback_occupants(&self, pair: i32) -> Vec<String> {
        let mut names = Vec::new();
        if pair == self.mix_playback_channel(MixLane::Main) {
            names.push("Main".into());
        }
        for chain in &self.plugin_chains {
            if chain.return_channel != pair {
                continue;
            }
            let title = chain.title();
            if let Some(lane) = self.lane_using_chain(chain.id) {
                names.push(format!("{} · {}", lane.title(), title));
            } else {
                names.push(title);
            }
        }
        names
    }

    pub fn playback_menu_label(&self, pair: i32) -> String {
        let label = Self::playback_pair_label(pair);
        let users = self.playback_occupants(pair);
        if users.is_empty() {
            label
        } else {
            format!("{label} — {}", users.join(", "))
        }
    }

    pub fn playback_pair_selectable(&self, pair: i32, chain: uuid::Uuid) -> bool {
        if self.plugin_chain(chain).is_some_and(|c| c.return_channel == pair) {
            return true;
        }
        if pair == self.mix_playback_channel(MixLane::Main) {
            return false;
        }
        !self.plugin_chains.iter().any(|c| c.id != chain && c.return_channel == pair)
    }

    /// Mix source for a return strip: plugin wet playback, or the last hardware input pair.
    pub fn return_source_id(&self, lane: ReturnLane) -> Option<ChannelID> {
        match self.chain_ref(lane)? {
            ChainRef { kind: crate::types::ChainKind::Plugin, id } => {
                let chain = self.plugin_chain(id)?;
                if !chain.is_loaded() {
                    return None;
                }
                Some(ChannelID::new(MixerBus::Playback, chain.return_channel))
            }
            ChainRef { kind: crate::types::ChainKind::Hardware, id } => {
                let chain = self.hardware_chain(id)?;
                let last = chain.stages.last().copied()?;
                let hw = self.hardware_preset(last)?;
                Some(ChannelID::new(MixerBus::Input, hw.input))
            }
        }
    }

    pub fn send_destination(&self, lane: ReturnLane) -> Option<SendDestination> {
        match self.chain_ref(lane)? {
            ChainRef { kind: crate::types::ChainKind::Hardware, id } => {
                let chain = self.hardware_chain(id)?;
                let first = chain.stages.first().copied()?;
                let hw = self.hardware_preset(first)?;
                Some(SendDestination::Output(hw.output))
            }
            ChainRef { kind: crate::types::ChainKind::Plugin, id } => {
                Some(SendDestination::Plugin(id))
            }
        }
    }

    pub fn hardware_output(&self, lane: ReturnLane) -> Option<i32> {
        match self.send_destination(lane)? {
            SendDestination::Output(index) => Some(index),
            SendDestination::Plugin(_) => None,
        }
    }

    pub fn mix_playback_channel(&self, lane: MixLane) -> i32 {
        match lane {
            MixLane::Strip(i) => {
                if let Some(strip) = self.strips.get(i as usize) {
                    strip.index
                } else {
                    i * 2
                }
            }
            MixLane::ReturnLane(ret) => {
                if let Some(ChainRef { kind: crate::types::ChainKind::Plugin, id }) =
                    self.chain_ref(ret)
                {
                    return self.plugin_chain(id).map(|p| p.return_channel).unwrap_or(ret as i32 * 2);
                }
                if ret == ReturnLane::Bus1 {
                    return self.mix_bus1;
                }
                if ret == ReturnLane::Bus2 {
                    return self.mix_bus2;
                }
                16 + ret as i32 * 2
            }
            MixLane::Main => 0,
        }
    }

    /// TotalMix output for a fader assign. Plugin bus targets are not a mix dest.
    pub fn listen_output(&self, assign: MixAssign) -> Option<i32> {
        match assign {
            MixAssign::Main => Some(self.main_output),
            MixAssign::Bus1 => self.hardware_output(ReturnLane::Bus1),
            MixAssign::Bus2 => self.hardware_output(ReturnLane::Bus2),
        }
    }

    pub fn output_index(&self, dest: SendDestination) -> i32 {
        match dest {
            SendDestination::Output(index) => index,
            SendDestination::Plugin(_) => self.main_output,
        }
    }

    pub fn apply_destination(&mut self, dest: SendDestination, lane: ReturnLane) {
        match dest {
            SendDestination::Plugin(id) => self.set_return_chain(lane, Some(ChainRef::plugin(id))),
            SendDestination::Output(index) => {
                if let Some(ChainRef { kind: crate::types::ChainKind::Hardware, id }) =
                    self.chain_ref(lane)
                {
                    if let Some(chain) = self.hardware_chain(id).cloned() {
                        if let Some(first) = chain.stages.first().copied() {
                            if let Some(preset) = self.hardware_preset_mut(first) {
                                preset.output = index;
                            }
                        }
                    }
                } else {
                    let input = self.return_lane(lane as i32).map(|r| r.input).unwrap_or(0);
                    let hid = self.insert_hardware_preset(index, input, "");
                    self.set_return_chain(lane, Some(ChainRef::hardware(hid)));
                }
            }
        }
        self.sync_legacy_dests();
    }

    pub fn set_return_chain(&mut self, lane: ReturnLane, ref_: Option<ChainRef>) {
        let key = (lane as i32).to_string();
        if let Some(next) = ref_ {
            self.return_chains.retain(|k, r| k == &key || r.id != next.id);
            self.return_chains.insert(key, next);
        } else {
            self.return_chains.remove(&key);
        }
        if let Some(row) = self.returns.iter_mut().find(|r| r.id == lane as i32) {
            row.effect = None;
        }
        self.sync_legacy_dests();
    }

    /// Replace the live send/bus map with the project's stored assignments.
    /// An empty project map is left alone so a new or older sidecar can be seeded.
    pub fn apply_project_return_chains(&mut self, project: &HashMap<String, ChainRef>) -> bool {
        if project.is_empty() || self.return_chains == *project {
            return false;
        }
        self.return_chains = project.clone();
        self.sync_legacy_dests();
        true
    }

    /// Replace the live strip map with the project's stored assignments.
    /// An empty project list is left alone so a new or older sidecar can be seeded.
    pub fn apply_project_strips(&mut self, project: &[StripBinding]) -> bool {
        if project.is_empty() || self.strips == project {
            return false;
        }
        let n = self.strips.len();
        for (i, binding) in project.iter().take(n).enumerate() {
            self.strips[i] = binding.clone();
        }
        true
    }

    pub fn set_return_effect(&mut self, lane: ReturnLane, ref_: Option<EffectRef>) {
        let mapped = ref_.and_then(|r| self.legacy_effect_to_chain(r));
        self.set_return_chain(lane, mapped);
    }

    fn legacy_effect_to_chain(&self, ref_: EffectRef) -> Option<ChainRef> {
        match ref_ {
            EffectRef::Hardware(id) => {
                let old = self.hardware_effects.iter().find(|h| h.id == id)?;
                self.hardware_presets
                    .iter()
                    .find(|p| p.output == old.output && p.input == old.input)
                    .and_then(|p| {
                        self.hardware_chains
                            .iter()
                            .find(|c| c.stages.first() == Some(&p.id))
                            .map(|c| ChainRef::hardware(c.id))
                    })
            }
            EffectRef::Plugin(id) => self
                .plugin_chains
                .get(id as usize)
                .or_else(|| self.plugin_chains.first())
                .map(|c| ChainRef::plugin(c.id)),
        }
    }

    pub fn insert_hardware_preset(&mut self, output: i32, input: i32, name: &str) -> uuid::Uuid {
        let title = if name.is_empty() {
            format!("Device {}", self.hardware_presets.len() + 1)
        } else {
            name.to_string()
        };
        let preset = HardwarePreset::new(title.clone(), output, input);
        let chain = HardwareChain::new(title, vec![preset.id]);
        let id = chain.id;
        self.hardware_presets.push(preset);
        self.hardware_chains.push(chain);
        id
    }

    pub fn add_hardware_preset(&mut self, output: i32, input: i32, name: &str) -> uuid::Uuid {
        let title = if name.is_empty() {
            format!("Device {}", self.hardware_presets.len() + 1)
        } else {
            name.to_string()
        };
        let preset = HardwarePreset::new(title.clone(), output, input);
        let id = preset.id;
        self.hardware_presets.push(preset);
        self.add_hardware_chain(&title, vec![id]);
        id
    }

    pub fn add_hardware_chain(&mut self, name: &str, stages: Vec<uuid::Uuid>) -> uuid::Uuid {
        let title = if name.is_empty() {
            format!("Hardware chain {}", self.hardware_chains.len() + 1)
        } else {
            name.to_string()
        };
        let chain = HardwareChain::new(title, stages);
        let id = chain.id;
        self.hardware_chains.push(chain);
        id
    }

    pub fn add_plugin_chain(&mut self, name: &str, return_channel: i32) -> uuid::Uuid {
        let title = if name.is_empty() {
            format!("Plugin chain {}", self.plugin_chains.len() + 1)
        } else {
            name.to_string()
        };
        let mut chain = PluginChain::new(title, return_channel);
        chain.stages.push(PluginStage::new("Plugin"));
        let id = chain.id;
        self.plugin_chains.push(chain);
        id
    }

    pub fn duplicate_hardware_preset(&mut self, id: uuid::Uuid) -> Option<uuid::Uuid> {
        let src = self.hardware_preset(id)?.clone();
        let names: Vec<String> = self.hardware_presets.iter().map(|p| p.name.clone()).collect();
        let mut copy = src;
        copy.id = uuid::Uuid::new_v4();
        copy.name = unique_copy_name(&names, &copy.name);
        let new_id = copy.id;
        let chain_name = copy.name.clone();
        self.hardware_presets.push(copy);
        self.add_hardware_chain(&chain_name, vec![new_id]);
        Some(new_id)
    }

    pub fn duplicate_hardware_chain(&mut self, id: uuid::Uuid) -> Option<uuid::Uuid> {
        let src = self.hardware_chain(id)?.clone();
        let names: Vec<String> = self.hardware_chains.iter().map(|c| c.name.clone()).collect();
        let mut copy = src;
        copy.id = uuid::Uuid::new_v4();
        copy.name = unique_copy_name(&names, &copy.name);
        let new_id = copy.id;
        self.hardware_chains.push(copy);
        Some(new_id)
    }

    pub fn duplicate_plugin_chain(&mut self, id: uuid::Uuid) -> Option<uuid::Uuid> {
        let src = self.plugin_chain(id)?.clone();
        let names: Vec<String> = self.plugin_chains.iter().map(|c| c.name.clone()).collect();
        let mut copy = src;
        copy.id = uuid::Uuid::new_v4();
        copy.name = unique_copy_name(&names, &copy.name);
        for stage in &mut copy.stages {
            stage.id = uuid::Uuid::new_v4();
        }
        copy.return_channel = self.next_free_playback_pair();
        let new_id = copy.id;
        self.plugin_chains.push(copy);
        Some(new_id)
    }

    pub fn rename_hardware_preset(&mut self, id: uuid::Uuid, name: &str) {
        let old = self.hardware_preset(id).map(|p| p.name.clone());
        let next = name.to_string();
        if let Some(p) = self.hardware_preset_mut(id) {
            p.name = next.clone();
        }
        if let Some(old) = old {
            for chain in &mut self.hardware_chains {
                if chain.name == old && chain.stages == [id] {
                    chain.name = next.clone();
                }
            }
        }
    }

    pub fn rename_hardware_chain(&mut self, id: uuid::Uuid, name: &str) {
        if let Some(c) = self.hardware_chain_mut(id) {
            c.name = name.to_string();
        }
    }

    pub fn rename_plugin_chain(&mut self, id: uuid::Uuid, name: &str) {
        if let Some(c) = self.plugin_chain_mut(id) {
            c.name = name.to_string();
        }
    }

    pub fn set_hardware_preset_io(&mut self, id: uuid::Uuid, output: Option<i32>, input: Option<i32>) {
        if let Some(p) = self.hardware_preset_mut(id) {
            if let Some(o) = output {
                p.output = o;
            }
            if let Some(i) = input {
                p.input = i;
            }
        }
        self.sync_legacy_dests();
    }

    pub fn remove_hardware_preset(&mut self, id: uuid::Uuid) {
        for chain in &mut self.hardware_chains {
            chain.stages.retain(|s| *s != id);
        }
        self.hardware_presets.retain(|p| p.id != id);
        self.sync_legacy_dests();
    }

    pub fn remove_hardware_chain(&mut self, id: uuid::Uuid) {
        self.return_chains.retain(|_, r| r.id != id);
        self.hardware_chains.retain(|c| c.id != id);
        self.sync_legacy_dests();
    }

    pub fn remove_plugin_chain(&mut self, id: uuid::Uuid) {
        self.return_chains.retain(|_, r| r.id != id);
        self.plugin_chains.retain(|c| c.id != id);
        self.sync_legacy_dests();
    }

    pub fn add_plugin_stage(&mut self, chain: uuid::Uuid) -> Option<uuid::Uuid> {
        let c = self.plugin_chain_mut(chain)?;
        if c.stages.len() >= MAX_PLUGIN_STAGES {
            return None;
        }
        let stage = PluginStage::new("Plugin");
        let id = stage.id;
        c.stages.push(stage);
        Some(id)
    }

    pub fn remove_plugin_stage(&mut self, stage: uuid::Uuid) {
        for chain in &mut self.plugin_chains {
            chain.stages.retain(|s| s.id != stage);
        }
    }

    pub fn add_hardware_chain_stage(&mut self, chain: uuid::Uuid, preset: uuid::Uuid) {
        if let Some(c) = self.hardware_chain_mut(chain) {
            c.stages.push(preset);
        }
    }

    pub fn remove_hardware_chain_stage_at(&mut self, chain: uuid::Uuid, index: usize) {
        if let Some(c) = self.hardware_chain_mut(chain) {
            if index < c.stages.len() {
                c.stages.remove(index);
            }
        }
    }

    pub fn move_hardware_chain_stage(&mut self, chain: uuid::Uuid, index: usize, delta: i32) {
        let Some(c) = self.hardware_chain_mut(chain) else { return };
        let next = index as i32 + delta;
        if next < 0 || next as usize >= c.stages.len() {
            return;
        }
        c.stages.swap(index, next as usize);
    }

    pub fn move_plugin_stage(&mut self, chain: uuid::Uuid, index: usize, delta: i32) {
        let Some(c) = self.plugin_chain_mut(chain) else { return };
        let next = index as i32 + delta;
        if next < 0 || next as usize >= c.stages.len() {
            return;
        }
        c.stages.swap(index, next as usize);
    }

    pub fn sync_legacy_dests(&mut self) {
        if let Some(dest) = self.send_destination(ReturnLane::SendA) {
            self.aux_a = dest;
        }
        if let Some(dest) = self.send_destination(ReturnLane::SendB) {
            self.aux_b = dest;
        }
        if let Some(out) = self.hardware_output(ReturnLane::Bus1) {
            self.mix_bus1 = out;
        }
        if let Some(out) = self.hardware_output(ReturnLane::Bus2) {
            self.mix_bus2 = out;
        }
    }

    pub fn migrate_effects_if_needed(&mut self) {
        self.migrate_catalog_once();
        self.migrate_return_assignments();
        self.sync_legacy_dests();
    }

    fn catalog_present(&self) -> bool {
        !self.hardware_presets.is_empty()
            || !self.hardware_chains.is_empty()
            || !self.plugin_chains.is_empty()
    }

    fn migrate_catalog_once(&mut self) {
        if self.catalog_present() {
            return;
        }
        if self.hardware_effects.is_empty() && self.plugins.is_empty() {
            let (presets, chains, returns) = Self::default_hardware_catalog();
            self.hardware_presets = presets;
            self.hardware_chains = chains;
            if self.return_chains.is_empty() {
                self.return_chains = returns;
            }
            if self.plugin_chains.is_empty() {
                self.plugin_chains = Self::default_plugin_chains();
            }
            return;
        }
        let mut hw_map: HashMap<i32, uuid::Uuid> = HashMap::new();
        for old in self.hardware_effects.clone() {
            let name = if old.name.is_empty() {
                format!("Hardware {}", old.id + 1)
            } else {
                old.name.clone()
            };
            let preset = HardwarePreset { id: uuid::Uuid::new_v4(), name: name.clone(), output: old.output, input: old.input };
            let chain = HardwareChain::new(name, vec![preset.id]);
            hw_map.insert(old.id, chain.id);
            self.hardware_presets.push(preset);
            self.hardware_chains.push(chain);
        }
        let mut pl_map: HashMap<i32, uuid::Uuid> = HashMap::new();
        for old in self.plugins.clone() {
            let mut chain = PluginChain::new(old.title(), old.return_channel);
            if old.is_loaded() || !old.name.is_empty() {
                chain.stages.push(PluginStage {
                    id: uuid::Uuid::new_v4(),
                    name: old.name.clone(),
                    bundle_path: old.bundle_path.clone(),
                    class_uid: old.class_uid.clone(),
                    bypassed: old.bypassed,
                });
            }
            pl_map.insert(old.id, chain.id);
            self.plugin_chains.push(chain);
        }
        if self.return_chains.is_empty() {
            for row in &self.returns {
                let Some(effect) = row.effect else { continue };
                let mapped = match effect {
                    EffectRef::Hardware(id) => hw_map.get(&id).copied().map(ChainRef::hardware),
                    EffectRef::Plugin(id) => pl_map.get(&id).copied().map(ChainRef::plugin),
                };
                if let Some(r) = mapped {
                    self.return_chains.insert(row.id.to_string(), r);
                }
            }
        }
        self.hardware_effects.clear();
        self.plugins.clear();
    }

    fn migrate_return_assignments(&mut self) {
        if !self.return_chains.is_empty() {
            for row in &mut self.returns {
                row.effect = None;
            }
            return;
        }
        for row in self.returns.clone() {
            let Some(effect) = row.effect else { continue };
            if let Some(mapped) = self.legacy_effect_to_chain(effect) {
                self.return_chains.insert(row.id.to_string(), mapped);
            }
        }
        for row in &mut self.returns {
            row.effect = None;
        }
    }

    pub fn gear_key(id: ChannelID) -> String {
        format!("{}.{}", id.bus.as_str(), id.index)
    }

    pub fn gear_name(&self, id: ChannelID) -> String {
        self.gear_names.get(&Self::gear_key(id)).cloned().unwrap_or_default()
    }

    pub fn is_strip_enabled(&self, strip: usize) -> bool {
        self.strips.get(strip).map(|s| s.enabled).unwrap_or(true)
    }

    pub fn is_return_enabled(&self, lane: ReturnLane) -> bool {
        self.return_lane(lane as i32).map(|r| r.enabled).unwrap_or(true)
    }

    pub fn dest_output(&self, assign: MixAssign) -> i32 {
        self.listen_output(assign).unwrap_or(self.main_output)
    }

    pub fn slot_destination(&self, slot: RoutingSlot) -> SendDestination {
        match slot {
            RoutingSlot::Mix => SendDestination::Output(self.main_output),
            RoutingSlot::SendA => self.send_destination(ReturnLane::SendA).unwrap_or(self.aux_a),
            RoutingSlot::SendB => self.send_destination(ReturnLane::SendB).unwrap_or(self.aux_b),
            RoutingSlot::SendC | RoutingSlot::SendD | RoutingSlot::SendE | RoutingSlot::SendF => {
                slot.lane()
                    .and_then(|lane| self.send_destination(lane))
                    .unwrap_or(SendDestination::Output(0))
            }
            RoutingSlot::Bus1 => self
                .send_destination(ReturnLane::Bus1)
                .unwrap_or(SendDestination::Output(self.mix_bus1)),
            RoutingSlot::Bus2 => self
                .send_destination(ReturnLane::Bus2)
                .unwrap_or(SendDestination::Output(self.mix_bus2)),
        }
    }

    pub fn set_slot_destination(&mut self, slot: RoutingSlot, dest: SendDestination) {
        match slot {
            RoutingSlot::Mix => self.main_output = self.output_index(dest),
            _ => {
                if let Some(lane) = slot.lane() {
                    self.apply_destination(dest, lane);
                }
            }
        }
    }

    pub fn next_free_plugin_id(&self) -> Option<i32> {
        Some(self.plugin_chains.len() as i32)
    }

    pub fn remove_hardware_effect(&mut self, id: i32) {
        if let Some(old) = self.hardware_effects.iter().find(|h| h.id == id).cloned() {
            if let Some(preset) = self
                .hardware_presets
                .iter()
                .find(|p| p.output == old.output && p.input == old.input)
                .map(|p| p.id)
            {
                self.remove_hardware_preset(preset);
            }
        }
        self.hardware_effects.retain(|h| h.id != id);
        self.sync_legacy_dests();
    }

    pub fn remove_plugin_slot(&mut self, id: i32) {
        if let Some(chain) = self.plugin_chains.get(id as usize).map(|c| c.id) {
            self.remove_plugin_chain(chain);
        }
        self.plugins.retain(|p| p.id != id);
        self.sync_legacy_dests();
    }

    pub fn set_return_enabled(&mut self, lane: ReturnLane, on: bool) {
        if let Some(row) = self.returns.iter_mut().find(|r| r.id == lane as i32) {
            row.enabled = on;
        }
    }

    pub fn storage_directory() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        PathBuf::from(home).join("Library/Application Support/MixLink")
    }

    pub fn storage_path() -> PathBuf {
        Self::storage_directory().join("session.mixlinkmap")
    }

    pub fn load() -> Self {
        let path = Self::storage_path();
        let Ok(data) = std::fs::read(&path) else {
            return Self::new();
        };
        let Ok(mut config) = serde_json::from_slice::<Self>(&data) else {
            return Self::new();
        };
        config.normalize_after_load();
        config.save();
        config
    }

    pub fn save(&self) {
        let path = Self::storage_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(data) = serde_json::to_vec(self) {
            let _ = std::fs::write(path, data);
        }
    }

    pub fn normalize_after_load(&mut self) {
        if self.returns.len() < ReturnLaneConfig::defaults().len() {
            let mut lanes = self.returns.clone();
            for fallback in ReturnLaneConfig::defaults() {
                if lanes.iter().any(|r| r.id == fallback.id) {
                    continue;
                }
                lanes.push(fallback);
            }
            lanes.sort_by_key(|r| r.id);
            self.returns = lanes;
        }
        for lane in self.visible_send_lanes() {
            self.ensure_return_lane(lane);
        }
        self.migrate_effects_if_needed();
        for strip in &mut self.strips {
            if strip.bus != MixerBus::Input {
                strip.bus = MixerBus::Input;
            }
        }
        self.effect_return_count = self.effect_return_count.clamp(2, MAX_SEND_COUNT);
    }
}

fn next_free_pair(used: &std::collections::HashSet<i32>, start: i32) -> i32 {
    let mut i = start;
    if i % 2 != 0 {
        i += 1;
    }
    while used.contains(&i) {
        i += 2;
    }
    i
}

/// Swift `JSONEncoder` stores `Data` as a standard Base64 string.
mod opt_base64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        bytes: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(b) => serializer.serialize_some(&encode(b)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) => decode(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }

    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    fn encode(data: &[u8]) -> String {
        let mut out = String::new();
        for chunk in data.chunks(3) {
            let a = chunk[0] as u32;
            let b = chunk.get(1).copied().unwrap_or(0) as u32;
            let c = chunk.get(2).copied().unwrap_or(0) as u32;
            let n = (a << 16) | (b << 8) | c;
            out.push(ALPHABET[(n >> 18) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(ALPHABET[(n & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    fn decode(s: &str) -> Result<Vec<u8>, &'static str> {
        let filtered: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
        if filtered.len() % 4 != 0 {
            return Err("invalid base64 length");
        }
        let mut out = Vec::new();
        for chunk in filtered.chunks(4) {
            let mut n = 0u32;
            let mut pads = 0;
            for (i, b) in chunk.iter().enumerate() {
                let v = if *b == b'=' {
                    pads += 1;
                    0
                } else {
                    ALPHABET.iter().position(|c| c == b).ok_or("invalid base64")? as u32
                };
                n |= v << (18 - i * 6);
            }
            out.push((n >> 16) as u8);
            if pads < 2 {
                out.push((n >> 8) as u8);
            }
            if pads < 1 {
                out.push(n as u8);
            }
        }
        Ok(out)
    }
}
