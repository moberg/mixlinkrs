//! Session map: `~/Library/Application Support/MixLink/session.mixlinkmap`.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::{
    ChannelID, EffectRef, HardwareEffect, MixAssign, MixLane, MixerBus, PluginSlot, ReturnLane,
    ReturnLaneConfig, RoutingSlot, SendDestination, StripBinding, ALL_SEND_LANES, MAX_SEND_COUNT,
};

const MAX_PLUGIN_SLOTS: i32 = 8;

/// MixLink session. Field names are camelCase on the wire (`CodingKeys`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfig {
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
    #[serde(default = "SessionConfig::default_plugins")]
    pub plugins: Vec<PluginSlot>,
    #[serde(default)]
    pub hardware_effects: Vec<HardwareEffect>,
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
            plugins: Self::default_plugins(),
            hardware_effects: Self::default_hardware_effects(),
            returns: ReturnLaneConfig::defaults(),
            effect_return_count: 2,
            pan_knobs_control_send_c: false,
            sends_post_fader: true,
            hardware_strips: false,
            projects_root_bookmark: None,
            current_project_relative: None,
        }
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

    pub fn default_hardware_effects() -> Vec<HardwareEffect> {
        vec![
            HardwareEffect { id: 0, name: String::new(), output: 14, input: 16 },
            HardwareEffect { id: 1, name: String::new(), output: 16, input: 18 },
            HardwareEffect { id: 2, name: String::new(), output: 12, input: 20 },
            HardwareEffect { id: 3, name: String::new(), output: 10, input: 22 },
        ]
    }

    pub fn plugin(&self, id: i32) -> Option<&PluginSlot> {
        self.plugins.iter().find(|p| p.id == id)
    }

    pub fn hardware_effect(&self, id: i32) -> Option<&HardwareEffect> {
        self.hardware_effects.iter().find(|h| h.id == id)
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

    pub fn effect_ref(&self, lane: ReturnLane) -> Option<EffectRef> {
        self.return_lane(lane as i32).and_then(|r| r.effect)
    }

    /// Mix source for a return strip: plugin wet playback, or the hardware input pair.
    pub fn return_source_id(&self, lane: ReturnLane) -> Option<ChannelID> {
        match self.effect_ref(lane)? {
            EffectRef::Plugin(id) => {
                let plugin = self.plugin(id)?;
                if !plugin.is_loaded() {
                    return None;
                }
                Some(ChannelID::new(MixerBus::Playback, plugin.return_channel))
            }
            EffectRef::Hardware(id) => {
                let hw = self.hardware_effect(id)?;
                Some(ChannelID::new(MixerBus::Input, hw.input))
            }
        }
    }

    pub fn send_destination(&self, lane: ReturnLane) -> Option<SendDestination> {
        match self.effect_ref(lane)? {
            EffectRef::Hardware(id) => {
                let hw = self.hardware_effect(id)?;
                Some(SendDestination::Output(hw.output))
            }
            EffectRef::Plugin(id) => Some(SendDestination::Plugin(id)),
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
                if let Some(ref_) = self.effect_ref(ret) {
                    match ref_ {
                        EffectRef::Plugin(id) => {
                            return self
                                .plugin(id)
                                .map(|p| p.return_channel)
                                .unwrap_or(ret as i32 * 2);
                        }
                        EffectRef::Hardware(_) => {}
                    }
                }
                if ret == ReturnLane::Bus1 {
                    return self.mix_bus1;
                }
                if ret == ReturnLane::Bus2 {
                    return self.mix_bus2;
                }
                self.plugin(ret as i32).map(|p| p.return_channel).unwrap_or(16 + ret as i32 * 2)
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
            SendDestination::Plugin(id) => {
                self.plugin(id).map(|p| p.send_output).unwrap_or(self.main_output)
            }
        }
    }

    pub fn apply_destination(&mut self, dest: SendDestination, lane: ReturnLane) {
        match dest {
            SendDestination::Plugin(id) => {
                self.set_return_effect(lane, Some(EffectRef::Plugin(id)))
            }
            SendDestination::Output(index) => {
                if let Some(EffectRef::Hardware(hid)) = self.effect_ref(lane) {
                    if let Some(hw) = self.hardware_effects.iter_mut().find(|h| h.id == hid) {
                        hw.output = index;
                    }
                } else {
                    let input = self.return_lane(lane as i32).map(|r| r.input).unwrap_or(0);
                    let hid = self.insert_hardware_effect(index, input, "");
                    self.set_return_effect(lane, Some(EffectRef::Hardware(hid)));
                }
            }
        }
        self.sync_legacy_dests();
    }

    pub fn set_return_effect(&mut self, lane: ReturnLane, ref_: Option<EffectRef>) {
        if let Some(row) = self.returns.iter_mut().find(|r| r.id == lane as i32) {
            row.effect = ref_;
        }
        self.sync_legacy_dests();
    }

    pub fn insert_hardware_effect(&mut self, output: i32, input: i32, name: &str) -> i32 {
        let id = self.hardware_effects.iter().map(|h| h.id).max().unwrap_or(-1) + 1;
        self.hardware_effects.push(HardwareEffect { id, name: name.to_string(), output, input });
        id
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
        let needs = self.hardware_effects.is_empty()
            || self.returns.iter().any(|r| r.effect.is_none() && r.id < 4);
        if !needs {
            self.sync_legacy_dests();
            return;
        }
        if self.hardware_effects.is_empty() {
            let aux_a = self.aux_a;
            let aux_b = self.aux_b;
            let mix_bus1 = self.mix_bus1;
            let mix_bus2 = self.mix_bus2;
            self.assign_migrated(ReturnLane::SendA, aux_a, 16);
            self.assign_migrated(ReturnLane::SendB, aux_b, 18);
            self.assign_migrated(ReturnLane::Bus1, SendDestination::Output(mix_bus1), 20);
            self.assign_migrated(ReturnLane::Bus2, SendDestination::Output(mix_bus2), 22);
        } else {
            for i in 0..self.returns.len() {
                if self.returns[i].effect.is_some() {
                    continue;
                }
                let input = self.returns[i].input;
                if let Some(hw) = self.hardware_effects.iter().find(|h| h.input == input) {
                    self.returns[i].effect = Some(EffectRef::Hardware(hw.id));
                }
            }
        }
        self.sync_legacy_dests();
    }

    fn assign_migrated(&mut self, lane: ReturnLane, dest: SendDestination, fallback_input: i32) {
        let Some(i) = self.returns.iter().position(|r| r.id == lane as i32) else {
            return;
        };
        if self.returns[i].effect.is_some() {
            return;
        }
        match dest {
            SendDestination::Plugin(id) => self.returns[i].effect = Some(EffectRef::Plugin(id)),
            SendDestination::Output(out) => {
                let input =
                    if self.returns[i].input != 0 { self.returns[i].input } else { fallback_input };
                let hid = self.insert_hardware_effect(out, input, "");
                if let Some(row) = self.returns.iter_mut().find(|r| r.id == lane as i32) {
                    row.effect = Some(EffectRef::Hardware(hid));
                }
            }
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
        let used: std::collections::HashSet<i32> = self.plugins.iter().map(|p| p.id).collect();
        (0..MAX_PLUGIN_SLOTS).find(|id| !used.contains(id))
    }

    pub fn remove_hardware_effect(&mut self, id: i32) {
        for row in &mut self.returns {
            if matches!(row.effect, Some(EffectRef::Hardware(hid)) if hid == id) {
                row.effect = None;
            }
        }
        self.hardware_effects.retain(|h| h.id != id);
        self.sync_legacy_dests();
    }

    pub fn remove_plugin_slot(&mut self, id: i32) {
        for row in &mut self.returns {
            if matches!(row.effect, Some(EffectRef::Plugin(pid)) if pid == id) {
                row.effect = None;
            }
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
        if self.plugins.is_empty() {
            self.plugins = Self::default_plugins();
        }
        if self.returns.len() < ReturnLaneConfig::defaults().len() {
            let mut lanes = self.returns.clone();
            for fallback in ReturnLaneConfig::defaults() {
                if lanes.iter().any(|r| r.id == fallback.id) {
                    continue;
                }
                if let Some(plugin) = self.plugins.iter().find(|p| p.id == fallback.id) {
                    lanes.push(ReturnLaneConfig::new(
                        fallback.id,
                        fallback.input,
                        None,
                        plugin.return_fader,
                        plugin.return_pan,
                        plugin.name.clone(),
                    ));
                } else {
                    let mut row = fallback;
                    row.effect = None;
                    lanes.push(row);
                }
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
