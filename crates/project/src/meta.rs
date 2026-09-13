//! Sidecar `project.json` metadata.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use analog::{ChainRef, HardwareChain, HardwarePreset, PluginChain, StripBinding};

use crate::document::{MixArrangement, MixGrid, MixLane};

/// One mix in the project browser list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MixListEntry {
    pub id: Uuid,
    pub name: String,
}

/// Dated-folder sidecar. Field names match MixLink `ProjectMeta` CodingKeys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    #[serde(default = "default_next_take")]
    pub next_take: i32,
    #[serde(default = "default_tempo")]
    pub tempo: f64,
    #[serde(default)]
    pub mixes: Vec<MixListEntry>,
    /// MixLink writes `activeMixID` (capital ID), not camelCase `activeMixId`.
    #[serde(default, rename = "activeMixID", alias = "activeMixId")]
    pub active_mix_id: Option<Uuid>,
    #[serde(default)]
    pub arrangement: Option<MixArrangement>,
    #[serde(default)]
    pub selected_lane: Option<MixLane>,
    #[serde(default)]
    pub take_start_frames: HashMap<String, i64>,
    /// Display titles for takes. WAV filenames stay `{N}-ch-…`.
    #[serde(default)]
    pub take_names: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub grid_enabled: bool,
    #[serde(default)]
    pub grid: MixGrid,
    #[serde(default = "default_pixels_per_bar")]
    pub pixels_per_bar: f64,
    /// Send/bus effect assignments for this project (`"0"` = Send A, …).
    #[serde(default)]
    pub return_chains: HashMap<String, ChainRef>,
    /// Mixer strip → hardware input assignments for this project.
    #[serde(default)]
    pub strips: Vec<StripBinding>,
    /// Hardware devices, hardware chains, and plugin chains for this project.
    /// Definitions travel with the folder so assignments resolve on another machine.
    #[serde(default)]
    pub hardware_presets: Vec<HardwarePreset>,
    #[serde(default)]
    pub hardware_chains: Vec<HardwareChain>,
    #[serde(default)]
    pub plugin_chains: Vec<PluginChain>,
}

fn default_next_take() -> i32 {
    1
}

fn default_tempo() -> f64 {
    120.0
}

fn default_true() -> bool {
    true
}

fn default_pixels_per_bar() -> f64 {
    48.0
}

impl Default for ProjectMeta {
    fn default() -> Self {
        Self {
            next_take: 1,
            tempo: 120.0,
            mixes: Vec::new(),
            active_mix_id: None,
            arrangement: None,
            selected_lane: None,
            take_start_frames: HashMap::new(),
            take_names: HashMap::new(),
            grid_enabled: true,
            grid: MixGrid::Bar1,
            pixels_per_bar: 48.0,
            return_chains: HashMap::new(),
            strips: Vec::new(),
            hardware_presets: Vec::new(),
            hardware_chains: Vec::new(),
            plugin_chains: Vec::new(),
        }
    }
}

impl ProjectMeta {
    pub fn catalog_is_empty(&self) -> bool {
        self.hardware_presets.is_empty()
            && self.hardware_chains.is_empty()
            && self.plugin_chains.is_empty()
    }

    pub fn normalize(&mut self) {
        self.next_take = self.next_take.max(1);
        if self.tempo < 20.0 {
            self.tempo = 120.0;
        }
        self.pixels_per_bar = self.pixels_per_bar.clamp(10.0, 16_000.0);
    }

    pub fn take_start_frame(&self, number: i32) -> i64 {
        self.take_start_frames.get(&number.to_string()).copied().unwrap_or(0).max(0)
    }

    pub fn set_take_start_frame(&mut self, number: i32, frame: i64) {
        self.take_start_frames.insert(number.to_string(), frame.max(0));
    }

    pub fn take_name(&self, number: i32) -> Option<&str> {
        self.take_names.get(&number.to_string()).map(String::as_str).filter(|s| !s.is_empty())
    }

    pub fn set_take_name(&mut self, number: i32, name: impl Into<String>) {
        let name = name.into().trim().to_string();
        if name.is_empty() {
            self.take_names.remove(&number.to_string());
        } else {
            self.take_names.insert(number.to_string(), name);
        }
    }

    pub fn take_title(number: i32, name: Option<&str>) -> String {
        match name.filter(|s| !s.is_empty()) {
            Some(name) => name.to_string(),
            None => format!("Take {number}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::MixDocument;

    #[test]
    fn mix_start_copies_take_then_stays_independent() {
        let mut meta = ProjectMeta::default();
        meta.set_take_start_frame(7, 48_000);
        let mut mix = MixDocument::empty("Mix 1", 2);
        mix.start_frame = meta.take_start_frame(7);
        assert_eq!(mix.start_frame, 48_000);
        mix.start_frame = 96_000;
        assert_eq!(meta.take_start_frame(7), 48_000);
        meta.set_take_start_frame(7, 0);
        assert_eq!(mix.start_frame, 96_000);
    }

    #[test]
    fn take_display_name_does_not_touch_empty() {
        let mut meta = ProjectMeta::default();
        assert_eq!(ProjectMeta::take_title(7, meta.take_name(7)), "Take 7");
        meta.set_take_name(7, "  Kick stem  ");
        assert_eq!(meta.take_name(7), Some("Kick stem"));
        assert_eq!(ProjectMeta::take_title(7, meta.take_name(7)), "Kick stem");
        meta.set_take_name(7, "   ");
        assert_eq!(meta.take_name(7), None);
    }
}
